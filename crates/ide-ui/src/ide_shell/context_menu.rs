//! O menu de contexto: uma superfície, como as outras. Ver a `14`.
//!
//! # Por que ele saiu do Explorer
//!
//! Existe **um** menu de contexto na IDE, e ele servia a três áreas — a árvore
//! do projeto, o texto do editor e a faixa de abas —, mas morava dentro do
//! estado do Explorer, com um campo por alvo ao lado dele: a pasta, o arquivo, a
//! aba. Ele nasceu ali porque, quando nasceu, **era** do Explorer.
//!
//! O que isso cobrava:
//!
//! - o despacho se chamava `run_explorer_command` e tratava `editor.copy`,
//!   `editor.paste` e `editor.split.right`. Quem procurava onde o Copiar era
//!   tratado não olhava no Explorer;
//! - o estado da árvore passou a carregar `DocumentId`, que é identidade de
//!   outra área;
//! - cada alvo novo acrescentava um campo, e os três eram **mutuamente
//!   exclusivos** — a assinatura de um `enum` escrito como campos soltos;
//! - fechar o menu zerava dois dos três, e a diferença não tinha motivo.
//!
//! Aqui o alvo é um só, e ele **é** o enum. Fechar o menu apaga o alvo, seja
//! qual for — não há como esquecer um.

use super::*;

/// De quem o menu aberto está falando.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AlvoDoMenu {
    /// Um item da árvore do projeto.
    ///
    /// A **pasta** é onde a criação acontece — clicando num arquivo, é a pasta
    /// dele. O arquivo vem junto quando foi um arquivo o clicado, porque
    /// renomear fala dele, e não da pasta.
    Explorer {
        pasta: PathBuf,
        arquivo: Option<PathBuf>,
    },
    /// Uma aba do editor.
    Aba(DocumentId),
    /// O texto do editor: as ações daqui falam do cursor e da seleção, e por
    /// isso não carregam alvo nenhum.
    Editor,
}

/// O menu de contexto e o alvo de que ele fala.
pub(super) struct ContextMenuSurface {
    pub(super) menu: ContextMenu,
    pub(super) alvo: Option<AlvoDoMenu>,
}

impl Default for ContextMenuSurface {
    fn default() -> Self {
        Self {
            menu: ContextMenu::new(EXPLORER_CONTEXT_MENU_ID, Vec::new()),
            alvo: None,
        }
    }
}

impl ContextMenuSurface {
    pub(super) fn is_open(&self) -> bool {
        self.menu.is_open()
    }

    /// Fecha e esquece o alvo. **Os dois juntos, sempre**: um menu fechado que
    /// lembra de quem falava é a próxima ação agindo sobre o alvo anterior.
    pub(super) fn close(&mut self) {
        self.menu.close();
        self.alvo = None;
    }

    /// A pasta em que criar, quando o menu aberto é o do Explorer.
    pub(super) fn pasta(&self) -> Option<&PathBuf> {
        match &self.alvo {
            Some(AlvoDoMenu::Explorer { pasta, .. }) => Some(pasta),
            _ => None,
        }
    }

    /// O arquivo clicado, quando foi um arquivo.
    pub(super) fn arquivo(&self) -> Option<&PathBuf> {
        match &self.alvo {
            Some(AlvoDoMenu::Explorer { arquivo, .. }) => arquivo.as_ref(),
            _ => None,
        }
    }

    /// A aba clicada, retirada do alvo: quem a consome age uma vez só.
    pub(super) fn take_aba(&mut self) -> Option<DocumentId> {
        match self.alvo.take() {
            Some(AlvoDoMenu::Aba(documento)) => Some(documento),
            outro => {
                self.alvo = outro;
                None
            }
        }
    }
}
