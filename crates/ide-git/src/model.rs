//! Os tipos que as capacidades compartilham.

use std::path::PathBuf;

/// O nome de uma branch, como o Git o escreve: `main`, `feature/busca`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BranchName(pub String);

impl BranchName {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BranchName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// O nome de um remoto: `origin`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RemoteName(pub String);

/// O identificador de um commit.
///
/// Guarda o hash **inteiro**, e a tela é quem abrevia: quem copia um hash vai
/// colar num comando, e um hash abreviado guardado seria a abreviação virando a
/// única coisa que a IDE tem.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommitId(pub String);

impl CommitId {
    /// Os primeiros sete caracteres, que é como o Git mesmo abrevia.
    #[must_use]
    pub fn short(&self) -> &str {
        let fim = self.0.char_indices().nth(7).map_or(self.0.len(), |(i, _)| i);
        &self.0[..fim]
    }
}

/// Para onde `HEAD` aponta.
///
/// O segundo caso não é curiosidade: um `checkout` de commit deixa o
/// repositório assim, e uma tela que só soubesse mostrar nome de branch
/// mostraria vazio sem dizer por quê.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Head {
    /// Uma branch, que é o caso de sempre.
    Branch(BranchName),
    /// Um commit direto: `detached HEAD`.
    Detached(CommitId),
    /// Repositório sem nenhum commit ainda.
    Unborn(BranchName),
}

impl Head {
    /// O que a barra de estado mostra.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Branch(branch) | Self::Unborn(branch) => branch.0.clone(),
            Self::Detached(commit) => commit.short().to_owned(),
        }
    }
}

/// Em que estado um arquivo está.
///
/// São os três painéis da aba `status` do gerenciador, e a divisão não é da
/// tela: é a que o `--porcelain=v2` devolve, e a que decide o que `stage` e
/// `discard` fazem em cada linha.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileState {
    /// Está no índice, e entra no próximo commit.
    Staged,
    /// Mudou na árvore de trabalho, e não está preparado.
    Modified,
    /// O Git ainda não o conhece.
    Untracked,
    /// Tem conflito a resolver.
    Conflicted,
}

/// Um arquivo e o estado dele.
///
/// Um arquivo pode aparecer **duas vezes** — preparado e alterado ao mesmo
/// tempo é o que acontece quando se edita depois do `add`. São duas entradas de
/// propósito: cada painel mostra as suas, e juntá-las obrigaria a tela a decidir
/// em qual dos dois o arquivo aparece.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusEntry {
    pub path: PathBuf,
    pub state: FileState,
}

/// O retrato do repositório, calculado uma vez e lido por todos.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryStatus {
    pub head: Option<Head>,
    pub entries: Vec<StatusEntry>,
}

impl RepositoryStatus {
    /// Quantos arquivos estão em cada estado.
    #[must_use]
    pub fn count(&self, state: FileState) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == state)
            .count()
    }

    /// Quantos arquivos mudaram, contando cada um uma vez.
    ///
    /// É o número da barra de estado. Um arquivo preparado **e** alterado
    /// aparece duas vezes nas entradas, e contaria dois — o que diria que há
    /// mais trabalho do que há.
    #[must_use]
    pub fn changed_files(&self) -> usize {
        let mut vistos: Vec<&PathBuf> = self.entries.iter().map(|entry| &entry.path).collect();
        vistos.sort_unstable();
        vistos.dedup();
        vistos.len()
    }
}

/// De que lado a diferença é pedida.
///
/// São duas perguntas diferentes sobre o mesmo arquivo, e confundi-las mostra a
/// diferença errada para quem já preparou parte do trabalho.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffSide {
    /// O que mudou na árvore de trabalho e ainda não está preparado.
    WorkingTree,
    /// O que está preparado, contra o último commit.
    Index,
}

/// O que uma linha da diferença é.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    /// Igual dos dois lados: está ali para dar contexto.
    Context,
    Added,
    Removed,
}

