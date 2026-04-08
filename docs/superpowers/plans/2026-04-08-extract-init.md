# `extract init` & SDK Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an interactive `extract init` CLI command (built on `rich` + `questionary`) that bootstraps `.extract/config.toml`, harden `Store()` to require it, drop the legacy path-string API, and bundle `mcp`/`rich`/`questionary` into base dependencies.

**Architecture:** New self-contained `extract.init` module (one file) holding all CLI logic. `Store.__init__` raises `MissingHierarchyError` when `config.toml` is missing or has no `[store] hierarchy`. The legacy path-string API and the runtime `ALTER TABLE` migration are deleted. The Rust TUI side stays permissive (`Option<String>` on `node_type`) so users can still browse legacy stores.

**Tech Stack:** Python 3.10+, `rich>=13.0`, `questionary>=2.0`, `mcp>=1.0` (all as base deps), SQLite via existing `Store`. Tests run via `nix develop --command bash -c 'uv run pytest ...'`. The full reference design is at `docs/superpowers/specs/2026-04-08-extract-init-design.md`.

**Important conventions:**
- All shell commands run inside `nix develop` per `CLAUDE.md`. Use `nix develop --command bash -c '<cmd>'` from the project root.
- Tests via `uv run pytest <path>` (not bare `pytest`).
- Theme inheritance is a hard constraint: ANSI-named colors only in `init.py`. NO hex/RGB. Tests enforce this via grep.
- All commits use the `Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>` trailer.

---

## Phase 1 — Dependency manifest

### Task 1: Bundle MCP, rich, and questionary into base dependencies

**Files:**
- Modify: `pyproject.toml`

- [ ] **Step 1: Read current `pyproject.toml`**

Run:
```bash
cat pyproject.toml
```

Expected current state (verify before editing):
```toml
[project]
dependencies = [
    "python-ulid>=3.0",
    "numpy>=1.21",
    "typing-extensions>=4.15.0",
    "tomli>=2.0; python_version < '3.11'",
]

[project.optional-dependencies]
mcp = ["mcp>=1.0"]
```

- [ ] **Step 2: Replace the dependency block**

Edit `pyproject.toml`. Change the `dependencies` array to:
```toml
dependencies = [
    "python-ulid>=3.0",
    "numpy>=1.21",
    "typing-extensions>=4.15.0",
    "tomli>=2.0; python_version < '3.11'",
    "mcp>=1.0",
    "rich>=13.0",
    "questionary>=2.0",
]
```

And **delete the entire `[project.optional-dependencies]` section** (the two lines `[project.optional-dependencies]` and `mcp = ["mcp>=1.0"]`).

- [ ] **Step 3: Sync the lockfile**

Run:
```bash
nix develop --command bash -c 'uv sync'
```
Expected: completes with no errors. Adds `rich`, `questionary`, and their transitive deps (`prompt-toolkit`, `wcwidth`, `markdown-it-py`, `pygments`, etc.) to `uv.lock`.

- [ ] **Step 4: Verify the new packages import**

Run:
```bash
nix develop --command bash -c 'uv run python -c "import rich, questionary, mcp; print(rich.__version__, questionary.__version__)"'
```
Expected: prints version strings (something like `13.x.x 2.x.x`), no `ImportError`.

- [ ] **Step 5: Commit**

```bash
git add pyproject.toml uv.lock
git commit -m "$(cat <<'EOF'
build: bundle mcp, rich, questionary into base dependencies

Drops the [mcp] optional extra. Adds rich (panels, syntax highlighting,
status output) and questionary (interactive prompts) as base deps so
that pip install extract-tracker provides the full extract init UX.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Drop the conditional MCP import

**Files:**
- Modify: `python/src/extract/mcp.py:14-36`

- [ ] **Step 1: Read the current top of `mcp.py`**

Run:
```bash
nix develop --command bash -c 'sed -n "14,40p" python/src/extract/mcp.py'
```

Expected current state:
```python
from typing import Any

try:
    from mcp.server.fastmcp import FastMCP
except ImportError:
    FastMCP = None  # type: ignore[assignment,misc]

from extract.store import Store

# Module-level state. Set by main() at startup; monkey-patched by tests.
_store: Store | None = None
mcp_server: Any = FastMCP("extract") if FastMCP else None


def _tool(fn):
    """Register a function with the FastMCP server if it's available.

    When `mcp` isn't installed, this is a no-op — the function is still
    defined at module level so unit tests can call it directly.
    """
    if mcp_server is not None:
        return mcp_server.tool()(fn)
    return fn
```

- [ ] **Step 2: Replace lines 14-36 with hard imports and a one-liner decorator**

Use Edit to replace the `try`/`except` block, the `mcp_server` line, and the conditional `_tool` decorator with:

```python
from typing import Any

from mcp.server.fastmcp import FastMCP

from extract.store import Store

# Module-level state. Set by main() at startup; monkey-patched by tests.
_store: Store | None = None
mcp_server = FastMCP("extract")


def _tool(fn):
    """Register a function with the FastMCP server."""
    return mcp_server.tool()(fn)
```

- [ ] **Step 3: Verify the module still imports**

Run:
```bash
nix develop --command bash -c 'uv run python -c "from extract import mcp; assert mcp.mcp_server is not None; assert mcp.FastMCP is not None; print(\"ok\")"'
```
Expected: prints `ok`.

- [ ] **Step 4: Run the existing MCP test suite to make sure nothing regressed**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_mcp.py -q 2>&1 | tail -10'
```
Expected: tests collect and either pass or show pre-existing failures unrelated to this change. If you see `ImportError` or `NameError` referencing `FastMCP`, your edit was wrong — re-check.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/mcp.py
git commit -m "$(cat <<'EOF'
refactor(mcp): drop conditional FastMCP import

mcp is now a base dependency (see previous commit), so the
try/except ImportError fallback and the conditional _tool decorator
are dead code. Hard-import FastMCP and simplify _tool to a one-liner.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 2 — `init.py` skeleton and pure functions (TDD)

### Task 3: Create the init module skeleton

**Files:**
- Create: `python/src/extract/init.py`

- [ ] **Step 1: Verify the file does not yet exist**

Run:
```bash
ls python/src/extract/init.py 2>&1
```
Expected: `ls: cannot access 'python/src/extract/init.py': No such file or directory`

- [ ] **Step 2: Write the skeleton module with all constants and stubs**

Create `python/src/extract/init.py` with this exact content:

```python
"""extract init: interactive .extract/ store bootstrapper.

Builds the welcome wizard on top of rich (panels, syntax highlighting,
status output) and questionary (interactive prompts). All colors must
be ANSI-named so the user's terminal theme applies. NO hex/RGB.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# ──────────────────────────────────────────────────────────────────────────
# Constants

LEVEL_NAME_RE = re.compile(r"^[a-z][a-z0-9_]*$")

RESERVED_NAMES = frozenset({
    "models", "artifacts", "extract", "config",
    "id", "path", "name", "parent_id", "node_type",
    "created_at", "metadata", "status",
})

# (hierarchy_string, label) pairs shown in the preset picker, in order.
PRESETS: list[tuple[str, str]] = [
    ("benchmark > model > variant", "general (recommended)"),
    ("dataset > model > seed", "multi-seed sweeps"),
    ("dataset > architecture > optimizer", "architecture comparison"),
    ("project > experiment", "minimal two-level"),
]

# Sample values for the quickstart snippet, keyed by level name.
# Anything not in this map gets the level name itself as a value
# (e.g. "foo" -> "foo_value") via _build_quickstart_snippet's fallback.
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

# Comments below reflect the actual config schema parsed in
# rust/src/config.rs (Config struct, ~line 183). Sections shown
# here are the seven that the Rust TUI parses today.
CONFIG_TEMPLATE = """\
# Extract store config. See DOC.md for all options.

[store]
hierarchy = "{hierarchy}"

# [summary]
# sections = ["runs", "metrics", "tables", "curves"]
# curve_width = 80
# curve_smooth = false

# [tables]
# # Ordered highlight rules; first match wins.
# # [[tables.highlight]]
# # min = 0.7
# # color = "green"

# [compare]
# sections = ["pivot", "config", "tables", "curves"]
# curve_width = 80

# [notifications]
# timeout = 3

# [theme]
# accent = "cyan"
# success = "green"
# warning = "yellow"
# error = "red"

# [metrics]
# minimize = ["loss", "error"]
# maximize = ["accuracy"]

# [info]
# fields = ["method.*", "task.num_train_epochs"]
"""

QUICKSTART_TEMPLATE = """\
from extract import Store

store = Store()
exp = store.experiment({{
{dict_lines}
}})
with exp.run(config={{"lr": 0.01}}) as run:
    run.log(step=0, loss=2.3, accuracy=0.1)

# Browse with: extract tui
"""


# ──────────────────────────────────────────────────────────────────────────
# Custom exceptions

class ConfigExistsError(Exception):
    """Raised by _preflight when path already has a configured store."""


# ──────────────────────────────────────────────────────────────────────────
# Stubs — implementations land in subsequent tasks
# (All functions are declared up-front so the module always imports cleanly.)

# Pure functions

def validate_hierarchy_levels(levels: list[str]) -> None:
    """Raise ValueError with a friendly message if any level is invalid."""
    raise NotImplementedError


def _snake_case(s: str) -> str:
    """Best-effort sanitization of an invalid level name into a valid suggestion."""
    raise NotImplementedError


def _build_quickstart_snippet(levels: list[str]) -> str:
    """Render QUICKSTART_TEMPLATE with sample values for each level."""
    raise NotImplementedError


def _find_git_root(start: "Path") -> "Path | None":
    """Walk up from `start` looking for a directory containing .git/.
    Returns the containing directory or None if no git repo within 32 levels."""
    raise NotImplementedError


# Filesystem helpers

def _preflight(path: "Path") -> None:
    """Refuse if path already has a configured store. Raises ConfigExistsError."""
    raise NotImplementedError


def _write_config(path: "Path", levels: list[str]) -> bool:
    """Create path if needed, write config.toml. Returns True if path was newly created."""
    raise NotImplementedError


def _update_gitignore(git_root: "Path") -> bool:
    """Append .extract/ to .gitignore if not present. Returns True if modified."""
    raise NotImplementedError


# Interactive prompts (filled in during Phase 6)

def _pick_hierarchy_interactive() -> list[str]:
    """Run screens 2 and 3. Returns the chosen hierarchy levels."""
    raise NotImplementedError


def _confirm_gitignore(git_root: "Path") -> bool:
    """Screen 4a: ask whether to add .extract/ to .gitignore. Default Yes."""
    raise NotImplementedError


def _confirm_write_config(console, path: "Path", levels: list[str]) -> bool:
    """Screen 4b: render preview, ask whether to write. Default Yes."""
    raise NotImplementedError


# Rich rendering (filled in during Phase 6)

def _render_welcome(console) -> None:
    """Screen 1: welcome banner Panel."""
    raise NotImplementedError


def _render_status_lines(
    console, path: "Path", levels: list[str],
    path_was_created: bool, gitignore_modified: bool,
) -> None:
    """Screen 5: ✓ Created / ✓ Wrote / ✓ Updated status lines."""
    raise NotImplementedError


def _render_quickstart(console, levels: list[str]) -> None:
    """Screen 6: green Panel with the syntax-highlighted Python snippet."""
    raise NotImplementedError


# Public entry point

def run(args: argparse.Namespace) -> int:
    """Execute extract init. Returns the exit code."""
    raise NotImplementedError
```

- [ ] **Step 3: Verify it imports**

Run:
```bash
nix develop --command bash -c 'uv run python -c "from extract import init; print(init.LEVEL_NAME_RE.pattern)"'
```
Expected: prints `^[a-z][a-z0-9_]*$`.

- [ ] **Step 4: Commit**

