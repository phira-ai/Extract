# Live Reload Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the TUI watch a running training job in place — curves fill in along a fixed x-axis, latest metrics tick over, all without polluting the headline-metric Summary panel.

**Architecture:** Add a separate `curve_points` SQL table for streaming chart data alongside the existing `scalar_metrics` table for headline values. New SDK method `Run.curve()` writes there. Run-level `total_steps` declares the chart's fixed x-axis bound. Rust TUI polls `PRAGMA data_version` on its existing 500ms tick and only re-runs queries when the database has actually changed; the refresh extends to the detail panel and compare view (with scroll position preserved). `Run.log_timeseries` and the `kind='timeseries'` artifact write path are removed in favor of the new streaming lane.

**Tech Stack:** Rust (ratatui, rusqlite, color_eyre, tokio), Python 3.10+ (sqlite3), SQLite WAL.

**Spec:** `docs/superpowers/specs/2026-04-08-live-reload-design.md`

---

## File Structure

**Files modified:**

| File | Responsibility | What changes |
|---|---|---|
| `python/src/extract/store.py` | Embedded `_SCHEMA` runs at every Store init | Add `curve_points` table; add `total_steps` column on `runs`; idempotent ALTER for existing dev DBs |
| `python/src/extract/experiment.py` | `Experiment.run()` constructor | Accept `total_steps` kwarg, persist on INSERT |
| `python/src/extract/run.py` | SDK write paths for metrics | Add `Run.curve()` with smaller flush threshold + wall-clock fallback; remove `Run.log_timeseries()` |
| `python/src/extract/metrics.py` | Artifact serialization helpers | Remove `save_timeseries` / `load_timeseries` |
| `python/src/extract/sync.py` | Cross-machine merge | Add `curve_points` to autoincrement-table tuple |
| `python/tests/test_curve.py` | NEW — covers `Run.curve()` and `total_steps` | Full TDD coverage of new SDK surface |
| `python/tests/test_sync_curve.py` | NEW — round-trip test for curves through sync | Smoke |
| `python/tests/test_mcp.py` | Existing MCP tests | Stays unchanged — MCP path is intentionally unaffected |
| `schema/migrations/001_init.sql` | Reference schema (used by Rust unit tests via `include_str!`) | Add new table + column to keep Rust tests in sync with the canonical schema |
| `rust/src/model.rs` | `Run` struct | Add `total_steps: Option<i64>` |
| `rust/src/db.rs` | DB layer | Add `data_version`, `list_curve_names`, `list_curve_points`; update every `SELECT ... FROM runs` to read `total_steps`; update `Run` row constructors; update test fixture INSERTs |
| `rust/src/artifact.rs` | Artifact helpers | Remove `load_timeseries` |
| `rust/src/app.rs` | `AppState` | Add `last_data_version` field; rewrite `load_all_metric_histories` to query `curve_points`; split hard/soft loaders for run preview, leaf preview, compare data; add `refresh_live` aggregator |
| `rust/src/main.rs` | Tick loop | Gate refresh on `data_version` change; call `refresh_live` |
| `rust/src/ui/summary.rs` | Detail panel chart | Accept and use `total_steps` for fixed x-axis |
| `rust/src/ui/compare.rs` | Compare view chart | Accept and use max `total_steps` across runs for fixed x-axis |
| `rust/src/ui/statusbar.rs` | Status bar | Append `● LIVE` indicator when any visible run is `running` |
| `rust/src/ui/detail.rs` | SummaryData wiring | Pass `total_steps` of preview run into SummaryData |
| `scripts/generate_test_data.py` | Test data generator | Migrate `log_timeseries(...)` calls to `run.curve(step=, ...)` |

**Each task produces a working build with passing tests, and gets its own commit.**

---

## Task 1: Schema additions for `curve_points` and `runs.total_steps`

**Files:**
- Modify: `python/src/extract/store.py:61-71` (the `_SCHEMA` constant; add column to `runs` and add new `curve_points` table)
- Modify: `python/src/extract/store.py:182-184` (after `executescript(_SCHEMA)`, add idempotent column-check for existing dev DBs)
- Modify: `schema/migrations/001_init.sql` (the canonical reference used by Rust unit tests via `include_str!`)
- Modify: `rust/src/db.rs:748-762` (test fixture INSERTs need a NULL for the new `total_steps` column on each `INSERT INTO runs`)

- [ ] **Step 1: Update `python/src/extract/store.py` `_SCHEMA` to add `total_steps` column on `runs` and the new `curve_points` table**

In `python/src/extract/store.py`, find the `runs` table definition and add the `total_steps` column. Then add the new `curve_points` table after `scalar_metrics`.

Replace this block:

```python
CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiments(id),
    name          TEXT,
    config        TEXT,
    started_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    ended_at      TEXT,
    status        TEXT NOT NULL DEFAULT 'running',
    hostname      TEXT,
    git_sha       TEXT,
    tags          TEXT,
    notes         TEXT
);
```

with:

```python
CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiments(id),
    name          TEXT,
    config        TEXT,
    started_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    ended_at      TEXT,
    status        TEXT NOT NULL DEFAULT 'running',
    hostname      TEXT,
    git_sha       TEXT,
    tags          TEXT,
    notes         TEXT,
    total_steps   INTEGER
);
```

Then, after the `idx_scalar_metrics_run_name` index, add:

```python
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

- [ ] **Step 2: Add idempotent ALTER TABLE for legacy DBs**

In `python/src/extract/store.py`, find this block at the end of `Store.__init__`:

```python
        with self.lock:
            self._conn.executescript(_SCHEMA)
            self._conn.commit()
```

Replace it with:

```python
        with self.lock:
            self._conn.executescript(_SCHEMA)
            # Idempotent migration for legacy DBs created before total_steps existed.
            # CREATE TABLE IF NOT EXISTS won't add columns to an existing table, so
            # we check PRAGMA table_info and ALTER if needed.
            cols = [r[1] for r in self._conn.execute("PRAGMA table_info(runs)").fetchall()]
            if "total_steps" not in cols:
                self._conn.execute("ALTER TABLE runs ADD COLUMN total_steps INTEGER")
            self._conn.commit()
```

- [ ] **Step 3: Mirror the schema changes in `schema/migrations/001_init.sql`**

This file is the canonical reference and is loaded by Rust unit tests via `include_str!`. Make the same edits: add `total_steps INTEGER` to the `runs` table definition (after `notes TEXT`), and add the `curve_points` table + index after the `scalar_metrics` index.

- [ ] **Step 4: Update Rust test fixture INSERTs in `rust/src/db.rs`**

Find the `test_db()` function around line 735. Each `INSERT INTO runs VALUES (...)` line uses positional values matching the schema column count. After adding `total_steps`, each runs INSERT needs one more `, NULL` at the end.

Replace these four lines:

```rust
             INSERT INTO runs VALUES ('r1', 'e_b', 'run1', '{\"lr\": 0.01}', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'completed', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r2', 'e_b', 'run2', '{\"lr\": 0.001}', '2026-01-02T00:00:00Z', NULL, 'running', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r3', 'e_c', 'run3', '{\"lr\": 0.01}', '2026-01-03T00:00:00Z', '2026-01-03T01:00:00Z', 'completed', NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r4', 'e_e', 'run4', '{\"lr\": 0.1}', '2026-01-04T00:00:00Z', NULL, 'failed', NULL, NULL, NULL, NULL);
```

with:

```rust
             INSERT INTO runs VALUES ('r1', 'e_b', 'run1', '{\"lr\": 0.01}', '2026-01-01T00:00:00Z', '2026-01-01T01:00:00Z', 'completed', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r2', 'e_b', 'run2', '{\"lr\": 0.001}', '2026-01-02T00:00:00Z', NULL, 'running', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r3', 'e_c', 'run3', '{\"lr\": 0.01}', '2026-01-03T00:00:00Z', '2026-01-03T01:00:00Z', 'completed', NULL, NULL, NULL, NULL, NULL);
             INSERT INTO runs VALUES ('r4', 'e_e', 'run4', '{\"lr\": 0.1}', '2026-01-04T00:00:00Z', NULL, 'failed', NULL, NULL, NULL, NULL, NULL);
```

- [ ] **Step 5: Run Python tests — they should still pass (no Rust model changes yet, schema only)**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests -x -q
```

Expected: all existing tests pass (no `total_steps`-aware code yet, but the schema change is backwards-compatible because the column is nullable).

- [ ] **Step 6: Run Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -30
```

Expected: all existing tests pass. The `Run` row constructors in `db.rs` still use 11 columns and don't read `total_steps` yet — that's Task 2's job.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add python/src/extract/store.py schema/migrations/001_init.sql rust/src/db.rs && git commit -m "$(cat <<'EOF'
schema: add curve_points table and runs.total_steps column

Adds the new streaming-curves table and the run-level total_steps
declaration for fixed chart x-axis bounds. Includes an idempotent
ALTER TABLE for legacy dev DBs created before this column existed.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Plumb `total_steps` into Rust `Run` model

**Files:**
- Modify: `rust/src/model.rs:15-28` (Run struct)
- Modify: `rust/src/db.rs:78-102` (`list_runs`)
- Modify: `rust/src/db.rs:104-127` (`get_run`)
- Modify: `rust/src/db.rs:296-319` (`recent_runs`)

- [ ] **Step 1: Add `total_steps` field to the `Run` struct**

In `rust/src/model.rs`, replace the `Run` struct:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub experiment_id: String,
    pub name: Option<String>,
    pub config: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub hostname: Option<String>,
    pub git_sha: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
}
```

with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub experiment_id: String,
    pub name: Option<String>,
    pub config: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub hostname: Option<String>,
    pub git_sha: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
    pub total_steps: Option<i64>,
}
```

- [ ] **Step 2: Update `Db::list_runs` to read `total_steps`**

In `rust/src/db.rs`, replace `list_runs`:

```rust
    pub fn list_runs(&self, experiment_id: &str) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes FROM runs WHERE experiment_id = ? ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![experiment_id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
            })
        })?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }
```

with:

```rust
    pub fn list_runs(&self, experiment_id: &str) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes, total_steps FROM runs WHERE experiment_id = ? ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![experiment_id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
    }
```

- [ ] **Step 3: Update `Db::get_run` to read `total_steps`**

Replace `get_run`:

```rust
    pub fn get_run(&self, id: &str) -> Result<Option<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes FROM runs WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
```

with:

```rust
    pub fn get_run(&self, id: &str) -> Result<Option<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, hostname, git_sha, tags, notes, total_steps FROM runs WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
```

- [ ] **Step 4: Update `Db::recent_runs` to read `total_steps`**

Replace `recent_runs`:

```rust
    pub fn recent_runs(&self, limit: i64) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, \
                    hostname, git_sha, tags, notes \
             FROM runs ORDER BY started_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
```

with:

```rust
    pub fn recent_runs(&self, limit: i64) -> Result<Vec<Run>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, experiment_id, name, config, started_at, ended_at, status, \
                    hostname, git_sha, tags, notes, total_steps \
             FROM runs ORDER BY started_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(Run {
                id: row.get(0)?,
                experiment_id: row.get(1)?,
                name: row.get(2)?,
                config: row.get(3)?,
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                status: row.get(6)?,
                hostname: row.get(7)?,
                git_sha: row.get(8)?,
                tags: row.get(9)?,
                notes: row.get(10)?,
                total_steps: row.get(11)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
```

- [ ] **Step 5: Run Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -30
```

Expected: all tests pass. Note that test fixture rows insert `NULL` for `total_steps` (Task 1, Step 4).

- [ ] **Step 6: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/model.rs rust/src/db.rs && git commit -m "$(cat <<'EOF'
rust(model): plumb runs.total_steps through Run struct and DB reads

Adds the optional total_steps field to the Run model and updates the
list_runs/get_run/recent_runs SELECTs to populate it.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `Db::data_version` polling primitive (TDD)

**Files:**
- Modify: `rust/src/db.rs` (add method near top of `impl Db`, around line 17 after `open`)
- Modify: `rust/src/db.rs` (add test in the existing `#[cfg(test)] mod tests` block)

- [ ] **Step 1: Write the failing test**

Add this test inside the `mod tests` block at the end of `rust/src/db.rs`, after `test_list_todos`:

```rust
    #[test]
    fn test_data_version_increments_on_external_write() {
        // Use a temp file (not in-memory) so a second connection can see writes.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path();

        // Initialize the schema via a writable connection.
        let writer = Connection::open(path).unwrap();
        writer
            .execute_batch(include_str!("../../schema/migrations/001_init.sql"))
            .unwrap();

        // Open the read-only Db (the same way the TUI does).
        let db = Db::open(path).unwrap();
        let v1 = db.data_version().unwrap();

        // Write something via the other connection.
        writer
            .execute(
                "INSERT INTO experiments (id, path, name, node_type) VALUES ('e1', 'a', 'a', 'benchmark')",
                [],
            )
            .unwrap();

        let v2 = db.data_version().unwrap();
        assert!(v2 > v1, "data_version should increment after external write (v1={v1}, v2={v2})");

        // Same connection, no changes — should NOT tick again.
        let v3 = db.data_version().unwrap();
        assert_eq!(v2, v3);
    }
```

- [ ] **Step 2: Add `tempfile` to `rust/Cargo.toml` dev-dependencies (if not present)**

Check first:

```bash
grep -n tempfile /home/phil_oh/Projects/Creations/Extract/rust/Cargo.toml
```

If absent, add it under `[dev-dependencies]`:

```toml
[dev-dependencies]
tempfile = "3"
```

If `[dev-dependencies]` doesn't exist either, add the section.

- [ ] **Step 3: Run the test — verify it fails (no `data_version` method yet)**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test test_data_version --quiet 2>&1 | tail -20
```

Expected: compile error on `db.data_version()` — "no method named `data_version` found".

- [ ] **Step 4: Add the `data_version` method**

In `rust/src/db.rs`, find the existing `Db::open` method around line 12-17:

```rust
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA journal_mode=WAL;")?;
        Ok(Self { conn })
    }
```

Add this method right after it:

```rust
    /// Returns SQLite's `PRAGMA data_version` counter, which increments whenever
    /// any other connection commits to the database file. Cheap (no I/O) — safe
    /// to call from a tight tick loop. Used by the TUI to skip refresh work
    /// when the store hasn't changed.
    pub fn data_version(&self) -> Result<i64> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA data_version", [], |row| row.get(0))?;
        Ok(v)
    }
```

- [ ] **Step 5: Run the test — verify it passes**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test test_data_version --quiet 2>&1 | tail -20
```

Expected: `test test_data_version_increments_on_external_write ... ok`.

- [ ] **Step 6: Run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/db.rs rust/Cargo.toml rust/Cargo.lock && git commit -m "$(cat <<'EOF'
rust(db): add data_version polling primitive

Cheap PRAGMA-based change-detection that the TUI tick loop will use
to skip refresh work when nothing has changed in the store.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `Db::list_curve_points` and `Db::list_curve_names` (TDD)

**Files:**
- Modify: `rust/src/db.rs` (add methods after `get_latest_metrics`, around line 158)
- Modify: `rust/src/db.rs` (add seed data to `test_db()` and add tests)

- [ ] **Step 1: Add seed `curve_points` rows to the test fixture**

In `rust/src/db.rs`, find the `test_db()` function and add these INSERTs at the end of the `execute_batch` SQL block (just before the closing `,`), right after the `run_params` inserts:

```rust
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 0, 1.0, 0.0);
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 1, 0.8, 0.5);
             INSERT INTO curve_points VALUES ('r1', 'train_loss', 2, 0.6, 1.0);
             INSERT INTO curve_points VALUES ('r1', 'lr_schedule', 0, 0.001, 0.0);
             INSERT INTO curve_points VALUES ('r1', 'lr_schedule', 1, 0.0009, 0.5);
             INSERT INTO curve_points VALUES ('r2', 'train_loss', 0, 1.2, 0.0);
```

- [ ] **Step 2: Write the failing tests**

Add these tests in the `mod tests` block, after `test_data_version_increments_on_external_write`:

```rust
    #[test]
    fn test_list_curve_names_returns_distinct_sorted() {
        let db = test_db();
        let names = db.list_curve_names("r1").unwrap();
        assert_eq!(names, vec!["lr_schedule".to_string(), "train_loss".to_string()]);
    }

    #[test]
    fn test_list_curve_names_empty_for_run_with_no_curves() {
        let db = test_db();
        let names = db.list_curve_names("r3").unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn test_list_curve_points_returns_step_ordered_rows() {
        let db = test_db();
        let points = db.list_curve_points("r1", "train_loss").unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], (0, 1.0, Some(0.0)));
        assert_eq!(points[1], (1, 0.8, Some(0.5)));
        assert_eq!(points[2], (2, 0.6, Some(1.0)));
    }

    #[test]
    fn test_list_curve_points_empty_for_unknown_metric() {
        let db = test_db();
        let points = db.list_curve_points("r1", "nonexistent").unwrap();
        assert!(points.is_empty());
    }
```

