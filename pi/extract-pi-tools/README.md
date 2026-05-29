# extract-pi-tools

Installable Pi package that exposes Extract read-only query commands as native Pi tools.

The tools return compact summaries by default to avoid wasting model context. For full configs, curves, or bulk aggregation, use the `meta.command` returned by a tool inside context-mode (`ctx_execute`) and print only the derived answer.

## Install

From the Extract repository:

```bash
pi install ./pi/extract-pi-tools
```

Project-scoped install:

```bash
pi install -l ./pi/extract-pi-tools
```

Then restart Pi or run `/reload`.

## Requirements

Extract CLI must be available from the project where Pi runs. The extension checks, in order:

1. `.venv/bin/extract`
2. `extract` on `PATH`
3. `.venv/bin/python -m extract`
4. `python -m extract`

For development:

```bash
nix develop
.venv/bin/python -m pip install -e .
```

## Tools

Native tools cap list outputs to 25 rows and summarize large objects. `extract_get_run` never returns full config; pass `configKeys` for specific dot-path config values.

- `extract_list_experiments`
- `extract_list_runs`
- `extract_get_run`
- `extract_compare_runs`
- `extract_search`
- `extract_list_todos`
- `extract_get_lineage`
- `extract_list_models`