```bash
git add python/src/extract/init.py
git commit -m "$(cat <<'EOF'
feat(init): module skeleton with constants and stub functions

Adds python/src/extract/init.py with the constants needed by the
extract init wizard: LEVEL_NAME_RE, RESERVED_NAMES, PRESETS,
SAMPLE_VALUES, CONFIG_TEMPLATE (matching rust/src/config.rs sections),
and QUICKSTART_TEMPLATE. All function bodies are NotImplementedError
stubs to be filled in by subsequent TDD tasks.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Implement and test `validate_hierarchy_levels` and `_snake_case`

**Files:**
- Create: `python/tests/test_init.py`
- Modify: `python/src/extract/init.py`

- [ ] **Step 1: Create the test file with validator tests**

Create `python/tests/test_init.py` with:

```python
"""Tests for the extract init module."""

from __future__ import annotations

import re
import sys
from pathlib import Path

import pytest

from extract import init


# ──────────────────────────────────────────────────────────────────────────
# validate_hierarchy_levels


class TestValidateHierarchyLevels:
    def test_valid(self):
        init.validate_hierarchy_levels(["benchmark", "model", "variant"])

    def test_single_level(self):
        init.validate_hierarchy_levels(["dataset"])

    def test_with_underscores_and_digits(self):
        init.validate_hierarchy_levels(["lr_001", "fold_3", "v2"])

    def test_uppercase_rejected(self):
        with pytest.raises(ValueError, match="must match"):
            init.validate_hierarchy_levels(["Benchmark"])

    def test_hyphen_rejected(self):
        with pytest.raises(ValueError, match="must match"):
            init.validate_hierarchy_levels(["foo-bar"])

    def test_leading_digit_rejected(self):
        with pytest.raises(ValueError, match="must match"):
            init.validate_hierarchy_levels(["1foo"])

    def test_empty_string_rejected(self):
        with pytest.raises(ValueError, match="must match"):
            init.validate_hierarchy_levels([""])

    def test_unicode_rejected(self):
        with pytest.raises(ValueError, match="must match"):
            init.validate_hierarchy_levels(["αβγ"])

    @pytest.mark.parametrize("name", sorted(init.RESERVED_NAMES))
    def test_each_reserved_name_rejected(self, name):
        with pytest.raises(ValueError, match="reserved"):
            init.validate_hierarchy_levels([name])

    def test_partial_reserved_in_list_rejected(self):
        with pytest.raises(ValueError, match="reserved"):
            init.validate_hierarchy_levels(["benchmark", "models", "variant"])


# ──────────────────────────────────────────────────────────────────────────
# _snake_case


class TestSnakeCase:
    def test_basic_hyphen(self):
        assert init._snake_case("My-Model") == "my_model"

    def test_basic_space(self):
        assert init._snake_case("foo bar") == "foo_bar"

    def test_strips_punctuation(self):
        assert init._snake_case("foo!") == "foo"

    def test_lowercases(self):
        assert init._snake_case("BENCHMARK") == "benchmark"

    def test_collapses_repeats(self):
        assert init._snake_case("foo--bar") == "foo_bar"

    def test_strips_leading_underscore(self):
        # Result must still be a valid level name (no leading digit/underscore)
        assert init._snake_case("_foo") == "foo"
```

- [ ] **Step 2: Run the tests, verify they fail with NotImplementedError**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestValidateHierarchyLevels::test_valid -v 2>&1 | tail -10'
```
Expected: `FAILED ... NotImplementedError`.

- [ ] **Step 3: Implement `validate_hierarchy_levels` and `_snake_case`**

Replace the two stub functions in `python/src/extract/init.py`:

```python
def validate_hierarchy_levels(levels: list[str]) -> None:
    """Raise ValueError with a friendly message if any level is invalid."""
    for level in levels:
        if not LEVEL_NAME_RE.match(level):
            suggestion = _snake_case(level)
            raise ValueError(
                f"'{level}' must match [a-z][a-z0-9_]* — try '{suggestion}'"
            )
        if level in RESERVED_NAMES:
            raise ValueError(
                f"'{level}' is a reserved name — pick something else"
            )


def _snake_case(s: str) -> str:
    """Best-effort sanitization of an invalid level name into a valid suggestion.

    Lowercases, replaces non-[a-z0-9] with underscores, collapses runs of
    underscores, strips leading/trailing underscores, and strips a leading
    digit if any. Returns a string that satisfies LEVEL_NAME_RE if non-empty.
    """
    s = s.lower()
    s = re.sub(r"[^a-z0-9]+", "_", s)
    s = re.sub(r"_+", "_", s)
    s = s.strip("_")
    s = re.sub(r"^[0-9]+", "", s)
    return s
```

- [ ] **Step 4: Run all init tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py -v 2>&1 | tail -30'
```
Expected: `TestValidateHierarchyLevels` and `TestSnakeCase` all pass (~17 tests).

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): hierarchy level validator and snake_case helper

Validates each level matches [a-z][a-z0-9_]* and is not in the
reserved-name blocklist (models, artifacts, extract, config, plus
the experiments table column names). Suggests a snake_cased
replacement when a level fails the regex check.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Implement and test `_build_quickstart_snippet`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append quickstart snippet tests to `test_init.py`**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# _build_quickstart_snippet


class TestBuildQuickstartSnippet:
    def test_known_recommended_levels(self):
        snippet = init._build_quickstart_snippet(["benchmark", "model", "variant"])
        assert '"benchmark": "imagenet"' in snippet
        assert '"model": "resnet50"' in snippet
        assert '"variant": "lr_0.01"' in snippet
        assert "from extract import Store" in snippet
        assert "with exp.run" in snippet

    def test_dataset_model_seed_levels(self):
        snippet = init._build_quickstart_snippet(["dataset", "model", "seed"])
        assert '"dataset": "imagenet"' in snippet
        assert '"model": "resnet50"' in snippet
        assert '"seed": "42"' in snippet

    def test_unknown_key_fallback(self):
        snippet = init._build_quickstart_snippet(["foo", "bar"])
        assert '"foo": "foo_value"' in snippet
        assert '"bar": "bar_value"' in snippet

    def test_alignment_does_not_crash_for_unequal_keys(self):
        snippet = init._build_quickstart_snippet(["a", "verylongkeyname"])
        # Both keys appear; we don't pin exact whitespace
        assert '"a"' in snippet
        assert '"verylongkeyname"' in snippet

    def test_single_level(self):
        snippet = init._build_quickstart_snippet(["benchmark"])
        assert '"benchmark": "imagenet"' in snippet
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestBuildQuickstartSnippet -v 2>&1 | tail -15'
```
Expected: failures with `NotImplementedError`.

- [ ] **Step 3: Implement `_build_quickstart_snippet`**

Replace the stub in `python/src/extract/init.py`:

```python
def _build_quickstart_snippet(levels: list[str]) -> str:
    """Render QUICKSTART_TEMPLATE with sample values for each level.

    Each line of the dict body is `    "key": "value",` aligned so that
    the values column is consistent. Levels not in SAMPLE_VALUES fall
    back to `"<level>_value"` so any custom hierarchy still produces a
    runnable snippet.
    """
    if not levels:
        # Empty hierarchy is invalid upstream; produce something readable anyway
        return QUICKSTART_TEMPLATE.format(dict_lines='    # (no levels)')

    pairs: list[tuple[str, str]] = []
    for level in levels:
        value = SAMPLE_VALUES.get(level, f"{level}_value")
        pairs.append((level, value))

    # Align the value column based on the widest key (including the surrounding quotes
    # and trailing colon).
    max_key_width = max(len(f'"{k}":') for k, _ in pairs)
    dict_lines = "\n".join(
        f'    {f"\"{k}\":":<{max_key_width + 1}}"{v}",'
        for k, v in pairs
    )
    return QUICKSTART_TEMPLATE.format(dict_lines=dict_lines)
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestBuildQuickstartSnippet -v 2>&1 | tail -15'
```
Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): quickstart snippet builder

Renders the Python snippet shown in the success panel by interpolating
sample values for each chosen hierarchy level. Known level names get
canonical sample values (benchmark→imagenet, model→resnet50,
variant→lr_0.01); unknown levels fall back to "<level>_value".

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Implement and test `_find_git_root`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append tests to `test_init.py`**

Append:

```python
# ──────────────────────────────────────────────────────────────────────────
# _find_git_root


class TestFindGitRoot:
    def test_finds_directly_at_start(self, tmp_path):
        (tmp_path / ".git").mkdir()
        assert init._find_git_root(tmp_path) == tmp_path

    def test_finds_at_parent(self, tmp_path):
        (tmp_path / ".git").mkdir()
        sub = tmp_path / "sub"
        sub.mkdir()
        assert init._find_git_root(sub) == tmp_path

    def test_finds_at_grandparent(self, tmp_path):
        (tmp_path / ".git").mkdir()
        deep = tmp_path / "a" / "b" / "c"
        deep.mkdir(parents=True)
        assert init._find_git_root(deep) == tmp_path

    def test_returns_none_no_repo(self, tmp_path):
        # tmp_path has no .git at all and is unlikely to have one in any parent
        # within 32 levels
        result = init._find_git_root(tmp_path / "sub")
        # If the system happens to have .git in some ancestor, that's fine —
        # but tmp_path itself should not
        if result is not None:
            assert result != tmp_path / "sub"

    def test_handles_file_named_git(self, tmp_path):
        # .git as a file (e.g. submodule pointer) should NOT count as a repo root
        # for our purposes — we look for a directory.
        (tmp_path / ".git").write_text("gitdir: ../other")
        result = init._find_git_root(tmp_path)
        # Either treat as not-found or as found; pin the tighter behavior:
        # we require a directory.
        assert result != tmp_path or (tmp_path / ".git").is_dir()
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestFindGitRoot -v 2>&1 | tail -10'
```
Expected: `NotImplementedError`.

- [ ] **Step 3: Implement `_find_git_root`**

Replace the stub:

```python
def _find_git_root(start: Path) -> Path | None:
    """Walk up from `start` looking for a directory containing .git/.
    Returns the containing directory or None if no git repo found within
    32 levels (a defensive cap so we never traverse the entire filesystem).
    A `.git` *file* (used by submodules) does NOT count — we require a directory.
    """
    current = start.resolve() if start.exists() else start.absolute()
    for _ in range(32):
        if (current / ".git").is_dir():
            return current
        parent = current.parent
        if parent == current:
            return None
        current = parent
    return None
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestFindGitRoot -v 2>&1 | tail -15'
```
Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): find git repo root by walking up the tree

Walks up from a starting path looking for a directory containing
.git/. Used by extract init to decide whether to offer the .gitignore
prompt. Caps the walk at 32 levels to avoid pathological filesystems.
Treats .git as a directory only — submodule .git files do not count.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 3 — Filesystem helpers (TDD)

### Task 7: Implement `_preflight` and `ConfigExistsError`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append preflight tests**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# _preflight


