use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, NotifyLevel, TodoFilter, View};
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

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        let AppEvent::Key(key) = event else {
            return Action::None;
        };

        // Text input mode
        if state.todo_input.is_some() {
            if keys::matches(key, keys::SELECT) {
                // Enter: commit
                let content = state.todo_input.take().unwrap_or_default();
                if !content.trim().is_empty() {
                    let db_path = state.store_root.join("extract.db");
                    match crate::db::Db::add_todo(&db_path, &content, 0) {
                        Ok(()) => {
                            state.notify(NotifyLevel::Success, "TODO added");
                            let _ = state.load_todo_data();
                        }
                        Err(e) => {
                            state.notify(NotifyLevel::Error, format!("Failed to add TODO: {e}"));
                        }
                    }
                }
                return Action::None;
            }

            if keys::matches(key, keys::BACK_ESC) {
                state.todo_input = None;
                return Action::None;
            }

            if keys::matches(key, keys::BACK_BACKSPACE) {
                if let Some(ref mut input) = state.todo_input {
                    input.pop();
                }
                return Action::None;
            }

            if let crossterm::event::KeyCode::Char(c) = key.code {
                if key.modifiers == crossterm::event::KeyModifiers::NONE
                    || key.modifiers == crossterm::event::KeyModifiers::SHIFT
                {
                    if let Some(ref mut input) = state.todo_input {
                        input.push(c);
                    }
                }
                return Action::None;
            }

            return Action::None;
        }

        // Normal mode
        if keys::matches(key, keys::BACK_ESC) {
            return Action::Navigate(View::Explorer);
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
                        state.notify(NotifyLevel::Error, format!("Failed to toggle TODO: {e}"));
                    }
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::ADD) {
            state.todo_input = Some(String::new());
            return Action::None;
        }

        if keys::matches(key, keys::TAB) {
            state.todo_filter = match state.todo_filter {
                TodoFilter::All => TodoFilter::Global,
                TodoFilter::Global => TodoFilter::Experiment,
                TodoFilter::Experiment => TodoFilter::Run,
                TodoFilter::Run => TodoFilter::All,
            };
            let _ = state.load_todo_data();
            return Action::None;
        }

        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let filter_label = match state.todo_filter {
            TodoFilter::All => "All",
            TodoFilter::Global => "Global",
            TodoFilter::Experiment => "Experiment",
            TodoFilter::Run => "Run",
        };

        let title = format!(" T TODOs [{filter_label}] ");

        let block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(self.theme.border_focused));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split inner area: list + optional input line
        let (list_area, input_area) = if state.todo_input.is_some() {
            let chunks = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(inner);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner, None)
        };

        // Render the list
        if state.todos.is_empty() {
            let text = Paragraph::new(Line::from(
                Span::styled(
                    "No TODOs. Press 'a' to add one.",
                    Style::default().fg(self.theme.accent_dim),
                ),
            ));
            frame.render_widget(text, list_area);
        } else {
            let mut lines: Vec<Line> = Vec::new();

            for (i, todo) in state.todos.iter().enumerate() {
                let is_selected = i == state.todo_cursor;

                // Priority indicator
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

                // Checkbox
                let checkbox = if todo.done { "[x] " } else { "[ ] " };

                // Scope label
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
            frame.render_widget(paragraph, list_area);
        }

        // Render input line if active
        if let (Some(area), Some(ref input)) = (input_area, &state.todo_input) {
            let prompt = Span::styled(" > ", Style::default().fg(self.theme.accent));
            let text_span = Span::styled(
                format!("{input}_"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::SLOW_BLINK),
            );
            let line = Line::from(vec![prompt, text_span]);
            let paragraph = Paragraph::new(line);
            frame.render_widget(paragraph, area);
        }
    }
}
