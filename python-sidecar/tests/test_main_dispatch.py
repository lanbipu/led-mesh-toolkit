"""End-to-end CLI dispatch tests via subprocess."""
from __future__ import annotations

import json
import subprocess
import sys


def _run_cli(args: list[str], stdin_payload: str) -> tuple[int, str, str]:
    proc = subprocess.run(
        [sys.executable, "-m", "lmt_vba_sidecar", *args],
        input=stdin_payload,
        capture_output=True,
        text=True,
        timeout=30,
    )
    return proc.returncode, proc.stdout, proc.stderr


def test_invalid_json_emits_error_event_and_exits_nonzero() -> None:
    code, out, _ = _run_cli(["reconstruct"], "not-json")
    assert code != 0
    last = json.loads(out.strip().splitlines()[-1])
    assert last["event"] == "error"
    assert last["code"] == "invalid_input"


def test_unknown_command_argparse_fails() -> None:
    code, _, err = _run_cli(["bogus"], "")
    assert code != 0
    assert "invalid choice" in err


def test_validation_error_emits_invalid_input() -> None:
    payload = json.dumps({
        "command": "reconstruct",
        "version": 1,
        "project": {},  # empty -> fails schema
        "images": [],
        "intrinsics": {},
        "pattern_meta": {},
    })
    code, out, _ = _run_cli(["reconstruct"], payload)
    assert code != 0
    last = json.loads(out.strip().splitlines()[-1])
    assert last["event"] == "error"
    assert last["code"] == "invalid_input"


def test_missing_subcommand_module_returns_not_implemented(tmp_path, monkeypatch) -> None:
    """When the subcommand module itself is absent, error message says
    'not yet implemented' (not generic internal_error)."""
    import importlib
    import io

    real_import_module = importlib.import_module
    target = "lmt_vba_sidecar.calibrate"

    def fake_import(name, *args, **kwargs):
        if name == target:
            raise ModuleNotFoundError(name=target)
        return real_import_module(name, *args, **kwargs)

    monkeypatch.setattr(importlib, "import_module", fake_import)

    from lmt_vba_sidecar import __main__ as m

    fake_stdin = io.StringIO(json.dumps({
        "command": "calibrate",
        "version": 1,
        "checkerboard_images": ["a.png"] * 5,
        "inner_corners": [8, 6],
        "square_size_mm": 20.0,
        "output_path": str(tmp_path / "ix.json"),
    }))
    monkeypatch.setattr(sys, "stdin", fake_stdin)
    captured = io.StringIO()
    monkeypatch.setattr(sys, "stdout", captured)
    rc = m.main(["calibrate"])
    assert rc == 1
    last = json.loads(captured.getvalue().strip().splitlines()[-1])
    assert last["event"] == "error"
    assert "not yet implemented" in last["message"]


def test_transitive_import_failure_does_not_say_not_implemented(tmp_path, monkeypatch) -> None:
    """A dependency import failure (different module name than the subcommand
    module) must propagate as internal_error with traceback, NOT as
    'not yet implemented'."""
    import importlib
    import io

    real_import_module = importlib.import_module
    subcommand_module = "lmt_vba_sidecar.calibrate"

    def fake_import(name, *args, **kwargs):
        if name == subcommand_module:
            # The subcommand module IS importable, but during its import
            # a different dep is missing.
            raise ModuleNotFoundError(name="some_required_dep")
        return real_import_module(name, *args, **kwargs)

    monkeypatch.setattr(importlib, "import_module", fake_import)

    from lmt_vba_sidecar import __main__ as m
    fake_stdin = io.StringIO(json.dumps({
        "command": "calibrate",
        "version": 1,
        "checkerboard_images": ["a.png"] * 5,
        "inner_corners": [8, 6],
        "square_size_mm": 20.0,
        "output_path": str(tmp_path / "ix.json"),
    }))
    monkeypatch.setattr(sys, "stdin", fake_stdin)
    captured = io.StringIO()
    monkeypatch.setattr(sys, "stdout", captured)
    rc = m.main(["calibrate"])
    assert rc == 1
    last = json.loads(captured.getvalue().strip().splitlines()[-1])
    assert last["event"] == "error"
    assert "not yet implemented" not in last["message"]
    assert "some_required_dep" in last["message"]
