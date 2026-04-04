# Compare/Diff v2 — Design Spec

## Overview

Nine improvements to the Phase 3 compare/diff feature: fix run selection model, improve labeling, add table layout intelligence, support multi-run diff with baselines, add a selection management UI, add run cycling in detail view, and add run deletion.

---

## 1. Tree Space → Run Picker Popup

**Current behavior:** Space on any tree node adds the experiment ID to `selected_runs_for_compare`, which breaks compare/diff (shows nothing for branch nodes).

**New behavior:** Space on a tree node checks the node type:

- **Branch node** (has children): Does nothing.
- **Leaf experiment with 1 run**: Directly toggles that run's ID in/out of the compare set.
- **Leaf experiment with 2+ runs**: Opens a centered popup listing all runs, sorted by completion time (newest first). Each row shows: status dot, date, key config diff. j/k to navigate, Space to toggle individual runs, Enter/Esc to close popup.

The popup is a new `RunPickerPopup` component rendered as an overlay in `layout.rs`. It sets a flag `state.run_picker: Option<RunPickerState>` containing the experiment's runs and selection state. While the picker is active, all key events route to it.

---

## 2. Run Labels

Add `experiment_name: String` to `CompareRunData`, populated during `load_compare_data` by looking up the run's experiment via `db.get_experiment(run.experiment_id)`.

Label logic in `CompareRunData::label()`:
1. If `run.name` is set, use it.
2. Otherwise, use the experiment leaf name (e.g., `lambda_1.0`).
3. If multiple runs in the compare set share the same experiment, append ` #N` (by insertion order within that experiment).

---

## 3. Curve Width

Currently `build_overlay_charts` receives `inner.width.saturating_sub(4)` — ignoring the configured width percentage.

Fix: compute `chart_width = (inner.width as f32 * (state.config.summary.curve_width.min(100) as f32 / 100.0)) as u16` and pass that instead. Same formula already used in `summary.rs`.

---

## 4. Table Highlight Rules

**Compare tables:** Pass `&state.config.tables` to `build_tables_section`. Apply `match_highlight_rule` per cell to determine color, exactly like `summary.rs` does. "transparent" cells render as blank spaces.

**Diff delta tables:** Keep the existing delta-specific coloring (green for positive, red for negative, dim for zero). The delta colormap overrides highlight rules because the delta value is the primary signal — applying accuracy-range highlight rules to delta values (which are small +/- numbers) would be meaningless.

---

## 5. Side-by-Side Table Layout

Algorithm for placing N tables (one per run) for the same artifact name:

```
row_label_w = 6       // "  R1  "
cell_w = 6            // per column
gap = 3               // between tables
table_w = row_label_w + cols * cell_w
tables_per_row = clamp(floor((available_width + gap) / (table_w + gap)), 1, N)
```

Render in chunks of `tables_per_row`:
- For each chunk, first render run label headers side by side.
- Then for each table row index, concatenate that row from each table in the chunk onto the same Line, separated by a gap.

Tables for the same artifact are assumed to have identical dimensions (same matrix shape across runs).

Applies to both compare view tables and diff delta tables.

---

## 6. Multi-Run Diff with Baseline

Remove the `== 2` restriction on the `d` key. Allow 2+ selected runs.

Add `compare_baseline: usize` to `AppState` — index into `selected_runs_for_compare`, default 0 (first selected run).

**Diff view changes:**
- Title: `"Diff: {baseline} vs {run2}, {run3}, ..."`
- Metric deltas: table with one column per non-baseline run, each showing `value`, `Δ = run_N - baseline`, direction arrow, color.
- Config changes: show baseline values as reference line, then +/- lines for each run that differs.
- Delta tables: one delta table per non-baseline run (`run_N - baseline`), laid out with the side-by-side algorithm.

---

## 7. Selection Floating Window + Tree Markers

### Floating Window

- Rendered in the bottom-right corner of the screen whenever `selected_runs_for_compare` is non-empty.
- Bordered panel titled `" Selected "`, width ~35 chars, height = number of selected runs + 2 (border).
- Each line: `★ label` for baseline run, `· label` for others.
- Border: dim when unfocused, cyan when focused.

