use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, NotifyLevel, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::compare::CompareView;
use crate::ui::dashboard::Dashboard;
use crate::ui::detail::{DetailPanel, DetailTab};
use crate::ui::diff::DiffView;
use crate::ui::help::HelpOverlay;
use crate::ui::lineage::LineageView;
use crate::ui::popup::PopupRenderer;
use crate::ui::registry::RegistryView;
use crate::ui::search::SearchPopup;
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
    pub popup: PopupRenderer,
    pub registry: RegistryView,
    pub lineage: LineageView,
    pub todo_view: TodoView,
    pub search: SearchPopup,
    pub help: HelpOverlay,
    pub statusbar: StatusBar,
    theme: Theme,
}

impl AppLayout {
    pub fn new(config: &crate::config::Config) -> Self {
        let theme = Theme::from_config(&config.theme);
        Self {
            tree: TreePanel::new(theme),
            detail: DetailPanel::new(theme),
            dashboard: Dashboard::new(theme),
            compare: CompareView::new(theme),
            diff: DiffView::new(theme),
            selection: SelectionWindow::new(theme),
            popup: PopupRenderer::new(theme),
            registry: RegistryView::new(theme),
            lineage: LineageView::new(theme),
            todo_view: TodoView::new(theme),
            search: SearchPopup::new(theme),
            help: HelpOverlay::new(theme),
            statusbar: StatusBar::new(theme),
            theme,
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        // Search popup intercepts all keys when active
        if state.search.is_some() {
            if let AppEvent::Key(key) = event {
                self.search.handle_key(key, state);
            }
            return Action::None;
        }

        // Help overlay dismisses on any key
        if state.show_help {
            if let AppEvent::Key(_) = event {
                state.show_help = false;
            }
            return Action::None;
        }

        // Skip global keybindings when a text input mode is active — let the
        // focused panel's input handler see every keystroke unmodified.
        let in_text_input = state.tag_edit.is_some()
            || state.note_input.is_some()
            || state.todo_input.is_some();

        if !in_text_input {
            // Global keys: gg/G, ?, work in all views
            if let AppEvent::Key(key) = event {
                if state.g_pending {
                    state.g_pending = false;
                    if keys::matches(key, keys::GO_TOP_G) {
                        self.go_to_edge(state, true);
                        return Action::None;
                    }
                    // Not a second g — fall through to normal handling
                }
                if keys::matches(key, keys::GO_TOP_G) {
                    state.g_pending = true;
                    return Action::None;
                }
                if keys::matches_shift(key, keys::GO_BOTTOM) {
                    self.go_to_edge(state, false);
                    return Action::None;
                }
                if keys::matches(key, keys::HELP) {
                    state.show_help = true;
                    return Action::None;
                }
            }

            // Global h/l → behave like shift-tab/tab
            if let AppEvent::Key(key) = event {
                if keys::matches(key, keys::NAV_RIGHT_L) {
                    // Simulate TAB
                    let tab_event = AppEvent::Key(crossterm::event::KeyEvent::new(
                        keys::TAB,
                        crossterm::event::KeyModifiers::NONE,
                    ));
                    return self.handle_event(&tab_event, state);
                }
                if keys::matches(key, keys::NAV_LEFT_H) {
                    // Simulate BACKTAB
                    let backtab_event = AppEvent::Key(crossterm::event::KeyEvent::new(
                        keys::BACKTAB,
                        crossterm::event::KeyModifiers::SHIFT,
                    ));
                    return self.handle_event(&backtab_event, state);
                }
            }
        }

        if let AppEvent::Key(key) = event {
            if state.delete_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_delete_confirm_key(key) {
                    if confirmed {
                        let confirm = state.delete_confirm.as_ref().unwrap();
                        let label = confirm.label.clone();
                        let target = confirm.target.clone();
                        match &target {
                            crate::app::DeleteTarget::Run { run_id } => {
                                let run_id = run_id.clone();
                                match state.delete_run(&run_id) {
                                    Ok(()) => {
                                        state.notify(
                                            crate::app::NotifyLevel::Success,
                                            format!("Deleted {label}"),
                                        );
                                        // Refresh run browser if open
                                        if let Some(ref mut browser) = state.run_browser {
                                            browser.runs.retain(|r| r.id != run_id);
                                            browser.apply_filter();
                                            if browser.cursor >= browser.filtered.len() && !browser.filtered.is_empty() {
                                                browser.cursor = browser.filtered.len() - 1;
                                            }
                                            if browser.runs.len() <= 1 {
                                                state.run_browser = None;
                                            }
                                        }
                                    }
                                    Err(e) => state.notify(
                                        crate::app::NotifyLevel::Error,
                                        format!("Delete failed: {e}"),
                                    ),
                                }
                            }
                            crate::app::DeleteTarget::Experiment { experiment_id } => {
                                let experiment_id = experiment_id.clone();
                                match state.delete_experiment(&experiment_id) {
                                    Ok(()) => {
                                        state.notify(
                                            crate::app::NotifyLevel::Success,
                                            format!("Deleted {label}"),
                                        );
                                    }
                                    Err(e) => state.notify(
                                        crate::app::NotifyLevel::Error,
                                        format!("Delete failed: {e}"),
                                    ),
                                }
                            }
                        }
                    }
                    state.delete_confirm = None;
                }
                return Action::None;
            }
            if state.archive_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_archive_confirm_key(key) {
                    if confirmed {
                        let confirm = state.archive_confirm.take().unwrap();
                        let db_path = state.store_root.join("extract.db");
                        match crate::db::Db::archive_experiment(&db_path, &confirm.experiment_id) {
                            Ok(()) => {
                                state.notify(crate::app::NotifyLevel::Success, format!("Archived '{}'", confirm.label));
                                let _ = state.refresh_experiments();
                                let _ = state.refresh_runs();
                                let _ = state.refresh_selection_summary();
                            }
                            Err(e) => {
                                state.notify(crate::app::NotifyLevel::Error, format!("Archive failed: {e}"));
                            }
                        }
                    } else {
                        state.archive_confirm = None;
                    }
                }
                return Action::None;
            }
            if state.run_picker.is_some() {
                self.popup.handle_run_picker_key(key, state);
                return Action::None;
            }
            if state.run_browser.is_some() {
                self.popup.handle_run_browser_key(key, state);
                return Action::None;
            }
        }

