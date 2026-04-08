# `extract init` & SDK Cleanup — Design Spec

## Overview

Add an interactive `extract init` CLI command that bootstraps a `.extract/` store with a configured hierarchy, and harden the Python SDK so that `Store()` requires `config.toml` to exist with a `[store] hierarchy` line. The legacy path-string API on `store.experiment()` is removed; the dict-spec is the only supported API. The interactive wizard is built on `rich` + `questionary` and inherits the user's terminal theme via ANSI-named colors.

**Motivation.** Today, the very first thing a new user must do is manually create `.extract/config.toml` with the right `[store] hierarchy = "..."` line before any dict-spec SDK call works. Skipping that step raises `ValueError: Cannot use dict spec without hierarchy` mid-training-script (`python/src/extract/store.py:271-275`). The hierarchy is the most consequential early decision in an Extract project — every experiment path, every TUI tree node, every dotted config key, every `.extract/artifacts/{run_id}/` lookup is keyed by it. Asking for it interactively, with examples, validation, and a syntax-highlighted preview is dramatically better UX than a runtime stacktrace.

**Scope.** A single cohesive change: add `extract init`, make `Store()` require what `init` writes, drop the legacy escape hatch, and bundle the dependencies it needs. Pre-1.0 cleanup; not backwards compatible.

**Tech stack.** Python 3.10+, `rich>=13.0`, `questionary>=2.0` (both bundled into base `dependencies`), `mcp>=1.0` (also moved from optional to base for consistency — see §5). All interactive UI inherits the terminal's theme via ANSI-named colors only; no hex/RGB anywhere.

---

## 1. Scope

### In scope

1. **New `extract init` CLI command.** Interactive wizard with five visual moments: welcome banner, hierarchy preset picker (with live-validated custom-entry path), gitignore prompt, config preview panel, success panel with quickstart snippet. Non-interactive mode via `extract init --hierarchy "a > b > c"`. Refuses to prompt if `stdin` is not a TTY and `--hierarchy` is missing.

2. **`Store()` hard requirement.** `Store(path)` raises `MissingHierarchyError` from `__init__` if `path/config.toml` is missing or has no `[store] hierarchy`. No state-(b) auto-migration for stores that have a populated `hierarchy` table in the DB but no `config.toml`. No `--migrate` for legacy state-(c) stores.

3. **Remove the legacy path-string API.**
   - Delete the `isinstance(spec, str)` branch in `Store.experiment()` (`store.py:223-233`).
   - Delete the entire `_experiment_by_path` method (`store.py:235-267`).
   - Delete the `ALTER TABLE experiments ADD COLUMN node_type` migration (`store.py:165-169`) — only existed to retrofit pre-hierarchy DBs.
   - Make `node_type TEXT NOT NULL` in the schema (`store.py:31` and `schema/migrations/001_init.sql:13`).
   - Drop the legacy test at `python/tests/test_hierarchy.py:41-43`.

4. **Bundle MCP and pretty-CLI dependencies into base.** Move `mcp>=1.0` from `[project.optional-dependencies]` to `dependencies`. Add `rich>=13.0` and `questionary>=2.0` to `dependencies`. Delete the `[project.optional-dependencies]` section. Drop the `try/except ImportError` fallback in `python/src/extract/mcp.py:16-19`, the `if FastMCP else None` guard at line 25, and the conditional `_tool` decorator at lines 28-36.

5. **Theme inheritance constraint.** All `rich` and `questionary` styling uses ANSI-named colors only:
   - `rich.Style(color="cyan")`, `rich.Style(color="green")`, etc. — never hex strings, never `bright_*` palette names that map to TrueColor.
   - `questionary.Style.from_dict({...})` uses `"ansicyan"`, `"ansigreen"`, `"ansibrightmagenta"`, etc.
   - `rich.syntax.Syntax(..., theme="ansi_dark")` for both the TOML preview panel and the Python quickstart panel.
   - A regression test in `python/tests/test_init.py` greps `python/src/extract/init.py` for hex literals (`"#RRGGBB"`) and asserts none exist.

6. **Continual-learning vocabulary scrub** in user-visible files only:
   - `README.md` — Quick Start examples
   - `DOC.md` — code examples throughout
   - `scripts/generate_test_data.py` — fixture data
   - `python/tests/test_mcp.py` — test fixtures
   - `python/tests/test_hierarchy.py` — test fixtures

   Replace `benchmark > method > variant` with `benchmark > model > variant`. Replace `cifar100`/`tinyimagenet`/`ewc`/`si`/`replay`/`lambda_*` with `imagenet`/`cifar10`/`resnet50`/`vit_base`/`lr_*`. Drop continual-learning-specific terminology (task, method, lambda regularization, online-EWC, etc.). Use generic supervised-learning vocabulary throughout.

   **Not scrubbed:** `docs/superpowers/briefs/`, `docs/superpowers/specs/`, `docs/superpowers/plans/` — historical artifacts that will be cleaned up in a separate later pass.

### Out of scope (explicitly deferred)

