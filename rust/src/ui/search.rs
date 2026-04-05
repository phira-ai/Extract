use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, Focus, SearchState, View};
use crate::ui::theme::Theme;

pub struct SearchPopup {
    theme: Theme,
}

impl SearchPopup {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
        }
    }

    /// Handle key events for the search popup.
    /// Returns true if the event was consumed.
    pub fn handle_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        if state.search.is_none() {
            return false;
        }

        match key.code {
            KeyCode::Esc => {
                state.search = None;
                return true;
            }
            KeyCode::Enter => {
                // Extract the selected result before mutating state
                let maybe_result = state.search.as_ref().and_then(|s| {
                    if s.results.is_empty() {
                        None
                    } else {
                        s.results.get(s.cursor).cloned()
                    }
                });

                match maybe_result {
                    None => {
                        state.search = None;
                    }
                    Some(result) => {
                        match result.result_type.as_str() {
                            "experiment" => {
                                if let Some(idx) = state
                                    .experiments
                                    .iter()
                                    .position(|e| e.id == result.id)
                                {
                                    state.selected_experiment = Some(idx);
                                    state.pending_tree_select = Some(result.id.clone());
                                    let _ = state.refresh_runs();
                                    // Auto-select latest run and focus detail
                                    if !state.runs.is_empty() {
                                        let run_idx = state.runs.len() - 1;
                                        state.selected_run = Some(run_idx);
                                        let _ = state.load_run_preview(run_idx);
                                        state.focus = Focus::Detail;
                                    }
                                }
                                state.current_view = View::Explorer;
                                state.search = None;
                            }
                            "run" => {
                                if let Some(exp_id) = result.experiment_id.clone() {
                                    if let Some(exp_idx) = state
                                        .experiments
                                        .iter()
                                        .position(|e| e.id == exp_id)
                                    {
                                        state.selected_experiment = Some(exp_idx);
                                        state.pending_tree_select = Some(exp_id);
                                        let _ = state.refresh_runs();
                                        if let Some(run_idx) = state
                                            .runs
                                            .iter()
                                            .position(|r| r.id == result.id)
                                        {
                                            state.selected_run = Some(run_idx);
                                            let _ = state.load_run_preview(run_idx);
                                        }
                                    }
                                }
                                state.focus = Focus::Detail;
                                state.current_view = View::Explorer;
                                state.search = None;
                            }
                            _ => {
                                state.search = None;
                            }
                        }
                    }
                }
                return true;
            }
            KeyCode::Backspace => {
                if let Some(ref mut search) = state.search {
                    search.query.pop();
                }
                if let Some(ref search) = state.search {
                    let query = search.query.clone();
                    let results = state.db.search(&query).unwrap_or_default();
                    if let Some(ref mut search) = state.search {
                        search.results = results;
                        search.cursor = 0;
                    }
                }
                return true;
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(ref mut search) = state.search {
                    if !search.results.is_empty()
                        && search.cursor + 1 < search.results.len()
                    {
                        search.cursor += 1;
                    }
                }
                return true;
            }
            KeyCode::Up => {
                if let Some(ref mut search) = state.search {
                    if search.cursor > 0 {
                        search.cursor -= 1;
                    }
                }
                return true;
            }
            KeyCode::Char(c) => {
                if let Some(ref mut search) = state.search {
                    search.query.push(c);
                }
                if let Some(ref search) = state.search {
                    let query = search.query.clone();
                    let results = state.db.search(&query).unwrap_or_default();
                    if let Some(ref mut search) = state.search {
                        search.results = results;
                        search.cursor = 0;
                    }
                }
                return true;
            }
            _ => {}
        }

        // Consume all keys while search popup is open
        true
    }

    /// Render the search popup.
    pub fn render(&self, frame: &mut Frame, area: Rect, search: &SearchState) {
        let result_count = search.results.len();
        let height = ((result_count as u16) + 4)
            .min(area.height.saturating_sub(4));
        let width = 60u16.min(area.width.saturating_sub(4));

        // Position near top, centered horizontally
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + 2;
        let popup_area = Rect::new(x, y, width, height.max(3));

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" / Search ")
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 {
            return;
        }

        // First line: prompt + query + blinking cursor
        let prompt = Span::styled(
            " > ",
            Style::default().fg(self.theme.accent),
        );
        let query_text = Span::raw(search.query.clone());
        let cursor = Span::styled(
            "_",
            Style::default().add_modifier(Modifier::SLOW_BLINK),
        );
        let input_line = Line::from(vec![prompt, query_text, cursor]);

        if result_count == 0 {
            if search.query.is_empty() {
                // Empty query: just show input, no results area
                frame.render_widget(Paragraph::new(input_line), inner);
            } else {
                // Non-empty query, no results
                let no_results = Line::from(vec![Span::styled(
                    "  No results",
                    Style::default().fg(self.theme.accent_dim),
                )]);
                let lines = vec![input_line, no_results];
                frame.render_widget(Paragraph::new(lines), inner);
            }
            return;
        }

        // Build lines: input + results
        let mut lines: Vec<Line> = vec![input_line];

        for (i, result) in search.results.iter().enumerate() {
            let is_selected = i == search.cursor;

            let type_tag = match result.result_type.as_str() {
                "experiment" => "[exp]",
                "run" => "[run]",
                _ => "[?]  ",
            };

            let tag_span = Span::styled(
                format!("{} ", type_tag),
                if is_selected {
                    self.theme.selected.add_modifier(Modifier::DIM)
                } else {
                    Style::default().fg(self.theme.accent_dim)
                },
            );

            let label_span = Span::styled(
                format!(" {} ", result.label.clone()),
                if is_selected {
                    self.theme.selected
                } else {
                    Style::default()
                },
            );

            let matched_span = Span::styled(
                result.matched_field.clone(),
                if is_selected {
                    self.theme.selected.add_modifier(Modifier::DIM)
                } else {
                    Style::default().fg(self.theme.accent_dim)
                },
            );

            lines.push(Line::from(vec![tag_span, label_span, matched_span]));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