class TestPreflight:
    def test_passes_on_nonexistent_path(self, tmp_path):
        init._preflight(tmp_path / "nope")  # Does not raise

    def test_passes_on_empty_dir(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        init._preflight(store_root)  # Does not raise

    def test_passes_when_dir_exists_no_config(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "extract.db").write_bytes(b"junk")  # Some random file
        init._preflight(store_root)  # Does not raise

    def test_passes_with_malformed_config(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[other]\nkey = 1\n")
        init._preflight(store_root)  # Does not raise — no [store] hierarchy

    def test_passes_with_store_section_no_hierarchy(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[store]\n")
        init._preflight(store_root)  # Does not raise — no hierarchy key

    def test_refuses_existing_configured_store(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text(
            '[store]\nhierarchy = "a > b > c"\n'
        )
        with pytest.raises(init.ConfigExistsError, match="already configured"):
            init._preflight(store_root)
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestPreflight -v 2>&1 | tail -15'
```
Expected: `NotImplementedError` failures.

- [ ] **Step 3: Implement `_preflight`**

Replace the stub:

```python
def _preflight(path: Path) -> None:
    """Refuse if `path` already has a configured store. Raises ConfigExistsError.

    A store is "configured" if `path/config.toml` exists AND has a
    `[store] hierarchy` key. A bare `[store]` section without `hierarchy`
    is treated as bootstrap-incomplete and we proceed.
    """
    config_path = path / "config.toml"
    if not config_path.exists():
        return

    # Use the same parser stdlib uses; tomllib for 3.11+, tomli backport for 3.10.
    try:
        import tomllib  # type: ignore[import-not-found]
    except ImportError:
        import tomli as tomllib  # type: ignore[no-redef]

    with open(config_path, "rb") as f:
        try:
            config = tomllib.load(f)
        except tomllib.TOMLDecodeError:
            # Malformed TOML — treat as not-configured; the new init will overwrite.
            return

    hierarchy = config.get("store", {}).get("hierarchy")
    if hierarchy:
        raise ConfigExistsError(
            f"{config_path} is already configured with hierarchy "
            f"'{hierarchy}'. Refusing to overwrite. To start over: "
            f"rm -rf {path}"
        )
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestPreflight -v 2>&1 | tail -15'
```
Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): preflight refuses already-configured stores

_preflight raises ConfigExistsError if path/config.toml has a
[store] hierarchy already set. Bare [store] sections without
hierarchy are treated as bootstrap-incomplete and we proceed
(this is the "user made the dir but forgot the config" case).

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Implement `_write_config`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append tests**

Append:

```python
# ──────────────────────────────────────────────────────────────────────────
# _write_config


class TestWriteConfig:
    def test_creates_directory_if_missing(self, tmp_path):
        store_root = tmp_path / ".extract"
        assert not store_root.exists()

        created = init._write_config(store_root, ["benchmark", "model", "variant"])
        assert created is True
        assert store_root.is_dir()
        assert (store_root / "config.toml").is_file()

    def test_returns_false_if_directory_existed(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()

        created = init._write_config(store_root, ["a", "b"])
        assert created is False
        assert (store_root / "config.toml").is_file()

    def test_writes_hierarchy_in_store_section(self, tmp_path):
        store_root = tmp_path / ".extract"
        init._write_config(store_root, ["benchmark", "model", "variant"])

        content = (store_root / "config.toml").read_text()
        assert '[store]' in content
        assert 'hierarchy = "benchmark > model > variant"' in content

    def test_writes_commented_default_sections(self, tmp_path):
        store_root = tmp_path / ".extract"
        init._write_config(store_root, ["a", "b"])

        content = (store_root / "config.toml").read_text()
        # Each of the seven sections from rust/src/config.rs appears as a comment
        for section in ["[summary]", "[tables]", "[compare]", "[notifications]",
                        "[theme]", "[metrics]", "[info]"]:
            assert f"# {section}" in content, f"missing commented section {section}"

    def test_round_trips_via_store(self, tmp_path):
        """The config we write must be openable by Store() after Phase 4 lands.
        For now we only verify the file parses and the hierarchy comes back."""
        try:
            import tomllib
        except ImportError:
            import tomli as tomllib

        store_root = tmp_path / ".extract"
        init._write_config(store_root, ["benchmark", "model", "variant"])

        with open(store_root / "config.toml", "rb") as f:
            parsed = tomllib.load(f)
        assert parsed["store"]["hierarchy"] == "benchmark > model > variant"
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestWriteConfig -v 2>&1 | tail -15'
```
Expected: `NotImplementedError`.

- [ ] **Step 3: Implement `_write_config`**

Replace the stub:

```python
def _write_config(path: Path, levels: list[str]) -> bool:
    """Create `path` if needed, write `path/config.toml` from CONFIG_TEMPLATE.

    Returns True if `path` did not exist before this call (i.e. we created it),
    False if it already existed.
    """
    created = not path.exists()
    path.mkdir(parents=True, exist_ok=True)

    hierarchy_str = " > ".join(levels)
    content = CONFIG_TEMPLATE.format(hierarchy=hierarchy_str)
    (path / "config.toml").write_text(content)
    return created
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestWriteConfig -v 2>&1 | tail -15'
```
Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): write config.toml from template

_write_config creates the store directory if needed and writes
config.toml with the chosen hierarchy in [store] plus commented
default sections matching the seven sections parsed by
rust/src/config.rs (summary, tables, compare, notifications,
theme, metrics, info). Returns whether the directory was newly
created so the success status line can say "Created" vs "Using".

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Implement `_update_gitignore`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append tests**

Append:

```python
# ──────────────────────────────────────────────────────────────────────────
# _update_gitignore


class TestUpdateGitignore:
    def test_creates_new_file(self, tmp_path):
        modified = init._update_gitignore(tmp_path)
        assert modified is True
        assert (tmp_path / ".gitignore").read_text() == ".extract/\n"

    def test_appends_when_missing(self, tmp_path):
        (tmp_path / ".gitignore").write_text("*.pyc\n__pycache__/\n")
        modified = init._update_gitignore(tmp_path)
        assert modified is True
        content = (tmp_path / ".gitignore").read_text()
        assert "*.pyc\n" in content
        assert "__pycache__/\n" in content
        assert ".extract/\n" in content
        # Order preserved: existing entries before the new one
        assert content.index("*.pyc") < content.index(".extract/")

    def test_idempotent_when_present(self, tmp_path):
        (tmp_path / ".gitignore").write_text("*.pyc\n.extract/\n")
        modified = init._update_gitignore(tmp_path)
        assert modified is False
        # File unchanged
        assert (tmp_path / ".gitignore").read_text() == "*.pyc\n.extract/\n"

    def test_idempotent_with_trailing_whitespace(self, tmp_path):
        (tmp_path / ".gitignore").write_text(".extract/  \n")
        modified = init._update_gitignore(tmp_path)
        assert modified is False

    def test_idempotent_without_trailing_slash(self, tmp_path):
        (tmp_path / ".gitignore").write_text(".extract\n")
        modified = init._update_gitignore(tmp_path)
        assert modified is False

    def test_appends_newline_if_file_missing_one(self, tmp_path):
        (tmp_path / ".gitignore").write_text("*.pyc")  # No trailing newline
        init._update_gitignore(tmp_path)
        content = (tmp_path / ".gitignore").read_text()
        # The result should have *.pyc on its own line and .extract/ on the next
        assert content == "*.pyc\n.extract/\n"
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestUpdateGitignore -v 2>&1 | tail -15'
```
Expected: `NotImplementedError`.

- [ ] **Step 3: Implement `_update_gitignore`**

Replace the stub:

```python
def _update_gitignore(git_root: Path) -> bool:
    """Append `.extract/` to `git_root/.gitignore` if not already present.

    Idempotent: matches existing entries that are `.extract`, `.extract/`,
    or surrounded by whitespace. Preserves the existing file otherwise —
    no reformatting, no added comments. Returns True if the file was modified.
    """
    gitignore = git_root / ".gitignore"

    if gitignore.exists():
        content = gitignore.read_text()
        for line in content.splitlines():
            stripped = line.strip().rstrip("/")
            if stripped == ".extract":
                return False
        # Need to append. Ensure there's a newline before our addition.
        if content and not content.endswith("\n"):
            content += "\n"
        content += ".extract/\n"
        gitignore.write_text(content)
        return True
    else:
        gitignore.write_text(".extract/\n")
        return True
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestUpdateGitignore -v 2>&1 | tail -15'
```
Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): idempotent .gitignore updater

_update_gitignore appends ".extract/" to the gitignore at the
detected git root. Creates the file if missing. Idempotent —
matches ".extract" with or without trailing slash, ignoring
surrounding whitespace. Preserves existing content verbatim.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 4 — SDK behavior change (the breaking part)

### Task 10: Add `MissingHierarchyError` and rewrite `test_hierarchy.py` fixtures

**Why this task is unusual:** `python/tests/test_hierarchy.py` is currently broken in master — its `tmp_store` fixture calls `Store(root=..., hierarchy="...")` but `Store.__init__` does not accept a `hierarchy` kwarg. ALL tests in that file ERROR during setup. This task brings the file back to a working state aligned with the new SDK behavior, *before* the next task introduces the hard requirement.

**Files:**
- Modify: `python/src/extract/store.py`
- Rewrite: `python/tests/test_hierarchy.py`

- [ ] **Step 1: Confirm the existing failure**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_hierarchy.py -v 2>&1 | tail -20'
```
Expected: every test errors with `TypeError: Store.__init__() got an unexpected keyword argument 'hierarchy'`. This confirms the file is currently broken — we are fixing it, not regressing it.

- [ ] **Step 2: Add `MissingHierarchyError` to `store.py`**

Open `python/src/extract/store.py` and find the imports block at the top. After the imports and before the `_SCHEMA = """...` constant, add:

```python
class MissingHierarchyError(Exception):
    """Raised when Store() is opened against a path that has no configured hierarchy.

    Recovery: run `extract init` in the store directory to write config.toml.
    """
```

- [ ] **Step 3: Rewrite `python/tests/test_hierarchy.py` from scratch**

Replace the **entire** content of `python/tests/test_hierarchy.py` with:

```python
"""Tests for Store hierarchy loading and dict-spec experiment creation.

These tests use the post-init SDK contract:
  - Store(root) requires root/config.toml with [store] hierarchy
  - There is no Store(hierarchy=...) constructor kwarg; bootstrap is via
    extract init (or test fixtures pre-writing config.toml)
  - The legacy path-string API on store.experiment() is gone
"""

from __future__ import annotations

import pytest

import extract
from extract.store import MissingHierarchyError, _parse_hierarchy


def _bootstrap(root, hierarchy="benchmark > model > variant"):
    """Helper: create root/config.toml with the given hierarchy line."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(f'[store]\nhierarchy = "{hierarchy}"\n')


@pytest.fixture
def tmp_store(tmp_path):
    """Create a Store with the standard hierarchy in a temp directory."""
    root = tmp_path / ".extract"
    _bootstrap(root)
    return extract.Store(root=root)


# ──────────────────────────────────────────────────────────────────────────
# Hierarchy loading


class TestHierarchyConfig:
    def test_hierarchy_loaded_from_config(self, tmp_store):
        assert tmp_store._hierarchy == ["benchmark", "model", "variant"]

    def test_hierarchy_persists_in_db(self, tmp_path):
        root = tmp_path / ".extract"
        _bootstrap(root)
        extract.Store(root=root)  # First open writes hierarchy table

        store2 = extract.Store(root=root)
        assert store2._load_hierarchy() == ["benchmark", "model", "variant"]

    def test_hierarchy_mismatch_raises(self, tmp_path):
        root = tmp_path / ".extract"
        _bootstrap(root, "benchmark > model > variant")
        extract.Store(root=root)  # First open populates DB

        # Now corrupt the config to a different hierarchy
        (root / "config.toml").write_text(
            '[store]\nhierarchy = "model > benchmark"\n'
        )
        with pytest.raises(ValueError, match="mismatch"):
            extract.Store(root=root)

    def test_parse_hierarchy_strips_whitespace(self):
        assert _parse_hierarchy("  a > b  >c ") == ["a", "b", "c"]

    def test_parse_hierarchy_empty_level_raises(self):
        with pytest.raises(ValueError, match="empty level"):
            _parse_hierarchy("a > > b")


# ──────────────────────────────────────────────────────────────────────────
# Dict-spec experiment creation


class TestDictExperiment:
    def test_creates_path_in_hierarchy_order(self, tmp_store):
        # Dict keys in arbitrary order, path follows hierarchy config
        exp = tmp_store.experiment({
            "variant": "lr_0.01",
            "benchmark": "imagenet",
            "model": "resnet50",
        })
        assert exp.path == "imagenet/resnet50/lr_0.01"
        assert exp.name == "lr_0.01"

    def test_ancestors_have_correct_node_type(self, tmp_store):
        tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
            "variant": "lr_0.01",
        })
        rows = tmp_store._conn.execute(
            "SELECT path, node_type FROM experiments ORDER BY path"
        ).fetchall()
        types = {r["path"]: r["node_type"] for r in rows}
        assert types["imagenet"] == "benchmark"
        assert types["imagenet/resnet50"] == "model"
        assert types["imagenet/resnet50/lr_0.01"] == "variant"

    def test_partial_spec_creates_partial_hierarchy(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
        })
        assert exp.path == "imagenet/resnet50"
        assert exp.name == "resnet50"

    def test_reuses_existing_ancestors(self, tmp_store):
        tmp_store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "v1"})
        tmp_store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "v2"})
        count = tmp_store._conn.execute(
            "SELECT COUNT(*) FROM experiments WHERE path = 'imagenet'"
        ).fetchone()[0]
        assert count == 1  # imagenet created only once

    def test_skipped_level_raises(self, tmp_store):
        with pytest.raises(ValueError, match="Cannot skip"):
            tmp_store.experiment({"benchmark": "imagenet", "variant": "lr_0.01"})

    def test_unknown_level_raises(self, tmp_store):
        with pytest.raises(ValueError, match="Unknown hierarchy levels"):
            tmp_store.experiment({"benchmark": "imagenet", "dataset": "oops"})

    def test_empty_spec_raises(self, tmp_store):
        with pytest.raises(ValueError, match="at least one"):
            tmp_store.experiment({})

    def test_runs_work_on_dict_experiments(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
            "variant": "lr_0.01",
        })
        with exp.run(config={"lr": 0.001}) as run:
            run.log(step=0, loss=0.5, accuracy=0.7)

        runs = exp.list_runs()
        assert len(runs) == 1
        assert runs[0]["status"] == "completed"


# ──────────────────────────────────────────────────────────────────────────
# Hard requirement: Store() must have config.toml


class TestMissingHierarchy:
    def test_raises_without_config_toml(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        # No config.toml at all
        with pytest.raises(MissingHierarchyError, match="config.toml"):
            extract.Store(root=store_root)

    def test_raises_with_empty_store_section(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[store]\n")
        with pytest.raises(MissingHierarchyError):
            extract.Store(root=store_root)

    def test_raises_with_no_store_section(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[other]\nkey = 1\n")
        with pytest.raises(MissingHierarchyError):
            extract.Store(root=store_root)

    def test_error_message_mentions_path(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        with pytest.raises(MissingHierarchyError) as exc_info:
            extract.Store(root=store_root)
        assert str(store_root) in str(exc_info.value)
        assert "extract init" in str(exc_info.value)


# ──────────────────────────────────────────────────────────────────────────
# Legacy API removal regressions


class TestLegacyAPIRemoved:
    def test_path_string_api_raises(self, tmp_store):
        # The legacy "string spec" branch is gone
        with pytest.raises((TypeError, AttributeError)):
            tmp_store.experiment("foo/bar/baz")  # type: ignore[arg-type]

    def test_experiment_by_path_method_gone(self):
        # The private method is removed entirely
        from extract.store import Store
        assert not hasattr(Store, "_experiment_by_path")

    def test_alter_table_migration_gone(self):
        """Grep store.py source for the dropped runtime migration."""
        from pathlib import Path
        src = Path(__file__).parent.parent / "src" / "extract" / "store.py"
        content = src.read_text()
        assert "ADD COLUMN node_type" not in content
```

- [ ] **Step 4: Verify the new tests fail in the expected way**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_hierarchy.py -v 2>&1 | tail -30'
```
Expected: many tests still fail because we haven't implemented the hard requirement yet (the `MissingHierarchyError` tests fail because `Store()` succeeds when it shouldn't, and the `TestLegacyAPIRemoved` tests fail because the legacy path is still there). Some tests that previously errored (the dict-spec ones) now pass because the fixture works. **Do NOT mark any test as "broken" — these failures are expected and will be fixed in the next task.**

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/store.py python/tests/test_hierarchy.py
git commit -m "$(cat <<'EOF'
test(hierarchy): rewrite tests for post-init SDK contract

The previous test_hierarchy.py was broken in master — its fixture
called Store(root=..., hierarchy="...") but Store.__init__ does not
accept that kwarg, so every test errored at collection time.

This rewrite:
- Adds MissingHierarchyError class to store.py
- Drops the legacy path-string API tests
- Replaces Store(hierarchy=...) with config.toml-bootstrapped fixtures
- Adds TestMissingHierarchy and TestLegacyAPIRemoved test classes
- Scrubs CL vocabulary (cifar100/ewc/lambda → imagenet/resnet50/lr_0.01)

The hard-requirement and legacy-removal tests will start passing in
the next commit when Store.__init__ enforces the new contract.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: Enforce hard requirement in `Store.__init__` and remove legacy API

**Files:**
- Modify: `python/src/extract/store.py`

- [ ] **Step 1: Read the current `Store.__init__` block to confirm line numbers**

Run:
```bash
nix develop --command bash -c 'sed -n "146,220p" python/src/extract/store.py'
```
Expected: shows the `Store` class definition, the `__init__` method, and the helper methods `_load_config_hierarchy`, `_load_hierarchy`, `_save_hierarchy`. Note the structure matches what's described below before editing.

- [ ] **Step 2: Replace `Store.__init__` body**

Use Edit to replace the body of `Store.__init__` (everything from `def __init__` through `self._hierarchy = config_hierarchy or db_hierarchy`) with:

```python
    def __init__(self, root: str | Path = ".extract") -> None:
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "artifacts").mkdir(exist_ok=True)
        (self.root / "models").mkdir(exist_ok=True)

        self.lock = threading.Lock()

        db_path = self.root / "extract.db"
        self._conn = sqlite3.connect(str(db_path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row

        with self.lock:
            self._conn.executescript(_SCHEMA)
            self._conn.commit()

        # config.toml is the single source of truth for hierarchy.
        # The hierarchy table in the DB is a write-through cache.
        config_hierarchy = self._load_config_hierarchy()
        if not config_hierarchy:
            raise MissingHierarchyError(
                f"No config.toml with [store] hierarchy found at "
                f"{self.root}/config.toml. Run `extract init` in this "
                f"directory to set up the store."
            )

        db_hierarchy = self._load_hierarchy()
        if db_hierarchy and config_hierarchy != db_hierarchy:
            raise ValueError(
                f"Hierarchy mismatch: config.toml has "
                f"{' > '.join(config_hierarchy)} but DB has "
                f"{' > '.join(db_hierarchy)}"
            )

        if not db_hierarchy:
            self._save_hierarchy(config_hierarchy)

        self._hierarchy = config_hierarchy
```

The key changes:
- Removed the `try/except` `ALTER TABLE` migration (lines 165-169 in the original)
- Added the `MissingHierarchyError` raise after `_load_config_hierarchy()` returns empty
- Simplified the final assignment from `config_hierarchy or db_hierarchy` to just `config_hierarchy`

- [ ] **Step 3: Remove `_experiment_by_path` and simplify `experiment()`**

Find the `experiment` method (currently at lines 223-233) and `_experiment_by_path` (currently at lines 235-267). Replace BOTH methods with this single method:

```python
    def experiment(self, spec: dict[str, str]) -> Experiment:
        """Create or get an experiment from a hierarchy-keyed dict.

        Args:
            spec: Dict mapping hierarchy levels to values, e.g.
                  {"benchmark": "imagenet", "model": "resnet50",
                   "variant": "lr_0.01"}.
                  Keys must be hierarchy levels declared in config.toml;
                  values cannot skip levels.
        """
        return self._experiment_by_dict(spec)
```

(`_experiment_by_dict` itself stays — it's the implementation. We're just deleting the dispatcher's `isinstance` branch and the legacy path method.)

- [ ] **Step 4: Update the docstring on `_experiment_by_dict`**

Find `_experiment_by_dict` (just below the new `experiment` method). Its current docstring is:

```python
        """Create experiment from a hierarchy-keyed dict."""
```

Leave that unchanged — it's already accurate. (Just confirming nothing needs to change here.)

- [ ] **Step 5: Update the schema constant `_SCHEMA`**

Find `_SCHEMA` near the top of `store.py` (around line 20-50). The `experiments` table currently includes:
```sql
node_type   TEXT
```

Change it to:
```sql
node_type   TEXT NOT NULL
```

(The trailing comma, if any, stays the same.)

- [ ] **Step 6: Run the hierarchy tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_hierarchy.py -v 2>&1 | tail -50'
```
Expected: all tests in `test_hierarchy.py` pass — `TestHierarchyConfig`, `TestDictExperiment`, `TestMissingHierarchy`, `TestLegacyAPIRemoved`.

- [ ] **Step 7: Run the FULL test suite to catch downstream breakage**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/ -x 2>&1 | tail -40'
```
Expected: most tests pass, but `test_mcp.py` may fail because its fixture uses CL vocab — that's OK and gets fixed in Task 12. Also any other test that calls `Store(tmp_path)` without writing config.toml first will now raise `MissingHierarchyError`. **If any test besides `test_mcp.py` fails, document it and fix it in this same commit before moving on.** Run with `--tb=short` to see what's wrong.

- [ ] **Step 8: Commit**

```bash
git add python/src/extract/store.py
git commit -m "$(cat <<'EOF'
feat(store)!: require config.toml hierarchy, drop legacy path API

BREAKING CHANGE: Store(root) now requires root/config.toml with
[store] hierarchy. Bootstrap is via `extract init`.

- Adds hard MissingHierarchyError raise in Store.__init__ when
  config.toml is missing or has no [store] hierarchy
- Removes the legacy isinstance(spec, str) branch in experiment()
- Removes _experiment_by_path entirely
- Removes the runtime ALTER TABLE migration for node_type
- Schema: node_type TEXT NOT NULL on the experiments table

Existing populated stores that already have config.toml continue
to work unchanged. Stores without config.toml refuse to open;
recovery is `extract init` in the store directory.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 12: Update the SQL migration script to NOT NULL

**Files:**
- Modify: `schema/migrations/001_init.sql`

- [ ] **Step 1: Read the current migration**

Run:
```bash
nix develop --command bash -c 'cat schema/migrations/001_init.sql'
```
Expected: a CREATE TABLE statement for `experiments` with a `node_type TEXT` column (currently nullable).

- [ ] **Step 2: Edit the migration**

Change the line `node_type TEXT` (or `node_type   TEXT`) to `node_type TEXT NOT NULL`. The exact whitespace should match surrounding columns.

- [ ] **Step 3: Verify the migration matches the embedded `_SCHEMA`**

Run:
```bash
nix develop --command bash -c 'grep -n "node_type" schema/migrations/001_init.sql python/src/extract/store.py'
```
Expected: both files now have `node_type TEXT NOT NULL`.

- [ ] **Step 4: Commit**

```bash
git add schema/migrations/001_init.sql
git commit -m "$(cat <<'EOF'
schema: node_type TEXT NOT NULL on experiments

Mirrors the embedded _SCHEMA change. Every experiment row created
through the dict-spec API has a non-null node_type matching the
hierarchy level it represents; the legacy path-string API that
created NULL node_type rows is gone.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 13: Scrub CL vocab from `test_mcp.py` and verify it passes

**Files:**
- Modify: `python/tests/test_mcp.py`

This task replaces continual-learning vocabulary in the test fixtures (`benchmark > method > variant`, `cifar100`, `ewc`, `si`, `replay`, `lambda_*`) with generic supervised-learning vocabulary (`benchmark > model > variant`, `imagenet`, `cifar10`, `resnet50`, `vit_base`, `lr_*`). Function: rename only — no behavior change.

- [ ] **Step 1: Identify all CL vocab occurrences**

Run:
```bash
nix develop --command bash -c 'grep -n -E "cifar100|tinyimagenet|benchmark > method|method.*ewc|method.*si|method.*replay|lambda_[0-9]" python/tests/test_mcp.py'
```
Expected: a list of line numbers and matches. Note them all.

- [ ] **Step 2: Apply the scrub**

Use Edit on `python/tests/test_mcp.py` to make the following replacements throughout. Be precise — these are search-and-replace mappings:

| Find (literal) | Replace with |
|---|---|
| `benchmark > method > variant` | `benchmark > model > variant` |
| `"benchmark": "cifar100"` | `"benchmark": "imagenet"` |
| `"benchmark": "tinyimagenet"` | `"benchmark": "cifar10"` |
| `"method": "ewc"` | `"model": "resnet50"` |
| `"method": "si"` | `"model": "vit_base"` |
| `"method": "replay"` | `"model": "convnext"` |
| `"variant": "lambda_1.0"` | `"variant": "lr_0.01"` |
| `"variant": "lambda_0.5"` | `"variant": "lr_0.005"` |
| `"variant": "online_ewc"` | `"variant": "lr_0.001"` |
| `"variant": "c_0.5"` | `"variant": "wd_0.01"` |
| `"variant": "buffer_500"` | `"variant": "bs_64"` |
| `"variant": "buffer_1000"` | `"variant": "bs_128"` |
| `"variant": "variant_a"` | `"variant": "default"` |

Also rename test data references:
- `ewc-cifar100` → `resnet-imagenet`
- `ewc-l1.0-a` → `resnet-lr01-a`
- `ewc-l1.0-b` → `resnet-lr01-b`
- `si-a` → `vit-default`
- `cifar100/ewc/lambda_1.0` → `imagenet/resnet50/lr_0.01`
- `cifar100/si/variant_a` → `imagenet/vit_base/default`

If you encounter `lambda_1.0` or `c_0.5` or similar in *string assertion arguments* (e.g. `assert exp.path == "cifar100/ewc/lambda_1.0"`), update those to match the new path.

- [ ] **Step 3: Run the MCP test suite**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_mcp.py -v 2>&1 | tail -40'
```
Expected: tests all pass (or at minimum, none fail because of CL-vocab references; if anything fails it should be a logic error from your edits).

- [ ] **Step 4: Run the FULL test suite to confirm the SDK behavior change is fully bedded in**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/ 2>&1 | tail -20'
```
Expected: every test passes. If any test fails because of `MissingHierarchyError`, the fixture for that test doesn't pre-write `config.toml` — fix the fixture in the same commit.

- [ ] **Step 5: Commit**

```bash
git add python/tests/test_mcp.py
git commit -m "$(cat <<'EOF'
test(mcp): scrub continual-learning vocabulary

Replaces CL-specific test data (cifar100/ewc/lambda_*) with generic
supervised-learning vocabulary (imagenet/resnet50/lr_*). Test logic
is unchanged; this is purely a vocabulary rename to keep the package
domain-agnostic.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 5 — `extract init` CLI integration (non-interactive first)

### Task 14: Wire `init` subparser into `__main__.py`

**Files:**
- Modify: `python/src/extract/__main__.py`

- [ ] **Step 1: Read the current dispatcher**

Run:
```bash
nix develop --command bash -c 'sed -n "47,114p" python/src/extract/__main__.py'
```
Expected: shows `main(argv)` with the existing `tui` and `sync` subparsers and the `if args.command == "tui": ... elif args.command == "sync": ...` dispatcher.

- [ ] **Step 2: Add the `init` subparser**

In `python/src/extract/__main__.py`, find this block (right after the `tui` subparser definition):

```python
    # --- sync ---
    sync_parser = sub.add_parser("sync", help="Sync .extract/ between machines")
```

Insert the following block immediately above it (between `tui` and `sync`):

```python
    # --- init ---
    init_parser = sub.add_parser(
        "init", help="Bootstrap a .extract/ store with a hierarchy"
    )
    init_parser.add_argument(
        "path", nargs="?", default=".extract",
        help="Path to create the store at (default: .extract)"
    )
    init_parser.add_argument(
        "--hierarchy", default=None,
        help="Skip the interactive picker; use this hierarchy "
             "(e.g. 'benchmark > model > variant')"
    )
    init_parser.add_argument(
        "--no-gitignore", action="store_true",
        help="Do not add .extract/ to .gitignore"
    )
```

- [ ] **Step 3: Add the dispatch branch**

Find this block in `main()`:

```python
    if args.command == "tui":
        binary = _find_tui_binary()
        ...
        os.execvp(binary, [binary, "--store", args.store])

    elif args.command == "sync":
```

Insert this branch between `tui` and `sync`:

```python
    elif args.command == "init":
        from extract import init  # lazy import — pulls in rich/questionary only when needed
        sys.exit(init.run(args))

```

Make sure the existing `elif args.command == "sync":` line follows immediately after.

- [ ] **Step 4: Smoke-test the new subcommand**

Run:
```bash
nix develop --command bash -c 'uv run python -m extract init --help 2>&1'
```
Expected: argparse prints the `init` subcommand help including `--hierarchy` and `--no-gitignore` flags.

Then:
```bash
nix develop --command bash -c 'uv run python -m extract init --hierarchy "a > b > c" /tmp/extract-init-smoke 2>&1'
```
Expected: `NotImplementedError` from the stub `run()` — confirms the dispatch is wired correctly. Then clean up:

```bash
rm -rf /tmp/extract-init-smoke
```

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/__main__.py
git commit -m "$(cat <<'EOF'
feat(cli): wire extract init subparser to extract.init.run

Adds the init subcommand to the existing argparse dispatcher.
The lazy import of extract.init means rich/questionary are only
loaded when the user actually runs init, not on every extract tui
or extract sync invocation.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 15: Implement non-interactive `run()` and TTY check

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

This task implements the **non-interactive path** of `run()` only. The interactive prompts come in Phase 6. After this task, `extract init --hierarchy "..."` works end-to-end including writing files and printing status.

- [ ] **Step 1: Append non-interactive end-to-end tests**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# Non-interactive run() tests


def _make_args(path, hierarchy=None, no_gitignore=False):
    """Helper to construct an argparse.Namespace like __main__.py would."""
    import argparse
    return argparse.Namespace(
        command="init",
        path=str(path),
        hierarchy=hierarchy,
        no_gitignore=no_gitignore,
    )


class TestRunNonInteractive:
    def test_happy_path(self, tmp_path):
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=True)
        rc = init.run(args)
        assert rc == 0
        assert (store_root / "config.toml").exists()

        # Verify the config opens via Store()
        import extract
        store = extract.Store(root=store_root)
        assert store._hierarchy == ["benchmark", "model", "variant"]

    def test_invalid_hierarchy_flag_uppercase(self, tmp_path):
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="Foo > Bar", no_gitignore=True)
        rc = init.run(args)
        assert rc == 1
        assert not store_root.exists()

    def test_invalid_hierarchy_flag_reserved(self, tmp_path):
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="benchmark > models > variant",
                          no_gitignore=True)
        rc = init.run(args)
        assert rc == 1
        assert not store_root.exists()

    def test_invalid_hierarchy_flag_empty_level(self, tmp_path):
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="a > > b", no_gitignore=True)
        rc = init.run(args)
        assert rc == 1

    def test_existing_configured_store_refused(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text(
            '[store]\nhierarchy = "x > y"\n'
        )
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=True)
        rc = init.run(args)
        assert rc == 1
        # Original config untouched
        assert (store_root / "config.toml").read_text() == '[store]\nhierarchy = "x > y"\n'

    def test_completes_bootstrap_when_dir_exists(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()  # Pre-create the dir, no config inside
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=True)
        rc = init.run(args)
        assert rc == 0
        assert (store_root / "config.toml").exists()

    def test_creates_gitignore_when_in_git_repo(self, tmp_path):
        # Set up git repo
        (tmp_path / ".git").mkdir()
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=False)
        rc = init.run(args)
        assert rc == 0
        gi = tmp_path / ".gitignore"
        assert gi.exists()
        assert ".extract/" in gi.read_text()

    def test_skips_gitignore_when_no_git_repo(self, tmp_path):
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=False)
        rc = init.run(args)
        assert rc == 0
        # No .gitignore created (no git repo detected)
        assert not (tmp_path / ".gitignore").exists()

    def test_skips_gitignore_with_flag(self, tmp_path):
        (tmp_path / ".git").mkdir()
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy="benchmark > model > variant",
                          no_gitignore=True)
        rc = init.run(args)
        assert rc == 0
        # --no-gitignore beats the git-repo detection
        assert not (tmp_path / ".gitignore").exists()

    def test_tty_check_errors_without_hierarchy(self, tmp_path, monkeypatch):
        # Force isatty to return False so the TTY check trips
        monkeypatch.setattr("sys.stdin.isatty", lambda: False)
        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy=None, no_gitignore=True)
        rc = init.run(args)
        assert rc == 2  # usage error
        assert not store_root.exists()
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestRunNonInteractive -v 2>&1 | tail -20'
```
Expected: `NotImplementedError` failures.

- [ ] **Step 3: Implement the non-interactive `run()`**

Add the `Console` import at the top of `python/src/extract/init.py`. Find the imports block and replace it with:

```python
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from rich.console import Console
```

Then replace the `run()` stub at the bottom of the file with:

```python
def run(args: argparse.Namespace) -> int:
    """Execute extract init. Returns the exit code.

    Exit codes:
      0  success (or user clean-aborted at preview confirm)
      1  refused / invalid hierarchy / write failure
      2  usage error (non-TTY without --hierarchy)
      130 user pressed Ctrl-C
    """
    console = Console()

    # TTY check: in non-interactive mode, --hierarchy is required
    interactive = sys.stdin.isatty()
    if not interactive and args.hierarchy is None:
        print(
            "error: extract init requires --hierarchy when running "
            "non-interactively.\n"
            "       Example: extract init --hierarchy "
            '"benchmark > model > variant"',
            file=sys.stderr,
        )
        return 2

    store_root = Path(args.path).resolve()

    # Pre-flight: refuse if already configured
    try:
        _preflight(store_root)
    except ConfigExistsError as e:
        console.print(f"[ansired]error:[/ansired] {e}")
        return 1

    # Resolve hierarchy: from --hierarchy flag or interactive picker
    if args.hierarchy is not None:
        try:
            from extract.store import _parse_hierarchy
            levels = _parse_hierarchy(args.hierarchy)
            validate_hierarchy_levels(levels)
        except ValueError as e:
            console.print(f"[ansired]error:[/ansired] {e}")
            return 1
    else:
        # Interactive picker — implemented in Phase 6
        try:
            levels = _pick_hierarchy_interactive()
        except KeyboardInterrupt:
            return 130

    # Resolve gitignore decision
    git_root = None if args.no_gitignore else _find_git_root(store_root.parent)
    if git_root is not None and interactive:
        try:
            wants_gitignore = _confirm_gitignore(git_root)
        except KeyboardInterrupt:
            return 130
    else:
        wants_gitignore = git_root is not None  # Auto-yes in non-interactive when in repo

    # Confirm preview (interactive only — non-interactive auto-confirms)
    if interactive:
        try:
            if not _confirm_write_config(console, store_root, levels):
                console.print("Aborted. No files written.")
                return 0
        except KeyboardInterrupt:
            return 130

    # Write phase
    try:
        path_was_created = _write_config(store_root, levels)
        gitignore_modified = (
            _update_gitignore(git_root) if (wants_gitignore and git_root) else False
        )
    except OSError as e:
        console.print(f"[ansired]error:[/ansired] write failed: {e}")
        return 1

    # Status lines
    _render_status_lines(console, store_root, levels, path_was_created, gitignore_modified)

    # Quickstart panel (only in interactive mode — non-interactive output should
    # stay tight for scripts and CI logs)
    if interactive:
        _render_quickstart(console, levels)

    return 0
