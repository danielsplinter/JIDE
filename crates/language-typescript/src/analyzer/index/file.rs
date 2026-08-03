//! O índice em disco: registros de tamanho fixo, lidos por deslocamento.
//!
//! É a fase 1 da `25`, e o formato segue o que a `20` provou para Java —
//! registros fixos, textos à parte, tabela de nomes ordenada. O que muda é a
//! **leitura**.
//!
//! # A diferença que importa: nada é lido inteiro
//!
//! A `20` lê o arquivo todo para um vetor de bytes, e por isso os 103 MB do
//! índice de Java são memória nossa, retida enquanto a IDE viver — ela própria
//! registra que isso é "redução, não empréstimo". Aqui só a **tabela de nomes**
//! entra em memória, porque toda busca por texto a percorre; os registros de
//! símbolo ficam no disco e são lidos por deslocamento, quando um nome casa.
//!
//! Medido: leituras de 4 KB em posições aleatórias de um arquivo de 100 MB
//! custam cerca de 30 µs, e quem mantém em cache o que foi lido é o sistema
//! operacional — com a política dele, sem terceira cópia da mesma informação e
//! sem `unsafe`, que é onde a `20` esbarrou (ADR-023).

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use ide_domain::SymbolKind;

/// Assinatura do arquivo. Um arquivo que não comece assim não é nosso.
const MAGIC: [u8; 8] = *b"ERTSIDX1";

/// Versão do formato.
///
/// Muda **sempre** que a forma de um registro mudar, inclusive a numeração das
/// espécies. Arquivo de outra versão é descartado, não convertido: reconstruir é
/// caro e correto, e converter formato velho é código que ninguém testa.
const VERSION: u32 = 1;

/// Números das espécies, escritos à mão.
///
/// Derivar da ordem do `enum` faria reordenar uma variante corromper todo
/// arquivo já gravado, em silêncio.
const fn numero_da_especie(kind: SymbolKind) -> u8 {
    match kind {
        SymbolKind::Class => 1,
        SymbolKind::Interface => 2,
        SymbolKind::Enum => 3,
        _ => 0,
    }
}

const fn especie_do_numero(numero: u8) -> Option<SymbolKind> {
    match numero {
        1 => Some(SymbolKind::Class),
        2 => Some(SymbolKind::Interface),
        3 => Some(SymbolKind::Enum),
        _ => None,
    }
}

mod tamanho {
    /// `(início, tamanho)` de um texto dentro do blob.
    pub(super) const TEXTO: usize = 8;
    /// `(texto, primeiro símbolo, quantos)`.
    pub(super) const NOME: usize = 12;
    /// `(texto do caminho, espécie, linha, coluna, linha fim, coluna fim)`.
    pub(super) const SIMBOLO: usize = 24;
}

/// Um símbolo lido do índice: onde ele está e o que ele é.
///
/// Nomeado, e não uma tupla de quatro campos: quem lê `simbolos()` precisa saber
/// o que é cada posição, e uma tupla obriga a voltar aqui para descobrir.
pub(super) struct Gravado {
    pub(super) arquivo: PathBuf,
    pub(super) kind: SymbolKind,
    pub(super) inicio: (u32, u32),
    pub(super) fim: (u32, u32),
}

/// Uma declaração pronta para gravar.
pub(super) struct Declaracao {
    pub(super) nome: String,
    pub(super) arquivo: PathBuf,
    pub(super) kind: SymbolKind,
    pub(super) inicio: (u32, u32),
    pub(super) fim: (u32, u32),
}

/// Acumula os textos e devolve o número de cada um, sem repetir.
///
/// Num monorepo o mesmo caminho aparece em dezenas de declarações e o mesmo nome
/// em várias; repetir a cadeia em cada registro foi, medido na `20`, a maior
/// parte do arquivo.
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
}

