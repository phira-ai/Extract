use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Row, Table};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct RegistryView {
    theme: Theme,
}

impl RegistryView {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent, state: &mut AppState) -> Action {
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.models.is_empty() {
                let max = state.models.len() - 1;
                if state.registry_cursor < max {
                    state.registry_cursor += 1;
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.registry_cursor = state.registry_cursor.saturating_sub(1);
            return Action::None;
        }

        if keys::matches(key, keys::SELECT) {
            if let Some(model) = state.models.get(state.registry_cursor).cloned() {
                if let Some(run_id) = &model.run_id {
                    if let Ok(Some(run)) = state.db.get_run(run_id) {
                        let exp_id = run.experiment_id.clone();

                        // Find the experiment index
                        if let Some(exp_idx) = state
                            .experiments
                            .iter()
                            .position(|e| e.id == exp_id)
                        {
                            state.selected_experiment = Some(exp_idx);
                            let _ = state.refresh_runs();

                            // Find the run index within the refreshed runs list
                            if let Some(run_idx) = state.runs.iter().position(|r| r.id == run.id) {
                                state.selected_run = Some(run_idx);
                                let _ = state.load_run_preview(run_idx);
                            }

                            // Tell tree panel to expand and select this experiment
                            state.pending_tree_select = Some(exp_id);
                        }

                        state.current_view = View::Explorer;
                        state.focus = Focus::Detail;
                    }
                }
            }
            return Action::None;
        }

        if keys::matches_shift(key, keys::LINEAGE) {
            let _ = state.load_lineage_data();
            state.current_view = View::Lineage;
            return Action::None;
        }

        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);

        let block = Block::bordered()
            .title(" M Models ")
            .border_style(border_style)
            .border_set(border::ROUNDED);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if state.models.is_empty() {
            let msg = Line::from(Span::styled(
                "No models registered. Use run.register_model() in the Python SDK.",
                Style::default().fg(self.theme.accent_dim),
            ));
            frame.render_widget(
                ratatui::widgets::Paragraph::new(msg),
                inner,
            );
            return;
        }

        let header = Row::new(vec![
            Cell::from("Name").style(self.theme.header),
            Cell::from("Version").style(self.theme.header),
            Cell::from("Framework").style(self.theme.header),
            Cell::from("Run").style(self.theme.header),
            Cell::from("Path").style(self.theme.header),
            Cell::from("Created").style(self.theme.header),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = state
            .models
            .iter()
            .enumerate()
            .map(|(i, model)| {
                let run_label = model
                    .run_id
                    .as_deref()
                    .and_then(|rid| state.db.get_run(rid).ok().flatten())
                    .map(|run| {
                        if let Some(name) = &run.name {
                            name.clone()
                        } else {
                            // Fall back to experiment name
                            state
                                .db
                                .get_experiment(&run.experiment_id)
                                .ok()
                                .flatten()
                                .map(|e| e.name)
                                .unwrap_or_else(|| run.experiment_id.clone())
                        }
                    })
                    .unwrap_or_else(|| "-".to_string());

                let framework = model
                    .framework
                    .as_deref()
                    .unwrap_or("-")
                    .to_string();

                let created = if model.created_at.len() >= 10 {
                    model.created_at[..10].to_string()
                } else {
                    model.created_at.clone()
                };

                let row_style = if i == state.registry_cursor {
                    self.theme.selected
                } else {
                    Style::default()
                };

                Row::new(vec![
                    Cell::from(model.name.clone()),
                    Cell::from(model.version.clone()),
                    Cell::from(framework),
                    Cell::from(run_label),
                    Cell::from(model.artifact_path.clone()),
                    Cell::from(created),
                ])
                .style(row_style)
            })
            .collect();

        let widths = [
            Constraint::Percentage(18),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(18),
            Constraint::Percentage(30),
            Constraint::Percentage(12),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .column_spacing(1);

        frame.render_widget(table, inner);
    }
}
