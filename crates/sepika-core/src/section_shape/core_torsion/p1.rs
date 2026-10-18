use std::time::Instant;

use faer::linalg::solvers::Solve;
use faer::sparse::linalg::solvers::{Llt, SymbolicLlt};
use faer::sparse::{SparseColMat, Triplet};

use super::mesh::Mesh;

#[derive(Debug)]
pub(super) struct P1Solution {
    pub j_hat: f64,
    pub relative_residual: f64,
    pub free_dofs: usize,
    pub nnz: usize,
    pub min_phi: f64,
    pub max_phi: f64,
    pub assembly_seconds: f64,
    pub factorization_seconds: f64,
    pub solve_seconds: f64,
}

struct Assembly {
    stiffness: SparseColMat<usize, f64>,
    load: Vec<f64>,
    free_index: Vec<Option<usize>>,
    double_areas: Vec<f64>,
}

fn zeros<T: Default + Clone>(len: usize, context: &str) -> Result<Vec<T>, String> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|e| format!("コアP1: {context}の資源確保失敗: {e}"))?;
    values.resize(len, T::default());
    Ok(values)
}

fn assemble(mesh: &Mesh) -> Result<Assembly, String> {
    if mesh.points.is_empty() || mesh.triangles.is_empty() || mesh.boundary_vertices.is_empty() {
        return Err("コアP1: 節点・三角形・外周節点が必要です".into());
    }
    if mesh.points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("コアP1: 節点座標が非有限です".into());
    }
    let mut boundary = zeros::<bool>(mesh.points.len(), "境界マスク")?;
    for &index in &mesh.boundary_vertices {
        let value = boundary
            .get_mut(index)
            .ok_or_else(|| format!("コアP1: 外周節点索引{index}が範囲外です"))?;
        if *value {
            return Err(format!("コアP1: 外周節点索引{index}が重複しています"));
        }
        *value = true;
    }
    let mut free_index = zeros::<Option<usize>>(mesh.points.len(), "自由度対応")?;
    let mut free_dofs = 0usize;
    for (index, is_boundary) in boundary.into_iter().enumerate() {
        if !is_boundary {
            free_index[index] = Some(free_dofs);
            free_dofs = free_dofs
                .checked_add(1)
                .ok_or("コアP1: 自由度数の整数上限超過")?;
        }
    }
    if free_dofs == 0 {
        return Err("コアP1: 自由度が0のため数値Jを確定できません".into());
    }
    let capacity = mesh
        .triangles
        .len()
        .checked_mul(9)
        .ok_or("コアP1: 行列項数の整数上限超過")?;
    let mut triplets = Vec::new();
    triplets
        .try_reserve_exact(capacity)
        .map_err(|e| format!("コアP1: 行列項の資源確保失敗: {e}"))?;
    let mut load = zeros::<f64>(free_dofs, "荷重")?;
    let mut double_areas = zeros::<f64>(mesh.triangles.len(), "要素面積")?;
    for (element, triangle) in mesh.triangles.iter().enumerate() {
        let mut points = [[0.0; 2]; 3];
        for (local, &index) in triangle.iter().enumerate() {
            points[local] = *mesh
                .points
                .get(index)
                .ok_or_else(|| format!("コアP1: 要素{element}の節点索引{index}が範囲外です"))?;
        }
        let u = [points[1][0] - points[0][0], points[1][1] - points[0][1]];
        let v = [points[2][0] - points[0][0], points[2][1] - points[0][1]];
        let w = [points[2][0] - points[1][0], points[2][1] - points[1][1]];
        let double_area = u[0] * v[1] - u[1] * v[0];
        let longest_edge_squared = [u, v, w]
            .into_iter()
            .map(|e| e[0] * e[0] + e[1] * e[1])
            .fold(0.0, f64::max);
        let ratio = double_area / longest_edge_squared;
        if !double_area.is_finite()
            || !ratio.is_finite()
            || double_area <= 0.0
            || ratio <= 128.0 * f64::EPSILON
        {
            return Err(format!(
                "コアP1: 要素{element}が反転・退化または数値的に特異です"
            ));
        }
        double_areas[element] = double_area;
        let gradients = [
            [-w[1] / double_area, w[0] / double_area],
            [v[1] / double_area, -v[0] / double_area],
            [-u[1] / double_area, u[0] / double_area],
        ];
        for i in 0..3 {
            let Some(row) = free_index[triangle[i]] else {
                continue;
            };
            load[row] += double_area / 3.0;
            for j in i..3 {
                let Some(col) = free_index[triangle[j]] else {
                    continue;
                };
                let value = double_area / 2.0
                    * (gradients[i][0] * gradients[j][0] + gradients[i][1] * gradients[j][1]);
                if !value.is_finite() {
                    return Err(format!("コアP1: 要素{element}の剛性が非有限です"));
                }
                triplets.push(Triplet::new(row, col, value));
                if row != col {
                    triplets.push(Triplet::new(col, row, value));
                }
            }
        }
    }
    if load.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err("コアP1: 荷重が非正・非有限、または孤立した自由節点です".into());
    }
    let stiffness =
        SparseColMat::<usize, f64>::try_new_from_triplets(free_dofs, free_dofs, &triplets)
            .map_err(|e| format!("コアP1: 疎行列集約失敗: {e:?}"))?;
    if stiffness.val().iter().any(|v| !v.is_finite()) {
        return Err("コアP1: 集約した剛性が非有限です".into());
    }
    for col in 0..free_dofs {
        for (row, value) in stiffness
            .symbolic()
            .row_idx_of_col(col)
            .zip(stiffness.val_of_col(col))
        {
            let rows = stiffness.symbolic().row_idx_of_col_raw(row);
            let transpose_index = rows
                .binary_search(&col)
                .map_err(|_| "コアP1: 集約した剛性の対称成分が欠落しています")?;
            let transpose = stiffness.val_of_col(row)[transpose_index];
            if (value - transpose).abs() > 128.0 * f64::EPSILON * value.abs().max(transpose.abs()) {
                return Err("コアP1: 集約した剛性が非対称です".into());
            }
        }
    }
    Ok(Assembly {
        stiffness,
        load,
        free_index,
        double_areas,
    })
}

