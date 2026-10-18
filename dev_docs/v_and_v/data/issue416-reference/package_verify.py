"""収載ファイルの完全性と既存結果の対応だけを検証する。求解は行わない。"""

import argparse
import hashlib
import json
import math
from pathlib import Path


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-sources", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    manifest = read_json(root / "manifest.json")
    listed = {entry["path"] for entry in manifest["files"]}
    actual = {
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_file() and path.name != "manifest.json"
    }
    assert listed == actual, (listed - actual, actual - listed)
    for entry in manifest["files"]:
        path = (root / entry["path"]).resolve()
        assert path.is_relative_to(root)
        assert len(path.read_bytes()) == entry["bytes"], entry["path"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == entry["sha256"], entry["path"]
        if entry["kind"] == "byte-identical-copy":
            assert entry["sha256"] == entry["source_sha256"]
        if args.check_sources and "source_path" in entry:
            source = Path(entry["source_path"])
            assert hashlib.sha256(source.read_bytes()).hexdigest() == entry["source_sha256"], source
        if path.suffix == ".json":
            read_json(path)

    def data(folder, name):
        return read_json(root / folder / name)

    env = "sepika416-reference-environment"
    calibration = "sepika416-reference-calibration"
    old = "sepika416-h-box-reference"
    reproduction = "sepika416-reference-reproduction"
    core = "sepika416-core-reference"
    assert len(data(env, "calibration-results.json")) == 6
    calibration_rows = data(calibration, "results.json") + data(calibration, "refinement/results.json")
    renamed_rows = []
    for row in calibration_rows:
        row = dict(row)
        row["residual_within_1e_minus_10"] = row.pop("residual_within_1e10")
        renamed_rows.append(row)
    assert renamed_rows == data(calibration, "all-results.json")
    assert len(renamed_rows) == 25

    old_rows = data(old, "results.json")
    new_rows = data(reproduction, "results.json")
    comparisons = data(reproduction, "old-new-comparison.json")
    assert len(old_rows) == len(new_rows) == len(comparisons) == 31
    for index, (before, after, comparison, setting) in enumerate(
        zip(old_rows, new_rows, comparisons, data(reproduction, "settings.json"), strict=True)
    ):
        assert after["row_index"] == comparison["row_index"] == index
        for key in ("shape", "series", "boundary_segments_per_quadrant", "normalized_max_area_setting"):
            assert before[key] == after[key] == setting[key]
        assert comparison["old_j_mm4"] == before["j_mm4"]
        assert comparison["new_j_mm4"] == after["j_mm4"]
        assert comparison["old_mesh_sha256"] == before["mesh_sha256"]
        assert comparison["new_mesh_sha256"] == after["mesh_sha256"]
    assert sum(row["mesh_bytes_equal_to_old_hash"] for row in comparisons) == 17

    raw = data(core, "results.json")
    audited = data(core, "audited-results.json")
    corrected = data(core, "corrected-metadata-results.json")
    correction = data(core, "axis-mapping-correction.json")
    assert len(raw) == len(audited) == len(corrected) == 12
    assert correction["correct_metadata"] == "(x_ref,y_ref)=(SEPIKA element local z,y)"
    for initial, before, after in zip(raw, audited, corrected, strict=True):
        assert all(before[key] == value for key, value in initial.items())
        assert before["axis_mapping"] == correction["old_metadata"]
        assert after["axis_mapping"] == correction["correct_metadata"]
        assert {key: value for key, value in before.items() if key != "axis_mapping"} == {
            key: value for key, value in after.items()
            if key not in ("axis_mapping", "axis_mapping_correction")
        }
    for folder, rows in ((reproduction, new_rows), (core, raw)):
        for row in rows:
            assert row["status"] == "ok"
            assert math.isfinite(row["j_mm4"]) and row["j_mm4"] > 0
            assert row["original_relative_residual"] <= 1e-10
            assert row["mesh_sha256_before"] == row["mesh_sha256_after"]
        for check in data(folder, "savedmesh-reload-checks.json"):
            row = rows[check["row_index"]]
            assert check["source_j_mm4"] == check["reloaded_j_mm4"] == row["j_mm4"]
            assert check["mesh_sha256"] == row["mesh_sha256_before"]
            assert check["mesh_generated"] is False
    print(json.dumps({
        "files_checked": len(listed),
        "source_files_checked": args.check_sources,
        "historical_rows": {"bootstrap": 6, "calibration": 25, "h_box_old": 31,
                            "h_box_reproduction": 31, "core": 12},
        "new_numerical_solve": False,
        "mesh_arrays_checked": False,
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
