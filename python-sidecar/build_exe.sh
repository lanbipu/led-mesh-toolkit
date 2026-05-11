#!/usr/bin/env bash
# Build a single-file macOS arm64 executable with PyInstaller.
# Usage: ./build_exe.sh
# Output: target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENV="$SCRIPT_DIR/.venv-build"

if [[ ! -d "$VENV" ]]; then
    python3.12 -m venv "$VENV"
fi
# shellcheck disable=SC1091
source "$VENV/bin/activate"
pip install -e "$SCRIPT_DIR[dev]"

case "$(uname -m)" in
    arm64) PLATFORM="darwin-arm64" ;;
    x86_64) PLATFORM="darwin-x86_64" ;;
    *) echo "unsupported arch: $(uname -m)"; exit 1 ;;
esac

OUT="$ROOT/target/sidecar-vendor/$PLATFORM"
mkdir -p "$OUT"

pyinstaller \
    --onefile \
    --name lmt-vba-sidecar \
    --distpath "$OUT" \
    --workpath "$SCRIPT_DIR/build" \
    --specpath "$SCRIPT_DIR/build" \
    --collect-all cv2 \
    --collect-submodules scipy \
    --collect-submodules lmt_vba_sidecar \
    --paths "$SCRIPT_DIR/src" \
    "$SCRIPT_DIR/src/lmt_vba_sidecar/__main__.py"

echo "Built: $OUT/lmt-vba-sidecar"
