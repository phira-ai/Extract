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

## Release workflow

Use `.github/workflows/release.yml` from the Actions UI.

Inputs:

- `release_type`: `patch`, `minor`, `major`, or `exact`
- `exact_version`: required only when `release_type=exact`
- `validate_only`: runs rehearsal without commit/tag

The workflow:

1. Requires `refs/heads/main`.
2. Verifies checkout equals latest `origin/main`.
3. Computes the next version from `pyproject.toml`.
4. Syncs versions across:
   - `pyproject.toml`
   - `rust/Cargo.toml`
   - `uv.lock`
   - `rust/Cargo.lock`
5. Runs Python and Rust tests.
6. Builds and checks Python distributions.
7. Commits `chore: release vX.Y.Z`.
8. Tags `vX.Y.Z`.
9. Pushes commit and tag atomically to `main`.

Tag push triggers `.github/workflows/package.yml`, which builds wheels and publishes to PyPI.

## Local release checks

```bash
nix develop
uv run python -m extract.release_versioning --release-type patch --dry-run
uv run pytest python/tests
cargo test --manifest-path rust/Cargo.toml
python -m build
python -m twine check dist/*
```

Test wheel in clean environment:

```bash
python -m venv /tmp/extract-test
/tmp/extract-test/bin/pip install dist/*.whl
/tmp/extract-test/bin/extract --help
```

## GitHub repository setup

### Actions permissions

Repository settings:

1. Open **Settings → Actions → General**.
2. Under **Actions permissions**, allow GitHub-hosted actions and the third-party actions used by the workflows:
   - `actions/checkout`
   - `actions/setup-python`
   - `astral-sh/setup-uv`
   - `dtolnay/rust-toolchain`
   - `pypa/gh-action-pypi-publish`
3. Under **Workflow permissions**, select **Read and write permissions**.

The workflows request their own narrowed job permissions, but the repository-level workflow-permission ceiling must still allow write access for release commits and tags.

### Environments

Create these repository environments under **Settings → Environments**:

- `pypi`
- `testpypi`

Recommended protection:

- `pypi`: require a reviewer before deployment.
- `testpypi`: no reviewer, or reviewer optional.

Do not store PyPI API tokens in these environments for normal releases. Publishing uses trusted publishing / OIDC.

### Release push token

Create repository secret `RELEASE_WORKFLOW_TOKEN` under **Settings → Secrets and variables → Actions → Repository secrets**.

Use a fine-grained personal access token with:

- Resource owner: same owner as this repository.
- Repository access: only this repository.
- Permissions:
  - **Contents: Read and write**
  - **Metadata: Read-only**

This secret must be a repository Actions secret, not an environment secret, because `.github/workflows/release.yml` does not run inside an environment.

Reason: `.github/workflows/release.yml` creates the release commit and tag. A tag pushed with the default `GITHUB_TOKEN` does not trigger the separate package-publish workflow, so the release workflow must push with a PAT-backed token.

### Branch protection / rulesets

If `main` is protected, allow the PAT owner to push the release commit and tag. Depending on current GitHub ruleset UI, configure one of:

- ruleset bypass for the PAT owner,
- branch protection bypass for administrators, if the PAT owner is an admin,
- or a dedicated release bot/user with permission to push to `main`.

The release workflow pushes directly to `main`; it does not open a pull request.

## PyPI trusted publishing

GitHub Actions workflow `.github/workflows/package.yml` builds sdists/wheels. Tag pushes matching `v*` publish to PyPI when trusted publishing is configured.

PyPI setup needed once:

1. In PyPI, create project `extract-tracker`, or create a pending trusted publisher for the first upload.
2. Add a GitHub trusted publisher with:
   - owner: GitHub user/org that owns this repo
   - repository: this repo name
   - workflow filename: `package.yml`
   - environment: `pypi`
3. In TestPyPI, add the same trusted publisher but with environment `testpypi`.
4. Do not configure `PYPI_API_TOKEN` for routine releases.

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