- `extract init --migrate` for legacy state-(c) stores. Recovery is `rm -rf .extract && extract init`.
- Auto-detection of state (b) "DB has hierarchy but no config.toml" — refuse like everything else.
- TUI-based init flow. CLI only.
- Multi-store init in one command.
- Importing wandb/mlflow/tensorboard runs.
- A `--force` flag for overwriting existing configured stores.
- Run completion notifications, live reload, any other PLAN.md item.
- Rust TUI changes. The TUI stays permissive (`Option<String>` on `node_type` in `rust/src/model.rs:12`) so users can still browse legacy stores even though the Python SDK refuses to write to them. This asymmetry is intentional: SDK strict, TUI permissive viewer.
- Cross-platform Windows testing of the wizard. Linux is the primary target.

---

## 2. CLI Surface

### Command signature

```
extract init [path] [--hierarchy "a > b > c"] [--no-gitignore]
```

### Arguments

| Argument | Default | Description |
|---|---|---|
| `path` (positional) | `.extract` | Where to create the store directory |
| `--hierarchy "a > b > c"` | (none) | Skip the interactive hierarchy picker; use this value directly |
| `--no-gitignore` | (off) | Skip the gitignore prompt and do not modify `.gitignore` |

No `--yes`, no `--force`, no `--quiet`, no `--migrate`. Surface stays minimal.

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success — config written, or user cleanly aborted at the preview confirm |
| `1` | Refused — existing configured store, file write failure, validation rejection in non-interactive mode |
| `2` | Usage error — non-TTY without `--hierarchy`, malformed `--hierarchy` value |
| `130` | User pressed Ctrl-C (translated from `KeyboardInterrupt`) |

### TTY enforcement

At the start of `init.run(args)`, if `sys.stdin.isatty()` is `False` AND `args.hierarchy is None`:

```
error: extract init requires --hierarchy when running non-interactively.
       Example: extract init --hierarchy "benchmark > model > variant"
```

Exit code 2.

### Pre-flight checks

Before printing the welcome banner:

1. Resolve `path` to an absolute Path. If it does not exist, ok — we'll create it.
2. If `path/config.toml` exists AND it has `[store] hierarchy`:
   ```
   error: <path>/config.toml is already configured with hierarchy 'X > Y > Z'.
          Refusing to overwrite. To start over: rm -rf <path>
   ```
   Exit code 1.
3. If `path` exists but `config.toml` doesn't (or has no `[store] hierarchy`): proceed. This is the bootstrap-completion case.

---

## 3. Interactive Flow (prompt-by-prompt)

The five visual moments correspond directly to the v3 mockup at `.superpowers/brainstorm/2840909-1775613724/content/rich-questionary-mockup-v3.html`. Reference that file for the visual reference; the descriptions below are normative.

### Screen 1 — Welcome banner

A `rich.Panel` rendered once at the top of the flow:

- Title: `extract init` (cyan, bold)
- Border: rounded, magenta
- Body:
  ```
  Welcome to Extract.

  Local-first experiment tracking for deep learning.
  This wizard will set up a fresh .extract/ store in
  the current directory.
  ```

Always shown in interactive mode. Suppressed in non-interactive (`--hierarchy`) mode along with screens 2, 3, and the confirmation prompts.

### Screen 2 — Hierarchy preset picker (`questionary.select`)

Skipped if `args.hierarchy is not None`. Five options in this exact order:

```
❯ benchmark › model › variant       — general (recommended)
  dataset › model › seed              — multi-seed sweeps
  dataset › architecture › optimizer  — architecture comparison
  project › experiment                — minimal two-level
  ✎ Define my own                     — type a custom hierarchy
```

The first option is the default (cursor starts there). Arrow keys + Enter to select. Esc raises `KeyboardInterrupt` → exit code 130.

If the user picks one of the four preset options, the levels are extracted directly (`"benchmark › model › variant"` → `["benchmark", "model", "variant"]`) and the flow advances to Screen 4a (skipping Screen 3).

If the user picks "Define my own", advance to Screen 3.

### Screen 3 — Custom hierarchy text input (`questionary.text` with live validation)

Prompt: `Hierarchy levels (separated by ' > '):`

The validator (`questionary.text(..., validate=...)`) runs on every keystroke:

1. Calls `_parse_hierarchy(input_str)` from `python/src/extract/store.py:138-143` (reused, not duplicated). If it raises `ValueError`, return the error message inline.
2. For each parsed level, check `LEVEL_NAME_RE.match(level)` where `LEVEL_NAME_RE = re.compile(r"^[a-z][a-z0-9_]*$")`. On failure, return:
   ```
   '{level}' must match [a-z][a-z0-9_]* — try '{snake_cased(level)}'
   ```
   The `snake_cased` helper lowercases, replaces hyphens and spaces with underscores, strips other punctuation. Suggestion is best-effort; user is free to retype.
3. For each level, check `level not in RESERVED_NAMES` where:
   ```python
   RESERVED_NAMES = frozenset({
       "models", "artifacts", "extract", "config",
       "id", "path", "name", "parent_id", "node_type",
       "created_at", "metadata", "status",
   })
   ```
   On failure, return: `'{level}' is a reserved name — pick something else`.

