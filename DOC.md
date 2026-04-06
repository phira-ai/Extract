# Extract — LLM Agent Documentation

Local-first experiment tracking for deep learning. Two components: a **Python SDK** for logging and a **Rust TUI** for visualization.

Storage: single `.extract/` directory with SQLite (WAL mode) + file artifacts. No server.

---

## Python SDK API

### `Store`

```python
from extract import Store

store = Store()  # reads from .extract/ in current directory
```

**`Store(root=".extract")`**
- `root: str | Path` — path to `.extract/` directory. Created if absent. Default: `".extract"`.
- Hierarchy is read from `config.toml` (`[store] hierarchy = "..."`).

**`store.experiment(spec: dict[str, str] | str) -> Experiment`**
- Dict spec (preferred): keys must be hierarchy levels in order from root. Creates intermediate nodes. Values cannot skip levels.
  ```python
  store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "v1"})
  store.experiment({"benchmark": "cifar100"})  # partial — stops at benchmark level
  ```
- String spec (legacy): slash-delimited path, no `node_type` set.
  ```python
  store.experiment("cifar100/ewc/v1")
  ```

**`store.list_experiments(prefix="") -> list[Experiment]`**
- Returns all experiments, optionally filtered by path prefix.

**`store.todo(content: str, priority: int = 0) -> None`**
- Creates a global-scoped TODO. Priority: 0=low, 1=mid, 2=high.

**`store.list_todos(scope_type="global", scope_id=None) -> list[dict]`**
- Returns TODOs filtered by scope. Ordered by priority DESC, then created_at.

**`store.close() -> None`**
- Closes the database connection.

---

### `Experiment`

Returned by `store.experiment()`. Properties: `id: str`, `path: str`, `name: str`.

**`experiment.run(config=None, name=None) -> Run`**
- `config: dict | None` — JSON-serializable config dict. Stored as JSON string.
- `name: str | None` — human-readable run name.
- Returns a `Run`. Usable as a context manager or directly. Automatically captures `hostname` and `git_sha` (current HEAD).
- Run status: `"running"` on creation, `"completed"` or `"failed"` on finish.

**`experiment.list_runs() -> list[dict]`**
- Returns all runs for this experiment as dicts, ordered by `started_at`.

---

### `Run`

Usable as a context manager or via direct calls. Properties: `id: str`.

```python
# Context manager — auto-finishes on exit
with experiment.run(config={"lr": 0.001}, name="run-001") as run:
    run.log(step=0, loss=1.0)

# Direct call — finish explicitly
run = experiment.run(config={"lr": 0.001}, name="run-002")
run.log(step=0, loss=1.0)
run.finish()
```

#### Lifecycle

**`run.finish(status="completed") -> None`**
- Flushes metric buffer, sets `ended_at` and `status`.
- Idempotent — safe to call multiple times.
- Called automatically by `__exit__` when used as context manager.
- All mutating methods (`log`, `log_table`, `tag`, etc.) raise `RuntimeError` after `finish()`.

#### Scalar Metrics

**`run.log(step: int, **kwargs: float | int | str) -> None`**
- Numeric values (int, float) → `scalar_metrics` table (headline metrics).
- String values → `run_params` table (categorical parameters, deduplicated by name).
- Buffered in memory; flushed every 100 entries or on `finish()`.
- `wall_time` is automatically recorded (seconds since run start).
- Shown in the TUI metrics summary and comparison tables. **Not** rendered as curves — use `log_timeseries()` for curve data.

```python
run.log(step=0, loss=2.3, accuracy=0.1, arch="resnet18")
run.log(step=1, loss=1.8, accuracy=0.3)
```

#### Artifacts

**`run.log_table(name: str, data: np.ndarray, step=None, axes=None) -> None`**
- Saves NumPy array as `.npy` file under `artifacts/{run_id}/matrices/`.
- `axes: dict | None` — metadata like `{"rows": "task", "cols": "step"}`.
- File: `{name}[_step_{step}].npy`

**`run.log_timeseries(name: str, steps: list, values: list) -> None`**
- Saves as JSON under `artifacts/{run_id}/timeseries/{name}.json`.
- Format: `{"steps": [...], "values": [...]}`
- Rendered as curves in the TUI Summary and Compare views. **Not** shown in headline metrics. Use this for curve-only data (e.g. per-task loss curves) that should not appear in the metrics summary.

**`run.log_text(name: str, content: str) -> None`**
- Saves as markdown under `artifacts/{run_id}/text/{name}.md`.