/// O que a **margem do editor** mostra numa linha.
///
/// Não é o mesmo que [`DiffLineKind`], e a diferença é o motivo de os dois
/// existirem: o diff fala de linhas de um lado e do outro, e a margem fala do
/// arquivo que está na tela. Trocar uma linha por outra são duas linhas no
/// diff — uma removida e uma acrescentada — e **uma** marca na margem, porque na
/// tela é uma linha só.
///
/// # Por que são duas, e não três
///
/// Havia uma terceira, `Modified`, para a linha trocada. Ela dizia menos do que
/// as outras duas: quem olha a margem quer saber **onde há código novo para
/// reler**, e uma linha trocada tem código novo. Onde entrou código a marca é
/// verde, mesmo que algo tenha saído dali junto; a vermelha fica para o que só
/// perdeu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineChange {
    /// Recebeu código — tendo perdido algum ou não.
    Added,
    /// Só perdeu: a linha marcada é a que ficou no lugar do que saiu.
    Removed,
}

/// Uma linha da diferença.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
    /// A linha no arquivo de agora, contada a partir de zero.
    ///
    /// Ausente numa linha removida, porque ela **não existe** no arquivo de
    /// agora — e é isso que impede a margem de marcar uma linha que só existia
    /// antes, deslocando todas as marcas abaixo dela.
    pub new_line: Option<usize>,
}

/// Um trecho contíguo de diferença.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Hunk {
    pub old_start: usize,
    pub new_start: usize,
    pub lines: Vec<DiffLine>,
}

/// A diferença de um arquivo.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDiff {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// As linhas do arquivo **de então** que saíram.
    ///
    /// Contadas no lado esquerdo da comparação, e não no direito: uma linha
    /// removida não existe no arquivo de agora, e é justamente por isso que ela
    /// precisa de um número próprio — sem ele, não há onde marcá-la.
    #[must_use]
    pub fn removed_lines(&self) -> Vec<usize> {
        let mut linhas = Vec::new();
        for hunk in &self.hunks {
            let mut antiga = hunk.old_start;
            for linha in &hunk.lines {
                match linha.kind {
                    DiffLineKind::Removed => {
                        linhas.push(antiga);
                        antiga += 1;
                    }
                    // Contexto existe dos dois lados e anda no dois; acrescentada
                    // só existe no de agora, e não anda aqui.
                    DiffLineKind::Context => antiga += 1,
                    DiffLineKind::Added => {}
                }
            }
        }
        linhas
    }

    /// As linhas do arquivo de agora que mudaram, e como a margem as mostra.
    ///
    /// # As três regras, e o defeito que cada uma evita
    ///
    /// - **onde entrou código, a marca é verde** — tendo saído algo dali junto
    ///   ou não. Editar uma linha é o caso mais comum de todos, e ali o que
    ///   interessa é que há código novo para reler. Mas **linha que ficou em
    ///   branco não recebeu código**: apagar o conteúdo de uma linha é, no diff,
    ///   a antiga removida e uma vazia acrescentada, e verde ali diria "há algo
    ///   novo" sobre um vazio;
    /// - **remoção sem acréscimo marca a linha que ficou no lugar.** Ela não tem
    ///   linha própria no arquivo de agora: o que sobrou é a fronteira entre
    ///   duas linhas. Sem marcar nada, apagar um bloco não deixaria sinal, e
    ///   quem olha o arquivo não saberia que algo saiu dali;
    /// - **uma linha, uma marca.** Contar a remoção e o acréscimo de uma troca
    ///   como dois sinais encheria a margem onde houve uma alteração só.
    #[must_use]
    pub fn changed_lines(&self) -> Vec<(usize, LineChange)> {
        let mut marcas: Vec<(usize, LineChange)> = Vec::new();
        for hunk in &self.hunks {
            // Quantas remoções ainda esperam por um acréscimo que as substitua.
            let mut removidas = 0usize;
            let mut proxima = hunk.new_start;
            let fechar_remocao = |marcas: &mut Vec<(usize, LineChange)>,
                                      removidas: &mut usize,
                                      linha: usize| {
                if *removidas > 0 {
                    marcas.push((linha, LineChange::Removed));
                    *removidas = 0;
                }
            };
            for linha in &hunk.lines {
                match linha.kind {
                    DiffLineKind::Removed => removidas += 1,
                    DiffLineKind::Added => {
                        let numero = linha.new_line.unwrap_or(proxima);
                        proxima = numero + 1;
                        // **Linha que ficou em branco não recebeu código.**
                        // Apagar o conteúdo de uma linha é, no diff, a linha
                        // antiga removida e uma vazia acrescentada; contá-la
                        // como acréscimo pintava de verde uma linha onde só se
                        // perdeu — e verde ali diz "há algo novo para reler"
                        // sobre um vazio.
                        let vazia = linha.text.trim().is_empty();
                        let substitui = removidas > 0;
                        removidas = removidas.saturating_sub(1);
                        // Vazia **e** substituindo alguma coisa é perda. Vazia
                        // sem substituir nada é uma linha em branco que alguém
                        // acrescentou, e essa é nova como qualquer outra.
                        marcas.push((
                            numero,
                            if vazia && substitui {
                                LineChange::Removed
                            } else {
                                LineChange::Added
                            },
                        ));
                    }
                    DiffLineKind::Context => {
                        let numero = linha.new_line.unwrap_or(proxima);
                        proxima = numero + 1;
                        fechar_remocao(&mut marcas, &mut removidas, numero);
                    }
                }
            }
            // Remoção no fim do trecho: a linha que ficou no lugar é a seguinte.
            fechar_remocao(&mut marcas, &mut removidas, proxima);
        }
        marcas.sort_unstable_by_key(|(linha, _)| *linha);
        marcas.dedup_by_key(|(linha, _)| *linha);
        marcas
    }
}

