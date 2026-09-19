//! Terminal markdown: bold, italic, code, and visible/underlined links.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub fn render(markdown: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut style = Style::default();
    let mut link: Option<String> = None;
    let mut code_block = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::Strong) => style = style.add_modifier(Modifier::BOLD),
            Event::End(TagEnd::Strong) => style = style.remove_modifier(Modifier::BOLD),
            Event::Start(Tag::Emphasis) => style = style.add_modifier(Modifier::ITALIC),
            Event::End(TagEnd::Emphasis) => style = style.remove_modifier(Modifier::ITALIC),
            Event::Start(Tag::Heading { .. }) => style = style.add_modifier(Modifier::BOLD),
            Event::End(TagEnd::Heading(_)) => {
                style = style.remove_modifier(Modifier::BOLD);
                lines.push(Vec::new());
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link = Some(dest_url.to_string());
                style = style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
            }
            Event::End(TagEnd::Link) => {
                if let Some(url) = link.take() {
                    let label = lines
                        .last()
                        .and_then(|row| row.last())
                        .map(|s| s.content.as_ref().to_string())
                        .unwrap_or_default();
                    if !url.is_empty() && label != url {
                        push_span(
                            &mut lines,
                            Span::styled(format!(" {url}"), Style::default().fg(Color::DarkGray)),
                        );
                    }
                }
                style = Style::default();
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code_block = true;
                lines.push(Vec::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                code_block = false;
                lines.push(Vec::new());
            }
            Event::Code(text) => push_span(
                &mut lines,
                Span::styled(text.to_string(), Style::default().fg(Color::Cyan)),
            ),
            Event::Text(text) if code_block => {
                for (i, row) in text.split('\n').enumerate() {
                    if i > 0 {
                        lines.push(Vec::new());
                    }
                    push_span(
                        &mut lines,
                        Span::styled(row.to_string(), Style::default().fg(Color::DarkGray)),
                    );
                }
            }
            Event::Text(text) => push_span(&mut lines, Span::styled(text.to_string(), style)),
            Event::SoftBreak => push_span(&mut lines, Span::raw(" ")),
            Event::HardBreak | Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Item) => {
                lines.push(Vec::new());
            }
            Event::Start(Tag::Item) => push_span(&mut lines, Span::raw("• ")),
            Event::Rule => {
                lines.push(vec![Span::styled(
                    "────────",
                    Style::default().fg(Color::DarkGray),
                )]);
                lines.push(Vec::new());
            }
            _ => {}
        }
    }
    let mut rendered: Vec<Line<'static>> = lines
        .into_iter()
        .map(|spans| {
            if spans.is_empty() {
                Line::default()
            } else {
                Line::from(spans)
            }
        })
        .collect();
    while rendered.last().is_some_and(|l| l.spans.is_empty()) {
        rendered.pop();
    }
    if rendered.is_empty() {
        vec![Line::from(markdown.to_string())]
    } else {
        rendered
    }
}

fn push_span(lines: &mut Vec<Vec<Span<'static>>>, span: Span<'static>) {
    if span.content.is_empty() {
        return;
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines.last_mut().unwrap().push(span);
}

pub fn wrap(lines: Vec<Line<'static>>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let mut out = Vec::new();
    for line in lines {
        if line.spans.is_empty() {
            out.push(Line::default());
            continue;
        }
        let mut row: Vec<Span<'static>> = Vec::new();
        let mut col = 0;
        for span in line.spans {
            let style = span.style;
            for ch in span.content.chars() {
                if col >= width {
                    out.push(Line::from(std::mem::take(&mut row)));
                    col = 0;
                }
                if let Some(last) = row.last_mut() {
                    if last.style == style {
                        last.content.to_mut().push(ch);
                        col += 1;
                        continue;
                    }
                }
                row.push(Span::styled(ch.to_string(), style));
                col += 1;
            }
        }
        out.push(Line::from(row));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plain(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn bold_and_link() {
        let text = plain(&render("**Hello** [docs](https://example.com/a)."));
        assert!(text.contains("Hello"), "{text}");
        assert!(text.contains("docs"), "{text}");
        assert!(text.contains("https://example.com/a"), "{text}");
        assert!(!text.contains("**"), "{text}");
    }
    #[test]
    fn wrap_keeps_text() {
        let wrapped = wrap(render("abcdef"), 3);
        assert_eq!(plain(&wrapped).replace('\n', ""), "abcdef");
        assert_eq!(wrapped.len(), 2);
    }
}