#### Tags & Notes

**`run.tag(*tags: str) -> None`**
- Appends tags. Stored as JSON array in `runs.tags`.

**`run.note(content: str) -> None`**
- Appends to run's notes (newline-separated).

#### TODOs

**`run.todo(content: str, priority: int = 0) -> None`**
- Creates a TODO scoped to this run. Priority: 0=low, 1=mid, 2=high.

#### Model Registry

**`run.register_model(name: str, version: str, path: str, metadata=None, framework="pytorch") -> None`**
- Copies model file/directory to `models/{name}/{version}/`.
- Records in `models` table. `(name, version)` must be unique.

#### Lineage

**`run.derived_from(run=None, model=None, version=None) -> None`**
- Records this run as derived from another run (by run ID) or model (by name+version or model ID).

**`run.branched_from(experiment=None, run=None) -> None`**
- Records this run as branched from an experiment (by ID) or another run (by ID).

---

### Sync Module

```python
from extract import sync
```

**`sync.push(root: Path, remote: str) -> None`**
- Rsync local store to remote. Checkpoints WAL first.

**`sync.pull(root: Path, remote: str) -> dict[str, int]`**
- Rsync remote into temp dir, merge DB and artifacts into local store.
- Returns `{table_name: rows_added}`.

**`sync.export_archive(root: Path, output: Path) -> None`**
- Creates `.tar.gz` of the store.

**`sync.import_archive(archive: Path, root: Path) -> dict[str, int]`**
- Extracts archive, merges into target store.
- Returns `{table_name: rows_added}`.

**`sync.merge_db(src_path: Path, dst_path: Path) -> dict[str, int]`**
- Low-level DB merge. Experiments matched by `path` (not ID). Runs use ULIDs so never collide. Returns per-table row counts.

---

### CLI

```bash
extract tui [--store .extract]
extract sync push <remote> [--root .extract]
extract sync pull <remote> [--root .extract]
extract sync export <output.tar.gz> [--root .extract]
extract sync import <archive.tar.gz> [--root .extract]
```

### Install

```bash
pip install extract-tracker
```

Installs the Python SDK, the `extract` CLI, and the compiled TUI binary.

---

## Database Schema

SQLite with WAL journal mode. 9 tables:

### `experiments`
Hierarchical namespace nodes.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `path` | TEXT NOT NULL | slash-delimited, e.g. `"cifar100/ewc/v1"` |
| `name` | TEXT NOT NULL | leaf name of this node |
| `parent_id` | TEXT FK → experiments | NULL for root nodes |
| `created_at` | TEXT | ISO 8601 |
| `metadata` | TEXT | JSON, nullable |
| `status` | TEXT | default `"created"` |
| `node_type` | TEXT | hierarchy level name, e.g. `"benchmark"` |

### `runs`
Single execution within an experiment.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `experiment_id` | TEXT FK → experiments | NOT NULL |
| `name` | TEXT | human-readable |
| `config` | TEXT | JSON dict |
| `started_at` | TEXT | ISO 8601, auto-set |
| `ended_at` | TEXT | set on `finish()` |
| `status` | TEXT | `"running"`, `"completed"`, `"failed"` |
| `hostname` | TEXT | auto-captured |
| `git_sha` | TEXT | auto-captured from HEAD |
| `tags` | TEXT | JSON array of strings |
| `notes` | TEXT | plain text, newline-separated |

### `scalar_metrics`
Time-series numeric values.

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | autoincrement |
| `run_id` | TEXT FK → runs | |
| `step` | INTEGER | |
| `name` | TEXT | metric name |
| `value` | REAL | |
| `wall_time` | REAL | seconds since run start |

Unique constraint: `(run_id, name, step)`.

### `artifacts`
Files associated with a run.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `run_id` | TEXT FK → runs | |
| `name` | TEXT | artifact name |
| `kind` | TEXT | `"matrix"`, `"timeseries"`, `"text"` |
| `step` | INTEGER | nullable |
| `rel_path` | TEXT | relative to store root |
| `shape` | TEXT | JSON array for matrices |
| `dtype` | TEXT | numpy dtype string |
| `metadata` | TEXT | JSON, e.g. `{"axes": {...}}` |
| `created_at` | TEXT | ISO 8601 |

### `models`
Versioned model registry.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `name` | TEXT | model name |
| `version` | TEXT | version string |
| `run_id` | TEXT FK → runs | nullable |
| `artifact_path` | TEXT | absolute path to copied model |
| `framework` | TEXT | e.g. `"pytorch"` |
| `metadata` | TEXT | JSON, nullable |
| `created_at` | TEXT | ISO 8601 |

