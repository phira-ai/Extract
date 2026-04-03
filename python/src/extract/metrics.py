"""Helper functions for saving and loading metric artifacts."""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np


def save_npy(data: np.ndarray, path: Path) -> None:
    """Save a NumPy array to a .npy file, creating parent directories."""
    path.parent.mkdir(parents=True, exist_ok=True)
    np.save(path, data)


def load_npy(path: Path) -> np.ndarray:
    """Load a NumPy array from a .npy file."""
    return np.load(path)


def save_timeseries(steps: list, values: list, path: Path) -> None:
    """Save a timeseries as JSON with steps and values arrays."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump({"steps": steps, "values": values}, f)


def load_timeseries(path: Path) -> dict:
    """Load a timeseries from a JSON file."""
    with open(path) as f:
        return json.load(f)


def save_text(content: str, path: Path) -> None:
    """Save text content to a file, creating parent directories."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        f.write(content)
