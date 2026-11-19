use super::{browser, Duration, Instant, Reader, Reading, Request, SourceView};
use crate::HNCLIItem;
use crossterm::event::KeyCode;

impl Reader {
    // False delegates to the existing discussion tree controls.
    pub fn key(&mut self, key: KeyCode) -> bool {
        if self.settings.is_some() {
            self.settings_key(key);
            return true;
        }
        if self.snapshot.is_some()
            && (key == KeyCode::Char('M')
                || (key == KeyCode::Char('S')
                    && self
                        .active
                        .and_then(|id| self.readings.get(&id))
                        .is_none_or(|r| {
                            r.article.result.is_none() && r.discussion.result.is_none()
                        })))
        {
            self.browser_notice = "Snapshot preview · install hn for live AI summaries".into();
            self.notice_started = Some(Instant::now());
            return true;
        }
        if key == KeyCode::Char('M') {
            self.edit_settings();
            return true;
        }
        if key == KeyCode::Char('S') {
            let retained = self
                .active
                .and_then(|id| self.readings.get(&id))
                .is_some_and(|r| {
                    r.article.result.is_some()
                        || r.discussion.result.is_some()
                        || r.article.pending()
                        || r.discussion.pending()
                });
            if !retained && (self.config.key().is_empty() || self.auth_failed) {
                self.edit_settings();
                return true;
            }
            let generate = !retained
                && self
                    .active
                    .and_then(|id| self.readings.get(&id))
                    .is_some_and(|r| !r.article.requested() || !r.discussion.requested());
            if generate {
                self.request(Request::Missing);
            }
            let mut saved = Ok(());
            if let Some(r) = self.active.and_then(|id| self.readings.get_mut(&id)) {
                r.shown = if r.source.is_some() {
                    saved = r.leave_source(self.store.as_mut());
                    false
                } else {
                    generate || !r.shown || (r.moved && r.growth_cap == 0)
                };
                if r.shown {
                    r.moved = false;
                }
                r.citation_focus = false;
                r.reveal_citation = false;
            }
            self.report_save(saved);
            return true;
        }
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            return false;
        };
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left => {
                let was_source = reading.source.is_some();
                let saved = reading.leave_source(self.store.as_mut());
                if !was_source {
                    reading.article.cancel_preparation();
                    reading.discussion.cancel_preparation();
                    self.active = None;
                    self.close_browser();
                }
                self.report_save(saved);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let citations = reading.citations();
                let count = citations.len();
                if count > 0 {
                    // Tab is an explicit request to read the summary, like S.
                    reading.shown = true;
                    reading.moved = false;
                    if reading.citation_focus {
                        reading.citation = (reading.citation
                            + if key == KeyCode::Tab { 1 } else { count - 1 })
                            % count;
                    }
                    reading.citation_focus = true;
                    reading.reveal_citation = true;
                    if reading.source.is_some()
                        && reading.source.as_ref().map(|source| source.id)
                            != Some(citations[reading.citation])
                    {
                        reading.source = Some(SourceView::new(citations[reading.citation]));
                    }
                }
            }
            KeyCode::Enter if reading.source.is_none() && reading.citation_focus => {
                reading.source = reading
                    .citations()
                    .get(reading.citation)
                    .copied()
                    .map(SourceView::new);
            }
            KeyCode::Char('J') | KeyCode::Char('K')
                if reading.source.is_none() && reading.shown =>
            {
                reading.citation_focus = false;
                let old = reading.summary_scroll;
                reading.summary_scroll = if key == KeyCode::Char('J') {
                    reading.summary_scroll.saturating_add(1)
                } else {
                    reading.summary_scroll.saturating_sub(1)
                };
                reading.moved |= reading.summary_scroll != old;
            }
            _ => {
                if reading.source.is_none() {
                    reading.citation_focus = false;
                    return false;
                }
                let source = reading.source.as_mut().unwrap();
                source.scroll_key(key);
            }
        }
        true
    }
}

