use ide_core::init_logging;
use ide_ui::{IdeShell, NavigationRequest};
use ui_core::{Point, Size, WindowId};
use ui_render_api::{FrameInfo, UiRenderer};
use ui_render_wgpu::WgpuRenderer;
use ui_window_api::WindowRequest;
use ui_window_winit::WinitWindow;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{CursorIcon, WindowId as WinitWindowId},
};

#[derive(Default)]
struct NativeIde {
    window: Option<WinitWindow>,
    renderer: Option<WgpuRenderer>,
    shell: Option<IdeShell>,
    startup_error: Option<String>,
    cursor: Point,
    control_pressed: bool,
    navigation_requests: Vec<NavigationRequest>,
}

impl NativeIde {
    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let request = WindowRequest {
            title: "ER IDE — Rust Native IDE".to_owned(),
            logical_size: Size::new(1280.0, 800.0),
        };
        let window = WinitWindow::create_hidden(event_loop, WindowId(1), &request)
            .map_err(|error| error.to_string())?;
        let renderer = pollster::block_on(WgpuRenderer::new(window.inner().clone()))
            .map_err(|error| error.to_string())?;
        let root = std::env::current_dir().map_err(|error| error.to_string())?;
        self.shell = Some(IdeShell::open(&root).map_err(|error| error.to_string())?);
        self.renderer = Some(renderer);
        window.show();
        self.window = Some(window);
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let window = self
            .window
            .as_ref()
            .ok_or_else(|| "window unavailable".to_owned())?;
        let size = window.logical_size();
        let shell = self
            .shell
            .as_mut()
            .ok_or_else(|| "shell unavailable".to_owned())?;
        let commands = shell.paint(size);
        let renderer = self
            .renderer
            .as_mut()
            .ok_or_else(|| "renderer unavailable".to_owned())?;
        renderer
            .begin_frame(FrameInfo {
                window_id: window.handle().id,
                logical_size: size,
                scale_factor: window.scale_factor(),
            })
            .map_err(|error| error.to_string())?;
        renderer
            .submit(&commands)
            .map_err(|error| error.to_string())?;
        renderer.end_frame().map_err(|error| error.to_string())
    }

    fn choose_project(&mut self) {
        let Some(current) = self
            .shell
            .as_ref()
            .map(|shell| shell.workspace_path().to_path_buf())
        else {
            return;
        };
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Abrir projeto")
            .set_directory(current)
            .pick_folder()
        else {
            return;
        };
        match IdeShell::open(&folder) {
            Ok(shell) => {
                self.shell = Some(shell);
                if let Some(window) = self.window.as_ref() {
                    window
                        .inner()
                        .set_title(&format!("ER IDE — {}", folder.display()));
                    window.request_redraw();
                }
            }
            Err(error) => {
                self.startup_error = Some(format!(
                    "failed to open project {}: {error}",
                    folder.display()
                ));
            }
        }
    }
}

impl ApplicationHandler for NativeIde {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(30),
        ));
        if let (Some(window), Some(shell)) = (self.window.as_ref(), self.shell.as_mut())
            && shell.update_terminals(window.logical_size())
        {
            window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(error) = self.initialize(event_loop)
        {
            self.startup_error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WinitWindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if window.inner().id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner().inner_size();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let logical = position.to_logical::<f32>(window.scale_factor());
                self.cursor = Point::new(logical.x, logical.y);
                let redraw = self
                    .shell
                    .as_mut()
                    .is_some_and(|shell| shell.pointer_move(self.cursor, window.logical_size()));
                let resizing = self.shell.as_ref().is_some_and(IdeShell::terminal_resizing);
                let sidebar_resizing = self.shell.as_ref().is_some_and(IdeShell::sidebar_resizing);
                window.inner().set_cursor(if sidebar_resizing {
                    CursorIcon::EwResize
                } else if resizing {
                    CursorIcon::NsResize
                } else {
                    CursorIcon::Default
                });
                if redraw {
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.pointer_down_with_modifiers(
                        self.cursor,
                        window.logical_size(),
                        self.control_pressed,
                    );
                    if let Some(request) = shell.take_navigation_request() {
                        tracing::info!(
                            token = request.token,
                            byte_offset = request.byte_offset,
                            "definition navigation requested"
                        );
                        self.navigation_requests.push(request);
                    }
                }
                let open_project = self
                    .shell
                    .as_mut()
                    .is_some_and(IdeShell::take_open_project_request);
                window.request_redraw();
                if open_project {
                    self.choose_project();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell.pointer_up();
                }
                window.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y.round() as isize * 3,
                    MouseScrollDelta::PixelDelta(position) => -(position.y / 22.0).round() as isize,
                };
                if let Some(shell) = self.shell.as_mut() {
                    shell.scroll(self.cursor, lines, window.logical_size());
                }
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.control_pressed = modifiers.state().control_key();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.escape();
                        }
                    }
                    Key::Named(NamedKey::F3) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.toggle_search();
                        }
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.key_down("Backspace");
                        }
                    }
                    Key::Named(NamedKey::Enter) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.key_down("Enter");
                        }
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.key_down("ArrowLeft");
                        }
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.key_down("ArrowRight");
                        }
                    }
                    _ => {
                        if let Some(text) = event.text
                            && !text.chars().any(char::is_control)
                            && let Some(shell) = self.shell.as_mut()
                        {
                            shell.text_input(&text);
                        }
                    }
                }
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.startup_error = Some(error);
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging("info")?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = NativeIde::default();
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.startup_error {
        return Err(error.into());
    }
    Ok(())
}
