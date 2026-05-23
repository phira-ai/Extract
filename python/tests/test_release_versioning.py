from pathlib import Path

import pytest
from extract.release_versioning import (
    bump_version,
    compute_target_version,
    sync_repo_versions,
    validate_exact_version,
)


def _write_release_files(root: Path, version: str = "0.1.0") -> None:
    (root / "rust").mkdir()
    (root / "pyproject.toml").write_text(
        f"""
[project]
name = "extract-tracker"
version = "{version}"
""".lstrip(),
        encoding="utf-8",
    )
    (root / "rust" / "Cargo.toml").write_text(
        f"""
[package]
name = "extract-tui"
version = "{version}"
edition = "2024"
""".lstrip(),
        encoding="utf-8",
    )
    (root / "uv.lock").write_text(
        f"""
[[package]]
name = "extract-tracker"
version = "{version}"
source = {{ editable = "." }}
""".lstrip(),
        encoding="utf-8",
    )
    (root / "rust" / "Cargo.lock").write_text(
        f"""
[[package]]
name = "extract-tui"
version = "{version}"
""".lstrip(),
        encoding="utf-8",
    )


def test_bump_version() -> None:
    assert bump_version("1.2.3", "patch") == "1.2.4"
    assert bump_version("1.2.3", "minor") == "1.3.0"
    assert bump_version("1.2.3", "major") == "2.0.0"


@pytest.mark.parametrize("requested", ["0.1.0", "0.0.9"])
def test_validate_exact_version_rejects_non_increase(requested: str) -> None:
    with pytest.raises(ValueError, match="greater than current"):
        validate_exact_version("0.1.0", requested)


def test_compute_target_version_exact(tmp_path: Path) -> None:
    _write_release_files(tmp_path)
    assert compute_target_version(tmp_path, "exact", "0.2.0") == "0.2.0"


def test_sync_repo_versions_updates_all_targets(tmp_path: Path) -> None:
    _write_release_files(tmp_path)

    changed = sync_repo_versions(tmp_path, "0.2.0", write=True)

    assert changed == [
        "pyproject.toml",
        "rust/Cargo.toml",
        "uv.lock",
        "rust/Cargo.lock",
    ]
    assert 'version = "0.2.0"' in (tmp_path / "pyproject.toml").read_text()
    assert 'version = "0.2.0"' in (tmp_path / "rust" / "Cargo.toml").read_text()
    assert 'version = "0.2.0"' in (tmp_path / "uv.lock").read_text()
    assert 'version = "0.2.0"' in (tmp_path / "rust" / "Cargo.lock").read_text()


def test_sync_repo_versions_check_detects_drift(tmp_path: Path) -> None:
    _write_release_files(tmp_path)
    (tmp_path / "rust" / "Cargo.toml").write_text(
        '[package]\nname = "extract-tui"\nversion = "0.2.0"\n',
        encoding="utf-8",
    )

    changed = sync_repo_versions(tmp_path, "0.1.0", write=False)

    assert changed == ["rust/Cargo.toml"]
