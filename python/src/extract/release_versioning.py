from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Literal

import tomli

ReleaseType = Literal["patch", "minor", "major", "exact"]

SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")

TARGETS: tuple[tuple[str, str, str, int], ...] = (
    ("pyproject.toml", "generic", r'(?m)^version = "\d+\.\d+\.\d+"$', 1),
    ("rust/Cargo.toml", "generic", r'(?m)^version = "\d+\.\d+\.\d+"$', 1),
    ("uv.lock", "uv", "extract-tracker", 1),
    ("rust/Cargo.lock", "cargo-lock", "extract-tui", 1),
)


def _parse_semver(version: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(version)
    if match is None:
        raise ValueError(f"Invalid semantic version: {version}")
    major, minor, patch = match.groups()
    return int(major), int(minor), int(patch)


def _discover_repo_root(start: Path | None = None) -> Path:
    current = (start or Path.cwd()).resolve()
    for candidate in (current, *current.parents):
        if (candidate / "pyproject.toml").exists():
            return candidate
    raise ValueError(f"Could not find pyproject.toml from {current}")


def _extract_version(text: str) -> str:
    match = re.search(r'"(\d+\.\d+\.\d+)"', text)
    if match is None:
        raise ValueError(f"Could not extract version from: {text}")
    return match.group(1)


def _project_version(repo_root: Path) -> str:
    pyproject = repo_root / "pyproject.toml"
    data = tomli.loads(pyproject.read_text(encoding="utf-8"))
    try:
        version = data["project"]["version"]
    except KeyError as exc:
        raise ValueError(f"Missing project.version in {pyproject}") from exc
    if not isinstance(version, str):
        raise ValueError(f"Invalid project.version in {pyproject}")
    _parse_semver(version)
    return version


def bump_version(current: str, release_type: ReleaseType) -> str:
    major, minor, patch = _parse_semver(current)
    if release_type == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if release_type == "minor":
        return f"{major}.{minor + 1}.0"
    if release_type == "major":
        return f"{major + 1}.0.0"
    raise ValueError(f"Unsupported release type for bump_version: {release_type}")


def validate_exact_version(current: str, requested: str) -> str:
    current_parts = _parse_semver(current)
    requested_parts = _parse_semver(requested)
    if requested_parts <= current_parts:
        raise ValueError(
            f"Exact version must be greater than current version {current}: {requested}"
        )
    return requested


def compute_target_version(
    repo_root: Path, release_type: ReleaseType, exact_version: str | None
) -> str:
    current = _project_version(repo_root)
    if release_type == "exact":
        if exact_version is None:
            raise ValueError("--exact-version is required for --release-type exact")
        return validate_exact_version(current, exact_version)
    return bump_version(current, release_type)


def _replace_generic_version(
    original: str,
    relative_path: str,
    pattern: str,
    expected_count: int,
    target_version: str,
) -> tuple[str, bool]:
    rewritten, count = re.subn(
        pattern,
        lambda match: match.group(0).replace(
            _extract_version(match.group(0)), target_version
        ),
        original,
    )
    if count != expected_count:
        raise ValueError(
            f"{relative_path} replacement count mismatch: "
            f"expected {expected_count}, got {count}"
        )
    return rewritten, rewritten != original


def _replace_named_package_version(
    original: str,
    relative_path: str,
    package_name: str,
    expected_count: int,
    target_version: str,
) -> tuple[str, bool]:
    pattern = re.compile(
        rf'(\[\[package\]\]\nname = "{re.escape(package_name)}"\nversion = ")'
        rf'(?P<version>\d+\.\d+\.\d+)(")'
    )
    count = len(pattern.findall(original))
    if count != expected_count:
        raise ValueError(
            f"{relative_path} replacement count mismatch: "
            f"expected {expected_count}, got {count}"
        )
    rewritten = pattern.sub(rf"\g<1>{target_version}\3", original)
    return rewritten, rewritten != original


def _replace_version(
    path: Path,
    relative_path: str,
    kind: str,
    pattern_or_package: str,
    expected_count: int,
    target_version: str,
) -> tuple[str, bool]:
    try:
        original = path.read_text(encoding="utf-8")
    except FileNotFoundError as exc:
        raise ValueError(f"Missing target file: {relative_path}") from exc

    if kind == "generic":
        return _replace_generic_version(
            original, relative_path, pattern_or_package, expected_count, target_version
        )
    if kind in {"uv", "cargo-lock"}:
        return _replace_named_package_version(
            original, relative_path, pattern_or_package, expected_count, target_version
        )
    raise ValueError(f"Unsupported version target kind: {kind}")


def _atomic_write_files(pending_writes: list[tuple[Path, str]]) -> None:
    staged_files: list[tuple[Path, Path, Path]] = []
    for index, (path, rewritten) in enumerate(pending_writes):
        temp_path = path.with_name(f".__extract_versioning.{index}.{path.name}.tmp")
        backup_path = path.with_name(f".__extract_backup.{index}.{path.name}.bak")
        if temp_path.exists():
            temp_path.unlink()
        if backup_path.exists():
            backup_path.unlink()
        temp_path.write_text(rewritten, encoding="utf-8")
        staged_files.append((path, temp_path, backup_path))

    moved_backups: list[tuple[Path, Path]] = []
    try:
        for path, _temp_path, backup_path in staged_files:
            path.replace(backup_path)
            moved_backups.append((path, backup_path))
        for path, temp_path, _backup_path in staged_files:
            temp_path.replace(path)
    except Exception:
        for _path, temp_path, _backup_path in staged_files:
            if temp_path.exists():
                temp_path.unlink()
        for path, backup_path in reversed(moved_backups):
            if path.exists():
                path.unlink()
            if backup_path.exists():
                backup_path.replace(path)
        raise
    else:
        for _path, _temp_path, backup_path in staged_files:
            if backup_path.exists():
                backup_path.unlink()


def sync_repo_versions(repo_root: Path, target_version: str, write: bool) -> list[str]:
    _parse_semver(target_version)
    changed_files: list[str] = []
    pending_writes: list[tuple[Path, str]] = []
    for relative_path, kind, pattern_or_package, expected_count in TARGETS:
        path = repo_root / relative_path
        rewritten, changed = _replace_version(
            path,
            relative_path,
            kind,
            pattern_or_package,
            expected_count,
            target_version,
        )
        if changed:
            changed_files.append(relative_path)
            pending_writes.append((path, rewritten))
    if write:
        _atomic_write_files(pending_writes)
    return changed_files


def _write_github_output(path: str | None, *, target_version: str) -> None:
    if path is None:
        return
    with Path(path).open("a", encoding="utf-8") as handle:
        handle.write(f"target_version={target_version}\n")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="python -m extract.release_versioning")
    parser.add_argument(
        "--release-type",
        choices=("patch", "minor", "major", "exact"),
        default="patch",
    )
    parser.add_argument("--exact-version")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--github-output")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    try:
        args = parser.parse_args(argv)
        repo_root = _discover_repo_root()
        current = _project_version(repo_root)

        if args.check:
            changed_files = sync_repo_versions(repo_root, current, write=False)
            if changed_files:
                print(f"DRIFT {current}", file=sys.stdout)
                for changed_file in changed_files:
                    print(changed_file, file=sys.stdout)
                return 1
            print(f"OK {current}", file=sys.stdout)
            return 0

        if args.release_type == "exact" and args.exact_version is None:
            parser.error("--exact-version is required for --release-type exact")

        target_version = compute_target_version(
            repo_root, args.release_type, args.exact_version
        )
        changed_files = sync_repo_versions(
            repo_root, target_version, write=not args.dry_run
        )
        _write_github_output(args.github_output, target_version=target_version)
        mode = "DRY_RUN" if args.dry_run else "UPDATED"
        print(f"{mode} {target_version}", file=sys.stdout)
        for changed_file in changed_files:
            print(changed_file, file=sys.stdout)
        return 0
    except SystemExit as exc:
        if isinstance(exc.code, int):
            return exc.code
        return 1
    except (OSError, ValueError) as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
