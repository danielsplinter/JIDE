//! Áreas dos painéis que ainda não são arranjados pelo motor.
//!
//! A moldura saiu daqui: ela é declarada ao anfitrião e lida do arranjo. O que
//! resta é o interior do painel de depuração e a fileira de ações do título.

use ide_application::DebugRequest;
use ui_core::{Rect, Size};

use crate::ide_shell::{DEBUG_ROW_HEIGHT, TITLE_HEIGHT};

pub(super) struct Geometry {
    pub(super) content_top: f32,
    pub(super) content_bottom: f32,
    pub(super) editor_bottom: f32,
    pub(super) editor_width: f32,
    pub(super) editor_height: f32,
    pub(super) terminal_height: f32,
}


pub(super) fn action_button_rects(size: Size) -> [Rect; 3] {
    const SIDE: f32 = 28.0;
    const GAP: f32 = 2.0;
    let top = (TITLE_HEIGHT - SIDE) / 2.0;
    let first = (size.width - 10.0 - SIDE * 3.0 - GAP * 2.0).max(0.0);
    [0.0, 1.0, 2.0].map(|index| Rect::new(first + index * (SIDE + GAP), top, SIDE, SIDE))
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

pub(super) fn debug_panel_geometry(panel: Rect, frame_count: usize) -> DebugPanelGeometry {
    let button_width = (panel.size.width - 20.0) / DEBUG_BUTTONS.len() as f32;
    let buttons = (0..DEBUG_BUTTONS.len())
        .map(|index| {
            Rect::new(
                panel.origin.x + 10.0 + index as f32 * button_width,
                panel.origin.y + 34.0,
                button_width - 4.0,
                26.0,
            )
        })
        .collect();
    let frames_top = panel.origin.y + 86.0;
    let visible_frames = frame_count.clamp(1, 8) as f32;
    let frames_height = visible_frames * DEBUG_ROW_HEIGHT;
    let list_x = panel.origin.x + 6.0;
    let list_width = (panel.size.width - 12.0).max(0.0);
    let variables_top = frames_top + frames_height + 30.0;
    DebugPanelGeometry {
        panel,
        buttons,
        frames: Rect::new(list_x, frames_top, list_width, frames_height),
        variables: Rect::new(
            list_x,
            variables_top,
            list_width,
            (panel.origin.y + panel.size.height - variables_top).max(0.0),
        ),
    }
}
