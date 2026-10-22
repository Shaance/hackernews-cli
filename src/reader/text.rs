use super::{Progress, Reading, Section};
use ratatui::{
    style::{Color, Modifier, Style},
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
        let normal = Style::default();
        let mut target = None;
        lines.push(Line::styled(
            format!("ARTICLE · {}", self.article.label()),
            normal.fg(Color::Yellow),
        ));
        if let Some(result) = &self.article.result {
            let article = &result.value;
            if result.capture.input.partial {
                append(
                    &mut lines,
                    "Partial source · opening portion only",
                    width,
                    normal.fg(Color::Yellow),
                );
            }
            append(&mut lines, &article.text, width, normal);
            append(
                &mut lines,
                &format!("Captured {}", result.capture.captured),
                width,
                normal.fg(Color::DarkGray),
            );
        }
        self.article.append_status(&mut lines, width);
        let source_hint = if self.citations().is_empty() {
            ""
        } else {
            " · Tab Sources · Enter Open"
        };
        for row in textwrap::wrap(
            &format!("DISCUSSION · {}{source_hint}", self.discussion.label()),
            width,
        ) {
            lines.push(Line::styled(row.into_owned(), normal.fg(Color::Yellow)));
        }
        if let Some(result) = &self.discussion.result {
            let discussion = &result.value;
            for row in textwrap::wrap(
                &format!(
                    "{} · {}",
                    result.capture.input.coverage, result.capture.captured
                ),
                width,
            ) {
                lines.push(Line::styled(row.into_owned(), normal.fg(Color::DarkGray)));
            }
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
                let body = format!("{} {} ", if active { ">" } else { "•" }, point.text);
                let token = format!("[{}]", self.citation + 1);
                let focus = active.then(|| refs.find(&token)).flatten().map(|i| {
                    let start = body.len() + i;
                    start..start + token.len()
                });
                let text = format!("{body}{refs}");
                let point_style = if active {
                    normal.fg(Color::Cyan)
                } else {
                    normal
                };
                let mut consumed = 0;
                for row in textwrap::wrap(&text, width) {
                    let start = consumed + text[consumed..].find(row.as_ref()).unwrap();
                    consumed = start + row.len();
                    let mut line = Line::styled(row.to_string(), point_style);
                    if let Some(focus) = &focus {
                        let left = focus.start.max(start);
                        let right = focus.end.min(consumed);
                        if left < right {
                            target.get_or_insert(lines.len());
                            line = Line::from(vec![
                                Span::styled(row[..left - start].to_owned(), point_style),
                                Span::styled(
                                    row[left - start..right - start].to_owned(),
                                    point_style.add_modifier(Modifier::BOLD | Modifier::REVERSED),
                                ),
                                Span::styled(row[right - start..].to_owned(), point_style),
                            ]);
                        }
                    }
                    lines.push(line);
                }
                lines.push(Line::raw(""));
            }
        }
        self.discussion.append_status(&mut lines, width);
        if lines
            .last()
            .is_some_and(|line| line.spans.is_empty() || line.width() == 0)
        {
            lines.pop();
        }
        for line in &mut lines {
            line.spans.insert(
                0,
                ratatui::text::Span::styled("│ ", normal.fg(Color::Yellow)),
            );
        }
        (lines, target)
    }

    pub(super) fn source_lines(&self, id: i32, width: usize) -> Vec<Line<'static>> {
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
            append(
                &mut lines,
                &format!("{label} · {} · #{}", source.author, source.id),
                width,
                Style::default().fg(Color::Yellow),
            );
            if source.excerpt.is_empty() {
                append(
                    &mut lines,
                    "Parent comment unavailable in captured sources",
                    width,
                    Style::default().fg(Color::DarkGray),
                );
            } else {
                append(&mut lines, &source.excerpt, width, Style::default());
                if source.text.len() > source.excerpt.len() {
                    append(
                        &mut lines,
                        "Additional context · not sent to the model",
                        width,
                        Style::default().fg(Color::DarkGray),
                    );
                    append(
                        &mut lines,
                        &source.text[source.excerpt.len()..],
                        width,
                        Style::default(),
                    );
                }
            }
            current = source.parent;
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
                "{} · Refresh failed · r Retry",
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
    fn append_status(&self, lines: &mut Vec<Line<'static>>, width: usize) {
        if let Progress::Failed(_, error) = &self.progress {
            append(
                lines,
                &error.message,
                width,
                Style::default().fg(Color::Yellow),
            );
        } else if self.result.is_none() {
            append(
                lines,
                if self.pending() {
                    "Working…"
                } else {
                    "M Settings"
                },
                width,
                Style::default(),
            );
        }
    }
}
