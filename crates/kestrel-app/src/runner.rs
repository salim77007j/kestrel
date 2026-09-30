//! The window event loop and frame pump.
//!
//! Ordering matters and is fixed here: Servo is pumped on the thread that
//! created it, the interface is built from a settled app state, and the frame is
//! composited last. Anything that violates that order shows up as a hung or
//! flickering window, so it is encoded in one place rather than scattered.

use crate::app::App;
use crate::delegate::TabDelegate;
use crate::engine::{Shared, WakeEvent, Waker};
use crate::render::Compositor;
use anyhow::Result;
use egui_winit::State;
use std::cell::RefCell;
use std::rc::Rc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct Ui {
    window: Rc<Window>,
    egui: egui::Context,
    state: State,
    compositor: Compositor,
}

impl EngineHost {
    /// Run the browser until the window is closed.
    pub fn run(
        &self,
        event_loop: EventLoop<WakeEvent>,
        app: App,
        rendering: Rc<dyn servo::RenderingContext>,
        screenshot: Option<String>,
    ) -> Result<()> {
        *self.shared.app.borrow_mut() = Some(app);
        *self.shared.rendering.borrow_mut() = Some(rendering);
        let mut runner = Runner {
            shared: self.shared.clone(),
            app: RefCell::new(None),
            ui: RefCell::new(None),
            frames: 0,
            screenshot,
            last_output: egui::FullOutput::default(),
        };
        // Hand the app over from the shared slot to the runner.
        let taken = self.shared.app.borrow_mut().take();
        *runner.app.borrow_mut() = taken;
        event_loop.run_app(&mut runner)?;
        Ok(())
    }
}

struct Runner {
    shared: Rc<Shared>,
    app: RefCell<Option<App>>,
    ui: RefCell<Option<Ui>>,
    frames: u32,
    screenshot: Option<String>,
    /// The most recent egui output, kept so a screenshot can re-render the
    /// exact frame the user would have seen.
    last_output: egui::FullOutput,
}

impl Runner {
    /// Ensure the active tab has an engine view for the URL it should show.
    fn sync_active_view(&self) {
        let mut app = match self.app.borrow_mut() {
            Some(a) => a,
            None => return,
        };
        let (Some(tab), Some(servo)) = (app.strip.active_tab(), app.engine.as_ref()) else {
            return;
        };
        let (id, url) = (tab.id, tab.url.clone());
        if tab.engine_view.is_some() {
            return;
        }
        let context = match self.shared.rendering.borrow().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let delegate = TabDelegate::new(self.shared.clone(), id);
        match servo::WebViewBuilder::new(servo, context)
            .url(match url::Url::parse(&url) {
                Ok(u) => u,
                Err(_) => return,
            })
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(delegate)
            .build()
        {
            view => {
                app.views.insert(id, view);
                if let Some(t) = app.strip.get_mut(id) {
                    t.engine_view = Some(id);
                    t.state = kestrel_ui::tabs::TabState::Loading;
                }
                self.shared.mark_dirty(id);
            }
        }
    }

    /// Paint the active tab and read the result back for compositing.
    fn paint_active(&self) {
        let mut app = match self.app.borrow_mut() {
            Some(a) => a,
            None => return,
        };
        let (Some(tab), Some(servo)) = (app.strip.active_tab(), app.engine.as_ref()) else {
            return;
        };
        let id = tab.id;
        let Some(view) = app.views.get(&id) else { return };

        // Only the active tab is painted. Background tabs keep their state but
        // consume no compositor time, which is the cheap form of throttling.
        view.paint();
        let _ = servo;

        // Read the painted surface back into a CPU image the compositor can
        // upload. Doing this only when the engine reports a new frame keeps the
        // cost off idle pages.
        let context = match self.shared.rendering.borrow().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let size = context.size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let rect = webrender_api::units::DeviceIntRect::from_origin_and_size(
            webrender_api::units::DeviceIntPoint::new(0, 0),
            webrender_api::units::DeviceIntSize::new(size.width, size.height),
        );
        if let Some(img) = context.read_to_image(rect) {
            self.shared.frames.borrow_mut().insert(id, img);
        }
        self.shared.dirty.borrow_mut().retain(|d| *d != id);
    }