```

The interactive helpers (`_pick_hierarchy_interactive`, `_confirm_gitignore`, `_confirm_write_config`, `_render_quickstart`) are still stubs — they'll be implemented in Phase 6. The non-interactive path doesn't call them.

We also need to fill in `_render_status_lines` now since the non-interactive path uses it. Replace the stub (declared in Task 3) with the real implementation:

```python
def _render_status_lines(
    console: "Console",
    path: Path,
    levels: list[str],
    path_was_created: bool,
    gitignore_modified: bool,
) -> None:
    """Print the ✓ Created / ✓ Wrote / ✓ Updated status lines from screen 5."""
    verb = "Created" if path_was_created else "Using"
    pretty_hierarchy = " › ".join(levels)
    console.print(f"[ansigreen]✓[/ansigreen] {verb} [ansicyan]{path}[/ansicyan]")
    console.print(
        f"[ansigreen]✓[/ansigreen] Wrote [ansicyan]{path}/config.toml[/ansicyan] "
        f"[dim](hierarchy: {pretty_hierarchy})[/dim]"
    )
    if gitignore_modified:
        console.print(f"[ansigreen]✓[/ansigreen] Updated [ansicyan].gitignore[/ansicyan]")
```

Note the markup: `[ansigreen]`, `[ansicyan]`, `[ansired]`, `[dim]` — all ANSI-named, no hex.

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestRunNonInteractive -v 2>&1 | tail -30'
```
Expected: all 10 tests pass.

