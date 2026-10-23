use super::{browser, Reader, Reading, Request};
use crossterm::event::KeyCode;
use hn_lib::HNCLIItem;

impl Reader {
    // False delegates to the existing discussion tree controls.
    pub fn key(&mut self, key: KeyCode) -> bool {
        if self.settings.is_some() {
            self.settings_key(key);
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
            if let Some(r) = self.active.and_then(|id| self.readings.get_mut(&id)) {
                r.shown = if r.source.is_some() {
                    r.leave_source();
                    false
                } else {
                    generate || !r.shown || (r.moved && r.growth_cap == Some(0))
                };
                if r.shown {
                    r.growth_cap = None;
                }
                r.citation_focus = false;
                r.reveal_citation = false;
            }
            return true;
        }
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            return false;
        };
        match key {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') | KeyCode::Left => {
                let was_source = reading.source.is_some();
                reading.leave_source();
                if !was_source {
                    reading.article.cancel_preparation();
                    reading.discussion.cancel_preparation();
                    self.active = None;
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let citations = reading.citations();
                let count = citations.len();
                if count > 0 {
                    // Tab is an explicit request to read the summary, like S.
                    reading.shown = true;
                    reading.growth_cap = None;
                    if reading.citation_focus {
                        reading.citation = (reading.citation
                            + if key == KeyCode::Tab { 1 } else { count - 1 })
                            % count;
                    }
                    reading.citation_focus = true;
                    reading.reveal_citation = true;
                    if reading.source.is_some()
                        && reading.source != Some(citations[reading.citation])
                    {
                        reading.source = Some(citations[reading.citation]);
                        reading.source_scroll = 0;
                    }
                }
            }
            KeyCode::Enter if reading.source.is_none() && reading.citation_focus => {
                reading.source = reading.citations().get(reading.citation).copied();
                reading.source_scroll = 0;
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
                let scroll = &mut reading.source_scroll;
                let page_step = reading.page_rows.saturating_sub(1).max(1);
                match key {
                    KeyCode::Char('j') | KeyCode::Down => *scroll = scroll.saturating_add(1),
                    KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                    KeyCode::PageDown => *scroll = scroll.saturating_add(page_step),
                    KeyCode::PageUp => *scroll = scroll.saturating_sub(page_step),
                    KeyCode::Char('g') => *scroll = 0,
                    KeyCode::Char('G') => *scroll = usize::MAX,
                    _ => {}
                }
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
        let reading = self.readings.entry(story.id).or_insert_with(|| Reading {
            theme: self.theme,
            ..Reading::new(story.clone())
        });
        reading.story = story;
        reading.moved = false;
        reading.growth_cap = Some(0);
        reading.leave_source();
        reading.citation_focus = false;
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
    pub fn read_original(&mut self, story: HNCLIItem, focus: bool) {
        if self.browser_busy {
            self.desired_browser = Some((story, focus));
            self.browser_notice = "Opening latest article…".into();
            return;
        }
        self.browser_busy = true;
        self.browser_notice = "Opening article…".into();
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
                    self.browser = target;
                    self.browser_notice = notice;
                }
                Err(e) => self.browser_notice = e,
            }
            if let Some((story, focus)) = self.desired_browser.take() {
                self.read_original(story, focus);
            }
        }
        self.poll_jobs();
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