- [ ] **Step 3: Run the tests — verify they fail**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test list_curve --quiet 2>&1 | tail -20
```

Expected: compile error on `db.list_curve_names(...)` and `db.list_curve_points(...)` — methods not found.

- [ ] **Step 4: Implement the methods**

In `rust/src/db.rs`, find the closing `}` of `get_latest_metrics` (around line 157). Right after it, add:

```rust
    /// Distinct metric names that have at least one curve point for this run,
    /// in alphabetical order. Used by the TUI to enumerate curves to render.
    pub fn list_curve_names(&self, run_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT name FROM curve_points WHERE run_id = ? ORDER BY name",
        )?;
        let rows = stmt.query_map(params![run_id], |row| row.get::<_, String>(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// All curve points for a (run, metric), ordered by step ascending.
    /// Returns (step, value, wall_time) tuples.
    pub fn list_curve_points(
        &self,
        run_id: &str,
        name: &str,
    ) -> Result<Vec<(i64, f64, Option<f64>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT step, value, wall_time FROM curve_points \
             WHERE run_id = ? AND name = ? ORDER BY step",
        )?;
        let rows = stmt.query_map(params![run_id, name], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?, row.get::<_, Option<f64>>(2)?))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
```

- [ ] **Step 5: Run the tests — verify they pass**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test list_curve --quiet 2>&1 | tail -20
```

Expected: 4 tests pass.

- [ ] **Step 6: Run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/db.rs && git commit -m "$(cat <<'EOF'
rust(db): list_curve_points and list_curve_names

Streaming-curve read API. Replaces the per-tick filesystem walk over
timeseries JSON artifacts that the TUI currently does.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Python SDK — `Experiment.run(total_steps=)` (TDD)

**Files:**
- Modify: `python/src/extract/experiment.py:55-88` (the `run` method)
- Create: `python/tests/test_curve.py` (new test file for the curve/total_steps surface)

- [ ] **Step 1: Create the test file with a failing test**

Create `python/tests/test_curve.py`:

```python
"""Tests for Run.curve() streaming API and Experiment.run(total_steps=)."""

from __future__ import annotations

import time

import pytest

import extract


def _bootstrap(root, hierarchy="benchmark > model > variant"):
    """Helper: create root/config.toml with the given hierarchy line."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(f'[store]\nhierarchy = "{hierarchy}"\n')


@pytest.fixture
def tmp_store(tmp_path):
    root = tmp_path / ".extract"
    _bootstrap(root)
    return extract.Store(root=root)


# ──────────────────────────────────────────────────────────────────────────
# total_steps declaration


class TestTotalStepsDeclaration:
    def test_total_steps_persisted_on_run_open(self, tmp_store):
        exp = tmp_store.experiment(
            {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
        )
        with exp.run(config={"lr": 0.01}, total_steps=1000) as run:
            run_id = run.id

        row = tmp_store._conn.execute(
            "SELECT total_steps FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
        assert row["total_steps"] == 1000

    def test_total_steps_optional_defaults_null(self, tmp_store):
        exp = tmp_store.experiment(
            {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
        )
        with exp.run(config={"lr": 0.01}) as run:
            run_id = run.id

        row = tmp_store._conn.execute(
            "SELECT total_steps FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
        assert row["total_steps"] is None
```

- [ ] **Step 2: Run the test — verify it fails**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_curve.py::TestTotalStepsDeclaration -x -q
```

Expected: `TypeError: run() got an unexpected keyword argument 'total_steps'`.

- [ ] **Step 3: Add the `total_steps` parameter to `Experiment.run`**

In `python/src/extract/experiment.py`, replace the `run` method:

```python
    def run(self, config: dict | None = None, name: str | None = None) -> Run:
        """Create a new run for this experiment and return it as a context manager."""
        run_id = str(ULID())
        hostname = socket.gethostname()
        git_sha = _git_sha()
        config_json = json.dumps(config) if config is not None else None

        with self._store.lock:
            # Auto-suffix duplicate names within this experiment.
            if name is not None:
                row = self._store._conn.execute(
                    "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name = ?",
                    (self._id, name),
                ).fetchone()
                if row[0] > 0:
                    # Find the next available suffix.
                    row2 = self._store._conn.execute(
                        "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name LIKE ?",
                        (self._id, f"{name}_%"),
                    ).fetchone()
                    name = f"{name}_{row[0] + row2[0]}"

            self._store._conn.execute(
                "INSERT INTO runs (id, experiment_id, name, config, status, "
                "hostname, git_sha, tags) VALUES (?, ?, ?, ?, 'running', ?, ?, '[]')",
                (run_id, self._id, name, config_json, hostname, git_sha),
            )
            self._store._conn.commit()

        return Run(
            store=self._store,
            experiment_id=self._id,
            run_id=run_id,
        )
```

with:

```python
    def run(
        self,
        config: dict | None = None,
        name: str | None = None,
        total_steps: int | None = None,
    ) -> Run:
        """Create a new run for this experiment and return it as a context manager.

        Args:
            config: Run config dict (serialized to JSON).
            name: Optional human-readable name (auto-suffixed on duplicates).
            total_steps: If set, declares the run's training-loop length so the
                TUI charts can pin their x-axis to a fixed bound and the curve
                fills left-to-right rather than rescaling.
        """
        run_id = str(ULID())
        hostname = socket.gethostname()
        git_sha = _git_sha()
        config_json = json.dumps(config) if config is not None else None

        with self._store.lock:
            # Auto-suffix duplicate names within this experiment.
            if name is not None:
                row = self._store._conn.execute(
                    "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name = ?",
                    (self._id, name),
                ).fetchone()
                if row[0] > 0:
                    # Find the next available suffix.
                    row2 = self._store._conn.execute(
                        "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name LIKE ?",
                        (self._id, f"{name}_%"),
                    ).fetchone()
                    name = f"{name}_{row[0] + row2[0]}"

            self._store._conn.execute(
                "INSERT INTO runs (id, experiment_id, name, config, status, "
                "hostname, git_sha, tags, total_steps) "
                "VALUES (?, ?, ?, ?, 'running', ?, ?, '[]', ?)",
                (run_id, self._id, name, config_json, hostname, git_sha, total_steps),
            )
            self._store._conn.commit()

        return Run(
            store=self._store,
            experiment_id=self._id,
            run_id=run_id,
        )
```

- [ ] **Step 4: Run the test — verify it passes**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_curve.py::TestTotalStepsDeclaration -x -q
```

Expected: 2 tests pass.

- [ ] **Step 5: Run all Python tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests -x -q
```

Expected: all tests pass (existing tests don't pass `total_steps`, so they continue to work via the `None` default).

- [ ] **Step 6: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add python/src/extract/experiment.py python/tests/test_curve.py && git commit -m "$(cat <<'EOF'
sdk(experiment): accept total_steps kwarg on run()

Persists the declared training-loop length on runs.total_steps so
charts can pin their x-axis bound.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Python SDK — `Run.curve()` streaming method (TDD)

**Files:**
- Modify: `python/src/extract/run.py` (add `curve()` method, curve buffer, update `_flush()` and `finish()`)
- Modify: `python/tests/test_curve.py` (add `Run.curve` tests)

- [ ] **Step 1: Write failing tests for `Run.curve()`**

Append to `python/tests/test_curve.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# Run.curve() streaming API


def _make_run(store, total_steps=None):
    exp = store.experiment(
        {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
    )
    return exp.run(config={"lr": 0.01}, total_steps=total_steps)


class TestRunCurveBasic:
    def test_curve_writes_to_curve_points_table(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0)
            r.curve(step=1, train_loss=0.8)

        rows = tmp_store._conn.execute(
            "SELECT step, name, value FROM curve_points WHERE run_id = ? ORDER BY step",
            (run.id,),
        ).fetchall()
        assert len(rows) == 2
        assert rows[0]["step"] == 0
        assert rows[0]["name"] == "train_loss"
        assert rows[0]["value"] == 1.0
        assert rows[1]["value"] == 0.8

    def test_curve_supports_multiple_metrics_per_step(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0, accuracy=0.5)

        rows = tmp_store._conn.execute(
            "SELECT name, value FROM curve_points WHERE run_id = ? ORDER BY name",
            (run.id,),
        ).fetchall()
        assert len(rows) == 2
        assert rows[0]["name"] == "accuracy"
        assert rows[0]["value"] == 0.5
        assert rows[1]["name"] == "train_loss"
        assert rows[1]["value"] == 1.0

    def test_curve_does_not_pollute_scalar_metrics(self, tmp_store):
        """The whole point of the split: curve() data must NOT appear in scalar_metrics."""
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0)
            r.curve(step=1, train_loss=0.8)

        rows = tmp_store._conn.execute(
            "SELECT * FROM scalar_metrics WHERE run_id = ?", (run.id,)
        ).fetchall()
        assert len(rows) == 0

    def test_log_does_not_pollute_curve_points(self, tmp_store):
        """And vice versa — log() must NOT appear in curve_points."""
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.log(step=0, Cl=0.7, Fgt=0.1)

        rows = tmp_store._conn.execute(
            "SELECT * FROM curve_points WHERE run_id = ?", (run.id,)
        ).fetchall()
        assert len(rows) == 0

    def test_curve_rejects_string_values(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            with pytest.raises((TypeError, ValueError)):
                r.curve(step=0, label="not a number")

    def test_curve_after_finish_raises(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, loss=1.0)
        with pytest.raises(RuntimeError, match="finished"):
            run.curve(step=1, loss=0.9)


class TestRunCurveBuffering:
    def test_curve_flushes_at_threshold(self, tmp_store):
        """Buffer should flush automatically once it hits _CURVE_FLUSH_THRESHOLD."""
        from extract import run as run_mod

        run = _make_run(tmp_store, total_steps=100)
        # Bypass the context manager so we can inspect the buffer state
        # without triggering the on-exit flush.
        try:
            # Write threshold-1 points; nothing should be flushed yet.
            for i in range(run_mod._CURVE_FLUSH_THRESHOLD - 1):
                run.curve(step=i, loss=float(i))
            count_before = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count_before == 0

            # One more point — should trigger a flush.
            run.curve(step=run_mod._CURVE_FLUSH_THRESHOLD - 1, loss=99.0)
            count_after = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count_after == run_mod._CURVE_FLUSH_THRESHOLD
        finally:
            run.finish()

    def test_curve_finish_flushes_remaining(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, loss=1.0)
            r.curve(step=1, loss=0.9)
            # No automatic flush yet (below threshold).
            count_before = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (r.id,)
            ).fetchone()[0]
            assert count_before == 0
        # After finish (context exit), everything should be flushed.
        count_after = tmp_store._conn.execute(
            "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
        ).fetchone()[0]
        assert count_after == 2

    def test_curve_wall_clock_flush(self, tmp_store, monkeypatch):
        """Sparse logging should still flush within the wall-clock window."""
        from extract import run as run_mod

        # Use a fake clock so the test is deterministic and instant.
        fake_now = [1000.0]
        monkeypatch.setattr(run_mod.time, "monotonic", lambda: fake_now[0])

        run = _make_run(tmp_store, total_steps=1000)
        try:
            run.curve(step=0, loss=1.0)
            count = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count == 0  # below threshold, not yet flushed

            # Advance the clock past the wall-clock window.
            fake_now[0] += run_mod._CURVE_FLUSH_INTERVAL_SEC + 0.1
            run.curve(step=1, loss=0.9)

            count = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count == 2  # both points now flushed
        finally:
            run.finish()
```

- [ ] **Step 2: Run the tests — verify they fail**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_curve.py -x -q
```

Expected: failures with "no attribute 'curve'" or similar.

- [ ] **Step 3: Implement `Run.curve()` and the buffer machinery**

In `python/src/extract/run.py`, find the existing `_FLUSH_THRESHOLD = 100` constant near the top and replace this:

```python
_FLUSH_THRESHOLD = 100
```

with:

```python
_FLUSH_THRESHOLD = 100              # scalar_metrics (headline)
_CURVE_FLUSH_THRESHOLD = 10         # curve_points (streaming) — smaller for live UX
_CURVE_FLUSH_INTERVAL_SEC = 2.0     # wall-clock fallback for slow training loops
```

Then find the `Run.__init__` method and replace it:

```python
    def __init__(self, store: Store, experiment_id: str, run_id: str) -> None:
        self._store = store
        self._experiment_id = experiment_id
        self._id = run_id
        self._start_time = time.time()
        self._finished = False
        self._buffer: list[tuple[str, int, str, float, float]] = []  # (run_id, step, name, value, wall_time)
```

with:

```python
    def __init__(self, store: Store, experiment_id: str, run_id: str) -> None:
        self._store = store
        self._experiment_id = experiment_id
        self._id = run_id
        self._start_time = time.time()
        self._finished = False
        # Headline (scalar_metrics) buffer.
        self._buffer: list[tuple[str, int, str, float, float]] = []  # (run_id, step, name, value, wall_time)
        # Streaming-curve (curve_points) buffer + wall-clock flush bookkeeping.
        self._curve_buffer: list[tuple[str, int, str, float, float]] = []
        self._curve_last_flush: float = time.monotonic()
```

Now find the `_flush` method and update it to also handle curves. Replace:

```python
    def _flush(self) -> None:
        """Flush the scalar metrics buffer to the database."""
        if not self._buffer:
            return

        with self._store.lock:
            self._store._conn.executemany(
                "INSERT OR REPLACE INTO scalar_metrics "
                "(run_id, step, name, value, wall_time) VALUES (?, ?, ?, ?, ?)",
                self._buffer,
            )
            self._store._conn.commit()

        self._buffer.clear()
```

with:

```python
    def _flush(self) -> None:
        """Flush the scalar metrics buffer to the database."""
        if not self._buffer:
            return

        with self._store.lock:
            self._store._conn.executemany(
                "INSERT OR REPLACE INTO scalar_metrics "
                "(run_id, step, name, value, wall_time) VALUES (?, ?, ?, ?, ?)",
                self._buffer,
            )
            self._store._conn.commit()

        self._buffer.clear()

    def _flush_curves(self) -> None:
        """Flush the streaming-curve buffer to the database."""
        if not self._curve_buffer:
            return

        with self._store.lock:
            self._store._conn.executemany(
                "INSERT OR REPLACE INTO curve_points "
                "(run_id, name, step, value, wall_time) VALUES (?, ?, ?, ?, ?)",
                # Reorder: buffer is (run_id, step, name, value, wall_time);
                # the table expects (run_id, name, step, value, wall_time).
                [(rid, name, step, val, wt) for (rid, step, name, val, wt) in self._curve_buffer],
            )
            self._store._conn.commit()

        self._curve_buffer.clear()
        self._curve_last_flush = time.monotonic()
```

Now find the `finish` method and update it to flush both buffers. Replace:

```python
    def finish(self, status: str = "completed") -> None:
        """Flush metrics and finalize the run.

        Idempotent — safe to call multiple times.
        """
        if self._finished:
            return
        self._finished = True
        self._flush()
        with self._store.lock:
            self._store._conn.execute(
                "UPDATE runs SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), "
                "status = ? WHERE id = ?",
                (status, self._id),
            )
            self._store._conn.commit()
```

with:

```python
    def finish(self, status: str = "completed") -> None:
        """Flush metrics and finalize the run.

        Idempotent — safe to call multiple times.
        """
        if self._finished:
            return
        self._finished = True
        self._flush()
        self._flush_curves()
        with self._store.lock:
            self._store._conn.execute(
                "UPDATE runs SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), "
                "status = ? WHERE id = ?",
                (status, self._id),
            )
            self._store._conn.commit()
```

Now add the `curve()` method. Find the existing `log()` method, and right after it (before `_log_param`), insert:

```python
    def curve(self, step: int, **kwargs: float | int) -> None:
        """Log streaming-curve points at a given step.

        Unlike `log()`, curve points are stored in a separate table that the
        TUI's chart panel reads but headline-summary queries do not. Use this
        for high-frequency training values (per-step loss, accuracy) that
        should drive a live chart but should NOT clutter the run summary.

        Numeric values only — strings raise TypeError. Buffered and flushed
        in batches of `_CURVE_FLUSH_THRESHOLD` or after `_CURVE_FLUSH_INTERVAL_SEC`
        seconds, whichever comes first.
        """
        self._check_active()
        wall_time = time.time() - self._start_time
        for name, value in kwargs.items():
            if isinstance(value, str) or not isinstance(value, (int, float)):
                raise TypeError(
                    f"curve() values must be numeric, got {type(value).__name__} for {name!r}"
                )
            self._curve_buffer.append((self._id, step, name, float(value), wall_time))

        # Threshold flush.
        if len(self._curve_buffer) >= _CURVE_FLUSH_THRESHOLD:
            self._flush_curves()
            return
        # Wall-clock flush — keeps slow training loops feeling live in the TUI.
        if time.monotonic() - self._curve_last_flush >= _CURVE_FLUSH_INTERVAL_SEC:
            self._flush_curves()
```

- [ ] **Step 4: Run the tests — verify they pass**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_curve.py -x -q
```

Expected: all tests pass.

- [ ] **Step 5: Run all Python tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests -x -q
```

Expected: all tests pass (existing tests don't use `curve`).

- [ ] **Step 6: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add python/src/extract/run.py python/tests/test_curve.py && git commit -m "$(cat <<'EOF'
sdk(run): add Run.curve() streaming method

New method writes to curve_points (separate from scalar_metrics) so
high-frequency training values drive the TUI chart without polluting
headline-metric Summary panels. Smaller batch threshold and wall-clock
fallback keep slow training loops feeling live.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Remove `Run.log_timeseries` and `save_timeseries`

**Files:**
- Modify: `python/src/extract/run.py:172-188` (remove `log_timeseries`)
- Modify: `python/src/extract/run.py:15` (remove `save_timeseries` import)
- Modify: `python/src/extract/metrics.py:22-32` (remove `save_timeseries` and `load_timeseries`)
- Modify: `scripts/generate_test_data.py:65-67` and `scripts/generate_test_data.py:110-112` (replace `log_timeseries` with `curve` loops)

- [ ] **Step 1: Remove `Run.log_timeseries`**

In `python/src/extract/run.py`, delete this entire method:

```python
    def log_timeseries(self, name: str, steps: list, values: list) -> None:
        """Save a timeseries as a JSON artifact."""
        self._check_active()
        rel_dir = Path("artifacts") / self._id / "timeseries"
        rel_path = rel_dir / f"{name}.json"
        abs_path = self._store.root / rel_path

        save_timeseries(steps, values, abs_path)

        artifact_id = str(ULID())
        with self._store.lock:
            self._store._conn.execute(
                "INSERT INTO artifacts "
                "(id, run_id, name, kind, rel_path) VALUES (?, ?, ?, 'timeseries', ?)",
                (artifact_id, self._id, name, str(rel_path)),
            )
            self._store._conn.commit()
```

- [ ] **Step 2: Remove the `save_timeseries` import**

In `python/src/extract/run.py`, replace:

```python
from extract.metrics import save_npy, save_text, save_timeseries
```

with:

```python
from extract.metrics import save_npy, save_text
```

- [ ] **Step 3: Remove `save_timeseries` and `load_timeseries` from `metrics.py`**

In `python/src/extract/metrics.py`, delete these two functions entirely:

```python
def save_timeseries(steps: list, values: list, path: Path) -> None:
    """Save a timeseries as JSON with steps and values arrays."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump({"steps": steps, "values": values}, f)


def load_timeseries(path: Path) -> dict:
    """Load a timeseries from a JSON file."""
    with open(path) as f:
        return json.load(f)
```

If `json` is no longer used after this deletion, also remove `import json` from the top of the file. Verify with `grep`:

```bash
grep -n json /home/phil_oh/Projects/Creations/Extract/python/src/extract/metrics.py
```

If only the `import json` line remains, remove it.

- [ ] **Step 4: Update `scripts/generate_test_data.py` to use `run.curve` instead of `log_timeseries`**

Find this block (around line 64-67):

```python
        # Log loss timeseries artifact
        steps_list = list(range(50))
        loss_values = [1.0 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)
```

Replace with:

```python
        # Stream loss as curve points (TUI live-chart lane).
        for s in range(50):
            run.curve(step=s, loss_curve=1.0 / (s + 1))
```

Find the same block again around line 110-112:

```python
        steps_list = list(range(50))
        loss_values = [1.2 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)
```

Replace with:

```python
        for s in range(50):
            run.curve(step=s, loss_curve=1.2 / (s + 1))
```

Also: any of the loops above that call `run.run(config=...)` should consider declaring `total_steps=50` for the demo charts to look right. Update the `.run(config=...)` calls in the same with-blocks to also pass `total_steps=50`. Search:

```bash
grep -n "for step in range(50)" /home/phil_oh/Projects/Creations/Extract/scripts/generate_test_data.py
```

For each match, find the enclosing `with store.experiment(...).run(config=...)` block and add `total_steps=50` to the kwargs. Same for `range(30)` blocks → `total_steps=30`.

- [ ] **Step 5: Run all Python tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests -x -q
```

Expected: all tests pass.

- [ ] **Step 6: Sanity-check that the test data script still runs (no DB write — just import)**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command python -c "import ast; ast.parse(open('scripts/generate_test_data.py').read()); print('OK')"
```

Expected: `OK`.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add python/src/extract/run.py python/src/extract/metrics.py scripts/generate_test_data.py && git commit -m "$(cat <<'EOF'
sdk(run): remove log_timeseries in favor of streaming curve()

The JSON-blob-on-disk timeseries path can't be appended incrementally
and feeds a chart that won't update live. The new run.curve() method
covers the same use case with proper streaming semantics. Test data
generator migrated.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Add `curve_points` to `extract sync` (TDD)

**Files:**
- Modify: `python/src/extract/sync.py:139` (add `curve_points` to the autoincrement-table tuple)
- Create: `python/tests/test_sync_curve.py` (round-trip smoke test)

- [ ] **Step 1: Write the failing test**

Create `python/tests/test_sync_curve.py`:

```python
"""Round-trip test: curve_points must propagate through extract sync."""

from __future__ import annotations

import pytest

import extract
from extract.sync import _merge_databases  # private helper, but stable API


def _bootstrap(root):
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(
        '[store]\nhierarchy = "benchmark > model > variant"\n'
    )


@pytest.fixture
def two_stores(tmp_path):
    """Create two empty stores. Returns (src_store, dst_store)."""
    src_root = tmp_path / "src" / ".extract"
    dst_root = tmp_path / "dst" / ".extract"
    _bootstrap(src_root)
    _bootstrap(dst_root)
    src = extract.Store(root=src_root)
    dst = extract.Store(root=dst_root)
    yield src, dst
    src.close()
    dst.close()


class TestSyncCurvePoints:
    def test_curve_points_round_trip(self, two_stores):
        src, dst = two_stores

        # Write some curves into the source.
        exp = src.experiment(
            {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
        )
        with exp.run(config={"lr": 0.01}, total_steps=5, name="src-run") as run:
            for s in range(5):
                run.curve(step=s, loss=1.0 - 0.1 * s)
            run.log(step=0, accuracy=0.9)  # also a headline metric

        # Merge src into dst.
        stats = _merge_databases(src._conn, dst._conn)
        dst._conn.commit()

        # The dst should now have the curve_points.
        rows = dst._conn.execute(
            "SELECT name, step, value FROM curve_points ORDER BY step"
        ).fetchall()
        assert len(rows) == 5
        assert rows[0]["name"] == "loss"
        assert rows[0]["step"] == 0
        assert rows[0]["value"] == pytest.approx(1.0)
        assert rows[4]["value"] == pytest.approx(0.6)

        # And the headline scalar_metrics row.
        scalars = dst._conn.execute(
            "SELECT name, value FROM scalar_metrics"
        ).fetchall()
        assert len(scalars) == 1
        assert scalars[0]["name"] == "accuracy"

        # Stats dict should report the new table.
        assert stats.get("curve_points", 0) == 5
```

- [ ] **Step 2: Check what the actual sync helper API is**

The test uses `_merge_databases` — confirm it exists and accepts those args:

```bash
grep -n "_merge_databases\|def merge" /home/phil_oh/Projects/Creations/Extract/python/src/extract/sync.py
```

If the helper has a different name or signature, adjust the test accordingly. The actual function may be called `_merge` or `merge_stores`. Read the file if uncertain:

```bash
sed -n '1,50p' /home/phil_oh/Projects/Creations/Extract/python/src/extract/sync.py
```

Adjust the import and call site in the test to match.

- [ ] **Step 3: Run the test — verify it fails**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_sync_curve.py -x -q
```

Expected: failure — `curve_points` table is empty in dst because sync doesn't know about it.

- [ ] **Step 4: Add `curve_points` to the sync table tuple**

In `python/src/extract/sync.py`, find this line (around line 139):

```python
        for table in ("scalar_metrics", "run_params"):
```

Replace with:

```python
        for table in ("scalar_metrics", "run_params", "curve_points"):
```

- [ ] **Step 5: Run the test — verify it passes**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests/test_sync_curve.py -x -q
```

Expected: passes.

- [ ] **Step 6: Run all Python tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract && nix develop --command pytest python/tests -x -q
```

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add python/src/extract/sync.py python/tests/test_sync_curve.py && git commit -m "$(cat <<'EOF'
sync: include curve_points in cross-machine merge

Curve data now propagates through extract sync alongside scalar_metrics
and run_params. UNIQUE(run_id, name, step) provides the dedup key.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Add `last_data_version` to `AppState` and gate the tick on it

**Files:**
- Modify: `rust/src/app.rs:324-378` (add `last_data_version: i64` field to `AppState` struct)
- Modify: `rust/src/app.rs:386-447` (initialize `last_data_version: 0` in `AppState::new`)
- Modify: `rust/src/main.rs:53-62` (gate tick refresh on `data_version`)

- [ ] **Step 1: Add the field to `AppState`**

In `rust/src/app.rs`, find the `AppState` struct (around line 324). After the `pub g_pending: bool,` line at the end of the struct, add:

```rust
    /// SQLite data_version watermark — used to skip tick refresh work when
    /// the database hasn't changed since the last tick.
    pub last_data_version: i64,
```

- [ ] **Step 2: Initialize the field in `AppState::new`**

In the `AppState::new` constructor, find the `g_pending: false,` line in the struct literal and add right after it (and before the closing `})`):

```rust
            last_data_version: 0,
