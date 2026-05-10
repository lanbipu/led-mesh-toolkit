"""CLI entry point. Subcommand routing only at this point."""
from __future__ import annotations

import argparse
import sys


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="lmt-vba-sidecar")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("calibrate", help="Run camera calibration")
    sub.add_parser("generate_pattern", help="Generate ChArUco patterns")
    sub.add_parser("reconstruct", help="Run BA + reconstruct MeasuredPoints")
    args = parser.parse_args(argv)
    print(f"sidecar {args.command} not yet implemented", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
