# Compare/Diff v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Nine improvements to the compare/diff feature: run-only selection with picker popup, proper run labels, table layout intelligence, multi-run diff with baselines, selection management UI, run cycling, and run deletion.

**Architecture:** The existing compare/diff views get enhanced in-place. Two new UI modules are added: `popup.rs` (centered overlays for run picker and delete confirmation) and `selection.rs` (floating selection window). The Focus enum gains a Selection variant. AppState gains fields for baseline tracking, marked experiment caching, and popup state. The DB module gets a writable connection path for deletions.

**Tech Stack:** Rust, ratatui 0.29, rusqlite (bundled), crossterm 0.28, tui-tree-widget

**Spec:** `docs/superpowers/specs/2026-04-04-compare-diff-v2-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `rust/src/keys.rs` | Modify | Add CYCLE_PREV, CYCLE_NEXT, DELETE, BASELINE constants |
| `rust/src/app.rs` | Modify | Focus::Selection, new AppState fields, CompareRunData.experiment_name, label logic, load_compare_data changes, helper methods |
| `rust/src/db.rs` | Modify | Add `delete_run()` with separate writable connection |
| `rust/src/ui/mod.rs` | Modify | Declare popup and selection modules |
| `rust/src/ui/popup.rs` | Create | RunPickerPopup + DeleteConfirmPopup overlay components |
| `rust/src/ui/selection.rs` | Create | Floating selection window component |
| `rust/src/ui/tree.rs` | Modify | Replace Space handler with run picker logic, add tree markers |
| `rust/src/ui/detail.rs` | Modify | Add [/] cycling, x delete, fix default to newest, Tab routing to Selection |
| `rust/src/ui/compare.rs` | Modify | Curve width fix, highlight rules, side-by-side table layout |
| `rust/src/ui/diff.rs` | Modify | Multi-run baseline, side-by-side delta tables, N-run metric deltas and config changes |
| `rust/src/ui/layout.rs` | Modify | Selection window + popup overlay rendering, Focus::Selection event routing |
| `rust/src/ui/statusbar.rs` | Modify | Updated key hints for all new bindings |

---

### Task 1: Foundation — keys, Focus, AppState fields

**Files:**
- Modify: `rust/src/keys.rs`
- Modify: `rust/src/app.rs`

- [ ] **Step 1: Add key constants to keys.rs**

Add after the existing constants at the end of the constants block (before `matches` function):

```rust
pub const CYCLE_PREV: KeyCode = KeyCode::Char('[');
pub const CYCLE_NEXT: KeyCode = KeyCode::Char(']');
pub const DELETE: KeyCode = KeyCode::Char('x');
pub const BASELINE: KeyCode = KeyCode::Char('b');
pub const YES: KeyCode = KeyCode::Char('y');
pub const NO: KeyCode = KeyCode::Char('n');
```

- [ ] **Step 2: Add Focus::Selection variant**

In `app.rs`, update the Focus enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Detail,
    Selection,
}
```

- [ ] **Step 3: Add experiment_name to CompareRunData and fix label()**

```rust
pub struct CompareRunData {
    pub run: Run,
    pub experiment_name: String,
    pub latest_metrics: Vec<ScalarMetric>,
    pub run_params: Vec<RunParam>,
    pub config: Option<JsonValue>,
    pub metric_histories: Vec<(String, Vec<ScalarMetric>)>,
    pub tables: Vec<(String, TableData, Option<(String, String)>)>,
}

impl CompareRunData {
    pub fn label(&self) -> String {
        if let Some(ref name) = self.run.name {
            return name.clone();
        }
        self.experiment_name.clone()
    }
}
```

The `#N` suffix for duplicate experiment names will be handled in `load_compare_data` (Task 2).

- [ ] **Step 4: Add new fields to AppState**

Add these fields to AppState struct after `compare_data`:

```rust
    pub compare_data: Option<CompareData>,
    pub compare_baseline: usize,
    pub marked_experiment_ids: std::collections::HashSet<String>,
    pub selection_cursor: usize,
    pub run_picker: Option<RunPickerState>,
    pub delete_confirm: Option<DeleteConfirmState>,
```

Add the popup state structs before AppState:

```rust
/// State for the run picker popup.
pub struct RunPickerState {
    pub experiment_name: String,
    pub runs: Vec<Run>,
    pub selected: Vec<String>,  // run IDs toggled on
    pub cursor: usize,
}

/// State for the delete confirmation popup.
pub struct DeleteConfirmState {
    pub run_id: String,
    pub label: String,
}
```

- [ ] **Step 5: Update AppState::new() with field initializers**

Add to the initializer in `new()`:

```rust
            compare_data: None,
            compare_baseline: 0,
            marked_experiment_ids: std::collections::HashSet::new(),
            selection_cursor: 0,
            run_picker: None,
            delete_confirm: None,
```

- [ ] **Step 6: Add helper to recompute marked_experiment_ids**

Add method to AppState:

```rust
    pub fn refresh_marked_experiments(&mut self) {
        self.marked_experiment_ids.clear();
        for run_id in &self.selected_runs_for_compare {
            if let Ok(Some(run)) = self.db.get_run(run_id) {
                self.marked_experiment_ids.insert(run.experiment_id.clone());
            }
        }
    }
```

- [ ] **Step 7: Add helper to load preview for a specific run by index**

Add method to AppState (reuses existing loading logic but for a specific run):

```rust
    pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
        self.summary_scroll = 0;
        let Some(run) = self.runs.get(run_idx) else {
            return Ok(());
        };
        let run_id = run.id.clone();

        self.load_all_metric_histories(&run_id)?;
        self.run_params = self.db.list_run_params(&run_id)?;
        self.artifacts = self.db.list_artifacts(&run_id)?;
        self.load_first_table()?;
        Ok(())
    }
```

- [ ] **Step 8: Build and verify**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no errors (warnings about unused fields are fine at this stage)

- [ ] **Step 9: Commit**

```
git add rust/src/keys.rs rust/src/app.rs
git commit -m "feat: foundation — keys, Focus::Selection, AppState fields for v2"
```

---

### Task 2: Run labels in load_compare_data

**Files:**
- Modify: `rust/src/app.rs`

- [ ] **Step 1: Update load_compare_data to populate experiment_name and handle #N suffix**

