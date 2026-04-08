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

**`store.experiment(spec: dict[str, str]) -> Experiment`**
- Keys must be hierarchy levels in order from root. Creates intermediate nodes. Values cannot skip levels.
  ```python
  store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "v1"})
  store.experiment({"benchmark": "imagenet"})  # partial — stops at benchmark level
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
extract init [path] [--hierarchy "a > b > c"] [--no-gitignore]
extract tui [--store .extract]
extract sync push <remote> [--root .extract]
extract sync pull <remote> [--root .extract]
extract sync export <output.tar.gz> [--root .extract]
extract sync import <archive.tar.gz> [--root .extract]
```

**`extract init`** — Bootstrap a `.extract/` store with a hierarchy. Interactive by default; use `--hierarchy` for non-interactive/CI use. Refuses if the store is already configured.

### Install

```bash
pip install extract-tracker            # SDK + CLI + TUI binary + MCP server
```

---

## MCP Server

Read-only MCP (Model Context Protocol) server that exposes the store to LLM agents (Claude Code, Claude Desktop, any MCP-capable host). Lets an agent inspect experiments, runs, metrics, configs, lineage, models, and TODOs without writing Python.

### Entry Point

```bash
python -m extract.mcp [--store PATH]
```

- `--store PATH` — path to the `.extract/` directory. Default: `.extract` (resolved relative to the server's cwd, which is the MCP host's cwd). Launching `claude` in a project folder automatically binds to that project's store.
- Transport: stdio. Spawned by the MCP host as a subprocess; not invoked interactively.
- Read-only. No tools that mutate the store in v1.

### Registering with Claude Code

Add a project-scoped `.mcp.json` at the project root:

```json
{
  "mcpServers": {
    "extract": {
      "command": ".venv/bin/python",
      "args": ["-m", "extract.mcp"]
    }
  }
}
```

The relative `command` resolves against Claude's cwd (= project root when launched there). For globally-installed Python: replace with the absolute path or `python` if it's on `$PATH`.

### Tools

All 8 tools are read-only. Run IDs are ULIDs; agents discover them via `list_runs` / `search` and pass them to other tools verbatim. Listing tools share an envelope: `{items, total, truncated}`, default `limit=50`, hard cap `500` (clamps silently with `limit_clamped: true`).

**`list_experiments(prefix: str = "", limit: int = 50) -> dict`**
- Lists experiments, optionally filtered by path prefix. Each item: `{id, path, name, node_type, parent_id, n_runs}`.

**`list_runs(experiment_id: str | None = None, limit: int = 50) -> dict`**
- All runs (newest-first) or scoped to one experiment. Each item: `{id, label, experiment_id, experiment_path, name, status, started_at, ended_at, tags, git_sha, hostname, config_summary}`. `config_summary` is `{n_keys, top_level_keys}` — call `get_run` for the full config.

**`get_run(run_id: str) -> dict`**
- Full detail: `{id, experiment_id, experiment_path, name, label, status, started_at, ended_at, hostname, git_sha, tags, notes, config, metrics_final, metrics_available, run_params, artifacts, todos}`. `metrics_final` is the last value of each scalar metric; histories not included (use `compare_runs` with `include_history=True`).

**`compare_runs(run_ids: list[str], include_history: bool = False) -> dict`**
- 2–10 runs. Returns `{runs, metrics, config_diffs}`. Per metric: `{direction, values, ranking}` plus optional `history` (a list of `[step, value]` pairs per run). `direction` is `"min"` or `"max"` from name heuristics (`loss`, `error`, `mse`, etc. → min; everything else → max). `ranking` is best-to-worst by final value. `config_diffs` only contains keys where at least two runs differ; nested configs are flattened with dot notation (`method.lora_r`).

**`search(query: str = "", filters: dict | None = None, limit: int = 50) -> dict`**
- Substring + structured filter search over runs. Returns the same shape as `list_runs`.
- `query`: case-insensitive substring against run name, tags, notes. Empty = no text filter.
- `filters` (all AND-combined, all optional): `tag` (str), `status` (`"running" | "completed" | "failed"`), `experiment_prefix` (str), `started_after` (ISO 8601), `started_before` (ISO 8601).

**`list_todos(scope_type: str = "global", scope_id: str | None = None, include_done: bool = False, limit: int = 50) -> dict`**
- `scope_type`: `"global" | "experiment" | "run"`. `scope_id` required for non-global scopes, must be `None` for global. `include_done=False` by default.

**`get_lineage(node_type: str, node_id: str, direction: str = "both", depth: int = 2) -> dict`**
- BFS walk of the lineage DAG. `node_type`: `"experiment" | "run" | "model"`. `direction`: `"ancestors" | "descendants" | "both"`. `depth`: 1–5. Returns flat `{root, nodes, edges}` (not a tree — handles DAG diamonds). Labels: `path#name` for runs, `path` for experiments, `name@version` for models.

**`list_models(name_prefix: str = "", limit: int = 50) -> dict`**
- Each item: `{id, name, version, run_id, framework, artifact_path, metadata, created_at}`.

### Errors

All tool-visible errors raise `ValueError` with agent-readable strings: `"Run not found: 'X'"`, `"compare_runs requires at least 2 run_ids (got 1)"`, `"Unknown filter: 'X'. Valid filters: tag, status, experiment_prefix, started_after, started_before"`, etc. Listing tools accept `limit > 500` and silently clamp (with `limit_clamped: true` in the response) rather than erroring — the agent can recover without retrying.