/// Escreve o índice no caminho dado.
///
/// Grava num arquivo temporário e renomeia: um desligamento no meio da escrita
/// deixaria um arquivo pela metade, e ler índice truncado é pior do que não ter
/// índice.
pub(super) fn write(mut declaracoes: Vec<Declaracao>, path: &Path) -> io::Result<()> {
    // **Ordenado pelo nome**, que é o que permite a busca por nome exato sair em
    // tempo logarítmico e a faixa por prefixo ser contígua.
    declaracoes.sort_by(|esquerda, direita| {
        esquerda
            .nome
            .cmp(&direita.nome)
            .then_with(|| esquerda.arquivo.cmp(&direita.arquivo))
    });

    let mut textos = Textos::default();
    let mut simbolos: Vec<u8> = Vec::new();
    let mut nomes: Vec<u8> = Vec::new();
    let mut anterior: Option<(String, u32, u32)> = None;

    for declaracao in &declaracoes {
        let caminho = textos.id(&declaracao.arquivo.to_string_lossy());
        let posicao = u32::try_from(simbolos.len() / tamanho::SIMBOLO).unwrap_or(u32::MAX);
        simbolos.extend_from_slice(&caminho.to_le_bytes());
        simbolos.extend_from_slice(&u32::from(numero_da_especie(declaracao.kind)).to_le_bytes());
        simbolos.extend_from_slice(&declaracao.inicio.0.to_le_bytes());
        simbolos.extend_from_slice(&declaracao.inicio.1.to_le_bytes());
        simbolos.extend_from_slice(&declaracao.fim.0.to_le_bytes());
        simbolos.extend_from_slice(&declaracao.fim.1.to_le_bytes());

        // As declarações vêm ordenadas por nome, então as do mesmo nome são
        // vizinhas: contar quantas são é somar enquanto o nome não muda, e o
        // registro de nome sai quando ele muda.
        let mesmo_nome = anterior
            .as_ref()
            .is_some_and(|(nome, _, _)| *nome == declaracao.nome);
        if mesmo_nome {
            if let Some((_, _, quantos)) = anterior.as_mut() {
                *quantos += 1;
            }
        } else {
            if let Some((nome, primeiro, quantos)) = anterior.take() {
                let id = textos.id(&nome);
                nomes.extend_from_slice(&id.to_le_bytes());
                nomes.extend_from_slice(&primeiro.to_le_bytes());
                nomes.extend_from_slice(&quantos.to_le_bytes());
            }
            anterior = Some((declaracao.nome.clone(), posicao, 1));
        }
    }
    if let Some((nome, primeiro, quantos)) = anterior {
        let id = textos.id(&nome);
        nomes.extend_from_slice(&id.to_le_bytes());
        nomes.extend_from_slice(&primeiro.to_le_bytes());
        nomes.extend_from_slice(&quantos.to_le_bytes());
    }

    let mut tabela: Vec<u8> = Vec::with_capacity(textos.tabela.len() * tamanho::TEXTO);
    for (inicio, comprimento) in &textos.tabela {
        tabela.extend_from_slice(&inicio.to_le_bytes());
        tabela.extend_from_slice(&comprimento.to_le_bytes());
    }

    // O cabeçalho traz deslocamento **e** contagem de cada área, para a consulta
    // ir direto onde precisa sem percorrer o que veio antes.
    let cabecalho = CABECALHO as u64;
    let off_textos = cabecalho;
    let off_tabela = off_textos + textos.blob.len() as u64;
    let off_nomes = off_tabela + tabela.len() as u64;
    let off_simbolos = off_nomes + nomes.len() as u64;

    let mut saida: Vec<u8> = Vec::with_capacity(CABECALHO);
    saida.extend_from_slice(&MAGIC);
    saida.extend_from_slice(&VERSION.to_le_bytes());
    saida.extend_from_slice(&off_textos.to_le_bytes());
    saida.extend_from_slice(&(textos.blob.len() as u64).to_le_bytes());
    saida.extend_from_slice(&off_tabela.to_le_bytes());
    saida.extend_from_slice(&(textos.tabela.len() as u32).to_le_bytes());
    saida.extend_from_slice(&off_nomes.to_le_bytes());
    saida.extend_from_slice(&((nomes.len() / tamanho::NOME) as u32).to_le_bytes());
    saida.extend_from_slice(&off_simbolos.to_le_bytes());
    saida.extend_from_slice(&((simbolos.len() / tamanho::SIMBOLO) as u32).to_le_bytes());

    if let Some(pasta) = path.parent() {
        fs::create_dir_all(pasta)?;
    }
    // O temporário leva o número do processo: dois construindo o índice do mesmo
    // projeto ao mesmo tempo escreveriam no mesmo arquivo e um renomearia o
    // pedaço do outro. Acontece em teste, e aconteceria com duas IDEs abertas.
    let temporario = path.with_extension(format!("parcial{}", std::process::id()));
    {
        let mut arquivo = File::create(&temporario)?;
        arquivo.write_all(&saida)?;
        arquivo.write_all(&textos.blob)?;
        arquivo.write_all(&tabela)?;
        arquivo.write_all(&nomes)?;
        arquivo.write_all(&simbolos)?;
        arquivo.sync_all()?;
    }
    fs::rename(&temporario, path)
}

