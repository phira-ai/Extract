use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct LineageView {
    theme: Theme,
}

impl LineageView {
    pub fn new() -> Self {
        Self {
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
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.lineage_nodes.is_empty()
                && state.lineage_cursor + 1 < state.lineage_nodes.len()
            {
                state.lineage_cursor += 1;
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.lineage_cursor = state.lineage_cursor.saturating_sub(1);
            return Action::None;
        }

        if keys::matches(key, keys::SELECT) {
            if let Some(node) = state.lineage_nodes.get(state.lineage_cursor).cloned() {
                match node.entity_type.as_str() {
                    "run" => {
                        if let Ok(Some(run)) = state.db.get_run(&node.entity_id) {
                            if let Some(exp_idx) = state
                                .experiments
                                .iter()
                                .position(|e| e.id == run.experiment_id)
                            {
                                state.selected_experiment = Some(exp_idx);
                                let _ = state.refresh_runs();
                                if let Some(run_idx) =
                                    state.runs.iter().position(|r| r.id == node.entity_id)
                                {
                                    state.selected_run = Some(run_idx);
                                    let _ = state.load_run_preview(run_idx);
                                }
                            }
                            state.current_view = View::Explorer;
                            state.focus = Focus::Detail;
                        }
                    }
                    "model" => {
                        let _ = state.load_registry_data();
                        if let Some(idx) = state
                            .models
                            .iter()
                            .position(|m| m.id == node.entity_id)
                        {
                            state.registry_cursor = idx;
                        }
                        state.current_view = View::Registry;
                    }
                    _ => {}
                }
            }
            return Action::None;
        }

        Action::None
    }

    fn node_color(&self, entity_type: &str) -> Color {
        match entity_type {
            "experiment" => Color::Blue,
            "run" => Color::Green,
            "model" => Color::Yellow,
            _ => Color::DarkGray,
        }
    }

    /// Build a top-down tree representation of the lineage DAG.
    /// Returns a list of (indent_level, node_index, connector_prefix) tuples
    /// representing the tree in display order.
    fn build_tree_lines(&self, state: &AppState) -> Vec<(usize, usize, String)> {
        if state.lineage_nodes.is_empty() {
            return Vec::new();
        }

        // Build adjacency: parent_idx → Vec<child_idx>
        let mut children_of: Vec<Vec<usize>> = vec![vec![]; state.lineage_nodes.len()];
        let mut has_parent = vec![false; state.lineage_nodes.len()];

        for edge in &state.lineage_edges {
            let parent_pos = state.lineage_nodes.iter().position(|n| {
                n.entity_type == edge.parent_type && n.entity_id == edge.parent_id
            });
            let child_pos = state.lineage_nodes.iter().position(|n| {
                n.entity_type == edge.child_type && n.entity_id == edge.child_id
            });
            if let (Some(pi), Some(ci)) = (parent_pos, child_pos) {
                if !children_of[pi].contains(&ci) {
                    children_of[pi].push(ci);
                }
                has_parent[ci] = true;
            }
        }

        // Roots: nodes with no incoming edges
        let roots: Vec<usize> = (0..state.lineage_nodes.len())
            .filter(|i| !has_parent[*i])
            .collect();

        let mut lines = Vec::new();

        fn walk(
            node_idx: usize,
            depth: usize,
            prefix: &str,
            is_last: bool,
            children_of: &[Vec<usize>],
            lines: &mut Vec<(usize, usize, String)>,
        ) {
            let connector = if depth == 0 {
                String::new()
            } else if is_last {
                format!("{prefix}└── ")
            } else {
                format!("{prefix}├── ")
            };

            lines.push((depth, node_idx, connector));

            let children = &children_of[node_idx];
            let child_prefix = if depth == 0 {
                String::new()
            } else if is_last {
                format!("{prefix}    ")
            } else {
                format!("{prefix}│   ")
            };

            for (i, &child) in children.iter().enumerate() {
                let child_is_last = i == children.len() - 1;
                walk(child, depth + 1, &child_prefix, child_is_last, children_of, lines);
            }
        }

        for (i, &root) in roots.iter().enumerate() {
            if i > 0 {
                // Empty line between root trees
                lines.push((0, usize::MAX, String::new()));
            }
            walk(root, 0, "", i == roots.len() - 1, &children_of, &mut lines);
        }

        lines
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);
        let block = Block::bordered()
            .title(" L Lineage ")
            .border_style(border_style);
        let block_inner = block.inner(area);
        frame.render_widget(block, area);

        if state.lineage_nodes.is_empty() {
            let msg = Paragraph::new(Line::from(Span::styled(
                "No lineage data. Use run.derived_from() or run.branched_from() in the Python SDK.",
                Style::default().fg(self.theme.accent_dim),
            )));
            frame.render_widget(msg, block_inner);
            return;
        }

        if block_inner.height < 2 {
            return;
        }

        // Reserve 1 line for info bar
        let tree_area = Rect::new(
            block_inner.x,
            block_inner.y,
            block_inner.width,
            block_inner.height.saturating_sub(1),
        );
        let info_area = Rect::new(
            block_inner.x,
            block_inner.y + block_inner.height.saturating_sub(1),
            block_inner.width,
            1,
        );

        let tree_lines = self.build_tree_lines(state);

        // Map lineage_cursor to the display line index
        let cursor_line = tree_lines
            .iter()
            .position(|(_, ni, _)| *ni == state.lineage_cursor)
            .unwrap_or(0);

        // Compute scroll offset to keep cursor visible
        let visible_height = tree_area.height as usize;
        let scroll = if cursor_line >= visible_height {
            cursor_line - visible_height + 1
        } else {
            0
        };

        let display_lines: Vec<Line> = tree_lines
            .iter()
            .skip(scroll)
            .take(visible_height)
            .map(|(_, node_idx, connector)| {
                if *node_idx == usize::MAX {
                    // Separator line
                    return Line::from("");
                }

                let node = &state.lineage_nodes[*node_idx];
                let is_selected = *node_idx == state.lineage_cursor;
                let color = if is_selected {
                    Color::White
                } else {
                    self.node_color(&node.entity_type)
                };

                let type_tag = match node.entity_type.as_str() {
                    "model" => "mod",
                    "run" => "run",
                    "experiment" => "exp",
                    other => other,
                };

                // Find the relation label for this edge (from parent to this node)
                let relation = state.lineage_edges.iter()
                    .find(|e| e.child_type == node.entity_type && e.child_id == node.entity_id)
                    .map(|e| e.relation.as_str())
                    .unwrap_or("");

                let base_style = if is_selected {
                    self.theme.selected
                } else {
                    Style::default()
                };

                let mut spans = Vec::new();

                // Tree connector (dim)
                if !connector.is_empty() {
                    spans.push(Span::styled(
                        connector.clone(),
                        if is_selected { base_style } else { Style::default().fg(self.theme.accent_dim) },
                    ));
                }

                // Type tag
                spans.push(Span::styled(
                    format!("[{type_tag}] "),
                    if is_selected { base_style } else { Style::default().fg(color).add_modifier(Modifier::BOLD) },
                ));

                // Label
                spans.push(Span::styled(
                    node.label.clone(),
                    if is_selected { base_style } else { Style::default().fg(color) },
                ));

                // Relation (if any)
                if !relation.is_empty() {
                    spans.push(Span::styled(
                        format!("  ({relation})"),
                        if is_selected { base_style } else { Style::default().fg(self.theme.accent_dim) },
                    ));
                }

                Line::from(spans)
            })
            .collect();

        frame.render_widget(Paragraph::new(display_lines), tree_area);

        // Info bar
        if let Some(node) = state.lineage_nodes.get(state.lineage_cursor) {
            let info = Line::from(vec![
                Span::styled(
                    format!(" [{}] ", node.entity_type),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    node.label.clone(),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", node.entity_id),
                    Style::default().fg(self.theme.accent_dim),
                ),
            ]);
            frame.render_widget(Paragraph::new(info), info_area);
        }
    }
}
