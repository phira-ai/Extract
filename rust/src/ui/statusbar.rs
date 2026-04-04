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
        let bindings = match (state.current_view, state.focus) {
            (View::Explorer, Focus::Tree) => {
                let mut b = vec![
                    ("q", "quit"),
                    ("j/k", "navigate"),
                    ("Enter", "select"),
                    ("Space", "mark"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                }
                if n_marked == 2 {
                    b.push(("d", "diff"));
                }
                b.push(("Tab", "focus detail"));
                b
            }
            (View::Detail, _) | (View::Explorer, Focus::Detail) => {
                let mut b = vec![
                    ("Esc", "back"),
                    ("Tab", "switch tab"),
                    ("j/k", "navigate"),
                    ("Space", "mark"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                }
                if n_marked == 2 {
                    b.push(("d", "diff"));
                }
                b.push(("q", "quit"));
                b
            }
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

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().fg(self.theme.accent_dim));
        frame.render_widget(bar, area);
    }
}
