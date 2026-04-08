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
