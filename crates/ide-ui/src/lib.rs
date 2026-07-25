#![doc = "Shell visual da IDE baseado no ERLibUi."]

pub use ui_api::Widget;
pub use ui_components::{DockPanel, SplitPane, Tabs, TreeView};
pub use ui_editor::CodeEditor;

use ui_core::{Color, FontId, Point, Rect, Size};
use ui_render_api::{DrawTextCommand, FillRectCommand, PaintCommand, StrokeRectCommand};

#[derive(Clone, Debug)]
pub struct IdeShell {
    pub workspace_name: String,
    pub files: Vec<String>,
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub editor_lines: Vec<String>,
    pub terminal_lines: Vec<String>,
    pub search_query: String,
    pub status: String,
}

impl IdeShell {
    pub fn demo() -> Self {
        Self {
            workspace_name: "ide".to_owned(),
            files: vec![
                "▾ crates".to_owned(), "  ▾ ide-app".to_owned(), "      main.rs".to_owned(),
                "  ▸ ide-core".to_owned(), "  ▸ ide-ui".to_owned(), "  Cargo.toml".to_owned(),
            ],
            tabs: vec!["main.rs".to_owned(), "Cargo.toml".to_owned()],
            active_tab: 0,
            editor_lines: vec![
                "use ide_ui::IdeShell;".to_owned(), String::new(), "fn main() {".to_owned(),
                "    let shell = IdeShell::demo();".to_owned(), "    shell.run();".to_owned(), "}".to_owned(),
            ],
            terminal_lines: vec![
                "Terminal — PowerShell".to_owned(),
                "PS C:\\workspace\\ide> cargo check".to_owned(),
                "Finished dev profile".to_owned(),
            ],
            search_query: String::new(),
            status: "Rust  •  UTF-8  •  Ln 1, Col 1".to_owned(),
        }
    }

    pub fn paint(&self, size: Size) -> Vec<PaintCommand> {
        let sidebar = 260.0_f32.min(size.width * 0.32);
        let activity = 48.0;
        let title = 36.0;
        let tabs = 38.0;
        let status = 24.0;
        let terminal = 180.0_f32.min(size.height * 0.30);
        let content_top = title + tabs;
        let content_bottom = size.height - status;
        let editor_x = activity + sidebar;
        let editor_width = (size.width - editor_x).max(0.0);
        let editor_height = (content_bottom - content_top - terminal).max(0.0);
        let background = Color::rgba(0.055, 0.067, 0.09, 1.0);
        let surface = Color::rgba(0.075, 0.09, 0.12, 1.0);
        let elevated = Color::rgba(0.10, 0.12, 0.16, 1.0);
        let border = Color::rgba(0.18, 0.21, 0.28, 1.0);
        let text = Color::rgba(0.86, 0.89, 0.95, 1.0);
        let muted = Color::rgba(0.55, 0.60, 0.70, 1.0);
        let accent = Color::rgba(0.30, 0.55, 0.96, 1.0);
        let mut commands = vec![
            fill(Rect::new(0.0, 0.0, size.width, size.height), background),
            fill(Rect::new(0.0, 0.0, size.width, title), elevated),
            fill(Rect::new(0.0, title, activity, content_bottom - title), elevated),
            fill(Rect::new(activity, title, sidebar, content_bottom - title), surface),
            fill(Rect::new(editor_x, title, editor_width, tabs), elevated),
            fill(Rect::new(editor_x, content_top + editor_height, editor_width, terminal), surface),
            fill(Rect::new(0.0, content_bottom, size.width, status), accent),
            stroke(Rect::new(editor_x, content_top + editor_height, editor_width, terminal), border),
            label("ER IDE", Point::new(14.0, 9.0), text, 16.0),
            label("EXPLORER", Point::new(activity + 14.0, title + 14.0), muted, 12.0),
            label(&self.workspace_name, Point::new(activity + 14.0, title + 42.0), text, 14.0),
            label("⌕", Point::new(15.0, title + 18.0), text, 22.0),
            label("▣", Point::new(15.0, title + 62.0), text, 20.0),
        ];
        for (index, file) in self.files.iter().take(18).enumerate() {
            commands.push(label(file, Point::new(activity + 14.0, title + 70.0 + index as f32 * 23.0), text, 14.0));
        }
        let mut tab_x = editor_x;
        for (index, tab) in self.tabs.iter().enumerate() {
            if index == self.active_tab {
                commands.push(fill(Rect::new(tab_x, title, 140.0, tabs), background));
                commands.push(fill(Rect::new(tab_x, title, 140.0, 2.0), accent));
            }
            commands.push(label(tab, Point::new(tab_x + 14.0, title + 11.0), text, 14.0));
            tab_x += 140.0;
        }
        commands.push(PaintCommand::PushClip(Rect::new(editor_x, content_top, editor_width, editor_height)));
        for (index, line) in self.editor_lines.iter().take(40).enumerate() {
            let y = content_top + 15.0 + index as f32 * 22.0;
            commands.push(label(&(index + 1).to_string(), Point::new(editor_x + 12.0, y), muted, 13.0));
            commands.push(label(line, Point::new(editor_x + 55.0, y), syntax_color(line, text, accent, muted), 15.0));
        }
        commands.push(PaintCommand::PopClip);
        let terminal_y = content_top + editor_height;
        for (index, line) in self.terminal_lines.iter().take(7).enumerate() {
            commands.push(label(line, Point::new(editor_x + 14.0, terminal_y + 14.0 + index as f32 * 22.0), if index == 0 { text } else { muted }, 14.0));
        }
        if !self.search_query.is_empty() {
            let width = 380.0_f32.min((editor_width - 24.0).max(100.0));
            commands.push(fill(Rect::new(size.width - width - 12.0, content_top + 12.0, width, 42.0), elevated));
            commands.push(stroke(Rect::new(size.width - width - 12.0, content_top + 12.0, width, 42.0), accent));
            commands.push(label(&format!("Search: {}", self.search_query), Point::new(size.width - width, content_top + 24.0), text, 14.0));
        }
        commands.push(label(&self.status, Point::new(12.0, content_bottom + 5.0), Color::rgba(1.0, 1.0, 1.0, 1.0), 12.0));
        commands
    }
}

fn fill(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::FillRect(FillRectCommand { rect, color })
}
fn stroke(rect: Rect, color: Color) -> PaintCommand {
    PaintCommand::StrokeRect(StrokeRectCommand { rect, color, width: 1.0 })
}
fn label(text: &str, origin: Point, color: Color, size: f32) -> PaintCommand {
    PaintCommand::DrawText(DrawTextCommand { font_id: FontId(0), text: text.to_owned(), origin, color, size })
}
fn syntax_color(line: &str, plain: Color, keyword: Color, muted: Color) -> Color {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") { muted }
    else if ["use ", "fn ", "let ", "pub ", "struct ", "impl "].iter().any(|prefix| trimmed.starts_with(prefix)) { keyword }
    else { plain }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_paints_every_phase_one_region() {
        let commands = IdeShell::demo().paint(Size::new(1280.0, 800.0));
        let texts = commands.iter().filter_map(|command| match command {
            PaintCommand::DrawText(command) => Some(command.text.as_str()),
            _ => None,
        }).collect::<Vec<_>>();
        for expected in ["EXPLORER", "main.rs", "Terminal — PowerShell", "Rust  •  UTF-8  •  Ln 1, Col 1"] {
            assert!(texts.contains(&expected), "missing shell region: {expected}");
        }
    }
}
