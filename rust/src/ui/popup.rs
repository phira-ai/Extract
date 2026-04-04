use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, DeleteConfirmState, RunPickerState};
use crate::keys;
use crate::ui::theme::Theme;

pub struct PopupRenderer {
    theme: Theme,
}

impl PopupRenderer {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    /// Handle key events for the run picker popup.
    /// Returns true when the popup should close.
    pub fn handle_run_picker_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(ref mut picker) = state.run_picker else {
            return false;
        };

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if picker.cursor + 1 < picker.runs.len() {
                picker.cursor += 1;
            }
            return false;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if picker.cursor > 0 {
                picker.cursor -= 1;
            }
            return false;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            let run_id = picker.runs[picker.cursor].id.clone();
            if picker.selected.contains(&run_id) {
                picker.selected.retain(|id| id != &run_id);
            } else {
                picker.selected.push(run_id);
            }
            return false;
        }

        if keys::matches(key, keys::SELECT) || keys::matches(key, keys::BACK_ESC) {
            // Apply selections: add newly selected run IDs, remove deselected ones
            let picker = state.run_picker.take().unwrap();
            let experiment_run_ids: Vec<String> =
                picker.runs.iter().map(|r| r.id.clone()).collect();

            // Remove all runs from this experiment
            state
                .selected_runs_for_compare
                .retain(|id| !experiment_run_ids.contains(id));

            // Add back the selected ones
            for id in &picker.selected {
                if !state.selected_runs_for_compare.contains(id) {
                    state.selected_runs_for_compare.push(id.clone());
                }
            }

            state.refresh_marked_experiments();
            // run_picker is already None from take()
            return true;
        }

        false
    }

    /// Handle key events for the delete confirmation popup.
    /// Returns Some(true) on 'y', Some(false) on any other key.
    pub fn handle_delete_confirm_key(&self, key: &KeyEvent) -> Option<bool> {
        if keys::matches(key, keys::YES) {
            Some(true)
        } else {
            Some(false)
        }
    }

    /// Render the run picker popup.
    pub fn render_run_picker(&self, frame: &mut Frame, area: Rect, picker: &RunPickerState) {
        let height = (picker.runs.len() as u16 + 4).min(area.height.saturating_sub(4));
        let width = 60u16.min(area.width.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let title = format!(" {} — select runs ", picker.experiment_name);
        let block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(self.theme.accent));

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let mut lines: Vec<Line> = Vec::new();
        for (i, run) in picker.runs.iter().enumerate() {
            let is_selected = picker.selected.contains(&run.id);
            let is_cursor = i == picker.cursor;

            let check = if is_selected { "[x] " } else { "[ ] " };

            let date = run
                .ended_at
                .as_deref()
                .unwrap_or(&run.started_at)
                .chars()
                .take(19)
                .collect::<String>();

            let config_summary = run
                .config
                .as_ref()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| {
                    v.as_object().map(|obj| {
                        obj.iter()
                            .take(3)
                            .map(|(k, v)| {
                                let val = match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                format!("{}={}", k, val)
                            })
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                })
                .unwrap_or_default();

            let label = run
                .name
                .as_deref()
                .map(|n| format!("{} ", n))
                .unwrap_or_default();

            let line_style = if is_cursor {
                self.theme.selected
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::styled(check, if is_cursor { line_style } else { Style::default() }),
                Span::styled(
                    "● ",
                    if is_cursor {
                        line_style.fg(match run.status.as_str() {
                            "completed" => self.theme.status_completed.fg.unwrap_or(self.theme.success),
                            "running" => self.theme.status_running.fg.unwrap_or(self.theme.warning),
                            "failed" => self.theme.status_failed.fg.unwrap_or(self.theme.error),
                            _ => self.theme.accent_dim,
                        })
                    } else {
                        match run.status.as_str() {
                            "completed" => self.theme.status_completed,
                            "running" => self.theme.status_running,
                            "failed" => self.theme.status_failed,
                            _ => Style::default().fg(self.theme.accent_dim),
                        }
                    },
                ),
                Span::styled(label, line_style),
                Span::styled(format!("{} ", date), line_style),
                Span::styled(
                    config_summary,
                    if is_cursor {
                        line_style
                    } else {
                        Style::default().fg(self.theme.accent_dim)
                    },
                ),
            ]);
            lines.push(line);
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    /// Render the delete confirmation popup.
    pub fn render_delete_confirm(
        &self,
        frame: &mut Frame,
        area: Rect,
        confirm: &DeleteConfirmState,
    ) {
        let popup_area = centered_rect(44, 5, area);
        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" Delete ")
            .title_bottom(Line::from(vec![
                Span::styled(" [y]", Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)),
                Span::raw(" confirm  "),
                Span::styled("[esc]", Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)),
                Span::raw(" cancel "),
            ]))
            .border_style(Style::default().fg(self.theme.error));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let text = Paragraph::new(Line::from(format!(" Delete run {}?", confirm.label)));
        frame.render_widget(text, inner);
    }
}

/// Create a centered rectangle of the given size within the area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
