//! Restoring and saving the window layout.

use super::{App, INSPECTOR_MIN, INSPECTOR_MAX};
use rune_kit::PaneState;
use crate::layout::Layout;

impl App {
    /// Put the reader back where they left off: the tab, the page, the selected story and the
    /// inspector's width. A headless run has no layout, so this is a no-op there.
    pub(super) fn restore(&mut self) {
        if let Some(tab) = self.layout.tab {
            self.feed.tab = tab;
        }
        if let Some(page) = self.layout.page.filter(|page| *page > 1) {
            self.feed.display_cursor = (page as usize - 1) * self.feed.page_size as usize;
        }
        self.feed.pending_anchor = self.layout.selected;
        if let Some(width) = self.layout.inspector {
            let width = width.clamp(INSPECTOR_MIN, INSPECTOR_MAX);
            self.inspector_width.set(width);
            self.inspector_default_width = width;
            self.inspector = PaneState::new(width / self.zoom.scale())
                .range(INSPECTOR_MIN / self.zoom.scale(), INSPECTOR_MAX / self.zoom.scale())
                .collapsed();
        }
    }

    /// Rune rounds view widths; only a grip change may replace the window-point width.
    pub(super) fn inspector_window_width(&self) -> f32 {
        let zoom = self.zoom.scale();
        let stored = self.inspector_width.get();
        let projected = (stored / zoom).round().clamp(self.inspector.min_width(), self.inspector.max_width());
        let shown = self.inspector.width();
        if shown != projected { self.inspector_width.set(shown * zoom); }
        self.inspector_width.get()
    }

    /// What the window would write now: its size, the inspector's width, and where the feed is.
    pub(super) fn snapshot(&self) -> Layout {
        Layout {
            window: self.window_size.get().or(self.layout.window),
            zoom: Some(self.zoom.scale()),
            inspector: Some(self.inspector_window_width()),
            tab: Some(self.feed.tab),
            page: Some(self.feed.stepper()),
            selected: self.feed.selected_id(),
        }
    }

    /// Write `layout`, remembering it so an unchanged window writes nothing.
    pub(super) fn write_layout(&mut self, layout: Layout, now: f64) {
        let Some(path) = self.layout_path.as_ref() else {
            return;
        };
        if layout.save(path).is_ok() {
            self.layout = layout.clone();
            self.layout_written = layout;
            self.layout_saved_at = now;
        }
    }
}