    fn frame(&self, ui: &Ui) -> Result<egui::FullOutput> {
        self.sync_active_view();

        // Advance the engine.
        if let Some(app) = self.app.borrow().as_ref() {
            if let Some(servo) = app.engine.as_ref() {
                servo.spin_event_loop();
            }
        }

        self.paint_active();

        // Build the interface.
        ui.egui.begin_frame();
        let output = {
            let raw = ui
                .state
                .take_input(&ui.egui)
                .unwrap_or_else(|_| Default::default());
            let mut app = match self.app.borrow_mut() {
                Some(a) => a,
                None => return Ok(egui::FullOutput::default()),
            };
            // Apply any navigation the interface queued last frame.
            if let Some(url) = app.take_navigation() {
                if let Some(tab) = app.strip.active_tab_mut() {
                    if tab.url != url {
                        // A new document means the old engine view is stale.
                        let id = tab.id;
                        if tab.engine_view.is_some() {
                            app.views.remove(&id);
                            tab.engine_view = None;
                        }
                        tab.navigate(&url);
                        // An internal page replaces the web view rather than
                        // navigating the engine to a `kestrel://` URL it cannot
                        // resolve.
                        if kestrel_ui::tabs::Page::from_url(&url).is_some() {
                            app.page = kestrel_ui::tabs::Page::from_url(&url).unwrap();
                        } else {
                            app.page = kestrel_ui::tabs::Page::Web;
                        }
                    }
                }
            }
            let mut output = egui::FullOutput::default();
            let _ = crate::chrome::ui(&mut app, &ui.egui, &raw, &mut output);
            // Upload the latest page image, if the engine produced one.
            let active_id = app.strip.active_tab().map(|t| t.id);
            let latest = active_id
                .and_then(|id| self.shared.frames.borrow().get(&id).cloned());
            if let Some(img) = latest {
                let texture = ui.egui.load_texture(
                    "page",
                    egui::ColorImage::from_rgba_unmultiplied(
                        img.width() as usize,
                        img.height() as usize,
                        &img.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                );
                app.page_texture = Some(texture);
            }
            output
        };
        ui.egui.end_frame();

        ui.compositor.render(&ui.egui, output.clone())?;
        self.shared.frames_rendered.set(self.shared.frames_rendered.get() + 1);
        Ok(output)
    }

    /// Write a PNG of the current frame, then ask the loop to stop.
    fn capture(&self, ui: &Ui, event_loop: &ActiveEventLoop) {
        let Some(path) = self.screenshot.clone() else { return };
        match ui.compositor.capture(&ui.egui, &self.last_output) {
            Some(img) => {
                if let Err(e) = img.save(&path) {
                    log::error!("could not write {path}: {e}");
                } else {
                    log::info!("wrote screenshot to {path}");
                }
            }
            None => log::error!("capture produced no image"),
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<WakeEvent> for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("Kestrel")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760.0, 540.0));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                log::error!("could not create a window: {e}");
                event_loop.exit();
                return;
            }
        };
        let compositor = match Compositor::new(&window) {
            Ok(c) => c,
            Err(e) => {
                log::error!("could not create the GPU compositor: {e:#}");
                event_loop.exit();
                return;
            }
        };
        let egui = egui::Context::default();
        let state = State::new(
            window.clone(),
            &egui,
            std::sync::Arc::new(egui_winit::default_input_handler()),
        );
        *self.ui.borrow_mut() = Some(Ui {
            window,
            egui,
            state,
            compositor,
        });
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let ui_ref = self.ui.borrow();
        let Some(ui) = ui_ref.as_ref() else { return };
        match &event {
            WindowEvent::CloseRequested => {
                if let Some(app) = self.app.borrow().as_ref() {
                    app.save_session();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                let _ = ui.compositor.resize(&ui.window, *size);
            }
            WindowEvent::RedrawRequested => match self.frame(ui) {
                Ok(output) => {
                    self.last_output = output;
                    ui.window.request_redraw();
                }
                Err(e) => {
                    log::error!("frame failed: {e:#}");
                    ui.window.request_redraw();
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let Key::Named(named) = event.logical_key else {
                    return;
                };
                let ctrl = ui.egui.input(|i| i.modifiers.ctrl || i.modifiers.mac_cmd);
                if self.shortcut(named, event.state, ctrl) {
                    ui.window.request_redraw();
                }
            }
            _ => {
                let _ = ui.state.on_window_event(&ui.egui, &event);
            }
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let ui_ref = self.ui.borrow();
        let Some(ui) = ui_ref.as_ref() else { return };
        self.frames += 1;
        if self.screenshot.is_some() && self.frames >= 3 {
            self.capture(ui, _event_loop);
            return;
        }
        ui.window.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: WakeEvent) {
        if let Some(ui) = self.ui.borrow().as_ref() {
            ui.window.request_redraw();
        }
    }
}

impl Runner {
    /// Browser-level keyboard shortcuts. Returns true when one was handled.
    fn shortcut(&self, key: NamedKey, state: ElementState, ctrl: bool) -> bool {
        if state != ElementState::Pressed {
            return false;
        }
        let mut app = match self.app.borrow_mut() {
            Some(a) => a,
            None => return false,
        };
        match (ctrl, key) {
            (true, NamedKey::ArrowLeft) => app.go_back(),
            (true, NamedKey::ArrowRight) => app.go_forward(),
            (true, NamedKey::KeyR) => app.reload(),
            (true, NamedKey::KeyT) => app.new_tab(),
            (true, NamedKey::KeyW) => app.close_active_tab(),
            (true, NamedKey::KeyL) => {
                app.pending.navigate = Some(app.settings.homepage.clone());
            }
            (false, NamedKey::Escape) => {
                if app.page != kestrel_ui::tabs::Page::Web {
                    app.page = kestrel_ui::tabs::Page::Web;
                }
            }
            _ => return false,
        }
        true
    }
}
