import struct

import numpy as np
import pytest

from _dpx_fixtures import write_dpx, DPX_HEADER_SIZE
from lmt_vba_sidecar.dpx import read_dpx_gray8


def test_roundtrip_recovers_grayscale_le(tmp_path):
    rng = np.random.default_rng(0)
    g = rng.integers(0, 256, size=(7, 11), dtype=np.uint8)
    p = tmp_path / "f.dpx"
    write_dpx(p, g)
    out = read_dpx_gray8(p)
    assert out.dtype == np.uint8 and out.shape == (7, 11)
    np.testing.assert_array_equal(out, g)


def test_roundtrip_big_endian(tmp_path):
    g = np.array([[0, 64, 128], [192, 255, 1]], np.uint8)
    p = tmp_path / "be.dpx"
    write_dpx(p, g, endian=">")
    np.testing.assert_array_equal(read_dpx_gray8(p), g)


def test_fixture_header_matches_real_disguise_layout(tmp_path):
    # Pin the fixture writer to the verified real-sample offsets so the tests
    # below decode disguise-shaped bytes, not an ad-hoc format.
    p = tmp_path / "h.dpx"
    write_dpx(p, np.zeros((4, 5), np.uint8))
    raw = p.read_bytes()
    assert raw[:4] == b"XPDS"
    assert struct.unpack_from("<I", raw, 4)[0] == DPX_HEADER_SIZE == 8192
    assert struct.unpack_from("<I", raw, 772)[0] == 5   # width
    assert struct.unpack_from("<I", raw, 776)[0] == 4   # height
    assert raw[800] == 50
    assert raw[803] == 10
    assert struct.unpack_from("<H", raw, 804)[0] == 1
    assert struct.unpack_from("<H", raw, 806)[0] == 0


def _make_bad(tmp_path, mutate):
    g = np.zeros((4, 5), np.uint8)
    p = tmp_path / "bad.dpx"
    write_dpx(p, g)
    raw = bytearray(p.read_bytes())
    mutate(raw)
    p.write_bytes(bytes(raw))
    return p


def test_raises_on_bad_magic(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(slice(0, 4), b"FAKE"))
    with pytest.raises(ValueError, match="not a DPX"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_bit_depth(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(803, 12))
    with pytest.raises(ValueError, match="bit depth"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_descriptor(tmp_path):
    p = _make_bad(tmp_path, lambda r: r.__setitem__(800, 6))  # 6 = luma
    with pytest.raises(ValueError, match="descriptor"):
        read_dpx_gray8(p)


def test_raises_on_unsupported_packing(tmp_path):
    p = _make_bad(tmp_path, lambda r: struct.pack_into("<H", r, 804, 0))
    with pytest.raises(ValueError, match="packing"):
        read_dpx_gray8(p)


def test_raises_on_rle_encoding(tmp_path):
    p = _make_bad(tmp_path, lambda r: struct.pack_into("<H", r, 806, 1))
    with pytest.raises(ValueError, match="RLE"):
        read_dpx_gray8(p)


def test_raises_on_truncated_pixels(tmp_path):
    g = np.zeros((4, 5), np.uint8)
    p = tmp_path / "trunc.dpx"
    write_dpx(p, g)
    raw = p.read_bytes()
    p.write_bytes(raw[:-8])  # drop two pixel words
    with pytest.raises(ValueError, match="truncated"):
        read_dpx_gray8(p)
