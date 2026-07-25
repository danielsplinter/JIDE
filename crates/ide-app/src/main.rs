use ide_core::init_logging;
use ide_ui::IdeShell;
use ui_core::{Size, WindowId};
use ui_render_api::{FrameInfo, UiRenderer};
use ui_render_wgpu::WgpuRenderer;
use ui_window_api::WindowRequest;
use ui_window_winit::WinitWindow;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::WindowId as WinitWindowId,
};

#[derive(Default)]
struct NativeIde {
    window: Option<WinitWindow>,
    renderer: Option<WgpuRenderer>,
    shell: Option<IdeShell>,
    startup_error: Option<String>,
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
        self.shell = Some(IdeShell::demo());
        self.renderer = Some(renderer);
        window.show();
        self.window = Some(window);
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let window = self.window.as_ref().ok_or_else(|| "window unavailable".to_owned())?;
        let size = window.logical_size();
        let commands = self.shell.as_ref().ok_or_else(|| "shell unavailable".to_owned())?.paint(size);
        let renderer = self.renderer.as_mut().ok_or_else(|| "renderer unavailable".to_owned())?;
        renderer.begin_frame(FrameInfo {
            window_id: window.handle().id,
            logical_size: size,
            scale_factor: window.scale_factor(),
        }).map_err(|error| error.to_string())?;
        renderer.submit(&commands).map_err(|error| error.to_string())?;
        renderer.end_frame().map_err(|error| error.to_string())
    }
}

impl ApplicationHandler for NativeIde {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() && let Err(error) = self.initialize(event_loop) {
            self.startup_error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WinitWindowId, event: WindowEvent) {
        let Some(window) = self.window.as_ref() else { return };
        if window.inner().id() != id { return; }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() { renderer.resize(size.width, size.height); }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.inner().inner_size();
                if let Some(renderer) = self.renderer.as_mut() { renderer.resize(size.width, size.height); }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                match event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        if let Some(shell) = self.shell.as_mut() { shell.search_query.clear(); }
                    }
                    Key::Named(NamedKey::F3) => {
                        if let Some(shell) = self.shell.as_mut() {
                            shell.search_query = if shell.search_query.is_empty() { "symbol".to_owned() } else { String::new() };
                        }
                    }
                    _ => {}
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
    if let Some(error) = app.startup_error { return Err(error.into()); }
    Ok(())
}
