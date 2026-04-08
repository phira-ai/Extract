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
