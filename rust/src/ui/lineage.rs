use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

const NODE_SPACING_X: f64 = 30.0;
const NODE_SPACING_Y: f64 = 12.0;

pub struct LineageView {
    pub theme: Theme,
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
            if state.lineage_cursor > 0 {
                state.lineage_cursor = state.lineage_cursor.saturating_sub(1);
            }
            return Action::None;
        }

        if keys::matches(key, keys::SELECT) {
            if let Some(node) = state.lineage_nodes.get(state.lineage_cursor).cloned() {
                match node.entity_type.as_str() {
                    "run" => {
                        // Find the run, then its experiment, and navigate to Explorer/Detail
                        if let Ok(Some(run)) = state.db.get_run(&node.entity_id) {
                            // Find and select the experiment
                            if let Some(exp_idx) = state
                                .experiments
                                .iter()
                                .position(|e| e.id == run.experiment_id)
                            {
                                state.selected_experiment = Some(exp_idx);
                                let _ = state.refresh_runs();

                                // Find and select the run
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
                        // Load registry data and navigate to the model
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

    fn compute_bounds(&self, state: &AppState) -> (f64, f64, f64, f64) {
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for node in &state.lineage_nodes {
            let px = node.x * NODE_SPACING_X;
            let py = -(node.y * NODE_SPACING_Y);
            min_x = min_x.min(px);
            max_x = max_x.max(px);
            min_y = min_y.min(py);
            max_y = max_y.max(py);
        }

        (min_x, max_x, min_y, max_y)
    }

    fn node_color(&self, entity_type: &str) -> Color {
        match entity_type {
            "experiment" => Color::Blue,
            "run" => Color::Green,
            "model" => Color::Yellow,
            _ => Color::DarkGray,
        }
    }

    fn truncate_label(label: &str, max_len: usize) -> String {
        if label.len() <= max_len {
            label.to_string()
        } else {
            format!("{}...", &label[..max_len.saturating_sub(3)])
        }
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

        // Reserve 1 line at bottom for info bar
        if block_inner.height < 2 {
            return;
        }
        let canvas_area = Rect::new(
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

        // Compute canvas bounds
        let (min_x, max_x, min_y, max_y) = self.compute_bounds(state);
        let x_lo = min_x - NODE_SPACING_X;
        let x_hi = max_x + NODE_SPACING_X;
        let y_lo = min_y - NODE_SPACING_Y;
        let y_hi = max_y + NODE_SPACING_Y;

        // Build a lookup from (entity_type, entity_id) -> (px, py) for edge drawing
        let node_positions: Vec<(f64, f64)> = state
            .lineage_nodes
            .iter()
            .map(|n| (n.x * NODE_SPACING_X, -(n.y * NODE_SPACING_Y)))
            .collect();

        let selected_cursor = state.lineage_cursor;

        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([x_lo, x_hi])
            .y_bounds([y_lo, y_hi])
            .paint(|ctx| {
                // Draw edges
                for edge in &state.lineage_edges {
                    let parent_pos = state
                        .lineage_nodes
                        .iter()
                        .enumerate()
                        .find(|(_, n)| {
                            n.entity_type == edge.parent_type && n.entity_id == edge.parent_id
                        })
                        .map(|(i, _)| node_positions[i]);

                    let child_pos = state
                        .lineage_nodes
                        .iter()
                        .enumerate()
                        .find(|(_, n)| {
                            n.entity_type == edge.child_type && n.entity_id == edge.child_id
                        })
                        .map(|(i, _)| node_positions[i]);

                    if let (Some((px, py)), Some((cx, cy))) = (parent_pos, child_pos) {
                        ctx.draw(&CanvasLine {
                            x1: px,
                            y1: py,
                            x2: cx,
                            y2: cy,
                            color: Color::DarkGray,
                        });
                    }
                }

                // Draw nodes as points
                for (i, node) in state.lineage_nodes.iter().enumerate() {
                    let (px, py) = node_positions[i];
                    let color = if i == selected_cursor {
                        Color::White
                    } else {
                        self.node_color(&node.entity_type)
                    };
                    ctx.draw(&Points {
                        coords: &[(px, py)],
                        color,
                    });
                }

                // Draw labels next to nodes
                for (i, node) in state.lineage_nodes.iter().enumerate() {
                    let (px, py) = node_positions[i];
                    let color = if i == selected_cursor {
                        Color::White
                    } else {
                        self.node_color(&node.entity_type)
                    };
                    let label = Self::truncate_label(&node.label, 20);
                    ctx.print(px + 1.5, py, Line::from(Span::styled(label, Style::default().fg(color))));
                }
            });

        frame.render_widget(canvas, canvas_area);

        // Info bar: show selected node details
        if let Some(node) = state.lineage_nodes.get(selected_cursor) {
            let info = Line::from(vec![
                Span::styled(
                    format!(" [{}] ", node.entity_type),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} ", node.label),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("({})", node.entity_id),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);
            frame.render_widget(Paragraph::new(info), info_area);
        }
    }
}