If a test fails about the gitignore not being created when expected: the issue is likely that `_find_git_root(store_root.parent)` is being called with a parent that doesn't have `.git/`. Read the failing test carefully and trace through `_find_git_root`. The logic should walk up from `store_root.parent`, which is `tmp_path` in the test, which DOES have `.git/`.

- [ ] **Step 5: Smoke-test from the CLI**

Run a real end-to-end smoke test:
```bash
nix develop --command bash -c '
  TMPDIR=$(mktemp -d)
  cd $TMPDIR
  uv --project /home/phil_oh/Projects/Creations/Extract run python -m extract init --hierarchy "benchmark > model > variant" --no-gitignore
  cat .extract/config.toml
  cd - >/dev/null
  rm -rf $TMPDIR
'
```
Expected: prints the three `✓` status lines, then `cat` shows a `config.toml` with `hierarchy = "benchmark > model > variant"` plus the commented sections.

- [ ] **Step 6: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): non-interactive run() with TTY check

Implements the non-interactive code path of extract init: parses
--hierarchy, validates, runs preflight, writes config.toml, optionally
updates .gitignore, and prints rich status lines. Refuses to prompt
when stdin is not a TTY and --hierarchy is missing (exit 2). Catches
ConfigExistsError and validation errors and converts them to exit 1.

