//! 3 次元質点系（各階 Ux, Uy, θz）の固有値と時刻歴。

use super::eigen::LumpedMassModal;
use super::model::{LumpedMassModel, StorySpatial};
use super::time_history::{
    dir_ductility, story_spring, StickDirPeaks, StickResponse, STICK_NEWTON,
};
use crate::statics::analysis::SeismicDir;
use faer::Side;
use sepika_material::{HysteresisMaterial, UniaxialMaterial};
use sepika_math::solver::SolveError;

pub(crate) fn rigidity_translation_coefficients(
    spatial: &[StorySpatial],
    i: usize,
    axis: usize,
    stride: usize,
) -> Vec<(usize, f64)> {
    let point = spatial[i].rigidity_xy;
    let arm = |center: [f64; 2]| {
        if axis == 0 {
            -(point[1] - center[1])
        } else {
            point[0] - center[0]
        }
    };
    let mut coefficients = vec![
        (i * stride + axis, 1.0),
        (i * stride + 2, arm(spatial[i].mass_xy)),
    ];
    if i > 0 {
        coefficients.extend([
            ((i - 1) * stride + axis, -1.0),
            ((i - 1) * stride + 2, -arm(spatial[i - 1].mass_xy)),
        ]);
    }
    coefficients
}

pub(crate) fn rigidity_translation(
    u: &[f64],
    spatial: &[StorySpatial],
    i: usize,
    axis: usize,
    stride: usize,
) -> f64 {
    rigidity_translation_coefficients(spatial, i, axis, stride)
        .iter()
        .map(|&(index, coefficient)| u[index] * coefficient)
        .sum()
}

fn assemble_spatial_k(spatial: &[StorySpatial], k_scale: impl Fn(usize, usize) -> f64) -> Vec<f64> {
    let nd = 3 * spatial.len();
    let mut k = vec![0.0; nd * nd];
    for (i, story) in spatial.iter().enumerate() {
        for axis in 0..3 {
            let stiffness = match axis {
                0 => story.skeleton_x.k1,
                1 => story.skeleton_y.k1,
                _ => story.kr,
            }
            .max(0.0)
                * k_scale(i, axis);
            let mut coefficients = if axis < 2 {
                rigidity_translation_coefficients(spatial, i, axis, 3)
            } else {
                vec![(i * 3 + 2, 1.0)]
            };
            if axis == 2 && i > 0 {
                coefficients.push(((i - 1) * 3 + 2, -1.0));
            }
            for &(a, ca) in &coefficients {
                for &(b, cb) in &coefficients {
                    k[a * nd + b] += stiffness * ca * cb;
                }
            }
        }
    }
    k
}

fn mass_diag(lm: &LumpedMassModel) -> Result<Vec<f64>, SolveError> {
    let n = lm.stories.len();
    let mut m = vec![0.0; 3 * n];
    for i in 0..n {
        if lm.stories[i].mass <= 0.0 {
            return Err(SolveError::InvalidInput(format!(
                "層 {:?} の質量が 0 以下のため固有値解析できません",
                lm.stories[i].story
            )));
        }
        let j = lm.spatial[i].j;
        if j <= 0.0 {
            return Err(SolveError::InvalidInput(format!(
                "層 {:?} の回転慣性 J が 0 以下のため 3 次元質点系の固有値解析できません",
                lm.stories[i].story
            )));
        }
        m[3 * i] = lm.stories[i].mass;
        m[3 * i + 1] = lm.stories[i].mass;
        m[3 * i + 2] = j;
    }
    Ok(m)
}