In `load_compare_data`, after building runs_data, update the experiment_name field and handle duplicates. Replace the `runs_data.push(CompareRunData { ... })` block and the code after the main loop:

In the per-run loading section, after `let run = ...` and the dedup check, add the experiment lookup:

```rust
            let experiment_name = self
                .db
                .get_experiment(&run.experiment_id)?
                .map(|e| e.name.clone())
                .unwrap_or_else(|| {
                    let id = &run.id;
                    if id.len() > 8 { id[id.len() - 8..].to_string() } else { id.clone() }
                });
```

Update the push to include it:

```rust
            runs_data.push(CompareRunData {
                run,
                experiment_name,
                latest_metrics,
                // ... rest unchanged
            });
```

After the main loop, before computing metric_names etc., add dedup suffix logic:

```rust
        // Add #N suffix for runs sharing the same experiment_name
        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for rd in &runs_data {
            *name_counts.entry(rd.experiment_name.clone()).or_default() += 1;
        }
        let mut name_indices: HashMap<String, usize> = HashMap::new();
        for rd in &mut runs_data {
            let count = name_counts[&rd.experiment_name];
            if count > 1 {
                let idx = name_indices.entry(rd.experiment_name.clone()).or_insert(0);
                *idx += 1;
                rd.experiment_name = format!("{} #{}", rd.experiment_name, idx);
            }
        }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no errors

- [ ] **Step 3: Commit**

```
git add rust/src/app.rs
git commit -m "feat: run labels show experiment name with #N suffix for duplicates"
```

---

### Task 3: Curve width + table highlight rules in compare view

**Files:**
- Modify: `rust/src/ui/compare.rs`

- [ ] **Step 1: Fix curve width to use config percentage**

In the `render` method, change the Curves section in the match:

```rust
                    CompareSection::Curves => {
                        let chart_width = ((inner.width as f32)
                            * (state.config.summary.curve_width.min(100) as f32 / 100.0))
                            as u16;
                        self.build_overlay_charts(
                            &mut lines,
                            data,
                            chart_width.max(20),
                        )
                    }
```

- [ ] **Step 2: Add highlight rules to build_tables_section**

Change the signature to accept highlight rules:

```rust
    fn build_tables_section(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        tables_config: &crate::config::TablesConfig,
        available_width: u16,
    ) {
```

Add the import at the top of the file:

```rust
use crate::config::parse_color;
use crate::ui::summary::match_highlight_rule;
```

Wait — `match_highlight_rule` is private in summary.rs. Make it `pub(crate)`:

In `rust/src/ui/summary.rs`, change:
```rust
fn match_highlight_rule<'a>(cell: &CellValue, rules: &'a [HighlightRule]) -> &'a str {
```
to:
```rust
pub(crate) fn match_highlight_rule<'a>(cell: &CellValue, rules: &'a [HighlightRule]) -> &'a str {
```

Then in compare.rs `build_tables_section`, replace the cell rendering loop:

```rust
                    for r in 0..table.rows {
                        let mut spans: Vec<Span<'static>> = vec![Span::styled(
                            format!("  R{:<3} ", r + 1),
                            Style::default().fg(self.theme.accent_dim),
                        )];
                        for c in 0..table.cols {
                            let cell = &table.values[r][c];
                            let color_name = crate::ui::summary::match_highlight_rule(
                                cell,
                                &tables_config.highlight,
                            );
                            if color_name == "transparent" {
                                spans.push(Span::raw(" ".repeat(cell_width)));
                            } else {
                                let display = cell.display(cell_width);
                                spans.push(Span::styled(
                                    display,
                                    Style::default().fg(parse_color(color_name)),
                                ));
                            }
                        }
                        lines.push(Line::from(spans));
                    }
```

Update the call site in `render` to pass config:

```rust
                    CompareSection::Tables => self.build_tables_section(
                        &mut lines,
                        data,
                        &state.config.tables,
                        inner.width,
                    ),
