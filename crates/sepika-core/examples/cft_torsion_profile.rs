use std::collections::HashMap;
use std::error::Error;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sepika_core::section_shape::{
    CftCoreTorsion, CoreTorsionAttempt, CoreTorsionLevel, SectionShape,
};
use serde_json::{json, Value};

#[derive(Clone)]
struct Case {
    label: String,
    width_mm: f64,
    height_mm: f64,
    thick_mm: f64,
    outer_r_mm: f64,
}

fn grid() -> Vec<Case> {
    let mut cases = Vec::new();
    for (ratio_name, ratio) in [("1", 1.0), ("4_3", 4.0 / 3.0), ("2", 2.0), ("4", 4.0)] {
        for thickness_ratio in [10, 20, 50, 100] {
            let thick_mm = 300.0 / f64::from(thickness_ratio);
            for (radius_name, outer_r_mm) in [
                ("zero", 0.0),
                ("t", thick_mm),
                ("t_plus", thick_mm * (1.0 + 1e-6)),
                ("2_5t", 2.5 * thick_mm),
                ("upper_minus", 150.0 * (1.0 - 1e-6)),
                ("upper", 150.0),
            ] {
                cases.push(Case {
                    label: format!("hb{ratio_name}_bt{thickness_ratio}_r{radius_name}"),
                    width_mm: 300.0,
                    height_mm: 300.0 * ratio,
                    thick_mm,
                    outer_r_mm,
                });
            }
        }
    }
    cases
}

fn attempt(value: CoreTorsionAttempt) -> Value {
    json!({
        "quarter_arc_segments": value.quarter_arc_segments,
        "max_triangle_area": value.max_triangle_area,
        "max_vertices": value.max_vertices,
        "max_triangles": value.max_triangles,
    })
}

fn level(value: &CoreTorsionLevel) -> Value {
    json!({
        "attempt": attempt(value.attempt),
        "j_hat": value.j_hat,
        "relative_residual": value.relative_residual,
        "free_dofs": value.free_dofs,
        "nnz": value.nnz,
        "min_phi": value.min_phi,
        "max_phi": value.max_phi,
        "polygon_area": value.polygon_area,
        "triangle_area": value.triangle_area,
        "max_actual_area": value.max_actual_area,
        "min_signed_double_area": value.min_signed_double_area,
        "min_area_edge_ratio": value.min_area_edge_ratio,
        "min_angle_rad": value.min_angle_rad,
        "original_boundary_segments": value.original_boundary_segments,
        "boundary_edges": value.boundary_edges,
        "vertices": value.vertices,
        "triangles": value.triangles,
        "fingerprint": value.fingerprint.to_string(),
        "mesh_seconds": value.mesh_seconds,
        "assembly_seconds": value.assembly_seconds,
        "factorization_seconds": value.factorization_seconds,
        "solve_seconds": value.solve_seconds,
    })
}

