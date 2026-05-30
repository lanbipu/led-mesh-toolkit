"""Read disguise 10-bit Method-A DPX frames -> 8-bit grayscale.

Pure numpy + cv2 (no extra deps, PyInstaller-safe, no runtime ffmpeg). Scoped to
the disguise variant verified against a real sample; raises ValueError on anything
else rather than silently misdecoding. Unpack formula verified pixel-exact vs
ffmpeg: R=(w>>22)&0x3FF, G=(w>>12)&0x3FF, B=(w>>2)&0x3FF (2 pad bits at LSB).
"""
from __future__ import annotations

import struct
from pathlib import Path

import cv2
import numpy as np

_DESCRIPTOR_RGB = 50
_PACKING_METHOD_A = 1
_ENCODING_NONE = 0
_BIT_DEPTH = 10
_MIN_HEADER = 812  # last field we read is element data offset at 808..812


def read_dpx_gray8(path) -> np.ndarray:
    """Return an (H, W) uint8 grayscale frame from a disguise 10-bit RGB Method-A
    DPX. Raises ValueError on any non-disguise variant or truncation."""
    raw = Path(path).read_bytes()
    if len(raw) < _MIN_HEADER:
        raise ValueError(f"{path}: file too small to be a DPX ({len(raw)} bytes)")

    magic = raw[:4]
    if magic == b"XPDS":
        end = "<"
    elif magic == b"SDPX":
        end = ">"
    else:
        raise ValueError(f"{path}: not a DPX (magic {magic!r})")

    data_off = struct.unpack_from(end + "I", raw, 4)[0]
    width = struct.unpack_from(end + "I", raw, 772)[0]
    height = struct.unpack_from(end + "I", raw, 776)[0]
    descriptor = raw[800]
    bit_depth = raw[803]
    packing = struct.unpack_from(end + "H", raw, 804)[0]
    encoding = struct.unpack_from(end + "H", raw, 806)[0]

    if bit_depth != _BIT_DEPTH:
        raise ValueError(f"{path}: unsupported DPX bit depth {bit_depth} (only 10 supported)")
    if descriptor != _DESCRIPTOR_RGB:
        raise ValueError(f"{path}: unsupported DPX descriptor {descriptor} (only 50=RGB supported)")
    if packing != _PACKING_METHOD_A:
        raise ValueError(f"{path}: unsupported DPX packing {packing} (only 1=Method A supported)")
    if encoding != _ENCODING_NONE:
        raise ValueError(f"{path}: RLE-encoded DPX not supported (encoding={encoding})")
    if width == 0 or height == 0:
        raise ValueError(f"{path}: bad DPX dimensions {width}x{height}")

    need = width * height * 4  # Method A: one 32-bit word per RGB pixel
    if len(raw) < data_off + need:
        raise ValueError(
            f"{path}: truncated DPX pixel data (need {data_off + need} bytes, have {len(raw)})"
        )

    words = np.frombuffer(raw[data_off:data_off + need], dtype=end + "u4").reshape(height, width)
    r8 = (((words >> 22) & 0x3FF) >> 2).astype(np.uint8)  # 10-bit -> 8-bit
    g8 = (((words >> 12) & 0x3FF) >> 2).astype(np.uint8)
    b8 = (((words >> 2) & 0x3FF) >> 2).astype(np.uint8)
    rgb8 = np.stack([r8, g8, b8], axis=-1)
    return cv2.cvtColor(rgb8, cv2.COLOR_RGB2GRAY)  # BT.601 luma, matches IMREAD_GRAYSCALE
