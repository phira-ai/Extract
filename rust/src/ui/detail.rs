use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, SelectionSummary, View};
use crate::event::AppEvent;
use crate::keys;
use crate::model::Run;
use crate::ui::summary::{SummaryData, SummaryRenderer};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Summary,
    Info,
}

pub struct DetailPanel {
    pub active_tab: DetailTab,
    summary: SummaryRenderer,
    theme: Theme,
}

impl DetailPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            active_tab: DetailTab::Summary,
            summary: SummaryRenderer::new(theme),
            theme,
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent, state: &mut AppState) -> Action {
        // Tag picker popup mode
        if state.tag_picker.is_some() {
            self.handle_tag_picker_key(key, state);
            return Action::None;
        }

        // Run rename input mode
        if state.run_rename.is_some() {
            self.handle_run_rename_key(key, state);
            return Action::None;
        }

        // Note append input mode
        if state.note_input.is_some() {
            match key.code {
                crossterm::event::KeyCode::Enter => {
                    let line = state.note_input.take().unwrap_or_default();
                    if !line.trim().is_empty() {
                        let db_path = state.store_root.join("extract.db");
                        if let Some(idx) = state.selected_run {
                            if let Some(run) = state.runs.get(idx) {
                                let _ = crate::db::Db::append_note(&db_path, "runs", &run.id, line.trim());
                            }
                        } else if let Some(idx) = state.selected_experiment {
                            if let Some(exp) = state.experiments.get(idx) {
                                let _ = crate::db::Db::append_note(&db_path, "experiments", &exp.id, line.trim());
                            }
                        }
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Esc => {
                    state.note_input = None;
                    return Action::None;
                }
                crossterm::event::KeyCode::Backspace => {
                    if let Some(ref mut input) = state.note_input {
                        input.pop();
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Char(c) => {
                    if key.modifiers == crossterm::event::KeyModifiers::NONE
                        || key.modifiers == crossterm::event::KeyModifiers::SHIFT
                    {
                        if let Some(ref mut input) = state.note_input {
                            input.push(c);
                        }
                    }
                    return Action::None;
                }
                _ => return Action::None,
            }
        }

        // S/I switch detail tabs
        if keys::matches_shift(key, keys::SUMMARY_TAB) {
            self.active_tab = DetailTab::Summary;
            return Action::None;
        }
        if keys::matches_shift(key, keys::INFO_TAB) {
            self.active_tab = DetailTab::Info;
            return Action::None;
        }

        // Tab → next panel: Selection (if marked) or Tree
        if keys::matches(key, keys::TAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            } else {
                state.focus = Focus::Tree;
            }
            return Action::None;
        }

        // Shift-Tab → previous panel: Tree
        if keys::matches(key, keys::BACKTAB) {
            state.focus = Focus::Tree;
            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.focus = Focus::Tree;
            state.current_view = View::Explorer;
            return Action::None;
        }

        // Summary tab: Shift+R renames the focused run.
        if self.active_tab == DetailTab::Summary && keys::matches_shift(key, keys::RUN_RENAME) {
            self.open_run_rename(state);
            return Action::None;
        }

        // Summary tab: j/k scrolls
        if self.active_tab == DetailTab::Summary {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                let max_scroll = state.summary_total_lines.saturating_sub(state.summary_visible_height);
                if (state.summary_scroll as usize) < max_scroll {
                    state.summary_scroll += 1;
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                state.summary_scroll = state.summary_scroll.saturating_sub(1);
                return Action::None;
            }
        }

        // Info tab: j/k scrolls
        if self.active_tab == DetailTab::Info {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                let max_scroll = state.info_total_lines.saturating_sub(state.info_visible_height);
                if (state.info_scroll as usize) < max_scroll {
                    state.info_scroll += 1;
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                state.info_scroll = state.info_scroll.saturating_sub(1);
                return Action::None;
            }
        }

        if keys::matches(key, keys::CYCLE_NEXT) {
            if let Some(idx) = state.selected_run {
                if idx + 1 < state.runs.len() {
                    state.selected_run = Some(idx + 1);
                    let _ = state.load_run_preview(idx + 1);
                    self.load_metrics_for_selected_run(state);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::CYCLE_PREV) {
            if let Some(idx) = state.selected_run {
                if idx > 0 {
                    state.selected_run = Some(idx - 1);
                    let _ = state.load_run_preview(idx - 1);
                    self.load_metrics_for_selected_run(state);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::COMPARE) {
            if state.selected_runs_for_compare.len() >= 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Compare;
                    return Action::Navigate(View::Compare);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::DIFF) {
            if state.selected_runs_for_compare.len() >= 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Diff;
                    return Action::Navigate(View::Diff);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::DELETE) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                let run_id = run.id.clone();
                let label = run.name.clone().unwrap_or_else(|| {
                    if run_id.len() > 8 {
                        run_id[run_id.len() - 8..].to_string()
                    } else {
                        run_id.clone()
                    }
                });
                state.delete_confirm = Some(crate::app::DeleteConfirmState {
                    target: crate::app::DeleteTarget::Run { run_id },
                    label,
                });
            }
            return Action::None;
        }

        // t: open tag picker popup
        if keys::matches(key, keys::TAG_EDIT) {
            self.open_tag_picker(state);
            return Action::None;
        }

        // n: append note popup
        if keys::matches(key, keys::NOTE_APPEND) {
            state.note_input = Some(String::new());
            return Action::None;
        }

        // Ctrl+E: open notes in $EDITOR
        if key.code == crossterm::event::KeyCode::Char('e')
            && key.modifiers == crossterm::event::KeyModifiers::CONTROL
        {
            if let Some(idx) = state.selected_run {
                if let Some(run) = state.runs.get(idx) {
                    return Action::SuspendForEditor {
                        table: "runs".to_string(),
                        id: run.id.clone(),
                    };
                }
            } else if let Some(idx) = state.selected_experiment {
                if let Some(exp) = state.experiments.get(idx) {
                    return Action::SuspendForEditor {
                        table: "experiments".to_string(),
                        id: exp.id.clone(),
                    };
                }
            }
            return Action::None;
        }

        // Shift+F: mark failed (running runs only)
        if keys::matches_shift(key, keys::MARK_FAILED) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                if run.status == "running" {
                    let db_path = state.store_root.join("extract.db");
                    let _ = crate::db::Db::set_status(&db_path, "runs", &run.id, "failed");
                    state.notify(crate::app::NotifyLevel::Success, "Run marked failed");
                    let _ = state.refresh_runs();
                }
            }
            return Action::None;
        }

        // Shift+C: mark completed (running or failed runs only)
        if keys::matches_shift(key, keys::MARK_COMPLETED) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                if run.status == "running" || run.status == "failed" {
                    let db_path = state.store_root.join("extract.db");
                    let _ = crate::db::Db::set_status(&db_path, "runs", &run.id, "completed");
                    state.notify(crate::app::NotifyLevel::Success, "Run marked completed");
                    let _ = state.refresh_runs();
                }
            }
            return Action::None;
        }

        // Shift+A: archive run
        if keys::matches_shift(key, keys::ARCHIVE) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                if run.status != "archived" {
                    let db_path = state.store_root.join("extract.db");
                    let _ = crate::db::Db::set_status(&db_path, "runs", &run.id, "archived");
                    state.notify(crate::app::NotifyLevel::Success, "Run archived");
                    let _ = state.refresh_runs();
                    let _ = state.refresh_selection_summary();
                }
            }
            return Action::None;
        }

        // Shift+U: unarchive run
        if keys::matches_shift(key, keys::UNARCHIVE) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                if run.status == "archived" {
                    let db_path = state.store_root.join("extract.db");
                    let _ = crate::db::Db::unarchive_item(&db_path, "runs", &run.id);
                    state.notify(crate::app::NotifyLevel::Success, "Run unarchived");
                    let _ = state.refresh_runs();
                    let _ = state.refresh_selection_summary();
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    fn load_metrics_for_selected_run(&mut self, state: &mut AppState) {
        if let Some(run_idx) = state.selected_run {
            if let Some(run) = state.runs.get(run_idx) {
                state.metrics = state
                    .db
                    .get_latest_metrics(&run.id)
                    .unwrap_or_default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        let focused = state.focus == Focus::Detail;
        let border_style = if focused {
            Style::default().fg(self.theme.border_focused)
        } else {
            Style::default().fg(self.theme.border)
        };

        let run_indicator = if focused && state.runs.len() > 1 {
            let idx = state.selected_run.unwrap_or(0);
            let run_name = state.runs.get(idx).and_then(|r| r.name.as_deref()).unwrap_or("");
            if run_name.is_empty() {
                format!(" run {}/{} ", idx + 1, state.runs.len())
            } else {
                format!(" {} {}/{} ", run_name, idx + 1, state.runs.len())
            }
        } else {
            String::new()
        };

        let block = Block::bordered()
            .title(format!(" 2 Detail{run_indicator}"))
            .border_style(border_style)
            .border_set(border::ROUNDED);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let selected_run = state.selected_run.and_then(|i| state.runs.get(i).cloned());

        let Some(run) = selected_run else {
            let msg = Paragraph::new("Select an experiment and run to view details.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        };

        if focused {
            // Split inner into tab bar + content
            let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);
            self.render_tab_bar(frame, chunks[0]);
            match self.active_tab {
                DetailTab::Summary => self.render_summary(frame, chunks[1], state),
                DetailTab::Info => self.render_info(frame, chunks[1], &run, state),
            }
        } else {
            match self.active_tab {
                DetailTab::Summary => self.render_summary(frame, inner, state),
                DetailTab::Info => self.render_info(frame, inner, &run, state),
            }
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let mnemonic = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        let mut spans: Vec<Span> = Vec::new();

        // Summary tab
        let sum_style = if self.active_tab == DetailTab::Summary {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };
        spans.push(Span::raw(" ["));
        spans.push(Span::styled("S", mnemonic));
        spans.push(Span::styled("ummary]", sum_style));

        // Info tab
        let info_style = if self.active_tab == DetailTab::Info {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };
        spans.push(Span::raw(" ["));
        spans.push(Span::styled("I", mnemonic));
        spans.push(Span::styled("nfo]", info_style));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        // Build SummaryData from the SelectionSummary::Leaf
        let (name, runs, run_metrics, aggregate_metrics, unique_configs) =
            match &state.selection_summary {
                SelectionSummary::Leaf {
                    name,
                    runs,
                    run_metrics,
                    aggregate_metrics,
                    unique_configs,
                } => (
                    name.clone(),
                    runs.clone(),
                    run_metrics.clone(),
                    aggregate_metrics.clone(),
                    *unique_configs,
                ),
                _ => return,
            };

        // Resolve the preview run's total_steps for the chart x-axis pin.
        // Per-run detail view uses state.selected_run; leaf preview falls back
        // to the same picker as reload_leaf_preview_data so the pinned axis
        // matches the loaded data.
        let preview_total_steps = if let Some(idx) = state.selected_run {
            state.runs.get(idx).and_then(|r| r.total_steps)
        } else {
            state.leaf_preview_run().and_then(|r| r.total_steps)
        };

        let data = SummaryData {
            name: &name,
            runs: &runs,
            run_metrics: &run_metrics,
            aggregate_metrics: &aggregate_metrics,
            unique_configs,
            run_params: &state.run_params,
            metric_histories: &state.metric_histories,
            table: state.cached_table.as_ref(),
            table_title: state.cached_table_title.as_deref(),
            table_axes: state
                .cached_table_axes
                .as_ref()
                .map(|(r, c)| (r.as_str(), c.as_str())),
            preview_total_steps,
            selected_run: state.selected_run,
            panel_width: area.width,
            tag_defs: &state.config.tags.definitions,
        };

        let sections = state.config.summary.sections.clone();
        let total = self.summary.render(
            frame,
            area,
            &data,
            &sections,
            state.summary_scroll,
            state.config.summary.curve_width,
            state.config.summary.curve_height,
            state.config.summary.curve_smooth,
            &state.config.tables,
        );
        state.summary_total_lines = total;
        state.summary_visible_height = area.height as usize;
    }

    fn render_info(&self, frame: &mut Frame, area: Rect, run: &Run, state: &mut AppState) {
        use crate::config::key_passes_filters;

        let mut lines = Vec::new();

        // Build metadata rows as aligned key-value pairs.
        let mut meta: Vec<(&str, String, Option<Style>)> = Vec::new();
        meta.push(("Run ID", run.id.clone(), None));
        if let Some(ref name) = run.name {
            meta.push(("Name", name.clone(), None));
        }
        let status_style = match run.status.as_str() {
            "running" => Some(self.theme.status_running),
            "completed" => Some(self.theme.status_completed),
            "failed" => Some(self.theme.status_failed),
            _ => None,
        };
        meta.push(("Status", run.status.clone(), status_style));
        let time_fmt = &state.config.info.time_format;
        let mut time_parts = vec![crate::config::format_timestamp(&run.started_at, time_fmt)];
        if let Some(ref ended) = run.ended_at {
            time_parts.push(crate::config::format_timestamp(ended, time_fmt));
        }
        meta.push(("Time", time_parts.join(" \u{2192} "), None));
        if let Some(ref hostname) = run.hostname {
            meta.push(("Host", hostname.clone(), None));
        }
        if let Some(ref git_sha) = run.git_sha {
            meta.push(("Git SHA", git_sha.clone(), None));
        }

        let meta_key_width = meta.iter().map(|(k, _, _)| k.len()).max().unwrap_or(4);
        for (label, value, val_style) in &meta {
            let val_span = if let Some(s) = val_style {
                Span::styled(value.clone(), *s)
            } else {
                Span::raw(value.clone())
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<width$}  ", label, width = meta_key_width),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                val_span,
            ]));
        }

        if let Some(ref config) = run.config {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Config",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(config) {
                if let Some(obj) = parsed.as_object() {
                    // Flatten nested config into (dotted_key, leaf_value) pairs.
                    let mut flat: Vec<(String, String)> = Vec::new();
                    fn flatten(
                        prefix: &str,
                        value: &serde_json::Value,
                        out: &mut Vec<(String, String)>,
                    ) {
                        match value {
                            serde_json::Value::Object(map) => {
                                for (k, v) in map {
                                    let key = if prefix.is_empty() {
                                        k.clone()
                                    } else {
                                        format!("{prefix}.{k}")
                                    };
                                    flatten(&key, v, out);
                                }
                            }
                            serde_json::Value::String(s) => {
                                out.push((prefix.to_string(), s.clone()));
                            }
                            serde_json::Value::Null => {
                                out.push((prefix.to_string(), "null".to_string()));
                            }
                            serde_json::Value::Bool(b) => {
                                out.push((prefix.to_string(), b.to_string()));
                            }
                            serde_json::Value::Number(n) => {
                                out.push((prefix.to_string(), n.to_string()));
                            }
                            serde_json::Value::Array(arr) => {
                                out.push((
                                    prefix.to_string(),
                                    serde_json::to_string(arr).unwrap_or_default(),
                                ));
                            }
                        }
                    }
                    for (k, v) in obj {
                        flatten(k, v, &mut flat);
                    }

                    // Apply field filter from config.
                    let filters = &state.config.info.fields;
                    flat.retain(|(k, _)| key_passes_filters(k, filters));

                    let key_width =
                        flat.iter().map(|(k, _)| k.len()).max().unwrap_or(8).max(4);
                    for (k, val_str) in &flat {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {:<width$}  ", k, width = key_width),
                                Style::default().fg(self.theme.accent_dim),
                            ),
                            Span::raw(val_str.clone()),
                        ]));
                    }
                } else {
                    lines.push(Line::from(format!("  {config}")));
                }
            } else {
                lines.push(Line::from(format!("  {config}")));
            }
        }

        if let Some(ref notes) = run.notes {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Notes",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for note_line in notes.lines() {
                lines.push(render_note_line(note_line, &self.theme));
            }
        }

        state.info_total_lines = lines.len();
        state.info_visible_height = area.height as usize;
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.info_scroll, 0));
        frame.render_widget(paragraph, area);
    }

    fn open_run_rename(&self, state: &mut AppState) {
        let Some(run) = state.selected_run.and_then(|idx| state.runs.get(idx)) else {
            return;
        };
        let buffer = run.name.clone().unwrap_or_default();
        let cursor = buffer.chars().count();
        state.run_rename = Some(crate::app::RunRenameState {
            run_id: run.id.clone(),
            buffer,
            cursor,
        });
    }

    fn handle_run_rename_key(&self, key: &KeyEvent, state: &mut AppState) {
        let Some(rename) = state.run_rename.as_mut() else {
            return;
        };

        match key.code {
            KeyCode::Esc => {
                state.run_rename = None;
            }
            KeyCode::Enter => {
                let rename = state.run_rename.take().unwrap();
                let db_path = state.store_root.join("extract.db");
                match crate::db::Db::rename_run(&db_path, &rename.run_id, &rename.buffer) {
                    Ok(()) => {
                        let trimmed = rename.buffer.trim();
                        let value = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        };
                        if let Some(run) = state
                            .runs
                            .iter_mut()
                            .find(|run| run.id == rename.run_id)
                        {
                            run.name = value;
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
            }
            KeyCode::Backspace => {
                let cursor = rename.cursor.min(rename.buffer.chars().count());
                rename.cursor = remove_char_before_cursor(&mut rename.buffer, cursor);
            }
            KeyCode::Left => {
                rename.cursor = rename.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                let len = rename.buffer.chars().count();
                rename.cursor = (rename.cursor + 1).min(len);
            }
            KeyCode::Char(c) => {
                if accepts_text_modifiers(key) {
                    let cursor = rename.cursor.min(rename.buffer.chars().count());
                    let byte_idx = char_to_byte_index(&rename.buffer, cursor);
                    rename.buffer.insert(byte_idx, c);
                    rename.cursor = cursor + 1;
                }
            }
            _ => {}
        }
    }

    fn open_tag_picker(&self, state: &mut AppState) {
        // Determine target entity and current tags.
        let (table, id, current_tags_json) = if let Some(idx) = state.selected_run {
            if let Some(run) = state.runs.get(idx) {
                ("runs".to_string(), run.id.clone(), run.tags.as_deref())
            } else {
                return;
            }
        } else if let Some(idx) = state.selected_experiment {
            if let Some(exp) = state.experiments.get(idx) {
                ("experiments".to_string(), exp.id.clone(), exp.tags.as_deref())
            } else {
                return;
            }
        } else {
            return;
        };

        let current_tags: Vec<String> = current_tags_json
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default();

        // Build candidates: config-defined tags first, then any current tags not in config.
        let mut candidates: Vec<crate::app::TagCandidate> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for def in &state.config.tags.definitions {
            let active = current_tags.contains(&def.name);
            seen.insert(def.name.clone());
            candidates.push(crate::app::TagCandidate {
                name: def.name.clone(),
                color: Some(crate::config::parse_color(&def.color)),
                active,
            });
        }

        for tag in &current_tags {
            if !seen.contains(tag) {
                candidates.push(crate::app::TagCandidate {
                    name: tag.clone(),
                    color: None,
                    active: true,
                });
            }
        }

        let filtered: Vec<usize> = (0..candidates.len()).collect();

        state.tag_picker = Some(crate::app::TagPickerState {
            query: String::new(),
            candidates,
            filtered,
            cursor: 0,
            current_tags,
            table,
            id,
        });
    }

    fn handle_tag_picker_key(&self, key: &KeyEvent, state: &mut AppState) {
        let Some(ref mut picker) = state.tag_picker else { return };

        match key.code {
            crossterm::event::KeyCode::Esc => {
                state.tag_picker = None;
            }
            crossterm::event::KeyCode::Enter => {
                // Toggle the selected candidate, or create a new tag from query text.
                if let Some(&fi) = picker.filtered.get(picker.cursor) {
                    let name = picker.candidates[fi].name.clone();
                    if picker.current_tags.contains(&name) {
                        picker.current_tags.retain(|t| t != &name);
                        picker.candidates[fi].active = false;
                    } else {
                        picker.current_tags.push(name.clone());
                        picker.candidates[fi].active = true;
                    }
                } else if picker.query_is_new_tag() {
                    // Create a new tag from the query text.
                    let new_tag = picker.query.trim().to_string();
                    picker.current_tags.push(new_tag.clone());
                    picker.candidates.push(crate::app::TagCandidate {
                        name: new_tag,
                        color: None,
                        active: true,
                    });
                    picker.query.clear();
                    picker.apply_filter();
                }
                // Save tags immediately.
                let tags_json = serde_json::to_string(&picker.current_tags)
                    .unwrap_or_else(|_| "[]".to_string());
                let db_path = state.store_root.join("extract.db");
                let _ = crate::db::Db::update_tags(&db_path, &picker.table, &picker.id, &tags_json);
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Tab => {
                if !picker.filtered.is_empty() && picker.cursor + 1 < picker.filtered.len() {
                    picker.cursor += 1;
                }
            }
            crossterm::event::KeyCode::Up => {
                if picker.cursor > 0 {
                    picker.cursor -= 1;
                }
            }
            crossterm::event::KeyCode::Backspace => {
                picker.query.pop();
                picker.apply_filter();
            }
            crossterm::event::KeyCode::Char(c) => {
                if key.modifiers == crossterm::event::KeyModifiers::NONE
                    || key.modifiers == crossterm::event::KeyModifiers::SHIFT
                {
                    picker.query.push(c);
                    picker.apply_filter();
                }
            }
            _ => {}
        }
    }

    pub fn render_tag_picker(&self, frame: &mut Frame, area: Rect, picker: &crate::app::TagPickerState) {
        let max_results = 8;
        let visible_count = picker.filtered.len().min(max_results);
        let has_new_tag = picker.query_is_new_tag();
        let extra_lines = if has_new_tag { 1 } else { 0 };
        let height = (visible_count as u16 + extra_lines + 3).min(area.height.saturating_sub(4));
        let width = 50u16.min(area.width.saturating_sub(4));

        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + 2;
        let popup_area = Rect::new(x, y, width, height.max(3));

        frame.render_widget(ratatui::widgets::Clear, popup_area);

        // Show current tags in the title.
        let title = if picker.current_tags.is_empty() {
            " Tags ".to_string()
        } else {
            format!(" Tags: {} ", picker.current_tags.join(", "))
        };

        let block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(ratatui::symbols::border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Input line.
        let prompt = Span::styled(" > ", Style::default().fg(self.theme.accent));
        let query_text = Span::raw(picker.query.clone());
        let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
        lines.push(Line::from(vec![prompt, query_text, cursor]));

        // Filtered candidates.
        let default_colors = [
            ratatui::style::Color::Magenta,
            ratatui::style::Color::Blue,
            ratatui::style::Color::Cyan,
            ratatui::style::Color::Green,
            ratatui::style::Color::Yellow,
        ];

        for (vi, &ci) in picker.filtered.iter().enumerate().take(max_results) {
            let cand = &picker.candidates[ci];
            let is_selected = vi == picker.cursor;
            let check = if cand.active { "[x] " } else { "[ ] " };
            let bg = cand.color.unwrap_or(default_colors[ci % default_colors.len()]);

            let check_span = Span::styled(
                check,
                if is_selected { self.theme.selected } else { Style::default() },
            );
            let chip = Span::styled(
                format!(" {} ", cand.name),
                Style::default().fg(ratatui::style::Color::Black).bg(bg).add_modifier(Modifier::BOLD),
            );
            let name_span = Span::styled(
                format!(" {}", cand.name),
                if is_selected { self.theme.selected } else { Style::default() },
            );

            lines.push(Line::from(vec![
                Span::raw(" "),
                check_span,
                chip,
                name_span,
            ]));
        }

        // Show "create new tag" option if query doesn't match any candidate.
        if has_new_tag {
            let is_selected = picker.cursor >= picker.filtered.len();
            let style = if is_selected { self.theme.selected } else { Style::default().fg(self.theme.accent) };
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(format!("+ create \"{}\"", picker.query.trim()), style),
            ]));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    pub fn render_note_popup(&self, frame: &mut Frame, area: Rect, input: &str) {
        self.render_text_input_popup(frame, area, " Append Note ", input);
    }

    pub fn render_run_rename_popup(
        &self,
        frame: &mut Frame,
        area: Rect,
        rename: &crate::app::RunRenameState,
    ) {
        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = 3u16;
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(ratatui::widgets::Clear, popup_area);

        let block = Block::bordered()
            .title(" Rename Run ")
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(ratatui::symbols::border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let line = Line::from(rename_input_spans(&rename.buffer, rename.cursor));
        frame.render_widget(Paragraph::new(line), inner);
    }

    fn render_text_input_popup(
        &self,
        frame: &mut Frame,
        area: Rect,
        title: &'static str,
        input: &str,
    ) {
        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = 3u16;
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(ratatui::widgets::Clear, popup_area);

        let block = Block::bordered()
            .title(title)
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(ratatui::symbols::border::ROUNDED);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
        let line = Line::from(vec![Span::raw(input.to_string()), cursor]);
        frame.render_widget(Paragraph::new(line), inner);
    }
}

fn rename_input_spans(input: &str, cursor: usize) -> Vec<Span<'static>> {
    let cursor = cursor.min(input.chars().count());
    let mut chars = input.chars();
    let before: String = chars.by_ref().take(cursor).collect();
    let cursor_style = Style::default().add_modifier(Modifier::REVERSED | Modifier::SLOW_BLINK);
    let mut spans = Vec::new();

    if !before.is_empty() {
        spans.push(Span::raw(before));
    }

    if let Some(cursor_char) = chars.next() {
        spans.push(Span::styled(cursor_char.to_string(), cursor_style));
        let after: String = chars.collect();
        if !after.is_empty() {
            spans.push(Span::raw(after));
        }
    } else {
        spans.push(Span::styled(" ", cursor_style));
    }

    spans
}

fn accepts_text_modifiers(key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT
}

fn char_to_byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| s.len())
}

