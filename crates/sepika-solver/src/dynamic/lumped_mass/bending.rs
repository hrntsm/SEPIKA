//! 弾性曲げ要素と履歴せん断ばねの直列接続。

use super::model::{LumpedMassModel, StickDim, StoryTrilinear};
use super::time_history::{dir_ductility, StickDirPeaks, StickResponse, STICK_NEWTON};
use super::LumpedMassModal;
use crate::statics::analysis::SeismicDir;
use faer::Side;
use sepika_material::{HysteresisMaterial, HysteresisRule, UniaxialMaterial};
use sepika_math::{dense_lu::CheckedDenseLu, solver::SolveError};

fn invalid(message: impl Into<String>) -> SolveError {
    SolveError::InvalidInput(message.into())
}

/// 層せん断力 `[N]` から基部固定の曲げ層間変位 `[mm]` を下層順に返す。
/// 階高 `[mm]`・EI [N·mm²] は正の有限値、配列長は同一であること。
pub fn bending_drifts(heights: &[f64], ei: &[f64], shear: &[f64]) -> Result<Vec<f64>, SolveError> {
    if heights.len() != ei.len() || ei.len() != shear.len() {
        return Err(invalid("曲げ変形の配列長が一致しません"));
    }
    let mut moment = vec![0.0; ei.len() + 1];
    for i in (0..ei.len()).rev() {
        if !heights[i].is_finite()
            || heights[i] <= 0.0
            || !ei[i].is_finite()
            || ei[i] <= 0.0
            || !shear[i].is_finite()
        {
            return Err(invalid(format!(
                "層 {} の曲げ入力が不正です: h={}, EI={}, Q={}",
                i + 1,
                heights[i],
                ei[i],
                shear[i]
            )));
        }
        moment[i] = moment[i + 1] + shear[i] * heights[i];
    }
    let mut theta = 0.0;
    let mut drift = Vec::with_capacity(ei.len());
    for i in 0..ei.len() {
        let h = heights[i];
        drift.push(h * theta + h * h * (2.0 * moment[i] + moment[i + 1]) / (6.0 * ei[i]));
        theta += h * (moment[i] + moment[i + 1]) / (2.0 * ei[i]);
    }
    if drift.iter().any(|x| !x.is_finite()) {
        return Err(invalid("曲げ変形が非有限です"));
    }
    Ok(drift)
}

/// [ub, θb, ut, θt] の弾性要素剛性。h `[mm]`、EI [N·mm²]、Ks [N/mm] は正の有限値。
pub fn bending_shear_element(h: f64, ei: f64, ks: f64) -> Result<[[f64; 4]; 4], SolveError> {
    if [h, ei, ks].iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err(invalid("曲げせん断要素の h・EI・Ks が不正です"));
    }
    let kb = 12.0 * ei / h.powi(3);
    let kt = 1.0 / (1.0 / kb + 1.0 / ks);
    let b = [-1.0, -h / 2.0, 1.0, -h / 2.0];
    let r = [0.0, -1.0, 0.0, 1.0];
    Ok(std::array::from_fn(|i| {
        std::array::from_fn(|j| kt * b[i] * b[j] + ei / h * r[i] * r[j])
    }))
}

fn spring(sk: &StoryTrilinear) -> HysteresisMaterial {
    HysteresisMaterial::new(HysteresisRule::MaxPointOriented {
        crack: (sk.q1, sk.d1),
        yield_point: (sk.q2, sk.d2),
        ultimate: (sk.q3, sk.d3),
    })
}

fn stride(lm: &LumpedMassModel) -> usize {
    if lm.dim == StickDim::Spatial {
        5
    } else {
        2
    }
}

