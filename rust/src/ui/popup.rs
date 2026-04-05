use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, DeleteConfirmState, RunBrowserState, RunPickerState};
use crate::keys;
use crate::ui::theme::Theme;

pub struct PopupRenderer {
    theme: Theme,
}

impl PopupRenderer {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
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
                // Count: already selected from other experiments + this picker's selections
                let other_count = state.selected_runs_for_compare.len()
                    - state.selected_runs_for_compare.iter()
                        .filter(|id| picker.runs.iter().any(|r| r.id == **id))
                        .count();
                if other_count + picker.selected.len() < crate::ui::tree::MAX_COMPARE_RUNS {
                    picker.selected.push(run_id);
                } else {
                    state.notify(
                        crate::app::NotifyLevel::Warn,
                        format!("Max {} runs for compare", crate::ui::tree::MAX_COMPARE_RUNS),
                    );
                }
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

    /// Handle key events for the run browser popup.
    /// Returns true when the popup should close.
    pub fn handle_run_browser_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(ref mut browser) = state.run_browser else {
            return false;
        };

        let is_searching = browser.search_query.is_some();

        // Search mode: handle text input
        if is_searching {
            match key.code {
                KeyCode::Esc => {
                    // Cancel search, restore full list
                    browser.search_query = None;
                    browser.filtered = (0..browser.runs.len()).collect();
                    browser.cursor = 0;
                    browser.scroll_offset = 0;
                    return false;
                }
                KeyCode::Enter => {
                    // Confirm filter, exit search mode but keep filtered results
                    browser.search_query = None;
                    return false;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = browser.search_query {
                        q.pop();
                    }
                    browser.apply_filter();
                    return false;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = browser.search_query {
                        q.push(c);
                    }
                    browser.apply_filter();
                    return false;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !browser.filtered.is_empty() && browser.cursor + 1 < browser.filtered.len() {
                        browser.cursor += 1;
                    }
                    return false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if browser.cursor > 0 {
                        browser.cursor -= 1;
                    }
                    return false;
                }
                _ => return false,
            }
        }

        // Normal mode
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !browser.filtered.is_empty() && browser.cursor + 1 < browser.filtered.len() {
                browser.cursor += 1;
            }
            return false;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if browser.cursor > 0 {
                browser.cursor -= 1;
            }
            return false;
        }

        if keys::matches(key, keys::SEARCH) {
            browser.search_query = Some(String::new());
            return false;
        }

        if keys::matches(key, keys::SELECT) {
            // Select the run under cursor and navigate to it
            if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                state.selected_run = Some(run_idx);
                let _ = state.load_run_preview(run_idx);
                state.metrics = state.runs.get(run_idx)
                    .map(|r| state.db.get_latest_metrics(&r.id).unwrap_or_default())
                    .unwrap_or_default();
                state.focus = crate::app::Focus::Detail;
            }
            state.run_browser = None;
            return true;
        }

        if keys::matches(key, keys::DELETE) {
            if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                if let Some(run) = browser.runs.get(run_idx) {
                    let run_id = run.id.clone();
                    let label = run.name.clone().unwrap_or_else(|| {
                        if run_id.len() > 8 {
                            run_id[run_id.len() - 8..].to_string()
                        } else {
                            run_id.clone()
                        }
                    });
                    state.delete_confirm = Some(DeleteConfirmState { run_id, label });
                }
            }
            return false;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.run_browser = None;
            return true;
        }

        false
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
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

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
            .border_style(Style::default().fg(self.theme.error))
            .border_set(border::ROUNDED);
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