Unique constraint: `(name, version)`.

### `lineage`
Directed edges between experiments, runs, and models.

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | autoincrement |
| `parent_type` | TEXT | `"experiment"`, `"run"`, `"model"` |
| `parent_id` | TEXT | |
| `child_type` | TEXT | `"experiment"`, `"run"`, `"model"` |
| `child_id` | TEXT | |
| `relation` | TEXT | `"derived_from"`, `"branched_from"` |
| `metadata` | TEXT | JSON, nullable |
| `created_at` | TEXT | ISO 8601 |

Unique constraint: `(parent_type, parent_id, child_type, child_id, relation)`.

### `run_params`
Categorical/string key-value attributes for a run.

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | autoincrement |
| `run_id` | TEXT FK → runs | |
| `name` | TEXT | param name |
| `value` | TEXT | param value |

Unique constraint: `(run_id, name)`.

### `hierarchy`
User-defined level ordering.

| Column | Type | Notes |
|--------|------|-------|
| `level_order` | INTEGER PK | 0-indexed |
| `level_name` | TEXT UNIQUE | e.g. `"benchmark"` |

### `todos`
Task notes scoped to global, experiment, or run.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `scope_type` | TEXT | `"global"`, `"experiment"`, `"run"` |
| `scope_id` | TEXT | nullable (NULL for global) |
| `content` | TEXT | |
| `done` | INTEGER | 0 or 1 |
| `priority` | INTEGER | 0=low, 1=mid, 2=high |
| `created_at` | TEXT | ISO 8601 |
| `completed_at` | TEXT | nullable |

---

## Store Directory Layout

```
.extract/
├── extract.db                          # SQLite database
├── extract.db-wal                      # WAL journal (auto-managed)
├── extract.db-shm                      # shared memory (auto-managed)
├── config.toml                         # TUI configuration
├── sync.lock                           # present during sync operations
├── artifacts/
│   └── {run_id}/
│       ├── matrices/{name}.npy         # NumPy arrays
│       ├── timeseries/{name}.json      # {"steps": [], "values": []}
│       └── text/{name}.md              # markdown text
└── models/
    └── {model_name}/{version}/         # copied model files
```

---

## Configuration (`config.toml`)

### `[store]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `hierarchy` | `string` | (none) | Experiment tree levels separated by ` > `, e.g. `"benchmark > method > variant"` |

### `[summary]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sections` | `string[]` | `["runs", "metrics", "tables", "curves"]` | Display order in detail panel |
| `curve_width` | `int` | `80` | Chart width as % of panel (1-100) |
| `curve_smooth` | `bool` | `false` | Catmull-Rom curve interpolation |

### `[compare]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sections` | `string[]` | `["pivot", "config", "tables", "curves"]` | Display order in compare view |
| `curve_width` | `int` | `50` | Chart width as % of panel (1-100) |

### `[notifications]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `timeout` | `int` | `3` | Auto-dismiss timeout in seconds |

### `[metrics]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `minimize` | `string[]` | `[]` | Metrics where lower is better |
| `maximize` | `string[]` | `[]` | Metrics where higher is better |