**Focus model:** Add `Focus::Selection` variant. Tab from Tree/Detail focuses the Selection window. Tab from Selection returns to Tree. Esc from Selection also returns to Tree.

**Keys when focused:** j/k navigate cursor, Space deselects the highlighted run, `b` sets highlighted run as baseline, Esc/Tab returns focus to Tree.

### Tree Markers

Add `marked_experiment_ids: HashSet<String>` to `AppState`, recomputed whenever `selected_runs_for_compare` changes (lookup each run's experiment_id).

In `build_tree_items`, leaf experiments whose ID is in `marked_experiment_ids` get a colored `●` prefix (using `theme.success` color).

---

## 8. Run Cycling in Detail Panel

Currently the detail panel defaults to `selected_run = Some(0)` (oldest) and loads preview data from the latest completed run — a mismatch.

**Fix:**
- Default `selected_run` to the last index (newest run).
- Add `[` / `]` keys in the detail panel to cycle `selected_run` backward/forward.
- On cycle: reload metric histories, run params, artifacts, and cached table for the newly selected run (reuse the loading logic from `refresh_leaf_preview` but targeting the specific run by index).
- Both Summary and Info tabs reflect the selected run.
- Status bar shows `[/]` cycle hint and `"run N/M"` indicator.

---

## 9. Delete a Run

**Key:** `x` in the detail panel (when viewing a run) or in the selection floating window.

**Confirmation:** Centered popup: `"Delete run {label}? [y/n]"`. Only `y` confirms; any other key cancels.

**Implementation:**
- The TUI DB opens with `PRAGMA query_only=ON`. For deletion, open a separate writable `Connection` to the same DB file.
- Delete from tables: `scalar_metrics` (by run_id), `run_params` (by run_id), `artifacts` (by run_id), `runs` (by id).
- Remove artifact directory: `{store_root}/artifacts/{run_id}/` if it exists.
- After delete: remove from `selected_runs_for_compare` if present, refresh the experiment's run list, adjust `selected_run` index, recompute `marked_experiment_ids`.

---

## New Key Bindings Summary

| Key | Context | Action |
|-----|---------|--------|
| `Space` | Tree (leaf) | Toggle single run / open run picker popup |
| `Space` | Detail | Toggle current run for compare |
| `Space` | Selection window | Deselect highlighted run |
| `Space` | Run picker popup | Toggle run selection |
| `[` / `]` | Detail | Cycle through runs |
| `b` | Selection window | Set highlighted run as baseline |
| `x` | Detail / Selection | Delete run (with confirmation) |
| `Tab` | Tree/Detail | Cycle focus: Tree → Detail → Selection → Tree |
| `Esc` | Selection / Popups | Close / return to Tree |

---

## New Files

| File | Purpose |
|------|---------|
| `rust/src/ui/selection.rs` | Floating selection window component |
| `rust/src/ui/popup.rs` | Shared popup rendering: run picker, delete confirmation |

## Modified Files

| File | Changes |
|------|---------|
| `app.rs` | Add `compare_baseline`, `marked_experiment_ids`, `run_picker`, `delete_confirm` fields. Add `experiment_name` to `CompareRunData`. Add `delete_run()` method. Update label logic. |
| `config.rs` | No changes needed |
| `keys.rs` | Add `CYCLE_PREV`, `CYCLE_NEXT`, `DELETE`, `BASELINE` constants |
| `db.rs` | Add `delete_run()` with separate writable connection |
| `ui/mod.rs` | Declare `selection` and `popup` modules |
| `ui/tree.rs` | Replace Space handler with run picker logic. Add tree markers to `build_tree_items`. |
| `ui/detail.rs` | Add `[`/`]` cycling, `x` delete, fix default selected_run to newest |
| `ui/compare.rs` | Fix curve width. Add highlight rules to tables. Implement side-by-side layout. Pass panel width to tables. |
| `ui/diff.rs` | Multi-run baseline support. Side-by-side delta tables. Update metric deltas and config changes for N runs. |
| `ui/layout.rs` | Add selection window + popup rendering as overlays. Update focus routing for Selection. |
| `ui/statusbar.rs` | Update hints for new keys, show run N/M indicator |
