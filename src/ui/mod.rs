//! UI rendering module

pub mod comments;
pub mod stories;
pub mod widgets;

use crate::app::{App, View};
use ratatui::Frame;

/// Render the application UI
pub fn render(f: &mut Frame, app: &mut App, tick: usize) {
    match &app.view {
        View::Stories => stories::render(f, app, tick),
        View::Comments { .. } => comments::render(f, app, tick),
    }
}
