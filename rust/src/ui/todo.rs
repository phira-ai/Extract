use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, NotifyLevel, TodoFilter, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct TodoView {
    pub theme: Theme,
}

impl TodoView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    /// Resolve the scope (type + id) for a new TODO based on the active filter.
    fn resolve_scope(state: &AppState) -> (String, Option<String>) {
        match state.todo_filter {
            TodoFilter::All | TodoFilter::Global => ("global".to_string(), None),
            TodoFilter::Experiment => {
                let id = state.selected_experiment
                    .and_then(|i| state.experiments.get(i))
                    .map(|e| e.id.clone());
                ("experiment".to_string(), id)
            }
            TodoFilter::Run => {
                let id = state.selected_run
                    .and_then(|i| state.runs.get(i))
                    .map(|r| r.id.clone());
                ("run".to_string(), id)
            }
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        let AppEvent::Key(key) = event else {
            return Action::None;
        };

        // Text input mode (popup)
        if state.todo_input.is_some() {
            match key.code {
                crossterm::event::KeyCode::Enter => {
                    let content = state.todo_input.take().unwrap_or_default();
                    if !content.trim().is_empty() {
                        let db_path = state.store_root.join("extract.db");
                        let (scope_type, scope_id) = Self::resolve_scope(state);
                        match crate::db::Db::add_todo(&db_path, &content, 0, &scope_type, scope_id.as_deref()) {
                            Ok(()) => {
                                state.notify(NotifyLevel::Success, format!("TODO added ({scope_type})"));
                                let _ = state.load_todo_data();
                            }
                            Err(e) => {
                                state.notify(NotifyLevel::Error, format!("Failed to add: {e}"));
                            }
                        }
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Esc => {
                    state.todo_input = None;
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

        // Normal mode
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

        if keys::matches(key, keys::ADD) {
            // For experiment/run scopes, require a selection
            match state.todo_filter {
                TodoFilter::Experiment => {
                    if state.selected_experiment.is_none() {
                        state.notify(NotifyLevel::Warn, "Select an experiment first");
                        return Action::None;
                    }
                }
                TodoFilter::Run => {
                    if state.selected_run.is_none() || state.runs.is_empty() {
                        state.notify(NotifyLevel::Warn, "Select a run first");
                        return Action::None;
                    }
                }
                _ => {}
            }
            state.todo_input = Some(String::new());
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
                        state.notify(NotifyLevel::Error, format!("Failed to set priority: {e}"));
                    }
                }
            }
            return Action::None;
        }

        // Filter tabs: A/G/E/R (shifted) to switch scope
        let new_filter = match key.code {
            crossterm::event::KeyCode::Char('A') if key.modifiers == crossterm::event::KeyModifiers::SHIFT => Some(TodoFilter::All),
            crossterm::event::KeyCode::Char('G') if key.modifiers == crossterm::event::KeyModifiers::SHIFT => Some(TodoFilter::Global),
            crossterm::event::KeyCode::Char('E') if key.modifiers == crossterm::event::KeyModifiers::SHIFT => Some(TodoFilter::Experiment),
            crossterm::event::KeyCode::Char('R') if key.modifiers == crossterm::event::KeyModifiers::SHIFT => Some(TodoFilter::Run),
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
                tab_spans.push(Span::styled(" ", Style::default().fg(self.theme.accent_dim)));
            }
            if *filter == state.todo_filter {
                tab_spans.push(Span::styled(
                    label.chars().next().unwrap().to_string(),
                    self.theme.tab_active,
                ));
                tab_spans.push(Span::styled(
                    &label[1..],
                    self.theme.tab_active,
                ));
            } else {
                tab_spans.push(Span::styled(
                    label.chars().next().unwrap().to_string(),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ));
                tab_spans.push(Span::styled(
                    &label[1..],
                    self.theme.tab_inactive,
                ));
            }
        }

        let block = Block::bordered()
            .title(Line::from(
                std::iter::once(Span::raw(" T "))
                    .chain(tab_spans)
                    .chain(std::iter::once(Span::raw(" ")))
                    .collect::<Vec<_>>(),
            ))
            .border_style(Style::default().fg(self.theme.border_focused));

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
            let mut lines: Vec<Line> = Vec::new();

            for (i, todo) in state.todos.iter().enumerate() {
                let is_selected = i == state.todo_cursor;

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

                let priority_span = if is_selected {
                    Span::styled(priority_text, row_style)
                } else {
                    Span::styled(priority_text, priority_style)
                };

                let line = Line::from(vec![
                    priority_span,
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
                ]);

                lines.push(line);
            }

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }

        // Centered input popup
        if let Some(ref input) = state.todo_input {
            let scope_label = match state.todo_filter {
                TodoFilter::All | TodoFilter::Global => " New TODO (global) ".to_string(),
                TodoFilter::Experiment => {
                    let name = state.selected_experiment
                        .and_then(|i| state.experiments.get(i))
                        .map(|e| e.name.as_str())
                        .unwrap_or("?");
                    format!(" New TODO (exp:{name}) ")
                }
                TodoFilter::Run => {
                    let name = state.selected_run
                        .and_then(|i| state.runs.get(i))
                        .and_then(|r| r.name.as_deref())
                        .unwrap_or("?");
                    format!(" New TODO (run:{name}) ")
                }
            };

            let popup_width = 50u16.min(area.width.saturating_sub(4));
            let popup_height = 3u16;
            let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
            let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
            let popup_area = Rect::new(x, y, popup_width, popup_height);

            frame.render_widget(Clear, popup_area);

            let popup_block = Block::bordered()
                .title(scope_label)
                .border_style(Style::default().fg(self.theme.accent));
            let popup_inner = popup_block.inner(popup_area);
            frame.render_widget(popup_block, popup_area);

            let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
            let line = Line::from(vec![Span::raw(input.as_str()), cursor]);
            frame.render_widget(Paragraph::new(line), popup_inner);
        }
    }
}
