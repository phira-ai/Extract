"""Tests for the extract init module."""

from __future__ import annotations

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

    def test_round_trips_via_tomllib(self, tmp_path):
        """The config we write must be parseable as valid TOML."""
        try:
            import tomllib
        except ImportError:
            import tomli as tomllib

        store_root = tmp_path / ".extract"
        init._write_config(store_root, ["benchmark", "model", "variant"])

        with open(store_root / "config.toml", "rb") as f:
            parsed = tomllib.load(f)
        assert parsed["store"]["hierarchy"] == "benchmark > model > variant"


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
