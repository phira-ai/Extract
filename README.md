# Extract

Local-first experiment tracking for deep learning. A Rust TUI for browsing, comparing, and analyzing experiments, paired with a Python SDK for logging metrics, artifacts, and models.

Built for hierarchical experiment organization (benchmark > method > variant), run comparison, and artifact management — all stored in a single SQLite database with no server required.

## Install

```bash
pip install extract-tracker
```

This installs the Python SDK, the `extract` CLI, and the compiled TUI binary.

### Development

```bash
nix develop          # dev shell with Rust, Python, SQLite, maturin
pip install -e .     # editable install (builds Rust binary + links Python)
```

## Quick Start

### 1. Configure hierarchy

Edit `.extract/config.toml`:

```toml
[store]
hierarchy = "benchmark > method > variant"
```

### 2. Log experiments (Python)

```python
from extract import Store

store = Store()
exp = store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "v1"})

# Context manager — auto-finishes on exit
with exp.run(config={"lr": 0.001, "epochs": 100}, name="run-001") as run:
    run.log(step=0, accuracy=0.85, arch="resnet18")      # headline metrics (summary tables)
    run.log_timeseries("train_loss", steps=list(range(100)),  # curve-only data (plotted, not in summary)
                       values=[0.5 * 0.95**i for i in range(100)])
    run.log_table("confusion_matrix", np_array)
    run.tag("baseline", "production")

# Direct call — for when the run spans multiple files
run = exp.run(config={"lr": 0.001}, name="run-002")
train(model, run)     # pass run around freely
run.finish()          # explicit finalize
```

### 3. Browse experiments (TUI)

```bash
extract tui                    # reads from ./.extract/
extract tui --store /path/to/.extract
```

## TUI Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `gg` / `G` | Jump to top / bottom |
| `Enter` | Expand / select |
| `Tab` / `Shift+Tab` | Cycle panels |
| `1` / `2` / `3` | Focus Tree / Detail / Selection panel |
| `/` | Search experiments and runs |
| `?` | Help overlay |
| `q` | Quit |

### Runs & Comparison

| Key | Action |
|-----|--------|
| `Space` | Mark run for comparison |
| `r` | Open run browser |
| `c` | Compare marked runs |
| `d` | Diff marked runs (config) |
| `h` / `l` | Cycle through runs |
| `x` | Delete run |

### Detail Panel

| Key | Action |
|-----|--------|
| `S` | Summary tab |
| `I` | Info tab |

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

## Configuration

Edit `.extract/config.toml`:

```toml
[store]
hierarchy = "benchmark > method > variant"

[summary]
sections = ["runs", "metrics", "tables", "curves"]
curve_width = 80
curve_smooth = false

[compare]
sections = ["pivot", "config", "tables", "curves"]
curve_width = 50

[metrics]
minimize = ["forgetting_rate"]    # lower is better
maximize = ["custom_score"]       # higher is better
# Unlisted metrics use name heuristics (e.g. "loss" → minimize)

[notifications]
timeout = 3

[tables]
# Cell highlight rules (first match wins)
# [[tables.highlight]]
# min = 0.7
# color = "red"

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
```

### Highlight Rule Fields

- `eq` — exact float match
- `min` — inclusive lower bound
- `max` — exclusive upper bound
- `pattern` — substring match
- `color` — `"red"`, `"green"`, `"yellow"`, `"blue"`, `"cyan"`, `"magenta"`, `"white"`, `"orange"`, `"none"`

## Store Structure

```
.extract/
├── extract.db           # SQLite database (WAL mode)
├── config.toml          # TUI configuration
├── artifacts/
│   └── {run_id}/
│       ├── matrices/*.npy
│       ├── timeseries/*.json
│       └── text/*.md
└── models/
    └── {name}/{version}/
```

## Tech Stack

- **TUI**: Rust + ratatui + crossterm + rusqlite
- **SDK**: Python 3.10+ + numpy + ulid
- **Storage**: SQLite (WAL mode) + .npy / .json artifacts
- **Dev**: Nix flake