/// Que operação está no meio do caminho.
///
/// As três param entre commits e precisam de continuar, abortar ou pular. A
/// fase 3 só produz a primeira, e as outras duas existem aqui porque **quem
/// rodou `rebase` no terminal integrado deixou o repositório assim** — e a IDE
/// precisa dizer isso em vez de mostrar uma tela que não corresponde ao disco.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingOperation {
    Merge,
    Rebase,
    CherryPick,
}

impl PendingOperation {
    /// Como ela se chama na tela.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Merge => "Merge",
            Self::Rebase => "Rebase",
            Self::CherryPick => "Cherry-pick",
        }
    }
}

/// O que uma fusão produziu.
///
/// **Conflito não é erro.** É o resultado esperado de fundir dois trabalhos que
/// tocaram a mesma linha, e tratá-lo como falha faria a IDE dizer que algo deu
/// errado quando o que houve foi trabalho a fazer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MergeOutcome {
    /// Fundiu, e já commitou.
    Merged,
    /// Já estava contido: não havia o que trazer.
    AlreadyUpToDate,
    /// Parou com conflitos, e estes são os arquivos.
    Conflicted { paths: Vec<PathBuf> },
}

/// Um item guardado no `stash`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StashEntry {
    /// A posição na pilha: zero é o mais recente.
    pub index: usize,
    /// A descrição que o `git` dá a ele.
    pub message: String,
}

/// Um commit, como a tabela do histórico o mostra.
///
/// **A data vem pronta, e não como número.** Formatá-la exigiria fuso e
/// calendário aqui dentro; o `git` já sabe fazer isso, e o formato curto é o
/// mesmo em qualquer máquina porque quem pede diz qual quer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitSummary {
    pub id: CommitId,
    /// A primeira linha da mensagem, que é o que a coluna mostra.
    pub summary: String,
    pub author: String,
    /// Data local, já formatada como `2026-08-06 19:14`.
    pub date: String,
    /// Os pais deste commit.
    ///
    /// Dois pais é uma fusão, e é disso que o grafo é feito: sem eles não há
    /// como saber de onde para onde vai cada traço.
    pub parents: Vec<CommitId>,
}

/// Onde um commit fica no grafo, e o que sai dele.
///
/// **A conta é da IDE, e o traço é da biblioteca.** Qual faixa cada commit
/// ocupa sai do histórico — é aritmética sobre pais e filhos —, e isso não é
/// desenho. Ver a `22`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GraphRow {
    /// Em que faixa o ponto deste commit fica, contada da esquerda.
    pub lane: usize,
    /// Quantas faixas estão ocupadas nesta linha.
    ///
    /// É a largura que o desenho precisa reservar: sem ela, a coluna do grafo
    /// teria de ser medida linha a linha.
    pub width: usize,
    /// As faixas que atravessam esta linha sem parar nela.
    pub passing: Vec<usize>,
    /// Para que faixas os pais deste commit seguem.
    pub parents: Vec<usize>,
}

