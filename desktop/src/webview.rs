//! The live article pane's child view: one wry `WebView` (a `WKWebView`) added as a subview of
//! the Rune window's content view, pinned to the article pane's rect.
//!
//! This is the pattern `prototypes/rune-webview` proved at this pin, with the spike's marker page
//! replaced by the story's own URL. The only Rune API it needs is
//! `WindowHandle::with_raw_handle` (`crates/rune/src/window/handle.rs:133`); everything else is
//! wry's own child-view builder. The rect comes from `El::on_layout`
//! (`crates/rune/src/el/layout_watch.rs:33`), which reports `[x, y, w, h]` in window points with
//! the origin at the top-left, y down — the same space wry's `Rect` uses.
//!
//! Everything here runs on the UI thread. `WKWebView` must be created, resized and dropped on
//! the thread that owns the window, so the view lives in a thread-local and every entry point is
//! called from a Rune handler (`poll`, `on_layout`, `on_window`, `on_close`) — all of which run
//! there.
//!
//! Off macOS there is no child host: the functions are no-ops that report the absence, and the
//! article pane takes its fallback branch. `wry` is a macOS-only dependency for the same reason
//! (see `Cargo.toml`), so nothing here pulls webkit2gtk or WebView2 into a Linux or Windows
//! build.

#[cfg(target_os = "macos")]
pub use macos::{close, create, load_url, set_bounds, set_visible};

#[cfg(not(target_os = "macos"))]
pub use stub::{close, create, load_url, set_bounds, set_visible};

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;

    use rune::WindowHandle;
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{Rect, WebView, WebViewBuilder};

    thread_local! {
        /// The single child view. `None` before creation and after `close`.
        static CHILD: RefCell<Option<WebView>> = const { RefCell::new(None) };
    }

    /// Rune's `on_layout` rect is `[x, y, w, h]` in window points with the origin at the
    /// top-left, y down. wry's `Rect` is the same space (`window_position` flips it for AppKit),
    /// so the rect passes through unchanged apart from a floor of one point: a zero-sized
    /// `WKWebView` frame is a degenerate view, and a collapsed region should read as "tiny", not
    /// "broken".
    fn rect([x, y, w, h]: [f32; 4]) -> Rect {
        Rect {
            position: LogicalPosition::new(x, y).into(),
            size: LogicalSize::new(w.max(1.0), h.max(1.0)).into(),
        }
    }

    /// Build the child view under `window` and take ownership of it.
    ///
    /// `with_raw_handle` lends the platform handle for the duration of the closure only, and
    /// `build_as_child` needs it for exactly that long: it adds the `WKWebView` to the window's
    /// content view and keeps no reference to the handle afterwards.
    ///
    /// The view starts on `about:blank`; the caller loads the story's URL with [`load_url`] as
    /// soon as it has one, so a story that changes while the child is being built still lands on
    /// the right page.
    pub fn create(window: &WindowHandle, bounds: [f32; 4]) -> Result<(), String> {
        window.with_raw_handle(|raw| {
            let webview = WebViewBuilder::new()
                .with_bounds(rect(bounds))
                .with_url("about:blank")
                .build_as_child(&raw)
                .map_err(|error| error.to_string())?;
            CHILD.with(|slot| *slot.borrow_mut() = Some(webview));
            Ok(())
        })?
    }

    /// Follow the Rune element. Called from `on_layout`, which fires after the first layout and
    /// again whenever the element moves or resizes by more than half a point — so a window
    /// resize, or the inspector opening, reaches the child on the same frame the Rune layout
    /// settles.
    pub fn set_bounds(bounds: [f32; 4]) {
        CHILD.with(|slot| {
            if let Some(webview) = slot.borrow().as_ref() {
                let _ = webview.set_bounds(rect(bounds));
            }
        });
    }

    /// Hide or show without dropping the page: `WKWebView.setHidden`, so scroll position, form
    /// state and cookies survive a toggle.
    pub fn set_visible(visible: bool) {
        CHILD.with(|slot| {
            if let Some(webview) = slot.borrow().as_ref() {
                let _ = webview.set_visible(visible);
            }
        });
    }

    pub fn load_url(url: &str) -> Result<(), String> {
        CHILD.with(|slot| match slot.borrow().as_ref() {
            Some(webview) => webview.load_url(url).map_err(|error| error.to_string()),
            None => Err("no child view to load into".to_owned()),
        })
    }

    /// Drop the child. `WKWebView` deallocates on this thread, which is the one that made it.
    pub fn close() {
        CHILD.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod stub {
    use rune::WindowHandle;

    /// No child-view host on this platform: the article pane falls back to the extracted text.
    pub fn create(_window: &WindowHandle, _bounds: [f32; 4]) -> Result<(), String> {
        Err("no child-view host on this platform".to_owned())
    }

    pub fn set_bounds(_bounds: [f32; 4]) {}

    pub fn set_visible(_visible: bool) {}

    pub fn load_url(_url: &str) -> Result<(), String> {
        Err("no child-view host on this platform".to_owned())
    }

    pub fn close() {}
}
