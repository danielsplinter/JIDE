//! O estado dos arquivos e do índice.

use async_trait::async_trait;
use ide_domain::CancellationToken;

use std::path::{Path, PathBuf};

use crate::error::GitResult;
use crate::model::{DiffSide, FileDiff, RepositoryStatus};

/// O que a árvore de trabalho responde.
///
/// **Leitura recebe token; escrita não.** Cancelar uma leitura é jogar fora uma
/// resposta que ninguém quer mais — trocar de arquivo antes do `diff` chegar.
/// Cancelar uma escrita pela metade deixaria o repositório num estado que
/// ninguém pediu, e preparar três arquivos é rápido demais para valer o risco.
#[async_trait]
pub trait WorkingTreeService: Send + Sync {
    /// O retrato do repositório: para onde `HEAD` aponta e o que mudou.
    async fn status(&self, cancel: &CancellationToken) -> GitResult<RepositoryStatus>;

    /// O que mudou num arquivo, de um lado ou do outro.
    async fn diff(
        &self,
        path: &Path,
        side: DiffSide,
        cancel: &CancellationToken,
    ) -> GitResult<FileDiff>;

    /// O conteúdo do arquivo no último commit.
    ///
    /// É o lado esquerdo da comparação. Vem como texto e não como caminho: o
    /// arquivo de então **não existe** no disco, e materializá-lo num temporário
    /// daria a quem abrisse uma cópia editável do passado.
    async fn committed_text(&self, path: &Path, cancel: &CancellationToken) -> GitResult<String>;

    /// Põe os caminhos no índice.
    async fn stage(&self, paths: &[PathBuf]) -> GitResult<()>;

    /// Tira os caminhos do índice, sem tocar no arquivo.
    async fn unstage(&self, paths: &[PathBuf]) -> GitResult<()>;

    /// Destrutivo: joga fora alteração não commitada, sem rede de recuperação.
    ///
    /// **Só alcança o que o Git rastreia.** Arquivo não rastreado não é
    /// descartado — seria apagá-lo do disco, e não há de onde trazê-lo de volta.
    /// Quem quiser apagá-lo apaga pelo Explorer, onde apagar é o que se espera.
    async fn discard(&self, paths: &[PathBuf]) -> GitResult<()>;
}
