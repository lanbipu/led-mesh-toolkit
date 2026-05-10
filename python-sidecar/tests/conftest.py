"""Shared pytest fixtures."""
from __future__ import annotations

import pathlib

import pytest


@pytest.fixture
def tmp_out(tmp_path: pathlib.Path) -> pathlib.Path:
    """Return a clean tmp path for sidecar outputs (no setup)."""
    return tmp_path
