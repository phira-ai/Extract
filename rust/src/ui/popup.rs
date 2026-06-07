use chrono::{DateTime, Local};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, DeleteConfirmState, RunBrowserState, RunPickerState};
use crate::keys;
use crate::model::Run;
use crate::ui::theme::Theme;

/// Fixed popup dimensions.
const POPUP_WIDTH: u16 = 80;
const POPUP_HEIGHT: u16 = 24;
/// Empty lines at top and bottom inside the popup content area.
const PADDING: u16 = 1;

pub struct PopupRenderer {
    theme: Theme,
}

impl PopupRenderer {
    pub fn new(theme: Theme) -> Self {
        Self { theme }
    }

    // ── Run Picker (Space — mark for compare) ──────────────────────────

    /// Handle key events for the run picker popup.
    /// Returns true when the popup should close.
    pub fn handle_run_picker_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(ref mut picker) = state.run_picker else {
            return false;
        };

        let is_searching = picker.search_query.is_some();

        // Search mode
        if is_searching {
            match key.code {
                KeyCode::Esc => {
                    picker.search_query = None;
                    picker.filtered = (0..picker.runs.len()).collect();
                    picker.cursor = 0;
                    picker.scroll_offset = 0;
                    return false;
                }
                KeyCode::Enter => {
                    // Empty query = clear filter; non-empty = keep filter applied
                    let is_empty = picker.search_query.as_ref()
                        .map_or(true, |q| q.is_empty());
                    picker.search_query = None;
                    if is_empty {
                        picker.filtered = (0..picker.runs.len()).collect();
                        picker.cursor = 0;
                        picker.scroll_offset = 0;
                    }
                    return false;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = picker.search_query {
                        q.pop();
                    }
                    picker.apply_filter();
                    return false;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = picker.search_query {
                        q.push(c);
                    }
                    picker.apply_filter();
                    return false;
                }
                KeyCode::Down => {
                    if !picker.filtered.is_empty() && picker.cursor + 1 < picker.filtered.len() {
                        picker.cursor += 1;
                    }
                    return false;
                }
                KeyCode::Up => {
                    if picker.cursor > 0 {
                        picker.cursor -= 1;
                    }
                    return false;
                }
                _ => return false,
            }
        }