fn remove_char_before_cursor(s: &mut String, cursor: usize) -> usize {
    if cursor == 0 || s.is_empty() {
        return cursor;
    }
    let remove_at = cursor - 1;
    let start = char_to_byte_index(s, remove_at);
    let end = char_to_byte_index(s, cursor);
    s.replace_range(start..end, "");
    remove_at
}

/// Render a single line of notes text, highlighting LaTeX math delimiters.
///
/// `$...$` and `$$...$$` blocks are rendered in italic + a distinct color
/// so they visually stand out as math. Common LaTeX commands get unicode
/// substitutions for readability: \alpha → α, \beta → β, \sum → Σ, etc.
fn render_note_line<'a>(text: &str, theme: &crate::ui::theme::Theme) -> Line<'a> {
    let math_style = Style::default()
        .fg(theme.warning)
        .add_modifier(Modifier::ITALIC);
    let text_style = Style::default();

    let mut spans: Vec<Span<'a>> = vec![Span::raw("  ")]; // indent
    let mut rest = text;

    while !rest.is_empty() {
        // Try $$ (display math) first, then $ (inline math).
        if let Some(start) = rest.find("$$") {
            // Push text before the delimiter.
            if start > 0 {
                spans.push(Span::styled(rest[..start].to_string(), text_style));
            }
            let after_open = &rest[start + 2..];
            if let Some(end) = after_open.find("$$") {
                let math = &after_open[..end];
                spans.push(Span::styled(
                    format!(" {} ", latex_to_unicode(math)),
                    math_style,
                ));
                rest = &after_open[end + 2..];
            } else {
                // No closing $$ — render rest as-is.
                spans.push(Span::styled(rest[start..].to_string(), math_style));
                rest = "";
            }
        } else if let Some(start) = rest.find('$') {
            if start > 0 {
                spans.push(Span::styled(rest[..start].to_string(), text_style));
            }
            let after_open = &rest[start + 1..];
            if let Some(end) = after_open.find('$') {
                let math = &after_open[..end];
                spans.push(Span::styled(
                    format!(" {} ", latex_to_unicode(math)),
                    math_style,
                ));
                rest = &after_open[end + 1..];
            } else {
                spans.push(Span::styled(rest[start..].to_string(), text_style));
                rest = "";
            }
        } else {
            spans.push(Span::styled(rest.to_string(), text_style));
            rest = "";
        }
    }

    Line::from(spans)
}