impl Reader {
    pub fn comments_moved(&mut self) {
        if let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) {
            reading.moved = true;
        }
    }
    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }
    pub(crate) fn source_id(&self) -> Option<i32> {
        self.active
            .and_then(|id| self.readings.get(&id))
            .and_then(|reading| reading.source.as_ref().map(|source| source.id))
    }
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }
    pub fn open(&mut self, story: HNCLIItem) {
        if self.active != Some(story.id) {
            if let Some(previous) = self.active.and_then(|id| self.readings.get_mut(&id)) {
                previous.article.cancel_preparation();
                previous.discussion.cancel_preparation();
            }
        }
        self.active = Some(story.id);
        let reading = self.readings.entry(story.id).or_insert_with(|| {
            let mut reading = Reading {
                theme: self.theme,
                ..Reading::new(story.clone())
            };
            if let Some(store) = &self.store {
                reading.restore(store);
            }
            reading
        });
        reading.story = story;
        reading.moved = false;
        reading.panel_motion = None;
        reading.growth_cap = 0;
        let saved = reading.leave_source(self.store.as_mut());
        reading.citation_focus = false;
        self.report_save(saved);
        self.load_captured_summary();
        if self.config.automatic {
            self.request(Request::Automatic);
        }
    }
    pub fn current_story(&self) -> Option<HNCLIItem> {
        self.active
            .and_then(|id| self.readings.get(&id))
            .map(|r| r.story.clone())
    }
    pub fn has_browser(&self) -> bool {
        self.browser.is_some() || self.browser_busy
    }
    /// Back to the feed: close the companion article tab/pane this CLI created.
    /// In snapshot previews the article pane lives in the web page, so the close
    /// is signalled with an OSC instead of a browser command.
    pub fn close_browser(&mut self) {
        if self.snapshot.is_some() {
            use std::io::Write;
            let _ = write!(std::io::stdout(), "\x1b]777;hn-article-close\x07");
            let _ = std::io::stdout().flush();
            self.browser_notice = "Article pane closed".into();
            self.notice_started = Some(Instant::now());
            return;
        }
        self.desired_browser = None;
        if let Some(target) = self.browser.take() {
            tokio::spawn(async move {
                let _ = browser::close(&target).await;
            });
            self.browser_notice = "Closing article pane…".into();
            self.notice_started = Some(Instant::now());
        }
    }
    pub fn read_original(&mut self, story: HNCLIItem, focus: bool) {
        if self.snapshot.is_some() {
            use std::io::Write;
            let _ = write!(std::io::stdout(), "\x1b]777;hn-article={}\x07", story.id);
            let _ = std::io::stdout().flush();
            self.browser_notice = "Original website in article pane".into();
            self.notice_started = Some(Instant::now());
            return;
        }
        if self.browser_busy {
            self.desired_browser = Some((story, focus));
            self.browser_notice = "Opening latest article…".into();
            self.notice_started = Some(Instant::now());
            return;
        }
        self.browser_busy = true;
        self.browser_notice = "Opening article…".into();
        self.notice_started = Some(Instant::now());
        let existing = self.browser.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = browser::open(story, existing, focus)
                .await
                .map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }
    pub fn poll(&mut self) {
        while let Ok(result) = self.rx.try_recv() {
            self.browser_busy = false;
            match result {
                Ok((target, notice)) => {
                    self.notice_started = Some(Instant::now());
                    self.browser = target;
                    self.browser_notice = notice;
                    if self.active.is_none() {
                        // The reader was left while the pane was still opening.
                        self.close_browser();
                    }
                }
                Err(e) => {
                    self.notice_started = None;
                    self.browser_notice = e;
                }
            }
            if let Some((story, focus)) = self.desired_browser.take() {
                self.read_original(story, focus);
            }
        }
        if !self.browser_busy
            && self
                .notice_started
                .is_some_and(|start| start.elapsed() >= Duration::from_secs(4))
        {
            self.browser_notice.clear();
            self.notice_started = None;
        }
        self.poll_jobs();
    }

    pub fn has_active_motion(&self, now: Instant) -> bool {
        // Clearing the notice is timed even with reduced motion or outside the reader.
        (!self.browser_busy && !self.browser_notice.is_empty() && self.notice_started.is_some())
            || self
                .active
                .and_then(|id| self.readings.get(&id))
                .is_some_and(|r| r.has_active_motion(now))
            || (self.theme.motion
                && self
                    .settings
                    .as_ref()
                    .and_then(|s| s.opened)
                    .is_some_and(|start| {
                        now.saturating_duration_since(start) < Duration::from_millis(120)
                    }))
    }

    pub fn has_pending_work(&self) -> bool {
        self.browser_busy
            || !self.jobs.is_empty()
            || self
                .readings
                .values()
                .any(|reading| reading.article.pending() || reading.discussion.pending())
    }
}