        // Normal mode
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !picker.filtered.is_empty() && picker.cursor + 1 < picker.filtered.len() {
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

        if keys::matches(key, keys::SEARCH) {
            picker.search_query = Some(String::new());
            return false;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            if let Some(&run_idx) = picker.filtered.get(picker.cursor) {
                let run_id = picker.runs[run_idx].id.clone();
                if picker.selected.contains(&run_id) {
                    picker.selected.retain(|id| id != &run_id);
                } else {
                    let other_count = state.selected_runs_for_compare.len()
                        - state
                            .selected_runs_for_compare
                            .iter()
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
            }
            return false;
        }

        if keys::matches(key, keys::SELECT) || keys::matches(key, keys::BACK_ESC) {
            let picker = state.run_picker.take().unwrap();
            let experiment_run_ids: Vec<String> =
                picker.runs.iter().map(|r| r.id.clone()).collect();

            state
                .selected_runs_for_compare
                .retain(|id| !experiment_run_ids.contains(id));

            for id in &picker.selected {
                if !state.selected_runs_for_compare.contains(id) {
                    state.selected_runs_for_compare.push(id.clone());
                }
            }

            state.refresh_marked_experiments();
            return true;
        }

        false
    }

    // ── Delete Confirmation ────────────────────────────────────────────

    /// Handle key events for the delete confirmation popup.
    pub fn handle_delete_confirm_key(&self, key: &KeyEvent) -> Option<bool> {
        if keys::matches(key, keys::YES) {
            Some(true)
        } else {
            Some(false)
        }
    }

    // ── Archive Confirmation ───────────────────────────────────────────

    /// Handle key events for the archive confirmation popup.
    pub fn handle_archive_confirm_key(&self, key: &KeyEvent) -> Option<bool> {
        if keys::matches(key, keys::YES) {
            Some(true)
        } else {
            Some(false)
        }
    }

    pub fn render_archive_confirm(&self, frame: &mut Frame, area: Rect, confirm: &crate::app::ArchiveConfirmState) {
        let msg = format!("Archive '{}' and all descendants?", confirm.label);
        let width = (msg.len() as u16 + 6).min(area.width.saturating_sub(4));
        let height = 5u16;
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" Archive ")
            .border_style(Style::default().fg(self.theme.warning))
            .border_set(border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let lines = vec![
            Line::from(msg),
            Line::from(""),
            Line::from(vec![
                Span::styled("[y]", Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)),
                Span::raw(" confirm  "),
                Span::styled("[any]", Style::default().fg(self.theme.accent_dim)),
                Span::raw(" cancel"),
            ]),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }

    // ── Run Browser (r — navigate to run) ──────────────────────────────

    /// Handle key events for the run browser popup.
    /// Returns true when the popup should close.
    pub fn handle_run_browser_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(ref mut browser) = state.run_browser else {
            return false;
        };

        let is_searching = browser.search_query.is_some();
        let is_renaming = browser.rename_buffer.is_some();

        // Rename mode
        if is_renaming {
            match key.code {
                KeyCode::Esc => {
                    browser.rename_buffer = None;
                    browser.rename_cursor = 0;
                    return false;
                }
                KeyCode::Enter => {
                    if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                        if let Some(run) = browser.runs.get(run_idx) {
                            let run_id = run.id.clone();
                            let new_name = browser.rename_buffer.take().unwrap_or_default();
                            browser.rename_cursor = 0;
                            let db_path = state.store_root.join("extract.db");
                            match crate::db::Db::rename_run(&db_path, &run_id, &new_name) {
                                Ok(()) => {
                                    let trimmed = new_name.trim();
                                    let value = if trimmed.is_empty() {
                                        None
                                    } else {
                                        Some(trimmed.to_string())
                                    };
                                    if let Some(run) = browser.runs.get_mut(run_idx) {
                                        run.name = value.clone();
                                    }
                                    if let Some(state_idx) = state.runs.iter().position(|r| r.id == run_id) {
                                        state.runs[state_idx].name = value;
                                    }
                                    let _ = state.refresh_selection_summary();
                                    state.notify(crate::app::NotifyLevel::Success, "Run renamed");
                                }
                                Err(err) => {
                                    state.notify(
                                        crate::app::NotifyLevel::Error,
                                        format!("Rename failed: {err}"),
                                    );
                                }
                            }
                        } else {
                            browser.rename_buffer = None;
                            browser.rename_cursor = 0;
                        }
                    } else {
                        browser.rename_buffer = None;
                        browser.rename_cursor = 0;
                    }
                    return false;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut name) = browser.rename_buffer {
                        let cursor = browser.rename_cursor.min(name.chars().count());
                        browser.rename_cursor = remove_char_before_cursor(name, cursor);
                    }
                    return false;
                }
                KeyCode::Left => {
                    browser.rename_cursor = browser.rename_cursor.saturating_sub(1);
                    return false;
                }
                KeyCode::Right => {
                    if let Some(ref name) = browser.rename_buffer {
                        let len = name.chars().count();
                        browser.rename_cursor = (browser.rename_cursor + 1).min(len);
                    }
                    return false;
                }
                KeyCode::Char(c) => {
                    if accepts_text_modifiers(key) {
                        if let Some(ref mut name) = browser.rename_buffer {
                            let cursor = browser.rename_cursor.min(name.chars().count());
                            let byte_idx = char_to_byte_index(name, cursor);
                            name.insert(byte_idx, c);
                            browser.rename_cursor = cursor + 1;
                        }
                    }
                    return false;
                }
                _ => return false,
            }
        }

        // Search mode
        if is_searching {
            match key.code {
                KeyCode::Esc => {
                    browser.search_query = None;
                    browser.filtered = (0..browser.runs.len()).collect();
                    browser.cursor = 0;
                    browser.scroll_offset = 0;
                    return false;
                }
                KeyCode::Enter => {
                    let is_empty = browser.search_query.as_ref()
                        .map_or(true, |q| q.is_empty());
                    browser.search_query = None;
                    if is_empty {
                        browser.filtered = (0..browser.runs.len()).collect();
                        browser.cursor = 0;
                        browser.scroll_offset = 0;
                    }
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
                KeyCode::Down => {
                    if !browser.filtered.is_empty()
                        && browser.cursor + 1 < browser.filtered.len()
                    {
                        browser.cursor += 1;
                    }
                    return false;
                }
                KeyCode::Up => {
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

        if is_rename_key(key) {
            if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                if let Some(run) = browser.runs.get(run_idx) {
                    let name = run.name.clone().unwrap_or_default();
                    browser.rename_cursor = name.chars().count();
                    browser.rename_buffer = Some(name);
                }
            }
            return false;
        }

        if keys::matches(key, keys::SELECT) {
            if let Some(&filtered_idx) = browser.filtered.get(browser.cursor) {
                if let Some(run) = browser.runs.get(filtered_idx) {
                    let run_id = run.id.clone();
                    if let Some(state_idx) = state.runs.iter().position(|r| r.id == run_id) {
                        state.selected_run = Some(state_idx);
                        let _ = state.load_run_preview(state_idx);
                        state.metrics =
                            state.db.get_latest_metrics(&run_id).unwrap_or_default();
                        state.focus = crate::app::Focus::Detail;
                    }
                }
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
                    state.delete_confirm = Some(DeleteConfirmState {
                        target: crate::app::DeleteTarget::Run { run_id },
                        label,
                    });
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

    // ── Rendering ──────────────────────────────────────────────────────

    /// Render the run picker popup (Space — select runs for compare).
    pub fn render_run_picker(&self, frame: &mut Frame, area: Rect, picker: &mut RunPickerState) {
        let is_searching = picker.search_query.is_some();
        let width = POPUP_WIDTH.min(area.width.saturating_sub(4));
        let height = POPUP_HEIGHT.min(area.height.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let title = format!(" {} — select runs ", picker.experiment_name);
        let footer_spans = if is_searching {
            search_footer_spans(&self.theme)
        } else {
            vec![
                Span::styled("j/k", Style::default().fg(self.theme.accent)),
                Span::styled(" nav  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Space", Style::default().fg(self.theme.accent)),
                Span::styled(" toggle  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("/", Style::default().fg(self.theme.accent)),
                Span::styled(" search  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Enter", Style::default().fg(self.theme.accent)),
                Span::styled(" confirm  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Esc", Style::default().fg(self.theme.accent)),
                Span::styled(" close", Style::default().fg(self.theme.accent_dim)),
            ]
        };
        let block = Block::bordered()
            .title(title)
            .title_bottom(Line::from(footer_spans))
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let diff_keys = differing_config_keys(&picker.runs);
        let mut lines: Vec<Line> = Vec::new();

        // Top padding
        for _ in 0..PADDING.min(inner.height) {
            lines.push(Line::raw(""));
        }

        // Search input line
        if let Some(ref query) = picker.search_query {
            lines.push(search_input_line(query, &self.theme));
        }

        // Scrollable area
        let reserved = lines.len() + PADDING as usize;
        let list_height = (inner.height as usize).saturating_sub(reserved);
        let scroll = compute_scroll(picker.cursor, picker.scroll_offset, list_height);
        picker.scroll_offset = scroll;

        for (vi, &run_idx) in picker
            .filtered
            .iter()
            .enumerate()
            .skip(scroll)
            .take(list_height)
        {
            let run = &picker.runs[run_idx];
            let is_cursor = vi == picker.cursor;
            let is_selected = picker.selected.contains(&run.id);
            let line_style = if is_cursor {
                self.theme.selected
            } else {
                Style::default()
            };

            let check = if is_selected { "[x] " } else { "[ ] " };
            let label = run
                .name
                .as_deref()
                .map(|n| format!("{} ", n))
                .unwrap_or_default();
            let date = format_date(run);
            let config_summary = differing_config_summary(run, &diff_keys);

            lines.push(Line::from(vec![
                Span::styled(
                    check,
                    if is_cursor {
                        line_style
                    } else {
                        Style::default()
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
            ]));
        }

        // Bottom padding
        for _ in 0..PADDING {
            lines.push(Line::raw(""));
        }

        frame.render_widget(Paragraph::new(lines), inner);
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
                Span::styled(
                    " [y]",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" confirm  "),
                Span::styled(
                    "[esc]",
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" cancel "),
            ]))
            .border_style(Style::default().fg(self.theme.error))
            .border_set(border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let kind = match &confirm.target {
            crate::app::DeleteTarget::Run { .. } => "run",
            crate::app::DeleteTarget::Experiment { .. } => "experiment",
        };
        let text = Paragraph::new(Line::from(format!(" Delete {kind} {}?", confirm.label)));
        frame.render_widget(text, inner);
    }

    /// Render the run browser popup (r — navigate to run).
    pub fn render_run_browser(
        &self,
        frame: &mut Frame,
        area: Rect,
        browser: &mut RunBrowserState,
    ) {
        let is_searching = browser.search_query.is_some();
        let is_renaming = browser.rename_buffer.is_some();
        let width = POPUP_WIDTH.min(area.width.saturating_sub(4));
        let height = POPUP_HEIGHT.min(area.height.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let title = format!(" {} — runs ", browser.experiment_name);
        let footer_spans = if is_searching {
            search_footer_spans(&self.theme)
        } else if is_renaming {
            rename_footer_spans(&self.theme)
        } else {
            vec![
                Span::styled("j/k", Style::default().fg(self.theme.accent)),
                Span::styled(" nav  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Enter", Style::default().fg(self.theme.accent)),
                Span::styled(" select  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("R", Style::default().fg(self.theme.accent)),
                Span::styled(" rename  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("/", Style::default().fg(self.theme.accent)),
                Span::styled(" search  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("x", Style::default().fg(self.theme.accent)),
                Span::styled(" delete  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Esc", Style::default().fg(self.theme.accent)),
                Span::styled(" close", Style::default().fg(self.theme.accent_dim)),
            ]
        };

        let block = Block::bordered()
            .title(title)
            .title_bottom(Line::from(footer_spans))
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let diff_keys = differing_config_keys(&browser.runs);
        let mut lines: Vec<Line> = Vec::new();

        // Top padding
        for _ in 0..PADDING.min(inner.height) {
            lines.push(Line::raw(""));
        }

        // Search input line
        if let Some(ref query) = browser.search_query {
            lines.push(search_input_line(query, &self.theme));
        }

        // Scrollable area
        let reserved = lines.len() + PADDING as usize;
        let list_height = (inner.height as usize).saturating_sub(reserved);
        let scroll = compute_scroll(browser.cursor, browser.scroll_offset, list_height);
        browser.scroll_offset = scroll;

        for (vi, &run_idx) in browser
            .filtered
            .iter()
            .enumerate()
            .skip(scroll)
            .take(list_height)
        {
            let run = &browser.runs[run_idx];
            let is_cursor = vi == browser.cursor;
            let line_style = if is_cursor {
                self.theme.selected
            } else {
                Style::default()
            };

            let date = format_date(run);
            let config_summary = differing_config_summary(run, &diff_keys);
            let mut row_spans = if is_cursor {
                browser
                    .rename_buffer
                    .as_ref()
                    .map(|name| rename_label_spans(name, browser.rename_cursor, line_style))
                    .unwrap_or_else(|| {
                        let label = run
                            .name
                            .as_deref()
                            .map(|n| format!("{} ", n))
                            .unwrap_or_default();
                        vec![Span::styled(format!("  {}", label), line_style)]
                    })
            } else {
                let label = run
                    .name
                    .as_deref()
                    .map(|n| format!("{} ", n))
                    .unwrap_or_default();
                vec![Span::styled(format!("  {}", label), line_style)]
            };
            row_spans.push(Span::styled(format!("{} ", date), line_style));
            row_spans.push(Span::styled(
                config_summary,
                if is_cursor {
                    line_style
                } else {
                    Style::default().fg(self.theme.accent_dim)
                },
            ));

            lines.push(Line::from(row_spans));
        }

        // Bottom padding
        for _ in 0..PADDING {
            lines.push(Line::raw(""));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }
}

// ── Shared helpers ─────────────────────────────────────────────────────

/// Create a centered rectangle of the given size within the area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

/// Compute scroll offset to keep cursor visible within list_height rows.
fn compute_scroll(cursor: usize, current_offset: usize, list_height: usize) -> usize {
    if list_height == 0 {
        return 0;
    }
    if cursor >= current_offset + list_height {
        cursor.saturating_sub(list_height - 1)
    } else if cursor < current_offset {
        cursor
    } else {
        current_offset
    }
}

/// Format a run timestamp for display in the user's local timezone.
fn format_date(run: &Run) -> String {
    let raw = run.ended_at.as_deref().unwrap_or(&run.started_at);
    format_local_timestamp(raw).unwrap_or_else(|| raw.chars().take(19).collect())
}

fn format_local_timestamp(raw: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string())
}

fn is_rename_key(key: &KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('R')) && accepts_text_modifiers(key)
}

fn accepts_text_modifiers(key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT
}

/// Build search input line with blinking cursor.
fn search_input_line<'a>(query: &str, theme: &Theme) -> Line<'a> {
    Line::from(vec![
        Span::styled(" / ", Style::default().fg(theme.accent)),
        Span::raw(query.to_string()),
        Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
    ])
}

/// Footer spans shown during search mode (shared by both popups).
fn search_footer_spans(theme: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled("Type", Style::default().fg(theme.accent)),
        Span::styled(" filter  ", Style::default().fg(theme.accent_dim)),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::styled(" confirm  ", Style::default().fg(theme.accent_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::styled(" cancel", Style::default().fg(theme.accent_dim)),
    ]
}

fn rename_footer_spans(theme: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled("Type", Style::default().fg(theme.accent)),
        Span::styled(" name  ", Style::default().fg(theme.accent_dim)),
        Span::styled("←/→", Style::default().fg(theme.accent)),
        Span::styled(" cursor  ", Style::default().fg(theme.accent_dim)),
        Span::styled("Enter", Style::default().fg(theme.accent)),
        Span::styled(" save  ", Style::default().fg(theme.accent_dim)),
        Span::styled("Esc", Style::default().fg(theme.accent)),
        Span::styled(" cancel", Style::default().fg(theme.accent_dim)),
    ]
}

fn rename_label_spans(name: &str, cursor: usize, line_style: Style) -> Vec<Span<'static>> {
    let cursor = cursor.min(name.chars().count());
    let mut chars = name.chars();
    let before: String = chars.by_ref().take(cursor).collect();
    let cursor_style = line_style.add_modifier(Modifier::REVERSED | Modifier::SLOW_BLINK);
    let mut spans = vec![Span::styled("  ", line_style)];

    if !before.is_empty() {
        spans.push(Span::styled(before, line_style));
    }

    if let Some(cursor_char) = chars.next() {
        spans.push(Span::styled(cursor_char.to_string(), cursor_style));
        let after: String = chars.collect();
        if !after.is_empty() {
            spans.push(Span::styled(after, line_style));
        }
    } else {
        spans.push(Span::styled(" ", cursor_style));
    }

    spans.push(Span::styled(" ", line_style));
    spans
}

fn remove_char_before_cursor(name: &mut String, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }

    let start = char_to_byte_index(name, cursor - 1);
    let end = char_to_byte_index(name, cursor);
    name.replace_range(start..end, "");
    cursor - 1
}

fn char_to_byte_index(value: &str, char_idx: usize) -> usize {
    value
        .char_indices()
        .nth(char_idx)
        .map_or(value.len(), |(byte_idx, _)| byte_idx)
}

/// Compare configs across all runs and return only the keys whose values differ.
fn differing_config_keys(runs: &[Run]) -> Vec<String> {
    use std::collections::HashMap;

    let mut all_keys: Vec<String> = Vec::new();
    let mut values_per_key: HashMap<String, Vec<Option<String>>> = HashMap::new();

    // Collect all config values per key across runs
    for run in runs {
        let parsed = run
            .config
            .as_ref()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());

        if let Some(serde_json::Value::Object(obj)) = &parsed {
            for key in obj.keys() {
                if !all_keys.contains(key) {
                    all_keys.push(key.clone());
                }
            }
        }
    }

    // For each key, collect the value from every run
    for key in &all_keys {
        let mut vals = Vec::with_capacity(runs.len());
        for run in runs {
            let parsed = run
                .config
                .as_ref()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());
            let val = parsed
                .as_ref()
                .and_then(|v| v.get(key))
                .map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
            vals.push(val);
        }
        values_per_key.insert(key.clone(), vals);
    }

    // Keep only keys where not all values are identical
    all_keys
        .into_iter()
        .filter(|key| {
            let vals = &values_per_key[key];
            if vals.is_empty() {
                return false;
            }
            let first = &vals[0];
            vals.iter().any(|v| v != first)
        })
        .collect()
}

/// Build a config summary string showing only the differing keys for this run.
fn differing_config_summary(run: &Run, diff_keys: &[String]) -> String {
    if diff_keys.is_empty() {
        return String::new();
    }
    let parsed = run
        .config
        .as_ref()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok());

    let Some(serde_json::Value::Object(obj)) = parsed else {
        return String::new();
    };

    diff_keys
        .iter()
        .filter_map(|key| {
            obj.get(key).map(|v| {
                let val = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{}={}", key, val)
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}
