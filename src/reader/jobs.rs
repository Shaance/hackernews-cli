use super::*;
use saved::{ArticleData, DiscussionData, Header, Record};

pub enum Output {
    ArticleSource(Result<Capture<ArticleCapture>, Failure>),
    DiscussionSource(Result<Capture<DiscussionCapture>, Failure>),
    Article(Provider, Result<Article, Failure>),
    Discussion(Provider, Result<Discussion, Failure>),
}
pub struct Completion {
    pub story: i32,
    pub attempt: Attempt,
    pub output: Output,
}
impl<T, C, S> Section<T, C, S> {
    pub(super) fn queue(&mut self, profile: Profile, next: &mut u64, request: Request) {
        if matches!(request, Request::Automatic | Request::Missing) && self.requested() {
            return;
        }
        if matches!(request, Request::Retry) && !self.failed() {
            return;
        }
        if !matches!(request, Request::Refresh) {
            if matches!(self.progress, Progress::Queued(a) | Progress::Running(a) if a.profile == profile)
            {
                return;
            }
            if self.result.as_ref().is_some_and(|r| {
                r.profile == profile
                    && r.capture().is_none_or(|original| {
                        self.capture
                            .as_ref()
                            .is_some_and(|current| Arc::ptr_eq(current, original))
                    })
            }) && !(matches!(request, Request::Retry) && self.failed())
            {
                self.cancel_preparation();
                self.progress = Progress::Idle;
                return;
            }
            if let Progress::Preparing(attempt, _, desired) = &mut self.progress {
                *desired = profile;
                attempt.automatic = false;
                return;
            }
        }
        let refreshing = matches!(self.progress, Progress::Failed(a, _) if a.refresh);
        self.cancel_preparation();
        if matches!(request, Request::Refresh)
            || self.result.as_ref().is_some_and(|r| r.capture().is_none())
        {
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
                self.capture = self.result.as_ref().and_then(|r| r.capture().cloned());
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
    fn finish(
        &mut self,
        attempt: Attempt,
        result: Result<T, Failure>,
    ) -> Option<Completed<T, C, S>> {
        if !matches!(self.progress, Progress::Running(a) if a == attempt) {
            return None;
        }
        match result {
            Ok(value) => {
                self.progress = Progress::Idle;
                Some(Completed {
                    value,
                    profile: attempt.profile,
                    basis: Basis::Captured(
                        self.capture
                            .as_ref()
                            .expect("running inference has input")
                            .clone(),
                    ),
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
        if reading.staged.as_ref().is_some_and(|(r, _, _)| {
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
        let mut saved = Ok(());
        match completion.output {
            Output::ArticleSource(result) => {
                reading.article.prepared(completion.attempt, result);
            }
            Output::DiscussionSource(result) => {
                reading.discussion.prepared(completion.attempt, result);
            }
            Output::Article(provider, result) => {
                if matches!(reading.article.progress, Progress::Running(a) if a == completion.attempt)
                {
                    self.auth_failed |= result.as_ref().err().is_some_and(|e| e.auth);
                }
                if let Some(value) = reading.article.finish(completion.attempt, result) {
                    if let (Some(store), Basis::Captured(capture)) = (&mut self.store, &value.basis)
                    {
                        saved = store.save_article(
                            Record {
                                header: Header::new(reading.story.id, provider, value.profile),
                                data: ArticleData {
                                    article: value.value.clone(),
                                    partial: capture.input.partial,
                                },
                            },
                            capture.input.text.trim().is_empty(),
                        );
                    }
                    reading.article.result = Some(value);
                }
            }
            Output::Discussion(provider, result) => {
                if matches!(reading.discussion.progress, Progress::Running(a) if a == completion.attempt)
                {
                    self.auth_failed |= result.as_ref().err().is_some_and(|e| e.auth);
                }
                if let Some(value) = reading.discussion.finish(completion.attempt, result) {
                    let comments = reading
                        .story
                        .comments
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or_default();
                    if reading.source.is_some() {
                        reading.staged = Some((value, provider, comments));
                    } else {
                        reading.discussion.result = Some(value);
                        reading.citation = 0;
                        reading.citation_focus = false;
                        if let Some(store) = &mut self.store {
                            saved = save_discussion(
                                store,
                                reading.story.id,
                                reading.discussion.result.as_ref().unwrap(),
                                provider,
                                comments,
                            );
                        }
                    }
                }
            }
        }
        self.report_save(saved);
    }
    pub(super) fn report_save(&mut self, result: std::io::Result<()>) {
        if let Err(error) = result {
            self.browser_notice = format!("Summary save failed: {error}");
            self.notice_started = None;
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

pub(super) fn save_discussion(
    store: &mut SummaryStore,
    story_id: i32,
    result: &Completed<Discussion, DiscussionCapture, DiscussionContext>,
    provider: Provider,
    comments_at_save: u32,
) -> std::io::Result<()> {
    let Basis::Captured(capture) = &result.basis else {
        return Ok(());
    };
    store.save_discussion(Record {
        header: Header::new(story_id, provider, result.profile),
        data: DiscussionData {
            discussion: result.value.clone(),
            context: DiscussionContext {
                comments_at_save,
                coverage: capture.input.coverage.clone(),
                sources: capture.input.sources.clone(),
            },
        },
    })
}