/// Best-effort substitution of common LaTeX commands with unicode equivalents.
fn latex_to_unicode(math: &str) -> String {
    let mut s = math.to_string();
    let replacements = [
        // Greek lowercase
        ("\\alpha", "α"), ("\\beta", "β"), ("\\gamma", "γ"), ("\\delta", "δ"),
        ("\\epsilon", "ε"), ("\\zeta", "ζ"), ("\\eta", "η"), ("\\theta", "θ"),
        ("\\iota", "ι"), ("\\kappa", "κ"), ("\\lambda", "λ"), ("\\mu", "μ"),
        ("\\nu", "ν"), ("\\xi", "ξ"), ("\\pi", "π"), ("\\rho", "ρ"),
        ("\\sigma", "σ"), ("\\tau", "τ"), ("\\phi", "φ"), ("\\chi", "χ"),
        ("\\psi", "ψ"), ("\\omega", "ω"),
        // Greek uppercase
        ("\\Gamma", "Γ"), ("\\Delta", "Δ"), ("\\Theta", "Θ"), ("\\Lambda", "Λ"),
        ("\\Xi", "Ξ"), ("\\Pi", "Π"), ("\\Sigma", "Σ"), ("\\Phi", "Φ"),
        ("\\Psi", "Ψ"), ("\\Omega", "Ω"),
        // Operators & symbols
        ("\\sum", "Σ"), ("\\prod", "Π"), ("\\int", "∫"),
        ("\\infty", "∞"), ("\\partial", "∂"), ("\\nabla", "∇"),
        ("\\approx", "≈"), ("\\neq", "≠"), ("\\leq", "≤"), ("\\geq", "≥"),
        ("\\pm", "±"), ("\\times", "×"), ("\\cdot", "·"), ("\\div", "÷"),
        ("\\sqrt", "√"), ("\\propto", "∝"),
        ("\\in", "∈"), ("\\notin", "∉"), ("\\subset", "⊂"), ("\\supset", "⊃"),
        ("\\cup", "∪"), ("\\cap", "∩"), ("\\emptyset", "∅"),
        ("\\forall", "∀"), ("\\exists", "∃"),
        ("\\rightarrow", "→"), ("\\leftarrow", "←"), ("\\Rightarrow", "⇒"),
        ("\\ell", "ℓ"), ("\\mathcal", ""), ("\\mathrm", ""), ("\\mathbb", ""),
        ("\\hat", "̂"), ("\\bar", "̄"), ("\\tilde", "̃"),
        ("\\frac", "/"), ("\\log", "log"), ("\\exp", "exp"), ("\\max", "max"),
        ("\\min", "min"), ("\\arg", "arg"), ("\\lim", "lim"),
        // Subscript/superscript markers (best effort)
        ("_", "₋"), ("^", "^"),
    ];
    for (from, to) in replacements {
        s = s.replace(from, to);
    }
    // Clean up braces that were part of LaTeX grouping.
    s = s.replace('{', "").replace('}', "");
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use rusqlite::Connection;

    fn setup_state() -> (tempfile::TempDir, AppState, DetailPanel) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("extract.db");

        let writer = Connection::open(&db_path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL").unwrap();
        writer
            .execute_batch(include_str!("../../../schema/migrations/001_init.sql"))
            .unwrap();
        writer
            .execute_batch(include_str!("../../../schema/migrations/002_experiment_metadata.sql"))
            .unwrap();
        writer
            .execute_batch(
                "INSERT INTO hierarchy VALUES (0, 'benchmark');
                 INSERT INTO experiments VALUES ('e1', 'a', 'a', NULL, '2026-01-01T00:00:00Z', NULL, 'active', 'benchmark', NULL, NULL);
                 INSERT INTO runs VALUES ('r1', 'e1', 'old', NULL, '2026-01-01T00:00:00Z', NULL, 'completed', NULL, NULL, '[]', NULL, 10);",
            )
            .unwrap();
        drop(writer);

        let db = crate::db::Db::open(&db_path).unwrap();
        let mut state = AppState::new(db, tmp.path().to_path_buf()).unwrap();
        state.selected_experiment = Some(0);
        state.refresh_runs().unwrap();
        state.selected_run = Some(0);
        state.load_run_preview(0).unwrap();
        state.focus = Focus::Detail;

        let mut panel = DetailPanel::new(Theme::default());
        panel.active_tab = DetailTab::Summary;
        (tmp, state, panel)
    }

    #[test]
    fn rename_input_uses_block_cursor_at_end() {
        let spans = rename_input_spans("old", 3);
        let cursor = spans.last().expect("cursor span");

        assert_eq!(cursor.content.as_ref(), " ");
        assert!(cursor.style.add_modifier.contains(Modifier::REVERSED));
        assert!(cursor.style.add_modifier.contains(Modifier::SLOW_BLINK));
    }

    #[test]
    fn rename_input_uses_block_cursor_over_character() {
        let spans = rename_input_spans("old", 1);
        let cursor = &spans[1];

        assert_eq!(cursor.content.as_ref(), "l");
        assert!(cursor.style.add_modifier.contains(Modifier::REVERSED));
        assert!(!spans.iter().any(|span| span.content.as_ref() == "_"));
    }

    #[test]
    fn shift_r_opens_summary_run_rename_for_selected_run() {
        let (_tmp, mut state, mut panel) = setup_state();

        panel.handle_event(
            &AppEvent::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            &mut state,
        );

        let rename = state.run_rename.as_ref().expect("rename input should open");
        assert_eq!(rename.run_id, "r1");
        assert_eq!(rename.buffer, "old");
        assert_eq!(rename.cursor, 3);
    }

    #[test]
    fn summary_run_rename_commits_to_db_and_visible_state() {
        let (_tmp, mut state, mut panel) = setup_state();

        panel.handle_event(
            &AppEvent::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            &mut state,
        );
        let rename = state.run_rename.as_mut().unwrap();
        rename.buffer = "new name".to_string();
        rename.cursor = rename.buffer.chars().count();

        panel.handle_event(
            &AppEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            &mut state,
        );

        assert!(state.run_rename.is_none());
        assert_eq!(
            state.db.get_run("r1").unwrap().unwrap().name.as_deref(),
            Some("new name")
        );
        assert_eq!(state.runs[0].name.as_deref(), Some("new name"));
        match &state.selection_summary {
            SelectionSummary::Leaf { runs, .. } => {
                assert_eq!(runs[0].name.as_deref(), Some("new name"));
            }
            _ => panic!("expected leaf selection summary"),
        }
    }

    #[test]
    fn shift_r_does_not_open_rename_on_info_tab() {
        let (_tmp, mut state, mut panel) = setup_state();
        panel.active_tab = DetailTab::Info;

        panel.handle_event(
            &AppEvent::Key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            &mut state,
        );

        assert!(state.run_rename.is_none());
    }
}
