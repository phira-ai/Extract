# Live Reload — Design

**Date:** 2026-04-08
**Status:** Shipped (2026-04-09). **Partially superseded by `feature/log-api-cleanup` (2026-04-09):** `Run.log()` no longer takes a `step=` parameter — headline metrics are single-value per run. MCP `compare_runs` uses `include_curves=True` (top-level `curves` response field) instead of `include_history=True`. This doc preserves the original design rationale but is no longer accurate on those two points.
**PLAN.md item:** "Live reload — WAL-aware auto-refresh when training writes new data"

## Goal

While a training run writes to the store, the TUI should update its detail and compare views in place — curves fill in, latest metrics tick over — without the user pressing any key. The chart's x-axis is **fixed** from a declared training-loop length so the curve fills left-to-right rather than rescaling. The y-axis stays auto-fit. Streaming metrics must not pollute the headline-metric surfaces (Summary panel, branch rankings, compare-view headline columns, MCP `compare_runs` / `get_run`).

## Background

The current tick loop in `rust/src/main.rs:53-62` already calls `refresh_experiments`, `refresh_runs`, and `refresh_selection_summary` every 500ms unconditionally. So the tree, run list, and selection-summary panel already update during training. What is **missing**:

- The detail-panel curves (`load_run_preview` / `refresh_leaf_preview`) load once when a run/leaf is selected and never refresh.
- The compare view (`load_compare_data`) is frozen the moment `c` is pressed.
- The unconditional tick is wasteful on large stores when nothing has changed.

The current Python SDK has two writers for time-series data:

- `Run.log(step=N, **kwargs)` — buffered batched insert into the SQLite `scalar_metrics` table. Used for headline values like `Cl`, `Fgt`. Read by `get_latest_metrics`, `aggregate_final_metrics`, `child_best_metrics`, MCP, and the Summary panel.
- `Run.log_timeseries(name, steps, values)` — writes a complete JSON file under `artifacts/{run_id}/timeseries/`. Read by the curve panel via `crate::artifact::load_timeseries`. Cannot be appended; rewritten whole each call.

The current chart panel **only renders timeseries JSON artifacts**, not the `scalar_metrics` table. So `run.log(step=...)` data goes to a table the curves never read. Symmetrically, the test project pattern `_extract_losses` accumulates curve points in a Python dict and dumps them via `log_timeseries` once at end — the worst possible shape for live updates.

## Design

### 1. Schema and Python SDK

**Migration `schema/migrations/002_live_curves.sql`:**

```sql
ALTER TABLE runs ADD COLUMN total_steps INTEGER;

CREATE TABLE IF NOT EXISTS curve_points (
    run_id    TEXT    NOT NULL REFERENCES runs(id),
    name      TEXT    NOT NULL,
    step      INTEGER NOT NULL,
    value     REAL    NOT NULL,
    wall_time REAL,
    UNIQUE(run_id, name, step)
);

CREATE INDEX IF NOT EXISTS idx_curve_points_run_name_step
    ON curve_points(run_id, name, step);
```

`runs.total_steps` is nullable. Runs that don't declare it fall back to the current auto-fit-to-max-step chart behavior. `curve_points` is structurally identical to the streaming half of `scalar_metrics` but lives in a physically separate table so headline aggregation queries cannot see it.

**Python SDK changes:**