```

- [ ] **Step 3: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 4: Commit**

```
git add rust/src/ui/compare.rs rust/src/ui/summary.rs
git commit -m "fix: curve width respects config, tables use highlight rules in compare view"
```

---

### Task 4: Side-by-side table layout

**Files:**
- Modify: `rust/src/ui/compare.rs`
- Modify: `rust/src/ui/diff.rs`

- [ ] **Step 1: Rewrite build_tables_section with side-by-side layout**

Replace the entire `build_tables_section` method in compare.rs:

```rust
    fn build_tables_section(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        tables_config: &crate::config::TablesConfig,
        available_width: u16,
    ) {
        if data.table_names.is_empty() {
            return;
        }

        let cell_width: usize = 6;
        let row_label_w: usize = 6; // "  R1  "
        let gap: usize = 3;

        for table_name in &data.table_names {
            // Collect (run_index, table) for runs that have this artifact
            let run_tables: Vec<(usize, &crate::artifact::TableData)> = data
                .runs
                .iter()
                .enumerate()
                .filter_map(|(i, rd)| {
                    rd.tables
                        .iter()
                        .find(|(n, _, _)| n == table_name)
                        .map(|(_, t, _)| (i, t))
                })
                .collect();

            if run_tables.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {table_name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            let first_table = run_tables[0].1;
            let table_w = row_label_w + first_table.cols * cell_width;
            let tables_per_row = ((available_width as usize + gap) / (table_w + gap))
                .max(1)
                .min(run_tables.len());

            for chunk in run_tables.chunks(tables_per_row) {
                // Run label headers
                let mut header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, &(ri, _)) in chunk.iter().enumerate() {
                    if ci > 0 {
                        header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    let color = RUN_COLORS[ri % RUN_COLORS.len()];
                    header_spans.push(Span::styled(
                        format!("  {:<width$}", data.runs[ri].label(), width = table_w - 2),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                }
                lines.push(Line::from(header_spans));

                // Table rows
                let n_rows = chunk.iter().map(|(_, t)| t.rows).max().unwrap_or(0);
                for r in 0..n_rows {
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    for (ci, &(_, table)) in chunk.iter().enumerate() {
                        if ci > 0 {
                            spans.push(Span::raw(" ".repeat(gap)));
                        }
                        spans.push(Span::styled(
                            format!("  R{:<3}", r + 1),
                            Style::default().fg(self.theme.accent_dim),
                        ));
                        if r < table.rows {
                            for c in 0..table.cols {
                                let cell = &table.values[r][c];
                                let color_name = crate::ui::summary::match_highlight_rule(
                                    cell,
                                    &tables_config.highlight,
                                );
                                if color_name == "transparent" {
                                    spans.push(Span::raw(" ".repeat(cell_width)));
                                } else {
                                    let display = cell.display(cell_width);
                                    spans.push(Span::styled(
                                        display,
                                        Style::default().fg(parse_color(color_name)),
                                    ));
                                }
                            }
                        }
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }
        }
    }
```

- [ ] **Step 2: Apply side-by-side layout to diff.rs delta tables**

Rewrite `build_delta_tables` in diff.rs to handle N runs vs baseline with side-by-side layout. The baseline is `data.runs[baseline_idx]`. Each non-baseline run produces a delta table. These are laid out side-by-side using the same algorithm.

```rust
    fn build_delta_tables(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
        available_width: u16,
    ) {
        if data.table_names.is_empty() {
            return;
        }

        let cell_width: usize = 8;
        let row_label_w: usize = 6;
        let gap: usize = 3;

        for table_name in &data.table_names {
            let baseline_table = data.runs[baseline_idx]
                .tables
                .iter()
                .find(|(n, _, _)| n == table_name)
                .map(|(_, t, _)| t);

            let Some(bt) = baseline_table else { continue };

            // Collect non-baseline runs that have this table with matching dimensions
            let delta_runs: Vec<(usize, &crate::artifact::TableData)> = data
                .runs
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != baseline_idx)
                .filter_map(|(i, rd)| {
                    rd.tables
                        .iter()
                        .find(|(n, _, _)| n == table_name)
                        .filter(|(_, t, _)| t.rows == bt.rows && t.cols == bt.cols)
                        .map(|(_, t, _)| (i, t))
                })
                .collect();

            if delta_runs.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  Delta: {table_name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            let table_w = row_label_w + bt.cols * cell_width;
            let tables_per_row = ((available_width as usize + gap) / (table_w + gap))
                .max(1)
                .min(delta_runs.len());

            for chunk in delta_runs.chunks(tables_per_row) {
                // Run label headers: "run_N - baseline"
                let mut header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, &(ri, _)) in chunk.iter().enumerate() {
                    if ci > 0 {
                        header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    let color = RUN_COLORS[ri % RUN_COLORS.len()];
                    let label = format!(
                        "{} - {}",
                        data.runs[ri].label(),
                        data.runs[baseline_idx].label()
                    );
                    header_spans.push(Span::styled(
                        format!("  {:<width$}", label, width = table_w - 2),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                }
                lines.push(Line::from(header_spans));

                // Column headers
                let mut col_header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, _) in chunk.iter().enumerate() {
                    if ci > 0 {
                        col_header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    col_header_spans.push(Span::raw(format!("{:>row_label_w$}", "")));
                    for c in 0..bt.cols {
                        col_header_spans.push(Span::styled(
                            format!("{:>cell_width$}", format!("C{}", c + 1)),
                            Style::default().fg(self.theme.accent_dim),
                        ));
                    }
                }
                lines.push(Line::from(col_header_spans));

                // Delta rows
                for r in 0..bt.rows {
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    for (ci, &(_, table)) in chunk.iter().enumerate() {
                        if ci > 0 {
                            spans.push(Span::raw(" ".repeat(gap)));
                        }
                        spans.push(Span::styled(
                            format!("  R{:<3}", r + 1),
                            Style::default().fg(self.theme.accent_dim),
                        ));
                        for c in 0..bt.cols {
                            let v_base = bt.values[r][c].as_f64();
                            let v_run = table.values[r][c].as_f64();
                            match (v_base, v_run) {
                                (Some(a), Some(b)) => {
                                    let delta = b - a;
                                    let color = if delta.abs() < f64::EPSILON {
                                        self.theme.accent_dim
                                    } else if delta > 0.0 {
                                        self.theme.success
                                    } else {
                                        self.theme.error
                                    };
                                    let sign = if delta > 0.0 { "+" } else { "" };
                                    spans.push(Span::styled(
                                        format!("{:>cell_width$}", format!("{sign}{:.2}", delta)),
                                        Style::default().fg(color),
                                    ));
                                }
                                _ => {
                                    spans.push(Span::styled(
                                        format!("{:>cell_width$}", "\u{00b7}"),
                                        Style::default().fg(self.theme.accent_dim),
                                    ));
                                }
                            }
                        }
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }
        }
    }
```

Add the RUN_COLORS constant to diff.rs (same as compare.rs):

```rust
use ratatui::style::Color;

const RUN_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Red,
];
```

Update the call site in `render` to pass baseline_idx and width. This will be fully wired in Task 7.

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no errors

- [ ] **Step 4: Commit**

```
git add rust/src/ui/compare.rs rust/src/ui/diff.rs
git commit -m "feat: side-by-side table layout in compare and diff views"
```

---

### Task 5: Run cycling in detail panel

**Files:**
- Modify: `rust/src/ui/detail.rs`
- Modify: `rust/src/ui/tree.rs`

- [ ] **Step 1: Fix default selected_run to newest**

In `tree.rs`, in the `handle_key` method's SELECT handler (the block that sets `state.selected_run = Some(0)`), change to:

```rust
                        if !state.runs.is_empty() {
                            state.selected_run = Some(state.runs.len() - 1);
                        }
```

- [ ] **Step 2: Add [/] cycling keys in detail.rs**

In `detail.rs` `handle_key`, add before the TOGGLE_SELECT handler:

```rust
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
```

- [ ] **Step 3: Show run N/M in detail title**

In `detail.rs` `render`, change the block title to show current run position:

```rust
        let run_indicator = if state.runs.len() > 1 {
            format!(
                " run {}/{} ",
                state.selected_run.map(|i| i + 1).unwrap_or(0),
                state.runs.len()
            )
        } else {
            String::new()
        };

        let block = Block::bordered()
            .title(format!(" Detail{run_indicator}"))
            .border_style(border_style);
```

- [ ] **Step 4: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 5: Commit**

```
git add rust/src/ui/detail.rs rust/src/ui/tree.rs
git commit -m "feat: [/] to cycle runs in detail panel, default to newest"
```

---

### Task 6: Run picker popup

**Files:**
- Create: `rust/src/ui/popup.rs`
- Modify: `rust/src/ui/mod.rs`
- Modify: `rust/src/ui/tree.rs`
- Modify: `rust/src/ui/layout.rs`

- [ ] **Step 1: Create popup.rs**

```rust
use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, DeleteConfirmState, RunPickerState};
use crate::keys;
use crate::ui::theme::Theme;

pub struct PopupRenderer {
    theme: Theme,
}

impl PopupRenderer {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    /// Handle key events for the run picker popup. Returns true if popup should close.
    pub fn handle_run_picker_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(picker) = &mut state.run_picker else {
            return true;
        };

        if keys::matches(key, keys::BACK_ESC) || keys::matches(key, keys::SELECT) {
            // Apply selections: add newly selected, remove deselected
            let selected = picker.selected.clone();
            for run_id in &selected {
                if !state.selected_runs_for_compare.contains(run_id) {
                    state.selected_runs_for_compare.push(run_id.clone());
                }
            }
            // Remove runs from this experiment that were deselected
            let all_run_ids: Vec<String> = picker.runs.iter().map(|r| r.id.clone()).collect();
            state.selected_runs_for_compare.retain(|id| {
                !all_run_ids.contains(id) || selected.contains(id)
            });
            state.refresh_marked_experiments();
            state.run_picker = None;
            return true;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if picker.cursor + 1 < picker.runs.len() {
                picker.cursor += 1;
            }
            return false;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            picker.cursor = picker.cursor.saturating_sub(1);
            return false;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            if let Some(run) = picker.runs.get(picker.cursor) {
                let run_id = run.id.clone();
                if picker.selected.contains(&run_id) {
                    picker.selected.retain(|id| id != &run_id);
                } else {
                    picker.selected.push(run_id);
                }
            }
            return false;
        }

        false
    }

    /// Handle key events for delete confirmation. Returns Some(true) to confirm, Some(false) to cancel.
    pub fn handle_delete_confirm_key(&self, key: &KeyEvent) -> Option<bool> {
        if keys::matches(key, keys::YES) {
            return Some(true);
        }
        // Any other key cancels
        Some(false)
    }

    pub fn render_run_picker(&self, frame: &mut Frame, area: Rect, picker: &RunPickerState) {
        let height = (picker.runs.len() as u16 + 4).min(area.height.saturating_sub(4));
        let width = 50.min(area.width.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(format!(" Select runs: {} ", picker.experiment_name))
            .border_style(Style::default().fg(self.theme.border_focused));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(Line::from(Span::styled(
            " Space: toggle  Enter/Esc: done",
            Style::default().fg(self.theme.accent_dim),
        )));

        for (i, run) in picker.runs.iter().enumerate() {
            let is_selected = picker.selected.contains(&run.id);
            let is_cursor = i == picker.cursor;

            let marker = if is_selected { "\u{2713} " } else { "  " };
            let date = run.ended_at.as_deref()
                .or(Some(&run.started_at))
                .and_then(|d| d.get(..10))
                .unwrap_or("          ");
            let status_style = match run.status.as_str() {
                "completed" => self.theme.status_completed,
                "running" => self.theme.status_running,
                "failed" => self.theme.status_failed,
                _ => Style::default(),
            };

            let config_hint = run.config.as_ref()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| v.as_object().map(|o| {
                    o.iter()
                        .take(3)
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                }))
                .unwrap_or_default();

            let line_style = if is_cursor {
                self.theme.selected
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {marker}"),
                    if is_selected {
                        Style::default().fg(self.theme.success)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(format!("{:<11}", run.status), status_style),
                Span::raw(format!("{date} ")),
                Span::styled(config_hint, Style::default().fg(self.theme.accent_dim)),
            ]).style(line_style));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }

    pub fn render_delete_confirm(
        &self,
        frame: &mut Frame,
        area: Rect,
        confirm: &DeleteConfirmState,
    ) {
        let popup_area = centered_rect(40, 5, area);
        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" Confirm Delete ")
            .border_style(Style::default().fg(self.theme.error));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let lines = vec![
            Line::from(format!(" Delete run {}?", confirm.label)),
            Line::from(Span::styled(
                " [y] confirm  [any key] cancel",
                Style::default().fg(self.theme.accent_dim),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
```

- [ ] **Step 2: Declare popup module in mod.rs**

```rust
pub mod popup;
```

- [ ] **Step 3: Update tree.rs Space handler to open run picker**

Replace the TOGGLE_SELECT handler in tree.rs:

```rust
        if keys::matches(key, keys::TOGGLE_SELECT) {
            let selected = self.tree_state.selected().to_vec();
            if let Some(last_id) = selected.last() {
                // Only allow on leaf experiments (no children)
                let has_children = state
                    .experiments
                    .iter()
                    .any(|e| e.parent_id.as_deref() == Some(last_id));
                if has_children {
                    return Action::None;
                }

                // Get runs for this experiment
                let runs = state.db.list_runs(last_id).unwrap_or_default();
                if runs.is_empty() {
                    return Action::None;
                }

                let exp_name = state
                    .experiments
                    .iter()
                    .find(|e| e.id == *last_id)
                    .map(|e| e.name.clone())
                    .unwrap_or_default();

                if runs.len() == 1 {
                    // Single run: direct toggle
                    let run_id = runs[0].id.clone();
                    if state.selected_runs_for_compare.contains(&run_id) {
                        state.selected_runs_for_compare.retain(|id| id != &run_id);
                    } else {
                        state.selected_runs_for_compare.push(run_id);
                    }
                    state.refresh_marked_experiments();
                } else {
                    // Multiple runs: open picker popup
                    // Pre-select runs already in compare set
                    let already_selected: Vec<String> = runs
                        .iter()
                        .filter(|r| state.selected_runs_for_compare.contains(&r.id))
                        .map(|r| r.id.clone())
                        .collect();
                    // Sort by completion time (newest first)
                    let mut sorted_runs = runs;
                    sorted_runs.sort_by(|a, b| {
                        let a_time = a.ended_at.as_deref().unwrap_or(&a.started_at);
                        let b_time = b.ended_at.as_deref().unwrap_or(&b.started_at);
                        b_time.cmp(a_time)
                    });
                    state.run_picker = Some(crate::app::RunPickerState {
                        experiment_name: exp_name,
                        runs: sorted_runs,
                        selected: already_selected,
                        cursor: 0,
                    });
                }
            }
            return Action::None;
        }
```

- [ ] **Step 4: Wire popup events and rendering in layout.rs**

In `layout.rs`, add the popup import and field:

```rust
use crate::ui::popup::PopupRenderer;
```

Add to AppLayout struct:
```rust
    pub popup: PopupRenderer,
```

Initialize in `new()`:
```rust
            popup: PopupRenderer::new(),
```

In `handle_event`, add popup interception at the very top (before view routing):

```rust
    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        // Popup interception: popups capture all input
        if let AppEvent::Key(key) = event {
            if state.delete_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_delete_confirm_key(key) {
                    if confirmed {
                        // Will be implemented in Task 10 (delete run)
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

        // Route to full-screen views first
        // ... rest unchanged
```

In `render`, add popup overlay at the very end (after statusbar):

```rust
        // Render status bar
        self.statusbar.render(frame, status_area, state);

        // Popup overlays (rendered last, on top of everything)
        if let Some(ref picker) = state.run_picker {
            self.popup.render_run_picker(frame, area, picker);
        }
        if let Some(ref confirm) = state.delete_confirm {
            self.popup.render_delete_confirm(frame, area, confirm);
        }
```

Also add the same popup overlay rendering inside the Compare and Diff render branches (after statusbar but before return):

```rust
            View::Compare => {
                self.compare.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
                if let Some(ref picker) = state.run_picker {
                    self.popup.render_run_picker(frame, area, picker);
                }
                if let Some(ref confirm) = state.delete_confirm {
                    self.popup.render_delete_confirm(frame, area, confirm);
                }
                return;
            }
```

(Same for Diff branch.)

- [ ] **Step 5: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```
git add rust/src/ui/popup.rs rust/src/ui/mod.rs rust/src/ui/tree.rs rust/src/ui/layout.rs
git commit -m "feat: run picker popup for multi-run experiments, tree markers"
```

---

### Task 7: Multi-run diff with baseline

**Files:**
- Modify: `rust/src/ui/diff.rs`
- Modify: `rust/src/ui/tree.rs`
- Modify: `rust/src/ui/detail.rs`

- [ ] **Step 1: Remove == 2 restriction on d key**

In both `tree.rs` and `detail.rs`, change the DIFF key handler from:
```rust
            if state.selected_runs_for_compare.len() == 2 {
```
to:
```rust
            if state.selected_runs_for_compare.len() >= 2 {
```

- [ ] **Step 2: Rewrite diff.rs render for N runs with baseline**

Update the title in `render`:

```rust
        let baseline_idx = state.compare_baseline.min(data.runs.len().saturating_sub(1));

        let title = {
            let baseline_label = data.runs[baseline_idx].label();
            let other_labels: Vec<String> = data
                .runs
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != baseline_idx)
                .map(|(_, rd)| rd.label())
                .collect();
            format!(" Diff: {} vs {} ", baseline_label, other_labels.join(", "))
        };
```

Pass `baseline_idx` and `inner.width` to all build methods:

```rust
            self.build_metric_deltas(&mut lines, data, baseline_idx);
            self.build_config_changes(&mut lines, data, baseline_idx);
            self.build_delta_tables(&mut lines, data, baseline_idx, inner.width);
```

- [ ] **Step 3: Rewrite build_metric_deltas for N runs**

```rust
    fn build_metric_deltas(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
    ) {
        if data.metric_names.is_empty() {
            return;
        }

        let non_baseline: Vec<usize> = (0..data.runs.len())
            .filter(|i| *i != baseline_idx)
            .collect();

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Metric Deltas".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        let label_width = 14;
        let col_width = 28; // "0.8500 Δ +0.0300 ↑"

        // Column headers: baseline value | each run's delta
        let mut header = vec![Span::raw(format!("  {:<label_width$}", ""))];
        header.push(Span::styled(
            format!("{:<14}", data.runs[baseline_idx].label()),
            Style::default().fg(self.theme.accent_dim),
        ));
        for &ri in &non_baseline {
            let color = RUN_COLORS[ri % RUN_COLORS.len()];
            header.push(Span::styled(
                format!("{:<col_width$}", data.runs[ri].label()),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(header));

        for metric_name in &data.metric_names {
            let base_val = data.runs[baseline_idx]
                .latest_metrics
                .iter()
                .find(|m| m.name == *metric_name)
                .map(|m| m.value);

            let mut spans = vec![Span::raw(format!("  {:<label_width$}", metric_name))];

            // Baseline value
            spans.push(Span::raw(format!(
                "{:<14}",
                base_val
                    .map(|v| format!("{:.4}", v))
                    .unwrap_or_else(|| "-".to_string())
            )));

            let lower_better = is_lower_better(metric_name);

            for &ri in &non_baseline {
                let run_val = data.runs[ri]
                    .latest_metrics
                    .iter()
                    .find(|m| m.name == *metric_name)
                    .map(|m| m.value);

                match (base_val, run_val) {
                    (Some(a), Some(b)) => {
                        let delta = b - a;
                        let is_improvement = if lower_better { delta < 0.0 } else { delta > 0.0 };
                        let (color, arrow) = if delta.abs() < f64::EPSILON {
                            (self.theme.accent_dim, " ")
                        } else if is_improvement {
                            (
                                self.theme.success,
                                if delta > 0.0 { "\u{2191}" } else { "\u{2193}" },
                            )
                        } else {
                            (
                                self.theme.error,
                                if delta > 0.0 { "\u{2191}" } else { "\u{2193}" },
                            )
                        };
                        let sign = if delta > 0.0 { "+" } else { "" };
                        spans.push(Span::styled(
                            format!(
                                "{:<col_width$}",
                                format!("{:.4} \u{0394} {sign}{:.4} {arrow}", b, delta)
                            ),
                            Style::default().fg(color),
                        ));
                    }
                    (_, Some(b)) => {
                        spans.push(Span::styled(
                            format!("{:<col_width$}", format!("{:.4} (new)", b)),
                            Style::default().fg(self.theme.success),
                        ));
                    }
                    (Some(_), None) => {
                        spans.push(Span::styled(
                            format!("{:<col_width$}", "- (missing)"),
                            Style::default().fg(self.theme.error),
                        ));
                    }
                    _ => {}
                }
            }
            lines.push(Line::from(spans));
        }
    }
```

- [ ] **Step 4: Rewrite build_config_changes for N runs**

```rust
    fn build_config_changes(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
    ) {
        if data.config_keys.is_empty() {
            return;
        }

        let non_baseline: Vec<usize> = (0..data.runs.len())
            .filter(|i| *i != baseline_idx)
            .collect();

        let mut change_lines: Vec<Line<'static>> = Vec::new();

        for key in &data.config_keys {
            let base_val = data.runs[baseline_idx]
                .config
                .as_ref()
                .and_then(|c| c.get(key))
                .map(format_json_value);

            for &ri in &non_baseline {
                let run_val = data.runs[ri]
                    .config
                    .as_ref()
                    .and_then(|c| c.get(key))
                    .map(format_json_value);

                if base_val == run_val {
                    continue;
                }

                let run_label = data.runs[ri].label();
                match (&base_val, &run_val) {
                    (Some(a), Some(b)) => {
                        change_lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {run_label}: "),
                                Style::default().fg(self.theme.accent_dim),
                            ),
                            Span::styled(
                                format!("{key}: {a}"),
                                Style::default().fg(self.theme.error),
                            ),
                            Span::raw(" \u{2192} "),
                            Span::styled(
                                format!("{b}"),
                                Style::default().fg(self.theme.success),
                            ),
                        ]));
                    }
                    (None, Some(b)) => {
                        change_lines.push(Line::from(Span::styled(
                            format!("  {run_label}: + {key}: {b}"),
                            Style::default().fg(self.theme.success),
                        )));
                    }
                    (Some(a), None) => {
                        change_lines.push(Line::from(Span::styled(
                            format!("  {run_label}: - {key}: {a}"),
                            Style::default().fg(self.theme.error),
                        )));
                    }
                    _ => {}
                }
            }
        }

        if change_lines.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Config Changes".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());
        lines.extend(change_lines);
    }
```

- [ ] **Step 5: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```
git add rust/src/ui/diff.rs rust/src/ui/tree.rs rust/src/ui/detail.rs
git commit -m "feat: multi-run diff with baseline support"
```

---

### Task 8: Selection floating window

**Files:**
- Create: `rust/src/ui/selection.rs`
- Modify: `rust/src/ui/mod.rs`
- Modify: `rust/src/ui/layout.rs`
- Modify: `rust/src/ui/detail.rs`

- [ ] **Step 1: Create selection.rs**

```rust
use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, Focus};
use crate::keys;
use crate::ui::theme::Theme;

pub struct SelectionWindow {
    theme: Theme,
}

impl SelectionWindow {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, key: &KeyEvent, state: &mut AppState) {
        if keys::matches(key, keys::BACK_ESC) || keys::matches(key, keys::TAB) {
            state.focus = Focus::Tree;
            return;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.selected_runs_for_compare.is_empty() {
                state.selection_cursor = (state.selection_cursor + 1)
                    .min(state.selected_runs_for_compare.len().saturating_sub(1));
            }
            return;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.selection_cursor = state.selection_cursor.saturating_sub(1);
            return;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            // Deselect the highlighted run
            if state.selection_cursor < state.selected_runs_for_compare.len() {
                state.selected_runs_for_compare.remove(state.selection_cursor);
                if state.selection_cursor >= state.selected_runs_for_compare.len()
                    && state.selection_cursor > 0
                {
                    state.selection_cursor -= 1;
                }
                // Adjust baseline if needed
                if state.compare_baseline >= state.selected_runs_for_compare.len()
                    && !state.selected_runs_for_compare.is_empty()
                {
                    state.compare_baseline = 0;
                }
                state.refresh_marked_experiments();
            }
            return;
        }

        if keys::matches(key, keys::BASELINE) {
            if state.selection_cursor < state.selected_runs_for_compare.len() {
                state.compare_baseline = state.selection_cursor;
            }
            return;
        }

        if keys::matches(key, keys::DELETE) {
            // Trigger delete confirmation for highlighted run
            if state.selection_cursor < state.selected_runs_for_compare.len() {
                let run_id = state.selected_runs_for_compare[state.selection_cursor].clone();
                let label = state
                    .db
                    .get_run(&run_id)
                    .ok()
                    .flatten()
                    .and_then(|r| r.name.clone())
                    .unwrap_or_else(|| {
                        if run_id.len() > 8 {
                            run_id[run_id.len() - 8..].to_string()
                        } else {
                            run_id.clone()
                        }
                    });
                state.delete_confirm = Some(crate::app::DeleteConfirmState {
                    run_id,
                    label,
                });
            }
            return;
        }

        if keys::matches(key, keys::QUIT) {
            // Let quit propagate — don't handle here
            return;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        if state.selected_runs_for_compare.is_empty() {
            return;
        }

        let focused = state.focus == Focus::Selection;
        let n = state.selected_runs_for_compare.len();
        let height = (n as u16 + 2).min(area.height / 2);
        let width = 35.min(area.width.saturating_sub(2));

        let x = area.x + area.width.saturating_sub(width + 1);
        let y = area.y + area.height.saturating_sub(height + 2); // above status bar
        let rect = Rect::new(x, y, width, height);

        frame.render_widget(Clear, rect);

        let border_color = if focused {
            self.theme.border_focused
        } else {
            self.theme.border
        };
        let block = Block::bordered()
            .title(" Selected ")
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let mut lines: Vec<Line<'static>> = Vec::new();
        for (i, run_id) in state.selected_runs_for_compare.iter().enumerate() {
            let is_baseline = i == state.compare_baseline;
            let is_cursor = focused && i == state.selection_cursor;

            let marker = if is_baseline { "\u{2605} " } else { "\u{00b7} " };
            let label = state
                .db
                .get_run(run_id)
                .ok()
                .flatten()
                .and_then(|r| {
                    if let Some(name) = &r.name {
                        Some(name.clone())
                    } else {
                        state
                            .db
                            .get_experiment(&r.experiment_id)
                            .ok()
                            .flatten()
                            .map(|e| e.name.clone())
                    }
                })
                .unwrap_or_else(|| {
                    if run_id.len() > 8 {
                        run_id[run_id.len() - 8..].to_string()
                    } else {
                        run_id.clone()
                    }
                });

            let style = if is_cursor {
                self.theme.selected
            } else {
                Style::default()
            };

            lines.push(
                Line::from(vec![
                    Span::styled(
                        marker.to_string(),
                        if is_baseline {
                            Style::default().fg(self.theme.warning)
                        } else {
                            Style::default().fg(self.theme.accent_dim)
                        },
                    ),
                    Span::raw(label),
                ])
                .style(style),
            );
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
```

- [ ] **Step 2: Declare selection module in mod.rs**

Add `pub mod selection;` to `rust/src/ui/mod.rs`.

- [ ] **Step 3: Wire selection into layout.rs**

Add import:
```rust
use crate::ui::selection::SelectionWindow;
```

Add field to AppLayout:
```rust
    pub selection: SelectionWindow,
```

Initialize in `new()`:
```rust
            selection: SelectionWindow::new(),
```

Update `handle_event` — add Selection focus routing after popup interception but before view routing:

```rust
        // Selection window focus
        if state.focus == Focus::Selection {
            if let AppEvent::Key(key) = event {
                if keys::matches(key, keys::QUIT) {
                    return Action::Quit;
                }
                self.selection.handle_event(key, state);
                return Action::None;
            }
        }
```

Update `render` — render selection window as overlay (before popup overlays but after all other rendering). Add at the end of the method, before the popup overlays:

```rust
        // Selection floating window (rendered on top of main content)
        self.selection.render(frame, inner, state);
```

Also add it inside the Compare and Diff render branches.

- [ ] **Step 4: Update Tab handling for Focus cycling**

In `tree.rs`, update the TAB handler to cycle to Selection when runs are marked:

```rust
        if keys::matches(key, keys::TAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            } else {
                state.focus = Focus::Detail;
            }
            return Action::None;
        }
```

Wait — the user said Tab from Tree → Detail → Selection → Tree. Let me keep it:
- Tab from Tree → Detail (existing)
- Tab from Detail → Selection (if runs marked) or Tree
- Tab from Selection → Tree

In `detail.rs`, update the TAB handler at the top of `handle_key`:

```rust
        if keys::matches(key, keys::TAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            } else {
                state.focus = Focus::Tree;
            }
            return Action::None;
        }
```

Remove the Shift+Tab handler in detail.rs (it was doing the same as Tab — toggling between Summary/Info tabs — which is now handled differently).

Actually, Tab in Detail currently switches tabs (Summary/Info). Let me reconsider. The user probably wants Tab to cycle focus between panels, not switch tabs. The current behavior is that Tab in the tree focuses Detail, and Tab in Detail switches tabs. Let me change it so Tab always cycles focus panels: Tree → Detail → Selection → Tree. Tab no longer switches detail tabs — that was already confusing UX. Use `[` and `]` for tab switching? No, those are now run cycling.

Let me keep it simple: Tab cycles focus. Remove the tab-switching behavior in detail panel (users can still see both tabs, they just navigate with j/k which shows different content).

In `detail.rs`, replace both TAB handlers (regular and shift) with:

```rust
        if keys::matches(key, keys::TAB) || keys::matches_shift(key, keys::TAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            } else {
                state.focus = Focus::Tree;
            }
            return Action::None;
        }
```

- [ ] **Step 5: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```
git add rust/src/ui/selection.rs rust/src/ui/mod.rs rust/src/ui/layout.rs rust/src/ui/detail.rs rust/src/ui/tree.rs
git commit -m "feat: floating selection window with baseline and focus cycling"
```

---

### Task 9: Tree markers

**Files:**
- Modify: `rust/src/ui/tree.rs`

- [ ] **Step 1: Update build_tree_items to accept and show markers**

Change `build_tree_items` signature and add marker logic:

```rust
fn build_tree_items<'a>(
    experiments: &[Experiment],
    marked_experiment_ids: &std::collections::HashSet<String>,
) -> Vec<TreeItem<'a, String>> {
```

Update the inner `build_children` to also accept the set:

```rust
    fn build_children<'a>(
        parent_id: Option<&str>,
        children_map: &HashMap<Option<String>, Vec<&Experiment>>,
        marked: &std::collections::HashSet<String>,
    ) -> Vec<TreeItem<'a, String>> {
```

In the label construction:

```rust
                let marker = if marked.contains(&exp.id) { "\u{25cf} " } else { "" };
                let label = if sub_children.is_empty() {
                    format!("{marker}{}", exp.name)
                } else {
                    format!("{marker}{} [{}]", exp.name, sub_children.len())
                };
```

Update recursive calls to pass `marked`.

Update the call in `render`:

```rust
        let tree_items = build_tree_items(&state.experiments, &state.marked_experiment_ids);
```

- [ ] **Step 2: Call refresh_marked_experiments in detail.rs after Space toggle**

In `detail.rs`, after the Space toggle block that modifies `selected_runs_for_compare`, add:

```rust
            state.refresh_marked_experiments();
```

- [ ] **Step 3: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 4: Commit**

```
git add rust/src/ui/tree.rs rust/src/ui/detail.rs
git commit -m "feat: tree markers for experiments with selected runs"
```

---

### Task 10: Delete run

**Files:**
- Modify: `rust/src/db.rs`
- Modify: `rust/src/app.rs`
- Modify: `rust/src/ui/detail.rs`
- Modify: `rust/src/ui/layout.rs`

- [ ] **Step 1: Add delete_run to db.rs**

Add a method that opens a separate writable connection:

```rust
    /// Delete a run and all its associated data.
    /// Opens a separate writable connection since the main one is read-only.
    pub fn delete_run(db_path: &Path, run_id: &str) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        conn.execute("DELETE FROM scalar_metrics WHERE run_id = ?", params![run_id])?;
        conn.execute("DELETE FROM run_params WHERE run_id = ?", params![run_id])?;
        conn.execute("DELETE FROM artifacts WHERE run_id = ?", params![run_id])?;
        conn.execute("DELETE FROM lineage WHERE (parent_type = 'run' AND parent_id = ?) OR (child_type = 'run' AND child_id = ?)", params![run_id, run_id])?;
        conn.execute("DELETE FROM runs WHERE id = ?", params![run_id])?;

        Ok(())
    }
```

- [ ] **Step 2: Add delete helper to AppState**

```rust
    pub fn delete_run(&mut self, run_id: &str) -> Result<()> {
        let db_path = self.store_root.join("extract.db");
        crate::db::Db::delete_run(&db_path, run_id)?;

        // Remove artifact files
        let artifacts_dir = self.store_root.join("artifacts").join(run_id);
        if artifacts_dir.exists() {
            let _ = std::fs::remove_dir_all(&artifacts_dir);
        }

        // Remove from compare selection
        self.selected_runs_for_compare.retain(|id| id != run_id);
        if self.compare_baseline >= self.selected_runs_for_compare.len()
            && !self.selected_runs_for_compare.is_empty()
        {
            self.compare_baseline = 0;
        }
        self.refresh_marked_experiments();

        // Refresh runs list
        let _ = self.refresh_runs();
        if self.runs.is_empty() {
            self.selected_run = None;
        } else if let Some(idx) = self.selected_run {
            if idx >= self.runs.len() {
                self.selected_run = Some(self.runs.len() - 1);
            }
        }

        Ok(())
    }
```

- [ ] **Step 3: Add x key handler in detail.rs**

In `detail.rs` `handle_key`, add before the QUIT handler:

```rust
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
                state.delete_confirm = Some(crate::app::DeleteConfirmState { run_id, label });
            }
            return Action::None;
        }
```

- [ ] **Step 4: Wire delete confirmation in layout.rs handle_event**

Replace the delete_confirm handler placeholder:

```rust
            if state.delete_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_delete_confirm_key(key) {
                    if confirmed {
                        let run_id = state.delete_confirm.as_ref().unwrap().run_id.clone();
                        let _ = state.delete_run(&run_id);
                    }
                    state.delete_confirm = None;
                }
                return Action::None;
            }
```

- [ ] **Step 5: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```
git add rust/src/db.rs rust/src/app.rs rust/src/ui/detail.rs rust/src/ui/layout.rs
git commit -m "feat: delete run with confirmation popup"
```

---

### Task 11: Status bar updates

**Files:**
- Modify: `rust/src/ui/statusbar.rs`

- [ ] **Step 1: Update all status bar bindings**

Replace the entire bindings match in statusbar.rs:

```rust
        let n_marked = state.selected_runs_for_compare.len();
        let bindings: Vec<(&str, &str)> = match (state.current_view, state.focus) {
            (View::Explorer, Focus::Tree) => {
                let mut b = vec![
                    ("j/k", "navigate"),
                    ("Enter", "select"),
                    ("Space", "mark"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                    b.push(("d", "diff"));
                }
                if n_marked > 0 {
                    b.push(("Tab", "selection"));
                } else {
                    b.push(("Tab", "detail"));
                }
                b.push(("q", "quit"));
                b
            }
            (View::Explorer, Focus::Detail) | (View::Detail, _) => {
                let mut b = vec![
                    ("Esc", "back"),
                    ("j/k", "scroll"),
                    ("Space", "mark"),
                    ("[/]", "cycle run"),
                    ("x", "delete"),
                ];
                if n_marked >= 2 {
                    b.push(("c", "compare"));
                    b.push(("d", "diff"));
                }
                b.push(("Tab", "next"));
                b.push(("q", "quit"));
                b
            }
            (View::Explorer, Focus::Selection) => vec![
                ("j/k", "navigate"),
                ("Space", "deselect"),
                ("b", "baseline"),
                ("x", "delete"),
                ("Tab/Esc", "back"),
                ("q", "quit"),
            ],
            (View::Compare, _) | (View::Diff, _) => vec![
                ("Esc", "back"),
                ("j/k", "scroll"),
                ("q", "quit"),
            ],
            _ => vec![("q", "quit"), ("Esc", "back")],
        };
```

Also add run position indicator when in Detail focus:

After the marked count display, add:

```rust
        // Show run position in detail view
        if matches!(state.focus, Focus::Detail) || matches!(state.current_view, View::Detail) {
            if let Some(idx) = state.selected_run {
                if state.runs.len() > 1 {
                    spans.push(Span::styled(
                        format!("  run {}/{}", idx + 1, state.runs.len()),
                        Style::default().fg(self.theme.accent_dim),
                    ));
                }
            }
        }
```

- [ ] **Step 2: Build and verify**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 3: Commit**

```
git add rust/src/ui/statusbar.rs
git commit -m "feat: updated status bar with all new key hints"
```

---

### Task 12: Final build, test, and integration verify

**Files:** All modified files

- [ ] **Step 1: Full build with no errors**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no output (clean build)

- [ ] **Step 2: All tests pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all 19+ tests pass

- [ ] **Step 3: Regenerate test data**

Run: `cd python && uv run python ../scripts/generate_test_data.py`
Expected: 7 runs, 13 experiments

- [ ] **Step 4: Manual verification checklist**

Launch TUI: `cargo run -- --store ../.extract`

Test each feature:
- [ ] Navigate to cifar100/ewc/lambda_1.0 (leaf with 2 runs) → press Space → run picker popup appears
- [ ] In popup: j/k navigates, Space toggles, Enter closes → runs added to compare set
- [ ] Tree shows ● marker next to lambda_1.0
- [ ] Floating selection window appears in bottom-right
- [ ] Tab from tree → detail → selection window → tree
- [ ] In selection window: j/k, Space deselects, b changes baseline (★ moves)
- [ ] Select 2+ runs → press c → compare view shows experiment names as labels
- [ ] Compare view curves respect 80% width
- [ ] Compare view tables show side-by-side with highlight rules
- [ ] Press d → diff view shows baseline vs others with delta columns
- [ ] In detail panel: [/] cycles between runs, title shows "run N/M"
- [ ] Detail Summary tab updates curves/table for cycled run
- [ ] Press x in detail → delete confirm popup → y deletes, n cancels
- [ ] After delete: run removed from list and compare set
