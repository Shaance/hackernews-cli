use super::{App, View};
use std::time::{Duration, Instant};

// Delay before showing loading indicators to avoid flicker
const LOADING_INDICATOR_DELAY_MS: u64 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusScope {
    Stories,
    Comments,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ViewStatus {
    loading: bool,
    error: Option<String>,
    loading_since: Option<Instant>,
}

impl App {
    /// Set story-view error
    pub fn set_story_error(&mut self, error: String) {
        self.set_scoped_error(StatusScope::Stories, error);
    }

    /// Set comment-view error
    pub fn set_comment_error(&mut self, error: String) {
        self.set_scoped_error(StatusScope::Comments, error);
    }

    /// Set story-view loading state
    pub fn set_story_loading(&mut self, loading: bool) {
        self.set_scoped_loading(StatusScope::Stories, loading);
    }

    /// Set comment-view loading state
    pub fn set_comment_loading(&mut self, loading: bool) {
        self.set_scoped_loading(StatusScope::Comments, loading);
    }

    /// Recompute comment loading without dismissing current errors.
    pub fn recompute_comment_loading(&mut self, loading: bool) {
        self.set_scoped_loading_preserving_error(StatusScope::Comments, loading);
    }

    pub(super) fn clear_story_status(&mut self) {
        self.clear_status(StatusScope::Stories);
    }

    pub(super) fn clear_comment_status(&mut self) {
        self.clear_status(StatusScope::Comments);
    }

    fn set_scoped_loading(&mut self, scope: StatusScope, loading: bool) {
        let status = self.status_mut(scope);
        status.loading = loading;
        status.loading_since = if loading { Some(Instant::now()) } else { None };
        if loading {
            status.error = None;
        }
    }

    fn set_scoped_loading_preserving_error(&mut self, scope: StatusScope, loading: bool) {
        let status = self.status_mut(scope);
        status.loading = loading;
        status.loading_since = if loading { Some(Instant::now()) } else { None };
    }

    fn set_scoped_error(&mut self, scope: StatusScope, error: String) {
        let status = self.status_mut(scope);
        status.error = Some(error);
        status.loading = false;
        status.loading_since = None;
    }

    fn clear_status(&mut self, scope: StatusScope) {
        *self.status_mut(scope) = ViewStatus::default();
    }

    fn active_status_scope(&self) -> StatusScope {
        match self.view {
            View::Stories => StatusScope::Stories,
            View::Comments { .. } => StatusScope::Comments,
        }
    }

    fn status(&self, scope: StatusScope) -> &ViewStatus {
        match scope {
            StatusScope::Stories => &self.story_status,
            StatusScope::Comments => &self.comment_status,
        }
    }

    fn status_mut(&mut self, scope: StatusScope) -> &mut ViewStatus {
        match scope {
            StatusScope::Stories => &mut self.story_status,
            StatusScope::Comments => &mut self.comment_status,
        }
    }

    fn active_status(&self) -> &ViewStatus {
        self.status(self.active_status_scope())
    }

    pub fn is_loading(&self) -> bool {
        self.active_status().loading
    }

    pub fn error(&self) -> Option<&str> {
        self.active_status().error.as_deref()
    }

    /// Whether loading indicator should be visible (debounced)
    pub fn should_show_loading(&self) -> bool {
        let status = self.active_status();
        if !status.loading {
            return false;
        }

        match status.loading_since {
            Some(started) => started.elapsed() >= Duration::from_millis(LOADING_INDICATOR_DELAY_MS),
            None => true,
        }
    }
}