fn mass(lm: &LumpedMassModel) -> Result<Vec<f64>, SolveError> {
    let n = lm.stories.len();
    let ns = stride(lm);
    let nd = n * ns;
    if lm.bending.len() != n || (ns == 5 && lm.spatial.len() != n) {
        return Err(invalid("曲げせん断型の層諸元が不足しています"));
    }
    let mut m = vec![0.0; nd * nd];
    for i in 0..n {
        let s = &lm.stories[i];
        let b = lm.bending[i];
        let needed = if ns == 5 {
            vec![s.mass, s.height, b.ei_x, b.ei_y, b.inertia[0], b.inertia[1]]
        } else {
            vec![
                s.mass,
                s.height,
                if lm.dir == SeismicDir::X {
                    b.ei_y
                } else {
                    b.ei_x
                },
                if lm.dir == SeismicDir::X {
                    b.inertia[1]
                } else {
                    b.inertia[0]
                },
            ]
        };
        if needed.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(invalid(format!(
                "層 {:?} の質量・階高・EI・曲げ回転慣性が不正です: {:?}",
                s.story, needed
            )));
        }
        let d = i * ns;
        m[d * nd + d] = s.mass;
        if ns == 2 {
            m[(d + 1) * nd + d + 1] = if lm.dir == SeismicDir::X {
                b.inertia[1]
            } else {
                b.inertia[0]
            };
        } else {
            let [jx, jy, xy] = b.inertia;
            if !xy.is_finite() || jx * jy - xy * xy <= 0.0 {
                return Err(invalid(format!(
                    "層 {:?} の水平軸回転慣性行列が正定値ではありません: {:?}",
                    s.story, b.inertia
                )));
            }
            let jz = lm.spatial[i].j;
            if !jz.is_finite() || (jz - jx - jy).abs() > 1e-9 * jz || jz <= 0.0 {
                return Err(invalid(format!(
                    "層 {:?} の Jz={} と Jx+Jy={} が整合しません",
                    s.story,
                    jz,
                    jx + jy
                )));
            }
            m[(d + 1) * nd + d + 1] = s.mass;
            m[(d + 2) * nd + d + 2] = jz;
            m[(d + 3) * nd + d + 3] = jx;
            m[(d + 4) * nd + d + 4] = jy;
            m[(d + 3) * nd + d + 4] = -xy;
            m[(d + 4) * nd + d + 3] = -xy;
        }
    }
    Ok(m)
}

fn add_outer(k: &mut [f64], nd: usize, b: &[(usize, f64)], c: f64) {
    for &(i, x) in b {
        for &(j, y) in b {
            k[i * nd + j] += c * x * y;
        }
    }
}
fn dot(b: &[(usize, f64)], u: &[f64]) -> f64 {
    b.iter().map(|&(i, x)| x * u[i]).sum()
}

struct Axis {
    b: Vec<(usize, f64)>,
    r: Vec<(usize, f64)>,
    ei: f64,
    h: f64,
    sk: StoryTrilinear,
    spring: HysteresisMaterial,
    shear: f64,
    deformation: f64,
}
fn axes(lm: &LumpedMassModel) -> Result<Vec<Axis>, SolveError> {
    let ns = stride(lm);
    let mut out = Vec::new();
    for (i, s) in lm.stories.iter().enumerate() {
        for axis in 0..if ns == 5 { 2 } else { 1 } {
            let x = if ns == 5 {
                axis == 0
            } else {
                lm.dir == SeismicDir::X
            };
            let ei = if x {
                lm.bending[i].ei_y
            } else {
                lm.bending[i].ei_x
            };
            let sk = if ns == 2 {
                s.skeleton
            } else if x {
                lm.spatial[i].skeleton_x
            } else {
                lm.spatial[i].skeleton_y
            };
            if !sk.is_valid_spring() {
                return Err(invalid(format!(
                    "層 {:?} のせん断骨格が不正です: {:?}",
                    s.story, sk
                )));
            }
            let t = if ns == 2 { 0 } else { axis };
            let r = if ns == 2 {
                1
            } else if x {
                4
            } else {
                3
            };
            let sign = if ns == 5 && !x { -1.0 } else { 1.0 };
            let mut b = vec![(i * ns + t, 1.0), (i * ns + r, -s.height / 2.0 * sign)];
            let mut rv = vec![(i * ns + r, sign)];
            if ns == 5 {
                let sp = lm.spatial[i];
                let ecc = if x {
                    -(sp.rigidity_xy[1] - sp.mass_xy[1])
                } else {
                    sp.rigidity_xy[0] - sp.mass_xy[0]
                };
                b.push((i * ns + 2, ecc));
                if i > 0 {
                    b.push(((i - 1) * ns + 2, -ecc));
                }
            }
            if i > 0 {
                b.push(((i - 1) * ns + t, -1.0));
                b.push(((i - 1) * ns + r, -s.height / 2.0 * sign));
                rv.push(((i - 1) * ns + r, -sign));
            }
            out.push(Axis {
                b,
                r: rv,
                ei,
                h: s.height,
                sk,
                spring: spring(&sk),
                shear: 0.0,
                deformation: 0.0,
            });
        }
    }
    Ok(out)
}

