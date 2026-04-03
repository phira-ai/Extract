use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::model::Run;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Scalars,
    Info,
}

pub struct DetailPanel {
    pub active_tab: DetailTab,
    pub table_state: TableState,
    theme: Theme,
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            active_tab: DetailTab::Scalars,
            table_state: TableState::default(),
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent, state: &mut AppState) -> Action {
        if keys::matches(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Scalars => DetailTab::Info,
                DetailTab::Info => DetailTab::Scalars,
            };
            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.focus = Focus::Tree;
            state.current_view = View::Explorer;
            return Action::None;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if self.active_tab == DetailTab::Scalars {
                self.table_state.select_next();
            } else {
                // Navigate between runs
                if let Some(idx) = state.selected_run {
                    if idx + 1 < state.runs.len() {
                        state.selected_run = Some(idx + 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if self.active_tab == DetailTab::Scalars {
                self.table_state.select_previous();
            } else {
                // Navigate between runs
                if let Some(idx) = state.selected_run {
                    if idx > 0 {
                        state.selected_run = Some(idx - 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            // Mark current run for comparison
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                let run_id = run.id.clone();
                if state.selected_runs_for_compare.contains(&run_id) {
                    state.selected_runs_for_compare.retain(|id| id != &run_id);
                } else {
                    state.selected_runs_for_compare.push(run_id);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    fn load_metrics_for_selected_run(&self, state: &mut AppState) {
        if let Some(run_idx) = state.selected_run {
            if let Some(run) = state.runs.get(run_idx) {
                state.metrics = state
                    .db
                    .get_latest_metrics(&run.id)
                    .unwrap_or_default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let focused = state.focus == Focus::Detail;
        let border_style = if focused {
            Style::default().fg(self.theme.border_focused)
        } else {
            Style::default().fg(self.theme.border)
        };

        let block = Block::bordered()
            .title(" Detail ")
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // If no run is selected, show a message
        let selected_run = state
            .selected_run
            .and_then(|i| state.runs.get(i));

        let Some(run) = selected_run else {
            let msg = Paragraph::new("Select an experiment and run to view details.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        };

        // Split inner into tab bar + content
        let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);

        // Render tab bar
        self.render_tab_bar(frame, chunks[0]);

        // Render content based on active tab
        match self.active_tab {
            DetailTab::Scalars => self.render_scalars(frame, chunks[1], state),
            DetailTab::Info => self.render_info(frame, chunks[1], run),
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let scalars_style = if self.active_tab == DetailTab::Scalars {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };

        let info_style = if self.active_tab == DetailTab::Info {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };

        let tabs = Line::from(vec![
            Span::raw(" "),
            Span::styled("[Scalars]", scalars_style),
            Span::raw("  "),
            Span::styled("[Info]", info_style),
        ]);

        frame.render_widget(Paragraph::new(tabs), area);
    }

    fn render_scalars(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let header = Row::new(vec!["Metric", "Value", "Step"])
            .style(self.theme.header)
            .bottom_margin(1);

        let rows: Vec<Row> = state
            .metrics
            .iter()
            .map(|m| {
                Row::new(vec![
                    m.name.clone(),
                    format!("{:.6}", m.value),
                    m.step.to_string(),
                ])
            })
            .collect();

        let widths = [
            Constraint::Percentage(40),
            Constraint::Percentage(35),
            Constraint::Percentage(25),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_info(&self, frame: &mut Frame, area: Rect, run: &Run) {
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("Run ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&run.id),
        ]));

        if let Some(ref name) = run.name {
            lines.push(Line::from(vec![
                Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(name.as_str()),
            ]));
        }

        let status_style = match run.status.as_str() {
            "running" => self.theme.status_running,
            "completed" => self.theme.status_completed,
            "failed" => self.theme.status_failed,
            _ => Style::default(),
        };
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(run.status.as_str(), status_style),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Started: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(run.started_at.as_str()),
        ]));

        if let Some(ref ended) = run.ended_at {
            lines.push(Line::from(vec![
                Span::styled("Ended: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(ended.as_str()),
            ]));
        }

        if let Some(ref hostname) = run.hostname {
            lines.push(Line::from(vec![
                Span::styled("Host: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(hostname.as_str()),
            ]));
        }

        if let Some(ref git_sha) = run.git_sha {
            lines.push(Line::from(vec![
                Span::styled("Git SHA: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(git_sha.as_str()),
            ]));
        }

        if let Some(ref tags) = run.tags {
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(tags.as_str()),
            ]));
        }

        if let Some(ref notes) = run.notes {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Notes: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(notes.as_str()));
        }

        if let Some(ref config) = run.config {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Config: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            // Pretty-print JSON config
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(config) {
                if let Ok(pretty) = serde_json::to_string_pretty(&parsed) {
                    for line in pretty.lines() {
                        lines.push(Line::from(format!("  {line}")));
                    }
                } else {
                    lines.push(Line::from(format!("  {config}")));
                }
            } else {
                lines.push(Line::from(format!("  {config}")));
            }
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}
