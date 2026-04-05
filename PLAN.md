# Extract — Future Work

## Phase 6: MCP Server
- `python -m extract.mcp` exposing tools for LLM agents
- Tools: list_experiments, list_runs, get_run, compare_runs, search, create_todo, list_todos, log_metrics, get_lineage, list_models

## Phase 7: Polish
- `/` fuzzy search across experiments, runs, tags, notes
- `?` help overlay showing all keybindings
- `:` command palette
- `~/.config/extract/theme.toml` full RGB color overrides
- Lazy loading and pagination for large stores

## Beyond: High Value
- **Model checkpoint storage** — copy files into `.extract/models/`, track size/hash, `extract model export`
- **Live reload** — WAL-aware auto-refresh when training writes new data
- **`extract init`** — CLI to initialize `.extract/` with hierarchy config interactively
- **Heatmap cell annotations** — select cells in accuracy matrices, add notes

## Beyond: Medium Value
- **Run tagging from TUI** — tag/untag runs without Python SDK
- **Notes editor** — inline markdown notes on runs in detail panel
- **Export to CSV/JSON** — `extract export runs --format csv`
- **Run status management** — mark failed, archive old runs from TUI
- **Cross-experiment diff** — compare best run from EWC vs best from SI

## Beyond: Nice to Have
- **Sparklines in tree** — tiny inline loss curves next to leaf experiment names
- **Run completion notifications** — desktop notification when a run finishes
- **Remote TUI** — `extract-tui --remote user@hpc:/path` reads over SSH without syncing