fn relative_residual(assembly: &Assembly, phi: &faer::Mat<f64>) -> Result<f64, String> {
    let mut residual = zeros::<f64>(assembly.load.len(), "残差")?;
    for (value, load) in residual.iter_mut().zip(&assembly.load) {
        *value = -load;
    }
    for col in 0..assembly.load.len() {
        for (row, value) in assembly
            .stiffness
            .symbolic()
            .row_idx_of_col(col)
            .zip(assembly.stiffness.val_of_col(col))
        {
            residual[row] += value * phi[(col, 0)];
        }
    }
    let numerator = residual.iter().copied().fold(0.0f64, f64::hypot);
    let denominator = assembly.load.iter().copied().fold(0.0f64, f64::hypot);
    let relative = numerator / denominator;
    if !numerator.is_finite()
        || !denominator.is_finite()
        || !relative.is_finite()
        || relative > 1e-10
    {
        return Err(format!(
            "コアP1: 元の剛性・荷重に対する相対残差{relative:e}が品質基準1e-10を満たしません"
        ));
    }
    Ok(relative)
}

fn restore_phi(assembly: &Assembly, phi: &faer::Mat<f64>) -> Result<Vec<f64>, String> {
    let mut full = zeros::<f64>(assembly.free_index.len(), "全節点Phi")?;
    for (value, index) in full.iter_mut().zip(&assembly.free_index) {
        if let Some(index) = index {
            *value = phi[(*index, 0)];
        }
        if !value.is_finite() {
            return Err("コアP1: 求解したPhiが非有限です".into());
        }
    }
    Ok(full)
}

pub(super) fn solve(mesh: &Mesh) -> Result<P1Solution, String> {
    let start = Instant::now();
    let assembly = assemble(mesh)?;
    let assembly_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let symbolic = SymbolicLlt::try_new(assembly.stiffness.symbolic(), faer::Side::Lower)
        .map_err(|e| format!("コアP1: 疎LLt記号分解失敗: {e:?}"))?;
    let factor =
        Llt::try_new_with_symbolic(symbolic, assembly.stiffness.as_ref(), faer::Side::Lower)
            .map_err(|e| format!("コアP1: 疎LLt数値分解失敗（非正定値または資源不足）: {e:?}"))?;
    let factorization_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let mut phi = faer::Mat::<f64>::new();
    phi.try_reserve(assembly.load.len(), 1)
        .map_err(|e| format!("コアP1: 求解右辺の資源確保失敗: {e:?}"))?;
    phi.resize_with(assembly.load.len(), 1, |i, _| assembly.load[i]);
    factor.solve_in_place(phi.as_mut());
    let solve_seconds = start.elapsed().as_secs_f64();
    let full_phi = restore_phi(&assembly, &phi)?;
    let relative_residual = relative_residual(&assembly, &phi)?;
    let mut j_hat = 0.0;
    for (triangle, double_area) in mesh.triangles.iter().zip(&assembly.double_areas) {
        j_hat += double_area / 3.0 * triangle.iter().map(|i| full_phi[*i]).sum::<f64>();
    }
    if !j_hat.is_finite() || j_hat <= 0.0 {
        return Err("コアP1: 積分したJhatが非正または非有限です".into());
    }
    Ok(P1Solution {
        j_hat,
        relative_residual,
        free_dofs: assembly.load.len(),
        nnz: assembly.stiffness.val().len(),
        min_phi: full_phi.iter().copied().fold(f64::INFINITY, f64::min),
        max_phi: full_phi.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        assembly_seconds,
        factorization_seconds,
        solve_seconds,
    })
}

#[cfg(test)]
#[path = "p1_tests.rs"]
mod tests;