The interactive picker, gitignore prompt, preview confirm, and
quickstart panel are still stubs — Phase 6 implements those.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 6 — Interactive flow

### Task 16: Implement the welcome banner and quickstart panel renderers

**Files:**
- Modify: `python/src/extract/init.py`

These are pure rendering helpers — no prompts, no input. We implement them now so the interactive path has them ready.

- [ ] **Step 1: Add `_render_welcome` and `_render_quickstart` implementations**

Add the import for `rich.panel` and `rich.syntax` at the top of `python/src/extract/init.py`. Update the imports block to:

```python
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from rich.console import Console
from rich.panel import Panel
from rich.syntax import Syntax
```

Then replace the `_render_welcome` and `_render_quickstart` stubs with the real implementations:

```python
def _render_welcome(console: Console) -> None:
    """Screen 1: welcome banner. Magenta-bordered Panel with the wizard intro."""
    body = (
        "[bold]Welcome to Extract.[/bold]\n\n"
        "[dim]Local-first experiment tracking for deep learning.\n"
        "This wizard will set up a fresh [/dim][ansicyan].extract/[/ansicyan]"
        "[dim] store in\nthe current directory.[/dim]"
    )
    panel = Panel(
        body,
        title="[ansicyan bold]extract init[/ansicyan bold]",
        title_align="left",
        border_style="ansimagenta",
        padding=(1, 2),
    )
    console.print(panel)
    console.print()  # Blank line after the banner


def _render_quickstart(console: Console, levels: list[str]) -> None:
    """Screen 6: success quickstart panel with the syntax-highlighted Python snippet."""
    snippet = _build_quickstart_snippet(levels)
    syntax = Syntax(snippet, "python", theme="ansi_dark", background_color="default")
    panel = Panel(
        syntax,
        title="[ansigreen bold]Quickstart[/ansigreen bold]",
        title_align="left",
        border_style="ansigreen",
        padding=(1, 2),
    )
    console.print()
    console.print(panel)
```

Note: `background_color="default"` on `Syntax` is critical — it tells `rich` to use the terminal background instead of `theme`'s default dark background. Combined with `theme="ansi_dark"`, all colors come from the user's ANSI palette.

- [ ] **Step 2: Smoke-test the welcome banner**

Run:
```bash
nix develop --command bash -c '
  uv run python -c "
from rich.console import Console
from extract.init import _render_welcome, _render_quickstart
c = Console()
_render_welcome(c)
_render_quickstart(c, [\"benchmark\", \"model\", \"variant\"])
"
'
```
Expected: a magenta-bordered welcome panel followed by a green-bordered quickstart panel with a syntax-highlighted Python snippet. Colors should adapt to your terminal theme.

- [ ] **Step 3: Commit**

```bash
git add python/src/extract/init.py
git commit -m "$(cat <<'EOF'
feat(init): welcome banner and quickstart panel renderers

Adds _render_welcome (magenta-bordered Panel with the wizard intro)
and _render_quickstart (green-bordered Panel containing a syntax-
highlighted Python snippet). Both use ANSI-named colors only and
Syntax(theme="ansi_dark", background_color="default") so the user's
terminal theme applies to highlighting.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 17: Implement the hierarchy preset picker (`_pick_hierarchy_interactive`)

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append interactive picker tests with mocks**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# Interactive picker tests (mock questionary)


class FakeQuestionaryPrompt:
    """Helper that mimics questionary's chained .ask() pattern.

    Use as: monkeypatch.setattr(questionary, "select", lambda *a, **k: FakeQuestionaryPrompt(value))
    """
    def __init__(self, value):
        self._value = value

    def ask(self):
        if isinstance(self._value, BaseException):
            raise self._value
        return self._value


class TestPickHierarchyInteractive:
    def test_picks_first_preset(self, monkeypatch):
        import questionary
        # Mock select() to return the recommended preset's full label
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt("benchmark > model > variant"),
        )
        levels = init._pick_hierarchy_interactive()
        assert levels == ["benchmark", "model", "variant"]

    def test_picks_seed_preset(self, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt("dataset > model > seed"),
        )
        levels = init._pick_hierarchy_interactive()
        assert levels == ["dataset", "model", "seed"]

    def test_picks_custom_then_text(self, monkeypatch):
        import questionary
        # First call (select): pick custom
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(init._CUSTOM_SENTINEL),
        )
        # Second call (text): provide a custom hierarchy
        monkeypatch.setattr(
            questionary, "text",
            lambda *a, **k: FakeQuestionaryPrompt("task > model > seed"),
        )
        levels = init._pick_hierarchy_interactive()
        assert levels == ["task", "model", "seed"]

    def test_keyboard_interrupt_in_select(self, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(KeyboardInterrupt()),
        )
        with pytest.raises(KeyboardInterrupt):
            init._pick_hierarchy_interactive()

    def test_keyboard_interrupt_in_text(self, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(init._CUSTOM_SENTINEL),
        )
        monkeypatch.setattr(
            questionary, "text",
            lambda *a, **k: FakeQuestionaryPrompt(KeyboardInterrupt()),
        )
        with pytest.raises(KeyboardInterrupt):
            init._pick_hierarchy_interactive()

    def test_select_returns_none_on_esc(self, monkeypatch):
        """questionary returns None when the user presses Esc; we treat as KeyboardInterrupt."""
        import questionary
        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(None),
        )
        with pytest.raises(KeyboardInterrupt):
            init._pick_hierarchy_interactive()
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestPickHierarchyInteractive -v 2>&1 | tail -15'
```
Expected: failures (mostly `AttributeError` or `NotImplementedError`).

- [ ] **Step 3: Implement `_pick_hierarchy_interactive`**

Add a sentinel constant and the implementation. First, add the import at the top of `init.py`:

```python
import questionary
```

(Near the other imports — after `from rich.syntax import Syntax`.)

Then add a new constant after `RESERVED_NAMES`:

```python
_CUSTOM_SENTINEL = "__custom__"
```

And the questionary style. After `SAMPLE_VALUES`:

```python
# All styling uses ANSI-named colors so the user's terminal theme applies.
QUESTIONARY_STYLE = questionary.Style.from_dict({
    "qmark":       "ansigreen bold",
    "question":    "bold",
    "answer":      "ansicyan bold",
    "pointer":     "ansicyan bold",
    "highlighted": "ansicyan bold",
    "selected":    "ansicyan",
    "instruction": "",
})
```

Then replace the `_pick_hierarchy_interactive` stub (declared in Task 3) with the full implementation:

```python
def _pick_hierarchy_interactive() -> list[str]:
    """Run screens 2 and 3. Returns the chosen hierarchy levels.

    Raises KeyboardInterrupt if the user presses Esc/Ctrl-C at any prompt.
    """
    from extract.store import _parse_hierarchy

    # Build the picker choices: each preset, then the "Define my own" option.
    choices = []
    for hierarchy_str, label in PRESETS:
        choices.append(
            questionary.Choice(
                title=f"{hierarchy_str.replace(' > ', ' › ')}    — {label}",
                value=hierarchy_str,
            )
        )
    choices.append(
        questionary.Choice(
            title="✎ Define my own    — type a custom hierarchy",
            value=_CUSTOM_SENTINEL,
        )
    )

    # Screen 2: preset picker
    answer = questionary.select(
        "Pick a hierarchy or define your own:",
        choices=choices,
        style=QUESTIONARY_STYLE,
        instruction="(Use arrow keys)",
    ).ask()

    # questionary returns None on Esc — treat as Ctrl-C
    if answer is None:
        raise KeyboardInterrupt()

    if answer != _CUSTOM_SENTINEL:
        # Preset chosen — parse it directly
        return _parse_hierarchy(answer)

    # Screen 3: custom text input with live validation
    def _validate(text: str) -> bool | str:
        if not text.strip():
            return "Hierarchy cannot be empty"
        try:
            levels = _parse_hierarchy(text)
        except ValueError as e:
            return str(e)
        try:
            validate_hierarchy_levels(levels)
        except ValueError as e:
            return str(e)
        return True

    custom = questionary.text(
        "Hierarchy levels (separated by ' > '):",
        validate=_validate,
        style=QUESTIONARY_STYLE,
    ).ask()

    if custom is None:
        raise KeyboardInterrupt()

    return _parse_hierarchy(custom)
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestPickHierarchyInteractive -v 2>&1 | tail -20'
```
Expected: all 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): hierarchy preset picker with custom-entry path

_pick_hierarchy_interactive presents the four preset hierarchies
plus a "Define my own" option via questionary.select. If the user
picks custom, a follow-up questionary.text prompt with live regex+
reserved-name validation captures the input. Both Esc and Ctrl-C
raise KeyboardInterrupt for the orchestrator to translate to exit 130.

Uses questionary.Style with ANSI-named colors only so the user's
terminal theme controls the actual rendered colors.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 18: Implement `_confirm_gitignore` and `_confirm_write_config`

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append confirm-prompt tests**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# Confirm prompts


class TestConfirmGitignore:
    def test_returns_true_when_user_says_yes(self, tmp_path, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(True),
        )
        result = init._confirm_gitignore(tmp_path)
        assert result is True

    def test_returns_false_when_user_says_no(self, tmp_path, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(False),
        )
        assert init._confirm_gitignore(tmp_path) is False

    def test_keyboard_interrupt(self, tmp_path, monkeypatch):
        import questionary
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(KeyboardInterrupt()),
        )
        with pytest.raises(KeyboardInterrupt):
            init._confirm_gitignore(tmp_path)


class TestConfirmWriteConfig:
    def test_returns_true_when_user_says_yes(self, tmp_path, monkeypatch):
        import questionary
        from rich.console import Console
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(True),
        )
        console = Console()
        result = init._confirm_write_config(
            console, tmp_path / ".extract", ["benchmark", "model", "variant"]
        )
        assert result is True

    def test_returns_false_when_user_says_no(self, tmp_path, monkeypatch):
        import questionary
        from rich.console import Console
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(False),
        )
        console = Console()
        result = init._confirm_write_config(
            console, tmp_path / ".extract", ["a", "b"]
        )
        assert result is False
```

- [ ] **Step 2: Verify they fail**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestConfirmGitignore python/tests/test_init.py::TestConfirmWriteConfig -v 2>&1 | tail -15'
```
Expected: `NotImplementedError`.

- [ ] **Step 3: Implement both confirm helpers**

Replace the two stubs in `python/src/extract/init.py`:

```python
def _confirm_gitignore(git_root: Path) -> bool:
    """Screen 4a: ask whether to add .extract/ to .gitignore. Default Yes."""
    return questionary.confirm(
        f"Add .extract/ to .gitignore? (detected git repo at {git_root})",
        default=True,
        style=QUESTIONARY_STYLE,
    ).ask() or False  # questionary returns None on Esc; treat as no


def _confirm_write_config(
    console: Console, path: Path, levels: list[str]
) -> bool:
    """Screen 4b: render the preview panel and ask whether to write. Default Yes."""
    hierarchy_str = " > ".join(levels)
    content = CONFIG_TEMPLATE.format(hierarchy=hierarchy_str)

    syntax = Syntax(
        content, "toml", theme="ansi_dark", background_color="default"
    )
    panel = Panel(
        syntax,
        title=f"[ansiblue bold]Preview: {path}/config.toml[/ansiblue bold]",
        title_align="left",
        border_style="ansiblue",
        padding=(0, 1),
    )
    console.print()
    console.print(panel)
    console.print()

    return questionary.confirm(
        "Write this config?",
        default=True,
        style=QUESTIONARY_STYLE,
    ).ask() or False
```