fn restoring(
    lm: &LumpedMassModel,
    axes: &mut [Axis],
    u: &[f64],
    nonlinear: bool,
) -> Result<(Vec<f64>, Vec<f64>), SolveError> {
    let nd = u.len();
    let mut f = vec![0.0; nd];
    let mut k = vec![0.0; nd * nd];
    for (index, a) in axes.iter_mut().enumerate() {
        let d = dot(&a.b, u);
        let kb = 12.0 * a.ei / a.h.powi(3);
        let (q, kt, s) = if nonlinear {
            let mut s = a.deformation;
            let mut converged = false;
            let mut q = 0.0;
            let mut tangent = 0.0;
            for _ in 0..60 {
                (q, tangent) = a.spring.trial(s);
                let residual = kb * (d - s) - q;
                if residual.abs() <= 1e-10 * (q.abs() + kb * (d - s).abs()).max(1.0) {
                    converged = true;
                    break;
                }
                if !tangent.is_finite() || kb + tangent <= 0.0 {
                    return Err(invalid(format!(
                        "層要素 {} の内部せん断接線が不正です",
                        index + 1
                    )));
                }
                s += residual / (kb + tangent);
            }
            if !converged {
                return Err(SolveError::NonConvergence(format!(
                    "層要素 {} の曲げ・せん断直列釣合いが未収束です",
                    index + 1
                )));
            }
            (q, kb * tangent / (kb + tangent), s)
        } else {
            let kt = 1.0 / (1.0 / kb + 1.0 / a.sk.k1);
            (kt * d, kt, kt * d / a.sk.k1)
        };
        a.shear = q;
        a.deformation = s;
        let moment = a.ei / a.h * dot(&a.r, u);
        for &(i, v) in &a.b {
            f[i] += q * v;
        }
        for &(i, v) in &a.r {
            f[i] += moment * v;
        }
        add_outer(&mut k, nd, &a.b, kt);
        add_outer(&mut k, nd, &a.r, a.ei / a.h);
    }
    if stride(lm) == 5 {
        for (i, s) in lm.spatial.iter().enumerate() {
            if !s.kr.is_finite() || s.kr <= 0.0 {
                return Err(invalid(format!(
                    "層 {} のねじり剛性 KR={} が不正です",
                    i + 1,
                    s.kr
                )));
            }
            let mut b = vec![(i * 5 + 2, 1.0)];
            if i > 0 {
                b.push(((i - 1) * 5 + 2, -1.0));
            }
            let q = s.kr * dot(&b, u);
            for &(j, v) in &b {
                f[j] += v * q;
            }
            add_outer(&mut k, nd, &b, s.kr);
        }
    }
    Ok((f, k))
}

fn mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.chunks(b.len())
        .map(|row| row.iter().zip(b).map(|(x, y)| x * y).sum())
        .collect()
}
fn solve(k: &[f64], r: &[f64]) -> Result<Vec<f64>, SolveError> {
    let n = r.len();
    let d: Vec<_> = (0..n).map(|i| k[i * n + i].abs().sqrt()).collect();
    if d.iter().any(|x| !x.is_finite() || *x == 0.0) {
        return Err(invalid("曲げせん断求解行列の対角が不正です"));
    }
    let a: Vec<_> = (0..n * n).map(|i| k[i] / d[i / n] / d[i % n]).collect();
    let b: Vec<_> = r.iter().zip(&d).map(|(v, s)| v / s).collect();
    let y = CheckedDenseLu::factorize(&a, n)
        .and_then(|lu| lu.solve(&b, 1))
        .map_err(|e| SolveError::NonConvergence(format!("曲げせん断型の求解: {e}")))?;
    Ok(y.iter().zip(d).map(|(v, s)| v / s).collect())
}

