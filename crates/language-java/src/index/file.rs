//! O índice em disco: registros de tamanho fixo e os textos à parte.
//!
//! É a fase 1 da `20`. Consultar tem de ser saltar e ler — nunca desserializar o
//! todo —, e por isso nada aqui é de tamanho variável a não ser as duas áreas de
//! texto, alcançadas por deslocamento.
//!
//! **A tabela de nomes é ordenada.** Ela dá busca por nome exato em tempo
//! logarítmico e faixa por prefixo, que é o que a fase 3 precisa; o formato já a
//! contém para que a fase 3 não tenha de mudar o arquivo.

use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use ide_domain::{SymbolKind, TextPosition, TextRange, TypeDescriptor};

use super::{ExternalClass, IndexedSymbol, Occurrence, WorkspaceIndex};

/// Assinatura do arquivo. Um arquivo que não comece assim não é nosso.
const MAGIC: [u8; 8] = *b"ERIDEIDX";

/// Versão do formato.
///
/// Muda **sempre** que a forma de um registro mudar, inclusive a numeração das
/// espécies de símbolo. Um arquivo de outra versão é descartado, não convertido:
/// reconstruir o índice é caro mas correto, e converter formato velho é código
/// que ninguém testa.
const VERSION: u32 = 1;

#[cfg(test)]
pub(in crate::index) const VERSION_PARA_TESTE: u32 = VERSION;

/// Ausência de tipo, para o campo que guarda o descritor.
const SEM_TIPO: u32 = u32::MAX;

/// Números das espécies, escritos à mão de propósito.
///
/// Derivar da ordem do `enum` faria reordenar uma variante corromper todo
/// arquivo já gravado, em silêncio. Acrescentar variante aqui é acrescentar um
/// número novo; mexer nos existentes é mudar a `VERSION`.
const fn numero_da_especie(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Package => 0,
        SymbolKind::Class => 1,
        SymbolKind::Record => 2,
        SymbolKind::Interface => 3,
        SymbolKind::Enum => 4,
        SymbolKind::EnumConstant => 5,
        SymbolKind::Annotation => 6,
        SymbolKind::Constructor => 7,
        SymbolKind::Method => 8,
        SymbolKind::Field => 9,
        SymbolKind::Parameter => 10,
        SymbolKind::LocalVariable => 11,
    }
}

const fn especie_do_numero(numero: u8) -> Option<SymbolKind> {
    Some(match numero {
        0 => SymbolKind::Package,
        1 => SymbolKind::Class,
        2 => SymbolKind::Record,
        3 => SymbolKind::Interface,
        4 => SymbolKind::Enum,
        5 => SymbolKind::EnumConstant,
        6 => SymbolKind::Annotation,
        7 => SymbolKind::Constructor,
        8 => SymbolKind::Method,
        9 => SymbolKind::Field,
        10 => SymbolKind::Parameter,
        11 => SymbolKind::LocalVariable,
        _ => return None,
    })
}

/// Tamanho de cada registro, em bytes. São fixos: é o que permite saltar.
mod tamanho {
    pub(super) const TEXTO: usize = 8;
    pub(super) const NOME: usize = 12;
    pub(super) const ARQUIVO: usize = 24;
    pub(super) const SIMBOLO: usize = 44;
    pub(super) const OCORRENCIA: usize = 20;
    pub(super) const DECLARACAO: usize = 8;
    pub(super) const EXTERNA: usize = 12;
    pub(super) const GENERICO: usize = 4;
}

/// Onde cada área começa e quantos registros tem.
///
/// O cabeçalho traz deslocamento **e** contagem de tudo para que a fase 2 possa
/// mapear o arquivo e ir direto à área, sem percorrer o que veio antes.
#[derive(Default)]
struct Cabecalho {
    textos: (u64, u64),
    tabela_de_textos: (u64, u32),
    nomes: (u64, u32),
    arquivos: (u64, u32),
    simbolos: (u64, u32),
    ocorrencias: (u64, u32),
    declaracoes: (u64, u32),
    externas: (u64, u32),
    genericos: (u64, u32),
}

/// Tamanho do cabeçalho: assinatura, versão e os pares acima.
const CABECALHO: usize = 8 + 4 + 4 + (8 + 8) + 8 * (8 + 4 + 4);

#[cfg(test)]
pub(in crate::index) const CABECALHO_PARA_TESTE: usize = CABECALHO;

