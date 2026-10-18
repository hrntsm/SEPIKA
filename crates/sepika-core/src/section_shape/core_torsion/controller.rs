use std::fmt;
use std::time::Instant;

use super::mesh::{self, MeshSettings, RoundedCore};
use super::p1;

const BOUNDARY_SEGMENTS: [usize; 6] = [16, 32, 64, 128, 256, 512];
const AREA_DENOMINATORS: [f64; 8] = [
    128.0, 512.0, 2048.0, 8192.0, 32768.0, 131072.0, 524288.0, 2097152.0,
];
const CHANGE_LIMIT: f64 = 1e-3;

/// コア数値Jの実P1解。寸法はmm、Jはmm⁴、時間は秒。履歴は計算完了順。
#[derive(Debug)]
pub struct CoreTorsionSolution {
    pub j_mm4: f64,
    pub j_hat: f64,
    pub length_scale_mm: f64,
    pub levels: Vec<CoreTorsionLevel>,
    pub accepted_level: usize,
    pub internal_changes: [f64; 2],
    pub boundary_changes: Option<[f64; 2]>,
    pub total_seconds: f64,
}

/// 一つの正規化メッシュと実P1解の品質。面積・Φ・Jは正規化値、時間は秒。
#[derive(Debug)]
pub struct CoreTorsionLevel {
    pub attempt: CoreTorsionAttempt,
    pub j_hat: f64,
    pub relative_residual: f64,
    pub free_dofs: usize,
    pub nnz: usize,
    pub min_phi: f64,
    pub max_phi: f64,
    pub polygon_area: f64,
    pub triangle_area: f64,
    pub max_actual_area: f64,
    pub min_signed_double_area: f64,
    pub min_area_edge_ratio: f64,
    pub min_angle_rad: f64,
    pub original_boundary_segments: usize,
    pub boundary_edges: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub fingerprint: u64,
    pub mesh_seconds: f64,
    pub assembly_seconds: f64,
    pub factorization_seconds: f64,
    pub solve_seconds: f64,
}

/// 一回の計算設定。最大三角形面積は長辺1に正規化した値。
#[derive(Clone, Copy, Debug)]
pub struct CoreTorsionAttempt {
    pub quarter_arc_segments: usize,
    pub max_triangle_area: f64,
    pub max_vertices: usize,
    pub max_triangles: usize,
}

/// 数値J未確定の理由、完了済み解、失敗設定。履歴は計算完了順、時間は秒。
#[derive(Debug)]
pub struct CoreTorsionError {
    pub reason: String,
    pub levels: Vec<CoreTorsionLevel>,
    pub failed_attempt: Option<CoreTorsionAttempt>,
    pub total_seconds: f64,
}

impl fmt::Display for CoreTorsionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for CoreTorsionError {}

struct Controller {
    started: Instant,
    levels: Vec<CoreTorsionLevel>,
    cells: [[Option<usize>; 8]; 6],
    max_vertices: usize,
    max_triangles: usize,
    rows: usize,
    columns: usize,
}

impl Controller {
    fn error(self, reason: String, failed_attempt: Option<CoreTorsionAttempt>) -> CoreTorsionError {
        CoreTorsionError {
            reason,
            levels: self.levels,
            failed_attempt,
            total_seconds: self.started.elapsed().as_secs_f64(),
        }
    }