fn diagnose(case: &Case, swapped: bool) -> Value {
    let (width, height) = if swapped {
        (case.height_mm, case.width_mm)
    } else {
        (case.width_mm, case.height_mm)
    };
    let shape = SectionShape::CftBox {
        width,
        height,
        thick: case.thick_mm,
        corner_r: Some(case.outer_r_mm),
    };
    let started = Instant::now();
    let result = shape.try_cft_core_torsion();
    let wall_seconds = started.elapsed().as_secs_f64();
    let mut record = match result {
        Ok(Some(CftCoreTorsion::Numerical(solution))) => json!({
            "status": "success",
            "j_mm4": solution.j_mm4,
            "j_hat": solution.j_hat,
            "length_scale_mm": solution.length_scale_mm,
            "accepted_level": solution.accepted_level,
            "internal_changes": solution.internal_changes,
            "boundary_changes": solution.boundary_changes,
            "controller_seconds": solution.total_seconds,
            "levels": solution.levels.iter().map(level).collect::<Vec<_>>(),
            "failed_attempt": Value::Null,
            "reason": Value::Null,
        }),
        Err(error) => json!({
            "status": "unresolved",
            "j_mm4": Value::Null,
            "controller_seconds": error.total_seconds,
            "levels": error.levels.iter().map(level).collect::<Vec<_>>(),
            "failed_attempt": error.failed_attempt.map(attempt),
            "reason": error.reason,
        }),
        other => panic!("角形CFT診断の想定外の結果: {other:?}"),
    };
    record["wall_seconds"] = json!(wall_seconds);
    record
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 {
        return Err("引数: 出力ディレクトリ grid [開始label番号 終了label番号] | repeat label番号 [回数] | known".into());
    }
    if cfg!(debug_assertions) {
        return Err("releaseビルドで実行してください".into());
    }
    let output = PathBuf::from(&args[1]);
    fs::create_dir_all(&output)?;
    let mode = args[2].as_str();
    let all_cases = grid();
    let (cases, repetitions, alias_allowed): (Vec<Case>, usize, bool) = match mode {
        "grid" => {
            let start = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(0);
            let end = args.get(4).map(|v| v.parse()).transpose()?.unwrap_or(96);
            (
                all_cases.get(start..end).ok_or("label範囲不正")?.to_vec(),
                1,
                true,
            )
        }
        "repeat" => {
            let index: usize = args.get(3).ok_or("label番号が必要")?.parse()?;
            let repetitions = args.get(4).map(|v| v.parse()).transpose()?.unwrap_or(3);
            if repetitions < 3 {
                return Err("反復回数は3以上が必要です".into());
            }
            (
                vec![all_cases.get(index).ok_or("label番号不正")?.clone()],
                repetitions,
                false,
            )
        }
        "known" => (
            [0.0, 12.0, 30.0]
                .into_iter()
                .map(|outer_r_mm| Case {
                    label: format!("known_400x300x12_r{outer_r_mm}"),
                    width_mm: 300.0,
                    height_mm: 400.0,
                    thick_mm: 12.0,
                    outer_r_mm,
                })
                .collect(),
            1,
            true,
        ),
        _ => return Err("未知のmodeです".into()),
    };
    let mut jsonl = BufWriter::new(File::create_new(output.join("calls.jsonl"))?);
    let mut csv = BufWriter::new(File::create_new(output.join("calls.csv"))?);
    writeln!(csv, "call_id,label,swapped,repetition,kind,alias_of,status,j_mm4,wall_seconds,levels,swap_relative_difference")?;
    let started = Instant::now();
    let mut aliases: HashMap<(bool, u64, u64, u64), (usize, Value)> = HashMap::new();
    let mut actual_solves = 0usize;
    let mut successful_solves = 0usize;
    let mut unresolved_solves = 0usize;
    let mut call_id = 0usize;
    for case in &cases {
        for repetition in 0..repetitions {
            let mut direct_j = None;
            for swapped in [false, true] {
                let inner_width = case.width_mm - 2.0 * case.thick_mm;
                let inner_height = case.height_mm - 2.0 * case.thick_mm;
                let inner_r = (case.outer_r_mm - case.thick_mm).max(0.0);
                let key = (
                    swapped,
                    inner_width.to_bits(),
                    inner_height.to_bits(),
                    inner_r.to_bits(),
                );
                let alias = alias_allowed.then(|| aliases.get(&key)).flatten();
                let (mut record, alias_of) = if let Some((id, result)) = alias {
                    (result.clone(), Some(*id))
                } else {
                    eprintln!(
                        "call={call_id} label={} swapped={swapped} repetition={repetition}",
                        case.label
                    );
                    let result = diagnose(case, swapped);
                    actual_solves += 1;
                    if result["status"] == "success" {
                        successful_solves += 1;
                    } else {
                        unresolved_solves += 1;
                    }
                    aliases.insert(key, (call_id, result.clone()));
                    (result, None)
                };
                if !swapped {
                    direct_j = record["j_mm4"].as_f64();
                }
                let swap_difference = if swapped {
                    direct_j
                        .zip(record["j_mm4"].as_f64())
                        .map(|(a, b)| (b / a - 1.0).abs())
                } else {
                    None
                };
                record["call_id"] = json!(call_id);
                record["label"] = json!(case.label);
                record["swapped"] = json!(swapped);
                record["repetition"] = json!(repetition);
                record["kind"] = json!(if alias_of.is_some() {
                    "alias_no_solve"
                } else {
                    "fresh_diagnostic"
                });
                record["alias_of"] = json!(alias_of);
                record["swap_relative_difference"] = json!(swap_difference);
                record["width_mm"] = json!(if swapped {
                    case.height_mm
                } else {
                    case.width_mm
                });
                record["height_mm"] = json!(if swapped {
                    case.width_mm
                } else {
                    case.height_mm
                });
                record["thick_mm"] = json!(case.thick_mm);
                record["outer_r_mm"] = json!(case.outer_r_mm);
                record["inner_width_mm"] = json!(if swapped { inner_height } else { inner_width });
                record["inner_height_mm"] = json!(if swapped { inner_width } else { inner_height });
                record["inner_r_mm"] = json!(inner_r);
                record["inner_aspect_ratio"] = json!(inner_height / inner_width);
                if alias_of.is_some() {
                    record["wall_seconds"] = Value::Null;
                    record["controller_seconds"] = Value::Null;
                    record["levels"] = json!([]);
                }
                writeln!(
                    csv,
                    "{},{},{},{},{},{},{},{},{},{},{}",
                    call_id,
                    case.label,
                    swapped,
                    repetition,
                    record["kind"].as_str().unwrap(),
                    alias_of.map(|v| v.to_string()).unwrap_or_default(),
                    record["status"].as_str().unwrap(),
                    record["j_mm4"]
                        .as_f64()
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    record["wall_seconds"]
                        .as_f64()
                        .map(|v| v.to_string())
                        .unwrap_or_default(),
                    record["levels"].as_array().unwrap().len(),
                    swap_difference.map(|v| v.to_string()).unwrap_or_default()
                )?;
                serde_json::to_writer(&mut jsonl, &record)?;
                writeln!(jsonl)?;
                csv.flush()?;
                jsonl.flush()?;
                call_id += 1;
            }
        }
    }
    let manifest = json!({
        "mode": mode,
        "args": args,
        "pid": std::process::id(),
        "labels": cases.len(),
        "records": call_id,
        "actual_fresh_diagnostic_calls": actual_solves,
        "successful_fresh_solves": successful_solves,
        "unresolved_fresh_solves": unresolved_solves,
        "aliases_without_solve": call_id - actual_solves,
        "process_wall_seconds": started.elapsed().as_secs_f64(),
        "finished_unix_seconds": SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64(),
        "os_cold_reset": false,
        "diagnostic_cache": "公開診断は毎回fresh。aliasは求解せず、時間標本から除外",
        "process_order": "各labelで反復ごとに通常向き、H/B交換の順",
        "memory": "外側OS計測を参照。runner内でRSSやfactorfillを推定しない",
    });
    serde_json::to_writer_pretty(File::create_new(output.join("manifest.json"))?, &manifest)?;
    Ok(())
}
