//! Servo engine integration.
//!
//! Responsibilities:
//!  * create and own the Servo instance and the per-tab `WebView`s;
//!  * pump Servo's event loop on the main thread, as it requires;
//!  * implement `WebViewDelegate` so engine notifications reach the app;
//!  * enforce the privacy policy at the network boundary via `load_web_resource`.

use crate::app::App;
use anyhow::{Context, Result};
use kestrel_core::adblock::{Decision, ResourceType};
use kestrel_ui::tabs::TabState;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use servo::protocol_handler::{ProtocolRegistry, Request, Response};
use servo::{
    AllowOrDeny, EventLoopWaker, RenderingContext, Servo, ServoBuilder, WebResourceLoad,
    WebView, WebViewBuilder, WebViewDelegate,
};

/// Shared between the app and the engine delegates.
///
/// Servo's delegates are called from the main thread, so a `RefCell` is the
/// correct primitive here: it makes the single-threaded ownership explicit
/// rather than hiding it behind a lock.
pub struct Shared {
    pub app: RefCell<Option<App>>,
    /// Latest page image per tab, uploaded to the compositor.
    pub frames: RefCell<HashMap<u64, servo::RgbaImage>>,
    /// Tabs whose content changed since the last composited frame.
    pub dirty: RefCell<Vec<u64>>,
    /// Engine views by tab id.
    pub views: RefCell<HashMap<u64, WebView>>,
    /// The offscreen surface Servo renders page content into.
    pub rendering: RefCell<Option<Rc<dyn RenderingContext>>>,
    pub redraw: Cell<bool>,
    pub quit: Cell<bool>,
    /// Frames rendered, for the perf report.
    pub frames_rendered: Cell<u64>,
}

impl Shared {
    fn with_app<R>(&self, f: impl FnOnce(&mut App) -> R) -> Option<R> {
        let mut guard = self.app.borrow_mut();
        guard.as_mut().map(f)
    }

    fn mark_dirty(&self, id: u64) {
        let mut d = self.dirty.borrow_mut();
        if !d.contains(&id) {
            d.push(id);
        }
        self.redraw.set(true);
    }
}

/// The Servo event-loop waker. Servo calls this from its own threads when it
/// has work for the main thread.
#[derive(Clone)]
pub struct Waker(pub winit::event_loop::EventLoopProxy<WakeEvent>);

#[derive(Debug)]
pub enum WakeEvent {
    Wake,
    Quit,
}

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(WakeEvent::Wake);
    }
}

/// Owns the engine lifetime. Constructed before the event loop so the event
/// loop proxy can be handed to Servo at creation time.
pub struct EngineHost {
    pub shared: Rc<Shared>,
    /// Set when the engine is disabled for a UI-only run.
    pub enabled: bool,
}

impl EngineHost {
    pub fn new(_ui_only: bool) -> Result<Self> {
        Ok(Self {
            shared: Rc::new(Shared {
                app: RefCell::new(None),
                frames: RefCell::new(HashMap::new()),
                dirty: RefCell::new(Vec::new()),
                views: RefCell::new(HashMap::new()),
                rendering: RefCell::new(None),
                redraw: Cell::new(true),
                quit: Cell::new(false),
                frames_rendered: Cell::new(0),
            }),
            enabled: !_ui_only,
        })
    }

    /// Build the Servo instance. The context is an offscreen software surface
    /// rather than the window, so the browser owns a single GPU surface and can
    /// run with no GPU at all.
    pub fn build_servo(&self, waker: Waker) -> Result<(Servo, Rc<dyn RenderingContext>)> {
        let size = winit::dpi::PhysicalSize::new(1280, 800);
        let context = servo::SoftwareRenderingContext::new(size)
            .map_err(|e| anyhow::anyhow!("could not create the rendering context: {e:?}"))?;
        let context: Rc<dyn RenderingContext> = Rc::new(context);
        let _ = context.make_current();

        let servo = ServoBuilder::default()
            .event_loop_waker(Box::new(waker))
            .build();
        servo.setup_logging();
        Ok((servo, context))
    }

    /// Create an engine view for a tab.
    pub fn create_view(
        &self,
        servo: &Servo,
        context: &Rc<dyn RenderingContext>,
        url: &str,
        tab_id: u64,
    ) -> Option<WebView> {
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(e) => {
                log::warn!("cannot parse {url}: {e}");
                return None;
            }
        };
        let view = WebViewBuilder::new(servo, context.clone())
            .url(parsed)
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(self.shared.clone())
            .build();
        {
            let mut views = self.shared.views.borrow_mut();
            views.insert(tab_id, view.clone());
        }
        self.shared.mark_dirty(tab_id);
        Some(view)
    }
}

impl Drop for EngineHost {
    fn drop(&mut self) {
        if let Some(app) = self.shared.app.borrow_mut().take() {
            // Persist state before the engine goes away so a crash on exit
            // cannot lose the session.
            app.save_session();
        }
    }
}

/// Classify a request for the blocker's type-restricted rules.
pub fn resource_type_for(req: &servo::protocol_handler::Request) -> ResourceType {
    let dest = req.destination.clone();
    let mime = dest
        .headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if req.is_main_frame {
        return ResourceType::Document;
    }
    if mime.contains("script") {
        ResourceType::Script
    } else if mime.contains("image") {
        ResourceType::Image
    } else if mime.contains("css") {
        ResourceType::Stylesheet
    } else if mime.contains("font") {
        ResourceType::Font
    } else {
        ResourceType::Other
    }
}

/// Build the synthetic response used to cancel a blocked request.
///
/// We return an empty successful response rather than a network error: an
/// error is detectable by the page, whereas an empty body lets the page fail
/// quietly and gives no signal that blocking happened.
pub fn blocked_response(url: &str) -> Response {
    use servo::protocol_handler::HttpStatus;
    Response::new(
        url::Url::parse(url).unwrap_or_else(|_| url::Url::parse("about:blank").unwrap()),
    )
    .status_code(HttpStatus::OK)
    .body(servo::protocol_handler::ResponseBody::from(vec![]))
}

/// Frame pacing: request a redraw at most this often even when animations run.
pub const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(1);

/// How long to wait before a background tab is considered idle and its engine
/// work is throttled.
pub const BACKGROUND_THROTTLE: Duration = Duration::from_secs(2);

pub struct FrameTimer {
    last: Instant,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self { last: Instant::now() }
    }
    /// Whether enough time has passed to start another frame.
    pub fn ready(&self) -> bool {
        self.last.elapsed() >= MIN_FRAME_INTERVAL
    }
    pub fn tick(&mut self) {
        self.last = Instant::now();
    }
}

impl Default for FrameTimer {
    fn default() -> Self {
        Self::new()
    }
}

// `Arc` and `AllowOrDeny` appear in the delegate signatures; reference them so
// the imports document the contract even when a build feature omits a use.
#[allow(dead_code)]
fn _type_anchors(_: Arc<()>, _: AllowOrDeny, _: Rc<dyn RenderingContext>, _: Option<WebView>) {}
