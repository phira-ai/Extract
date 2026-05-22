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
