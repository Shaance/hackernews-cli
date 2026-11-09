//! The macOS child-view host: one wry `WebView` (a `WKWebView`) added as a subview of the Rune
//! window's content view, pinned to a rect in window points.
//!
//! Everything here runs on the UI thread. `WKWebView` must be created, resized and dropped on
//! the thread that owns the window, so the view lives in a thread-local and every entry point
//! is called from a Rune handler (`poll`, `on_layout`, `on_key`, `on_close`) — all of which run
//! there.

use std::cell::RefCell;

use rune::WindowHandle;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

thread_local! {
    /// The single child view. `None` before creation and after `close`.
    static CHILD: RefCell<Option<WebView>> = const { RefCell::new(None) };
}

/// Rune's `on_layout` rect is `[x, y, w, h]` in window points with the origin at the top-left,
/// y down. wry's `Rect` is the same space (`window_position` flips it for AppKit), so the rect
/// passes through unchanged apart from a floor of one point: a zero-sized `WKWebView` frame is
/// a degenerate view, and a collapsed region should read as "tiny", not "broken".
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
pub(super) fn create(window: &WindowHandle, bounds: [f32; 4], html: &str) -> Result<(), String> {
    window.with_raw_handle(|raw| {
        let webview = WebViewBuilder::new()
            .with_bounds(rect(bounds))
            .with_html(html)
            .build_as_child(&raw)
            .map_err(|error| error.to_string())?;
        CHILD.with(|slot| *slot.borrow_mut() = Some(webview));
        Ok(())
    })?
}

/// Follow the Rune element. Called from `on_layout`, which fires after the first layout and
/// again whenever the element moves or resizes by more than half a point — so a window resize
/// reaches the child on the same frame the Rune layout settles.
pub(super) fn set_bounds(bounds: [f32; 4]) {
    CHILD.with(|slot| {
        if let Some(webview) = slot.borrow().as_ref() {
            let _ = webview.set_bounds(rect(bounds));
        }
    });
}

/// Hide or show without dropping the page: `WKWebView.setHidden`, so scroll position, form
/// state and cookies survive a toggle.
pub(super) fn set_visible(visible: bool) {
    CHILD.with(|slot| {
        if let Some(webview) = slot.borrow().as_ref() {
            let _ = webview.set_visible(visible);
        }
    });
}

pub(super) fn load_url(url: &str) -> Result<(), String> {
    CHILD.with(|slot| match slot.borrow().as_ref() {
        Some(webview) => webview.load_url(url).map_err(|error| error.to_string()),
        None => Err("no child view to load into".to_owned()),
    })
}

/// Run JavaScript in the page and ignore the result. The IPC direction the spike does not need
/// but a real integration would: `evaluate_script` for host-to-page, `with_ipc_handler` plus
/// `window.ipc.postMessage(...)` for page-to-host.
#[allow(dead_code)]
pub(super) fn evaluate(js: &str) -> Result<(), String> {
    CHILD.with(|slot| match slot.borrow().as_ref() {
        Some(webview) => webview.evaluate_script(js).map_err(|error| error.to_string()),
        None => Err("no child view to evaluate in".to_owned()),
    })
}

/// Drop the child. `WKWebView` deallocates on this thread, which is the one that made it.
pub(super) fn close() {
    CHILD.with(|slot| {
        slot.borrow_mut().take();
    });
}
