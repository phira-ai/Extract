# MCP Server

Extract can expose a store through a read-only MCP server. Use this with Claude Code, Claude Desktop, or any MCP-capable host.

## Run server

```bash
python -m extract.mcp --store .extract
```

The MCP host normally launches this command as a subprocess over stdio. Relative `--store` paths resolve against the host process cwd.

## Claude Code config

Create `.mcp.json` in project root:

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

Then ask questions such as:

- "Compare the two resnet50 runs and tell me which had the lowest final loss."
- "What experiments are tagged production-candidate?"
- "Show lineage for the best MMLU run."

## Tools

All tools are read-only.

| Tool | Purpose |
|---|---|
| `list_experiments` | Browse experiment hierarchy with run counts |
| `list_runs` | List runs, optionally for one experiment, with labels and config summaries |
| `get_run` | Full run detail: config, final metrics, params, artifacts, TODOs |
| `compare_runs` | Compare 2–10 runs with rankings, optional histories, config diffs |
| `search` | Substring search plus tag/status/prefix/date filters |
| `list_todos` | TODOs scoped global / experiment / run |
| `get_lineage` | BFS walk of lineage DAG: ancestors, descendants, or both |
| `list_models` | Registered models with metadata |

Full schemas, response shapes, and error catalog live in [`DOC.md`](../DOC.md#mcp-server).

## Agent CLI surface

The same read-only operations are also exposed as JSON CLI commands for agents or hosts that cannot attach the MCP server:

```bash
extract experiments list --store .extract --format json
extract runs list --limit 20 --store .extract --format json
extract runs get RUN_ID --store .extract --format json
extract runs compare RUN_A RUN_B --store .extract --format json
extract search --query resnet --tag production-candidate --store .extract --format json
extract todos list --scope-type run --scope-id RUN_ID --store .extract --format json
extract lineage get run RUN_ID --direction both --store .extract --format json
extract models list --store .extract --format json
```

An installable Pi package in `pi/extract-pi-tools/` wraps these commands as native Pi tools such as `extract_list_runs`, `extract_get_run`, and `extract_compare_runs`:

```bash
pi install ./pi/extract-pi-tools      # user-scoped
pi install -l ./pi/extract-pi-tools   # project-scoped
```

Reload or restart Pi after installation. These native tools are context-safe summaries, not raw dumps: list tools cap rows, `extract_get_run` omits full config unless specific `configKeys` are requested, and large raw JSON is written to a temp file path. For full configs, curves, or bulk aggregation, rerun the returned `meta.command` inside `ctx_execute` and print only the derived answer.
