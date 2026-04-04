use std::collections::HashMap;

use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Frame;
use tui_tree_widget::{Tree, TreeItem, TreeState};

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::model::Experiment;
use crate::ui::theme::Theme;

pub struct TreePanel {
    pub tree_state: TreeState<String>,
    theme: Theme,
}

impl TreePanel {
    pub fn new() -> Self {
        let mut tree_state = TreeState::default();
        tree_state.select(Vec::new());
        Self {
            tree_state,
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
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            self.tree_state.key_down();
            self.sync_selection(state);
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            self.tree_state.key_up();
            self.sync_selection(state);
            return Action::None;
        }

        if keys::matches(key, keys::SELECT) {
            // Toggle open/close on the selected node
            self.tree_state.toggle_selected();

            // Check if the selected item is a leaf experiment (no children in the tree)
            // If so, load its runs
            let selected = self.tree_state.selected().to_vec();
            if let Some(last_id) = selected.last() {
                // Check if this experiment has children
                let has_children = state
                    .experiments
                    .iter()
                    .any(|e| e.parent_id.as_deref() == Some(last_id));

                if !has_children {
                    // It's a leaf experiment: select it and load its runs
                    if let Some(idx) = state
                        .experiments
                        .iter()
                        .position(|e| &e.id == last_id)
                    {
                        state.selected_experiment = Some(idx);
                        let _ = state.refresh_runs();

                        // Select the first run if available
                        if !state.runs.is_empty() {
                            state.selected_run = Some(0);
                        }

                        // Load metrics for the selected run
                        if let Some(run_idx) = state.selected_run {
                            if let Some(run) = state.runs.get(run_idx) {
                                state.metrics = state
                                    .db
                                    .get_latest_metrics(&run.id)
                                    .unwrap_or_default();
                            }
                        }

                        state.focus = Focus::Detail;
                        return Action::Navigate(View::Detail);
                    }
                }
            }

            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            self.tree_state.key_left();
            self.sync_selection(state);
            return Action::None;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            // Toggle run selection for comparison
            let selected = self.tree_state.selected().to_vec();
            if let Some(last_id) = selected.last() {
                if state.selected_runs_for_compare.contains(last_id) {
                    state
                        .selected_runs_for_compare
                        .retain(|id| id != last_id);
                } else {
                    state
                        .selected_runs_for_compare
                        .push(last_id.clone());
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        if keys::matches(key, keys::TAB) {
            state.focus = Focus::Detail;
            return Action::None;
        }

        Action::None
    }

    fn sync_selection(&self, state: &mut AppState) {
        let selected = self.tree_state.selected().to_vec();
        if let Some(last_id) = selected.last() {
            if let Some(idx) = state.experiments.iter().position(|e| e.id == *last_id) {
                state.selected_experiment = Some(idx);
                state.selected_run = None;
                state.metrics.clear();
                let _ = state.refresh_runs();
                let _ = state.refresh_selection_summary();
                // Load preview data (curves + matrix) for leaf experiments
                let has_children = state
                    .experiments
                    .iter()
                    .any(|e| e.parent_id.as_deref() == Some(last_id.as_str()));
                if !has_children {
                    let _ = state.refresh_leaf_preview();
                }
            }
        } else {
            state.selected_experiment = None;
            state.selected_run = None;
            state.runs.clear();
            state.metrics.clear();
            let _ = state.refresh_selection_summary();
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let focused = state.focus == Focus::Tree;
        let border_style = if focused {
            Style::default().fg(self.theme.border_focused)
        } else {
            Style::default().fg(self.theme.border)
        };

        let block = Block::bordered()
            .title(" Experiments ")
            .border_style(border_style);

        // Build tree items from experiments
        let tree_items = build_tree_items(&state.experiments);

        if let Ok(tree_widget) = Tree::new(&tree_items) {
            let tree_widget = tree_widget
                .block(block)
                .highlight_style(self.theme.selected)
                .highlight_symbol(">> ")
                .node_closed_symbol("\u{25b6} ")
                .node_open_symbol("\u{25bc} ")
                .node_no_children_symbol("  ");

            frame.render_stateful_widget(tree_widget, area, &mut self.tree_state);
        }
    }
}

/// Build a hierarchical tree of TreeItems from the flat list of experiments.
fn build_tree_items<'a>(experiments: &[Experiment]) -> Vec<TreeItem<'a, String>> {
    // Group experiments by parent_id
    let mut children_map: HashMap<Option<String>, Vec<&Experiment>> = HashMap::new();
    for exp in experiments {
        children_map
            .entry(exp.parent_id.clone())
            .or_default()
            .push(exp);
    }

    // Count runs per experiment (we show this in the label)
    // Since we don't have run counts in experiments, show just the name
    fn build_children<'a>(
        parent_id: Option<&str>,
        children_map: &HashMap<Option<String>, Vec<&Experiment>>,
    ) -> Vec<TreeItem<'a, String>> {
        let key = parent_id.map(String::from);
        let Some(children) = children_map.get(&key) else {
            return Vec::new();
        };

        children
            .iter()
            .filter_map(|exp| {
                let sub_children = build_children(Some(&exp.id), children_map);
                let label = if sub_children.is_empty() {
                    exp.name.clone()
                } else {
                    format!("{} [{}]", exp.name, sub_children.len())
                };

                if sub_children.is_empty() {
                    Some(TreeItem::new_leaf(exp.id.clone(), label))
                } else {
                    TreeItem::new(exp.id.clone(), label, sub_children).ok()
                }
            })
            .collect()
    }

    build_children(None, &children_map)
}
