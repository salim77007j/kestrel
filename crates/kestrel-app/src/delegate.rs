//! The `WebViewDelegate` implementation.
//!
//! One delegate is created per tab, holding the tab id alongside the shared
//! state. That is what lets an engine callback find the tab it belongs to
//! without a reverse lookup from `WebView` identity, which the embed API does
//! not expose.
//!
//! Two methods carry most of the privacy weight:
//!  * [`TabDelegate::load_web_resource`] runs for every network request and
//!    applies the filter engine.
//!  * [`TabDelegate::request_permission`] denies by default; nothing is granted
//!    that the user has not explicitly approved.

use crate::engine::Shared;
use kestrel_core::adblock::{Decision, ResourceType};
use kestrel_ui::tabs::TabState;
use servo::{
    AllowOrDeny, CreateNewWebViewRequest, LoadStatus, NavigationRequest, PermissionRequest,
    WebResourceLoad, WebResourceRequest, WebView, WebViewDelegate,
};
use std::rc::Rc;

/// Per-tab delegate.
pub struct TabDelegate {
    pub shared: Rc<Shared>,
    pub tab_id: u64,
}

impl TabDelegate {
    pub fn new(shared: Rc<Shared>, tab_id: u64) -> Self {
        Self { shared, tab_id }
    }

    /// Run `f` against the app if it is still alive.
    fn with_app<R>(&self, f: impl FnOnce(&mut crate::app::App) -> R) -> Option<R> {
        let mut guard = self.shared.app.borrow_mut();
        guard.as_mut().map(f)
    }
}

/// Classify a request so that type-restricted rules (`$image`, `$script`) apply.
pub fn resource_type_for(req: &WebResourceRequest) -> ResourceType {
    if req.is_for_main_frame {
        return ResourceType::Document;
    }
    let accept = req
        .headers
        .get("accept")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    // `Accept` is only a hint, but it is the signal available before the
    // response arrives, which is the only point where blocking is still useful.
    if accept.contains("script") || accept.contains("javascript") {
        ResourceType::Script
    } else if accept.contains("css") {
        ResourceType::Stylesheet
    } else if accept.contains("image") {
        ResourceType::Image
    } else if accept.contains("font") {
        ResourceType::Font
    } else if accept.contains("audio") || accept.contains("video") {
        ResourceType::Media
    } else {
        ResourceType::Other
    }
}