    fn evaluate(
        &mut self,
        core: RoundedCore,
        row: usize,
        column: usize,
    ) -> Result<(), (String, CoreTorsionAttempt)> {
        if self.cells[row][column].is_some() {
            return Ok(());
        }
        let attempt = CoreTorsionAttempt {
            quarter_arc_segments: BOUNDARY_SEGMENTS[row],
            max_triangle_area: 1.0 / AREA_DENOMINATORS[column],
            max_vertices: self.max_vertices,
            max_triangles: self.max_triangles,
        };
        let started = Instant::now();
        let mesh = mesh::generate(
            core,
            MeshSettings {
                quarter_arc_segments: attempt.quarter_arc_segments,
                max_triangle_area: attempt.max_triangle_area,
                max_vertices: attempt.max_vertices,
                max_triangles: attempt.max_triangles,
            },
        )
        .map_err(|e| (e, attempt))?;
        let mesh_seconds = started.elapsed().as_secs_f64();
        let solution = p1::solve(&mesh).map_err(|e| (e, attempt))?;
        let quality = mesh.quality;
        if solution.free_dofs != quality.free_vertices {
            return Err((
                "コア数値J: メッシュとP1の自由度数が一致しません".into(),
                attempt,
            ));
        }
        self.cells[row][column] = Some(self.levels.len());
        self.levels.push(CoreTorsionLevel {
            attempt,
            j_hat: solution.j_hat,
            relative_residual: solution.relative_residual,
            free_dofs: solution.free_dofs,
            nnz: solution.nnz,
            min_phi: solution.min_phi,
            max_phi: solution.max_phi,
            polygon_area: quality.polygon_area,
            triangle_area: quality.triangle_area,
            max_actual_area: quality.max_actual_area,
            min_signed_double_area: quality.min_signed_double_area,
            min_area_edge_ratio: quality.min_area_edge_ratio,
            min_angle_rad: quality.min_angle_rad,
            original_boundary_segments: quality.original_boundary_segments,
            boundary_edges: quality.boundary_edges,
            vertices: quality.vertices,
            triangles: quality.triangles,
            fingerprint: quality.fingerprint,
            mesh_seconds,
            assembly_seconds: solution.assembly_seconds,
            factorization_seconds: solution.factorization_seconds,
            solve_seconds: solution.solve_seconds,
        });
        Ok(())
    }

    fn cell(&self, row: usize, column: usize) -> &CoreTorsionLevel {
        &self.levels[self.cells[row][column].expect("評価済みセル")]
    }

    fn internal_changes(&self, row: usize, column: usize) -> Option<[f64; 2]> {
        if column < 2 {
            return None;
        }
        let a = self.cell(row, column - 2);
        let b = self.cell(row, column - 1);
        let c = self.cell(row, column);
        let denser = |old: &CoreTorsionLevel, new: &CoreTorsionLevel| {
            old.fingerprint != new.fingerprint
                && new.free_dofs > old.free_dofs
                && new.vertices > old.vertices
                && new.triangles > old.triangles
                && new.max_actual_area < old.max_actual_area
                && old.polygon_area == new.polygon_area
        };
        if !denser(a, b) || !denser(b, c) {
            return None;
        }
        let changes = [
            relative_change(a.j_hat, b.j_hat),
            relative_change(b.j_hat, c.j_hat),
        ];
        changes
            .iter()
            .all(|d| d.is_finite() && *d <= CHANGE_LIMIT)
            .then_some(changes)
    }
}

fn relative_change(previous: f64, current: f64) -> f64 {
    (current - previous).abs() / current.abs()
}

pub(in crate::section_shape) fn rounded_rect_core_torsion(
    b_mm: f64,
    h_mm: f64,
    r_mm: f64,
) -> Result<CoreTorsionSolution, CoreTorsionError> {
    run(b_mm, h_mm, r_mm, 200_000, 400_000, 6, 8)
}

