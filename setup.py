from setuptools import setup
from setuptools_rust import RustBin

setup(
    rust_extensions=[
        RustBin(
            "extract-tui",
            path="rust/Cargo.toml",
        ),
    ],
)