/// Assinatura, versão, e `(deslocamento, contagem)` de quatro áreas.
const CABECALHO: usize = 8 + 4 + (8 + 8) + (8 + 4) + (8 + 4) + (8 + 4);

/// Um índice aberto, pronto para responder.
///
/// **Ele não tem o arquivo em memória.** O que ele tem é a tabela de nomes —
/// pequena e percorrida por toda busca — e um descritor aberto para ir buscar o
/// resto quando um nome casar.
pub(super) struct Aberto {
    arquivo: Mutex<File>,
    /// Nome, primeiro símbolo e quantos, na ordem gravada.
    nomes: Vec<(String, u32, u32)>,
    off_textos: u64,
    off_simbolos: u64,
    textos: Vec<(u32, u32)>,
}

impl Aberto {
    /// Abre o índice e carrega **só** a tabela de nomes.
    pub(super) fn open(path: &Path) -> Option<Self> {
        let mut arquivo = File::open(path).ok()?;
        let mut cabecalho = [0u8; CABECALHO];
        arquivo.read_exact(&mut cabecalho).ok()?;
        if cabecalho.get(..8)? != MAGIC || ler_u32(&cabecalho, 8)? != VERSION {
            return None;
        }
        let off_textos = ler_u64(&cabecalho, 12)?;
        let len_textos = ler_u64(&cabecalho, 20)?;
        let off_tabela = ler_u64(&cabecalho, 28)?;
        let quantos_textos = ler_u32(&cabecalho, 36)? as usize;
        let off_nomes = ler_u64(&cabecalho, 40)?;
        let quantos_nomes = ler_u32(&cabecalho, 48)? as usize;
        let off_simbolos = ler_u64(&cabecalho, 52)?;

        // A tabela de textos e a de nomes vêm em **duas leituras**, e não uma por
        // registro: são as únicas áreas que toda busca percorre inteiras, e
        // buscá-las de trinta em trinta microssegundos seria pagar o preço da
        // leitura sob demanda onde ela não rende nada.
        let tabela = ler_bloco(&mut arquivo, off_tabela, quantos_textos * tamanho::TEXTO)?;
        let textos: Vec<(u32, u32)> = (0..quantos_textos)
            .filter_map(|indice| {
                let base = indice * tamanho::TEXTO;
                Some((ler_u32(&tabela, base)?, ler_u32(&tabela, base + 4)?))
            })
            .collect();

        let blob = ler_bloco(&mut arquivo, off_textos, len_textos as usize)?;
        let cru = ler_bloco(&mut arquivo, off_nomes, quantos_nomes * tamanho::NOME)?;
        let nomes: Vec<(String, u32, u32)> = (0..quantos_nomes)
            .filter_map(|indice| {
                let base = indice * tamanho::NOME;
                let (inicio, comprimento) = *textos.get(ler_u32(&cru, base)? as usize)?;
                let fatia = blob.get(inicio as usize..(inicio + comprimento) as usize)?;
                Some((
                    String::from_utf8(fatia.to_vec()).ok()?,
                    ler_u32(&cru, base + 4)?,
                    ler_u32(&cru, base + 8)?,
                ))
            })
            .collect();

        Some(Self {
            arquivo: Mutex::new(arquivo),
            nomes,
            off_textos,
            off_simbolos,
            textos,
        })
    }

