//! Os consumidores do observador de arquivos. Ver a fase 4 da `22`.
//!
//! # Por que eles moram na raiz de composição
//!
//! O observador é **um só** — um registro no sistema operacional para a árvore
//! inteira —, e quem se registra nele são áreas que não se conhecem: o índice de
//! linguagem e o Git. O único lugar que conhece as duas é este.
//!
//! Cada consumidor traz o filtro dele, e os dois filtros estão certos: o índice
//! quer o que é fonte e **ignora `.git`**; o Git quer exatamente `.git/HEAD`,
//! `.git/index` e `.git/refs/` e ignora o resto. O erro que a `21` nomeou não é
//! ter dois filtros — é ter dois filtros para a mesma pergunta.
//!
//! # Nenhum deles reage na hora
//!
//! Os dois mandam recado por canal, e quem reage é o laço de quadros. Reagir
//! aqui seria reindexar e falar com o `git` na linha de execução do observador,
//! enquanto a rajada seguinte chega — e o resultado apareceria na tela sem
//! ninguém ter pedido um quadro.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use ide_watch::WatchConsumer;

/// O que o observador viu, já separado por quem se interessou.
pub(super) enum MudancaNoDisco {
    /// Arquivos de código que mudaram fora da IDE.
    Fontes(Vec<PathBuf>),
    /// O repositório mudou: `HEAD`, o índice ou as referências.
    Repositorio,
}

/// O consumidor do índice de linguagem.
///
/// Ele não sabe **quais** extensões interessam: quem sabe é o provider de cada
/// linguagem, e perguntar a ele por evento seria uma travessia de contrato para
/// cada arquivo de uma rajada de milhares. O filtro daqui é grosseiro de
/// propósito — o que não é `.git` e tem extensão —, e quem recusa de verdade é o
/// `file_changed` do provider, que já sabe o que fazer com um caminho que não é
/// dele.
pub(super) struct ConsumidorDeFontes {
    pub(super) aviso: Sender<MudancaNoDisco>,
}

impl WatchConsumer for ConsumidorDeFontes {
    fn interessa(&self, path: &Path) -> bool {
        !dentro_do_git(path) && path.extension().is_some()
    }

    fn mudou(&self, lote: &[PathBuf]) {
        let _ = self.aviso.send(MudancaNoDisco::Fontes(lote.to_vec()));
    }

    fn perdeu_eventos(&self) {
        // Sem a lista, não há o que reindexar por caminho: mandar um lote vazio
        // seria pedir trabalho sobre nada. Quem perde evento aqui volta ao
        // comportamento de antes do observador, e a próxima gravação corrige.
    }
}

/// O consumidor do Git.
///
/// **Ele quer exatamente o que o índice ignora.** `.git/HEAD` muda no
/// `checkout`, `.git/index` no `add`, e `refs/` no `fetch` e no `commit` — e
/// nenhum deles é arquivo de código.
pub(super) struct ConsumidorDeGit {
    pub(super) aviso: Sender<MudancaNoDisco>,
}

impl WatchConsumer for ConsumidorDeGit {
    fn interessa(&self, path: &Path) -> bool {
        if !dentro_do_git(path) {
            return false;
        }
        // O `.git` tem muito arquivo que não diz nada sobre o repositório: os
        // objetos, os logs, os arquivos temporários que o próprio `git` escreve
        // e apaga. Reagir a todos faria um `commit` disparar dezenas de
        // varreduras — e é justamente o `index.lock` que aparece e some no meio
        // de cada uma delas.
        let nome = path.file_name().and_then(|nome| nome.to_str());
        let em_refs = path
            .components()
            .any(|parte| parte.as_os_str() == "refs" || parte.as_os_str() == "MERGE_HEAD");
        matches!(nome, Some("HEAD" | "index" | "ORIG_HEAD" | "MERGE_HEAD")) || em_refs
    }

    fn mudou(&self, _lote: &[PathBuf]) {
        // **O lote não interessa, e a mensagem não o carrega.** O que mudou no
        // `.git` não diz o que mostrar; quem responde isso é o `status`, e ele é
        // pedido inteiro de qualquer jeito.
        let _ = self.aviso.send(MudancaNoDisco::Repositorio);
    }

    fn perdeu_eventos(&self) {
        // Aqui perder evento tem conserto barato: o retrato é uma pergunta só.
        let _ = self.aviso.send(MudancaNoDisco::Repositorio);
    }
}

/// Se o caminho está dentro da pasta do repositório.
fn dentro_do_git(path: &Path) -> bool {
    path.components().any(|parte| parte.as_os_str() == ".git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    /// Os dois filtros são complementares, e é isso que os torna um observador
    /// só.
    ///
    /// O que interessa a um não interessa ao outro: sem essa separação, o índice
    /// reindexaria a cada `git add` e o Git perguntaria o `status` a cada tecla
    /// gravada.
    #[test]
    fn cada_consumidor_olha_para_o_lado_dele() {
        let (envio, _recepcao) = channel();
        let fontes = ConsumidorDeFontes {
            aviso: envio.clone(),
        };
        let git = ConsumidorDeGit { aviso: envio };

        let codigo = PathBuf::from("/projeto/src/Pedido.java");
        let head = PathBuf::from("/projeto/.git/HEAD");
        let referencia = PathBuf::from("/projeto/.git/refs/heads/main");
        let objeto = PathBuf::from("/projeto/.git/objects/ab/cdef");

        assert!(fontes.interessa(&codigo) && !git.interessa(&codigo));
        assert!(git.interessa(&head) && !fontes.interessa(&head));
        assert!(git.interessa(&referencia) && !fontes.interessa(&referencia));
        assert!(
            !git.interessa(&objeto) && !fontes.interessa(&objeto),
            "objeto não diz nada sobre o repositório, e não é código"
        );
    }

    /// O aviso do Git não carrega caminho.
    ///
    /// O que mudou no `.git` não diz o que mostrar: quem responde é o `status`,
    /// e ele é pedido inteiro de qualquer jeito. Carregar a lista daria a
    /// impressão de que ela decide alguma coisa.
    #[test]
    fn o_aviso_do_git_nao_carrega_caminho() {
        let (envio, recepcao) = channel();
        let git = ConsumidorDeGit { aviso: envio };
        git.mudou(&[PathBuf::from("/projeto/.git/HEAD")]);
        assert!(matches!(
            recepcao.try_recv(),
            Ok(MudancaNoDisco::Repositorio)
        ));
    }
}