pub(crate) fn eigen(lm: &LumpedMassModel, n_modes: usize) -> Result<LumpedMassModal, SolveError> {
    let mass = mass(lm)?;
    let ns = stride(lm);
    let n = lm.stories.len();
    let nd = n * ns;
    if nd == 0 || n_modes == 0 {
        return Ok(LumpedMassModal::default());
    }
    let (_, k) = restoring(lm, &mut axes(lm)?, &vec![0.0; nd], false)?;
    let m = faer::Mat::from_fn(nd, nd, |i, j| mass[i * nd + j]);
    let me = m
        .self_adjoint_eigen(Side::Lower)
        .map_err(|e| SolveError::NonConvergence(format!("曲げせん断質量行列: {e:?}")))?;
    if (0..nd).any(|i| !me.S()[i].is_finite() || me.S()[i] <= 0.0) {
        return Err(invalid("曲げせん断質量行列が正定値ではありません"));
    }
    let t = faer::Mat::from_fn(nd, nd, |i, j| me.U()[(i, j)] / me.S()[j].sqrt());
    let km = faer::Mat::from_fn(nd, nd, |i, j| k[i * nd + j]);
    let a = t.transpose() * km * &t;
    let eig = a
        .self_adjoint_eigen(Side::Lower)
        .map_err(|e| SolveError::NonConvergence(format!("曲げせん断固有値: {e:?}")))?;
    let modes = &t * eig.U();
    let mut result = LumpedMassModal::default();
    for j in 0..n_modes.min(nd) {
        let w2 = eig.S()[j];
        if !w2.is_finite() || w2 <= 0.0 {
            return Err(invalid(format!(
                "曲げせん断の第 {} 固有値が不正です: {}",
                j + 1,
                w2
            )));
        }
        let scale = if ns == 2 {
            modes[((n - 1) * ns, j)]
        } else {
            modes[((n - 1) * ns, j)].hypot(modes[((n - 1) * ns + 1, j)])
        };
        let scale = if scale.abs() > 1e-30 {
            scale
        } else {
            (0..nd).map(|i| modes[(i, j)].abs()).fold(0.0, f64::max)
        };
        let mut xyz = vec![[0.0; 3]; n];
        let mut rotations = vec![[0.0; 2]; n];
        let mut shape = vec![0.0; n];
        for i in 0..n {
            if ns == 2 {
                let axis = usize::from(lm.dir == SeismicDir::Y);
                xyz[i][axis] = modes[(i * ns, j)] / scale;
                rotations[i][1 - axis] =
                    modes[(i * ns + 1, j)] / scale * if axis == 1 { -1.0 } else { 1.0 };
                shape[i] = xyz[i][axis];
            } else {
                for c in 0..3 {
                    xyz[i][c] = modes[(i * ns + c, j)] / scale;
                }
                rotations[i] = [
                    modes[(i * ns + 3, j)] / scale,
                    modes[(i * ns + 4, j)] / scale,
                ];
                shape[i] = xyz[i][0].hypot(xyz[i][1]);
            }
        }
        result.omega2.push(w2);
        result.period.push(2.0 * std::f64::consts::PI / w2.sqrt());
        result.shapes.push(shape);
        result.shapes_xyz.push(xyz);
        result.bending_rotations.push(rotations);
    }
    Ok(result)
}

