use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, SelectionSummary, View};
use crate::event::AppEvent;
use crate::keys;
use crate::model::Run;
use crate::ui::summary::{SummaryData, SummaryRenderer};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Summary,
    Info,
}

pub struct DetailPanel {
    pub active_tab: DetailTab,
    summary: SummaryRenderer,
    theme: Theme,
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            active_tab: DetailTab::Summary,
            summary: SummaryRenderer::new(),
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
                DetailTab::Summary => DetailTab::Info,
                DetailTab::Info => DetailTab::Summary,
            };
            return Action::None;
        }

        if keys::matches_shift(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Summary => DetailTab::Info,
                DetailTab::Info => DetailTab::Summary,
            };
            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.focus = Focus::Tree;
            state.current_view = View::Explorer;
            return Action::None;
        }

        // Summary tab: j/k scrolls
        if self.active_tab == DetailTab::Summary {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                if (state.summary_scroll as usize) + 1 < state.summary_total_lines {
                    state.summary_scroll += 1;
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                state.summary_scroll = state.summary_scroll.saturating_sub(1);
                return Action::None;
            }
        }

        // Info tab: j/k navigates between runs
        if self.active_tab == DetailTab::Info {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                if let Some(idx) = state.selected_run {
                    if idx + 1 < state.runs.len() {
                        state.selected_run = Some(idx + 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                if let Some(idx) = state.selected_run {
                    if idx > 0 {
                        state.selected_run = Some(idx - 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
                return Action::None;
            }
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
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

        if keys::matches(key, keys::COMPARE) {
            if state.selected_runs_for_compare.len() >= 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Compare;
                    return Action::Navigate(View::Compare);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::DIFF) {
            if state.selected_runs_for_compare.len() == 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Diff;
                    return Action::Navigate(View::Diff);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    fn load_metrics_for_selected_run(&mut self, state: &mut AppState) {
        if let Some(run_idx) = state.selected_run {
            if let Some(run) = state.runs.get(run_idx) {
                state.metrics = state
                    .db
                    .get_latest_metrics(&run.id)
                    .unwrap_or_default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &mut AppState) {
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

        let selected_run = state.selected_run.and_then(|i| state.runs.get(i).cloned());

        let Some(run) = selected_run else {
            let msg = Paragraph::new("Select an experiment and run to view details.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        };

        // Split inner into tab bar + content
        let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);

        self.render_tab_bar(frame, chunks[0]);

        match self.active_tab {
            DetailTab::Summary => self.render_summary(frame, chunks[1], state),
            DetailTab::Info => self.render_info(frame, chunks[1], &run),
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let tabs = [
            ("Summary", DetailTab::Summary),
            ("Info", DetailTab::Info),
        ];

        let spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(label, tab)| {
                let style = if *tab == self.active_tab {
                    self.theme.tab_active
                } else {
                    self.theme.tab_inactive
                };
                vec![
                    Span::raw(" "),
                    Span::styled(format!("[{label}]"), style),
                ]
            })
            .collect();

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        // Build SummaryData from the SelectionSummary::Leaf
        let (name, runs, run_metrics, aggregate_metrics, unique_configs) =
            match &state.selection_summary {
                SelectionSummary::Leaf {
                    name,
                    runs,
                    run_metrics,
                    aggregate_metrics,
                    unique_configs,
                } => (
                    name.clone(),
                    runs.clone(),
                    run_metrics.clone(),
                    aggregate_metrics.clone(),
                    *unique_configs,
                ),
                _ => return,
            };

        let data = SummaryData {
            name: &name,
            runs: &runs,
            run_metrics: &run_metrics,
            aggregate_metrics: &aggregate_metrics,
            unique_configs,
            run_params: &state.run_params,
            metric_histories: &state.metric_histories,
            table: state.cached_table.as_ref(),
            table_title: state.cached_table_title.as_deref(),
            table_axes: state
                .cached_table_axes
                .as_ref()
                .map(|(r, c)| (r.as_str(), c.as_str())),
        };

        let sections = state.config.summary.sections.clone();
        let total = self.summary.render(
            frame,
            area,
            &data,
            &sections,
            state.summary_scroll,
            state.config.summary.curve_width,
            state.config.summary.curve_smooth,
            &state.config.tables,
        );
        state.summary_total_lines = total;
    }

    fn render_info(&self, frame: &mut Frame, area: Rect, run: &Run) {
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("Run ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(run.id.clone()),
        ]));

        if let Some(ref name) = run.name {
            lines.push(Line::from(vec![
                Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(name.clone()),
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
            Span::styled(run.status.clone(), status_style),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Started: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(run.started_at.clone()),
        ]));

        if let Some(ref ended) = run.ended_at {
            lines.push(Line::from(vec![
                Span::styled("Ended: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(ended.clone()),
            ]));
        }

        if let Some(ref hostname) = run.hostname {
            lines.push(Line::from(vec![
                Span::styled("Host: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(hostname.clone()),
            ]));
        }

        if let Some(ref git_sha) = run.git_sha {
            lines.push(Line::from(vec![
                Span::styled("Git SHA: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(git_sha.clone()),
            ]));
        }

        if let Some(ref tags) = run.tags {
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(tags.clone()),
            ]));
        }

        if let Some(ref notes) = run.notes {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Notes: ",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(notes.clone()));
        }

        if let Some(ref config) = run.config {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Config: ",
                Style::default().add_modifier(Modifier::BOLD),
            )));
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
