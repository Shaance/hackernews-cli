//! Native article page lifecycle and geometry.

use super::{App, Pane, Live};
use crate::webview;

impl App {
    /// Whether this run can host a live page at all: macOS, with a real window.
    ///
    /// A `WKWebView` needs an `NSWindow`, and a headless run has none — `--snap` and `--tree`
    /// render offscreen. So headless always takes the fallback branch, which is what makes the
    /// scene trees deterministic: the same arguments produce the same tree on every machine,
    /// with no child view to depend on.
    fn live_capable(&self) -> bool {
        cfg!(target_os = "macos") && !self.headless
    }

    /// Whether the article pane is the live page rather than the extracted text.
    fn live_mode(&self) -> bool {
        self.live_capable() && !matches!(self.live, Live::Failed { .. })
    }

    /// What the article pane draws: the live page, or the extracted text and why it is there.
    pub(super) fn pane(&self) -> Pane<'_> {
        if self.live_mode() {
            Pane::Live { loading: !matches!(&self.live, Live::Created { child, .. } if child.committed()) }
        } else {
            Pane::Extracted {
                reason: match &self.live {
                    Live::Failed { error, .. } => Some(error.as_str()),
                    _ => None,
                },
            }
        }
    }

    /// The article pane's live region reported its rect: pin the child to it.
    ///
    /// Called from `on_layout`, which runs on the UI thread — the only thread a `WKWebView` may
    /// be resized on.
    pub(crate) fn live_rect(&mut self, rect: [f32; 4]) {
        let rect = rect.map(|value| value * self.zoom.scale());
        self.live_bounds = Some(rect);
        if let Live::Created { child, .. } = &mut self.live {
            child.set_bounds(rect);
        }
    }

    /// Build this story's hidden child once the window and current pane bounds exist.
    /// Show it after its own navigation commits, only while the pane is on screen.
    ///
    /// The child is a subview of the window's content view, so it draws above everything Rune
    /// paints. That is why it is hidden while a dialog is open: the settings dialog and the
    /// keyboard sheet are Rune's, and a webview over them would bury the controls.
    pub(super) fn sync_live(&mut self) -> bool {
        if !self.live_mode() {
            return self.hide_live();
        }
        let Some(story) = self.story.as_ref() else {
            return self.hide_live();
        };
        // The same scheme-checked URL the terminal reader opens, with the HN item page as the
        // fallback for a story that has no usable link.
        let story_id = story.id();
        let url = hn_lib::url_open::safe_story_url(&story.story.url, story_id);
        let mut changed = false;
        if matches!(self.live, Live::Idle) {
            let (Some(window), Some(bounds)) = (self.native_window.as_ref(), self.live_bounds)
            else {
                // The window or the pane's rect has not landed yet; the next frame has both.
                return false;
            };
            let child = match webview::create(window, bounds, &url) {
                Ok(child) => child,
                Err(error) => {
                    self.live = Live::Failed { story_id, error };
                    return true;
                }
            };
            child.set_zoom(self.zoom.scale());
            self.live = Live::Created { story_id, child };
            changed = true;
        }
        if let Live::Created { child, .. } = &mut self.live {
            match self.live_bounds.filter(|_| child.committed() && self.settings.is_none() && !self.help) {
                Some(bounds) => changed |= child.show(bounds),
                None => changed |= child.hide(),
            }
        }
        changed
    }

    /// Take the child off screen, keeping the page: `set_visible` is `WKWebView.setHidden`, so
    /// scroll position and session survive the feed coming back.
    pub(super) fn hide_live(&mut self) -> bool {
        match &mut self.live {
            Live::Created { child, .. } => child.hide(),
            _ => false,
        }
    }
}