- [ ] **Step 4: Run the tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestConfirmGitignore python/tests/test_init.py::TestConfirmWriteConfig -v 2>&1 | tail -15'
```
Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): gitignore and preview confirmation prompts

_confirm_gitignore: questionary.confirm asking whether to add
.extract/ to .gitignore (default Yes).

_confirm_write_config: renders the syntax-highlighted TOML preview
in a blue-bordered Panel, then asks whether to write the file
(default Yes). Both prompts use ANSI-named styling.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 19: Wire welcome banner into the interactive `run()` and add full interactive end-to-end test

**Files:**
- Modify: `python/src/extract/init.py`
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Update `run()` to print the welcome banner in interactive mode**

Find the `run()` function in `python/src/extract/init.py`. Locate the line where `console = Console()` is created. Add the welcome banner call right after the TTY check passes (i.e. when we know we're in interactive mode and `--hierarchy` was not given), so it only fires for the interactive flow.

Replace this section of `run()`:

```python
    console = Console()

    # TTY check: in non-interactive mode, --hierarchy is required
    interactive = sys.stdin.isatty()
    if not interactive and args.hierarchy is None:
        print(
            "error: extract init requires --hierarchy when running "
            "non-interactively.\n"
            "       Example: extract init --hierarchy "
            '"benchmark > model > variant"',
            file=sys.stderr,
        )
        return 2

    store_root = Path(args.path).resolve()
```

with:

```python
    console = Console()

    # TTY check: in non-interactive mode, --hierarchy is required
    interactive = sys.stdin.isatty()
    if not interactive and args.hierarchy is None:
        print(
            "error: extract init requires --hierarchy when running "
            "non-interactively.\n"
            "       Example: extract init --hierarchy "
            '"benchmark > model > variant"',
            file=sys.stderr,
        )
        return 2

    # Welcome banner only in interactive mode (suppressed when --hierarchy given)
    if interactive and args.hierarchy is None:
        _render_welcome(console)

    store_root = Path(args.path).resolve()
```

- [ ] **Step 2: Append a fully-mocked end-to-end interactive test**

Append to `python/tests/test_init.py`:

```python
class TestRunInteractiveFullFlow:
    def test_full_interactive_happy_path(self, tmp_path, monkeypatch):
        """Mock all prompts; verify the full flow writes the config."""
        import questionary
        # Force isatty True so we go down the interactive path
        monkeypatch.setattr("sys.stdin.isatty", lambda: True)

        # Each questionary primitive returns True (or the first preset for select)
        select_calls = iter(["benchmark > model > variant"])
        confirm_calls = iter([True, True])  # gitignore=yes, write=yes

        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(next(select_calls)),
        )
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(next(confirm_calls)),
        )

        store_root = tmp_path / ".extract"
        # Pre-create .git/ at tmp_path so the gitignore branch fires
        (tmp_path / ".git").mkdir()

        args = _make_args(store_root, hierarchy=None, no_gitignore=False)
        rc = init.run(args)
        assert rc == 0
        assert (store_root / "config.toml").exists()
        assert ".extract/" in (tmp_path / ".gitignore").read_text()

    def test_full_interactive_user_aborts_at_preview(self, tmp_path, monkeypatch):
        import questionary
        monkeypatch.setattr("sys.stdin.isatty", lambda: True)

        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt("benchmark > model > variant"),
        )
        # gitignore prompt: yes, write prompt: NO
        confirm_calls = iter([True, False])
        monkeypatch.setattr(
            questionary, "confirm",
            lambda *a, **k: FakeQuestionaryPrompt(next(confirm_calls)),
        )

        store_root = tmp_path / ".extract"
        (tmp_path / ".git").mkdir()

        args = _make_args(store_root, hierarchy=None, no_gitignore=False)
        rc = init.run(args)
        assert rc == 0
        # Aborted at preview confirm — no files written
        assert not store_root.exists()
        assert not (tmp_path / ".gitignore").exists()

    def test_full_interactive_keyboard_interrupt_at_picker(self, tmp_path, monkeypatch):
        import questionary
        monkeypatch.setattr("sys.stdin.isatty", lambda: True)

        monkeypatch.setattr(
            questionary, "select",
            lambda *a, **k: FakeQuestionaryPrompt(KeyboardInterrupt()),
        )

        store_root = tmp_path / ".extract"
        args = _make_args(store_root, hierarchy=None, no_gitignore=True)
        rc = init.run(args)
        assert rc == 130
        assert not store_root.exists()
```

- [ ] **Step 3: Run the new tests**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestRunInteractiveFullFlow -v 2>&1 | tail -25'
```
Expected: all 3 tests pass. If you see hangs or stalls, check that `monkeypatch.setattr("sys.stdin.isatty", ...)` is being applied.

- [ ] **Step 4: Run the full init test suite as a regression**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py -v 2>&1 | tail -20'
```
Expected: every test in `test_init.py` passes (~50+ tests total across all classes).

- [ ] **Step 5: Commit**

```bash
git add python/src/extract/init.py python/tests/test_init.py
git commit -m "$(cat <<'EOF'
feat(init): wire interactive flow end-to-end

run() now calls _render_welcome at the top of the interactive path
(suppressed when --hierarchy is given for clean script output).
The full interactive flow — preset picker, custom text input,
gitignore confirm, preview confirm, write phase, status lines,
quickstart panel — is now complete.

Adds TestRunInteractiveFullFlow which exercises the entire flow
end-to-end with mocked questionary prompts for happy path, user
abort, and Ctrl-C scenarios.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 7 — Theme inheritance regression and MCP smoke test

### Task 20: Add hex-color regression test

**Files:**
- Modify: `python/tests/test_init.py`

- [ ] **Step 1: Append the regression test**

Append to `python/tests/test_init.py`:

```python
# ──────────────────────────────────────────────────────────────────────────
# Theme inheritance regression


class TestThemeInheritance:
    def test_no_hex_colors_in_init_module(self):
        """Theme inheritance: rich/questionary must use ANSI-named colors only.

        Hex literals (#RRGGBB or #RRGGBBAA) hardcode TrueColor values that
        ignore the user's terminal theme. The init wizard must inherit
        whatever colors the terminal is configured to use.
        """
        src = Path(__file__).parent.parent / "src" / "extract" / "init.py"
        content = src.read_text()
        # Match "#RRGGBB" or "#RRGGBBAA" inside string literals (single or double quoted)
        hex_pattern = re.compile(r'["\']#[0-9a-fA-F]{6,8}["\']')
        matches = hex_pattern.findall(content)
        assert not matches, (
            f"Hex color literals found in init.py: {matches}. "
            f"Use ANSI-named colors (ansicyan, ansigreen, etc.) instead."
        )

    def test_no_truecolor_named_styles_in_init_module(self):
        """Catch the easy mistake of using rich's deep_sky_blue3 / hot_pink etc.

        These are fancy named colors that map to TrueColor, not ANSI.
        """
        src = Path(__file__).parent.parent / "src" / "extract" / "init.py"
        content = src.read_text()
        # rich's TrueColor named palette includes things like "deep_sky_blue3",
        # "dark_orange3", "medium_purple4", "spring_green2", etc.
        # Catch any X_color_N patterns where N is a digit suffix.
        truecolor_pattern = re.compile(r'\b[a-z]+_[a-z]+_?\d+\b')
        # Some legitimate matches we want to allow
        allowed = {"benchmark_v0", "model_v0", "variant_v0"}
        matches = [
            m for m in truecolor_pattern.findall(content)
            if m not in allowed
        ]
        # This is a lint heuristic — if it triggers a false positive, expand allowed
        # rather than weakening the test
        for match in matches:
            assert "color" not in match.lower(), (
                f"Possible TrueColor name in init.py: {match!r}. "
                f"Use ANSI colors only."
            )
```

- [ ] **Step 2: Run the regression test**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py::TestThemeInheritance -v 2>&1 | tail -10'
```
Expected: both tests pass. If `test_no_hex_colors_in_init_module` fails, your earlier implementation contains a hex color literal — find and replace it with an ANSI name.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_init.py
git commit -m "$(cat <<'EOF'
test(init): hex-color regression test

Greps init.py for hex color literals (#RRGGBB) and TrueColor named
palette entries (deep_sky_blue3 etc). Both bypass terminal theme
inheritance, which is the whole point of the rich/questionary
styling discipline. Locks the constraint as a CI-enforced lint.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 21: Add MCP cleanup smoke test

**Files:**
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Append the smoke test**

Append (after the existing test classes) in `python/tests/test_mcp.py`:

```python
class TestMCPBundling:
    def test_module_loads_without_optional_extra(self):
        """After bundling, mcp.py imports cleanly with no conditional checks."""
        from extract import mcp as mcp_mod
        assert mcp_mod.mcp_server is not None
        assert mcp_mod.FastMCP is not None

    def test_no_try_except_importerror_for_mcp(self):
        """The conditional import fallback should be gone."""
        from pathlib import Path
        src = Path(__file__).parent.parent / "src" / "extract" / "mcp.py"
        content = src.read_text()
        assert "except ImportError" not in content, (
            "try/except ImportError fallback should be removed from mcp.py "
            "now that mcp is a base dependency"
        )
        assert "if FastMCP" not in content, (
            "Conditional `if FastMCP else None` should be removed"
        )
```

- [ ] **Step 2: Run the smoke test**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_mcp.py::TestMCPBundling -v 2>&1 | tail -15'
```
Expected: both tests pass.

- [ ] **Step 3: Commit**

```bash
git add python/tests/test_mcp.py
git commit -m "$(cat <<'EOF'
test(mcp): smoke test for bundled mcp dependency

Verifies that mcp.py imports without the try/except ImportError
fallback (which was removed in Task 2) and that mcp_server is
non-None at module load time.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 8 — Documentation and fixture vocabulary scrub

### Task 22: Update README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Read the current README**

Run:
```bash
nix develop --command bash -c 'cat README.md'
```

Note all occurrences of:
- `pip install` lines (especially `[mcp]` extras)
- The Quick Start section showing manual `config.toml` creation
- CL-vocab examples (`cifar100`, `ewc`, `lambda_*`, `benchmark > method > variant`)

- [ ] **Step 2: Update install instructions**

Find any line of the form `pip install 'extract-tracker[mcp]'` and change to `pip install extract-tracker`. Find any line that tells users to manually create `.extract/config.toml` and replace it with a sentence pointing at `extract init`.

- [ ] **Step 3: Update Quick Start section**

The current Quick Start (around line 35-45) likely tells users to manually create `config.toml`. Replace it with:

```markdown
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
with exp.run(config={"lr": 0.01}) as run:
    run.log(step=0, loss=2.3, accuracy=0.1)

# Browse with: extract tui
```
```

(Replace the existing Quick Start block.)

- [ ] **Step 4: Scrub remaining CL vocab in the README**

Search for any remaining `cifar100`, `ewc`, `method`, `lambda` references. Apply the same find/replace as Task 13:
- `benchmark > method > variant` → `benchmark > model > variant`
- `cifar100` → `imagenet`
- `ewc` → `resnet50`
- `lambda_*` → `lr_*`

Run:
```bash
nix develop --command bash -c 'grep -nE "cifar100|tinyimagenet|ewc|method.*variant|lambda_" README.md'
```
Expected: no matches.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs(readme): lead with extract init, scrub CL vocab

Updates the install instructions to drop the [mcp] extra (now
bundled). Quick Start now starts with extract init instead of
manually creating config.toml. Examples use generic supervised-
learning vocabulary (imagenet/resnet50/lr_0.01) instead of
continual-learning specifics.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 23: Update DOC.md

**Files:**
- Modify: `DOC.md`

- [ ] **Step 1: Read the relevant sections**

