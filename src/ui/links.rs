use crate::app::App;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let dialog = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    f.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Web links: original post + selected comment ")
        .title_bottom(" j/k select · Enter open · Esc close ");
    let links = app.current_links();
    if links.is_empty() {
        f.render_widget(
            Paragraph::new("No web links in this post or selected comment").block(block),
            dialog,
        );
    } else {
        let list = List::new(links.iter().map(|url| ListItem::new(*url)))
            .block(block)
            .highlight_symbol("▸ ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
        f.render_stateful_widget(
            list,
            dialog,
            &mut ListState::default().with_selected(Some(app.link_cursor)),
        );
    }
}
