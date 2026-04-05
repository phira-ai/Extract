use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, NotifyLevel, TodoFilter, TodoScopePicker, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct TodoView {
    pub theme: Theme,
}

impl TodoView {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        let AppEvent::Key(key) = event else {
            return Action::None;
        };

        // --- Scope picker mode ---
        if let Some(ref mut picker) = state.todo_scope_picker {
            match key.code {
                crossterm::event::KeyCode::Esc => {
                    state.todo_scope_picker = None;
                    return Action::None;
                }
                crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                    if !picker.items.is_empty() && picker.cursor + 1 < picker.items.len() {
                        picker.cursor += 1;
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    picker.cursor = picker.cursor.saturating_sub(1);
                    return Action::None;
                }
                crossterm::event::KeyCode::Enter => {
                    if let Some((id, _)) = picker.items.get(picker.cursor) {
                        let scope_type = picker.scope_type.clone();
                        let scope_id = id.clone();
                        state.todo_scope_picker = None;
                        // Store chosen scope and open input
                        state.todo_add_scope = Some((scope_type, Some(scope_id)));
                        state.todo_input = Some(String::new());
                    }
                    return Action::None;
                }
                _ => return Action::None,
            }
        }

        // --- Text input mode (popup) ---
        if state.todo_input.is_some() {
            match key.code {
                crossterm::event::KeyCode::Enter => {
                    let content = state.todo_input.take().unwrap_or_default();
                    if !content.trim().is_empty() {
                        let db_path = state.store_root.join("extract.db");
                        let (scope_type, scope_id) = state
                            .todo_add_scope
                            .take()
                            .unwrap_or_else(|| ("global".to_string(), None));
                        match crate::db::Db::add_todo(
                            &db_path,
                            &content,
                            0,
                            &scope_type,
                            scope_id.as_deref(),
                        ) {
                            Ok(()) => {
                                state.notify(
                                    NotifyLevel::Success,
                                    format!("TODO added ({scope_type})"),
                                );
                                let _ = state.load_todo_data();
                            }
                            Err(e) => {
                                state.notify(NotifyLevel::Error, format!("Failed to add: {e}"));
                            }
                        }
                    } else {
                        state.todo_add_scope = None;
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Esc => {
                    state.todo_input = None;
                    state.todo_add_scope = None;
                    return Action::None;
                }
                crossterm::event::KeyCode::Backspace => {
                    if let Some(ref mut input) = state.todo_input {
                        input.pop();
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Char(c) => {
                    if key.modifiers == crossterm::event::KeyModifiers::NONE
                        || key.modifiers == crossterm::event::KeyModifiers::SHIFT
                    {
                        if let Some(ref mut input) = state.todo_input {
                            input.push(c);
                        }
                    }
                    return Action::None;
                }
                _ => return Action::None,
            }
        }

        // --- Normal mode ---
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.todos.is_empty() && state.todo_cursor + 1 < state.todos.len() {
                state.todo_cursor += 1;
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if state.todo_cursor > 0 {
                state.todo_cursor -= 1;
            }
            return Action::None;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            if let Some(todo) = state.todos.get(state.todo_cursor) {
                let todo_id = todo.id.clone();
                let db_path = state.store_root.join("extract.db");
                match crate::db::Db::toggle_todo(&db_path, &todo_id) {
                    Ok(_) => {
                        let _ = state.load_todo_data();
                    }
                    Err(e) => {
                        state.notify(NotifyLevel::Error, format!("Failed to toggle: {e}"));
                    }
                }
            }
            return Action::None;
        }

        // x: delete selected todo
        if keys::matches(key, keys::DELETE) {
            if let Some(todo) = state.todos.get(state.todo_cursor) {
                let todo_id = todo.id.clone();
                let db_path = state.store_root.join("extract.db");
                match crate::db::Db::delete_todo(&db_path, &todo_id) {
                    Ok(()) => {
                        state.notify(NotifyLevel::Success, "TODO deleted");
                        let _ = state.load_todo_data();
                    }
                    Err(e) => {
                        state.notify(NotifyLevel::Error, format!("Failed to delete: {e}"));
                    }
                }
            }
            return Action::None;
        }

        // a: add new TODO
        if keys::matches(key, keys::ADD) {
            match state.todo_filter {
                TodoFilter::All | TodoFilter::Global => {
                    state.todo_add_scope = Some(("global".to_string(), None));
                    state.todo_input = Some(String::new());
                }
                TodoFilter::Experiment => {
                    // Build list of leaf experiments
                    let leaves: Vec<(String, String)> = state
                        .experiments
                        .iter()
                        .filter(|exp| {
                            !state
                                .experiments
                                .iter()
                                .any(|e| e.parent_id.as_deref() == Some(&exp.id))
                        })
                        .map(|exp| (exp.id.clone(), exp.path.clone()))
                        .collect();
                    if leaves.is_empty() {
                        state.notify(NotifyLevel::Warn, "No experiments found");
                    } else if leaves.len() == 1 {
                        state.todo_add_scope =
                            Some(("experiment".to_string(), Some(leaves[0].0.clone())));
                        state.todo_input = Some(String::new());
                    } else {
                        state.todo_scope_picker = Some(TodoScopePicker {
                            items: leaves,
                            cursor: 0,
                            scope_type: "experiment".to_string(),
                        });
                    }
                }
                TodoFilter::Run => {
                    // Build list of all runs (recent first)
                    let runs: Vec<(String, String)> = state
                        .db
                        .recent_runs(50)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|r| {
                            let label = r.name.clone().unwrap_or_else(|| {
                                state
                                    .db
                                    .get_experiment(&r.experiment_id)
                                    .ok()
                                    .flatten()
                                    .map(|e| e.name)
                                    .unwrap_or_else(|| r.id[r.id.len().saturating_sub(8)..].to_string())
                            });
                            (r.id, label)
                        })
                        .collect();
                    if runs.is_empty() {
                        state.notify(NotifyLevel::Warn, "No runs found");
                    } else if runs.len() == 1 {
                        state.todo_add_scope =
                            Some(("run".to_string(), Some(runs[0].0.clone())));
                        state.todo_input = Some(String::new());
                    } else {
                        state.todo_scope_picker = Some(TodoScopePicker {
                            items: runs,
                            cursor: 0,
                            scope_type: "run".to_string(),
                        });
                    }
                }
            }
            return Action::None;
        }

        // Priority keys: 0/1/2 set priority on selected todo
        if keys::matches(key, keys::PRIORITY_0)
            || keys::matches(key, keys::PRIORITY_1)
            || keys::matches(key, keys::PRIORITY_2)
        {
            let priority = match key.code {
                crossterm::event::KeyCode::Char('0') => 0,
                crossterm::event::KeyCode::Char('1') => 1,
                crossterm::event::KeyCode::Char('2') => 2,
                _ => 0,
            };
            if let Some(todo) = state.todos.get(state.todo_cursor) {
                let todo_id = todo.id.clone();
                let db_path = state.store_root.join("extract.db");
                match crate::db::Db::set_todo_priority(&db_path, &todo_id, priority) {
                    Ok(()) => {
                        let _ = state.load_todo_data();
                    }
                    Err(e) => {
                        state.notify(
                            NotifyLevel::Error,
                            format!("Failed to set priority: {e}"),
                        );
                    }
                }
            }
            return Action::None;
        }

        // Filter tabs: A/G/E/R (shifted) to switch scope
        let new_filter = match key.code {
            crossterm::event::KeyCode::Char('A')
                if key.modifiers == crossterm::event::KeyModifiers::SHIFT =>
            {
                Some(TodoFilter::All)
            }
            crossterm::event::KeyCode::Char('G')
                if key.modifiers == crossterm::event::KeyModifiers::SHIFT =>
            {
                Some(TodoFilter::Global)
            }
            crossterm::event::KeyCode::Char('E')
                if key.modifiers == crossterm::event::KeyModifiers::SHIFT =>
            {
                Some(TodoFilter::Experiment)
            }
            crossterm::event::KeyCode::Char('R')
                if key.modifiers == crossterm::event::KeyModifiers::SHIFT =>
            {
                Some(TodoFilter::Run)
            }
            _ => None,
        };
        if let Some(filter) = new_filter {
            if state.todo_filter != filter {
                state.todo_filter = filter;
                let _ = state.load_todo_data();
            }
            return Action::None;
        }

        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Build tab line for title
        let filters = [
            (TodoFilter::All, "All"),
            (TodoFilter::Global, "Global"),
            (TodoFilter::Experiment, "Experiment"),
            (TodoFilter::Run, "Run"),
        ];
        let mut tab_spans = Vec::new();
        for (i, (filter, label)) in filters.iter().enumerate() {
            if i > 0 {
                tab_spans.push(Span::styled(
                    " ",
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            if *filter == state.todo_filter {
                tab_spans.push(Span::styled(
                    label.chars().next().unwrap().to_string(),
                    self.theme.tab_active,
                ));
                tab_spans.push(Span::styled(&label[1..], self.theme.tab_active));
            } else {
                tab_spans.push(Span::styled(
                    label.chars().next().unwrap().to_string(),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ));
                tab_spans.push(Span::styled(&label[1..], self.theme.tab_inactive));
            }
        }

        let block = Block::bordered()
            .title(Line::from(
                std::iter::once(Span::raw(" TODO "))
                    .chain(tab_spans)
                    .chain(std::iter::once(Span::raw(" ")))
                    .collect::<Vec<_>>(),
            ))
            .border_style(Style::default().fg(self.theme.border_focused))
            .border_set(border::ROUNDED);

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Render the list
        if state.todos.is_empty() {
            let text = Paragraph::new(Line::from(Span::styled(
                "No TODOs. Press 'a' to add one.",
                Style::default().fg(self.theme.accent_dim),
            )));
            frame.render_widget(text, inner);
        } else {
            let visible_height = inner.height as usize;
            let scroll = if state.todo_cursor >= visible_height {
                (state.todo_cursor - visible_height + 1) as u16
            } else {
                0
            };
            let lines: Vec<Line> = state
                .todos
                .iter()
                .enumerate()
                .map(|(i, todo)| self.render_todo_line(i, todo, state))
                .collect();
            frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
        }

        // Scope picker popup
        if let Some(ref picker) = state.todo_scope_picker {
            self.render_scope_picker(frame, area, picker);
        }

        // Text input popup
        if let Some(ref input) = state.todo_input {
            self.render_input_popup(frame, area, input, state);
        }
    }

    fn render_todo_line<'a>(
        &self,
        idx: usize,
        todo: &crate::model::Todo,
        state: &AppState,
    ) -> Line<'a> {
        let is_selected = idx == state.todo_cursor;

        let (priority_text, priority_style) = if todo.priority >= 2 {
            (
                "!! ",
                Style::default()
                    .fg(self.theme.error)
                    .add_modifier(Modifier::BOLD),
            )
        } else if todo.priority == 1 {
            ("!  ", Style::default().fg(self.theme.warning))
        } else {
            ("   ", Style::default())
        };

        let checkbox = if todo.done { "[x] " } else { "[ ] " };

        let scope_label = match todo.scope_type.as_str() {
            "global" => String::new(),
            "experiment" => {
                if let Some(ref sid) = todo.scope_id {
                    let name = state
                        .db
                        .get_experiment(sid)
                        .ok()
                        .flatten()
                        .map(|e| e.name)
                        .unwrap_or_else(|| sid.clone());
                    format!(" [exp:{name}]")
                } else {
                    " [exp]".to_string()
                }
            }
            "run" => {
                if let Some(ref sid) = todo.scope_id {
                    let name = state
                        .db
                        .get_run(sid)
                        .ok()
                        .flatten()
                        .and_then(|r| r.name)
                        .unwrap_or_else(|| {
                            let s = sid.as_str();
                            s[s.len().saturating_sub(8)..].to_string()
                        });
                    format!(" [run:{name}]")
                } else {
                    " [run]".to_string()
                }
            }
            other => format!(" [{other}]"),
        };

        let row_style = if is_selected {
            self.theme.selected
        } else if todo.done {
            Style::default().fg(self.theme.accent_dim)
        } else {
            Style::default()
        };

        Line::from(vec![
            Span::styled(
                priority_text,
                if is_selected { row_style } else { priority_style },
            ),
            Span::styled(checkbox, row_style),
            Span::styled(todo.content.clone(), row_style),
            Span::styled(
                scope_label,
                if is_selected {
                    row_style
                } else {
                    Style::default().fg(self.theme.accent_dim)
                },
            ),
        ])
    }

    fn render_scope_picker(
        &self,
        frame: &mut Frame,
        area: Rect,
        picker: &TodoScopePicker,
    ) {
        let title = if picker.scope_type == "experiment" {
            " Select Experiment "
        } else {
            " Select Run "
        };

        let visible = 10usize.min(picker.items.len());
        let popup_width = 60u16.min(area.width.saturating_sub(4));
        let popup_height = (visible as u16 + 2).min(area.height.saturating_sub(2));
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        // Scroll to keep cursor visible
        let inner_h = inner.height as usize;
        let scroll = if picker.cursor >= inner_h {
            picker.cursor - inner_h + 1
        } else {
            0
        };

        let lines: Vec<Line> = picker
            .items
            .iter()
            .enumerate()
            .skip(scroll)
            .take(inner_h)
            .map(|(i, (_, label))| {
                let style = if i == picker.cursor {
                    self.theme.selected
                } else {
                    Style::default()
                };
                Line::from(Span::styled(format!(" {label}"), style))
            })
            .collect();

        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn render_input_popup(
        &self,
        frame: &mut Frame,
        area: Rect,
        input: &str,
        state: &AppState,
    ) {
        let scope_label = match state.todo_add_scope.as_ref() {
            Some((st, Some(id))) => match st.as_str() {
                "experiment" => {
                    let name = state
                        .db
                        .get_experiment(id)
                        .ok()
                        .flatten()
                        .map(|e| e.name)
                        .unwrap_or_else(|| "?".to_string());
                    format!(" New TODO (exp:{name}) ")
                }
                "run" => {
                    let name = state
                        .db
                        .get_run(id)
                        .ok()
                        .flatten()
                        .and_then(|r| r.name)
                        .unwrap_or_else(|| "?".to_string());
                    format!(" New TODO (run:{name}) ")
                }
                _ => " New TODO (global) ".to_string(),
            },
            _ => " New TODO (global) ".to_string(),
        };

        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = 3u16;
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(scope_label)
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);
        let popup_inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
        let line = Line::from(vec![Span::raw(input), cursor]);
        frame.render_widget(Paragraph::new(line), popup_inner);
    }
}
