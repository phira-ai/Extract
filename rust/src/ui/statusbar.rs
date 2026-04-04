use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{AppState, Focus, View};
use crate::ui::theme::Theme;

pub struct StatusBar {
    theme: Theme,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let n_marked = state.selected_runs_for_compare.len();
        let bindings: Vec<(&str, &str)> = match (state.current_view, state.focus) {
            (View::Explorer, Focus::Tree) => {
                let mut b = vec![
                    ("j/k", "navigate"),
                    ("Enter", "select"),
                    ("Space", "mark"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                    b.push(("d", "diff"));
                }
                if n_marked > 0 {
                    b.push(("Tab", "selection"));
                } else {
                    b.push(("Tab", "detail"));
                }
                b.push(("q", "quit"));
                b
            }
            (View::Explorer, Focus::Detail) | (View::Detail, _) => {
                let mut b = vec![
                    ("Esc", "back"),
                    ("j/k", "scroll"),
                    ("Space", "mark"),
                    ("[/]", "cycle run"),
                    ("x", "delete"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                    b.push(("d", "diff"));
                }
                b.push(("Tab", "next"));
                b.push(("q", "quit"));
                b
            }
            (View::Explorer, Focus::Selection) => vec![
                ("j/k", "navigate"),
                ("Space", "deselect"),
                ("b", "baseline"),
                ("x", "delete"),
                ("Tab/Esc", "back"),
                ("q", "quit"),
            ],
            (View::Compare, _) | (View::Diff, _) => vec![
                ("Esc", "back"),
                ("j/k", "scroll"),
                ("q", "quit"),
            ],
            _ => vec![("q", "quit"), ("Esc", "back")],
        };

        let mut spans = Vec::new();
        spans.push(Span::raw(" "));

        for (i, (key, desc)) in bindings.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    "  ",
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            spans.push(Span::styled(
                format!("[{key}]"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(format!(" {desc}")));
        }

        // Show compare count if any runs are selected
        if !state.selected_runs_for_compare.is_empty() {
            spans.push(Span::styled(
                format!("  | {} marked", state.selected_runs_for_compare.len()),
                Style::default().fg(self.theme.warning),
            ));
        }

        // Show run position in detail view
        if matches!(state.focus, Focus::Detail) || matches!(state.current_view, View::Detail) {
            if let Some(idx) = state.selected_run {
                if state.runs.len() > 1 {
                    spans.push(Span::styled(
                        format!("  run {}/{}", idx + 1, state.runs.len()),
                        Style::default().fg(self.theme.accent_dim),
                    ));
                }
            }
        }

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().fg(self.theme.accent_dim));
        frame.render_widget(bar, area);
    }
}