The user cannot submit until input is valid. On valid submit, advance to Screen 4a.

### Screen 4a — Gitignore prompt (`questionary.confirm`, default Yes)

Skipped if `--no-gitignore` was passed.

Detection: walk up from `path.parent` (the parent of the store directory, not the store directory itself), looking for a directory containing `.git/`. If none found within 32 levels, treat as "not in a git repo" and skip the prompt entirely. If found, prompt:

```
? Add .extract/ to .gitignore? (detected git repo at <gitroot>) (Y/n)
```

If user says yes, set a flag to be acted on in the write phase. If no, set the flag false. Either way, advance to Screen 4b.

### Screen 4b — Config preview panel (`rich.Panel` containing `rich.Syntax`)

Render the exact bytes of the `config.toml` file we're about to write, inside a blue-bordered panel titled `Preview: <path>/config.toml`. Use `rich.Syntax(content, lexer="toml", theme="ansi_dark")`.

Then prompt: `? Write this config? (Y/n)` (default Yes).

If user says no, exit cleanly with no changes:

```
Aborted. No files written.
```

Exit code 0.

If yes, advance to Screen 5.

### Screen 5 — Write phase

No prompts. Three potential status lines, each printed only if the corresponding action actually executed:

```
✓ Created .extract/                                           # if path didn't exist before
✓ Using .extract/                                             # if path already existed
✓ Wrote .extract/config.toml (hierarchy: benchmark › model › variant)
✓ Updated .gitignore                                          # only if gitignore was actually modified
```

The `✓` is green (`ansigreen`). Path names are cyan. Hierarchy text uses `›` (U+203A) as the visual separator (the `>` in TOML is the on-disk format; `›` is purely cosmetic in the status line).

### Screen 6 — Success / quickstart panel

A green-bordered `rich.Panel` titled `Quickstart`, containing a `rich.Syntax(snippet, lexer="python", theme="ansi_dark")`. The snippet is templated from `QUICKSTART_TEMPLATE` with the chosen hierarchy substituted in:

```python
from extract import Store

store = Store()
exp = store.experiment({
    "benchmark": "imagenet",
    "model":     "resnet50",
    "variant":   "lr_0.01",
})
with exp.run(config={"lr": 0.01}) as run:
    run.log(step=0, loss=2.3, accuracy=0.1)

# Browse with: extract tui
```

The dict keys come from the chosen hierarchy. The values are filled from a small lookup table keyed by level name:

| Level name | Sample value |
|---|---|
| `benchmark` / `dataset` / `task` / `project` | `"imagenet"` |
| `model` / `architecture` / `experiment` | `"resnet50"` |
| `variant` / `config` / `seed` / `optimizer` | `"lr_0.01"` |
| anything else | the level name itself, e.g. `"foo": "foo_value"` |

The fallback ensures any custom hierarchy still produces a runnable snippet.

---

## 4. Files Written

### `<path>/config.toml`

The full commented-defaults version (Q6=C):

```toml
# Extract store config. See DOC.md for all options.

[store]
hierarchy = "benchmark > model > variant"

# [summary]
# max_recent_runs = 5
# show_aggregates = true

# [compare]
# default_metric = "accuracy"
# lower_is_better = ["loss", "error"]

# [theme]
# accent = "cyan"
```

The exact list of commented sections must match what `rust/src/config.rs` actually parses. The implementer should read `rust/src/config.rs` and generate the commented sections from the actual default values, not invent new keys. If `rust/src/config.rs` defines `[notifications]`, `[reload]`, etc., those must be included as commented sections too. Comments are explanatory only; uncommenting them MUST produce a valid config that `extract-tui` accepts without error.

### `<gitroot>/.gitignore`

Append a single line: `.extract/`. Behavior:

- If `.gitignore` doesn't exist, create it with just `.extract/\n`.
- If it exists, append `.extract/\n` only if it's not already present. "Present" is checked line-by-line, stripping whitespace and trailing slashes for comparison: `.extract`, `.extract/`, `  .extract/  ` all count as already-present.
- The append preserves the user's existing gitignore intact (no reformatting, no rewriting, no comment additions). Just one line at the end.

### Files NOT written

- No `quickstart.py` (Q8=A: print-only).
- No `extract.db` — the database is created by the first `Store(path)` call, not by `init`. Running `extract init && extract tui` opens an empty store, which the TUI handles correctly.
- No models, artifacts, or any other directories under `<path>/`. Those are created lazily by `Store.__init__` on first use.

---

## 5. SDK Behavior Changes

### `python/src/extract/store.py`

**New exception class** (top of module, after imports):

```python
class MissingHierarchyError(Exception):
    """Raised when Store() is opened against a path with no configured hierarchy."""
```

**`Store.__init__` hard requirement.** After the existing block at lines 173-186 that loads config_hierarchy and db_hierarchy, replace the assignment `self._hierarchy = config_hierarchy or db_hierarchy` with:

```python
if not config_hierarchy:
    raise MissingHierarchyError(
        f"No config.toml with [store] hierarchy found at {self.root}/config.toml. "
        f"Run `extract init` in this directory to set up the store."
    )

# Sanity-check that the DB hierarchy matches (existing check)
if db_hierarchy and config_hierarchy != db_hierarchy:
    raise ValueError(
        f"Hierarchy mismatch: config.toml has {' > '.join(config_hierarchy)} "
        f"but DB has {' > '.join(db_hierarchy)}"
    )

# Persist to DB if not already there (existing behavior)
if not db_hierarchy:
    self._save_hierarchy(config_hierarchy)

self._hierarchy = config_hierarchy
```

The DB-first read path is gone — `config.toml` is the only source of truth; the DB `hierarchy` table is now strictly a write-through cache.

**Remove the schema migration** at lines 165-169:

```python
# DELETE THESE LINES:
try:
    self._conn.execute("ALTER TABLE experiments ADD COLUMN node_type TEXT")
except sqlite3.OperationalError:
    pass  # Column already exists
```

The column has been in the schema for a while; existing populated stores already have it. Fresh stores get it via the schema script. Drop the runtime migration entirely.

**Update `_SCHEMA` constant** at line 31:

```python
# BEFORE:
node_type   TEXT
# AFTER:
node_type   TEXT NOT NULL
```

**Simplify `experiment()`** at lines 223-233:

```python
# BEFORE:
def experiment(self, spec: dict[str, str] | str) -> Experiment:
    """..."""
    if isinstance(spec, str):
        return self._experiment_by_path(spec)
    return self._experiment_by_dict(spec)

# AFTER:
def experiment(self, spec: dict[str, str]) -> Experiment:
    """Create or get an experiment from a hierarchy-keyed dict.

    Args:
        spec: Dict mapping hierarchy levels to values, e.g.
              {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}.
              Keys must be hierarchy levels; values cannot skip levels.
    """
    return self._experiment_by_dict(spec)
```