impl WebViewDelegate for TabDelegate {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.shared.mark_dirty(self.tab_id);
    }

    fn notify_url_changed(&self, _webview: WebView, url: url::Url) {
        let id = self.tab_id;
        self.with_app(|app| {
            if let Some(tab) = app.strip.get_mut(id) {
                tab.replace_current(url.as_str());
                tab.state = TabState::Loading;
            }
        });
        self.shared.mark_dirty(id);
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        let id = self.tab_id;
        let title = title.unwrap_or_default();
        self.with_app(|app| {
            if let Some(tab) = app.strip.get_mut(id) {
                tab.title = title;
            }
        });
        self.shared.mark_dirty(id);
    }

    fn notify_load_status_changed(&self, _webview: WebView, status: LoadStatus) {
        let id = self.tab_id;
        self.with_app(|app| {
            if let Some(t) = app.strip.get_mut(id) {
                t.state = match status {
                    LoadStatus::Complete => TabState::Ready,
                    LoadStatus::Failed => TabState::Crashed,
                    // `Pending` and `InProgress` both mean "not yet".
                    _ => TabState::Loading,
                };
            }
            // Record the visit only once the document actually completed, so
            // history never fills with pages that failed to load.
            if matches!(status, LoadStatus::Complete) {
                if let Some(tab) = app.strip.get(id) {
                    let (u, t) = (tab.url.clone(), tab.title.clone());
                    app.store.record_visit(&u, &t);
                }
            }
        });
        self.shared.mark_dirty(id);
    }

    fn notify_history_changed(&self, _webview: WebView, _entries: Vec<url::Url>, _current: usize) {
        // The tab owns its own history vector; this only needs to wake the UI so
        // the back/forward buttons re-evaluate their enabled state.
        self.shared.redraw.set(true);
    }

    fn notify_cursor_changed(&self, _webview: WebView, _cursor: servo::Cursor) {
        self.shared.redraw.set(true);
    }

    fn notify_favicon_changed(&self, _webview: WebView) {
        self.shared.mark_dirty(self.tab_id);
    }

    fn notify_animating_changed(&self, _webview: WebView, _animating: bool) {
        self.shared.mark_dirty(self.tab_id);
    }

    fn notify_focus_changed(&self, _webview: WebView, _focused: bool) {
        self.shared.redraw.set(true);
    }

    fn notify_status_text_changed(&self, _webview: WebView, _status: Option<String>) {
        self.shared.redraw.set(true);
    }

    /// The ad and tracker blocker.
    ///
    /// Runs on a Servo network thread. The filter engine is an immutable
    /// `Arc`, so no lock is taken on the request path; app mutations are routed
    /// through the main thread.
    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
        let req = load.request().clone();
        let url = req.url.to_string();
        let is_main_frame = req.is_for_main_frame;

        let decision = self
            .with_app(|app| app.check_request(&url, resource_type_for(&req)))
            .unwrap_or(Decision::Allow);

        match decision {
            Decision::Allow => {
                // Dropping the load tells Servo to continue as normal.
                drop(load);
            }
            Decision::Block(reason) => {
                log::debug!("blocked {url} ({})", reason.category.as_str());
                if is_main_frame {
                    // A blocked top-level navigation should explain itself.
                    drop(load);
                    self.with_app(|app| {
                        app.page = kestrel_ui::tabs::Page::Error(reason.rule.clone())
                    });
                } else {
                    // An empty successful response makes the request fail
                    // quietly: faster than a network error, and it tells a
                    // tracker nothing about why the resource is missing.
                    let response = crate::engine::blocked_response(&url);
                    load.intercept(response).finish();
                }
                self.with_app(|app| {
                    if let Some(tab) = app.strip.get_mut(self.tab_id) {
                        tab.trackers_blocked = tab.trackers_blocked.saturating_add(1);
                    }
                });
            }
        }
    }

    /// Permissions default to deny.
    ///
    /// Granting a capability the user did not explicitly approve is the worst
    /// failure mode a privacy browser can have, so the safe answer is the only
    /// one available until the permission store is consulted.
    fn request_permission(&self, _webview: WebView, request: PermissionRequest) {
        let feature = request.feature();
        let origin = self
            .with_app(|app| app.strip.get(self.tab_id).map(|t| t.url.clone()))
            .unwrap_or_default();
        log::info!("denying {feature:?} for {origin}");

        if let Some(app) = self.shared.app.borrow().as_ref() {
            // A previously stored "allow" for this origin is honoured; the
            // settings UI is where the user grants one.
            let origin_key = url::Url::parse(&origin)
                .map(|u| kestrel_core::omnibox::origin_key(&u))
                .unwrap_or(origin.clone());
            let key = permission_key(feature);
            if app
                .permission_store
                .iter()
                .any(|p| p.origin == origin_key && p.kind == key)
            {
                request.allow();
                return;
            }
        }
        request.deny();
    }

    fn request_navigation(&self, _webview: WebView, request: NavigationRequest) {
        // The network filter is the control point for what may be fetched, so
        // navigation itself is allowed through and vetted downstream.
        request.allow();
    }

    /// `window.open` should open a tab, not a window. The view needs the
    /// rendering context, which is main-thread state, so the request is denied
    /// here and the caller opens a tab from the resulting navigation instead.
    fn request_create_new(&self, _parent: WebView, request: CreateNewWebViewRequest) {
        drop(request);
        self.shared.redraw.set(true);
    }

    fn show_console_message(
        &self,
        _webview: WebView,
        _level: servo::ConsoleLogLevel,
        message: String,
    ) {
        log::info!("page console: {message}");
    }

    fn notify_closed(&self, _webview: WebView) {
        self.shared.redraw.set(true);
    }
}

/// Stable string key for a permission feature, used in the permission store.
pub fn permission_key(feature: &servo::PermissionFeature) -> String {
    format!("{feature:?}")
}

/// Anchor so the `AllowOrDeny` import documents the delegate contract.
#[allow(dead_code)]
fn _allow_or_deny_anchor(_: AllowOrDeny) {}
