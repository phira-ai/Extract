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

import questionary
from rich.console import Console
from rich.panel import Panel
from rich.syntax import Syntax

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

# Sentinel used as the value for the "Define my own" option in the picker.
_CUSTOM_SENTINEL = "__custom__"

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
    """Render QUICKSTART_TEMPLATE with sample values for each level.

    Each dict line is formatted as `    "key": "value",` with a single
    space after the colon, regardless of key length. Levels not in
    SAMPLE_VALUES fall back to `"<level>_value"` so any custom hierarchy
    still produces a runnable snippet.
    """
    if not levels:
        # Empty hierarchy is invalid upstream; produce something readable anyway
        return QUICKSTART_TEMPLATE.format(dict_lines='    # (no levels)')

    pairs: list[tuple[str, str]] = []
    for level in levels:
        value = SAMPLE_VALUES.get(level, f"{level}_value")
        pairs.append((level, value))

    dict_lines_list = []
    for k, v in pairs:
        dict_lines_list.append(f'    "{k}": "{v}",')
    dict_lines = "\n".join(dict_lines_list)
    return QUICKSTART_TEMPLATE.format(dict_lines=dict_lines)


def _find_git_root(start: "Path") -> "Path | None":
    """Walk up from `start` looking for a directory containing .git/.
    Returns the containing directory or None if no git repo within 32 levels.
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


# Filesystem helpers

def _preflight(path: "Path") -> None:
    """Refuse if `path` already has a configured store. Raises ConfigExistsError.

    A store is "configured" if `path/config.toml` exists AND has a
    `[store] hierarchy` key. A bare `[store]` section without `hierarchy`
    is treated as bootstrap-incomplete and we proceed.
    """
    config_path = path / "config.toml"
    if not config_path.exists():
        return

    # Use tomllib for 3.11+, tomli backport for 3.10.
    try:
        import tomllib  # type: ignore[import-not-found]
    except ImportError:
        import tomli as tomllib  # type: ignore[no-redef]

    with open(config_path, "rb") as f:
        try:
            config = tomllib.load(f)
        except tomllib.TOMLDecodeError:
            # Malformed TOML — treat as not-configured; new init will overwrite.
            return

    hierarchy = config.get("store", {}).get("hierarchy")
    if hierarchy:
        raise ConfigExistsError(
            f"{config_path} is already configured with hierarchy "
            f"'{hierarchy}'. Refusing to overwrite. To start over: "
            f"rm -rf {path}"
        )


def _write_config(path: "Path", levels: list[str]) -> bool:
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


def _update_gitignore(git_root: "Path") -> bool:
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


# Interactive prompts (filled in during Phase 6)

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


def _confirm_gitignore(git_root: "Path") -> bool:
    """Screen 4a: ask whether to add .extract/ to .gitignore. Default Yes."""
    return questionary.confirm(
        f"Add .extract/ to .gitignore? (detected git repo at {git_root})",
        default=True,
        style=QUESTIONARY_STYLE,
    ).ask() or False  # questionary returns None on Esc; treat as no


def _confirm_write_config(
    console: Console, path: "Path", levels: list[str]
) -> bool:
    """Screen 4b: render the preview panel and ask whether to write. Default Yes."""
    hierarchy_str = " > ".join(levels)
    content = CONFIG_TEMPLATE.format(hierarchy=hierarchy_str)

    syntax = Syntax(
        content, "toml", theme="ansi_dark", background_color="default"
    )
    panel = Panel(
        syntax,
        title=f"[cyan bold]Preview: {path}/config.toml[/cyan bold]",
        title_align="left",
        border_style="blue",
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


# Rich rendering (filled in during Phase 6)

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
        border_style="magenta",
        padding=(1, 2),
    )
    console.print(panel)
    console.print()  # Blank line after the banner


def _render_status_lines(
    console, path: "Path", levels: list[str],
    path_was_created: bool, gitignore_modified: bool,
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


def _render_quickstart(console: Console, levels: list[str]) -> None:
    """Screen 6: success quickstart panel with the syntax-highlighted Python snippet."""
    snippet = _build_quickstart_snippet(levels)
    syntax = Syntax(snippet, "python", theme="ansi_dark", background_color="default")
    panel = Panel(
        syntax,
        title="[ansigreen bold]Quickstart[/ansigreen bold]",
        title_align="left",
        border_style="green",
        padding=(1, 2),
    )
    console.print()
    console.print(panel)


# Public entry point

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

    # Welcome banner only in interactive mode (suppressed when --hierarchy given)
    if interactive and args.hierarchy is None:
        _render_welcome(console)

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
