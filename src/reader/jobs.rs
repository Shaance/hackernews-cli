use super::*;

pub enum Output {
    ArticleSource(Result<Capture<ArticleCapture>, Failure>),
    DiscussionSource(Result<Capture<DiscussionCapture>, Failure>),
    Article(Result<Article, Failure>),
    Discussion(Result<Discussion, Failure>),
}
pub struct Completion {
    pub story: i32,
    pub attempt: Attempt,
    pub output: Output,
}
impl<T, C> Section<T, C> {
    pub(super) fn queue(&mut self, profile: Profile, next: &mut u64, request: Request) {
        if matches!(request, Request::Automatic | Request::Missing) && self.requested() {
            return;
        }
        if matches!(request, Request::Retry) && !self.failed() {
            return;
        }
        if !matches!(request, Request::Refresh) {
            if let Progress::Preparing(attempt, _, desired) = &mut self.progress {
                *desired = profile;
                attempt.automatic = false;
                return;
            }
            if matches!(self.progress, Progress::Queued(a) | Progress::Running(a) if a.profile == profile)
            {
                return;
            }
            if self.result.as_ref().is_some_and(|r| {
                r.profile == profile
                    && self
                        .capture
                        .as_ref()
                        .is_some_and(|c| Arc::ptr_eq(c, &r.capture))
            }) && !(matches!(request, Request::Retry) && self.failed())
            {
                self.cancel_preparation();
                self.progress = Progress::Idle;
                return;
            }
        }
        let refreshing = matches!(self.progress, Progress::Failed(a, _) if a.refresh);
        self.cancel_preparation();
        if matches!(request, Request::Refresh) {
            self.capture = None;
        }
        *next += 1;
        self.progress = Progress::Queued(Attempt {
            id: *next,
            profile,
            automatic: matches!(request, Request::Automatic),
            refresh: matches!(request, Request::Refresh)
                || refreshing
                || (self.capture.is_none() && self.result.is_some()),
        });
    }
    pub(super) fn cancel_preparation(&mut self) {
        if let Progress::Preparing(_, task, _) = &self.progress {
            task.abort();
        }
        if matches!(
            self.progress,
            Progress::Queued(_) | Progress::Preparing(_, _, _)
        ) {
            self.progress = Progress::Idle;
            // Abandoning a refresh restores the old input; a failed refresh does not.
            if self.capture.is_none() {
                self.capture = self.result.as_ref().map(|r| r.capture.clone());
            }
        }
    }
    fn prepared(&mut self, attempt: Attempt, result: Result<Capture<C>, Failure>) {
        let Progress::Preparing(current, _, desired) = self.progress else {
            return;
        };
        if current.id != attempt.id {
            return;
        }
        let next = Attempt {
            profile: desired,
            ..current
        };
        self.progress = match result {
            Ok(capture) => {
                self.capture = Some(Arc::new(capture));
                Progress::Queued(next)
            }
            Err(error) => Progress::Failed(next, error),
        };
    }
    fn finish(&mut self, attempt: Attempt, result: Result<T, Failure>) -> Option<Completed<T, C>> {
        if !matches!(self.progress, Progress::Running(a) if a == attempt) {
            return None;
        }
        match result {
            Ok(value) => {
                self.progress = Progress::Idle;
                Some(Completed {
                    value,
                    profile: attempt.profile,
                    capture: self
                        .capture
                        .as_ref()
                        .expect("running inference has input")
                        .clone(),
                })
            }
            Err(error) => {
                self.progress = Progress::Failed(attempt, error);
                None
            }
        }
    }
}
impl Reader {
    pub(super) fn request(&mut self, request: Request) {
        if self.config.key().is_empty() || self.auth_failed {
            return;
        }
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            return;
        };
        reading
            .article
            .queue(self.config.profile, &mut self.next_attempt, request);
        // A source view retains its parent result until returning.
        if reading.staged.as_ref().is_some_and(|r| {
            matches!(request, Request::Generate) && r.profile != self.config.profile
        }) || matches!(request, Request::Refresh)
        {
            reading.staged = None;
        }
        if reading.staged.is_none() {
            reading
                .discussion
                .queue(self.config.profile, &mut self.next_attempt, request);
        }
        self.start_queued();
    }
    pub(super) fn complete(&mut self, completion: Completion) {
        let Some(reading) = self.readings.get_mut(&completion.story) else {
            return;
        };
        match completion.output {
            Output::ArticleSource(result) => {
                reading.article.prepared(completion.attempt, result);
            }
            Output::DiscussionSource(result) => {
                reading.discussion.prepared(completion.attempt, result);
            }
            Output::Article(result) => {
                if matches!(reading.article.progress, Progress::Running(a) if a == completion.attempt)
                {
                    self.auth_failed |= result.as_ref().err().is_some_and(|e| e.auth);
                }
                if let Some(value) = reading.article.finish(completion.attempt, result) {
                    reading.article.result = Some(value);
                }
            }
            Output::Discussion(result) => {
                if matches!(reading.discussion.progress, Progress::Running(a) if a == completion.attempt)
                {
                    self.auth_failed |= result.as_ref().err().is_some_and(|e| e.auth);
                }
                if let Some(value) = reading.discussion.finish(completion.attempt, result) {
                    if reading.source.is_some() {
                        reading.staged = Some(value);
                    } else {
                        reading.discussion.result = Some(value);
                        reading.citation = 0;
                        reading.citation_focus = false;
                    }
                }
            }
        }
    }
    pub(super) fn poll_jobs(&mut self) {
        while let Some(result) = self.jobs.try_join_next() {
            match result {
                Ok(completion) => self.complete(completion),
                Err(error) if error.is_cancelled() => {}
                Err(_) => {
                    self.settings_error =
                        "Summary task stopped unexpectedly. Reopen hn to retry.".into()
                }
            }
        }
        self.start_queued();
    }
}
