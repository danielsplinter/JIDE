//! O repositório aberto, e as capacidades que ele entrega.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::branches::BranchService;
use crate::history::HistoryService;
use crate::integration::IntegrationService;
use crate::tags::TagService;
use crate::working_tree::WorkingTreeService;

/// Um repositório aberto.
///
/// Ele **entrega** serviços em vez de fazer tudo: não existe um `GitService` com
/// o Git inteiro dentro. Quem mostra a lista de alterações depende de
/// [`WorkingTreeService`] e não recompila quando o `rebase` mudar.
///
/// A construção é do `lib.rs`, e é o único ponto onde o adapter é nomeado —
/// daqui para fora ninguém sabe que existe linha de comando.
#[derive(Clone)]
pub struct Repository {
    root: PathBuf,
    working_tree: Arc<dyn WorkingTreeService>,
    branches: Arc<dyn BranchService>,
    history: Arc<dyn HistoryService>,
    integration: Arc<dyn IntegrationService>,
    tags: Arc<dyn TagService>,
}

impl Repository {
    pub(crate) fn new(
        root: PathBuf,
        working_tree: Arc<dyn WorkingTreeService>,
        branches: Arc<dyn BranchService>,
        history: Arc<dyn HistoryService>,
        integration: Arc<dyn IntegrationService>,
        tags: Arc<dyn TagService>,
    ) -> Self {
        Self {
            root,
            working_tree,
            branches,
            history,
            integration,
            tags,
        }
    }

    /// Onde este repositório começa.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn working_tree(&self) -> Arc<dyn WorkingTreeService> {
        Arc::clone(&self.working_tree)
    }

    #[must_use]
    pub fn branches(&self) -> Arc<dyn BranchService> {
        Arc::clone(&self.branches)
    }

    #[must_use]
    pub fn history(&self) -> Arc<dyn HistoryService> {
        Arc::clone(&self.history)
    }

    #[must_use]
    pub fn integration(&self) -> Arc<dyn IntegrationService> {
        Arc::clone(&self.integration)
    }

    #[must_use]
    pub fn tags(&self) -> Arc<dyn TagService> {
        Arc::clone(&self.tags)
    }
}

impl std::fmt::Debug for Repository {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repository").field("root", &self.root).finish()
    }
}