/// Acumula os textos e devolve o número de cada um, sem repetir.
#[derive(Default)]
struct Textos {
    blob: Vec<u8>,
    tabela: Vec<(u32, u32)>,
    vistos: HashMap<String, u32>,
}

impl Textos {
    fn id(&mut self, texto: &str) -> u32 {
        if let Some(id) = self.vistos.get(texto) {
            return *id;
        }
        let inicio = u32::try_from(self.blob.len()).unwrap_or(u32::MAX);
        let tamanho = u32::try_from(texto.len()).unwrap_or(u32::MAX);
        self.blob.extend_from_slice(texto.as_bytes());
        let id = u32::try_from(self.tabela.len()).unwrap_or(u32::MAX);
        self.tabela.push((inicio, tamanho));
        self.vistos.insert(texto.to_owned(), id);
        id
    }

    fn id_do_caminho(&mut self, path: &Path) -> u32 {
        self.id(&path.to_string_lossy())
    }
}

/// Escreve o índice, inteiro, no caminho dado.
///
/// Grava num arquivo temporário e renomeia: um desligamento no meio da escrita
/// deixaria um arquivo pela metade, e ler índice truncado é pior que não ter
/// índice.
pub(in crate::index) fn write(index: &WorkspaceIndex, path: &Path) -> io::Result<()> {
    let mut textos = Textos::default();

    // Os arquivos primeiro: símbolos e ocorrências os alcançam por número, e a
    // data e o tamanho são o que a fase 4 usa para saber o que mudou.
    let mut arquivos = Vec::with_capacity(index.files.len());
    for arquivo in &index.files {
        let id = textos.id_do_caminho(arquivo);
        let (modificado, tamanho) = match fs::metadata(arquivo) {
            Ok(dados) => (
                dados
                    .modified()
                    .ok()
                    .and_then(|instante| instante.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |desde| desde.as_secs()),
                dados.len(),
            ),
            Err(_) => (0, 0),
        };
        arquivos.push((id, modificado, tamanho));
    }

    // Ocorrências agrupadas por nome, e a tabela de nomes ordenada por texto: é
    // o que dá busca exata e faixa por prefixo em cima do mesmo arranjo.
    let mut nomes: Vec<&String> = index.references.keys().collect();
    nomes.sort_unstable();
    let mut tabela_de_nomes = Vec::with_capacity(nomes.len());
    let mut ocorrencias = Vec::new();
    for nome in nomes {
        let inicio = u32::try_from(ocorrencias.len()).unwrap_or(u32::MAX);
        let lista = &index.references[nome];
        ocorrencias.extend(lista.iter().copied());
        let id = textos.id(nome);
        tabela_de_nomes.push((
            id,
            inicio,
            u32::try_from(lista.len()).unwrap_or(u32::MAX),
        ));
    }

    let mut genericos: Vec<u32> = Vec::new();
    let mut simbolos = Vec::with_capacity(index.symbols.len());
    for simbolo in &index.symbols {
        let nome = textos.id(&simbolo.name);
        let (tipo, dimensoes, genericos_inicio, genericos_conta) = match &simbolo.type_descriptor {
            Some(descritor) => {
                let inicio = u32::try_from(genericos.len()).unwrap_or(u32::MAX);
                let ids: Vec<u32> = descritor
                    .generic_arguments
                    .iter()
                    .map(|argumento| textos.id(argumento))
                    .collect();
                let conta = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                genericos.extend(ids);
                (
                    textos.id(&descritor.name),
                    descritor.array_dimensions,
                    inicio,
                    conta,
                )
            }
            None => (SEM_TIPO, 0, 0, 0),
        };
        simbolos.push((
            nome,
            numero_da_especie(simbolo.kind),
            dimensoes,
            simbolo.range,
            simbolo.scope_depth,
            simbolo.file,
            tipo,
            genericos_inicio,
            genericos_conta,
        ));
    }

    let mut declaracoes: Vec<(u32, u32)> = index
        .declarations
        .iter()
        .map(|(nome, arquivo)| (textos.id(nome), textos.id_do_caminho(arquivo)))
        .collect();
    // Ordem estável: dois arquivos gravados do mesmo índice têm de ser iguais,
    // e a ordem de um `HashMap` não é.
    declaracoes.sort_unstable();

    let externas: Vec<(u32, u32, u32)> = index
        .external_classes
        .iter()
        .map(|classe| {
            (
                textos.id(&classe.simple),
                textos.id(&classe.binary),
                textos.id_do_caminho(&classe.origin),
            )
        })
        .collect();

    // Com tudo acumulado, os deslocamentos são conhecidos.
    let mut cabecalho = Cabecalho::default();
    let mut posicao = CABECALHO as u64;
    let reservar = |quantos: usize, tamanho: usize, posicao: &mut u64| -> (u64, u32) {
        let inicio = *posicao;
        *posicao += (quantos * tamanho) as u64;
        (inicio, u32::try_from(quantos).unwrap_or(u32::MAX))
    };
    cabecalho.textos = (posicao, textos.blob.len() as u64);
    posicao += textos.blob.len() as u64;
    cabecalho.tabela_de_textos = reservar(textos.tabela.len(), tamanho::TEXTO, &mut posicao);
    cabecalho.nomes = reservar(tabela_de_nomes.len(), tamanho::NOME, &mut posicao);
    cabecalho.arquivos = reservar(arquivos.len(), tamanho::ARQUIVO, &mut posicao);
    cabecalho.simbolos = reservar(simbolos.len(), tamanho::SIMBOLO, &mut posicao);
    cabecalho.ocorrencias = reservar(ocorrencias.len(), tamanho::OCORRENCIA, &mut posicao);
    cabecalho.declaracoes = reservar(declaracoes.len(), tamanho::DECLARACAO, &mut posicao);
    cabecalho.externas = reservar(externas.len(), tamanho::EXTERNA, &mut posicao);
    cabecalho.genericos = reservar(genericos.len(), tamanho::GENERICO, &mut posicao);

    let mut saida: Vec<u8> = Vec::with_capacity(posicao as usize);
    saida.extend_from_slice(&MAGIC);
    saida.extend_from_slice(&VERSION.to_le_bytes());
    saida.extend_from_slice(&0u32.to_le_bytes());
    saida.extend_from_slice(&cabecalho.textos.0.to_le_bytes());
    saida.extend_from_slice(&cabecalho.textos.1.to_le_bytes());
    for (inicio, quantos) in [
        cabecalho.tabela_de_textos,
        cabecalho.nomes,
        cabecalho.arquivos,
        cabecalho.simbolos,
        cabecalho.ocorrencias,
        cabecalho.declaracoes,
        cabecalho.externas,
        cabecalho.genericos,
    ] {
        saida.extend_from_slice(&inicio.to_le_bytes());
        saida.extend_from_slice(&quantos.to_le_bytes());
        saida.extend_from_slice(&0u32.to_le_bytes());
    }
    debug_assert_eq!(saida.len(), CABECALHO);

    saida.extend_from_slice(&textos.blob);
    for (inicio, tamanho) in &textos.tabela {
        saida.extend_from_slice(&inicio.to_le_bytes());
        saida.extend_from_slice(&tamanho.to_le_bytes());
    }
    for (id, inicio, quantas) in &tabela_de_nomes {
        saida.extend_from_slice(&id.to_le_bytes());
        saida.extend_from_slice(&inicio.to_le_bytes());
        saida.extend_from_slice(&quantas.to_le_bytes());
    }
    for (id, modificado, tamanho) in &arquivos {
        saida.extend_from_slice(&id.to_le_bytes());
        saida.extend_from_slice(&0u32.to_le_bytes());
        saida.extend_from_slice(&modificado.to_le_bytes());
        saida.extend_from_slice(&tamanho.to_le_bytes());
    }
    for (nome, especie, dimensoes, faixa, profundidade, arquivo, tipo, gen_inicio, gen_conta) in
        &simbolos
    {
        saida.extend_from_slice(&nome.to_le_bytes());
        saida.push(*especie);
        saida.push(*dimensoes);
        saida.extend_from_slice(&0u16.to_le_bytes());
        escrever_faixa(&mut saida, *faixa);
        saida.extend_from_slice(&profundidade.to_le_bytes());
        saida.extend_from_slice(&arquivo.to_le_bytes());
        saida.extend_from_slice(&tipo.to_le_bytes());
        saida.extend_from_slice(&gen_inicio.to_le_bytes());
        saida.extend_from_slice(&gen_conta.to_le_bytes());
    }
    for ocorrencia in &ocorrencias {
        saida.extend_from_slice(&ocorrencia.file.to_le_bytes());
        escrever_faixa(&mut saida, ocorrencia.range);
    }
    for (nome, caminho) in &declaracoes {
        saida.extend_from_slice(&nome.to_le_bytes());
        saida.extend_from_slice(&caminho.to_le_bytes());
    }
    for (simples, binario, origem) in &externas {
        saida.extend_from_slice(&simples.to_le_bytes());
        saida.extend_from_slice(&binario.to_le_bytes());
        saida.extend_from_slice(&origem.to_le_bytes());
    }
    for generico in &genericos {
        saida.extend_from_slice(&generico.to_le_bytes());
    }

    if let Some(pasta) = path.parent() {
        fs::create_dir_all(pasta)?;
    }
    let temporario = path.with_extension("parcial");
    {
        let mut arquivo = fs::File::create(&temporario)?;
        arquivo.write_all(&saida)?;
        arquivo.sync_all()?;
    }
    fs::rename(&temporario, path)
}