```

- [ ] **Step 3: Update the tick path in `main.rs`**

Find this block in `rust/src/main.rs`:

```rust
            AppEvent::Tick => {
                // Periodically refresh data from DB
                let _ = app.refresh_experiments();
                if app.selected_experiment.is_some() {
                    let _ = app.refresh_runs();
                }
                let _ = app.refresh_selection_summary();
                // Clear expired notifications
                app.clear_expired_notification(app.config.notifications.timeout);
            }
```

Replace it with:

```rust
            AppEvent::Tick => {
                // Only do refresh work when the DB has actually changed.
                // PRAGMA data_version is a free in-memory counter that ticks
                // whenever any other connection commits to the database file.
                if let Ok(v) = app.db.data_version() {
                    if v != app.last_data_version {
                        app.last_data_version = v;
                        let _ = app.refresh_experiments();
                        if app.selected_experiment.is_some() {
                            let _ = app.refresh_runs();
                        }
                        let _ = app.refresh_selection_summary();
                    }
                }
                // Clear expired notifications (always — independent of DB state).
                app.clear_expired_notification(app.config.notifications.timeout);
            }
```

- [ ] **Step 4: Build and run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -10
```

Expected: all tests pass. The TUI itself is not tested at this layer; the gate is a simple early-return.

- [ ] **Step 5: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/app.rs rust/src/main.rs && git commit -m "$(cat <<'EOF'
rust(app): gate tick refresh on PRAGMA data_version

