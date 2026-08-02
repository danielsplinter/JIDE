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

use super::{Dados, ExternalClass, IndexedSymbol, Occurrence};

/// Assinatura do arquivo. Um arquivo que não comece assim não é nosso.
const MAGIC: [u8; 8] = *b"ERIDEIDX";

/// Versão do formato.
///
/// Muda **sempre** que a forma de um registro mudar, inclusive a numeração das
/// espécies de símbolo. Um arquivo de outra versão é descartado, não convertido:
/// reconstruir o índice é caro mas correto, e converter formato velho é código
/// que ninguém testa.
const VERSION: u32 = 3;

#[cfg(test)]
pub(in crate::analyzer::index) const VERSION_PARA_TESTE: u32 = VERSION;

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
pub(in crate::analyzer::index) const CABECALHO_PARA_TESTE: usize = CABECALHO;

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
pub(in crate::analyzer::index) fn write(index: &Dados, path: &Path) -> io::Result<()> {
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
    // Ordenadas pelo nome **em minúsculas**: é o que dá faixa por prefixo sem
    // distinção de maiúsculas, que é como a busca por tipo compara. A faixa
    // sensível a maiúsculas, que a completação usa, é um subconjunto dela.
    // Nada endereça um símbolo por posição, então reordenar não quebra nada.
    let mut ordenados: Vec<&IndexedSymbol> = index.symbols.iter().collect();
    ordenados.sort_by(|esquerda, direita| {
        esquerda
            .name
            .to_ascii_lowercase()
            .cmp(&direita.name.to_ascii_lowercase())
            .then_with(|| esquerda.name.cmp(&direita.name))
            .then(esquerda.file.cmp(&direita.file))
            .then(esquerda.range.start.line.cmp(&direita.range.start.line))
            .then(esquerda.range.start.column.cmp(&direita.range.start.column))
    });
    for simbolo in ordenados {
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

    // Ordenadas pelo **texto** do nome, e não pelo número: é assim que quem lê o
    // mapeamento acha um tipo sem percorrer os trinta mil. Ordem estável também
    // resolve o outro problema: a de um `HashMap` não é.
    let mut declaracoes: Vec<(String, u32, u32)> = index
        .declarations
        .iter()
        .map(|(nome, arquivo)| {
            (
                nome.clone(),
                textos.id(nome),
                textos.id_do_caminho(arquivo),
            )
        })
        .collect();
    declaracoes.sort_unstable();
    let declaracoes: Vec<(u32, u32)> = declaracoes
        .into_iter()
        .map(|(_, nome, arquivo)| (nome, arquivo))
        .collect();

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
pub(in crate::analyzer::index) fn read(path: &Path) -> Option<Dados> {
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

    Some(Dados {
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
/// Um arquivo por raiz **e por JDK**, com o nome derivado dos dois: dois
/// projetos não disputam o mesmo arquivo, e trocar de JDK não faz um índice
/// responder pelas classes do outro.
/// A base segue o mesmo caminho de ambiente que a configuração da IDE usa: não
/// há por que inventar outro, nem trazer dependência para descobri-lo.
pub(in crate::analyzer::index) fn caminho_do_indice(root: &Path, toolchain: Option<&Path>) -> Option<PathBuf> {
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
    // O JDK entra na identidade do arquivo. Sem isso, trocar de JDK e reabrir
    // serviria as classes do anterior — resposta errada em silêncio, que é o
    // defeito que a `19` combateu. Cada JDK guarda o seu, e voltar ao anterior
    // reaproveita o que ele já tinha.
    toolchain.hash(&mut hasher);
    Some(
        base?
            .join("er-ide")
            .join(format!("indice-{:016x}.bin", hasher.finish())),
    )
}

/// O índice **carregado**: as respostas saem dos bytes, sem materializar nada.
///
/// É a fase 2 da `20`. A diferença para `read` é toda a razão de existir: `read`
/// devolve estruturas — nomes, caminhos e vetores que a IDE segura enquanto
/// viver. Aqui os bytes do arquivo são a estrutura, e uma consulta é um salto
/// dentro deles.
///
/// **A especificação pedia mapeamento, e isto é uma leitura.** `memmap2::Mmap`
/// exige `unsafe`, e o workspace tem `unsafe_code = "forbid"` — decisão de
/// arquitetura, com precedente na ADR-013. A troca é de uma linha no dia em que
/// essa decisão mudar, porque tudo abaixo trabalha sobre `&[u8]` e não sobre o
/// que os produziu. O que se perde enquanto isso é a **elasticidade**: estes
/// bytes são memória nossa, e o sistema operacional não os recupera sozinho.
pub(in crate::analyzer::index) struct Carregado {
    mapa: Vec<u8>,
    textos: (usize, usize),
    /// `(início, quantos)` de cada área, na ordem em que o cabeçalho as lista.
    areas: [(usize, usize); 8],
}

/// Índices das áreas dentro de `areas`, para não escrever número solto.
mod area {
    pub(super) const TEXTOS: usize = 0;
    pub(super) const NOMES: usize = 1;
    pub(super) const ARQUIVOS: usize = 2;
    pub(super) const SIMBOLOS: usize = 3;
    pub(super) const OCORRENCIAS: usize = 4;
    pub(super) const DECLARACOES: usize = 5;
    pub(super) const EXTERNAS: usize = 6;
    pub(super) const GENERICOS: usize = 7;
}

/// Um símbolo lido do arquivo, ainda em bytes.
#[derive(Clone, Copy)]
pub(crate) struct SimboloNoDisco<'a> {
    mapeado: &'a Carregado,
    registro: &'a [u8],
}

/// Uma classe externa lida do arquivo, ainda em bytes.
#[derive(Clone, Copy)]
pub(crate) struct ExternaNoDisco<'a> {
    mapeado: &'a Carregado,
    registro: &'a [u8],
}

impl Carregado {
    /// Mapeia o arquivo, conferindo tudo o que dá para conferir sem lê-lo.
    ///
    /// Recusa o que `read` recusaria: assinatura, versão e qualquer área que
    /// caia fora do arquivo. O que não dá para conferir aqui — um número de
    /// texto inválido lá dentro — é conferido ao ler cada registro, que responde
    /// vazio em vez de estourar.
    pub(in crate::analyzer::index) fn open(path: &Path) -> Option<Self> {
        let mapa = fs::read(path).ok()?;
        if mapa.len() < CABECALHO || mapa.get(..8)? != MAGIC || ler_u32(&mapa, 8)? != VERSION {
            return None;
        }
        let textos_inicio = usize::try_from(ler_u64(&mapa, 16)?).ok()?;
        let textos_tamanho = usize::try_from(ler_u64(&mapa, 24)?).ok()?;
        if textos_inicio.checked_add(textos_tamanho)? > mapa.len() {
            return None;
        }
        let tamanhos = [
            tamanho::TEXTO,
            tamanho::NOME,
            tamanho::ARQUIVO,
            tamanho::SIMBOLO,
            tamanho::OCORRENCIA,
            tamanho::DECLARACAO,
            tamanho::EXTERNA,
            tamanho::GENERICO,
        ];
        let mut areas = [(0usize, 0usize); 8];
        for (indice, area) in areas.iter_mut().enumerate() {
            let base = 32 + indice * 16;
            let inicio = usize::try_from(ler_u64(&mapa, base)?).ok()?;
            let quantos = ler_u32(&mapa, base + 8)? as usize;
            if inicio.checked_add(quantos.checked_mul(tamanhos[indice])?)? > mapa.len() {
                return None;
            }
            *area = (inicio, quantos);
        }
        Some(Self {
            mapa,
            textos: (textos_inicio, textos_tamanho),
            areas,
        })
    }

    fn registro(&self, area: usize, tamanho: usize, indice: usize) -> Option<&[u8]> {
        let (inicio, quantos) = self.areas[area];
        if indice >= quantos {
            return None;
        }
        let de = inicio + indice * tamanho;
        self.mapa.get(de..de + tamanho)
    }

    /// Um texto, pelo número. Sem cópia: aponta para dentro do arquivo lido.
    fn texto(&self, id: u32) -> Option<&str> {
        let registro = self.registro(area::TEXTOS, tamanho::TEXTO, id as usize)?;
        let inicio = self.textos.0 + ler_u32(registro, 0)? as usize;
        let fim = inicio + ler_u32(registro, 4)? as usize;
        std::str::from_utf8(self.mapa.get(inicio..fim)?).ok()
    }

    /// O caminho de um arquivo, pelo número.
    pub(in crate::analyzer::index) fn arquivo(&self, id: u32) -> Option<&str> {
        let registro = self.registro(area::ARQUIVOS, tamanho::ARQUIVO, id as usize)?;
        self.texto(ler_u32(registro, 0)?)
    }

    /// Quantos arquivos o índice conhece.
    pub(in crate::analyzer::index) fn arquivos(&self) -> usize {
        self.areas[area::ARQUIVOS].1
    }

    /// Caminho, data de modificação e tamanho de um arquivo indexado.
    ///
    /// É o que a fase 4 compara para saber o que mudou desde a gravação.
    pub(in crate::analyzer::index) fn arquivo_gravado(&self, indice: usize) -> Option<(&str, u64, u64)> {
        let registro = self.registro(area::ARQUIVOS, tamanho::ARQUIVO, indice)?;
        Some((
            self.texto(ler_u32(registro, 0)?)?,
            ler_u64(registro, 8)?,
            ler_u64(registro, 16)?,
        ))
    }

    /// O nome na posição dada da tabela **ordenada** de nomes.
    fn nome(&self, indice: usize) -> Option<&str> {
        let registro = self.registro(area::NOMES, tamanho::NOME, indice)?;
        self.texto(ler_u32(registro, 0)?)
    }

    /// Todos os nomes que têm ocorrências, na ordem em que estão gravados.
    ///
    /// Serve a quem precisa reconstruir o índice inteiro — a regravação da fase
    /// 4 — e não ao caminho de consulta, que vai direto ao nome procurado.
    pub(in crate::analyzer::index) fn nomes_gravados(&self) -> impl Iterator<Item = &str> {
        (0..self.areas[area::NOMES].1).filter_map(|indice| self.nome(indice))
    }

    /// Onde um nome aparece: a faixa de ocorrências dele.
    ///
    /// Busca binária sobre a tabela ordenada — é para isto que ela é ordenada, e
    /// é o que faz Ctrl+clique não tocar o resto do arquivo.
    pub(in crate::analyzer::index) fn ocorrencias_de(&self, nome: &str) -> impl Iterator<Item = Occurrence> {
        let faixa = self.procurar(area::NOMES, nome, |indice| self.nome(indice));
        let (inicio, quantas) = faixa
            .and_then(|indice| {
                let registro = self.registro(area::NOMES, tamanho::NOME, indice)?;
                Some((ler_u32(registro, 4)? as usize, ler_u32(registro, 8)? as usize))
            })
            .unwrap_or((0, 0));
        (inicio..inicio + quantas).filter_map(|indice| self.ocorrencia(indice))
    }

    fn ocorrencia(&self, indice: usize) -> Option<Occurrence> {
        let registro = self.registro(area::OCORRENCIAS, tamanho::OCORRENCIA, indice)?;
        Some(Occurrence {
            file: ler_u32(registro, 0)?,
            range: ler_faixa(registro, 4)?,
        })
    }

    /// Todas as declarações do projeto.
    pub(in crate::analyzer::index) fn simbolos(&self) -> impl Iterator<Item = SimboloNoDisco<'_>> {
        (0..self.areas[area::SIMBOLOS].1).filter_map(|indice| {
            Some(SimboloNoDisco {
                mapeado: self,
                registro: self.registro(area::SIMBOLOS, tamanho::SIMBOLO, indice)?,
            })
        })
    }

    /// As declarações cujo nome começa com o prefixo, sem distinguir maiúsculas.
    ///
    /// É o que a fase 3 da `20` entrega. A área de símbolos é ordenada pelo nome
    /// em minúsculas, então as que interessam são **contíguas**: acha-se a
    /// primeira por busca binária e anda-se enquanto o prefixo valer. Digitar
    /// uma letra deixa de percorrer as trezentas e trinta mil.
    ///
    /// Prefixo vazio devolve tudo — quem pede tudo recebe tudo, e é o único caso
    /// em que percorrer tudo é a resposta certa.
    pub(in crate::analyzer::index) fn simbolos_com_prefixo(
        &self,
        prefixo: &str,
    ) -> impl Iterator<Item = SimboloNoDisco<'_>> {
        let minusculo = prefixo.to_ascii_lowercase();
        let inicio = self.limite_inferior(&minusculo);
        let total = self.areas[area::SIMBOLOS].1;
        (inicio..total)
            .map(|indice| SimboloNoDisco {
                mapeado: self,
                registro: self
                    .registro(area::SIMBOLOS, tamanho::SIMBOLO, indice)
                    .unwrap_or_default(),
            })
            .take_while(move |simbolo| {
                simbolo
                    .name()
                    .to_ascii_lowercase()
                    .starts_with(minusculo.as_str())
            })
    }

    /// A primeira posição cujo nome em minúsculas não é menor que o alvo.
    fn limite_inferior(&self, alvo: &str) -> usize {
        let (mut baixo, mut alto) = (0usize, self.areas[area::SIMBOLOS].1);
        while baixo < alto {
            let meio = baixo + (alto - baixo) / 2;
            let Some(registro) = self.registro(area::SIMBOLOS, tamanho::SIMBOLO, meio) else {
                return alto;
            };
            let nome = SimboloNoDisco {
                mapeado: self,
                registro,
            }
            .name()
            .to_ascii_lowercase();
            if nome.as_str() < alvo {
                baixo = meio + 1;
            } else {
                alto = meio;
            }
        }
        baixo
    }

    /// Quantas declarações — serve à medição.
    #[cfg(test)]
    pub(in crate::analyzer::index) fn simbolos_conta(&self) -> usize {
        self.areas[area::SIMBOLOS].1
    }

    /// O arquivo que declara um tipo, pelo nome simples.
    pub(in crate::analyzer::index) fn declaracao(&self, nome: &str) -> Option<&str> {
        let indice = self.procurar(area::DECLARACOES, nome, |indice| {
            let registro = self.registro(area::DECLARACOES, tamanho::DECLARACAO, indice)?;
            self.texto(ler_u32(registro, 0)?)
        })?;
        let registro = self.registro(area::DECLARACOES, tamanho::DECLARACAO, indice)?;
        self.texto(ler_u32(registro, 4)?)
    }

    /// Quantos tipos declarados — serve à medição.
    #[cfg(test)]
    pub(in crate::analyzer::index) fn declaracoes_conta(&self) -> usize {
        self.areas[area::DECLARACOES].1
    }

    /// As classes do JDK e dos jars.
    pub(in crate::analyzer::index) fn externas(&self) -> impl Iterator<Item = ExternaNoDisco<'_>> {
        (0..self.areas[area::EXTERNAS].1).filter_map(|indice| {
            Some(ExternaNoDisco {
                mapeado: self,
                registro: self.registro(area::EXTERNAS, tamanho::EXTERNA, indice)?,
            })
        })
    }

    /// Busca binária numa área ordenada por texto.
    fn procurar<'a>(
        &'a self,
        area: usize,
        alvo: &str,
        chave: impl Fn(usize) -> Option<&'a str>,
    ) -> Option<usize> {
        let (mut baixo, mut alto) = (0usize, self.areas[area].1);
        while baixo < alto {
            let meio = baixo + (alto - baixo) / 2;
            match chave(meio)?.cmp(alvo) {
                std::cmp::Ordering::Less => baixo = meio + 1,
                std::cmp::Ordering::Greater => alto = meio,
                std::cmp::Ordering::Equal => return Some(meio),
            }
        }
        None
    }
}

