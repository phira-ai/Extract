# Phase 3: Comparison — Implementation Plan

**Goal:** Compare runs side-by-side with pivot tables, overlay charts, delta tables, and config diffs. Two modes: **Compare** (side-by-side) and **Diff** (deltas only).

---

## Design

### Entry Points
- In tree view: **Space** to mark runs for comparison (existing toggle)
- **c** to enter Compare mode (requires 2+ marked runs)
- **d** to enter Diff mode (requires exactly 2 marked runs)

### Compare View (`compare.rs`)

Full side-by-side comparison of marked runs. Scrollable, single panel layout:

```
┌─ Compare: run1 vs run2 ──────────────────────────────────────┐
│                                                               │
│  Pivot Table                                                  │
│  ─────────────────────────────────────────────────────        │
│                    run1          run2                          │
│  accuracy          0.850         0.800                        │
│  loss              0.020         0.024                        │
│  arch              resnet18      moe           ← categorical  │
│  fisher_label      empirical     diagonal      ← categorical  │
│                                                               │
│  Config                                                       │
│  ─────────────────────────────────────────────────────        │
│                    run1          run2                          │
│  lr                0.001         0.001                        │
│  lambda            1.0           0.5           ← highlighted  │
│  online            -             true          ← added        │
│                                                               │
│  accuracy_matrix                                              │
│  ─────────────────────────────────────────────────────        │
│  run1:                    run2:                                │
│  [table side-by-side]     [table side-by-side]                │
│                                                               │
│  Curves (overlay)                                             │
│  ─────────────────────────────────────────────────────        │
│  [loss chart with both runs overlaid, different colors]       │
│  [accuracy chart with both runs overlaid]                     │
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

**Sections (configurable order in `[compare]` config):**
1. **Pivot Table** — all final scalar metrics + run_params for each run in columns. Numeric values with their values, categorical/string values shown as-is.
2. **Config** — parsed JSON configs side-by-side. Changed values highlighted. Missing keys shown as `-`.
3. **Tables** — side-by-side rendering of matrix/table artifacts (one per run).
4. **Curves** — overlay charts: both runs' metric histories on the same chart, different colors per run.

### Diff View (`diff.rs`)

Exactly 2 runs. Shows only what differs:

```
┌─ Diff: run1 → run2 ──────────────────────────────────────────┐
│                                                               │
│  Metric Deltas                                                │
│  ─────────────────────────────────────────────────────        │
│  accuracy          0.850 → 0.800    Δ -0.050  ↓              │
│  loss              0.020 → 0.024    Δ +0.004  ↑              │
│                                                               │
│  Config Changes                                               │
│  ─────────────────────────────────────────────────────        │
│  - lambda: 1.0                     (red = removed/changed)   │
│  + lambda: 0.5                     (green = added/changed)   │
│  + online: true                    (green = new key)         │
│                                                               │
│  Delta Table (accuracy_matrix)                                │
│  ─────────────────────────────────────────────────────        │
│  [run2 - run1 values, diverging color: red=worse green=better]│
│                                                               │
└───────────────────────────────────────────────────────────────┘
```

**Sections:**
1. **Metric Deltas** — numeric metrics with old → new, delta value, direction arrow (↑↓), color-coded (green=better, red=worse using `is_lower_better` heuristic).
2. **Config Changes** — JSON diff with red/green highlighting. Only shows changed/added/removed keys.
3. **Delta Table** — element-wise `run2 - run1` for numeric table artifacts. Diverging color: positive=green, negative=red (or configurable).
4. **Categorical params** — omitted from diff (only shown in compare).

### Data Requirements

**Already available:**
- `selected_runs_for_compare: Vec<String>` in AppState (run IDs)
- `list_run_params(run_id)` for categorical values
- `get_scalar_metrics(run_id, name)` for metric histories
- `get_latest_metrics(run_id)` for final metric values
- `list_artifacts(run_id)` for table artifacts
- `load_table(path)` for loading table data

**New queries needed:**
- None — existing queries suffice. Compare/diff logic is computed in the TUI from existing data.

**New config:**
```toml
[compare]
sections = ["pivot", "config", "tables", "curves"]
```

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `rust/src/ui/compare.rs` | Create | Compare view: pivot table, side-by-side tables, overlay charts |
| `rust/src/ui/diff.rs` | Create | Diff view: metric deltas, config diff, delta tables |
| `rust/src/app.rs` | Modify | Add Compare/Diff view state, load data for marked runs |
| `rust/src/ui/layout.rs` | Modify | Route to compare/diff views based on current_view |
| `rust/src/ui/tree.rs` | Modify | Wire `c` and `d` keys to enter Compare/Diff modes |
| `rust/src/keys.rs` | Already exists | `COMPARE` and `DIFF` constants already defined |
| `rust/src/config.rs` | Modify | Add `[compare]` config section |
| `rust/src/ui/mod.rs` | Modify | Declare compare and diff modules |

---

## Tasks

### Task 1: Compare/Diff Data Loading in AppState
- Add `CompareData` struct holding loaded data for marked runs (metrics, params, configs, tables, metric histories)
- Add `load_compare_data()` method that loads everything for all marked runs
- Add `compare_data: Option<CompareData>` to AppState

### Task 2: Config Section for Compare
- Add `CompareConfig { sections: Vec<CompareSection> }` to config.rs
- `CompareSection` enum: `Pivot`, `Config`, `Tables`, `Curves`

### Task 3: Compare View (`compare.rs`)
- Pivot table: final metrics + run_params in columns per run
- Config diff: parse JSON configs, show side-by-side with highlights for differences
- Tables: side-by-side table rendering for each run's artifacts
- Overlay charts: render multiple runs' metrics on the same chart (different `chart_line_N` colors)
- Scrollable with j/k, Esc to return

### Task 4: Diff View (`diff.rs`)
- Metric deltas: old → new, Δ value, direction arrows, color-coded
- Config changes: red/green diff of JSON keys
- Delta tables: element-wise subtraction with diverging colormap
- No categorical params shown
- Scrollable with j/k, Esc to return

### Task 5: Wire Views into Layout and Tree
- `c` key in tree: validate 2+ marked runs, load compare data, navigate to Compare view
- `d` key in tree: validate exactly 2 marked runs, load compare data, navigate to Diff view
- Layout routes `View::Compare` → `compare.rs`, `View::Diff` → `diff.rs`
- Esc in compare/diff returns to Explorer

### Task 6: Test and Verify
- Update test data to have 2+ comparable runs (different configs, similar metrics)
- Run all tests
- Manual verification: mark two runs, press c/d, verify all sections render