The TUI's 500ms tick used to re-run list_experiments + list_runs +
refresh_selection_summary unconditionally. Now it skips the work
entirely when the SQLite data_version counter is unchanged, keeping
idle TUIs cheap on large stores.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: Replace timeseries-artifact curve loading with `curve_points` query

**Files:**
- Modify: `rust/src/app.rs:480-508` (rewrite `load_all_metric_histories`)
- Modify: `rust/src/artifact.rs:42-69` (delete `load_timeseries`)
- Modify: `rust/src/app.rs:7` (verify `crate::artifact` import is still needed)

- [ ] **Step 1: Rewrite `load_all_metric_histories` to query `curve_points`**

In `rust/src/app.rs`, find this method (around line 482):

```rust
    /// Load curve data for a given run.
    /// Only timeseries artifacts are loaded — scalar metrics from run.log()
    /// are headline-only and appear in latest_metrics, not here.
    fn load_all_metric_histories(&mut self, run_id: &str) -> Result<()> {
        self.metric_histories.clear();

        let artifacts = self.db.list_artifacts(run_id)?;
        for artifact in artifacts.iter().filter(|a| a.kind == "timeseries") {
            let path = self.store_root.join(&artifact.rel_path);
            if let Ok(points) = crate::artifact::load_timeseries(&path) {
                let history: Vec<ScalarMetric> = points
                    .into_iter()
                    .map(|(step, value)| ScalarMetric {
                        id: 0,
                        run_id: run_id.to_string(),
                        step,
                        name: artifact.name.clone(),
                        value,
                        wall_time: None,
                    })
                    .collect();
                if !history.is_empty() {
                    self.metric_histories
                        .push((artifact.name.clone(), history));
                }
            }
        }

        Ok(())
    }
```