---

## Database Schema

SQLite with WAL journal mode. 9 tables:

### `experiments`
Hierarchical namespace nodes.

| Column | Type | Notes |
|--------|------|-------|
| `id` | TEXT PK | ULID |
| `path` | TEXT NOT NULL | slash-delimited, e.g. `"imagenet/resnet50/v1"` |
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

#### Store Setup

### `[store]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `hierarchy` | `string` | (none) | Experiment tree levels separated by ` > `, e.g. `"benchmark > model > variant"` |

#### View Layout — controls what each TUI panel/view displays

### `[summary]`
Controls the Summary tab in the Detail panel (selected via `S`).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sections` | `string[]` | `["runs", "metrics", "tables", "curves"]` | Display order in detail panel |
| `curve_width` | `int` | `80` | Chart width as % of panel (1-100) |
| `curve_height` | `int` | auto | Chart height in lines (auto-scales by metric count: 12/10/8/6) |
| `curve_smooth` | `bool` | `false` | Catmull-Rom curve interpolation |

### `[info]`
Controls the Info tab in the Detail panel (selected via `I`) and the Config section in Compare/Diff views. Nested configs are flattened with dot-notation (e.g. `method.lora_r`, `task.num_train_epochs`).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `fields` | `string[]` | `[]` (show all) | Glob patterns for which config keys to display |

Dots act as path separators so glob semantics apply per segment:

| Pattern | Matches | Does not match |
|---------|---------|----------------|
| `method.*` | `method.name`, `method.lora_r` | `method.deep.nested` |
| `method.**` | `method.name`, `method.deep.nested` | `model.name` |
| `*.name` | `method.name`, `model.name` | `method.deep.name` |
| `**.name` | `method.name`, `method.deep.name` | `method.lora_r` |
| `method.lora_*` | `method.lora_r`, `method.lora_alpha` | `method.name` |
| `method.lora_?` | `method.lora_r` | `method.lora_alpha` |
| `{method,model}.*` | `method.name`, `model.name` | `task.name` |
| `!method.parent` | (excludes `method.parent`) | |

Negation patterns (`!`) exclude matching keys. Combine with positive patterns: `["method.**", "!method.parent"]` shows everything under method except `parent`. When empty (default), all config keys are shown.

### `[compare]`
Controls the Compare view (triggered via `c` with marked runs).

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `sections` | `string[]` | `["pivot", "config", "tables", "curves"]` | Display order in compare view |
| `curve_width` | `int` | `50` | Chart width as % of panel (1-100) |
| `curve_height` | `int` | auto | Chart height in lines (auto-scales by metric count: 12/10/8/6) |

#### Data Interpretation — controls how metrics and table values are evaluated

### `[metrics]`
Determines metric direction (minimize vs maximize) for ranking, comparison arrows, and improvement highlighting.

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

Colors: `"red"`, `"green"`, `"yellow"`, `"blue"`, `"cyan"`, `"magenta"`, `"white"`, `"black"`, `"darkgray"`, `"orange"`, `"none"` (terminal default), or any hex color (e.g. `"#ff6600"`).

#### Appearance

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

### `[notifications]`
| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `timeout` | `int` | `3` | Auto-dismiss timeout in seconds |

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

Assumes `config.toml` has `[store] hierarchy = "benchmark > model > variant"`.

```python
import numpy as np
from extract import Store

store = Store()

# Create experiment hierarchy
exp = store.experiment({
    "benchmark": "imagenet",
    "model": "resnet50",
    "variant": "lr-sweep"
})

# Context manager approach
with exp.run(config={"lr": 0.001, "bs": 32, "epochs": 50}, name="resnet50-lr1e3") as run:
    for step in range(50):
        run.log(step=step, loss=2.3 - step * 0.04, accuracy=step * 0.018)
    run.log(step=0, arch="resnet50", optimizer="sgd")
    run.log_table("confusion_matrix", np.random.rand(1000, 1000), axes={"rows": "true", "cols": "predicted"})
    run.log_timeseries("lr_schedule", steps=list(range(50)), values=[0.001 * (0.95 ** i) for i in range(50)])
    run.log_text("notes", "## Observations\nResNet50 with lr=1e-3 converges stably.")
    run.tag("sweep", "production-candidate")
    run.note("Best learning rate in sweep")
    run.todo("Try lr=0.005 next", priority=1)
    run.register_model("resnet50-imagenet", "v1.0", "/tmp/model.pt", framework="pytorch")
    run.derived_from(model="baseline-imagenet", version="v0.9")

# Direct call approach — run spans multiple scopes
run = exp.run(config={"lr": 0.005}, name="resnet50-lr5e3")
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
├── keys.rs          # keybinding constants and key matching (h/l=tab, arrows=nav/cycle)
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
├── __main__.py      # CLI entry point (extract tui / sync)
├── store.py         # Store class: DB, hierarchy, experiment creation
├── experiment.py    # Experiment class: run creation, run listing
├── run.py           # Run class: logging, artifacts, models, lineage
├── metrics.py       # helpers: save_npy, load_npy, save_timeseries, etc.
├── sync.py          # sync: rsync, tar archives, DB merging
└── mcp.py           # MCP server: 8 read-only tools over stdio
```
