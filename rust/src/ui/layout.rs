use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, NotifyLevel, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::compare::CompareView;
use crate::ui::dashboard::Dashboard;
use crate::ui::detail::DetailPanel;
use crate::ui::diff::DiffView;
use crate::ui::lineage::LineageView;
use crate::ui::popup::PopupRenderer;
use crate::ui::registry::RegistryView;
use crate::ui::selection::SelectionWindow;
use crate::ui::statusbar::StatusBar;
use crate::ui::theme::Theme;
use crate::ui::todo::TodoView;
use crate::ui::tree::TreePanel;

pub struct AppLayout {
    pub tree: TreePanel,
    pub detail: DetailPanel,
    pub dashboard: Dashboard,
    pub compare: CompareView,
    pub diff: DiffView,
    pub selection: SelectionWindow,
    pub statusbar: StatusBar,
    pub popup: PopupRenderer,
    pub registry: RegistryView,
    pub lineage: LineageView,
    pub todo_view: TodoView,
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
            selection: SelectionWindow::new(),
            statusbar: StatusBar::new(),
            popup: PopupRenderer::new(),
            registry: RegistryView::new(),
            lineage: LineageView::new(),
            todo_view: TodoView::new(),
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        if let AppEvent::Key(key) = event {
            if state.delete_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_delete_confirm_key(key) {
                    if confirmed {
                        let confirm = state.delete_confirm.as_ref().unwrap();
                        let run_id = confirm.run_id.clone();
                        let label = confirm.label.clone();
                        match state.delete_run(&run_id) {
                            Ok(()) => state.notify(
                                crate::app::NotifyLevel::Success,
                                format!("Deleted {label}"),
                            ),
                            Err(e) => state.notify(
                                crate::app::NotifyLevel::Error,
                                format!("Delete failed: {e}"),
                            ),
                        }
                    }
                    state.delete_confirm = None;
                }
                return Action::None;
            }
            if state.run_picker.is_some() {
                self.popup.handle_run_picker_key(key, state);
                return Action::None;
            }
        }

        // Route to full-screen views first (before panel shortcuts)
        match state.current_view {
            View::Compare => return self.compare.handle_event(event, state),
            View::Diff => return self.diff.handle_event(event, state),
            View::Registry => return self.registry.handle_event(event, state),
            View::Lineage => return self.lineage.handle_event(event, state),
            View::TodoGlobal => return self.todo_view.handle_event(event, state),
            _ => {}
        }

        // Explorer-only panel shortcuts (1, 2, 3) and view shortcuts
        if let AppEvent::Key(key) = event {
            if keys::matches(key, keys::PANEL_1) {
                state.focus = Focus::Tree;
                return Action::None;
            }
            if keys::matches(key, keys::PANEL_2) {
                if state.selected_run.is_none() && !state.runs.is_empty() {
                    state.selected_run = Some(state.runs.len() - 1);
                    let _ = state.load_run_preview(state.runs.len() - 1);
                }
                state.focus = Focus::Detail;
                return Action::None;
            }
            if keys::matches(key, keys::PANEL_3) {
                if !state.selected_runs_for_compare.is_empty() {
                    state.focus = Focus::Selection;
                }
                return Action::None;
            }

            // View shortcuts
            if keys::matches_shift(key, keys::REGISTRY) {
                let _ = state.load_registry_data();
                state.current_view = View::Registry;
                return Action::None;
            }
            if keys::matches_shift(key, keys::TODOS) {
                let _ = state.load_todo_data();
                state.current_view = View::TodoGlobal;
                return Action::None;
            }
            if keys::matches_shift(key, keys::LINEAGE) {
                let _ = state.load_lineage_data();
                state.current_view = View::Lineage;
                return Action::None;
            }
        }

        // Selection window focus
        if state.focus == Focus::Selection {
            if state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Tree;
                // Fall through to normal routing
            } else if let AppEvent::Key(key) = event {
                if keys::matches(key, keys::QUIT) {
                    return Action::Quit;
                }
                self.selection.handle_event(key, state);
                return Action::None;
            }
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
        // Process pending tree select (e.g., from registry Enter)
        self.tree.apply_pending_select(state);

        let area = frame.area();

        let inner = area;

        // Split: main content + status bar
        let vertical = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner);

        let main_area = vertical[0];
        let status_area = vertical[1];

        // Full-screen views: Compare / Diff
        match state.current_view {
            View::Compare => {
                self.compare.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
                self.selection.render(frame, main_area, state);
            }
            View::Diff => {
                self.diff.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
                self.selection.render(frame, main_area, state);
            }
            View::Registry => {
                self.registry.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
            View::Lineage => {
                self.lineage.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
            View::TodoGlobal => {
                self.todo_view.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
            _ => {
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
                        crate::app::SelectionSummary::Root { .. } => " 2 Overview ".to_string(),
                        crate::app::SelectionSummary::Branch { path, .. } => {
                            format!(" 2 {path} ")
                        }
                        crate::app::SelectionSummary::Leaf { name, .. } => {
                            format!(" 2 {name} ")
                        }
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

                // Selection window overlay
                self.selection.render(frame, main_area, state);
            }
        }

        // Popup overlays (rendered on top of everything)
        if let Some(ref picker) = state.run_picker {
            self.popup.render_run_picker(frame, area, picker);
        }
        if let Some(ref confirm) = state.delete_confirm {
            self.popup.render_delete_confirm(frame, area, confirm);
        }

        // Notification toast (rendered last, top-right)
        if let Some(ref notif) = state.notification {
            self.render_notification(frame, area, notif);
        }
    }

    fn render_notification(&self, frame: &mut Frame, area: Rect, notif: &crate::app::Notification) {
        let msg = &notif.message;
        let width = (msg.len() as u16 + 4).min(area.width.saturating_sub(2)); // +4 for border + padding
        let height = 3u16;

        let x = area.x + area.width.saturating_sub(width + 1);
        let y = area.y + 1;
        let toast_area = Rect::new(x, y, width, height);

        let border_color = match notif.level {
            NotifyLevel::Info => self.theme.accent,
            NotifyLevel::Success => self.theme.success,
            NotifyLevel::Warn => self.theme.warning,
            NotifyLevel::Error => self.theme.error,
        };

        let label = match notif.level {
            NotifyLevel::Info => " info ",
            NotifyLevel::Success => " ok ",
            NotifyLevel::Warn => " warn ",
            NotifyLevel::Error => " error ",
        };

        frame.render_widget(Clear, toast_area);

        let block = Block::bordered()
            .title(Span::styled(
                label,
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(toast_area);
        frame.render_widget(block, toast_area);

        let text = Paragraph::new(Line::from(Span::raw(msg.clone())));
        frame.render_widget(text, inner);
    }
}
