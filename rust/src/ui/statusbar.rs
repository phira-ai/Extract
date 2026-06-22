use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{AppState, Focus, View};
use crate::ui::detail::DetailTab;
use crate::ui::theme::Theme;

pub(crate) struct StatusBar {
    theme: Theme,
}

impl StatusBar {
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }

    fn action_hints<'a>(&self, state: &AppState, detail_tab: DetailTab) -> Vec<(&'a str, &'a str)> {
        match (state.current_view, state.focus) {
            (View::Explorer, Focus::Tree) => {
                let is_archived = state
                    .selected_experiment
                    .and_then(|idx| state.experiments.get(idx))
                    .map(|e| e.status == "archived")
                    .unwrap_or(false);

                if is_archived {
                    vec![("S-U", "unarchive")]
                } else {
                    vec![
                        ("Space", "mark"),
                        ("x", "delete"),
                        ("S-A", "archive"),
                    ]
                }
            }
            (View::Explorer, Focus::Detail) | (View::Detail, _) => {
                let run_status = state
                    .selected_run
                    .and_then(|i| state.runs.get(i))
                    .map(|r| r.status.as_str());

                match run_status {
                    Some("archived") => {
                        let mut b = vec![];
                        if detail_tab == DetailTab::Summary {
                            b.push(("S-r", "rename"));
                        }
                        b.push(("S-U", "unarchive"));
                        b
                    }
                    Some("running") => {
                        let mut b = vec![];
                        if detail_tab == DetailTab::Summary {
                            b.push(("S-r", "rename"));
                            b.push(("t", "tags"));
                        }
                        b.push(("n", "note"));
                        b.push(("C-e", "edit notes"));
                        b.push(("S-F", "fail"));
                        b.push(("S-C", "complete"));
                        b
                    }
                    Some("completed") | Some("failed") => {
                        let mut b = Vec::new();
                        if detail_tab == DetailTab::Summary {
                            b.push(("S-r", "rename"));
                            b.push(("t", "tags"));
                        }
                        b.push(("n", "note"));
                        b.push(("C-e", "edit notes"));
                        b.push(("S-A", "archive"));
                        b
                    }
                    _ => {
                        // Experiment node selected (no run)
                        vec![("S-A", "archive")]
                    }
                }
            }
            (View::Explorer, Focus::Selection) => vec![
                ("Space", "unmark"),
                ("b", "baseline"),
            ],
            (View::TodoGlobal, _) => vec![
                ("Space", "toggle"),
                ("a", "add"),
                ("x", "delete"),
                ("0/1/2", "priority"),
            ],
            (View::Compare, _) | (View::Diff, _) => vec![
                ("b", "baseline"),
            ],
            _ => vec![],
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState, detail_tab: DetailTab) {
        let hints = self.action_hints(state, detail_tab);

        let mut spans = Vec::new();
        spans.push(Span::raw(" "));

        for (i, (key, desc)) in hints.iter().enumerate() {
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

        // ● LIVE indicator — visible whenever any run in the store is actively
        // running, regardless of which experiment the user is currently viewing.
        // Falls back to the in-memory check if the DB query fails (defensive).
        let any_running = state
            .db
            .has_running_runs()
            .unwrap_or_else(|_| state.runs.iter().any(|r| r.status == "running"));
        if any_running {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "\u{25cf} LIVE",
                Style::default()
                    .fg(self.theme.success)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        // SHOW ALL badge when show_archived is on
        if state.show_archived {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "SHOW ALL",
                Style::default()
                    .fg(self.theme.warning)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().fg(self.theme.accent_dim));
        frame.render_widget(bar, area);
    }
}