    /// Quantos nomes distintos o índice tem.
    #[cfg(test)]
    pub(super) fn nomes(&self) -> usize {
        self.nomes.len()
    }

    /// Os nomes gravados, na ordem, para quem quer filtrar por texto.
    pub(super) fn cada_nome(&self) -> impl Iterator<Item = (&str, u32, u32)> {
        self.nomes
            .iter()
            .map(|(nome, primeiro, quantos)| (nome.as_str(), *primeiro, *quantos))
    }

    /// Os símbolos de um nome, lidos do disco agora.
    ///
    /// É aqui que a leitura sob demanda acontece: só os registros deste nome
    /// saem do arquivo, e só quando alguém já decidiu que ele interessa.
    pub(super) fn simbolos(&self, primeiro: u32, quantos: u32) -> Vec<Gravado> {
        let Ok(mut arquivo) = self.arquivo.lock() else {
            return Vec::new();
        };
        let base = self.off_simbolos + u64::from(primeiro) * tamanho::SIMBOLO as u64;
        let Some(bloco) = ler_bloco(&mut arquivo, base, quantos as usize * tamanho::SIMBOLO) else {
            return Vec::new();
        };
        (0..quantos as usize)
            .filter_map(|indice| {
                let campo = indice * tamanho::SIMBOLO;
                let caminho = self.texto(&mut arquivo, ler_u32(&bloco, campo)?)?;
                let kind = especie_do_numero(ler_u32(&bloco, campo + 4)? as u8)?;
                Some(Gravado {
                    arquivo: PathBuf::from(caminho),
                    kind,
                    inicio: (ler_u32(&bloco, campo + 8)?, ler_u32(&bloco, campo + 12)?),
                    fim: (ler_u32(&bloco, campo + 16)?, ler_u32(&bloco, campo + 20)?),
                })
            })
            .collect()
    }

    /// Um texto do blob, lido agora.
    fn texto(&self, arquivo: &mut File, id: u32) -> Option<String> {
        let (inicio, comprimento) = *self.textos.get(id as usize)?;
        let bytes = ler_bloco(
            arquivo,
            self.off_textos + u64::from(inicio),
            comprimento as usize,
        )?;
        String::from_utf8(bytes).ok()
    }
}

/// Lê um pedaço do arquivo, a partir de um deslocamento.
fn ler_bloco(arquivo: &mut File, deslocamento: u64, tamanho: usize) -> Option<Vec<u8>> {
    if tamanho == 0 {
        return Some(Vec::new());
    }
    arquivo.seek(SeekFrom::Start(deslocamento)).ok()?;
    let mut bytes = vec![0u8; tamanho];
    arquivo.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

fn ler_u32(bytes: &[u8], em: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(em..em + 4)?.try_into().ok()?))
}

fn ler_u64(bytes: &[u8], em: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(em..em + 8)?.try_into().ok()?))
}