pub(crate) fn lumped_mass_eigen_spatial(
    lm: &LumpedMassModel,
    n_modes: usize,
) -> Result<LumpedMassModal, SolveError> {
    let n = lm.stories.len();
    if n == 0 || n_modes == 0 || !lm.is_spatial() {
        return Ok(LumpedMassModal::default());
    }
    let mass = mass_diag(lm)?;
    let nd = mass.len();
    let k = assemble_spatial_k(&lm.spatial, |_, _| 1.0);
    let sqrt_m: Vec<f64> = mass.iter().map(|&mi| mi.sqrt()).collect();
    let a = faer::Mat::from_fn(nd, nd, |i, j| k[i * nd + j] / (sqrt_m[i] * sqrt_m[j]));
    let eig = a
        .self_adjoint_eigen(Side::Lower)
        .map_err(|e| SolveError::NonConvergence(format!("3次元質点系の固有値分解: {e:?}")))?;
    let s = eig.S();
    let u = eig.U();
    let take = n_modes.min(nd);
    let mut omega2 = Vec::with_capacity(take);
    let mut period = Vec::with_capacity(take);
    let mut shapes = Vec::with_capacity(take);
    let mut shapes_xyz = Vec::with_capacity(take);
    for j in 0..take {
        let w2 = s[j].max(0.0);
        let w = w2.sqrt();
        omega2.push(w2);
        period.push(if w > 0.0 {
            2.0 * std::f64::consts::PI / w
        } else {
            f64::INFINITY
        });
        let mut xyz = vec![[0.0; 3]; n];
        for i in 0..n {
            xyz[i] = [
                u[(3 * i, j)] / sqrt_m[3 * i],
                u[(3 * i + 1, j)] / sqrt_m[3 * i + 1],
                u[(3 * i + 2, j)] / sqrt_m[3 * i + 2],
            ];
        }
        let top = xyz[n - 1];
        let h = (top[0] * top[0] + top[1] * top[1]).sqrt();
        let scale = if h > 1e-30 {
            h
        } else if top[2].abs() > 1e-30 {
            top[2].abs()
        } else {
            1.0
        };
        for v in xyz.iter_mut() {
            v[0] /= scale;
            v[1] /= scale;
            v[2] /= scale;
        }
        let planar: Vec<f64> = xyz
            .iter()
            .map(|v| (v[0] * v[0] + v[1] * v[1]).sqrt().copysign(v[0] + v[1]))
            .collect();
        shapes.push(planar);
        shapes_xyz.push(xyz);
    }
    Ok(LumpedMassModal {
        bending_rotations: Vec::new(),
        omega2,
        period,
        shapes,
        shapes_xyz,
    })
}

fn k_matvec(k: &[f64], x: &[f64], nd: usize, scale: f64) -> Vec<f64> {
    let mut y = vec![0.0; nd];
    for i in 0..nd {
        let mut s = 0.0;
        for j in 0..nd {
            s += k[i * nd + j] * x[j];
        }
        y[i] = scale * s;
    }
    y
}

fn solve_dense(a: &[f64], b: &[f64], nd: usize) -> Vec<f64> {
    let mut m = vec![0.0; nd * (nd + 1)];
    for i in 0..nd {
        for j in 0..nd {
            m[i * (nd + 1) + j] = a[i * nd + j];
        }
        m[i * (nd + 1) + nd] = b[i];
    }
    for col in 0..nd {
        let mut piv = col;
        let mut best = m[col * (nd + 1) + col].abs();
        for i in (col + 1)..nd {
            let v = m[i * (nd + 1) + col].abs();
            if v > best {
                best = v;
                piv = i;
            }
        }
        if best < 1e-30 {
            continue;
        }
        if piv != col {
            for j in col..=nd {
                m.swap(col * (nd + 1) + j, piv * (nd + 1) + j);
            }
        }
        let diag = m[col * (nd + 1) + col];
        for i in (col + 1)..nd {
            let f = m[i * (nd + 1) + col] / diag;
            for j in col..=nd {
                m[i * (nd + 1) + j] -= f * m[col * (nd + 1) + j];
            }
        }
    }
    let mut x = vec![0.0; nd];
    for i in (0..nd).rev() {
        let mut s = m[i * (nd + 1) + nd];
        for j in (i + 1)..nd {
            s -= m[i * (nd + 1) + j] * x[j];
        }
        let d = m[i * (nd + 1) + i];
        x[i] = if d.abs() > 1e-30 { s / d } else { 0.0 };
    }
    x
}

fn relative_at_rigidity(u: &[f64], i: usize, spatial: &[StorySpatial]) -> [f64; 3] {
    [
        rigidity_translation(u, spatial, i, 0, 3),
        rigidity_translation(u, spatial, i, 1, 3),
        u[i * 3 + 2] - if i > 0 { u[(i - 1) * 3 + 2] } else { 0.0 },
    ]
}

fn add_story_force(f: &mut [f64], i: usize, spatial: &[StorySpatial], fx: f64, fy: f64, mz: f64) {
    for (axis, force) in [fx, fy].into_iter().enumerate() {
        for (index, coefficient) in rigidity_translation_coefficients(spatial, i, axis, 3) {
            f[index] += coefficient * force;
        }
    }
    f[i * 3 + 2] += mz;
    if i > 0 {
        f[(i - 1) * 3 + 2] -= mz;
    }
}

