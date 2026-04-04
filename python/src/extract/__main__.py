"""CLI entry point: python -m extract."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path


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

    if args.command == "sync":
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
