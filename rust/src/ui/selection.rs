use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, DeleteConfirmState, Focus};
use crate::keys;
use crate::ui::theme::Theme;

pub struct SelectionWindow {
    theme: Theme,
}

impl SelectionWindow {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, key: &KeyEvent, state: &mut AppState) {
        if keys::matches(key, keys::BACK_ESC) || keys::matches(key, keys::TAB) {
            state.focus = Focus::Tree;
            return;
        }

        let len = state.selected_runs_for_compare.len();
        if len == 0 {
            return;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if state.selection_cursor + 1 < len {
                state.selection_cursor += 1;
            }
            return;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.selection_cursor = state.selection_cursor.saturating_sub(1);
            return;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            if state.selection_cursor < len {
                state.selected_runs_for_compare.remove(state.selection_cursor);
                let new_len = state.selected_runs_for_compare.len();
                if new_len == 0 {
                    state.selection_cursor = 0;
                    state.compare_baseline = 0;
                } else {
                    if state.selection_cursor >= new_len {
                        state.selection_cursor = new_len - 1;
                    }
                    if state.compare_baseline >= new_len {
                        state.compare_baseline = 0;
                    }
                }
                state.refresh_marked_experiments();
            }
            return;
        }

        if keys::matches(key, keys::BASELINE) {
            state.compare_baseline = state.selection_cursor;
            return;
        }

        if keys::matches(key, keys::DELETE) {
            if state.selection_cursor < len {
                let run_id = state.selected_runs_for_compare[state.selection_cursor].clone();
                let label = run_label(&run_id, state);
                state.delete_confirm = Some(DeleteConfirmState { run_id, label });
            }
            return;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let runs = &state.selected_runs_for_compare;
        if runs.is_empty() {
            return;
        }

        let n_runs = runs.len();
        let width: u16 = 35;
        let height: u16 = (n_runs as u16) + 2; // +2 for borders

        // Position at bottom-right of area, leaving 1 row above status bar
        if area.width < width || area.height < height {
            return;
        }
        let x = area.x + area.width - width;
        let y = area.y + area.height - height;
        let rect = Rect::new(x, y, width, height);

        // Clear background
        frame.render_widget(Clear, rect);

        let focused = state.focus == Focus::Selection;
        let border_color = if focused {
            self.theme.border_focused
        } else {
            self.theme.border
        };

        let block = Block::bordered()
            .title(" Selected ")
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(rect);

        let mut lines: Vec<Line> = Vec::with_capacity(n_runs);
        for (i, run_id) in runs.iter().enumerate() {
            let label = run_label(run_id, state);
            let is_baseline = i == state.compare_baseline;
            let prefix = if is_baseline { "\u{2605} " } else { "\u{00b7} " };

            let mut style = if is_baseline {
                Style::default().fg(ratatui::style::Color::Yellow)
            } else {
                Style::default()
            };

            if focused && i == state.selection_cursor {
                style = self.theme.selected;
            }

            let line = Line::from(Span::styled(format!("{prefix}{label}"), style));
            lines.push(line);
        }

        frame.render_widget(block, rect);
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn run_label(run_id: &str, state: &AppState) -> String {
    if let Ok(Some(run)) = state.db.get_run(run_id) {
        if let Some(ref name) = run.name {
            return name.clone();
        }
        if let Ok(Some(exp)) = state.db.get_experiment(&run.experiment_id) {
            return exp.name.clone();
        }
        // Fall back to short ID
        let id = &run.id;
        if id.len() > 8 {
            id[id.len() - 8..].to_string()
        } else {
            id.clone()
        }
    } else {
        // Can't look up run, use short ID
        if run_id.len() > 8 {
            run_id[run_id.len() - 8..].to_string()
        } else {
            run_id.to_string()
        }
    }
}