/// Reparte os commits em faixas, na ordem em que a tabela os mostra.
///
/// O algoritmo é o mesmo de todo visualizador de histórico, e é simples de
/// propósito: cada linha tem uma lista de faixas esperando por um commit, o
/// commit ocupa a primeira que o espera — ou uma nova, se ninguém o esperava —,
/// e os pais dele passam a ser esperados.
///
/// **Uma página é uma página.** Um pai que está na página seguinte deixa a faixa
/// aberta, e é o que se quer: o traço sai pela borda de baixo, como sai numa
/// tela que continua rolando.
#[must_use]
pub fn graph_rows(commits: &[CommitSummary]) -> Vec<GraphRow> {
    // Cada posição é uma faixa, e o que está nela é o commit esperado ali.
    let mut faixas: Vec<Option<CommitId>> = Vec::new();
    let mut linhas = Vec::with_capacity(commits.len());
    for commit in commits {
        let minha = match faixas.iter().position(|faixa| faixa.as_ref() == Some(&commit.id)) {
            Some(indice) => indice,
            None => {
                let vaga = faixas.iter().position(Option::is_none);
                match vaga {
                    Some(indice) => indice,
                    None => {
                        faixas.push(None);
                        faixas.len() - 1
                    }
                }
            }
        };
        // **Todas as faixas que esperavam este commit convergem para a dele.**
        // Duas linhas que vêm do mesmo pai o esperam em faixas diferentes; sem
        // soltar as outras, a faixa fica esperando para sempre um commit que já
        // passou, e a largura do grafo nunca mais desce.
        for faixa in &mut faixas {
            if faixa.as_ref() == Some(&commit.id) {
                *faixa = None;
            }
        }
        // As que atravessam: ocupadas por outro commit, e não por este.
        let passing = faixas
            .iter()
            .enumerate()
            .filter(|(indice, faixa)| *indice != minha && faixa.is_some())
            .map(|(indice, _)| indice)
            .collect();
        // O primeiro pai continua na faixa deste commit; os outros abrem faixa.
        faixas[minha] = commit.parents.first().cloned();
        let mut destinos = Vec::new();
        if !commit.parents.is_empty() {
            destinos.push(minha);
        }
        for pai in commit.parents.iter().skip(1) {
            let existente = faixas.iter().position(|faixa| faixa.as_ref() == Some(pai));
            let indice = match existente {
                Some(indice) => indice,
                None => match faixas.iter().position(Option::is_none) {
                    Some(indice) => {
                        faixas[indice] = Some(pai.clone());
                        indice
                    }
                    None => {
                        faixas.push(Some(pai.clone()));
                        faixas.len() - 1
                    }
                },
            };
            destinos.push(indice);
        }
        // Faixas que ninguém mais espera saem do fim, para a largura não crescer
        // para sempre num histórico longo.
        while faixas.last().is_some_and(Option::is_none) {
            faixas.pop();
        }
        linhas.push(GraphRow {
            lane: minha,
            width: faixas.len().max(minha + 1),
            passing,
            parents: destinos,
        });
    }
    linhas
}


/// Um trecho de uma linha, em caracteres.
///
/// Contado em **caracteres**, e não em bytes: quem vai desenhar mede texto, e
/// medir meio caractere de um acento não devolve posição nenhuma.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineSpan {
    /// A linha, contada a partir de zero, no arquivo a que este trecho pertence.
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

impl FileDiff {
    /// Os trechos **acrescentados**, no arquivo de agora.
    #[must_use]
    pub fn added_spans(&self) -> Vec<LineSpan> {
        self.trechos(true)
    }

    /// Os trechos **removidos**, no arquivo de então.
    #[must_use]
    pub fn removed_spans(&self) -> Vec<LineSpan> {
        self.trechos(false)
    }