Replace with:

```rust
    /// Load curve data for a given run.
    /// Reads from the curve_points table (populated by run.curve() in the SDK).
    /// Headline metrics from run.log() live in scalar_metrics and are surfaced
    /// elsewhere — they don't appear in metric_histories.
    fn load_all_metric_histories(&mut self, run_id: &str) -> Result<()> {
        self.metric_histories.clear();

        let names = self.db.list_curve_names(run_id)?;
        for name in names {
            let points = self.db.list_curve_points(run_id, &name)?;
            if points.is_empty() {
                continue;
            }
            let history: Vec<ScalarMetric> = points
                .into_iter()
                .map(|(step, value, wall_time)| ScalarMetric {
                    id: 0,
                    run_id: run_id.to_string(),
                    step,
                    name: name.clone(),
                    value,
                    wall_time,
                })
                .collect();
            self.metric_histories.push((name, history));
        }

        Ok(())
    }
```

- [ ] **Step 2: Delete `load_timeseries` from `rust/src/artifact.rs`**

In `rust/src/artifact.rs`, delete this entire function:

```rust
/// Load a timeseries JSON artifact: {"steps": [...], "values": [...]}.
/// Returns (step, value) pairs.
pub fn load_timeseries(path: &Path) -> Result<Vec<(i64, f64)>> {
    let data = fs::read_to_string(path)?;
    let parsed: serde_json::Value = serde_json::from_str(&data)?;

    let steps = parsed["steps"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing 'steps' array"))?;
    let values = parsed["values"]
        .as_array()
        .ok_or_else(|| color_eyre::eyre::eyre!("missing 'values' array"))?;

    steps
        .iter()
        .zip(values.iter())
        .map(|(s, v)| {
            let step = s
                .as_i64()
                .or_else(|| s.as_f64().map(|f| f as i64))
                .ok_or_else(|| color_eyre::eyre::eyre!("invalid step value"))?;
            let value = v
                .as_f64()
                .ok_or_else(|| color_eyre::eyre::eyre!("invalid metric value"))?;
            Ok((step, value))
        })
        .collect()
}
```

If `use std::fs;` and `use serde_json::Value;`-equivalent imports are now unused, the compiler will warn. Remove unused imports:

- The `use std::fs;` line at the top can stay if other functions use it (check for `fs::` references). If `load_timeseries` was the only consumer, remove it.

- [ ] **Step 3: Build**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo build --quiet 2>&1 | tail -20
```

Expected: clean build, possibly with `unused import` warnings for any imports that are now orphaned. Fix warnings by removing unused imports.

- [ ] **Step 4: Run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/app.rs rust/src/artifact.rs && git commit -m "$(cat <<'EOF'
rust(app): read curves from curve_points instead of timeseries artifacts

The detail panel chart now sources data from the curve_points SQL
table via list_curve_points/list_curve_names. The old per-tick
filesystem walk over timeseries JSON artifacts is gone, along with
artifact::load_timeseries.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Split hard/soft loaders for detail panel + compare; add `refresh_live`

**Files:**
- Modify: `rust/src/app.rs:512-547` (`refresh_leaf_preview` — split into hard/soft)
- Modify: `rust/src/app.rs:911-924` (`load_run_preview` — split into hard/soft)
- Modify: `rust/src/app.rs:588-748` (`load_compare_data` — split into hard/soft)
- Modify: `rust/src/app.rs` (add `refresh_live` method)
- Modify: `rust/src/main.rs:53-62` (call `refresh_live` from the gated tick)

- [ ] **Step 1: Split `load_run_preview` into hard + soft halves**

In `rust/src/app.rs`, find:

```rust
    pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;
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

Replace with:

```rust
    /// Hard reload — used on user navigation (cycle to a new run, etc.).
    /// Resets scroll positions to the top.
    pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;
        self.reload_run_preview_data(run_idx)
    }

    /// Soft reload — used on data_version tick. Preserves scroll positions
    /// so the user's view doesn't jump to the top every 500ms while training
    /// writes new curve points.
    pub fn reload_run_preview_data(&mut self, run_idx: usize) -> Result<()> {
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

- [ ] **Step 2: Split `refresh_leaf_preview` into hard + soft halves**

Find:

```rust
    /// Load preview data (metric history + matrix) for a leaf experiment.
    /// Uses the latest completed run, or the first run if none completed.
    pub fn refresh_leaf_preview(&mut self) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;

        if self.runs.is_empty() {
            self.metric_histories.clear();
            self.run_params.clear();
            self.artifacts.clear();
            self.cached_table = None;
            self.cached_table_artifact_id = None;
            return Ok(());
        }

        // Pick a preview run: latest completed, or first
        let preview_run = self
            .runs
            .iter()
            .rev()
            .find(|r| r.status == "completed")
            .or(self.runs.first());

        let Some(run) = preview_run else {
            return Ok(());
        };
        let run_id = run.id.clone();

        // Load all metric histories and params for the preview run
        self.load_all_metric_histories(&run_id)?;
        self.run_params = self.db.list_run_params(&run_id)?;

        // Load artifacts and cache first table (matrix artifact)
        self.artifacts = self.db.list_artifacts(&run_id)?;
        self.load_first_table()?;

        Ok(())
    }
```

Replace with:

```rust
    /// Hard reload — used when the user navigates to a new leaf experiment.
    /// Resets scroll positions to the top.
    pub fn refresh_leaf_preview(&mut self) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;
        self.reload_leaf_preview_data()
    }

    /// Soft reload — used on data_version tick. Preserves scroll positions
    /// so the leaf summary view doesn't jump to the top while training
    /// writes new data.
    pub fn reload_leaf_preview_data(&mut self) -> Result<()> {
        if self.runs.is_empty() {
            self.metric_histories.clear();
            self.run_params.clear();
            self.artifacts.clear();
            self.cached_table = None;
            self.cached_table_artifact_id = None;
            return Ok(());
        }

        // Pick a preview run: latest completed, or first
        let preview_run = self
            .runs
            .iter()
            .rev()
            .find(|r| r.status == "completed")
            .or(self.runs.first());

        let Some(run) = preview_run else {
            return Ok(());
        };
        let run_id = run.id.clone();

        // Load all metric histories and params for the preview run
        self.load_all_metric_histories(&run_id)?;
        self.run_params = self.db.list_run_params(&run_id)?;

        // Load artifacts and cache first table (matrix artifact)
        self.artifacts = self.db.list_artifacts(&run_id)?;
        self.load_first_table()?;

        Ok(())
    }
```

- [ ] **Step 3: Split `load_compare_data` into hard + soft halves**

In `rust/src/app.rs`, find the existing `pub fn load_compare_data(&mut self) -> Result<()>` method (around line 588). It's long. Rename it to `reload_compare_data` (the soft version), and add a thin hard wrapper. Replace the signature line:

```rust
    pub fn load_compare_data(&mut self) -> Result<()> {
```

with:

```rust
    /// Hard reload — used when the user enters compare view via `c`.
    /// Resets scroll position to the top.
    pub fn load_compare_data(&mut self) -> Result<()> {
        self.reload_compare_data()?;
        if let Some(ref mut data) = self.compare_data {
            data.scroll = 0;
        }
        Ok(())
    }

    /// Soft reload — used on data_version tick. Preserves the user's scroll
    /// position in the compare view while curves continue to grow.
    pub fn reload_compare_data(&mut self) -> Result<()> {
```

(The body of the original `load_compare_data` stays exactly the same; we just rename and add a hard wrapper above.)

The original method already constructs a fresh `CompareData` struct with `scroll: 0` near the bottom. That `scroll: 0` is the bug we're avoiding for live refresh — but since the soft path is followed by a copy of the previous scroll value before the data is replaced, we need to preserve it differently. Find the end of the body where it builds the result:

```rust
        self.compare_data = Some(CompareData {
            runs: runs_data,
            metric_names,
            param_names,
            config_keys,
            table_names,
            timeseries_names,
            scroll: 0,
            total_lines: 0,
            visible_height: 0,
        });

        Ok(())
    }
```

Replace with:

```rust
        // Preserve the user's scroll position across soft reloads.
        // (The hard wrapper resets scroll back to 0 explicitly.)
        let preserved_scroll = self.compare_data.as_ref().map(|d| d.scroll).unwrap_or(0);
        let preserved_visible_height = self.compare_data.as_ref().map(|d| d.visible_height).unwrap_or(0);

        self.compare_data = Some(CompareData {
            runs: runs_data,
            metric_names,
            param_names,
            config_keys,
            table_names,
            timeseries_names,
            scroll: preserved_scroll,
            total_lines: 0,
            visible_height: preserved_visible_height,
        });

        Ok(())
    }