Unlisted metrics fall back to name-based heuristics (see [Metric Direction](#metric-direction)).

### `[[tables.highlight]]`
Ordered highlight rules for matrix/table cells. First match wins.

| Field | Type | Description |
|-------|------|-------------|
| `eq` | `float` | Exact value match |
| `min` | `float` | Inclusive lower bound |
| `max` | `float` | Exclusive upper bound |
| `pattern` | `string` | Substring match |
| `color` | `string` | Color name (see below) |

Colors: `"red"`, `"green"`, `"yellow"`, `"blue"`, `"cyan"`, `"magenta"`, `"white"`, `"black"`, `"darkgray"`, `"orange"`, `"none"` (terminal default).

### `[theme]`
All values are hex color strings. Omitted fields use ANSI 16-color defaults.

| Key | Default | Description |
|-----|---------|-------------|
| `fg` | `"#cdd6f4"` | Foreground text |
| `bg` | `"#1e1e2e"` | Background |
| `accent` | `"#89b4fa"` | Primary accent |
| `accent_dim` | `"#585b70"` | Dimmed accent |
| `success` | `"#a6e3a1"` | Success indicators |
| `warning` | `"#f9e2af"` | Warning indicators |
| `error` | `"#f38ba8"` | Error indicators |
| `border` | `"#585b70"` | Unfocused borders |
| `border_focused` | `"#89b4fa"` | Focused borders |

---

## Metric Direction

Metric direction (minimize vs maximize) determines ranking, comparison arrows, and improvement highlighting.

### Configuration

Override direction for specific metrics in `config.toml`:

```toml
[metrics]
minimize = ["forgetting_rate", "custom_loss"]
maximize = ["custom_score"]
```

### Heuristic Fallback

Metrics not listed in config use name-based heuristics. These patterns are recognized as **minimize** (lower is better):

`loss`, `error`, `perplexity`, `mse`, `mae`, `rmse`, `nll`, `cer`, `wer`, `fid`, `divergence`

All other unlisted metrics default to **maximize** (higher is better).

---

## Full Usage Example

Assumes `config.toml` has `[store] hierarchy = "benchmark > method > variant"`.

```python
import numpy as np
from extract import Store

store = Store()

# Create experiment hierarchy
exp = store.experiment({
    "benchmark": "cifar100",
    "method": "ewc",
    "variant": "lambda-sweep"
})

# Context manager approach
with exp.run(config={"lr": 0.001, "lambda": 1.0, "epochs": 50}, name="ewc-l1.0") as run:
    for step in range(50):
        run.log(step=step, loss=2.3 - step * 0.04, accuracy=step * 0.018)
    run.log(step=0, arch="resnet18", optimizer="adam")
    run.log_table("confusion_matrix", np.random.rand(10, 10), axes={"rows": "true", "cols": "predicted"})
    run.log_timeseries("lr_schedule", steps=list(range(50)), values=[0.001 * (0.95 ** i) for i in range(50)])
    run.log_text("notes", "## Observations\nEWC with lambda=1.0 shows stable forgetting curve.")
    run.tag("sweep", "production-candidate")
    run.note("Best lambda value in sweep")
    run.todo("Try lambda=0.5 next", priority=1)
    run.register_model("ewc-cifar100", "v1.0", "/tmp/model.pt", framework="pytorch")
    run.derived_from(model="baseline-cifar100", version="v0.9")

# Direct call approach — run spans multiple scopes
run = exp.run(config={"lr": 0.005}, name="ewc-l0.5")
run.log(step=0, loss=2.1)
run.tag("sweep")
run.finish()          # explicit finalize (idempotent)

# Global TODO
store.todo("Write up results for paper", priority=2)

store.close()
```

---

## Source Structure

```
rust/src/
├── main.rs          # entry point, CLI args, main loop
├── app.rs           # AppState, View/Focus enums, all application state
├── db.rs            # SQLite read-only access layer
├── model.rs         # data structs: Experiment, Run, ScalarMetric, Artifact, Model, etc.
├── config.rs        # TOML config parsing, theme/color handling
├── keys.rs          # keybinding constants and key matching
├── event.rs         # event handling (Key, Tick, Resize)
├── artifact.rs      # NumPy table loading, timeseries JSON loading, CellValue handling
└── ui/
    ├── layout.rs    # main layout orchestrator + event dispatcher
    ├── tree.rs      # experiment tree navigator
    ├── detail.rs    # detail panel (Summary/Info tabs)
    ├── dashboard.rs # dashboard view
    ├── summary.rs   # renders runs, metrics, curves, tables
    ├── chart.rs     # line chart rendering
    ├── compare.rs   # compare view (side-by-side runs)
    ├── diff.rs      # diff view (config differences)
    ├── search.rs    # search popup
    ├── popup.rs     # run picker, run browser, delete confirm
    ├── selection.rs # selection window for marked runs
    ├── registry.rs  # model registry view
    ├── lineage.rs   # lineage DAG view
    ├── todo.rs      # TODO management view
    ├── help.rs      # help overlay
    ├── statusbar.rs # status bar
    ├── theme.rs     # theme color/style definitions
    └── heatmap.rs   # heatmap visualization

python/src/extract/
├── __init__.py      # public API: Store, Experiment, Run, sync
├── __main__.py      # CLI entry point
├── store.py         # Store class: DB, hierarchy, experiment creation
├── experiment.py    # Experiment class: run creation, run listing
├── run.py           # Run class: logging, artifacts, models, lineage
├── metrics.py       # helpers: save_npy, load_npy, save_timeseries, etc.
└── sync.py          # sync: rsync, tar archives, DB merging
```
