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
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
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
                b.push(("M", "models"));
                b.push(("T", "todos"));
                b.push(("L", "lineage"));
                b.push(("/", "search"));
                b.push(("?", "help"));
                b.push(("Tab", "detail"));
                b.push(("q", "quit"));
                b
            }
            (View::Explorer, Focus::Detail) | (View::Detail, _) => {
                let has_runs = !state.runs.is_empty();
                let mut b = vec![
                    ("Esc", "back"),
                    ("j/k", "scroll"),
                ];
                if has_runs {
                    b.push(("h/l", "cycle run"));
                    b.push(("S/I", "tabs"));
                    b.push(("x", "delete"));
                }
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
                ("C/D", "switch view"),
                ("Tab", "selection"),
                ("q", "quit"),
            ],
            (View::Registry, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Enter", "go to run"),
                ("L", "lineage"),
                ("q", "quit"),
            ],
            (View::TodoGlobal, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Space", "toggle"),
                ("x", "delete"),
                ("0/1/2", "priority"),
                ("a", "add"),
                ("q", "quit"),
            ],
            (View::Lineage, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Enter", "go to entity"),
                ("q", "quit"),
            ],
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