fn run(
    b_mm: f64,
    h_mm: f64,
    r_mm: f64,
    max_vertices: usize,
    max_triangles: usize,
    rows: usize,
    columns: usize,
) -> Result<CoreTorsionSolution, CoreTorsionError> {
    let mut controller = Controller {
        started: Instant::now(),
        levels: Vec::new(),
        cells: [[None; 8]; 6],
        max_vertices,
        max_triangles,
        rows,
        columns,
    };
    if !b_mm.is_finite()
        || !h_mm.is_finite()
        || !r_mm.is_finite()
        || b_mm <= 0.0
        || h_mm <= 0.0
        || r_mm < 0.0
        || r_mm > b_mm.min(h_mm) / 2.0
    {
        return Err(controller.error(
            "コア数値J: 有限正の内法寸法と0≤内角R≤短辺/2が必要です".into(),
            None,
        ));
    }
    let length_scale_mm = b_mm.max(h_mm);
    let core = RoundedCore {
        b: b_mm / length_scale_mm,
        h: h_mm / length_scale_mm,
        r: r_mm / length_scale_mm,
    };
    if core.b <= 0.0 || core.h <= 0.0 || (r_mm > 0.0 && core.r == 0.0) {
        return Err(controller.error("コア数値J: 寸法正規化がアンダーフローしました".into(), None));
    }
    let mut convergence_columns = [0; 6];
    let mut accepted = None;
    for row in 0..if r_mm == 0.0 { 1 } else { controller.rows } {
        let mut convergence_column = None;
        for column in 0..controller.columns {
            if let Err((reason, attempt)) = controller.evaluate(core, row, column) {
                return Err(controller.error(reason, Some(attempt)));
            }
            if controller.internal_changes(row, column).is_some() {
                convergence_column = Some(column);
                break;
            }
        }
        let Some(column) = convergence_column else {
            return Err(controller.error(
                "コア数値J: 内部細分化上限で連続2区間の品質基準を満たしません".into(),
                None,
            ));
        };
        convergence_columns[row] = column;
        if r_mm == 0.0 {
            accepted = Some((row, column, None));
            break;
        }
        if row < 2 {
            continue;
        }
        let first_row = row - 2;
        let mut common_column = *convergence_columns[first_row..=row]
            .iter()
            .max()
            .expect("境界3段");
        loop {
            for candidate_row in first_row..=row {
                for candidate_column in 0..=common_column {
                    if let Err((reason, attempt)) =
                        controller.evaluate(core, candidate_row, candidate_column)
                    {
                        return Err(controller.error(reason, Some(attempt)));
                    }
                }
            }
            if (first_row..=row).all(|r| controller.internal_changes(r, common_column).is_some()) {
                break;
            }
            common_column += 1;
            if common_column == controller.columns {
                return Err(controller.error(
                    "コア数値J: 共通内部列の細分化上限で品質基準を満たしません".into(),
                    None,
                ));
            }
        }
        let a = controller.cell(first_row, common_column);
        let b = controller.cell(first_row + 1, common_column);
        let c = controller.cell(row, common_column);
        let changes = [
            relative_change(a.j_hat, b.j_hat),
            relative_change(b.j_hat, c.j_hat),
        ];
        if a.original_boundary_segments < b.original_boundary_segments
            && b.original_boundary_segments < c.original_boundary_segments
            && a.fingerprint != b.fingerprint
            && b.fingerprint != c.fingerprint
            && changes.iter().all(|d| d.is_finite() && *d <= CHANGE_LIMIT)
        {
            accepted = Some((row, common_column, Some(changes)));
            break;
        }
    }
    let Some((row, column, boundary_changes)) = accepted else {
        return Err(controller.error(
            "コア数値J: 境界細分化上限で連続2区間の品質基準を満たしません".into(),
            None,
        ));
    };
    let level = controller.cell(row, column);
    let accepted_level = controller.cells[row][column].expect("採用セル");
    let j_hat = level.j_hat;
    let internal_changes = controller
        .internal_changes(row, column)
        .expect("採用内部品質");
    let j_mm4 = ((j_hat * length_scale_mm) * length_scale_mm) * length_scale_mm * length_scale_mm;
    if !j_mm4.is_finite() || j_mm4 <= 0.0 {
        return Err(controller.error(
            "コア数値J: mm⁴への尺度復元がオーバーフローまたはアンダーフローしました".into(),
            None,
        ));
    }
    Ok(CoreTorsionSolution {
        j_mm4,
        j_hat,
        length_scale_mm,
        levels: controller.levels,
        accepted_level,
        internal_changes,
        boundary_changes,
        total_seconds: controller.started.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
