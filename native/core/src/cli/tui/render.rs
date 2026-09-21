use super::{model::display, App, Overlay};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// Wrap explicitly so scrolling and cursor coordinates use the same Unicode cell
// widths, without lossy byte slicing or an unbounded widget scroll offset.
fn wrap(text: &str, width: usize, limit: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = vec![];
    for line in text.split('\n') {
        let mut row = String::new();
        let mut cells = 0;
        for g in line.graphemes(true) {
            let n = g.width();
            if cells + n > width && !row.is_empty() {
                rows.push(std::mem::take(&mut row));
                cells = 0;
                if rows.len() >= limit {
                    return rows;
                }
            }
            row.push_str(g);
            cells += n;
        }
        rows.push(row);
        if rows.len() >= limit {
            return rows;
        }
    }
    rows
}
fn modal(area: Rect) -> Rect {
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(4);
    Rect::new(area.x + (area.width - width) / 2, area.y + 2, width, height)
}
fn block(title: impl Into<Line<'static>>, accent: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(title)
}
const HELP:&str="ShadowCode · native terminal\n\nEnter              Send task / queue a follow-up\nAlt-Enter / Ctrl-J  Insert newline\nF1                 Keyboard help\nF2                 Model picker\nF3 / Ctrl-L        Build / Plan / Review / Test\nF4                 Review pending approval (defaults to Deny)\nTab                Slash command picker\nCtrl-P             Saved conversations\nCtrl-G             Projects\nCtrl-N             New conversation\nCtrl-T             Review project trust\nCtrl-O             Expand / collapse tool output\nPage Up / Down     Scroll conversation\nCtrl-B / Ctrl-F    Older saved history / return to live\nUp / Down          Recall sent prompts\nCtrl-C / Esc       Stop selected task\nCtrl-Q / Ctrl-D    Quit\n\nApproval dialog: Tab selects Deny or Allow; Enter applies.\nPage Up / Down scrolls the exact request. Esc closes it.\nSession picker: type to filter; Ctrl-R searches saved history.\n/export /absolute/file.md exports the full conversation.\n/open /absolute/project switches project after owned work ends.\n\nClosing this terminal cancels its unfinished owned tasks.\nWhen attached to another engine, unrelated tasks continue.";
fn trust_text(path: &std::path::Path) -> String {
    format!("Trust this exact project?\n\n{}\n\nProject instructions and enabled integrations can influence model tasks. Command approvals still apply.",display(&path.display().to_string()))
}
pub(super) fn clamp_dialog_scroll(area: Rect, app: &mut App) {
    let popup = modal(area);
    let width = popup.width.saturating_sub(2) as usize;
    let height = popup.height.saturating_sub(2) as usize;
    match &mut app.overlay {
        Overlay::Help { scroll } => {
            *scroll = (*scroll).min(wrap(HELP, width, 10_000).len().saturating_sub(height))
        }
        Overlay::Trust { path, scroll } => {
            *scroll = (*scroll).min(
                wrap(&trust_text(path), width, 10_000)
                    .len()
                    .saturating_sub(height),
            )
        }
        Overlay::Approval { text, scroll, .. } => {
            *scroll = (*scroll).min(
                (wrap(text, width, 128_001).len() + usize::from(text.len() > 128_000))
                    .saturating_sub(height),
            )
        }
        _ => {}
    }
}
pub(super) fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let light = app.view.theme == "light";
    let bg = if light {
        Color::Rgb(246, 248, 252)
    } else {
        Color::Rgb(17, 20, 28)
    };
    let fg = if light {
        Color::Rgb(28, 34, 48)
    } else {
        Color::Rgb(223, 229, 240)
    };
    let accent = if light {
        Color::Rgb(67, 73, 172)
    } else {
        Color::Rgb(145, 151, 255)
    };
    frame.render_widget(Block::default().style(Style::default().bg(bg).fg(fg)), area);
    if area.width < 36 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("Resize terminal to at least 36 × 12.\nCtrl-Q quits safely."),
            area,
        );
        return;
    }
    let editor_rows = wrap(
        &display(&app.editor.text),
        area.width.saturating_sub(4) as usize,
        64_001,
    );
    let input_height = (editor_rows.len() as u16 + 2).clamp(3, 7);
    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(2),
        Constraint::Length(2),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .split(area);
    let mode = ["BUILD", "PLAN", "REVIEW", "TEST"][app.mode];
    let header = vec![
        Line::from(vec![
            Span::styled(
                " ◆ SHADOWCODE ",
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {mode} · {}", display(&app.view.model))),
        ]),
        Line::from(format!(
            " {} · {} · {}",
            display(&app.view.workspace.display().to_string()),
            if app.view.trusted {
                "trusted"
            } else {
                "untrusted"
            },
            display(&app.view.permission)
        )),
    ];
    frame.render_widget(Paragraph::new(header), parts[0]);
    let body = parts[1];
    let width = body.width.saturating_sub(4) as usize;
    let mut lines: Vec<Line> = vec![];
    if app.view.transcript.trimmed || app.view.older {
        lines.push(Line::styled(
            "Saved history window · Ctrl-B older · Ctrl-F live",
            Style::default().fg(accent),
        ));
    }
    if app.view.transcript.cards.is_empty() {
        for line in wrap("Start a conversation with your local model. Tasks, permissions and history are shared with the native desktop. F1 shows controls.",width,20){lines.push(Line::raw(line));}
    }
    for card in &app.view.transcript.cards {
        lines.push(Line::styled(
            card.title.clone(),
            Style::default()
                .fg(if card.failed { Color::Red } else { accent })
                .add_modifier(Modifier::BOLD),
        ));
        if card.tool && !app.expanded {
            lines.push(Line::raw("  Tool details collapsed · Ctrl-O to expand"));
        } else {
            let rows = wrap(&card.body, width, 400);
            let limited = rows.len() == 400;
            for row in rows {
                lines.push(Line::raw(row));
            }
            if limited {
                lines.push(Line::raw(
                    "[Display shortened · export conversation for full content]",
                ));
            }
        }
        lines.push(Line::raw(""));
    }
    let visible = body.height.saturating_sub(2) as usize;
    let bottom = lines.len().saturating_sub(visible);
    let start = bottom.saturating_sub(app.scroll.min(bottom));
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(start)
                .take(visible)
                .collect::<Vec<_>>(),
        )
        .block(block(format!(" {} ", display(&app.view.title)), accent)),
        body,
    );
    let status = if !app.view.approvals.is_empty() {
        format!(
            "{} approval(s) waiting · F4 to review",
            app.view.approvals.len()
        )
    } else if app.view.busy {
        "Working with engine…".into()
    } else {
        format!(
            "{}{}",
            app.view.job["status"].as_str().unwrap_or("Ready"),
            if app.view.older {
                " · viewing saved history"
            } else {
                ""
            }
        )
    };
    let error = if !app.notice.is_empty() {
        &app.notice
    } else {
        &app.view.error
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(status, Style::default().fg(accent)),
            Line::styled(display(error), Style::default().fg(Color::Red)),
        ]),
        parts[2],
    );
    let prefix = display(&app.editor.text[..app.editor.cursor]);
    let cursor_rows = wrap(&prefix, width, 64_001);
    let mut cy = cursor_rows.len().saturating_sub(1);
    let mut cx = cursor_rows.last().map_or(0, |l| l.width());
    if cx >= width {
        cy += 1;
        cx = 0;
    }
    let input_visible = parts[3].height.saturating_sub(2) as usize;
    let input_start = cy.saturating_sub(input_visible.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(
            editor_rows
                .into_iter()
                .skip(input_start)
                .take(input_visible)
                .map(Line::raw)
                .collect::<Vec<_>>(),
        )
        .block(block(
            format!(" Message · {} ", mode.to_lowercase()),
            accent,
        )),
        parts[3],
    );
    frame.render_widget(
        Paragraph::new(" F1 help · F2 models · F3 mode · F4 approvals · Ctrl-Q quit")
            .style(Style::default().fg(accent)),
        parts[4],
    );
    if matches!(app.overlay, Overlay::None) {
        frame.set_cursor_position((
            parts[3].x + 1 + cx as u16,
            parts[3].y + 1 + (cy - input_start) as u16,
        ));
        return;
    }
    let popup = modal(area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default().style(Style::default().bg(bg).fg(fg)),
        popup,
    );
    let inner_width = popup.width.saturating_sub(2) as usize;
    let height = popup.height.saturating_sub(2) as usize;
    let (title, rows) = match &app.overlay {
        Overlay::Help { scroll } => (
            " Help · PgUp/PgDn · Home · Esc ".to_string(),
            wrap(HELP, inner_width, 10_000)
                .into_iter()
                .skip(*scroll)
                .collect(),
        ),
        Overlay::Trust { path, scroll } => (
            if path == &app.view.workspace {
                " Trust · Enter confirms · PgUp/PgDn · Esc "
            } else {
                " Project changed · Esc to review again "
            }
            .into(),
            wrap(&trust_text(path), inner_width, 10_000)
                .into_iter()
                .skip(*scroll)
                .collect(),
        ),
        Overlay::Approval {
            id,
            text,
            allow,
            scroll,
        } => {
            let current = app.view.approvals.iter().any(|a| a["id"] == *id);
            let too_large = text.len() > 128_000;
            let title = format!(
                " {} · Tab switches · Enter applies · Esc closes ",
                if !current {
                    "Request no longer pending"
                } else if *allow {
                    "ALLOW"
                } else {
                    "DENY"
                }
            );
            let mut rows = if too_large {
                vec!["Request exceeds terminal review limit; Allow is disabled.".into()]
            } else {
                vec![]
            };
            rows.extend(wrap(text, inner_width, 128_001));
            let max = rows.len().saturating_sub(height);
            (title, rows.into_iter().skip((*scroll).min(max)).collect())
        }
        Overlay::Picker {
            kind,
            query,
            selected,
        } => {
            let choices = app.choices(*kind, &query.text);
            let start = selected.saturating_sub(height.saturating_sub(3));
            let mut rows = vec![
                format!("Filter: {}", display(&query.text)),
                "↑↓ choose · Enter selects · Esc closes · Ctrl-R searches sessions".into(),
            ];
            if choices.is_empty() {
                rows.push(
                    if app.view.busy {
                        "Loading…"
                    } else {
                        "No matching entries"
                    }
                    .into(),
                );
            }
            rows.extend(
                choices
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(height.saturating_sub(2))
                    .map(|(i, c)| {
                        format!(
                            "{} {}",
                            if i == *selected { "›" } else { " " },
                            display(&c.label)
                        )
                    }),
            );
            (format!(" {kind:?} "), rows)
        }
        Overlay::None => unreachable!(),
    };
    frame.render_widget(
        Paragraph::new(
            rows.into_iter()
                .take(height)
                .map(Line::raw)
                .collect::<Vec<_>>(),
        )
        .block(block(title, accent)),
        popup,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_wrap_preserves_graphemes() {
        assert_eq!(
            wrap("a界e\u{301}\n👩‍💻", 3, 20),
            vec!["a界", "e\u{301}", "👩‍💻"]
        );
        assert_eq!(wrap(&"x\n".repeat(100_000), 20, 400).len(), 400);
    }
    #[test]
    fn compact_dialogs_reveal_the_last_line_and_clamp_after_resize() {
        let mut app = App::new(Default::default());
        app.overlay = Overlay::Help { scroll: usize::MAX };
        let backend = ratatui::backend::TestBackend::new(36, 12);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                clamp_dialog_scroll(f.area(), &mut app);
                draw(f, &app)
            })
            .unwrap();
        let content = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            content.contains("tasks continue."),
            "The bottom of help must remain reachable: {content}"
        );
        let previous = match app.overlay {
            Overlay::Help { scroll } => scroll,
            _ => unreachable!(),
        };
        clamp_dialog_scroll(Rect::new(0, 0, 120, 80), &mut app);
        assert!(matches!(app.overlay, Overlay::Help { scroll: 0 }));
        assert!(previous > 0);
        app.overlay = Overlay::Trust {
            path: std::path::PathBuf::from(format!("/{}", "long-path/".repeat(100))),
            scroll: usize::MAX,
        };
        clamp_dialog_scroll(Rect::new(0, 0, 36, 12), &mut app);
        assert!(matches!(app.overlay,Overlay::Trust{scroll,..} if scroll>0 && scroll<1000));
    }
    #[test]
    fn renders_compact_and_full_terminals() {
        for (w, h) in [(0, 0), (20, 5), (36, 12), (100, 35)] {
            let backend = ratatui::backend::TestBackend::new(w, h);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let app = App::new(Default::default());
            terminal.draw(|f| draw(f, &app)).unwrap();
        }
    }
}