```

- [ ] **Step 4: Add `refresh_live` aggregator**

In `rust/src/app.rs`, find a good spot in the `impl AppState` block (e.g., right after `refresh_selection_summary` around line 836). Add:

```rust
    /// Single entry point for live refresh, called by the tick loop when
    /// PRAGMA data_version has incremented. Re-runs every visible query
    /// using the SOFT loaders so user scroll positions are preserved.
    pub fn refresh_live(&mut self) -> Result<()> {
        self.refresh_experiments()?;
        if self.selected_experiment.is_some() {
            self.refresh_runs()?;
        }
        self.refresh_selection_summary()?;
        // Detail panel — soft reload (preserves scroll).
        if let Some(idx) = self.selected_run {
            self.reload_run_preview_data(idx)?;
        } else if self.selected_experiment.is_some() {
            // Leaf preview path: only fires if the selected experiment is a leaf,
            // matching the existing behavior in the tree-panel selection handler.
            let is_leaf = if let Some(exp_idx) = self.selected_experiment {
                if let Some(exp) = self.experiments.get(exp_idx) {
                    !self.experiments.iter().any(|e| e.parent_id.as_deref() == Some(exp.id.as_str()))
                } else {
                    false
                }
            } else {
                false
            };
            if is_leaf {
                self.reload_leaf_preview_data()?;
            }
        }
        // Compare view — soft reload (preserves scroll), only if active.
        if self.compare_data.is_some() {
            self.reload_compare_data()?;
        }
        Ok(())
    }
```

- [ ] **Step 5: Wire `refresh_live` into the tick loop**

In `rust/src/main.rs`, find the tick handler (you updated this in Task 9 to gate on `data_version`):

```rust
            AppEvent::Tick => {
                // Only do refresh work when the DB has actually changed.
                if let Ok(v) = app.db.data_version() {
                    if v != app.last_data_version {
                        app.last_data_version = v;
                        let _ = app.refresh_experiments();
                        if app.selected_experiment.is_some() {
                            let _ = app.refresh_runs();
                        }
                        let _ = app.refresh_selection_summary();
                    }
                }
                // Clear expired notifications (always — independent of DB state).
                app.clear_expired_notification(app.config.notifications.timeout);
            }
```

Replace with:

```rust
            AppEvent::Tick => {
                // Only do refresh work when the DB has actually changed.
                if let Ok(v) = app.db.data_version() {
                    if v != app.last_data_version {
                        app.last_data_version = v;
                        let _ = app.refresh_live();
                    }
                }
                // Clear expired notifications (always — independent of DB state).
                app.clear_expired_notification(app.config.notifications.timeout);
            }
```

- [ ] **Step 6: Build and run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/app.rs rust/src/main.rs && git commit -m "$(cat <<'EOF'
rust(app): refresh_live aggregator with soft loaders

Splits load_run_preview, refresh_leaf_preview, and load_compare_data
into hard (resets scroll) and soft (preserves scroll) variants. The
new refresh_live method calls only the soft variants and is wired into
the data_version-gated tick loop, so the detail panel and compare view
update in place during training without the user's scroll position
jumping back to the top.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Pass `total_steps` to the summary chart for fixed x-axis

**Files:**
- Modify: `rust/src/ui/summary.rs:14-26` (add `total_steps` to `SummaryData`)
- Modify: `rust/src/ui/summary.rs:213-249` (pass through `build_curves`)
- Modify: `rust/src/ui/summary.rs:251-346` (use it in `render_chart_to_lines`)
- Modify: `rust/src/ui/detail.rs:285-299` (populate `total_steps` from the preview run)

- [ ] **Step 1: Add `total_steps` to `SummaryData`**

In `rust/src/ui/summary.rs`, find the `SummaryData` struct:

```rust
pub struct SummaryData<'a> {
    pub name: &'a str,
    pub runs: &'a [Run],
    pub run_metrics: &'a [Vec<ScalarMetric>],
    pub aggregate_metrics: &'a [MetricAggregate],
    pub unique_configs: i64,
    pub run_params: &'a [RunParam],
    pub metric_histories: &'a [(String, Vec<ScalarMetric>)],
    pub table: Option<&'a TableData>,
    pub table_title: Option<&'a str>,
    pub table_axes: Option<(&'a str, &'a str)>,
}
```

Replace with:

```rust
pub struct SummaryData<'a> {
    pub name: &'a str,
    pub runs: &'a [Run],
    pub run_metrics: &'a [Vec<ScalarMetric>],
    pub aggregate_metrics: &'a [MetricAggregate],
    pub unique_configs: i64,
    pub run_params: &'a [RunParam],
    pub metric_histories: &'a [(String, Vec<ScalarMetric>)],
    pub table: Option<&'a TableData>,
    pub table_title: Option<&'a str>,
    pub table_axes: Option<(&'a str, &'a str)>,
    /// If set, the curve chart's X axis is pinned at [0, total_steps - 1]
    /// (extending if observed steps overflow). If None, falls back to the
    /// legacy auto-fit-to-max-step behavior.
    pub preview_total_steps: Option<i64>,
}
```

- [ ] **Step 2: Plumb `preview_total_steps` through `build_curves`**

Find `build_curves`:

```rust
    fn build_curves(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &SummaryData,
        width: u16,
        height_override: Option<u16>,
        smooth: bool,
    ) {
        if data.metric_histories.is_empty() {
            return;
        }

        // Use configured height or auto-scale based on number of metrics
        let chart_height: u16 = height_override.unwrap_or_else(|| match data.metric_histories.len() {
            1 => 12,
            2 => 10,
            3 => 8,
            _ => 6,
        });

        for (name, history) in data.metric_histories {
            if history.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));

            let chart_lines = self.render_chart_to_lines(history, width, chart_height, smooth);
            lines.extend(chart_lines);
        }
    }
```

Replace with:

```rust
    fn build_curves(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &SummaryData,
        width: u16,
        height_override: Option<u16>,
        smooth: bool,
    ) {
        if data.metric_histories.is_empty() {
            return;
        }

        // Use configured height or auto-scale based on number of metrics
        let chart_height: u16 = height_override.unwrap_or_else(|| match data.metric_histories.len() {
            1 => 12,
            2 => 10,
            3 => 8,
            _ => 6,
        });

        for (name, history) in data.metric_histories {
            if history.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));

            let chart_lines = self.render_chart_to_lines(
                history,
                width,
                chart_height,
                smooth,
                data.preview_total_steps,
            );
            lines.extend(chart_lines);
        }
    }
```

- [ ] **Step 3: Use `preview_total_steps` in `render_chart_to_lines`**

Find `render_chart_to_lines`:

```rust
    fn render_chart_to_lines(
        &self,
        history: &[ScalarMetric],
        width: u16,
        height: u16,
        smooth: bool,
    ) -> Vec<Line<'static>> {
        let raw_points: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let points = if smooth && raw_points.len() >= 3 {
            catmull_rom_interpolate(&raw_points, (raw_points.len() * 4).max(100))
        } else {
            raw_points
        };

        let (x_min, x_max) = points
            .iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (x, _)| {
                (min.min(*x), max.max(*x))
            });
```

Replace the signature and the `(x_min, x_max)` block with:

```rust
    fn render_chart_to_lines(
        &self,
        history: &[ScalarMetric],
        width: u16,
        height: u16,
        smooth: bool,
        total_steps: Option<i64>,
    ) -> Vec<Line<'static>> {
        let raw_points: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let points = if smooth && raw_points.len() >= 3 {
            catmull_rom_interpolate(&raw_points, (raw_points.len() * 4).max(100))
        } else {
            raw_points
        };

        let observed_max_x = points
            .iter()
            .map(|(x, _)| *x)
            .fold(f64::MIN, f64::max);

        // X axis: pin to declared total_steps if present, extend on overflow.
        let x_min = 0.0_f64;
        let x_max = match total_steps.filter(|n| *n > 0) {
            Some(n) => ((n - 1) as f64).max(observed_max_x).max(1.0),
            None => observed_max_x.max(1.0),
        };
```

- [ ] **Step 4: Update the caller in `detail.rs` to populate `preview_total_steps`**

In `rust/src/ui/detail.rs`, find the `render_summary` method (around line 265). The current code builds `SummaryData` like this:

```rust
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
        };
```

Replace with:

```rust
        // Resolve the preview run's total_steps for the chart x-axis pin.
        // The leaf preview picks "latest completed or first" run; the per-run
        // detail view uses state.selected_run if set.
        let preview_total_steps = if let Some(idx) = state.selected_run {
            state.runs.get(idx).and_then(|r| r.total_steps)
        } else {
            state
                .runs
                .iter()
                .rev()
                .find(|r| r.status == "completed")
                .or(state.runs.first())
                .and_then(|r| r.total_steps)
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
        };
```

- [ ] **Step 5: Check for other `SummaryData` constructors**

```bash
grep -rn "SummaryData {" /home/phil_oh/Projects/Creations/Extract/rust/src/
```

Any other call site (e.g. in `dashboard.rs` or elsewhere) needs `preview_total_steps` added. For each match, add `preview_total_steps: None,` (the legacy fallback) unless the call site can sensibly determine a value.

- [ ] **Step 6: Build and run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/ui/summary.rs rust/src/ui/detail.rs && git commit -m "$(cat <<'EOF'
rust(ui): pin summary chart x-axis to runs.total_steps

When a run declares total_steps, the curve chart's X axis is pinned at
[0, total_steps - 1] from the moment the chart appears, so streaming
data fills left-to-right rather than rescaling. Falls back to auto-fit
when total_steps is not declared. Y axis remains auto-fit.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Fixed x-axis in compare view chart

**Files:**
- Modify: `rust/src/ui/compare.rs:574-667` (`render_overlay_chart_to_lines` accepts and uses `total_steps`)
- Modify: `rust/src/ui/compare.rs:528-571` (caller passes max `total_steps` across runs)

- [ ] **Step 1: Update `render_overlay_chart_to_lines` to accept a fixed x-axis bound**

In `rust/src/ui/compare.rs`, find `render_overlay_chart_to_lines`:

```rust
    fn render_overlay_chart_to_lines(
        &self,
        runs_data: &[(Vec<(f64, f64)>, Color)],
        width: u16,
        height: u16,
    ) -> Vec<Line<'static>> {
        // Compute global bounds
        let mut x_min = f64::MAX;
        let mut x_max = f64::MIN;
        let mut y_min = f64::MAX;
        let mut y_max = f64::MIN;

        for (data, _) in runs_data {
            for &(x, y) in data {
                x_min = x_min.min(x);
                x_max = x_max.max(x);
                y_min = y_min.min(y);
                y_max = y_max.max(y);
            }
        }

        if x_min >= x_max {
            x_max = x_min + 1.0;
        }
