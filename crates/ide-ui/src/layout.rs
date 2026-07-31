//! As faixas da moldura, como quem as lê precisa vê-las.
//!
//! Aqui não se calcula nada: a moldura é declarada ao anfitrião e lida do
//! arranjo. O que sobrou é a forma que os leitores usam e a lista dos passos de
//! depuração, que é vocabulário do domínio e não geometria.

use ide_application::DebugRequest;
use ui_core::Rect;

pub(super) struct Geometry {
    pub(super) content_top: f32,
    pub(super) content_bottom: f32,
    pub(super) editor_bottom: f32,
    pub(super) editor_width: f32,
    pub(super) editor_height: f32,
    pub(super) terminal_height: f32,
}



pub(super) const DEBUG_BUTTONS: [(&str, DebugRequest); 5] = [
    ("Cont.", DebugRequest::Continue),
    ("Sobre", DebugRequest::StepOver),
    ("Entrar", DebugRequest::StepInto),
    ("Sair", DebugRequest::StepOut),
    ("Fim", DebugRequest::Detach),
];

pub(super) struct DebugPanelGeometry {
    pub(super) panel: Rect,
    pub(super) buttons: Vec<Rect>,
    pub(super) frames: Rect,
    pub(super) variables: Rect,
}

