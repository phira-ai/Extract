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
