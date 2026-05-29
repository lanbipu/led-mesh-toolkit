import pytest
from lmt_vba_sidecar.ipc import CorrespondenceFile
from lmt_vba_sidecar.sl_reconstruct import validate_sl_provenance


def _corr(screen_id="MAIN", sha="abc"):
    return CorrespondenceFile.model_validate({
        "schema_version": 1, "screen_id": screen_id, "sl_meta_sha256": sha,
        "screen_resolution": [960, 540], "camera_image_size": [4000, 3000],
        "source_input": "/cap/p.mp4",
        "points": [{"id": 0, "u": 1.0, "v": 2.0, "x": 3.0, "y": 4.0}]})


def test_provenance_accepts_consistent_set():
    validate_sl_provenance([_corr(), _corr()], expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_mixed_screen_id():
    with pytest.raises(ValueError, match="screen_id"):
        validate_sl_provenance([_corr(screen_id="MAIN"), _corr(screen_id="FLOOR")],
                               expected_sha="abc", expected_screen_id="MAIN")


def test_provenance_rejects_sha_mismatch_vs_meta():
    with pytest.raises(ValueError, match="sl_meta_sha256"):
        validate_sl_provenance([_corr(sha="abc")], expected_sha="DIFFERENT",
                               expected_screen_id="MAIN")


def test_provenance_rejects_screen_id_not_matching_project():
    with pytest.raises(ValueError, match="project"):
        validate_sl_provenance([_corr(screen_id="MAIN")], expected_sha="abc",
                               expected_screen_id="FLOOR")