Run:
```bash
nix develop --command bash -c 'sed -n "1,40p" DOC.md'
nix develop --command bash -c 'grep -n "experiment.*spec\|String spec\|legacy\|cifar100\|ewc" DOC.md'
```

- [ ] **Step 2: Update the `store.experiment()` signature documentation**

Find the line documenting the signature (around line 23):

```markdown
**`store.experiment(spec: dict[str, str] | str) -> Experiment`**
```

Change to:

```markdown
**`store.experiment(spec: dict[str, str]) -> Experiment`**
```

Then find and **delete** the entire "String spec (legacy)" sub-bullet block (around lines 29-32):

```markdown
- String spec (legacy): slash-delimited path, no `node_type` set.
  ```python
  store.experiment("cifar100/ewc/v1")
  ```
```

The dict spec sub-bullet stays.

- [ ] **Step 3: Add an `extract init` row to the CLI section**

Find the CLI section in DOC.md (search for `## CLI` or similar). Add a row to whatever table/list documents subcommands:

```markdown
| `extract init [path] [--hierarchy ...] [--no-gitignore]` | Bootstrap a `.extract/` store with a hierarchy. Interactive by default; `--hierarchy "a > b > c"` for non-interactive use. |
```

(Or follow the existing format style — it might be a definition list rather than a table.)

- [ ] **Step 4: Scrub CL vocab from DOC.md**

Apply the same find/replace mapping as Task 13 throughout DOC.md.

Run:
```bash
nix develop --command bash -c 'grep -nE "cifar100|tinyimagenet|ewc|method.*variant|lambda_" DOC.md'
```
Expected: no matches (or only matches inside the example commented-out config sections that intentionally show generic strings).

- [ ] **Step 5: Drop any mention of the `[mcp]` extra**

Run:
```bash
nix develop --command bash -c 'grep -n "extract-tracker\[mcp\]\|optional-dependencies" DOC.md'
```
Replace any `pip install 'extract-tracker[mcp]'` with `pip install extract-tracker`.

- [ ] **Step 6: Commit**

```bash
git add DOC.md
git commit -m "$(cat <<'EOF'
docs(api): document extract init, drop legacy path API

- Updates store.experiment() signature to dict[str, str] only
- Removes the "String spec (legacy)" subsection entirely
- Adds extract init to the CLI subcommands documentation
- Drops the [mcp] extra mention (mcp is now a base dep)
- Scrubs continual-learning vocabulary from examples

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 24: Scrub CL vocab from `scripts/generate_test_data.py`

**Files:**
- Modify: `scripts/generate_test_data.py`

- [ ] **Step 1: Read the current script**

Run:
```bash
nix develop --command bash -c 'cat scripts/generate_test_data.py'
```

Note that this script needs Store() to be bootstrappable — verify how it currently creates the store. After Task 11, `Store(root)` requires config.toml. The script must either pre-write config.toml or call something equivalent to `extract init`.

- [ ] **Step 2: Add config.toml bootstrap at the start of the script**

Find the section where the script creates the Store. Currently it's something like:

```python
store = Store(root=...)
```

Replace it with:

```python
# Bootstrap the store with the new-required hierarchy config.
store_root = Path(...).resolve()
store_root.mkdir(parents=True, exist_ok=True)
(store_root / "config.toml").write_text(
    '[store]\nhierarchy = "benchmark > model > variant"\n'
)
store = Store(root=store_root)
```

(Use the script's actual `store_root` variable name.)

- [ ] **Step 3: Apply the CL vocab scrub**

Apply the same find/replace mapping as Task 13. The script currently creates experiments like:
- `{"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"}`
- `{"benchmark": "tinyimagenet", "method": "replay", "variant": "buffer_1000"}`

Replace each with the generic vocabulary mapping:
- `cifar100` → `imagenet`
- `tinyimagenet` → `cifar10`
- `method` (key name) → `model`
- `ewc` → `resnet50`
- `si` → `vit_base`
- `replay` → `convnext`
- `lambda_1.0` → `lr_0.01`
- `lambda_0.5` → `lr_0.005`
- `c_0.5` → `wd_0.01`
- `buffer_500` → `bs_64`
- `buffer_1000` → `bs_128`
- `online_ewc` → `lr_0.001`

Also update model registration calls (e.g. `register_model(name="ewc-cifar100", ...)` → `register_model(name="resnet-imagenet", ...)`).

- [ ] **Step 4: Run the script as a smoke test**

Run:
```bash
nix develop --command bash -c '
  rm -rf /tmp/extract-test-data
  uv run python scripts/generate_test_data.py /tmp/extract-test-data 2>&1 | tail -10
  ls /tmp/extract-test-data/.extract/
  rm -rf /tmp/extract-test-data
'
```
(Adjust the command-line arg if the script takes its target dir differently.)
Expected: script runs to completion with no errors. The `.extract/` directory exists with `config.toml`, `extract.db`, etc.

- [ ] **Step 5: Verify no CL vocab remains**

Run:
```bash
nix develop --command bash -c 'grep -nE "cifar100|tinyimagenet|ewc|method.*variant|lambda_|c_0\.|buffer_" scripts/generate_test_data.py'
```
Expected: no matches.

- [ ] **Step 6: Commit**

```bash
git add scripts/generate_test_data.py
git commit -m "$(cat <<'EOF'
chore(scripts): scrub CL vocab from generate_test_data

- Pre-writes config.toml before opening Store() (now required)
- Replaces continual-learning fixture data with generic supervised-
  learning vocabulary (imagenet/cifar10/resnet50/vit_base/convnext
  with lr_*, wd_*, bs_* variants)

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 9 — Final integration checks

### Task 25: Run the full test suite and verify everything passes

**Files:**
- (none — this is a verification task)

- [ ] **Step 1: Run the full pytest suite**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/ 2>&1 | tail -30'
```
Expected: every test passes. No errors, no failures, no warnings about missing fixtures or imports.

If anything fails, do not commit a workaround. Trace it to its root cause and fix the actual problem in a follow-up commit. Common root causes:
- A test that creates `Store()` without writing `config.toml` first
- A test that imports a removed function
- A test that asserts on the legacy path-string API

- [ ] **Step 2: Run the init test suite alone for clarity**

Run:
```bash
nix develop --command bash -c 'uv run pytest python/tests/test_init.py -v 2>&1 | tail -50'
```
Expected: all init tests pass (~55+ tests across the test classes).

- [ ] **Step 3: Run an end-to-end smoke test from the CLI**

Run:
```bash
nix develop --command bash -c '
  set -e
  TMPDIR=$(mktemp -d)
  cd $TMPDIR

  # Initialize a git repo so the gitignore branch fires
  git init -q

  # Run extract init non-interactively
  uv --project /home/phil_oh/Projects/Creations/Extract run python -m extract init --hierarchy "benchmark > model > variant"

  # Verify the outputs
  test -f .extract/config.toml || (echo "MISSING config.toml" && exit 1)
  test -f .gitignore || (echo "MISSING .gitignore" && exit 1)
  grep -q "hierarchy = \"benchmark > model > variant\"" .extract/config.toml || (echo "WRONG hierarchy" && exit 1)
  grep -q ".extract/" .gitignore || (echo "MISSING .extract/ in gitignore" && exit 1)

  # Verify Store() opens cleanly
  uv --project /home/phil_oh/Projects/Creations/Extract run python -c "
from extract import Store
s = Store(root=\".extract\")
assert s._hierarchy == [\"benchmark\", \"model\", \"variant\"]
print(\"Store opens cleanly\")
"

  # Verify dict-spec experiment works
  uv --project /home/phil_oh/Projects/Creations/Extract run python -c "
from extract import Store
s = Store(root=\".extract\")
exp = s.experiment({\"benchmark\": \"imagenet\", \"model\": \"resnet50\", \"variant\": \"lr_0.01\"})
assert exp.path == \"imagenet/resnet50/lr_0.01\"
print(\"experiment() works:\", exp.path)
"

  cd - >/dev/null
  rm -rf $TMPDIR
  echo "ALL SMOKE TESTS PASSED"
'
```
Expected: prints `ALL SMOKE TESTS PASSED` at the end with no errors.

- [ ] **Step 4: Verify the legacy path API really is gone**

Run:
```bash
nix develop --command bash -c '
  TMPDIR=$(mktemp -d)
  cd $TMPDIR
  uv --project /home/phil_oh/Projects/Creations/Extract run python -m extract init --hierarchy "a > b > c"
  uv --project /home/phil_oh/Projects/Creations/Extract run python -c "
from extract import Store
s = Store(root=\".extract\")
try:
    s.experiment(\"foo/bar\")
    print(\"FAIL: legacy API still works\")
except (TypeError, AttributeError) as e:
    print(\"OK: legacy API rejected:\", type(e).__name__)
"
  cd - >/dev/null
  rm -rf $TMPDIR
'
```
Expected: prints `OK: legacy API rejected: TypeError` (or `AttributeError`).

- [ ] **Step 5: Verify a non-init Store() raises the right error**

Run:
```bash
nix develop --command bash -c '
  TMPDIR=$(mktemp -d)
  uv --project /home/phil_oh/Projects/Creations/Extract run python -c "
from extract import Store
from extract.store import MissingHierarchyError
try:
    Store(root=\"$TMPDIR/empty\")
    print(\"FAIL: Store opened without config.toml\")
except MissingHierarchyError as e:
    print(\"OK: MissingHierarchyError:\", str(e)[:80])
"
  rm -rf $TMPDIR
'
```
Expected: prints `OK: MissingHierarchyError: No config.toml with [store] hierarchy ...`.

- [ ] **Step 6: No commit needed**

This task is verification only. If everything passes, the implementation is complete. If something fails, fix it and re-run the relevant verification step before declaring done.

---

## Self-Review Checklist (post-plan)

After writing all tasks, verify:

- **Spec coverage:** Every section of `docs/superpowers/specs/2026-04-08-extract-init-design.md` has a task that implements it. Specifically:
  - §1.1 (init command) → Tasks 3-9, 14-19
  - §1.2 (Store hard requirement) → Tasks 10-12
  - §1.3 (legacy API removal) → Tasks 11-13
  - §1.4 (dependency bundling) → Tasks 1-2
  - §1.5 (theme inheritance) → Tasks 16-19, 20
  - §1.6 (CL vocab scrub) → Tasks 13, 22-24
  - §2 (CLI surface) → Tasks 14-15
  - §3 (interactive flow) → Tasks 16-19
  - §4 (files written) → Tasks 8-9
  - §5 (SDK changes) → Tasks 10-12
  - §6 (dependency manifest) → Task 1
  - §7 (code organization) → Tasks 3-9, 14-19
  - §8 (testing strategy) → Tasks 4-9, 15, 17-21
  - §9 (implementation notes) → Embedded in tasks
  - §10 (open questions) → All locked in spec, referenced throughout

- **Placeholder scan:** Searched for `TBD`, `TODO`, `XXX`, `???`, `implement later`, `add appropriate`. None found.

- **Type consistency:** Function names match across tasks: `validate_hierarchy_levels`, `_snake_case`, `_build_quickstart_snippet`, `_find_git_root`, `_preflight`, `_write_config`, `_update_gitignore`, `_pick_hierarchy_interactive`, `_confirm_gitignore`, `_confirm_write_config`, `_render_welcome`, `_render_status_lines`, `_render_quickstart`, `run`. `ConfigExistsError`, `MissingHierarchyError`, `_CUSTOM_SENTINEL` constants used consistently.

- **TDD discipline:** Every task that adds production code starts with a failing test, then minimal implementation, then verification. Phase 4 (the breaking SDK change) has the unusual structure of fixing broken existing tests first (Task 10), then introducing the hard requirement (Task 11), then verifying the rest of the suite (Task 13).

- **Frequent commits:** Each task ends with a commit. Total: 25 commits across 25 tasks. No task batches multiple unrelated changes.

- **DRY:** `_parse_hierarchy` is reused from `store.py`, not duplicated in `init.py`. Validation logic lives in one function, called by both the interactive validator and the `--hierarchy` flag parser.

- **YAGNI:** No `--force`, no `--yes`, no `--migrate`, no `quickstart.py` file, no rich-side fallback for missing deps (deps are now mandatory).