- `Experiment.run(config=..., name=..., total_steps=N)` — new optional kwarg, written to `runs.total_steps` at INSERT time. Mental model: "the training loop will call `run.curve()` with `step` from `0..N-1`."
- `Run.curve(step: int, **kwargs: float)` — **new method.** Buffered and batched into `curve_points` via `executemany`. String values are an error here (they don't make sense as curve points). On `finish()`, the curve buffer flushes alongside the scalar buffer. **Flush policy:** the existing `_FLUSH_THRESHOLD = 100` is too coarse for live UX — at one log per second, the chart would freeze for ~1.5 minutes between flushes. Use a smaller threshold for curves (target: ~10 points) **and** a wall-clock fallback (force-flush if more than ~2 seconds have passed since the last flush, regardless of buffer fill). Both knobs live as module constants in `run.py` so they're trivially tunable.
- `Run.log(step, **kwargs)` — **unchanged.** Still writes to `scalar_metrics`. Still the only way to record headline values. This preserves the clean Summary panel.
- `Run.log_timeseries(name, steps, values)` — **removed.** Replacement: loop and call `run.curve(step=s, **{name: v})`. The `kind='timeseries'` artifact write path goes away on the SDK side.
- Old `kind='timeseries'` rows that may exist in legacy DBs are left in place and silently ignored by the TUI. No destructive cleanup in the migration.

**Test project mental model after the change:**

```python
with experiment.run(config=cfg, total_steps=max_steps) as run:
    for step in range(max_steps):
        loss = train_step(...)
        run.curve(step=step, train_loss=loss)        # streams, fills the chart
    run.log(step=0, Cl=cl, Fgt=fgt)                  # headline, lives in Summary
    run.log_table("accuracy_matrix", scores, axes=...)
```

### 2. Rust DB layer and change detection

**New methods on `db::Db`:**

```rust
// Cheap polling primitive — reads PRAGMA data_version, no I/O.
pub fn data_version(&self) -> Result<i64>;

// Streaming curve queries.
pub fn list_curve_names(&self, run_id: &str) -> Result<Vec<String>>;
pub fn list_curve_points(&self, run_id: &str, name: &str)
    -> Result<Vec<(i64, f64, Option<f64>)>>;  // (step, value, wall_time)
```

`data_version()` is `conn.query_row("PRAGMA data_version", [], |r| r.get(0))`. Safe to call from the existing `query_only=ON` connection. Returns a counter that increments whenever any other connection commits to the database file. No I/O cost.

**Changes to existing types:**

- `model::Run` gains `total_steps: Option<i64>`.
- Every `SELECT ... FROM runs` SQL in `db.rs` (and the matching `query_map` row constructors) gains the `total_steps` column.
- **No other db.rs methods change.** `get_latest_metrics`, `aggregate_final_metrics`, `child_best_metrics`, and the per-run latest-metrics SELECTs all remain scoped to `scalar_metrics` and are guaranteed to see only headline data because `curve_points` is a different table.

**Removed:**

- `crate::artifact::load_timeseries` and the per-tick filesystem-walk-and-JSON-parse loop in `app::AppState::load_all_metric_histories`. Curves now come from a single SQL query against `curve_points`.

### 2a. Other consumers that touch metric histories

Mental model clarification: `scalar_metrics` is **not** restricted to single-step headline rows. It still allows multiple `(step, value)` rows per `(run_id, name)` — the user simply *intends* it for "discrete checkpoint values" (rare, headline-worthy) rather than "high-frequency stream points" (the new `curve_points` lane). The Summary panel takes `MAX(step)` per name. Whether to call `run.log()` with N steps or `run.curve()` with N steps is a matter of intent: do you want this metric to appear in the Summary/rankings/MCP-headline columns?

Given that, only one consumer needs structural changes:

- **`extract sync`** in `python/src/extract/sync.py:139` merges a fixed list of tables (`scalar_metrics`, `run_params`). Add `curve_points` to that list so live curves propagate when pushing/pulling stores between machines. The merge loop is generic (it inspects `PRAGMA table_info` and uses `INSERT OR IGNORE`), so adding `curve_points` to the tuple is the only change needed — `curve_points` has no `id` column, and its `UNIQUE(run_id, name, step)` constraint provides the dedup key.

Two consumers stay unchanged:

- **MCP `compare_runs(include_history=True)`** in `python/src/extract/mcp.py:638-642` already reads histories from `scalar_metrics`. It continues to expose the discrete-checkpoint history of headline metrics — which is the correct semantic for an LLM agent comparing runs. Curves are TUI-facing, not LLM-facing in v1. If exposing curves to MCP becomes useful later, it can be a separate `compare_runs(include_curves=True)` flag — non-breaking follow-up.
- **MCP `get_run`** already excludes histories by design (mcp.py:375-376) — no change.

### 3. Live refresh, scroll preservation, fixed x-axis

**Tick path in `main.rs`:**

```rust
AppEvent::Tick => {
    let v = app.db.data_version()?;
    if v != app.last_data_version {
        app.last_data_version = v;
        let _ = app.refresh_live();
    }
    app.clear_expired_notification(app.config.notifications.timeout);
}
```

`AppState` gains `last_data_version: i64` initialized to 0 in `AppState::new`. The first tick after startup unconditionally fires once because `data_version()` returns a non-zero value, priming the cache.

**`AppState::refresh_live()` — single entry point that re-runs every visible query:**

1. `refresh_experiments()` — existing
2. `refresh_runs()` — existing, only if a leaf is selected
3. `refresh_selection_summary()` — existing
4. `refresh_detail_panel_soft()` — **new**, see below
5. `refresh_compare_data_soft()` — **new**, only if `compare_data.is_some()`

The current `main.rs` tick already runs steps 1-3 unconditionally every 500ms. The change is: (a) gate the whole block on `data_version`, and (b) add steps 4 and 5.

**Soft refresh — the scroll-preservation fix.**

The current `load_run_preview` and `refresh_leaf_preview` set `summary_scroll = 0` and `info_scroll = 0` at the top. If we called those every tick, the user's view would jump back to the top every 500ms. Factor out the data-loading from the cursor-resetting:

```rust
// Hard reload — used on user navigation. Resets scroll. (existing semantics)
pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
    self.summary_scroll = 0;
    self.info_scroll = 0;
    self.reload_run_preview_data(run_idx)
}

// Soft reload — used on data_version tick. Preserves scroll.
fn reload_run_preview_data(&mut self, run_idx: usize) -> Result<()> {
    // body of current load_run_preview minus the scroll resets
}
```

Same split for `refresh_leaf_preview`. `refresh_detail_panel_soft()` figures out which preview is currently displayed (specific run vs leaf default) and calls the corresponding data-only reload. The chart cache `cached_table_artifact_id` already short-circuits when the cached artifact ID matches, so soft refreshes are cheap when nothing changed for the currently-displayed run.

**Soft refresh for compare view.** `load_compare_data` currently resets `compare_data.scroll = 0`. Same split: a `reload_compare_data` that preserves scroll, called from `refresh_compare_data_soft`. The set of marked runs (`selected_runs_for_compare`) is the input — it doesn't change between ticks, so the same compare set re-runs against fresh data.

**Fixed x-axis in `summary.rs::build_curves` and the compare-view chart:**

The chart currently auto-fits its X axis to `[0, max_step_seen]`. New behavior:

```rust
let observed_max_step = /* max step across the metrics being plotted */;
let declared_max = run.total_steps
    .filter(|n| *n > 0)
    .map(|n| (n - 1) as f64);
let x_max = match declared_max {
    Some(d) => d.max(observed_max_step),  // honor declared, extend on overflow
    None    => observed_max_step.max(1.0), // legacy fallback
};
```

So: declared `total_steps` pins the axis from the moment the chart appears. If the training loop overshoots its declaration (unusual but possible), the axis silently extends rather than clipping the curve. Y axis stays auto-fit.

For the compare view with N runs that may declare different `total_steps`, the chart uses `max(declared_or_observed across runs)` so all curves share a single axis, and each one terminates wherever its own training ended.

**Status bar live indicator.** When any visible run has `status = 'running'`, the status bar shows a small `● LIVE` badge themed with `theme.success`. It disappears when nothing is actively training. No keybinding to toggle live mode — `data_version` polling is cheap enough that there is no reason to pause it.

**What does NOT auto-refresh:**

- Lineage view (`L`)
- Registry view (`M`)
- TODO view (`T`)
- Search results
- Help overlay

These stay frozen until the user re-enters the view. Adding any of them is a one-line addition to `refresh_live` later if it ever feels missing.

### 4. Out of scope, follow-ups, testing, risks

**Explicitly out of scope:**

- Per-metric or per-axis `total_steps`. Single run-level value only. If eval metrics ever need a different fixed axis, that's a non-breaking extension later.
- Pause/resume keybinding for live mode.
- Live updates for lineage/registry/todos/search.
- Incremental curve loading (only fetching `step > last_seen_step`). The v1 path re-runs the full `SELECT ... FROM curve_points WHERE run_id = ? AND name = ? ORDER BY step` on every change. Cheap enough at typical scales (10 metrics × 10k steps ≈ 100k rows is milliseconds in SQLite). If profiling shows this dominates, add a `last_step_seen` cache later — non-breaking.
- Cleanup of orphan `kind='timeseries'` artifact rows or files in legacy DBs. Migration is additive only.
- Optional `step` on `Run.log()` (the headline-only ergonomic improvement). Stays required for now to keep this PR focused.
- An `extract clean` command to wipe orphan artifacts. Separate task if/when needed.

**Testing strategy:**

- *Migration test* — apply 002 to a fixture DB built from `001_init.sql` plus seed runs/metrics; verify the new column and table exist and existing data is intact.
- *Python SDK test* — `Run.curve()` writes to `curve_points`, batches at threshold, flushes on `finish()`, rejects strings, raises after `finish()`. `Experiment.run(total_steps=N)` persists the value. `Run.log_timeseries` is gone — any existing test that uses it should fail and be rewritten.
- *Python SDK test* — verify `run.curve(...)` data does not appear via `run.log()`'s read paths by reading both tables in the test.
- *Rust DB test* — `data_version` increments when a separate connection writes; `list_curve_points` returns rows in step order; `list_curve_names` returns distinct names; existing aggregation tests in `db.rs` are unaffected.
- *Rust app integration test* — simulate a write from a second connection, call `refresh_live` on the AppState, verify curves appear in detail-panel state. Set scroll to nonzero, write data, refresh, assert scroll unchanged. Verify the no-op path: calling `refresh_live` twice in a row with no DB change returns without re-querying on the second call (`data_version` matches).
- *Chart axis test* — render with `total_steps = 1000` and a single point at step 5, assert the X axis extends to 1000 not 5. Render with `total_steps = None` and the same point, assert auto-fit. Render with `total_steps = 1000` and a point at step 1500, assert the axis extends to 1500.
- *MCP smoke test* — `compare_runs(include_history=False)` and `get_run` still return only headline metrics. `compare_runs(include_history=True)` continues to return `scalar_metrics` history (unchanged behavior) — curve_points data does NOT leak into MCP responses.
- *Sync smoke test* — push a store with `curve_points` rows to a temp destination, pull it back, verify the curves round-trip.

**Risks and mitigations:**

| Risk | Mitigation |
|---|---|
| `data_version` doesn't tick on a write (filesystem oddity, attached db, etc.) | Test against the actual `Db::open` connection in CI; document the limitation |
| `refresh_live` becomes slow at scale because compare view loads full histories for many runs | Monitor; add incremental `last_step_seen` cache as a follow-up. Acceptable for v1 |
| Write contention with WAL while training writes to `curve_points` and TUI reads | None expected — WAL allows concurrent reader; writer is serialized via Python's `_store.lock` |
| User upgrades and existing `log_timeseries` calls in test project break loudly | Intentional. User confirmed breaking changes are fine and they will migrate the test project |
| Curve buffer is not flushed on hard process kill (no `__exit__`) | Same behavior as today's `scalar_metrics` buffer. Document. Recommend `with` blocks |
| Chart x-axis pins at declared `total_steps` but training crashes at step 200/1000 — chart looks "incomplete forever" | Acceptable. The crashed run's `status = 'failed'` makes this visible elsewhere. The empty right-half of the chart is informative — you can see *where* it died |

**Implementation order** (rough plan-doc step order):

1. Schema migration + Rust `model::Run.total_steps` field + DB column reads. Compiles + passes existing tests.
2. New `db::Db` methods (`data_version`, `list_curve_points`, `list_curve_names`) with unit tests.
3. Python SDK: `Experiment.run(total_steps=...)`, `Run.curve()` (with smaller flush threshold + wall-clock fallback), removal of `Run.log_timeseries`. SDK tests.
4. `extract sync` table list updated to include `curve_points`. Sync round-trip smoke test.
5. Rust app: split hard/soft loaders, wire `last_data_version` and `refresh_live`, gate the tick on `data_version`. Existing tests still pass.
6. Chart: fixed x-axis from `total_steps` in summary view + compare view.
7. Status bar `● LIVE` indicator.
8. End-to-end manual smoke test against the test project at `~/Projects/Playground/Test-Extract`: open TUI, start a training run, watch curves fill in, watch headline metrics stay clean.
