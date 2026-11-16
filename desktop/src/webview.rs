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
pub use macos::{close, create, ready, set_bounds, set_visible, set_waker};

#[cfg(not(target_os = "macos"))]
pub use stub::{close, create, ready, set_bounds, set_visible, set_waker};

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use rune::{Waker, WindowHandle};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder};

    struct Child {
        view: WebView,
        ready: Rc<Cell<bool>>,
    }

    thread_local! {
        /// The single child view. `None` before creation and after `close`.
        static CHILD: RefCell<Option<Child>> = const { RefCell::new(None) };
        static WAKER: RefCell<Option<Waker>> = const { RefCell::new(None) };
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
    /// Each hidden child navigates directly to its story. Its own commit callback cannot
    /// mark a replacement ready, even when an older navigation finishes late.
    pub fn create(window: &WindowHandle, bounds: [f32; 4], url: &str) -> Result<(), String> {
        let ready = Rc::new(Cell::new(false));
        let committed = Rc::clone(&ready);
        let waker = WAKER.with(|slot| slot.borrow().clone());
        window.with_raw_handle(|raw| {
            let webview = WebViewBuilder::new()
                .with_bounds(rect(bounds))
                .with_visible(false)
                .with_url(url)
                .with_on_page_load_handler(move |event, url| {
                    // On macOS Started is didCommitNavigation, not the navigation request.
                    if matches!(event, PageLoadEvent::Started)
                        && !url.is_empty() && url != "about:blank"
                    {
                        committed.set(true);
                        if let Some(waker) = &waker { waker(); }
                    }
                })
                .build_as_child(&raw)
                .map_err(|error| error.to_string())?;
            CHILD.with(|slot| {
                *slot.borrow_mut() = Some(Child { view: webview, ready });
            });
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
                let _ = webview.view.set_bounds(rect(bounds));
            }
        });
    }

    /// Hide or show without dropping the page: `WKWebView.setHidden`, so scroll position, form
    /// state and cookies survive a toggle.
    pub fn set_visible(visible: bool) {
        CHILD.with(|slot| {
            if let Some(webview) = slot.borrow().as_ref() {
                let _ = webview.view.set_visible(visible);
            }
        });
    }

    pub fn set_waker(waker: Waker) {
        WAKER.with(|slot| *slot.borrow_mut() = Some(waker));
    }

    pub fn ready() -> bool {
        CHILD.with(|slot| slot.borrow().as_ref().is_some_and(|child| child.ready.get()))
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
    use rune::{Waker, WindowHandle};

    /// No child-view host on this platform: the article pane falls back to the extracted text.
    pub fn create(_window: &WindowHandle, _bounds: [f32; 4], _url: &str) -> Result<(), String> {
        Err("no child-view host on this platform".to_owned())
    }

    pub fn set_bounds(_bounds: [f32; 4]) {}

    pub fn set_visible(_visible: bool) {}

    pub fn set_waker(_waker: Waker) {}

    pub fn ready() -> bool { false }

    pub fn close() {}
}
