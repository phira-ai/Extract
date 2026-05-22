# Extract

Local-first experiment tracking for deep learning. Extract pairs a small Python SDK with a fast Rust TUI so you can log runs, watch training live, compare variants, inspect artifacts, and keep everything in a project-local SQLite store.

No hosted service. No daemon. No account. One `.extract/` directory per project.

## Why Extract

- **Local-first by default** — experiments live beside your code in `.extract/`.
- **Hierarchical experiments** — organize runs as `benchmark > model > variant` or any hierarchy you choose.
- **Live terminal UI** — stream curves during training, compare marked runs, browse artifacts, tags, notes, TODOs, lineage, and model registry entries.
- **Separated metric surfaces** — `run.curve()` stores dense per-step training series; `run.log()` stores headline metrics for summaries and rankings.
- **Portable stores** — sync with `rsync`, archive as `tar.gz`, or move one directory between machines.
- **Agent-readable** — optional read-only MCP server exposes experiments to Claude Code, Claude Desktop, and other MCP hosts.

## Install

```bash
pip install extract-tracker
```

Then initialize a store in your project:

```bash
extract init
```

`extract init` writes `.extract/config.toml` and asks for your experiment hierarchy. Example hierarchy:

```text
benchmark > model > variant
```

## 60-second quickstart

```python
from extract import Store

store = Store()
exp = store.experiment({
    "benchmark": "imagenet",
    "model": "resnet50",
    "variant": "lr_0.01",
})

with exp.run(config={"lr": 0.01}, total_steps=1000) as run:
    for step in range(1000):
        loss, acc = train_step(...)
        run.curve(step=step, train_loss=loss, train_acc=acc)

    run.log(final_acc=acc, final_loss=loss)
    run.tag("baseline")
    run.note("Stable run; use as comparison anchor.")
```

Open another terminal while training runs:

```bash
extract tui
```

Curves update live. Mark runs with `Space`, press `c` to compare, press `d` to diff configs.

## Core concepts

### Store

A store is a project-local `.extract/` directory:

```text
.extract/
├── extract.db
├── config.toml
├── artifacts/
└── models/
```

SQLite uses WAL mode, so training scripts can write while the TUI reads.

### Experiment

An experiment is a node in your configured hierarchy. With `benchmark > model > variant`, this call creates or reuses each path component:

```python
exp = store.experiment({
    "benchmark": "mmlu",
    "model": "llama-3.2-1b",
    "variant": "lora-r16",
})
```

### Run

A run is one execution under an experiment. Runs track config, status, hostname, git SHA, timestamps, tags, notes, metrics, artifacts, models, lineage, and TODOs.

```python
with exp.run(config=config, name="seed-1", total_steps=len(loader)) as run:
    ...
```

### Metrics

Use `curve()` for dense per-step values and `log()` for final or headline metrics.

```python
run.curve(step=step, train_loss=loss, eval_acc=acc)  # live charts
run.log(best_acc=best_acc, final_loss=final_loss)    # summary/ranking columns
```

### Artifacts and models

```python
run.log_table("confusion_matrix", matrix)
run.log_text("notes", "Observed better calibration after warmup.")
run.register_model("resnet50", "v1", "checkpoints/best.pt", framework="pytorch")
```

## CLI

```bash
extract init                             # create .extract/config.toml
extract tui                              # open Rust TUI
extract tui --store path/to/.extract     # browse another store
extract sync push user@hpc:/path/.extract/
extract sync pull user@hpc:/path/.extract/
extract sync export backup.tar.gz
extract sync import backup.tar.gz
python -m extract.mcp --store .extract   # read-only MCP server
```

## TUI highlights

| Key | Action |
|---|---|
| `j` / `k` | Move down / up |
| `Enter` | Expand / select |
| `Space` | Mark run for comparison |
| `c` | Compare marked runs |
| `d` | Diff marked run configs |
| `r` | Run browser |
| `/` | Search experiments and runs |
| `t` | Edit tags |
| `n` | Append note |
| `M` | Model registry |
| `T` | TODO view |
| `L` | Lineage DAG |
| `?` | Help |
| `q` | Quit |

Full keymap: [manual/tui.md](manual/tui.md).

## Configuration

Edit `.extract/config.toml`:

```toml
[store]
hierarchy = "benchmark > model > variant"

[summary]
sections = ["runs", "metrics", "tables", "curves"]
curve_width = 80
curve_smooth = false

[metrics]
minimize = ["loss", "forgetting_rate"]
maximize = ["accuracy", "f1", "custom_score"]
order = "alpha"

[[tags.definitions]]
name = "baseline"
color = "blue"

[theme]
accent = "#89b4fa"
error = "#f38ba8"
```

Full config reference: [manual/config.md](manual/config.md).

## Sync between machines

```bash
extract sync push user@hpc:/scratch/project/.extract/
extract sync pull user@hpc:/scratch/project/.extract/
```

Pull merges by experiment path and run ULID, so stores can move between laptop, workstation, and HPC jobs without a central server.

More: [manual/sync.md](manual/sync.md).

## MCP server

Expose your store to LLM agents with a read-only MCP server:

```bash
python -m extract.mcp --store .extract
```

Agents can list experiments, inspect runs, compare metrics, search tags/status, list TODOs, walk lineage, and read model registry metadata.

More: [manual/mcp.md](manual/mcp.md).

## Development

Use Nix for project dependencies:

```bash
nix develop
pip install -e .
pytest python/tests
```

Build distributions locally:

```bash
python -m build
python -m twine check dist/*
pip install dist/*.whl
extract --help
```

Packaging and release notes: [manual/packaging.md](manual/packaging.md).

## Project status

Extract is early-stage and optimized for local ML research workflows. Current package name is `extract-tracker`; Python import and CLI are both `extract`.

License not declared yet. Choose and add `LICENSE` before public distribution.