/// Onde o índice de um projeto mora.
///
/// Fora do projeto: um arquivo nosso dentro dele apareceria no controle de
/// versão, na busca por conteúdo e na árvore do Explorer de quem só quer editar.
pub(super) fn caminho_do_indice(root: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    Some(
        base.join("er-ide")
            .join("typescript")
            .join(format!("{:016x}.idx", hasher.finish())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declaracao(nome: &str, arquivo: &str, kind: SymbolKind) -> Declaracao {
        Declaracao {
            nome: nome.to_owned(),
            arquivo: PathBuf::from(arquivo),
            kind,
            inicio: (1, 0),
            fim: (1, 10),
        }
    }

    fn temporario(nome: &str) -> PathBuf {
        let caminho = std::env::temp_dir().join(format!(
            "er-ts-idx-{nome}-{}.idx",
            std::process::id()
        ));
        let _ = fs::remove_file(&caminho);
        caminho
    }

    /// O que entrou sai, com nome, arquivo, espécie e posição.
    #[test]
    fn what_goes_in_comes_back_out() {
        let caminho = temporario("ida-e-volta");
        assert!(
            write(
                vec![
                    declaracao("Pedido", "/p/pedido.ts", SymbolKind::Class),
                    declaracao("Resumo", "/p/resumo.ts", SymbolKind::Interface),
                ],
                &caminho,
            )
            .is_ok()
        );
        let Some(aberto) = Aberto::open(&caminho) else {
            panic!("o índice recém-escrito precisa abrir");
        };
        assert_eq!(aberto.nomes(), 2);
        let nomes: Vec<_> = aberto.cada_nome().map(|(nome, _, _)| nome).collect();
        assert_eq!(nomes, vec!["Pedido", "Resumo"], "gravado em ordem de nome");

        let Some((_, primeiro, quantos)) = aberto.cada_nome().next() else {
            panic!("há nomes");
        };
        let simbolos = aberto.simbolos(primeiro, quantos);
        assert_eq!(simbolos.len(), 1);
        assert_eq!(simbolos[0].arquivo, PathBuf::from("/p/pedido.ts"));
        assert_eq!(simbolos[0].kind, SymbolKind::Class);
        assert_eq!(simbolos[0].inicio, (1, 0));
        let _ = fs::remove_file(&caminho);
    }

    /// O mesmo nome em dois arquivos vira um nome com dois símbolos.
    ///
    /// **É o caso que a `25` diz ser o difícil em TypeScript**: `LoginService`
    /// em dois módulos são duas coisas. O índice guarda as duas, e quem resolve
    /// qual é a certa é a fase 2 — aqui elas não podem se perder.
    #[test]
    fn the_same_name_in_two_files_keeps_both() {
        let caminho = temporario("nome-repetido");
        assert!(
            write(
                vec![
                    declaracao("LoginService", "/p/a/login.ts", SymbolKind::Class),
                    declaracao("LoginService", "/p/b/login.ts", SymbolKind::Class),
                ],
                &caminho,
            )
            .is_ok()
        );
        let Some(aberto) = Aberto::open(&caminho) else {
            panic!("o índice precisa abrir");
        };
        assert_eq!(aberto.nomes(), 1, "um nome só");
        let Some((_, primeiro, quantos)) = aberto.cada_nome().next() else {
            panic!("há um nome");
        };
        assert_eq!(quantos, 2, "e dois símbolos sob ele");
        let simbolos = aberto.simbolos(primeiro, quantos);
        assert_eq!(simbolos.len(), 2);
        assert_ne!(
            simbolos[0].arquivo, simbolos[1].arquivo,
            "arquivos diferentes"
        );
        let _ = fs::remove_file(&caminho);
    }

    /// Índice vazio abre e responde vazio, em vez de falhar.
    #[test]
    fn an_empty_index_opens() {
        let caminho = temporario("vazio");
        assert!(write(Vec::new(), &caminho).is_ok());
        let Some(aberto) = Aberto::open(&caminho) else {
            panic!("índice vazio precisa abrir");
        };
        assert_eq!(aberto.nomes(), 0);
        let _ = fs::remove_file(&caminho);
    }

    /// Arquivo que não é nosso, ou de outra versão, é recusado.
    ///
    /// Sem isto, um arquivo qualquer com o nome certo seria lido como índice e
    /// as respostas viriam de lixo, sem erro nenhum a apontar.
    #[test]
    fn a_file_that_is_not_ours_is_refused() {
        let caminho = temporario("intruso");
        assert!(fs::write(&caminho, b"nao sou um indice, mas tenho bytes o bastante para o cabecalho inteiro caber aqui dentro").is_ok());
        assert!(Aberto::open(&caminho).is_none());
        let _ = fs::remove_file(&caminho);
    }

    /// O índice mora fora do projeto.
    #[test]
    fn the_index_lives_outside_the_project() {
        let projeto = PathBuf::from("/algum/projeto");
        let Some(caminho) = caminho_do_indice(&projeto) else {
            panic!("precisa haver onde guardar o índice");
        };
        assert!(
            !caminho.starts_with(&projeto),
            "um arquivo nosso dentro do projeto entraria no controle de versão: {caminho:?}"
        );
    }
}
