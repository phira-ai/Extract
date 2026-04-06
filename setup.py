from setuptools import setup
from setuptools_rust import RustBin
from wheel.bdist_wheel import bdist_wheel as _bdist_wheel


class bdist_wheel(_bdist_wheel):
    def finalize_options(self):
        _bdist_wheel.finalize_options(self)
        self.root_is_pure = False

    def get_tag(self):
        _, _, plat = _bdist_wheel.get_tag(self)
        return "py3", "none", plat


setup(
    rust_extensions=[
        RustBin(
            "extract-tui",
            path="rust/Cargo.toml",
        ),
    ],
    cmdclass={"bdist_wheel": bdist_wheel},
)
