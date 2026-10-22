//! Source preparation can be abandoned; submitted inference survives navigation.
use super::{
    jobs::{Completion, Output},
    *,
};

impl Reader {
    pub(super) fn start_queued(&mut self) {
        if self.config.key().is_empty() || self.auth_failed {
            return;
        }
        let Some(reading) = self.active.and_then(|id| self.readings.get_mut(&id)) else {
            return;
        };
        if let Progress::Queued(attempt) = reading.article.progress {
            if self.jobs.len() < 4 {
                let story = reading.story.clone();
                let network = self.network.clone();
                if let Some(capture) = reading.article.capture.clone() {
                    let key = self.config.key().to_owned();
                    let provider = self.config.provider;
                    self.jobs.spawn(async move {
                        Completion {
                            story: story.id,
                            attempt,
                            output: Output::Article(
                                network
                                    .article(provider, &key, attempt.profile, &capture.input)
                                    .await,
                            ),
                        }
                    });
                    reading.article.progress = Progress::Running(attempt);
                } else {
                    let task = self.jobs.spawn(async move {
                        Completion {
                            story: story.id,
                            attempt,
                            output: Output::ArticleSource(
                                sources::article_capture(&network, story, attempt.refresh).await,
                            ),
                        }
                    });
                    reading.article.progress = Progress::Preparing(attempt, task, attempt.profile);
                }
            }
        }
        if let Progress::Queued(attempt) = reading.discussion.progress {
            if self.jobs.len() < 4 {
                let story = reading.story.clone();
                let network = self.network.clone();
                if let Some(capture) = reading.discussion.capture.clone() {
                    let key = self.config.key().to_owned();
                    let provider = self.config.provider;
                    self.jobs.spawn(async move {
                        Completion {
                            story: story.id,
                            attempt,
                            output: Output::Discussion(
                                network
                                    .discussion(provider, &key, attempt.profile, &capture.input)
                                    .await,
                            ),
                        }
                    });
                    reading.discussion.progress = Progress::Running(attempt);
                } else {
                    let task = self.jobs.spawn(async move {
                        Completion {
                            story: story.id,
                            attempt,
                            output: Output::DiscussionSource(
                                sources::discussion(&network.service, story, attempt.refresh).await,
                            ),
                        }
                    });
                    reading.discussion.progress =
                        Progress::Preparing(attempt, task, attempt.profile);
                }
            }
        }
    }
}