pub(crate) fn lumped_mass_time_history_spatial(
    lm: &LumpedMassModel,
    accel: &[f64],
    dt: f64,
    h: f64,
) -> StickResponse {
    let n = lm.stories.len();
    if n == 0 || !dt.is_finite() || dt <= 0.0 || accel.is_empty() || !lm.is_spatial() {
        return StickResponse::empty(n);
    }
    let mass = match mass_diag(lm) {
        Ok(m) => m,
        Err(_) => return StickResponse::empty(n),
    };
    let nd = mass.len();
    let k_init = assemble_spatial_k(&lm.spatial, |_, _| 1.0);
    let mut springs_x: Vec<HysteresisMaterial> = lm
        .spatial
        .iter()
        .map(|s| story_spring(&s.skeleton_x))
        .collect();
    let mut springs_y: Vec<HysteresisMaterial> = lm
        .spatial
        .iter()
        .map(|s| story_spring(&s.skeleton_y))
        .collect();

    let omega1 = super::eigen::stick_omega1(lm);
    let a1 = if omega1 > 0.0 { 2.0 * h / omega1 } else { 0.0 };
    let beta = 0.25;
    let gamma = 0.5;
    let c1 = 1.0 / (beta * dt * dt);
    let c2 = gamma / (beta * dt);

    let dir_idx = match lm.dir {
        SeismicDir::X => 0,
        SeismicDir::Y => 1,
    };

    let mut u = vec![0.0; nd];
    let mut v = vec![0.0; nd];
    let mut a = vec![0.0; nd];
    for i in 0..n {
        a[3 * i + dir_idx] = -accel[0];
    }
    let mut time = Vec::with_capacity(accel.len());
    let mut roof = Vec::with_capacity(accel.len());
    let mut floor_disp = Vec::with_capacity(accel.len());
    let mut peak_drift = vec![0.0_f64; n];
    let mut peak_shear = vec![0.0_f64; n];
    let mut drift_dir = StickDirPeaks::zeros(n);
    let mut shear_dir = StickDirPeaks::zeros(n);
    let mut non_converged_steps = 0usize;
    let mut peak_force_scale = 0.0_f64;

    time.push(0.0);
    roof.push(0.0);
    floor_disp.push(vec![[0.0; 3]; n]);

    for (step, &ag) in accel.iter().enumerate().skip(1) {
        let mut p = vec![0.0; nd];
        for i in 0..n {
            p[3 * i + dir_idx] = -mass[3 * i + dir_idx] * ag;
        }
        let u_prev = &u;
        let v_prev = &v;
        let a_prev = &a;
        let mut u_tr = u.clone();
        let mut step_converged = false;

        for _iter in STICK_NEWTON.iters() {
            let mut f_int = vec![0.0; nd];
            let mut kt_x = vec![0.0; n];
            let mut kt_y = vec![0.0; n];
            for i in 0..n {
                let rel = relative_at_rigidity(&u_tr, i, &lm.spatial);
                let (qx, kx) = springs_x[i].trial(rel[0]);
                let (qy, ky) = springs_y[i].trial(rel[1]);
                kt_x[i] = kx.max(1e-6);
                kt_y[i] = ky.max(1e-6);
                let mz = lm.spatial[i].kr.max(0.0) * rel[2];
                add_story_force(&mut f_int, i, &lm.spatial, qx, qy, mz);
            }
            let a_tr: Vec<f64> = (0..nd)
                .map(|i| {
                    c1 * (u_tr[i] - u_prev[i])
                        - (1.0 / (beta * dt)) * v_prev[i]
                        - (1.0 / (2.0 * beta) - 1.0) * a_prev[i]
                })
                .collect();
            let v_tr: Vec<f64> = (0..nd)
                .map(|i| v_prev[i] + dt * ((1.0 - gamma) * a_prev[i] + gamma * a_tr[i]))
                .collect();
            let cv = k_matvec(&k_init, &v_tr, nd, a1);
            let mut r = vec![0.0; nd];
            let mut rnorm = 0.0;
            for i in 0..nd {
                r[i] = p[i] - mass[i] * a_tr[i] - cv[i] - f_int[i];
                rnorm += r[i] * r[i];
            }
            let ma: Vec<f64> = (0..nd).map(|i| mass[i] * a_tr[i]).collect();
            let scale = crate::common::newton::dynamic_force_scale(&p, &ma, &cv);
            peak_force_scale = peak_force_scale.max(scale);
            let ref_norm = crate::common::newton::dynamic_reference_norm(scale, peak_force_scale);
            if STICK_NEWTON.converged(rnorm.sqrt(), ref_norm) {
                step_converged = true;
                break;
            }
            let kt = assemble_spatial_k(&lm.spatial, |i, c| match c {
                0 => kt_x[i] / lm.spatial[i].skeleton_x.k1.max(1e-9),
                1 => kt_y[i] / lm.spatial[i].skeleton_y.k1.max(1e-9),
                _ => 1.0,
            });
            let mut keff = vec![0.0; nd * nd];
            let cd = c2 * a1;
            for i in 0..nd {
                for j in 0..nd {
                    keff[i * nd + j] = kt[i * nd + j] + cd * k_init[i * nd + j];
                }
                keff[i * nd + i] += c1 * mass[i];
            }
            let du = solve_dense(&keff, &r, nd);
            for i in 0..nd {
                u_tr[i] += du[i];
            }
        }
        if !step_converged {
            non_converged_steps += 1;
        }
        for s in springs_x.iter_mut().chain(springs_y.iter_mut()) {
            s.commit();
        }
        let a_new: Vec<f64> = (0..nd)
            .map(|i| {
                c1 * (u_tr[i] - u_prev[i])
                    - (1.0 / (beta * dt)) * v_prev[i]
                    - (1.0 / (2.0 * beta) - 1.0) * a_prev[i]
            })
            .collect();
        let v_new: Vec<f64> = (0..nd)
            .map(|i| v_prev[i] + dt * ((1.0 - gamma) * a_prev[i] + gamma * a_new[i]))
            .collect();
        u = u_tr;
        v = v_new;
        a = a_new;

        for i in 0..n {
            let rel = relative_at_rigidity(&u, i, &lm.spatial);
            peak_drift[i] = peak_drift[i].max(drift_dir.accumulate(i, rel[0], rel[1]));
            let (qx, _) = springs_x[i].probe(rel[0]);
            let (qy, _) = springs_y[i].probe(rel[1]);
            peak_shear[i] = peak_shear[i].max(shear_dir.accumulate(i, qx, qy));
        }
        time.push(step as f64 * dt);
        let top = 3 * (n - 1) + dir_idx;
        roof.push(u[top]);
        let mut frame = vec![[0.0; 3]; n];
        for i in 0..n {
            frame[i] = [u[3 * i], u[3 * i + 1], u[3 * i + 2]];
        }
        floor_disp.push(frame);
    }

    let dyx: Vec<f64> = lm.spatial.iter().map(|s| s.skeleton_x.d2).collect();
    let dyy: Vec<f64> = lm.spatial.iter().map(|s| s.skeleton_y.d2).collect();
    let ductility_dir = dir_ductility(&drift_dir, &dyx, &dyy);
    let ductility = ductility_dir.story_max();

    StickResponse {
        total_ductility_dir: None,
        story_total_ductility: None,
        bending_rotations: Vec::new(),
        time,
        roof_disp: roof,
        story_peak_drift: peak_drift,
        story_peak_shear: peak_shear,
        story_ductility: ductility,
        non_converged_steps,
        floor_disp,
        drift_dir,
        shear_dir,
        ductility_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        LumpedMassModel, LumpedMassType, LumpedStiffnessSource, StickDim, StorySpatial, StoryStick,
        StoryTrilinear,
    };
    use super::*;
    use crate::statics::analysis::SeismicDir;

    #[test]
    fn different_floor_centers_preserve_rigid_rotation_and_common_point_static_drift() {
        let spatial: Vec<_> = [0.0, 1000.0]
            .into_iter()
            .enumerate()
            .map(|(i, y)| StorySpatial {
                mass_xy: [0.0, y],
                rigidity_xy: [0.0, 500.0],
                j: 1e8,
                kr: 1e6,
                k1_x: 100.0 * (i + 1) as f64,
                k1_y: 100.0,
                skeleton_x: StoryTrilinear::elastic(100.0 * (i + 1) as f64),
                skeleton_y: StoryTrilinear::elastic(100.0),
            })
            .collect();
        let rigid = [0.0, 0.0, 0.001, -1.0, 0.0, 0.001];
        assert_eq!(relative_at_rigidity(&rigid, 1, &spatial), [0.0; 3]);
        let mut internal = vec![0.0; 6];
        add_story_force(&mut internal, 1, &spatial, 100.0, 0.0, 0.0);
        assert_eq!(internal, vec![-100.0, 0.0, 50000.0, 100.0, 0.0, 50000.0]);
        assert_eq!(
            internal.iter().zip(rigid).map(|(f, u)| f * u).sum::<f64>(),
            0.0
        );
        let k = assemble_spatial_k(&spatial, |_, _| 1.0);
        let u = solve_dense(&k, &[100.0, 0.0, -50000.0, 100.0, 0.0, 50000.0], 6);
        for (actual, expected) in u.iter().zip([2.0, 0.0, 0.0, 2.5, 0.0, 0.0]) {
            assert!((actual - expected).abs() < 1e-8, "{u:?}");
        }
        assert!((relative_at_rigidity(&u, 0, &spatial)[0] - 2.0).abs() < 1e-8);
        assert!((relative_at_rigidity(&u, 1, &spatial)[0] - 0.5).abs() < 1e-8);
    }

    #[test]
    fn spatial_eigen_uses_skeleton_k1_not_stale_field() {
        let mass = 10.0;
        let kx = 100.0;
        let ky = 400.0;
        let j = 1.0e8;
        let kr = 1.0e10;
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            stories: vec![StoryStick {
                story: sepika_core::ids::StoryId(0),
                mass,
                height: 3000.0,
                skeleton: StoryTrilinear::elastic(kx),
            }],
            dim: StickDim::Spatial,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: true,
            spatial: vec![StorySpatial {
                j,
                mass_xy: [0.0, 0.0],
                rigidity_xy: [0.0, 0.0],
                k1_x: 1.0,
                k1_y: 1.0,
                kr,
                skeleton_x: StoryTrilinear::elastic(kx),
                skeleton_y: StoryTrilinear::elastic(ky),
            }],
        };
        let modal = lumped_mass_eigen_spatial(&lm, 3).unwrap();
        assert_eq!(modal.period.len(), 3);
        let expected = [
            2.0 * std::f64::consts::PI * (mass / kx).sqrt(),
            2.0 * std::f64::consts::PI * (mass / ky).sqrt(),
            2.0 * std::f64::consts::PI * (j / kr).sqrt(),
        ];
        for (g, want) in modal.period.iter().zip(expected.iter()) {
            assert!((g - want).abs() / want < 1e-6, "period {g} vs {want}");
        }
    }

    #[test]
    fn spatial_th_x_motion_fills_directional_peaks() {
        let mass = 10.0;
        let k = 100.0;
        let lm = LumpedMassModel {
            identifications: Vec::new(),
            bending: Vec::new(),
            model_type: LumpedMassType::EquivalentShear,
            stories: vec![StoryStick {
                story: sepika_core::ids::StoryId(0),
                mass,
                height: 3000.0,
                skeleton: StoryTrilinear::elastic(k),
            }],
            dim: StickDim::Spatial,
            stiffness_source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: false,
            spatial: vec![StorySpatial {
                j: 1.0e8,
                mass_xy: [0.0, 0.0],
                rigidity_xy: [0.0, 0.0],
                k1_x: k,
                k1_y: k,
                kr: 1.0e10,
                skeleton_x: StoryTrilinear::elastic(k),
                skeleton_y: StoryTrilinear::elastic(k),
            }],
        };
        let dt = 0.01;
        let accel: Vec<f64> = (0..200)
            .map(|i| 500.0 * (2.0 * std::f64::consts::PI * 0.5 * i as f64 * dt).sin())
            .collect();
        let res = lumped_mass_time_history_spatial(&lm, &accel, dt, 0.02);
        let dx = res.drift_dir.x[0];
        let dy = res.drift_dir.y[0];
        assert!(dx > 1e-3, "X 加振なのに層間Xがほぼ 0: {dx}");
        assert!(
            dy < 0.05 * dx,
            "偏心なしなら層間Yは X の数%以下: Y={dy} X={dx}"
        );
        let s2 = std::f64::consts::FRAC_1_SQRT_2;
        assert!(
            (res.drift_dir.deg45[0] - dx * s2).abs() / dx < 0.05,
            "45° は X 成分の 1/√2: 45={}, X={dx}",
            res.drift_dir.deg45[0]
        );
        assert!(
            (res.story_peak_drift[0] - dx).abs() / dx < 0.05,
            "合成最大は層間Xに一致: max={} X={dx}",
            res.story_peak_drift[0]
        );
        assert!((res.ductility_dir.x[0] - res.story_ductility[0]).abs() < 1e-12);
        assert!(res.ductility_dir.y[0] < res.ductility_dir.x[0]);
    }
}
