use super::{Progress, Reading, Section};
use hn_lib::theme::Theme;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

pub(super) fn append(lines: &mut Vec<Line<'static>>, text: &str, width: usize, style: Style) {
    for paragraph in text.lines() {
        for row in textwrap::wrap(paragraph, width.max(1)) {
            lines.push(Line::styled(row.into_owned(), style));
        }
    }
    lines.push(Line::raw(""));
}

impl Reading {
    pub(super) fn summary_lines(&self, width: usize) -> (Vec<Line<'static>>, Option<usize>) {
        let mut lines = vec![];
        if !self.shown {
            return (lines, None);
        }
        let width = width.saturating_sub(4).max(1);
        let theme = self.theme;
        let normal = theme.fg;
        let mut target = None;
        lines.push(Line::styled(
            format!(
                "ARTICLE  {}{}",
                self.article.label(),
                self.article
                    .result
                    .as_ref()
                    .map(|r| format!(" · captured {}", r.capture.captured))
                    .unwrap_or_default()
            ),
            theme.dim,
        ));
        if let Some(result) = &self.article.result {
            let article = &result.value;
            if result.capture.input.partial {
                append(
                    &mut lines,
                    "Partial source · opening portion only",
                    width,
                    theme.accent,
                );
            }
            append(&mut lines, &article.text, width, normal);
        }
        self.article.append_status(&mut lines, width, theme);
        lines.push(Line::styled(
            format!(
                "DISCUSSION  {}{}",
                self.discussion.label(),
                self.discussion
                    .result
                    .as_ref()
                    .map(|r| format!(" · {} · {}", r.capture.input.coverage, r.capture.captured))
                    .unwrap_or_default()
            ),
            theme.dim,
        ));
        if let Some(result) = &self.discussion.result {
            let discussion = &result.value;
            if discussion.points.is_empty() {
                append(&mut lines, &discussion.overview, width, normal);
            }
            let citations = self.citations();
            let selected = citations.get(self.citation);
            for point in &discussion.points {
                let active =
                    self.citation_focus && selected.is_some_and(|id| point.sources.contains(id));
                let refs = point
                    .sources
                    .iter()
                    .filter_map(|id| citations.iter().position(|candidate| candidate == id))
                    .map(|i| format!("[{}]", i + 1))
                    .collect::<Vec<_>>()
                    .join(" ");
                let body = format!("• {} ", point.text);
                let token = format!("[{}]", self.citation + 1);
                let focus = active.then(|| refs.find(&token)).flatten().map(|i| {
                    let start = body.len() + i;
                    start..start + token.len()
                });
                let text = format!("{body}{refs}");
                let mut consumed = 0;
                for row in textwrap::wrap(&text, width) {
                    let start = consumed + text[consumed..].find(row.as_ref()).unwrap();
                    consumed = start + row.len();
                    let mut line = Line::from(vec![
                        Span::raw("  "),
                        Span::styled(
                            if active { "┃ " } else { "│ " },
                            if active { theme.accent } else { theme.line },
                        ),
                    ]);
                    // Split wrapped rows at bullet, text, citation and focus boundaries.
                    let mut cuts = vec![
                        start,
                        consumed,
                        "• ".len().clamp(start, consumed),
                        body.len().clamp(start, consumed),
                    ];
                    if let Some(focus) = &focus {
                        cuts.extend([
                            focus.start.clamp(start, consumed),
                            focus.end.clamp(start, consumed),
                        ]);
                    }
                    cuts.sort_unstable();
                    cuts.dedup();
                    for pair in cuts.windows(2) {
                        let (left, right) = (pair[0], pair[1]);
                        let style = if focus.as_ref().is_some_and(|f| f.contains(&left)) {
                            target.get_or_insert(lines.len());
                            theme
                                .accent
                                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                        } else if left >= body.len() {
                            theme.link
                        } else if left < "• ".len() {
                            theme.dim
                        } else {
                            normal
                        };
                        line.push_span(Span::styled(text[left..right].to_owned(), style));
                    }
                    lines.push(line);
                }
                lines.push(Line::raw(""));
            }
        }
        self.discussion.append_status(&mut lines, width, theme);
        if lines
            .last()
            .is_some_and(|line| line.spans.is_empty() || line.width() == 0)
        {
            lines.pop();
        }
        for line in &mut lines {
            if line.spans.first().is_none_or(|span| span.content != "  ") {
                line.spans.insert(0, Span::styled("│ ", theme.line));
                line.spans.insert(0, Span::raw("  "));
            }
        }
        (lines, target)
    }

    pub(super) fn source_lines(&self, id: i32, width: usize) -> Vec<Line<'static>> {
        let theme = self.theme;
        let width = width.saturating_sub(2);
        let mut lines = vec![];
        let mut current = Some(id);
        while let Some(source) = current.and_then(|id| {
            self.discussion
                .result
                .as_ref()
                .and_then(|r| r.capture.input.sources.iter().find(|s| s.id == id))
        }) {
            let label = if source.id == id {
                "CITED COMMENT"
            } else {
                "PARENT CONTEXT"
            };
            lines.push(Line::from(vec![
                Span::styled(label, theme.dim),
                Span::styled(format!("  {}", source.author), theme.fg),
                Span::styled(format!(" · #{}", source.id), theme.dim),
            ]));
            lines.push(Line::raw(""));
            if source.excerpt.is_empty() {
                append(
                    &mut lines,
                    "Parent comment unavailable in captured sources",
                    width,
                    theme.dim,
                );
            } else {
                append(&mut lines, &source.excerpt, width, theme.fg);
                if source.text.len() > source.excerpt.len() {
                    append(
                        &mut lines,
                        "Additional context · not sent to the model",
                        width,
                        theme.dim,
                    );
                    append(
                        &mut lines,
                        &source.text[source.excerpt.len()..],
                        width,
                        theme.fg,
                    );
                }
            }
            current = source.parent;
        }
        for line in &mut lines {
            line.spans.insert(0, Span::raw("  "));
        }
        lines
    }
}

impl<T, C> Section<T, C> {
    pub(super) fn label(&self) -> String {
        let actual = self.result.as_ref().map(|r| r.profile.label());
        match &self.progress {
            Progress::Queued(a) => format!(
                "{}{}{} · waiting",
                actual.unwrap_or(""),
                if actual.is_some() { " → " } else { "" },
                a.profile.label()
            ),
            Progress::Preparing(_, _, profile) => format!(
                "{}{}{} · working",
                actual.unwrap_or(""),
                if actual.is_some() { " → " } else { "" },
                profile.label()
            ),
            Progress::Running(a) => format!(
                "{}{}{} · working",
                actual.unwrap_or(""),
                if actual.is_some() { " → " } else { "" },
                a.profile.label()
            ),
            Progress::Failed(a, _) if a.refresh => format!(
                "{} · Refresh failed · M → Enter Retry",
                actual.unwrap_or(a.profile.label())
            ),
            Progress::Failed(a, _) => format!(
                "{} unavailable{}",
                a.profile.label(),
                actual
                    .map(|p| format!(" · showing {p}"))
                    .unwrap_or_default()
            ),
            Progress::Idle => actual.unwrap_or("Not requested").to_owned(),
        }
    }
    fn append_status(&self, lines: &mut Vec<Line<'static>>, width: usize, theme: Theme) {
        if let Progress::Failed(_, error) = &self.progress {
            append(lines, &error.message, width, theme.error);
        } else if self.result.is_none() {
            append(
                lines,
                if self.pending() {
                    "Working…"
                } else {
                    "M Settings"
                },
                width,
                theme.dim,
            );
        }
    }
}