        // Selection window intercepts keys when focused (works in all views)
        if state.focus == Focus::Selection && !state.selected_runs_for_compare.is_empty() {
            if let AppEvent::Key(key) = event {
                if keys::matches(key, keys::QUIT) {
                    return Action::Quit;
                }
                // Tab/BackTab/Esc from Selection: return focus to the main view
                if keys::matches(key, keys::TAB) || keys::matches(key, keys::BACKTAB) || keys::matches(key, keys::BACK_ESC) {
                    match state.current_view {
                        View::Compare | View::Diff => {
                            // In compare/diff, just unfocus selection (compare/diff has no sub-panels)
                            state.focus = Focus::Tree; // Tree means "main content" here
                        }
                        _ => {
                            state.focus = Focus::Tree;
                        }
                    }
                    return Action::None;
                }
                self.selection.handle_event(key, state);
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
            if keys::matches(key, keys::SEARCH) {
                state.search = Some(crate::app::SearchState {
                    query: String::new(),
                    results: Vec::new(),
                    cursor: 0,
                });
                return Action::None;
            }
            if keys::matches(key, keys::RUN_BROWSER) {
                // Open run browser for current leaf experiment with multiple runs
                if let Some(idx) = state.selected_experiment {
                    if let Some(exp) = state.experiments.get(idx) {
                        let has_children = state.experiments.iter()
                            .any(|e| e.parent_id.as_deref() == Some(&exp.id));
                        if !has_children && state.runs.len() > 1 {
                            let mut sorted_runs = state.runs.clone();
                            sorted_runs.sort_by(|a, b| {
                                let a_time = a.ended_at.as_deref().unwrap_or(&a.started_at);
                                let b_time = b.ended_at.as_deref().unwrap_or(&b.started_at);
                                b_time.cmp(a_time)
                            });
                            state.run_browser = Some(crate::app::RunBrowserState::new(
                                exp.name.clone(),
                                exp.id.clone(),
                                sorted_runs,
                            ));
                        }
                    }
                }
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

    /// Move cursor/scroll to top (true) or bottom (false) for the current view/focus.
    fn go_to_edge(&mut self, state: &mut AppState, top: bool) {
        match state.current_view {
            View::Compare | View::Diff => {
                if let Some(ref mut data) = state.compare_data {
                    data.scroll = if top { 0 } else { data.total_lines.saturating_sub(data.visible_height) as u16 };
                }
            }
            View::Registry => {
                if top {
                    state.registry_cursor = 0;
                } else if !state.models.is_empty() {
                    state.registry_cursor = state.models.len() - 1;
                }
            }
            View::Lineage => {
                if top {
                    state.lineage_cursor = 0;
                } else if !state.lineage_nodes.is_empty() {
                    state.lineage_cursor = state.lineage_nodes.len() - 1;
                }
            }
            View::TodoGlobal => {
                if top {
                    state.todo_cursor = 0;
                } else if !state.todos.is_empty() {
                    state.todo_cursor = state.todos.len() - 1;
                }
            }
            _ => match state.focus {
                Focus::Tree => {
                    if top {
                        self.tree.tree_state.select_first();
                    } else {
                        self.tree.tree_state.select_last();
                    }
                }
                Focus::Detail => {
                    if self.detail.active_tab == DetailTab::Info {
                        if top {
                            state.info_scroll = 0;
                        } else {
                            state.info_scroll = state.info_total_lines.saturating_sub(state.info_visible_height) as u16;
                        }
                    } else {
                        if top {
                            state.summary_scroll = 0;
                        } else {
                            state.summary_scroll = state.summary_total_lines.saturating_sub(state.summary_visible_height) as u16;
                        }
                    }
                }
                Focus::Selection => {
                    if top {
                        state.selection_cursor = 0;
                    } else if !state.selected_runs_for_compare.is_empty() {
                        state.selection_cursor = state.selected_runs_for_compare.len() - 1;
                    }
                }
            },
        }
    }

    pub fn render(&mut self, frame: &mut Frame, state: &mut AppState) {
        // Process pending tree select (e.g., from registry Enter)
        self.tree.apply_pending_select(state);

        let area = frame.area();

        // Reserve bottom 1 line for the statusbar
        let vertical = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
        let main_area = vertical[0];
        let statusbar_area = vertical[1];

        // Full-screen views
        match state.current_view {
            View::Compare => {
                self.compare.render(frame, main_area, state);
                self.selection.render(frame, main_area, state);
            }
            View::Diff => {
                self.diff.render(frame, main_area, state);
                self.selection.render(frame, main_area, state);
            }
            View::Registry => {
                self.registry.render(frame, main_area, state);
            }
            View::Lineage => {
                self.lineage.render(frame, main_area, state);
            }
            View::TodoGlobal => {
                self.todo_view.render(frame, main_area, state);
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
                        .border_style(border_style)
                        .border_set(border::ROUNDED);
                    let inner_detail = block.inner(detail_area);
                    frame.render_widget(block, detail_area);
                    self.dashboard.render(frame, inner_detail, state);
                }

                // Selection window overlay
                self.selection.render(frame, main_area, state);
            }
        }

        // Popup overlays (rendered on top of everything)
        if let Some(ref mut picker) = state.run_picker {
            self.popup.render_run_picker(frame, area, picker);
        }
        if let Some(ref mut browser) = state.run_browser {
            self.popup.render_run_browser(frame, area, browser);
        }
        if let Some(ref confirm) = state.delete_confirm {
            self.popup.render_delete_confirm(frame, area, confirm);
        }
        if let Some(ref confirm) = state.archive_confirm {
            self.popup.render_archive_confirm(frame, area, confirm);
        }
        if let Some(ref input) = state.note_input {
            self.detail.render_note_popup(frame, area, input);
        }

        // Notification toast
        if let Some(ref notif) = state.notification {
            self.render_notification(frame, area, notif);
        }

        // Search popup overlay
        if let Some(ref search) = state.search {
            self.search.render(frame, area, search);
        }

        // Help overlay
        if state.show_help {
            self.help.render(frame, area);
        }

        // Statusbar (always rendered at the bottom)
        self.statusbar.render(frame, statusbar_area, state, self.detail.active_tab);
    }

    fn render_notification(&self, frame: &mut Frame, area: Rect, notif: &crate::app::Notification) {
        let msg = &notif.message;
        let width = (msg.len() as u16 + 4).min(area.width.saturating_sub(2));
        let height = 3u16;

        let x = area.x + area.width.saturating_sub(width + 1);
        let y = area.y + 1;
        let toast_area = Rect::new(x, y, width, height);

        let border_color = match notif.level {
            NotifyLevel::Success => self.theme.success,
            NotifyLevel::Warn => self.theme.warning,
            NotifyLevel::Error => self.theme.error,
        };

        let label = match notif.level {
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
            .border_style(Style::default().fg(border_color))
            .border_set(border::ROUNDED);

        let inner = block.inner(toast_area);
        frame.render_widget(block, toast_area);

        let text = Paragraph::new(Line::from(Span::raw(msg.clone())));
        frame.render_widget(text, inner);
    }
}
