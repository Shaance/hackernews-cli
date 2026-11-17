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
pub use macos::{close, create, ready, set_bounds, set_visible, set_waker, take_back};

#[cfg(not(target_os = "macos"))]
pub use stub::{close, create, ready, set_bounds, set_visible, set_waker, take_back};

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::{Cell, RefCell};
    use std::ptr::NonNull;
    use std::rc::Rc;

    use block2::RcBlock;
    use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
    use rune::{Waker, WindowHandle};
    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewExtMacOS};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum BackPress { Idle, Held, Cancelled }

    /// The monitor belongs to one child and is removed before that child's webview is dropped.
    struct BackMonitor {
        token: Retained<AnyObject>,
        press: Rc<Cell<BackPress>>,
        pending: Rc<Cell<bool>>,
    }

    impl BackMonitor {
        fn new(view: &WebView, waker: Option<Waker>) -> Result<Self, String> {
            let mtm = MainThreadMarker::new().ok_or("webview monitor must run on the main thread")?;
            let view = view.webview();
            let window = view.window().ok_or("webview has no window")?;
            let press = Rc::new(Cell::new(BackPress::Idle));
            let pending = Rc::new(Cell::new(false));
            let on_press = Rc::clone(&press);
            let on_pending = Rc::clone(&pending);
            let block = RcBlock::new(move |ptr: NonNull<NSEvent>| -> *mut NSEvent {
                // AppKit invokes this local monitor on the main thread before dispatch.
                let event = unsafe { ptr.as_ref() };
                if event.r#type() == NSEventType::KeyDown {
                    if event.keyCode() == 53
                        && !event.modifierFlags().intersects(NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
                        && !view.isHiddenOrHasHiddenAncestor()
                        && event.window(mtm).is_some_and(|event_window| std::ptr::eq(&*event_window, &*window))
                    {
                        on_pending.set(true);
                        if let Some(waker) = &waker { waker(); }
                        return std::ptr::null_mut();
                    }
                    return ptr.as_ptr();
                }
                // Mouse utilities can send macOS's navigation swipe instead of button 3.
                if event.r#type() == NSEventType::Swipe {
                    let owned = event.window(mtm).is_some_and(|event_window| std::ptr::eq(&*event_window, &*window));
                    if owned && event.deltaX() > 0.5 && event.deltaX() > event.deltaY().abs() {
                        on_pending.set(true);
                        if let Some(waker) = &waker { waker(); }
                        return std::ptr::null_mut();
                    }
                    return ptr.as_ptr();
                }
                if event.buttonNumber() != 3 { return ptr.as_ptr(); }
                let same_window = event.window(mtm).is_some_and(|event_window| std::ptr::eq(&*event_window, &*window));
                if !same_window {
                    if event.r#type() == NSEventType::OtherMouseUp { on_press.set(BackPress::Idle); }
                    return ptr.as_ptr();
                }
                if event.r#type() == NSEventType::OtherMouseDown {
                    let point = view.convertPoint_fromView(event.locationInWindow(), None);
                    let bounds = view.bounds();
                    let inside = point.x >= bounds.origin.x && point.y >= bounds.origin.y
                        && point.x < bounds.origin.x + bounds.size.width && point.y < bounds.origin.y + bounds.size.height;
                    if !view.isHiddenOrHasHiddenAncestor() && inside {
                        on_press.set(BackPress::Held);
                        return std::ptr::null_mut();
                    }
                    on_press.set(BackPress::Idle);
                } else if event.r#type() == NSEventType::OtherMouseUp {
                    let held = on_press.replace(BackPress::Idle);
                    if held != BackPress::Idle {
                        if held == BackPress::Held && !view.isHiddenOrHasHiddenAncestor() {
                            on_pending.set(true);
                            if let Some(waker) = &waker { waker(); }
                        }
                        return std::ptr::null_mut();
                    }
                } else if event.r#type() == NSEventType::OtherMouseDragged && on_press.get() != BackPress::Idle {
                    return std::ptr::null_mut();
                }
                ptr.as_ptr()
            });
            let mask = NSEventMask::OtherMouseDown | NSEventMask::OtherMouseDragged | NSEventMask::OtherMouseUp | NSEventMask::Swipe | NSEventMask::KeyDown;
            // The block returns the original event or null and is retained by AppKit until removal.
            let token = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
                .ok_or("could not install webview mouse monitor")?;
            Ok(Self { token, press, pending })
        }

        fn hide(&self) {
            self.pending.set(false);
            if self.press.get() == BackPress::Held { self.press.set(BackPress::Cancelled); }
        }
    }

    impl Drop for BackMonitor {
        fn drop(&mut self) {
            // This token was returned by addLocalMonitor and is removed exactly once.
            unsafe { NSEvent::removeMonitor(&self.token); }
        }
    }

    struct Child {
        monitor: BackMonitor,
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
        let mouse_waker = waker.clone();
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
            let monitor = BackMonitor::new(&webview, mouse_waker)?;
            let old = CHILD.with(|slot| slot.borrow_mut().replace(Child { monitor, view: webview, ready }));
            drop(old);
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
                if !visible { webview.monitor.hide(); }
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

    pub fn take_back() -> bool {
        CHILD.with(|slot| slot.borrow().as_ref().is_some_and(|child| child.monitor.pending.replace(false)))
    }

    /// Drop the child. `WKWebView` deallocates on this thread, which is the one that made it.
    pub fn close() {
        let child = CHILD.with(|slot| slot.borrow_mut().take());
        drop(child);
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

    pub fn take_back() -> bool { false }

    pub fn close() {}
}