impl SimboloNoDisco<'_> {
    pub(in crate::analyzer::index) fn name(&self) -> &str {
        ler_u32(self.registro, 0)
            .and_then(|id| self.mapeado.texto(id))
            .unwrap_or_default()
    }

    pub(in crate::analyzer::index) fn kind(&self) -> SymbolKind {
        self.registro
            .get(4)
            .copied()
            .and_then(especie_do_numero)
            .unwrap_or(SymbolKind::LocalVariable)
    }

    /// Só o nome do tipo: é o que a completação mostra, e assim ela não paga
    /// montagem de descritor por símbolo.
    pub(in crate::analyzer::index) fn type_name(&self) -> Option<&str> {
        let tipo = ler_u32(self.registro, 32)?;
        if tipo == SEM_TIPO {
            return None;
        }
        self.mapeado.texto(tipo)
    }

    /// O descritor inteiro, para quem devolve `SemanticSymbol`.
    pub(in crate::analyzer::index) fn type_descriptor(&self) -> Option<TypeDescriptor> {
        let tipo = ler_u32(self.registro, 32)?;
        if tipo == SEM_TIPO {
            return None;
        }
        let inicio = ler_u32(self.registro, 36)? as usize;
        let quantos = ler_u32(self.registro, 40)? as usize;
        let generic_arguments = (inicio..inicio + quantos)
            .filter_map(|indice| {
                let registro = self
                    .mapeado
                    .registro(area::GENERICOS, tamanho::GENERICO, indice)?;
                Some(self.mapeado.texto(ler_u32(registro, 0)?)?.to_owned())
            })
            .collect();
        Some(TypeDescriptor {
            name: self.mapeado.texto(tipo)?.to_owned(),
            array_dimensions: self.registro.get(5).copied().unwrap_or(0),
            generic_arguments,
        })
    }

    pub(in crate::analyzer::index) fn range(&self) -> TextRange {
        ler_faixa(self.registro, 8).unwrap_or_default()
    }

    pub(in crate::analyzer::index) fn scope_depth(&self) -> u32 {
        ler_u32(self.registro, 24).unwrap_or(0)
    }

    pub(in crate::analyzer::index) fn file(&self) -> u32 {
        ler_u32(self.registro, 28).unwrap_or(u32::MAX)
    }
}

impl ExternaNoDisco<'_> {
    pub(in crate::analyzer::index) fn simple(&self) -> &str {
        ler_u32(self.registro, 0)
            .and_then(|id| self.mapeado.texto(id))
            .unwrap_or_default()
    }

    pub(in crate::analyzer::index) fn binary(&self) -> &str {
        ler_u32(self.registro, 4)
            .and_then(|id| self.mapeado.texto(id))
            .unwrap_or_default()
    }

    pub(in crate::analyzer::index) fn origin(&self) -> &str {
        ler_u32(self.registro, 8)
            .and_then(|id| self.mapeado.texto(id))
            .unwrap_or_default()
    }
}
