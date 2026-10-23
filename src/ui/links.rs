use crate::app::App;
use ratatui::{text::Line, Frame};

use super::widgets::{hint_line, render_modal};

pub fn render(f: &mut Frame, app: &App) {
    let theme = app.theme();
    let links = app.current_links();
    render_modal(f, theme, app.overlay_opened, "Links", |width, height| {
        let mut lines = vec![
            Line::styled("From the post and the selected comment", theme.dim),
            Line::raw(""),
        ];
        let selected = if links.is_empty() {
            lines.push(Line::styled(
                "No links in the post or the selected comment",
                theme.dim,
            ));
            None
        } else {
            let rows = usize::from(height.saturating_sub(4));
            let start = (app.link_cursor + 1).saturating_sub(rows);
            lines.extend(
                links
                    .iter()
                    .skip(start)
                    .take(rows)
                    .map(|url| Line::styled((*url).to_owned(), theme.link)),
            );
            (rows > 0).then_some(2 + app.link_cursor - start)
        };
        lines.push(Line::raw(""));
        let mut hints = hint_line(
            theme,
            &[("j/k", "select"), ("Enter", "open"), ("Esc", "close")],
            None,
            width + 4,
        );
        hints.spans.remove(0); // The modal supplies the two-column padding.
        lines.push(hints);
        (lines, selected)
    });
}