    /// Os trechos que mudaram dentro das linhas.
    ///
    /// # Por que isto não é a linha inteira
    ///
    /// Trocar uma palavra numa linha de oitenta colunas pinta as oitenta se o
    /// destaque for da linha — e quem olha tem de procurar o que mudou dentro do
    /// que foi marcado como mudado. O que se quer ver é a palavra.
    ///
    /// # Como as duas listas se emparelham
    ///
    /// Um trecho de remoções seguido de um de acréscimos é uma **troca**, e as
    /// linhas se emparelham pela ordem: a primeira removida com a primeira
    /// acrescentada. De cada par sai o que sobra depois de tirar o começo e o
    /// fim iguais.
    ///
    /// Sem par — mais removidas do que acrescentadas, ou o contrário — a linha
    /// inteira é o trecho: ela não foi trocada, ela entrou ou saiu.
    fn trechos(&self, acrescentados: bool) -> Vec<LineSpan> {
        let mut trechos = Vec::new();
        for hunk in &self.hunks {
            let mut antiga = hunk.old_start;
            // Cada corrida guarda (linha, texto) dos dois lados.
            let mut removidas: Vec<(usize, &str)> = Vec::new();
            let mut adicionadas: Vec<(usize, &str)> = Vec::new();
            let fechar = |removidas: &mut Vec<(usize, &str)>,
                              adicionadas: &mut Vec<(usize, &str)>,
                              trechos: &mut Vec<LineSpan>| {
                emparelhar(removidas, adicionadas, acrescentados, trechos);
                removidas.clear();
                adicionadas.clear();
            };
            for linha in &hunk.lines {
                match linha.kind {
                    DiffLineKind::Removed => {
                        removidas.push((antiga, linha.text.as_str()));
                        antiga += 1;
                    }
                    DiffLineKind::Added => {
                        adicionadas.push((linha.new_line.unwrap_or_default(), linha.text.as_str()));
                    }
                    DiffLineKind::Context => {
                        fechar(&mut removidas, &mut adicionadas, &mut trechos);
                        antiga += 1;
                    }
                }
            }
            fechar(&mut removidas, &mut adicionadas, &mut trechos);
        }
        trechos
    }
}

/// Empareja as linhas de uma troca e devolve o que difere em cada uma.
fn emparelhar(
    removidas: &[(usize, &str)],
    adicionadas: &[(usize, &str)],
    acrescentados: bool,
    trechos: &mut Vec<LineSpan>,
) {
    let meu = if acrescentados { adicionadas } else { removidas };
    let outro = if acrescentados { removidas } else { adicionadas };
    for (indice, (linha, texto)) in meu.iter().enumerate() {
        let comprimento = texto.chars().count();
        let Some((_, par)) = outro.get(indice) else {
            // Sem par: a linha inteira entrou ou saiu.
            if comprimento > 0 {
                trechos.push(LineSpan {
                    line: *linha,
                    start: 0,
                    end: comprimento,
                });
            }
            continue;
        };
        let (inicio, fim) = diferenca(texto, par);
        if fim > inicio {
            trechos.push(LineSpan {
                line: *linha,
                start: inicio,
                end: fim,
            });
        }
    }
}

/// Onde dois textos passam a diferir, e onde voltam a coincidir.
///
/// O começo igual e o fim igual saem fora; o que sobra é o que mudou. As duas
/// pontas são contadas em caracteres do **primeiro** texto, que é o que vai ser
/// marcado.
fn diferenca(meu: &str, outro: &str) -> (usize, usize) {
    let meus: Vec<char> = meu.chars().collect();
    let outros: Vec<char> = outro.chars().collect();
    let mut inicio = 0;
    while inicio < meus.len() && inicio < outros.len() && meus[inicio] == outros[inicio] {
        inicio += 1;
    }
    let mut fim_meu = meus.len();
    let mut fim_outro = outros.len();
    while fim_meu > inicio && fim_outro > inicio && meus[fim_meu - 1] == outros[fim_outro - 1] {
        fim_meu -= 1;
        fim_outro -= 1;
    }
    (inicio, fim_meu)
}