/// 曲げせん断型の時刻歴。地動加速度 [mm/s²]、dt `[s]`。不正入力・未収束はエラー。
pub fn bending_shear_time_history(
    lm: &LumpedMassModel,
    accel: &[f64],
    dt: f64,
    damping: f64,
) -> Result<StickResponse, SolveError> {
    if lm.model_type != super::model::LumpedMassType::EquivalentBendingShear {
        return Err(invalid(
            "曲げせん断時刻歴には曲げせん断型の質点系を指定してください",
        ));
    }
    let mass = mass(lm)?;
    let ns = stride(lm);
    let n = lm.stories.len();
    let nd = n * ns;
    if nd == 0
        || accel.is_empty()
        || accel.iter().any(|x| !x.is_finite())
        || !dt.is_finite()
        || dt <= 0.0
        || !damping.is_finite()
        || damping < 0.0
    {
        return Err(invalid(
            "曲げせん断時刻歴の質点数・加速度・時間刻み・減衰が不正です",
        ));
    }
    let mut ax = axes(lm)?;
    let (_, ki) = restoring(lm, &mut ax, &vec![0.0; nd], false)?;
    let omega = eigen(lm, 1)?.omega2[0].sqrt();
    let cc = 2.0 * damping / omega;
    let c1 = 4.0 / (dt * dt);
    let c2 = 2.0 / dt;
    let mut influence = vec![0.0; nd];
    for i in 0..n {
        influence[i * ns
            + if ns == 5 && lm.dir == SeismicDir::Y {
                1
            } else {
                0
            }] = 1.0;
    }
    let mg = mul(&mass, &influence);
    let mut u = vec![0.0; nd];
    let mut v = u.clone();
    let mut a: Vec<_> = influence.iter().map(|r| -r * accel[0]).collect();
    let mut out = StickResponse::empty(n);
    out.time.push(0.0);
    out.roof_disp.push(0.0);
    out.floor_disp.push(vec![[0.0; 3]; n]);
    out.bending_rotations.push(vec![[0.0; 2]; n]);
    let mut shear_drift = StickDirPeaks::zeros(n);
    let mut peak_scale = 0.0_f64;
    for (step, &ag) in accel.iter().enumerate().skip(1) {
        let p: Vec<_> = mg.iter().map(|m| -m * ag).collect();
        let mut trial = u.clone();
        let mut converged = false;
        for _ in STICK_NEWTON.iters() {
            let (f, kt) = restoring(lm, &mut ax, &trial, lm.nonlinear)?;
            let at: Vec<_> = (0..nd)
                .map(|i| c1 * (trial[i] - u[i]) - 4.0 / dt * v[i] - a[i])
                .collect();
            let vt: Vec<_> = (0..nd).map(|i| v[i] + dt * 0.5 * (a[i] + at[i])).collect();
            let ma = mul(&mass, &at);
            let cv: Vec<_> = mul(&ki, &vt).into_iter().map(|x| cc * x).collect();
            let r: Vec<_> = (0..nd).map(|i| p[i] - ma[i] - cv[i] - f[i]).collect();
            let scale = crate::common::newton::dynamic_force_scale(&p, &ma, &cv);
            peak_scale = peak_scale.max(scale);
            if STICK_NEWTON.converged(
                r.iter().map(|x| x * x).sum::<f64>().sqrt(),
                crate::common::newton::dynamic_reference_norm(scale, peak_scale),
            ) {
                converged = true;
                break;
            }
            let ke: Vec<_> = (0..nd * nd)
                .map(|i| kt[i] + c1 * mass[i] + c2 * cc * ki[i])
                .collect();
            let du = solve(&ke, &r)?;
            for i in 0..nd {
                trial[i] += du[i];
            }
        }
        if !converged {
            return Err(SolveError::NonConvergence(format!(
                "曲げせん断時刻歴 t={} s の釣合いが未収束です",
                step as f64 * dt
            )));
        }
        let anew: Vec<_> = (0..nd)
            .map(|i| c1 * (trial[i] - u[i]) - 4.0 / dt * v[i] - a[i])
            .collect();
        for i in 0..nd {
            v[i] += dt * 0.5 * (a[i] + anew[i]);
        }
        u = trial;
        a = anew;
        for x in &mut ax {
            if lm.nonlinear {
                x.spring.commit();
            }
        }
        let mut frame = vec![[0.0; 3]; n];
        let mut rot = vec![[0.0; 2]; n];
        for i in 0..n {
            let (dx, dy, qx, qy, sx, sy) = if ns == 2 {
                let d = u[i * ns] - if i > 0 { u[(i - 1) * ns] } else { 0.0 };
                let x = &ax[i];
                let axis = usize::from(lm.dir == SeismicDir::Y);
                frame[i][axis] = u[i * ns];
                rot[i][1 - axis] = u[i * ns + 1] * if axis == 1 { -1.0 } else { 1.0 };
                if axis == 0 {
                    (d, 0.0, x.shear, 0.0, x.deformation, 0.0)
                } else {
                    (0.0, d, 0.0, x.shear, 0.0, x.deformation)
                }
            } else {
                frame[i].copy_from_slice(&u[i * ns..i * ns + 3]);
                rot[i] = [u[i * ns + 3], u[i * ns + 4]];
                let sp = lm.spatial[i];
                let dz = u[i * ns + 2] - if i > 0 { u[(i - 1) * ns + 2] } else { 0.0 };
                let dx = u[i * ns]
                    - if i > 0 { u[(i - 1) * ns] } else { 0.0 }
                    - (sp.rigidity_xy[1] - sp.mass_xy[1]) * dz;
                let dy = u[i * ns + 1] - if i > 0 { u[(i - 1) * ns + 1] } else { 0.0 }
                    + (sp.rigidity_xy[0] - sp.mass_xy[0]) * dz;
                (
                    dx,
                    dy,
                    ax[i * 2].shear,
                    ax[i * 2 + 1].shear,
                    ax[i * 2].deformation,
                    ax[i * 2 + 1].deformation,
                )
            };
            out.story_peak_drift[i] =
                out.story_peak_drift[i].max(out.drift_dir.accumulate(i, dx, dy));
            out.story_peak_shear[i] =
                out.story_peak_shear[i].max(out.shear_dir.accumulate(i, qx, qy));
            shear_drift.accumulate(i, sx, sy);
        }
        out.time.push(step as f64 * dt);
        out.roof_disp
            .push(frame[n - 1][usize::from(lm.dir == SeismicDir::Y)]);
        out.floor_disp.push(frame);
        out.bending_rotations.push(rot);
    }
    let mut d1x = vec![f64::INFINITY; n];
    let mut d1y = d1x.clone();
    for i in 0..n {
        if ns == 5 {
            d1x[i] = ax[i * 2].sk.d1;
            d1y[i] = ax[i * 2 + 1].sk.d1;
        } else if lm.dir == SeismicDir::X {
            d1x[i] = ax[i].sk.d1;
        } else {
            d1y[i] = ax[i].sk.d1;
        }
    }
    out.ductility_dir = dir_ductility(&shear_drift, &d1x, &d1y);
    out.story_ductility = out.ductility_dir.story_max();
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::lumped_mass::{LumpedMassType, StoryBending, StorySpatial, StoryStick};
    use sepika_core::ids::StoryId;

    fn model(spatial: bool) -> LumpedMassModel {
        let mut lm = LumpedMassModel::from_stories(
            LumpedMassType::EquivalentBendingShear,
            vec![StoryStick {
                story: StoryId(0),
                mass: 2.0,
                height: 2.0,
                skeleton: StoryTrilinear::elastic(3.0),
            }],
        );
        lm.nonlinear = false;
        lm.bending = vec![StoryBending {
            ei_x: 16.0,
            ei_y: 8.0,
            inertia: [5.0, 4.0, 1.0],
        }];
        if spatial {
            lm.dim = StickDim::Spatial;
            lm.spatial = vec![StorySpatial {
                j: 9.0,
                mass_xy: [0.0; 2],
                rigidity_xy: [0.0; 2],
                k1_x: 3.0,
                k1_y: 6.0,
                kr: 7.0,
                skeleton_x: StoryTrilinear::elastic(3.0),
                skeleton_y: StoryTrilinear::elastic(6.0),
            }];
        }
        lm
    }
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9 * b.abs().max(1.0), "{a} != {b}");
    }

    #[test]
    fn time_history_entries_reject_wrong_model_and_keep_invalid_input_reason() {
        for spatial in [false, true] {
            let mut lm = model(spatial);
            let error =
                crate::dynamic::lumped_mass::lumped_mass_time_history(&lm, &[0., 1.], 0.01, 0.02)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("せん断型時刻歴"), "{error}");
            lm.model_type = LumpedMassType::EquivalentShear;
            let error = bending_shear_time_history(&lm, &[0., 1.], 0.01, 0.02)
                .unwrap_err()
                .to_string();
            assert!(error.contains("曲げせん断型"), "{error}");
            lm.model_type = LumpedMassType::EquivalentBendingShear;
            lm.stories[0].mass = f64::NAN;
            let error = bending_shear_time_history(&lm, &[0., 1.], 0.01, 0.02)
                .unwrap_err()
                .to_string();
            assert!(error.contains("質量"), "{error}");
            lm.stories[0].mass = 2.;
            lm.bending.clear();
            let error = bending_shear_time_history(&lm, &[0., 1.], 0.01, 0.02)
                .unwrap_err()
                .to_string();
            assert!(error.contains("不足"), "{error}");
        }
        assert!(serde_json::from_str::<LumpedMassType>(r#""BendingShearSeparated""#).is_err());
    }

    #[test]
    fn element_matches_four_by_four_and_fixed_base_cantilever() {
        let k = bending_shear_element(2.0, 8.0, 3.0).unwrap();
        let expected = [
            [2.4, 2.4, -2.4, 2.4],
            [2.4, 6.4, -2.4, -1.6],
            [-2.4, -2.4, 2.4, -2.4],
            [2.4, -1.6, -2.4, 6.4],
        ];
        for i in 0..4 {
            for j in 0..4 {
                near(k[i][j], expected[i][j]);
            }
        }
        let lm = model(false);
        let (_, k) = restoring(&lm, &mut axes(&lm).unwrap(), &[0.0; 2], false).unwrap();
        let u = solve(&k, &[6.0, 0.0]).unwrap();
        near(u[0], 4.0);
        near(u[1], 1.5);
        near(bending_drifts(&[2.0], &[8.0], &[6.0]).unwrap()[0], 2.0);
        near(u[0] - 2.0, 6.0 / 3.0);
    }

    #[test]
    fn infinite_bending_stiffness_converges_to_shear_displacement() {
        let mut lm = model(false);
        lm.bending[0].ei_y = 8e10;
        let (_, k) = restoring(&lm, &mut axes(&lm).unwrap(), &[0.0; 2], false).unwrap();
        let u = solve(&k, &[6.0, 0.0]).unwrap();
        near(u[0], 2.0);
        assert!(u[1].abs() < 1e-9);
    }

    #[test]
    fn two_layer_bending_integrates_linear_moment_exactly() {
        let d = bending_drifts(&[2.0, 3.0], &[8.0, 12.0], &[7.0, 3.0]).unwrap();
        // 下層 M=[23,9]、上層 M=[9,0]、下層上端回転4 rad。
        near(d[0], 55.0 / 12.0);
        near(d[1], 14.25);
        assert!(bending_drifts(&[2.0], &[0.0], &[1.0]).is_err());
    }

    #[test]
    fn planar_mass_and_eigenvalues_match_independent_two_by_two_problem() {
        let lm = model(false);
        assert_eq!(mass(&lm).unwrap(), vec![2.0, 0.0, 0.0, 4.0]);
        let modal = eigen(&lm, 9).unwrap();
        assert_eq!(modal.period.len(), 2);
        near(modal.omega2[0], (2.8 - 3.04_f64.sqrt()) / 2.0);
        near(modal.omega2[1], (2.8 + 3.04_f64.sqrt()) / 2.0);
        let (_, k) = restoring(&lm, &mut axes(&lm).unwrap(), &[0.0; 2], false).unwrap();
        for i in 0..2 {
            let u = modal.shapes[i][0];
            let r = modal.bending_rotations[i][0][1];
            near(u, 1.0);
            near(k[0] * u + k[1] * r, modal.omega2[i] * 2.0 * u);
            near(k[2] * u + k[3] * r, modal.omega2[i] * 4.0 * r);
        }
    }

    #[test]
    fn spatial_inertia_includes_product_and_y_rotation_sign() {
        let lm = model(true);
        let m = mass(&lm).unwrap();
        assert_eq!(m[3 * 5 + 3], 5.0);
        assert_eq!(m[4 * 5 + 4], 4.0);
        assert_eq!(m[3 * 5 + 4], -1.0);
        assert_eq!(m[2 * 5 + 2], 9.0);
        let (_, k) = restoring(&lm, &mut axes(&lm).unwrap(), &[0.0; 5], false).unwrap();
        for i in 0..5 {
            for j in 0..5 {
                near(k[i * 5 + j], k[j * 5 + i]);
            }
        }
        let u = solve(&k, &[6.0, 6.0, 0.0, 0.0, 0.0]).unwrap();
        near(u[0], 4.0);
        near(u[1], 2.0);
        near(u[3], -0.75);
        near(u[4], 1.5);
        let modes = eigen(&lm, 5).unwrap();
        assert_eq!(modes.period.len(), 5);
        for j in 0..5 {
            let v = modes.shapes_xyz[j][0];
            let r = modes.bending_rotations[j][0];
            let x = [v[0], v[1], v[2], r[0], r[1]];
            let kx = mul(&k, &x);
            let mx = mul(&m, &x);
            for c in 0..5 {
                near(kx[c], modes.omega2[j] * mx[c]);
            }
        }
    }

    #[test]
    fn internal_nonlinear_spring_equilibrium_and_commit_share_state() {
        let mut lm = model(false);
        lm.nonlinear = true;
        lm.stories[0].skeleton = StoryTrilinear {
            k1: 3.0,
            d1: 1.0,
            q1: 3.0,
            d2: 3.0,
            q2: 6.0,
            d3: 6.0,
            q3: 7.5,
        };
        let mut ax = axes(&lm).unwrap();
        let mut reference = spring(&lm.stories[0].skeleton);
        for s in [0.5, 2.0, 4.0, 1.0, -2.0, -4.0, 0.0, 3.0] {
            let (q, _) = reference.trial(s);
            let d = s + q / 12.0;
            let (f, k) = restoring(&lm, &mut ax, &[d, 0.0], true).unwrap();
            near(ax[0].deformation, s);
            near(ax[0].shear, q);
            near(f[0], q);
            assert!(k.iter().all(|v| v.is_finite()));
            ax[0].spring.commit();
            reference.commit();
        }
    }

    #[test]
    fn zero_ground_motion_and_nonlinear_response_keep_bending_histories() {
        for spatial in [false, true] {
            let mut lm = model(spatial);
            lm.nonlinear = true;
            let sk = StoryTrilinear {
                k1: 3.0,
                d1: 0.01,
                q1: 0.03,
                d2: 0.03,
                q2: 0.06,
                d3: 0.06,
                q3: 0.075,
            };
            lm.stories[0].skeleton = sk;
            if spatial {
                lm.spatial[0].skeleton_x = sk;
                lm.spatial[0].skeleton_y = sk;
            }
            let zero = bending_shear_time_history(&lm, &[0.0; 10], 0.01, 0.02).unwrap();
            assert!(zero.roof_disp.iter().all(|v| *v == 0.0));
            let wave: Vec<_> = (0..500).map(|i| (i as f64 * 0.03).sin() * 2.0).collect();
            let r = bending_shear_time_history(&lm, &wave, 0.01, 0.02).unwrap();
            assert_eq!(r.time.len(), wave.len());
            assert_eq!(r.bending_rotations.len(), wave.len());
            assert!(r.story_peak_drift[0] > 0.0);
            assert!(r.story_ductility[0] > 1.0);
            assert_eq!(r.non_converged_steps, 0);
        }
    }

    #[test]
    fn invalid_mass_inertia_and_stiffness_are_rejected() {
        let mut lm = model(true);
        lm.bending[0].inertia[2] = 10.0;
        assert!(eigen(&lm, 1).is_err());
        lm = model(false);
        lm.bending[0].inertia[1] = 0.0;
        assert!(eigen(&lm, 1).is_err());
        lm = model(false);
        lm.stories[0].skeleton.k1 = -1.0;
        assert!(eigen(&lm, 1).is_err());
    }
    #[test]
    fn bilinear_reduction_keeps_its_coincident_corner_and_existing_hysteresis() {
        let curve: Vec<_> = (1..=30)
            .map(|i| {
                let d = i as f64 * 0.1;
                (d, 50.0 * d.min(2.0) + 5.0 * (d - 2.0).max(0.0))
            })
            .collect();
        let sk = crate::dynamic::lumped_mass::fit_story_trilinear(&curve, 0.9);
        assert!((sk.k1 - 50.0).abs() < 1.0);
        assert!((sk.d3 - 3.0).abs() < 1e-6 && (sk.q3 - 105.0).abs() < 1e-6);
        assert_eq!(sk.d1, 2.0);
        assert_eq!(sk.d2, 2.0);
        assert!(sk.is_valid_spring());
        let mut lm = model(false);
        lm.nonlinear = true;
        lm.stories[0].skeleton = sk;
        let mut axis = axes(&lm).unwrap();
        for s in [0.5_f64, 1.5, 2.0, 2.5, 3.0] {
            let q = 50.0 * s.min(2.0) + 5.0 * (s - 2.0).max(0.0);
            let (f, _) = restoring(&lm, &mut axis, &[s + q / 12.0, 0.0], true).unwrap();
            near(axis[0].deformation, s);
            near(f[0], q);
            axis[0].spring.commit();
        }
        assert_eq!(lm.stories[0].skeleton, sk);
        assert!(eigen(&lm, 2).is_ok());
        assert!(bending_shear_time_history(&lm, &[0.0, 100.0, -100.0, 0.0], 0.1, 0.02).is_ok());
    }

    #[test]
    fn coincident_corners_accept_only_roundoff_equal_force_without_unit_scale_bias() {
        let sk = StoryTrilinear {
            k1: 50.0,
            d1: 2.0,
            q1: 100.0,
            d2: 2.0,
            q2: 100.0 - f64::EPSILON * 100.0,
            d3: 3.0,
            q3: 105.0,
        };
        for force_scale in [1e-9, 1.0, 1e9] {
            for length_scale in [1e-6, 1.0, 1e6] {
                let mut scaled = sk;
                scaled.k1 *= force_scale / length_scale;
                scaled.d1 *= length_scale;
                scaled.d2 *= length_scale;
                scaled.d3 *= length_scale;
                scaled.q1 *= force_scale;
                scaled.q2 *= force_scale;
                scaled.q3 *= force_scale;
                assert!(scaled.is_valid_spring());
                scaled.q2 = scaled.q1 * 0.99;
                assert!(!scaled.is_valid_spring());
                let mut lm = model(false);
                lm.nonlinear = true;
                lm.stories[0].skeleton = scaled;
                assert!(eigen(&lm, 1).is_err());
            }
        }
        let mut bad = sk;
        bad.k1 = -50.0;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.k1 = 40.0;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.d2 = 1.0;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.d2 = 2.5;
        bad.q2 = 90.0;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.q2 = f64::NAN;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.d1 = f64::MIN_POSITIVE;
        bad.q1 = f64::MAX;
        assert!(!bad.is_valid_spring());
        bad = sk;
        bad.d3 = bad.d2;
        bad.q3 = bad.q2;
        assert!(bad.is_valid_spring());
    }
}
