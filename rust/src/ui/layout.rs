use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::ui::compare::CompareView;
use crate::ui::dashboard::Dashboard;
use crate::ui::detail::DetailPanel;
use crate::ui::diff::DiffView;
use crate::ui::statusbar::StatusBar;
use crate::ui::theme::Theme;
use crate::ui::tree::TreePanel;

pub struct AppLayout {
    pub tree: TreePanel,
    pub detail: DetailPanel,
    pub dashboard: Dashboard,
    pub compare: CompareView,
    pub diff: DiffView,
    pub statusbar: StatusBar,
    theme: Theme,
}

impl AppLayout {
    pub fn new() -> Self {
        Self {
            tree: TreePanel::new(),
            detail: DetailPanel::new(),
            dashboard: Dashboard::new(),
            compare: CompareView::new(),
            diff: DiffView::new(),
            statusbar: StatusBar::new(),
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        // Route to full-screen views first
        match state.current_view {
            View::Compare => return self.compare.handle_event(event, state),
            View::Diff => return self.diff.handle_event(event, state),
            _ => {}
        }

        // Dispatch to the focused component
        match state.focus {
            Focus::Tree => self.tree.handle_event(event, state),
            Focus::Detail => {
                let action = self.detail.handle_event(event, state);
                action
            }
            Focus::Selection => Action::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, state: &mut AppState) {
        let area = frame.area();

        // Outer block with title
        let outer_block = Block::bordered()
            .title(" Extract ")
            .border_style(Style::default().fg(self.theme.border));
        let inner = outer_block.inner(area);
        frame.render_widget(outer_block, area);

        // Split: main content + status bar
        let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);

        let main_area = vertical[0];
        let status_area = vertical[1];

        // Full-screen views: Compare / Diff
        match state.current_view {
            View::Compare => {
                self.compare.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
                return;
            }
            View::Diff => {
                self.diff.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
                return;
            }
            _ => {}
        }

        // Split main area: 30% tree, 70% detail
        let horizontal = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(70),
        ])
        .split(main_area);

        let tree_area = horizontal[0];
        let detail_area = horizontal[1];

        // Render tree (always visible)
        self.tree.render(frame, tree_area, state);

        // Render right panel: detail if a run is selected, dashboard otherwise
        if state.selected_run.is_some() && !state.runs.is_empty() {
            self.detail.render(frame, detail_area, state);
        } else {
            let focused = state.focus == Focus::Detail;
            let border_style = if focused {
                Style::default().fg(self.theme.border_focused)
            } else {
                Style::default().fg(self.theme.border)
            };
            let title = match &state.selection_summary {
                crate::app::SelectionSummary::Root { .. } => " Overview ".to_string(),
                crate::app::SelectionSummary::Branch { path, .. } => format!(" {path} "),
                crate::app::SelectionSummary::Leaf { name, .. } => format!(" {name} "),
            };
            let block = Block::bordered()
                .title(title)
                .border_style(border_style);
            let inner_detail = block.inner(detail_area);
            frame.render_widget(block, detail_area);
            self.dashboard.render(frame, inner_detail, state);
        }

        // Render status bar
        self.statusbar.render(frame, status_area, state);
    }
}