/// Uma branch, como o painel da esquerda a mostra.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchSummary {
    pub name: BranchName,
    /// Se é para ela que `HEAD` aponta.
    pub current: bool,
    /// O upstream configurado, quando há.
    pub upstream: Option<BranchName>,
    /// Quantos commits ela tem a mais e a menos que o upstream.
    ///
    /// **Vem do que já foi buscado, e não do remoto.** Sem `fetch`, ela é a
    /// contagem contra o que se sabia da última vez — e é isso que a IDE tem
    /// para dizer. Prometer o número de agora exigiria falar com a rede a cada
    /// retrato.
    pub ahead: usize,
    pub behind: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, pais: &[&str]) -> CommitSummary {
        CommitSummary {
            id: CommitId(id.to_owned()),
            summary: format!("commit {id}"),
            author: "Teste".to_owned(),
            date: "2026-08-06 19:14".to_owned(),
            parents: pais.iter().map(|pai| CommitId((*pai).to_owned())).collect(),
        }
    }


    /// Trocar uma palavra marca a palavra, e não a linha inteira.
    ///
    /// Numa linha de oitenta colunas, marcar tudo obriga quem olha a procurar o
    /// que mudou dentro do que foi marcado como mudado.
    #[test]
    fn a_troca_de_uma_palavra_marca_so_a_palavra() {
        let diff = FileDiff {
            path: PathBuf::from("Pedido.java"),
            hunks: vec![Hunk {
                old_start: 0,
                new_start: 0,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Removed,
                        text: "    int total = 10;".to_owned(),
                        new_line: None,
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "    int total = 42;".to_owned(),
                        new_line: Some(0),
                    },
                ],
            }],
        };
        assert_eq!(
            diff.added_spans(),
            vec![LineSpan {
                line: 0,
                start: 16,
                end: 18
            }],
            "só o `42`"
        );
        assert_eq!(
            diff.removed_spans(),
            vec![LineSpan {
                line: 0,
                start: 16,
                end: 18
            }],
            "e só o `10` do outro lado"
        );
    }

    /// Linha que entrou ou saiu sem par é marcada inteira.
    ///
    /// Ela não foi trocada por nada: procurar um trecho ali seria comparar com
    /// uma linha que não existe.
    #[test]
    fn a_linha_sem_par_e_marcada_inteira() {
        let diff = FileDiff {
            path: PathBuf::from("a.txt"),
            hunks: vec![Hunk {
                old_start: 0,
                new_start: 0,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        text: "um".to_owned(),
                        new_line: Some(0),
                    },
                    DiffLine {
                        kind: DiffLineKind::Added,
                        text: "dois".to_owned(),
                        new_line: Some(1),
                    },
                ],
            }],
        };
        assert_eq!(
            diff.added_spans(),
            vec![LineSpan {
                line: 1,
                start: 0,
                end: 4
            }]
        );
        assert!(diff.removed_spans().is_empty());
    }

    /// Uma linha reta ocupa uma faixa só, do começo ao fim.
    ///
    /// É o caso de quase todo repositório, e é o que impede a coluna do grafo de
    /// reservar largura que ninguém usa.
    #[test]
    fn um_historico_sem_fusao_cabe_numa_faixa() {
        let commits = [commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])];
        let linhas = graph_rows(&commits);
        assert!(linhas.iter().all(|linha| linha.lane == 0));
        assert!(linhas.iter().all(|linha| linha.width == 1), "{linhas:?}");
        // O último não tem pai, e por isso não deixa faixa aberta.
        assert!(linhas[2].parents.is_empty());
    }

    /// Uma fusão abre a segunda faixa, e ela fecha quando os dois lados se
    /// encontram.
    ///
    /// **A largura precisa voltar a um.** Sem soltar a faixa que ninguém mais
    /// espera, um histórico longo empurraria a coluna do grafo até engolir a
    /// descrição.
    #[test]
    fn uma_fusao_abre_uma_faixa_e_o_encontro_a_fecha() {
        // m funde d (linha de cima) com b (linha de baixo); os dois vêm de a.
        let commits = [
            commit("m", &["d", "b"]),
            commit("d", &["a"]),
            commit("b", &["a"]),
            commit("a", &[]),
        ];
        let linhas = graph_rows(&commits);
        assert_eq!(linhas[0].lane, 0);
        assert_eq!(
            linhas[0].parents,
            vec![0, 1],
            "o primeiro pai segue na faixa, o segundo abre outra"
        );
        assert_eq!(linhas[0].width, 2);
        assert_eq!(linhas[1].lane, 0, "d continua na faixa da esquerda");
        assert_eq!(linhas[2].lane, 1, "b está na faixa que a fusão abriu");
        assert_eq!(
            linhas[3].width, 1,
            "com os dois lados no mesmo pai, a segunda faixa fecha: {linhas:?}"
        );
    }

    /// O que atravessa a linha sem parar nela é o que o traço precisa saber.
    ///
    /// Sem essa lista, a faixa da direita sumiria na altura de um commit da
    /// esquerda, e o traço apareceria cortado.
    #[test]
    fn as_faixas_que_atravessam_ficam_registradas() {
        let commits = [
            commit("m", &["d", "b"]),
            commit("d", &["a"]),
            commit("b", &["a"]),
            commit("a", &[]),
        ];
        let linhas = graph_rows(&commits);
        assert_eq!(
            linhas[1].passing,
            vec![1],
            "na altura de d, a faixa de b atravessa: {linhas:?}"
        );
    }
}
