"""Helper functions for saving and loading metric artifacts."""

from __future__ import annotations

from pathlib import Path

import numpy as np


def save_npy(data: np.ndarray, path: Path) -> None:
    """Save a NumPy array to a .npy file, creating parent directories."""
    path.parent.mkdir(parents=True, exist_ok=True)
    np.save(path, data)


def load_npy(path: Path) -> np.ndarray:
    """Load a NumPy array from a .npy file."""
    return np.load(path)


def save_text(content: str, path: Path) -> None:
    """Save text content to a file, creating parent directories."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        f.write(content)
