"""CLI entry point: python -m extract."""

from __future__ import annotations

import argparse
import os
import shutil
import sys
from pathlib import Path


def _find_tui_binary() -> str | None:
    """Locate the extract-tui binary.

    Search order:
    1. sys.prefix bin/ (venv or system Python install)
    2. On PATH
    """
    # 1. Same environment as the Python that's running (venv bin/, system bin/)
    candidate = Path(sys.prefix) / "bin" / "extract-tui"
    if candidate.is_file():
        return str(candidate)

    # 2. Anywhere on PATH
    found = shutil.which("extract-tui")
    if found:
        return found

    return None


def _print_merge_stats(verb: str, src: object, dst: object, stats: dict[str, int]) -> None:
    print(f"{verb} {src} → {dst}")
    exps = stats.get("experiments", 0)
    runs = stats.get("runs", 0)
    if exps or runs:
        parts = []
        if exps:
            parts.append(f"{exps} experiments")
        if runs:
            parts.append(f"{runs} runs")
        print(f"  Merged: {', '.join(parts)}")
    else:
        print("  No new data (already up to date)")


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(prog="extract", description="Extract experiment tracker")
    sub = parser.add_subparsers(dest="command")

    # --- tui ---
    tui_parser = sub.add_parser("tui", help="Launch the TUI explorer")
    tui_parser.add_argument("--store", default=".extract", help="Path to .extract/ directory")

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

    # --- sync ---
    sync_parser = sub.add_parser("sync", help="Sync .extract/ between machines")
    sync_sub = sync_parser.add_subparsers(dest="action")

    push_p = sync_sub.add_parser("push", help="Push local store to remote via rsync")
    push_p.add_argument("remote", help="Remote path, e.g. user@hpc:/path/.extract/")
    push_p.add_argument("--root", default=".extract", help="Local .extract/ directory")

    pull_p = sync_sub.add_parser("pull", help="Pull remote store to local via rsync")
    pull_p.add_argument("remote", help="Remote path, e.g. user@hpc:/path/.extract/")
    pull_p.add_argument("--root", default=".extract", help="Local .extract/ directory")

    export_p = sync_sub.add_parser("export", help="Archive .extract/ to tar.gz")
    export_p.add_argument("output", help="Output archive path, e.g. experiments.tar.gz")
    export_p.add_argument("--root", default=".extract", help="Local .extract/ directory")

    import_p = sync_sub.add_parser("import", help="Import tar.gz archive into .extract/")
    import_p.add_argument("archive", help="Archive path to import")
    import_p.add_argument("--root", default=".extract", help="Target .extract/ directory")

    args = parser.parse_args(argv)

    if args.command == "tui":
        binary = _find_tui_binary()
        if binary is None:
            print(
                "error: extract-tui binary not found.\n"
                "If installed via pip, reinstall with: pip install extract-tracker\n"
                "For development, build with: cargo build --release -p extract-tui",
                file=sys.stderr,
            )
            sys.exit(1)
        os.execvp(binary, [binary, "--store", args.store])

    elif args.command == "init":
        from extract import init  # lazy import — pulls in rich/questionary only when needed
        sys.exit(init.run(args))

    elif args.command == "sync":
        from extract.sync import export_archive, import_archive, pull, push

        root = Path(args.root).resolve()

        if args.action == "push":
            push(root, args.remote)
            print(f"Pushed {root} → {args.remote}")
        elif args.action == "pull":
            stats = pull(root, args.remote)
            _print_merge_stats("Pulled", args.remote, root, stats)
        elif args.action == "export":
            output = Path(args.output).resolve()
            export_archive(root, output)
            print(f"Exported {root} → {output}")
        elif args.action == "import":
            archive = Path(args.archive).resolve()
            stats = import_archive(archive, root)
            _print_merge_stats("Imported", archive, root, stats)
        else:
            sync_parser.print_help()
            sys.exit(1)
    else:
        parser.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
