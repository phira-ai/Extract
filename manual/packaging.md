# Packaging and Distribution

Primary distribution target: PyPI package `extract-tracker`.

Installed interfaces:

- Python import: `extract`
- CLI: `extract`
- bundled Rust binary: `extract-tui`

## Local development install

```bash
nix develop
pip install -e .
extract --help
```

`pip install -e .` builds the Rust TUI and installs the Python SDK in editable mode.

## Build locally

```bash
nix develop
python -m build
python -m twine check dist/*
pip install dist/*.whl
extract --help
```

## Source distribution

The source distribution includes:

- Python package under `python/src/extract`
- Rust crate under `rust/`
- `README.md`
- `manual/*.md`
- `MANIFEST.in`

Users installing from sdist need Rust toolchain available.

## Wheels

Rust binary means wheels are platform-specific. Build wheels for:

- Linux x86_64
- Linux aarch64
- macOS x86_64
- macOS arm64

Windows can be added after validating `extract tui` binary discovery and terminal behavior.

## Release checklist

1. Choose project license and add `LICENSE`.
2. Update version in `pyproject.toml` and `rust/Cargo.toml`.
3. Update changelog or release notes.
4. Run quality checks:

   ```bash
   nix develop
   pytest python/tests
   cargo test --manifest-path rust/Cargo.toml
   python -m build
   python -m twine check dist/*
   ```

5. Test wheel in clean environment:

   ```bash
   python -m venv /tmp/extract-test
   /tmp/extract-test/bin/pip install dist/*.whl
   /tmp/extract-test/bin/extract --help
   ```

6. Publish to TestPyPI.
7. Test install from TestPyPI.
8. Tag release: `vX.Y.Z`.
9. Publish to PyPI via trusted publishing.
10. Attach wheels and standalone `extract-tui` artifacts to GitHub Release if desired.

## PyPI trusted publishing

GitHub Actions workflow `.github/workflows/package.yml` builds sdists/wheels. Tag pushes matching `v*` publish to PyPI when repository trusted publishing is configured.

PyPI setup needed once:

1. Create PyPI project `extract-tracker`.
2. Add trusted publisher for this repository.
3. Environment: `pypi`.
4. Workflow name: `package.yml`.
5. Disable API tokens for routine releases.

## Name notes

Package name differs from import name:

```text
pip install extract-tracker
python -c "import extract"
extract --help
```

This is normal, but README and package metadata should consistently say:

- package: `extract-tracker`
- import: `extract`
- CLI: `extract`
