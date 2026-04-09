# Extract

Local-first experiment tracking for deep learning. A Rust TUI for browsing, comparing, and analyzing experiments, paired with a Python SDK for logging metrics, artifacts, and models.

Built for hierarchical experiment organization (benchmark > model > variant), run comparison, and artifact management — all stored in a single SQLite database with no server required.

## Install

```bash
pip install extract-tracker
```

### Development

```bash
nix develop          # dev shell with Rust, Python, SQLite, maturin
pip install -e .     # editable install (builds Rust binary + links Python)
```

## Quick Start

```bash
pip install extract-tracker
extract init
```

`extract init` walks you through choosing a hierarchy and writes
`.extract/config.toml`. After that:

```python
from extract import Store

store = Store()
exp = store.experiment({
    "benchmark": "imagenet",
    "model":     "resnet50",
    "variant":   "lr_0.01",
})
with exp.run(config={"lr": 0.01}, total_steps=1000) as run:
    for step in range(1000):
        loss, acc = train_step(...)
        run.curve(step=step, train_loss=loss, train_acc=acc)   # streams to live chart
    run.log(final_acc=acc)                                     # headline metric

# Browse with: extract tui
```

`run.curve()` streams high-frequency points to the live chart panel.
`run.log()` records headline metrics that show up in the Summary tab and in
ranking comparisons. The two write to physically separate tables, so streaming
training losses never clutter your headline-metric surfaces.

`total_steps=N` declares the training loop length so the chart's x-axis is
pinned at `[0, N-1]` from the moment the chart appears — the curve fills
left-to-right rather than rescaling.

## Live Watching

While a run is training, the TUI updates the detail and compare panels in
place — curves fill in along their fixed axis, latest metrics tick over, and
the status bar shows a `● LIVE` badge whenever any visible run is `running`.
No keybinding to enable; the tick loop polls SQLite's `data_version` and only
re-queries when the database has actually changed, so it stays cheap on idle
TUIs and large stores.

Open the TUI in a second terminal while training is running:

```bash
extract tui
```

Navigate to a leaf experiment and the curves will fill in as the training
loop calls `run.curve(...)`. Mark two runs with `Space` and press `c` to
watch parallel variants race in the compare view.

## TUI Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `gg` / `G` | Jump to top / bottom |
| `Enter` | Expand / select |
| `h` / `l` | Cycle panels (same as Tab / Shift+Tab) |
| `Tab` / `Shift+Tab` | Cycle panels |
| `1` / `2` / `3` | Focus Tree / Detail / Selection panel |
| `/` | Search experiments and runs |
| `?` | Help overlay |
| `q` | Quit |

### Experiment Tree

| Key | Action |
|-----|--------|
| `Left` | Go to parent node |
| `Right` | Go to first child / enter leaf |
| `Space` | Mark run for comparison |

### Detail Panel

| Key | Action |
|-----|--------|
| `Left` / `Right` | Cycle through runs |
| `S` | Summary tab |
| `I` | Info tab |

### Actions

| Key | Action |
|-----|--------|
| `t` | Edit tags (Summary tab) |
| `n` | Append note |
| `Ctrl+E` | Edit notes in $EDITOR |
| `Shift+F` | Mark run failed |
| `Shift+C` | Mark run completed |
| `Shift+A` | Archive run / experiment |
| `Shift+U` | Unarchive |
| `Shift+H` | Toggle show archived |

### Runs & Comparison

| Key | Action |
|-----|--------|
| `r` | Open run browser |
| `c` | Compare marked runs |
| `d` | Diff marked runs (config) |
| `x` | Delete run |

### Views

| Key | Action |
|-----|--------|
| `M` | Model registry |
| `T` | TODOs |
| `L` | Lineage DAG |

### TODO View

| Key | Action |
|-----|--------|
| `Space` | Toggle done |
| `a` | Add TODO |
| `x` | Delete TODO |
| `0` / `1` / `2` | Set priority (low / mid / high) |
| `A` / `G` / `E` / `R` | Filter: All / Global / Experiment / Run |

## Sync

Transfer experiment stores between machines:

```bash
extract sync push user@hpc:/path/.extract/     # upload via rsync
extract sync pull user@hpc:/path/.extract/     # download + merge
extract sync export backup.tar.gz              # archive to file
extract sync import backup.tar.gz              # restore from archive
```

Sync merges databases intelligently — experiments match by path, runs use ULIDs so they never collide.

## MCP Server

Expose your store to LLM agents (Claude Code, Claude Desktop, any MCP-capable host) via a read-only MCP server. Agents can browse experiments, compare runs, search by tag, and walk lineage without you writing any glue code.

```bash
python -m extract.mcp [--store .extract]
```

Run by an MCP host as a subprocess over stdio. The default `--store .extract` resolves relative to the host's cwd, so launching `claude` in a project folder automatically binds to that project's store.

### Register with Claude Code

Drop a `.mcp.json` at your project root:

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

Then ask the agent things like *"compare the two resnet50 runs and tell me which had the lowest final loss"* or *"what experiments are tagged production-candidate?"* — it will reach for the matching tools automatically.

### Tools (all read-only)

| Tool | Purpose |
|---|---|
| `list_experiments` | Browse the experiment hierarchy with run counts |
| `list_runs` | List runs (all or for one experiment) with labels and config summaries |
| `get_run` | Full detail for one run: config, final metrics, params, artifacts, todos |
| `compare_runs` | 2–10 runs with rankings, optional histories, config diffs |
| `search` | Substring + structured filters (tag, status, prefix, date range) |
| `list_todos` | TODOs scoped global / experiment / run |
| `get_lineage` | BFS walk of the lineage DAG (ancestors, descendants, or both) |
| `list_models` | Registered models with metadata |

Full schemas, response shapes, and error catalog: see [DOC.md](DOC.md#mcp-server).

## Configuration

Edit `.extract/config.toml`. Settings are grouped by what they affect:

### Store Setup

```toml
[store]
hierarchy = "benchmark > model > variant"
```

### View Layout — what each TUI panel/view displays

```toml
# Summary tab in Detail panel (S)
[summary]
sections = ["runs", "metrics", "tables", "curves"]
curve_width = 80       # chart width as % of panel
# curve_height = 10    # chart height in lines (default: auto-scales by metric count)
curve_smooth = false

# Info tab in Detail panel (I) + Config section in Compare/Diff views
# Nested configs are flattened with dot-notation (model.lora_r, task.num_train_epochs)
# Full glob syntax: * (single segment), ** (multi-segment), ? (single char), {a,b}
# Prefix with ! to exclude: ["model.**", "!model.parent"]
[info]
fields = ["model.*", "task.num_train_epochs"]   # empty = show all

# Compare view (c with marked runs)
[compare]
sections = ["pivot", "config", "tables", "curves"]
curve_width = 50
# curve_height = 10
```

### Data Interpretation — how metrics and table values are evaluated

```toml
[metrics]
minimize = ["forgetting_rate"]    # lower is better
maximize = ["custom_score"]       # higher is better
# Unlisted metrics use name heuristics (e.g. "loss" → minimize)

# Cell highlight rules for tables (first match wins)
# Fields: eq (exact), min (inclusive), max (exclusive), pattern (substring), color (name or hex)
# Colors: red, green, yellow, blue, cyan, magenta, white, orange, none, or hex (#ff6600)
[[tables.highlight]]
min = 0.7
color = "red"
```

### Appearance

```toml
[theme]
fg = "#cdd6f4"
bg = "#1e1e2e"
accent = "#89b4fa"
accent_dim = "#585b70"
success = "#a6e3a1"
warning = "#f9e2af"
error = "#f38ba8"
border = "#585b70"
border_focused = "#89b4fa"

[notifications]
timeout = 3
```

## Store Structure

```
.extract/
├── extract.db           # SQLite database (WAL mode)
├── config.toml          # TUI configuration
├── artifacts/
│   └── {run_id}/
│       ├── matrices/*.npy
│       └── text/*.md
└── models/
    └── {name}/{version}/
```

## Tech Stack

- **TUI**: Rust + ratatui + crossterm + rusqlite
- **SDK**: Python 3.10+ + numpy + ulid
- **Storage**: SQLite (WAL mode) + .npy artifacts
- **Dev**: Nix flake