```

Replace the signature and the bounds-computing block with:

```rust
    fn render_overlay_chart_to_lines(
        &self,
        runs_data: &[(Vec<(f64, f64)>, Color)],
        width: u16,
        height: u16,
        total_steps_max: Option<i64>,
    ) -> Vec<Line<'static>> {
        // Compute observed Y bounds; X is pinned to declared total_steps when
        // present (extending on overflow), else falls back to observed max.
        let mut observed_x_max = f64::MIN;
        let mut y_min = f64::MAX;
        let mut y_max = f64::MIN;

        for (data, _) in runs_data {
            for &(x, y) in data {
                observed_x_max = observed_x_max.max(x);
                y_min = y_min.min(y);
                y_max = y_max.max(y);
            }
        }

        let x_min = 0.0_f64;
        let x_max = match total_steps_max.filter(|n| *n > 0) {
            Some(n) => ((n - 1) as f64).max(observed_x_max).max(1.0),
            None => {
                if observed_x_max <= x_min {
                    x_min + 1.0
                } else {
                    observed_x_max
                }
            }
        };
```

- [ ] **Step 2: Update the caller to compute and pass `total_steps_max`**

Find the caller block in `build_compare_curves` (or similar — search for `render_overlay_chart_to_lines(` in `compare.rs`):

```rust
            let chart_lines =
                self.render_overlay_chart_to_lines(&all_points, chart_width.max(20), chart_height);
```

Replace with:

```rust
            // Pin the compare-view x-axis to the largest total_steps across
            // the runs being compared, so all curves share a single axis and
            // each terminates at its own training endpoint.
            let total_steps_max: Option<i64> = data
                .runs
                .iter()
                .filter_map(|rd| rd.run.total_steps)
                .max();

            let chart_lines = self.render_overlay_chart_to_lines(
                &all_points,
                chart_width.max(20),
                chart_height,
                total_steps_max,
            );
```

- [ ] **Step 3: Build and run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/ui/compare.rs && git commit -m "$(cat <<'EOF'
rust(ui): pin compare-view chart x-axis to max(total_steps) across runs

Compare view's overlay charts now share a single fixed x-axis, sourced
from the largest total_steps among the runs being compared. Each curve
terminates at its own training endpoint within that shared bound.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 14: `● LIVE` indicator in the status bar

**Files:**
- Modify: `rust/src/ui/statusbar.rs:99-132` (append the indicator after the keybinding spans)

- [ ] **Step 1: Add the indicator to `StatusBar::render`**

In `rust/src/ui/statusbar.rs`, find this block near the end of the `render` method:

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

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().fg(self.theme.accent_dim));
        frame.render_widget(bar, area);
    }
```

Replace with:

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

        // ● LIVE indicator — visible whenever any visible run is actively running.
        // We check the currently-loaded runs list (the ones the detail panel
        // would see) rather than all runs in the store.
        if state.runs.iter().any(|r| r.status == "running") {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "\u{25cf} LIVE",
                Style::default()
                    .fg(self.theme.success)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let line = Line::from(spans);
        let bar = Paragraph::new(line).style(Style::default().fg(self.theme.accent_dim));
        frame.render_widget(bar, area);
    }
```

- [ ] **Step 2: Build and run all Rust tests**

```bash
cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test --quiet 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add rust/src/ui/statusbar.rs && git commit -m "$(cat <<'EOF'
rust(ui): add ● LIVE indicator to status bar

Shows when any visible run has status='running', so the user has a
clear visual signal that the detail and compare views are actively
auto-refreshing from new data.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 15: Update PLAN.md to mark live reload done

**Files:**
- Modify: `PLAN.md:5` (move "Live reload" out of the future-work list)

- [ ] **Step 1: Remove the live reload bullet from PLAN.md**

In `PLAN.md`, find:

```markdown
- **Live reload** — WAL-aware auto-refresh when training writes new data
- **`extract init`** — CLI to initialize `.extract/` with hierarchy config interactively
```

Replace with:

```markdown
- **`extract init`** — CLI to initialize `.extract/` with hierarchy config interactively
```

(The `extract init` line stays — it's documented as still pending in PLAN.md even though the codebase has init landed; cleaning that up is out of scope for this PR.)

- [ ] **Step 2: Commit**

```bash
cd /home/phil_oh/Projects/Creations/Extract && git add PLAN.md && git commit -m "$(cat <<'EOF'
docs(plan): remove live reload from future work

Implemented in this branch — see docs/superpowers/specs/2026-04-08-live-reload-design.md
and the corresponding implementation plan.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 16: End-to-end manual smoke test (no commit)

**Files:** None — this is a manual verification step against the test project.

- [ ] **Step 1: Reinstall the SDK against the test project's venv**

```bash
cd /home/phil_oh/Projects/Playground/Test-Extract && nix develop --command pip install -e /home/phil_oh/Projects/Creations/Extract
```

- [ ] **Step 2: Migrate the test project's `_log_extract_final_metrics` to streaming `curve()`**

In `/home/phil_oh/Projects/Playground/Test-Extract/src/runners/sequential.py`, the current pattern (around lines 766-771):

```python
        for name, series in self._extract_losses.items():
            try:
                self._extract_run.log_timeseries(
                    name, steps=series["steps"], values=series["values"]
                )
            except Exception:
                ...
```

won't work anymore (`log_timeseries` is gone). Replace with a streaming-curve loop. The actual fix depends on how you want to restructure — either (a) change `RichProgressCallback.on_log` to call `run.curve(step=N, **metrics)` directly during training, or (b) keep the accumulator but flush via `run.curve` at end. Option (a) gives true live updates; (b) just makes the existing flow compile.

For the smoke test, option (b) is sufficient. Replace the block with:

```python
        for name, series in self._extract_losses.items():
            try:
                for step, value in zip(series["steps"], series["values"]):
                    self._extract_run.curve(step=step, **{name: value})
            except Exception:
                logger.warning("Failed to log Extract curve", exc_info=True)
```

For the live experience, also pass `total_steps` to `experiment.run(...)` in `_open_extract_run` (the call around line 728-731). The total comes from the trainer's `max_steps` — the easiest source is wherever `args.max_steps` is set in the training args. If unsure, pick a representative value like the longest task length and pass it as a constant.

- [ ] **Step 3: Run a short training job in one terminal**

```bash
cd /home/phil_oh/Projects/Playground/Test-Extract && nix develop --command python main.py [test-config-with-few-steps]
```

(Adjust the command per the test project's actual entry point.)

- [ ] **Step 4: In a second terminal, open the TUI**

```bash
cd /home/phil_oh/Projects/Playground/Test-Extract && nix develop --command extract tui
```

Navigate to the experiment and select a leaf with a running run. Verify:

- [ ] The `● LIVE` badge appears in the status bar.
- [ ] The detail panel's curve chart fills in left-to-right along a fixed x-axis (no rescaling).
- [ ] The Summary panel still shows only `Cl` and `Fgt` (or whatever headline metrics the test project logs via `run.log()`), NOT the streaming training losses.
- [ ] Scrolling the detail panel with `j`/`k` does not snap back to the top when new data arrives.
- [ ] Mark two runs and press `c` for the compare view; verify both curves grow live and the compare-view scroll position is preserved.
- [ ] When training finishes, `● LIVE` disappears.

- [ ] **Step 5: If everything works, you're done. If not, file follow-up issues for any rough edges and iterate.**

---

## Self-Review Notes

- **Spec coverage check:** All numbered sections of `2026-04-08-live-reload-design.md` map to tasks above. Section 1 (schema + SDK) → Tasks 1, 5, 6, 7. Section 2 (DB layer) → Tasks 2, 3, 4. Section 2a (sync) → Task 8. Section 3 (live refresh, scroll preservation, fixed x-axis) → Tasks 9, 10, 11, 12, 13, 14. Section 4 (testing & smoke) → Task 16 (manual) plus the per-task TDD.
- **No placeholders:** every step contains the exact code or command needed.
- **Type consistency:** `Run.total_steps: Option<i64>` (Rust) and `runs.total_steps INTEGER` (SQL) match. `_CURVE_FLUSH_THRESHOLD` and `_CURVE_FLUSH_INTERVAL_SEC` are referenced consistently between the implementation (Task 6, Step 3) and the tests (Task 6, Step 1). The new SDK method is `Run.curve(step=, **kwargs)` everywhere, never `Run.scalar` or `Run.stream`. The new Rust DB methods are `data_version`, `list_curve_names`, `list_curve_points` everywhere they're referenced.