fn escrever_faixa(saida: &mut Vec<u8>, faixa: TextRange) {
    saida.extend_from_slice(&faixa.start.line.to_le_bytes());
    saida.extend_from_slice(&faixa.start.column.to_le_bytes());
    saida.extend_from_slice(&faixa.end.line.to_le_bytes());
    saida.extend_from_slice(&faixa.end.column.to_le_bytes());
}

/// Lê o índice gravado, ou nada.
///
/// Devolve `None` para qualquer arquivo que não sirva — assinatura errada,
/// versão outra, truncado, deslocamento fora do arquivo, número de texto que não
/// existe. Nunca lê pela metade: quem chama reconstrói.
// Chamado pela fase 4, que é quem sabe decidir se o arquivo ainda vale. Até lá
// ele existe, está coberto por teste e não é usado — ler sem checar o que mudou
// serviria um índice vencido, que é o defeito que a `19` combateu.
#[allow(dead_code, reason = "o leitor entra em uso na fase 4 da 20")]
pub(in crate::index) fn read(path: &Path) -> Option<WorkspaceIndex> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() < CABECALHO || bytes.get(..8)? != MAGIC || ler_u32(&bytes, 8)? != VERSION {
        return None;
    }

    let textos_inicio = ler_u64(&bytes, 16)? as usize;
    let textos_tamanho = ler_u64(&bytes, 24)? as usize;
    let blob = bytes.get(textos_inicio..textos_inicio.checked_add(textos_tamanho)?)?;

    let mut areas = Vec::with_capacity(8);
    for indice in 0..8 {
        let base = 32 + indice * 16;
        areas.push((ler_u64(&bytes, base)? as usize, ler_u32(&bytes, base + 8)?));
    }
    let area = |indice: usize, tamanho: usize| -> Option<&[u8]> {
        let (inicio, quantos) = areas[indice];
        let fim = inicio.checked_add(quantos as usize * tamanho)?;
        bytes.get(inicio..fim)
    };

    // Os textos, resolvidos de uma vez: um número inválido em qualquer registro
    // invalida o arquivo inteiro, e é melhor descobrir aqui.
    let tabela = area(0, tamanho::TEXTO)?;
    let mut cadeias = Vec::with_capacity(areas[0].1 as usize);
    for registro in tabela.chunks_exact(tamanho::TEXTO) {
        let inicio = ler_u32(registro, 0)? as usize;
        let tamanho = ler_u32(registro, 4)? as usize;
        let trecho = blob.get(inicio..inicio.checked_add(tamanho)?)?;
        cadeias.push(std::str::from_utf8(trecho).ok()?.to_owned());
    }
    let texto = |id: u32| -> Option<&String> { cadeias.get(id as usize) };

    let genericos_crus = area(7, tamanho::GENERICO)?;
    let genericos: Vec<u32> = genericos_crus
        .chunks_exact(tamanho::GENERICO)
        .map(|registro| ler_u32(registro, 0))
        .collect::<Option<_>>()?;

    let mut files = Vec::with_capacity(areas[2].1 as usize);
    for registro in area(2, tamanho::ARQUIVO)?.chunks_exact(tamanho::ARQUIVO) {
        files.push(PathBuf::from(texto(ler_u32(registro, 0)?)?));
    }
    let file_ids = files
        .iter()
        .enumerate()
        .map(|(indice, caminho)| (caminho.clone(), u32::try_from(indice).unwrap_or(u32::MAX)))
        .collect();

    let ocorrencias_cruas = area(4, tamanho::OCORRENCIA)?;
    let mut references = HashMap::with_capacity(areas[1].1 as usize);
    for registro in area(1, tamanho::NOME)?.chunks_exact(tamanho::NOME) {
        let nome = texto(ler_u32(registro, 0)?)?.clone();
        let inicio = ler_u32(registro, 4)? as usize;
        let quantas = ler_u32(registro, 8)? as usize;
        let de = inicio.checked_mul(tamanho::OCORRENCIA)?;
        let ate = de.checked_add(quantas.checked_mul(tamanho::OCORRENCIA)?)?;
        let lista = ocorrencias_cruas
            .get(de..ate)?
            .chunks_exact(tamanho::OCORRENCIA)
            .map(|ocorrencia| {
                Some(Occurrence {
                    file: ler_u32(ocorrencia, 0)?,
                    range: ler_faixa(ocorrencia, 4)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        references.insert(nome, lista);
    }

    let mut symbols = Vec::with_capacity(areas[3].1 as usize);
    for registro in area(3, tamanho::SIMBOLO)?.chunks_exact(tamanho::SIMBOLO) {
        let tipo = ler_u32(registro, 32)?;
        let type_descriptor = if tipo == SEM_TIPO {
            None
        } else {
            let inicio = ler_u32(registro, 36)? as usize;
            let quantos = ler_u32(registro, 40)? as usize;
            Some(TypeDescriptor {
                name: texto(tipo)?.clone(),
                array_dimensions: *registro.get(5)?,
                generic_arguments: genericos
                    .get(inicio..inicio.checked_add(quantos)?)?
                    .iter()
                    .map(|id| texto(*id).cloned())
                    .collect::<Option<Vec<_>>>()?,
            })
        };
        symbols.push(IndexedSymbol {
            name: texto(ler_u32(registro, 0)?)?.clone(),
            kind: especie_do_numero(*registro.get(4)?)?,
            range: ler_faixa(registro, 8)?,
            type_descriptor,
            scope_depth: ler_u32(registro, 24)?,
            file: ler_u32(registro, 28)?,
        });
    }

    let mut declarations = HashMap::with_capacity(areas[5].1 as usize);
    for registro in area(5, tamanho::DECLARACAO)?.chunks_exact(tamanho::DECLARACAO) {
        declarations.insert(
            texto(ler_u32(registro, 0)?)?.clone(),
            PathBuf::from(texto(ler_u32(registro, 4)?)?),
        );
    }

    let mut external_classes = Vec::with_capacity(areas[6].1 as usize);
    for registro in area(6, tamanho::EXTERNA)?.chunks_exact(tamanho::EXTERNA) {
        external_classes.push(ExternalClass {
            simple: texto(ler_u32(registro, 0)?)?.clone(),
            binary: texto(ler_u32(registro, 4)?)?.clone(),
            origin: PathBuf::from(texto(ler_u32(registro, 8)?)?),
        });
    }

    Some(WorkspaceIndex {
        symbols,
        references,
        files,
        file_ids,
        external_classes,
        declarations,
    })
}

fn ler_u32(bytes: &[u8], em: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(em..em + 4)?.try_into().ok()?,
    ))
}

fn ler_u64(bytes: &[u8], em: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(em..em + 8)?.try_into().ok()?,
    ))
}

fn ler_faixa(bytes: &[u8], em: usize) -> Option<TextRange> {
    Some(TextRange {
        start: TextPosition {
            line: ler_u32(bytes, em)?,
            column: ler_u32(bytes, em + 4)?,
        },
        end: TextPosition {
            line: ler_u32(bytes, em + 8)?,
            column: ler_u32(bytes, em + 12)?,
        },
    })
}

/// Onde o índice de um projeto mora.
///
/// Um arquivo por raiz, com o nome derivado dela: duas IDEs em projetos
/// diferentes não disputam o mesmo arquivo.
/// A base segue o mesmo caminho de ambiente que a configuração da IDE usa: não
/// há por que inventar outro, nem trazer dependência para descobri-lo.
pub(in crate::index) fn caminho_do_indice(root: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    Some(
        base?
            .join("er-ide")
            .join(format!("indice-{:016x}.bin", hasher.finish())),
    )
}