(The dispatcher is gone; `experiment()` is now a thin wrapper around the dict implementation. Optionally, the implementation can be inlined and `_experiment_by_dict` deleted — implementer's choice during code review.)

**Delete `_experiment_by_path`** entirely (lines 235-267).

**Drop the docstring reference** in `_experiment_by_dict`'s docstring at line 229: remove `"or a plain path string (legacy mode, no node_type)"`.

### `schema/migrations/001_init.sql`

Line 13: `node_type TEXT` → `node_type TEXT NOT NULL`.

### `python/src/extract/mcp.py`

**Drop the conditional import** at lines 16-19:

```python
# BEFORE:
try:
    from mcp.server.fastmcp import FastMCP
except ImportError:
    FastMCP = None  # type: ignore[assignment,misc]
# AFTER:
from mcp.server.fastmcp import FastMCP
```

**Simplify the server initialization** at line 25:

```python
# BEFORE:
mcp_server: Any = FastMCP("extract") if FastMCP else None
# AFTER:
mcp_server = FastMCP("extract")
```

(Type annotation `Any` can be dropped — `mcp_server` is now non-optional.)

**Simplify the `_tool` decorator** at lines 28-36:

```python
# BEFORE:
def _tool(fn):
    """Register a function with the FastMCP server if it's available.

    When `mcp` isn't installed, this is a no-op — the function is still
    defined at module level so unit tests can call it directly.
    """
    if mcp_server is not None:
        return mcp_server.tool()(fn)
    return fn

# AFTER:
def _tool(fn):
    """Register a function with the FastMCP server."""
    return mcp_server.tool()(fn)
```

---

## 6. Dependency Manifest

### `pyproject.toml` diff

```toml
[project]
name = "extract-tracker"
version = "0.1.0"
description = "Extract: local-first experiment tracking for deep learning"
requires-python = ">=3.10"
dependencies = [
    "python-ulid>=3.0",
    "numpy>=1.21",
    "typing-extensions>=4.15.0",
    "tomli>=2.0; python_version < '3.11'",
    "mcp>=1.0",                # MOVED from optional-dependencies
    "rich>=13.0",              # NEW
    "questionary>=2.0",        # NEW
]

# DELETED ENTIRELY:
# [project.optional-dependencies]
# mcp = ["mcp>=1.0"]

[project.scripts]
extract = "extract.__main__:main"
```

The package no longer has any optional extras. `pip install extract-tracker` installs everything needed for `extract init`, `extract tui`, `extract sync`, `python -m extract.mcp`, and the SDK.

### Documentation

`README.md` and `DOC.md` install instructions update from `pip install 'extract-tracker[mcp]'` (where it appears for the MCP server section) to `pip install extract-tracker`.

---

## 7. Code Organization

### New files

```
python/src/extract/init.py         # ~150 LOC, single module, no submodules
python/tests/test_init.py          # ~250 LOC, full coverage
```

### `python/src/extract/init.py` structure

```python
"""extract init: interactive .extract/ store bootstrapper."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from rich.console import Console
from rich.panel import Panel
from rich.syntax import Syntax
from rich.text import Text
import questionary

from extract.store import _parse_hierarchy  # reuse, don't duplicate

# ──────────────────────────────────────────────────────────────────────────
# Constants

LEVEL_NAME_RE = re.compile(r"^[a-z][a-z0-9_]*$")

RESERVED_NAMES = frozenset({
    "models", "artifacts", "extract", "config",
    "id", "path", "name", "parent_id", "node_type",
    "created_at", "metadata", "status",
})

PRESETS: list[tuple[str, str]] = [
    ("benchmark > model > variant", "general (recommended)"),
    ("dataset > model > seed", "multi-seed sweeps"),
    ("dataset > architecture > optimizer", "architecture comparison"),
    ("project > experiment", "minimal two-level"),
]

# Sample values for the quickstart snippet, keyed by level name
SAMPLE_VALUES: dict[str, str] = {
    "benchmark": "imagenet",
    "dataset": "imagenet",
    "task": "imagenet",
    "project": "imagenet",
    "model": "resnet50",
    "architecture": "resnet50",
    "experiment": "resnet50",
    "variant": "lr_0.01",
    "config": "lr_0.01",
    "seed": "42",
    "optimizer": "adam",
}

CONFIG_TEMPLATE = """\
# Extract store config. See DOC.md for all options.

[store]
hierarchy = "{hierarchy}"

# [summary]
# max_recent_runs = 5
# show_aggregates = true

# [compare]
# default_metric = "accuracy"
# lower_is_better = ["loss", "error"]

# [theme]
# accent = "cyan"
"""

# Style dict for questionary — ANSI-named colors only
QUESTIONARY_STYLE = questionary.Style.from_dict({
    "qmark":     "ansigreen bold",
    "question":  "bold",
    "answer":    "ansicyan bold",
    "pointer":   "ansicyan bold",
    "highlighted": "ansicyan bold",
    "selected":  "ansicyan",
    "instruction": "",
})


# ──────────────────────────────────────────────────────────────────────────
# Custom exception

class ConfigExistsError(Exception):
    """Raised by _preflight when path already has a configured store."""


# ──────────────────────────────────────────────────────────────────────────
# Public entry point (called from extract.__main__)

def run(args: argparse.Namespace) -> int:
    """Execute extract init. Returns the exit code."""
    ...


# ──────────────────────────────────────────────────────────────────────────
# Validation (reused by interactive validator AND --hierarchy flag parser)

def validate_hierarchy_levels(levels: list[str]) -> None:
    """Raise ValueError with a friendly message if any level is invalid."""
    ...

def _snake_case(s: str) -> str:
    """Best-effort sanitization of an invalid level name into a valid suggestion."""
    ...


# ──────────────────────────────────────────────────────────────────────────
# Pre-flight

def _preflight(path: Path) -> None:
    """Refuse if path already has a configured store. Raises ConfigExistsError."""
    ...


# ──────────────────────────────────────────────────────────────────────────
# Interactive screens

def _pick_hierarchy_interactive() -> list[str]:
    """Run screens 2 and 3. Returns the chosen hierarchy levels."""
    ...

def _confirm_gitignore(git_root: Path) -> bool:
    """Screen 4a. Returns True if the user wants to add .extract/ to .gitignore."""
    ...

def _confirm_write_config(console: Console, path: Path, content: str) -> bool:
    """Screen 4b. Renders the preview panel and confirms. Returns True to proceed."""
    ...


# ──────────────────────────────────────────────────────────────────────────
# Side-effect functions

def _write_config(path: Path, levels: list[str]) -> bool:
    """Create path if needed, write config.toml. Returns True if path was newly created."""
    ...

def _update_gitignore(git_root: Path) -> bool:
    """Append .extract/ to .gitignore if not present. Returns True if modified."""
    ...

def _find_git_root(start: Path) -> Path | None:
    """Walk up from start looking for a directory containing .git/. Returns the
    containing directory or None."""
    ...


# ──────────────────────────────────────────────────────────────────────────
# Rich rendering

def _render_welcome(console: Console) -> None:
    """Screen 1. Welcome banner Panel."""
    ...

def _render_status_lines(console: Console, path: Path, levels: list[str],
                         path_was_created: bool, gitignore_modified: bool) -> None:
    """Screen 5. ✓ Created … / ✓ Wrote … / ✓ Updated …"""
    ...

def _render_quickstart(console: Console, levels: list[str]) -> None:
    """Screen 6. Green Panel containing a syntax-highlighted Python snippet."""
    ...

def _build_quickstart_snippet(levels: list[str]) -> str:
    """Render QUICKSTART_TEMPLATE with sample values for each level."""
    ...
```

`run(args)` is the orchestrator. The implementer should write it as a clear linear sequence: preflight → welcome → hierarchy → gitignore → preview confirm → write phase → success panel, with explicit error handling at each boundary.

### Modified files

| File | Change |
|---|---|
| `python/src/extract/__main__.py` | Add `init` subparser with positional `path`, `--hierarchy`, `--no-gitignore`. Dispatch to `extract.init.run(args)`. Pattern: `from extract import init; sys.exit(init.run(args))` inside the `elif args.command == "init":` branch. |
| `python/src/extract/store.py` | All changes from §5: add `MissingHierarchyError`, hard requirement in `__init__`, drop `_experiment_by_path`, simplify `experiment()`, drop `ALTER TABLE` migration, `node_type TEXT NOT NULL` in `_SCHEMA`. |
| `python/src/extract/mcp.py` | Drop conditional import and `_tool` guard per §5. |
| `schema/migrations/001_init.sql` | `node_type TEXT NOT NULL` on line 13. |
| `pyproject.toml` | Bundle deps per §6. |
| `python/tests/test_hierarchy.py` | Delete the legacy string-path test at lines 41-43. Verify all other tests still pass against the hard-required `Store()`. Scrub CL vocab (`benchmark > method > variant` → `benchmark > model > variant`, etc.). |
| `python/tests/test_mcp.py` | Verify against bundled `mcp` (no `[mcp]` extra). Scrub CL vocab. |
| `scripts/generate_test_data.py` | Scrub CL vocab. Generate test data with `imagenet`/`cifar10`/`resnet50`/`vit_base` and `lr_*` variants. |
| `README.md` | Quick Start: `pip install extract-tracker && extract init`. Remove the manual `config.toml` creation step. Scrub CL examples. |
| `DOC.md` | Add an `extract init [path] [--hierarchy ...] [--no-gitignore]` row in the CLI section. Remove the entire "String spec (legacy)" sub-bullet at lines 29-32. Update `store.experiment()` signature to `dict[str, str]` only at line 23. Scrub CL examples. |

---

## 8. Testing Strategy

All tests live in `python/tests/test_init.py` except for SDK regression tests (which go in `python/tests/test_hierarchy.py` alongside the existing hierarchy tests).

### 8.1 Pure-function unit tests (no I/O)

| Test | Asserts |
|---|---|
| `test_validate_levels_valid` | `validate_hierarchy_levels(["benchmark", "model", "variant"])` returns without raising |
| `test_validate_levels_uppercase_rejected` | `["My-Model"]` raises `ValueError` matching `"must match"` |
| `test_validate_levels_leading_digit_rejected` | `["1foo"]` raises `ValueError` |
| `test_validate_levels_hyphen_rejected` | `["foo-bar"]` raises `ValueError` |
| `test_validate_levels_empty_rejected` | `[""]` raises `ValueError` |
| `test_validate_levels_unicode_rejected` | `["αβγ"]` raises `ValueError` |
| `test_validate_levels_each_reserved_name_rejected` | parametrize over all 11 reserved names; each raises `ValueError` matching `"reserved"` |
| `test_snake_case_basic` | `_snake_case("My-Model")` returns `"my_model"` |
| `test_snake_case_spaces_to_underscores` | `_snake_case("foo bar")` returns `"foo_bar"` |
| `test_snake_case_strips_punctuation` | `_snake_case("foo!")` returns `"foo"` |
| `test_build_quickstart_snippet_known_keys` | `_build_quickstart_snippet(["benchmark", "model", "variant"])` contains `"imagenet"`, `"resnet50"`, `"lr_0.01"` |
| `test_build_quickstart_snippet_unknown_key_fallback` | `_build_quickstart_snippet(["foo", "bar"])` contains `"foo": "foo_value"` and `"bar": "bar_value"` |

### 8.2 Filesystem unit tests (`tmp_path`, no prompts)

| Test | Asserts |
|---|---|
| `test_find_git_root_finds_parent` | `tmp_path/.git/` exists → returns `tmp_path` |
| `test_find_git_root_finds_grandparent` | `tmp_path/.git/`, `tmp_path/sub/store/` → finds `tmp_path` from `sub/store/` |
| `test_find_git_root_returns_none_no_repo` | `tmp_path` has no `.git` → returns `None` |
| `test_preflight_passes_on_nonexistent_path` | `tmp_path/.extract/` doesn't exist → no exception |
| `test_preflight_passes_on_empty_dir` | `tmp_path/.extract/` exists but empty → no exception |
| `test_preflight_passes_when_dir_exists_but_no_config` | dir exists with random files but no `config.toml` → no exception |
| `test_preflight_refuses_existing_configured_store` | pre-create `tmp_path/.extract/config.toml` with `[store] hierarchy` → raises `ConfigExistsError` |
| `test_preflight_passes_with_malformed_config` | pre-create `config.toml` without `[store]` section → no exception (proceeds to overwrite) |
| `test_write_config_creates_file_with_hierarchy` | call `_write_config(tmp_path, ["a", "b"])`; read back; assert TOML contains `hierarchy = "a > b"` and the commented sections |
| `test_write_config_round_trips_via_store` | call `_write_config`; then `Store(tmp_path)._hierarchy == ["a", "b"]` (verifies SDK can open what `init` writes) |
| `test_update_gitignore_creates_new_file` | `tmp_path` no `.gitignore` → file created containing exactly `.extract/\n` |
| `test_update_gitignore_appends_when_missing` | pre-existing `.gitignore` with `*.pyc\n` → appended; file now contains both `*.pyc` and `.extract/`; preserves the newline before |
| `test_update_gitignore_idempotent_when_present` | pre-existing `.gitignore` already contains `.extract/` → no change, returns `False` |
| `test_update_gitignore_handles_trailing_whitespace` | line `.extract/  \n` counts as already-present |
| `test_update_gitignore_handles_no_trailing_slash` | line `.extract\n` counts as already-present |

### 8.3 Non-interactive end-to-end tests

These use `init.run(args)` directly with a `Namespace` constructed in the test, no `argparse` involved, no prompt mocking.

| Test | Asserts |
|---|---|
| `test_run_noninteractive_happy_path` | `args = Namespace(path=tmp_path, hierarchy="benchmark > model > variant", no_gitignore=True)`; `run(args) == 0`; `tmp_path/config.toml` exists and `Store(tmp_path)` opens cleanly |
| `test_run_noninteractive_invalid_hierarchy_flag` | `--hierarchy "Foo > Bar"` → `run(args)` returns 1; no files written |
| `test_run_noninteractive_invalid_hierarchy_reserved` | `--hierarchy "models > foo"` → `run(args)` returns 1; no files written |
| `test_run_noninteractive_existing_configured_store` | pre-create `config.toml` with hierarchy; `run(args)` returns 1; original config untouched |
| `test_run_noninteractive_completes_bootstrap` | pre-create empty `tmp_path/.extract/` dir; `run(args)` returns 0; config.toml written |
| `test_run_noninteractive_creates_gitignore_when_git_repo` | `tmp_path/.git/` exists; `args.no_gitignore=False`; run; `.gitignore` contains `.extract/` |
| `test_run_noninteractive_skips_gitignore_when_no_repo` | no `.git/`; default args; no `.gitignore` written |
| `test_run_noninteractive_skips_gitignore_with_flag` | `.git/` present, `no_gitignore=True` → no gitignore changes |
| `test_run_tty_check_errors_without_hierarchy` | monkeypatch `sys.stdin.isatty` to return False; no `--hierarchy` → returns exit code 2 |

### 8.4 Interactive flow tests (mock `questionary`)

`monkeypatch` the `questionary.select`, `questionary.text`, and `questionary.confirm` factories at the module level. Each returns a stub object whose `.ask()` method returns the canned value.

| Test | Asserts |
|---|---|
| `test_interactive_picks_preset` | mock `questionary.select` to return `"benchmark > model > variant — general (recommended)"`; mock confirms to True; `run` writes config; assert hierarchy in file |
| `test_interactive_picks_custom_then_validates` | mock select → `"✎ Define my own — type a custom hierarchy"`; mock text → `"task > model > seed"`; assert validator was called with that string and returned True; assert config written |
| `test_interactive_user_rejects_preview` | mock confirms to return False on the "Write this config?" step; assert no files written, exit code 0, output contains "Aborted" |
| `test_interactive_keyboard_interrupt_in_select` | mock `questionary.select(...).ask()` to raise `KeyboardInterrupt`; assert exit code 130, no files written |
| `test_interactive_keyboard_interrupt_in_confirm` | same for the confirm step |
| `test_interactive_gitignore_yes` | mock select, text, all confirms to yes; pre-create `tmp_path/.git/`; assert `.gitignore` modified |
| `test_interactive_gitignore_no` | same but mock the gitignore confirm to False; assert no `.gitignore` modification |

### 8.5 SDK behavior regression tests (in `test_hierarchy.py`)

| Test | Asserts |
|---|---|
| `test_store_raises_without_config` | `Store(tmp_path)` where tmp_path has no `config.toml` → raises `MissingHierarchyError`; error message contains the absolute path |
| `test_store_raises_with_empty_config` | write `config.toml` with `[store]` but no `hierarchy` key → raises `MissingHierarchyError` |
| `test_store_raises_with_no_store_section` | write `config.toml` with only `[other]` section → raises `MissingHierarchyError` |
| `test_store_opens_with_valid_config` | write minimal valid config → `Store(tmp_path)` succeeds and `_hierarchy == ["a", "b", "c"]` |
| `test_legacy_path_string_api_removed` | with a valid Store, `with pytest.raises((TypeError, AttributeError)): store.experiment("foo/bar")` |
| `test_experiment_by_path_method_gone` | `assert not hasattr(Store, "_experiment_by_path")` |
| `test_alter_table_migration_gone` | grep `python/src/extract/store.py` for `"ADD COLUMN node_type"` and assert no match |

### 8.6 Theme inheritance regression test

```python
def test_no_hex_colors_in_init_module():
    """Theme inheritance: rich/questionary must use ANSI-named colors only."""
    src = Path(__file__).parent.parent / "src" / "extract" / "init.py"
    content = src.read_text()
    # Match "#RRGGBB" and "#RRGGBBAA" inside string literals
    hex_pattern = re.compile(r'["\']#[0-9a-fA-F]{6,8}["\']')
    matches = hex_pattern.findall(content)
    assert not matches, f"hex color literals found in init.py: {matches}"
```

### 8.7 MCP cleanup smoke test (in `test_mcp.py` or new module)

```python
def test_mcp_module_loads_without_optional_extra():
    """After bundling, mcp.py must import cleanly without conditional checks."""
    from extract import mcp as mcp_mod
    # mcp_server is no longer Optional after the cleanup
    assert mcp_mod.mcp_server is not None
    assert mcp_mod.FastMCP is not None
```

### Coverage gaps explicitly NOT tested

- Visual rendering of `rich.Panel` / `rich.Syntax` output bytes — too brittle; we trust the libraries.
- `questionary`'s actual TTY rendering and arrow-key handling — same; we trust the library and only test our integration via mocks.
- Cross-platform `.gitignore` line endings — single platform (Linux) per project conventions.
- Multi-process race conditions (two `extract init` invocations against the same path simultaneously) — out of scope.

---

## 9. Implementation Notes

- **Reuse `_parse_hierarchy` from `store.py`** for the hierarchy splitting in both `validate_hierarchy_levels` and the `--hierarchy` flag parser. Do not duplicate the parsing logic.
- **Keep `init.py` self-contained.** The only `extract.*` import is `_parse_hierarchy`. No circular imports back into `store.py` for anything else.
- **The `console` object is constructed once in `run(args)`** with `Console()` (no special flags — let `rich` auto-detect TTY, color depth, terminal width). Pass it down to the rendering helpers; do not create multiple `Console` instances.
- **Read `rust/src/config.rs` before writing `CONFIG_TEMPLATE`.** The commented sections must reflect the actual config schema, not invented defaults. Sections to inspect at minimum: `[summary]`, `[compare]`, `[theme]`, `[notifications]` (if present), `[reload]` (if present).
- **Implement `__main__.py` dispatch defensively.** The argparse `init` subparser should be added in `main()` next to the existing `tui` and `sync` subparsers. The dispatch branch:
  ```python
  elif args.command == "init":
      from extract import init  # lazy import — pulls in rich/questionary only when needed
      sys.exit(init.run(args))
  ```
  Lazy import keeps `extract tui` and `extract sync` invocations from paying the `rich`/`questionary` import cost.
- **`KeyboardInterrupt` handling** lives in `run(args)`, not in each helper. Wrap the orchestration body in `try/except KeyboardInterrupt` and return 130. Helpers can let `KeyboardInterrupt` propagate.

---

## 10. Open Questions Resolved

These are decisions reached during the brainstorming session, captured here so the implementer doesn't relitigate them:

| # | Question | Decision |
|---|---|---|
| Q1 | What if `.extract/` already exists? | Smart bootstrap: complete the bootstrap if no `config.toml`; refuse if config exists with hierarchy. No `--force`. |
| Q2 | Should `Store()` require `extract init` first? | Yes — hard requirement. Legacy path-string API removed. Clean break. |
| Q3 | Migration policy for existing stores? | None. Single rule: `config.toml` required, period. No state-(b) auto-migration. State-(c) recovery is `rm -rf` and re-init. |
| Q4 | Non-interactive mode shape? | `--hierarchy "a > b > c"` flag + TTY enforcement. No `--yes` (nothing to skip). |
| Q5 | Hierarchy validation strictness? | Strict `^[a-z][a-z0-9_]*$` per level + reserved-name blocklist (11 names). |
| Q6 | Config seeding scope? | Full commented defaults — `[store]` is uncommented; `[summary]`, `[compare]`, `[theme]`, etc. are commented placeholders. |
| Q7 | `.gitignore` integration? | Always offer interactive prompt (default Yes). Skip if `--no-gitignore` or no `.git/` parent. Append `.extract/` to whole-directory ignore. |
| Q8 | Quickstart snippet location? | `rich.Panel` printed to stdout. No `quickstart.py` file. |
| — | gum vs rich+questionary? | `rich + questionary`. Rich earns its weight beyond `init` (sync output, MCP stderr). Theme inheritance via ANSI-named colors. |
| — | Recommended hierarchy preset? | `benchmark > model > variant` (general, recommended). Other presets: `dataset > model > seed`, `dataset > architecture > optimizer`, `project > experiment`. |
| — | CL vocab scrub scope? | User-visible files only (`README.md`, `DOC.md`, `scripts/generate_test_data.py`, `python/tests/test_*.py`). Historical specs/plans/briefs untouched. |
| — | Bundle vs optional extras for new deps? | Bundle. `pip install extract-tracker` installs everything. `[project.optional-dependencies]` section deleted. |

---

## 11. Visual Reference

The interactive flow's expected appearance is captured in:

```
.superpowers/brainstorm/2840909-1775613724/content/rich-questionary-mockup-v3.html
```

This file shows all five visual moments (welcome banner, preset picker, custom-entry validation, preview panel, success/quickstart panel) with the recommended preset and ANSI colors approximated. The actual implementation will inherit the user's terminal theme, so colors may differ visually from the mockup — but the structure and layout are normative.
