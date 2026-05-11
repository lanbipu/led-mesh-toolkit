"""CLI entry: read JSON command from stdin, dispatch, emit NDJSON events."""
from __future__ import annotations

import argparse
import json
import sys
import traceback

from pydantic import ValidationError

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    CalibrateInput,
    ErrorEvent,
    GeneratePatternInput,
    ReconstructInput,
)


def _emit_error(code: str, message: str) -> None:
    write_event(ErrorEvent(event="error", code=code, message=message, fatal=True))


def _read_stdin_json() -> dict:
    raw = sys.stdin.read()
    return json.loads(raw)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lmt-vba-sidecar")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("calibrate")
    sub.add_parser("generate_pattern")
    sub.add_parser("reconstruct")
    args = parser.parse_args(argv)

    try:
        raw = _read_stdin_json()
    except json.JSONDecodeError as exc:
        _emit_error("invalid_input", f"stdin is not valid JSON: {exc}")
        return 1

    # Map subcommand → (input model, lazy-importer) so we only complain
    # "not yet implemented" when the SUBCOMMAND module itself is missing —
    # a transitive ImportError (e.g. cv2 unavailable) should fall through
    # to the internal-error path with a real traceback so packaging /
    # version-skew bugs don't masquerade as feature absence.
    SUBCOMMAND_MODULES = {
        "calibrate": "lmt_vba_sidecar.calibrate",
        "generate_pattern": "lmt_vba_sidecar.pattern",
        "reconstruct": "lmt_vba_sidecar.reconstruct",
    }
    SUBCOMMAND_ENTRYPOINTS = {
        "calibrate": ("run_calibrate", CalibrateInput),
        "generate_pattern": ("run_generate_pattern", GeneratePatternInput),
        "reconstruct": ("run_reconstruct", ReconstructInput),
    }

    try:
        cmd_model_cls = SUBCOMMAND_ENTRYPOINTS[args.command][1]
        cmd_obj = cmd_model_cls.model_validate(raw)
    except ValidationError as exc:
        _emit_error("invalid_input", f"input did not match schema: {exc}")
        return 1

    module_name = SUBCOMMAND_MODULES[args.command]
    fn_name = SUBCOMMAND_ENTRYPOINTS[args.command][0]
    try:
        import importlib
        module = importlib.import_module(module_name)
    except ModuleNotFoundError as exc:
        if exc.name == module_name:
            _emit_error(
                "internal_error",
                f"subcommand `{args.command}` not yet implemented (module {module_name} missing)",
            )
            return 1
        # Different module missing → real dependency failure, not stub absence.
        tb = traceback.format_exc()
        _emit_error("internal_error", f"dependency import failed for {args.command}: {exc}\n{tb}")
        return 1
    except Exception as exc:
        tb = traceback.format_exc()
        _emit_error("internal_error", f"failed loading {args.command}: {exc}\n{tb}")
        return 1

    try:
        run_fn = getattr(module, fn_name)
    except AttributeError:
        _emit_error("internal_error", f"module {module_name} missing entry point `{fn_name}`")
        return 1

    try:
        return run_fn(cmd_obj)
    except Exception as exc:
        tb = traceback.format_exc()
        _emit_error("internal_error", f"{exc}\n{tb}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
