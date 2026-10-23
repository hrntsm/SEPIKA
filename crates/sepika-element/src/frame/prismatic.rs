//! 直線材（等断面の線材）が共有する定式化。
//!
//! いずれも要素局所系（節点自由度 12）で返す。全体系への回転は呼び出し側の責務とする。

use crate::behavior::LocalMat;
use sepika_core::model::SectionMassProperties;
use sepika_math::dense_lu::CheckedDenseLu;

/// 材端解放を静縮約した局所剛性 12×12。
///
/// `releases` は解放する要素端回転自由度と、節点回転との間に挟む回転ばね剛性
/// `k_s` [N·mm/rad] の組。ピンは `k_s = 0`、空なら `k_elem` をそのまま返す。
///
/// 内部並びは \[外部 0..11（節点 ux,uy,uz,rx,ry,rz ×2）, 内部 12..（解放した
/// 要素端回転を `releases` の順に並べる）\] とし、内部自由度を静縮約する。
///
/// `releases` は最大 6 個。
///
/// 非駆動・非連成の両端共通ねじり回転を除き、縮約不能・非有限なら `None`。
pub(crate) fn condense_end_releases(
    k_elem: &LocalMat,
    releases: &[(usize, f64)],
) -> Option<LocalMat> {
    condense_expanded(
        k_elem,
        None,
        SectionMassProperties::default(),
        0.0,
        0.0,
        releases,
    )
    .map(|(_, k)| k)
}

/// 端部解放を質量へ反映した局所質量を返す。
///
/// `Kbb` が特異または非有限の場合は `None`。内部回転の慣性ゲージは除去しない。
pub(crate) fn condense_end_releases_with_mass(
    k_elem: &LocalMat,
    m_elem: &LocalMat,
    properties: SectionMassProperties,
    li: f64,
    lj: f64,
    releases: &[(usize, f64)],
) -> Option<LocalMat> {
    condense_expanded(k_elem, Some(m_elem), properties, li, lj, releases).and_then(|(m, _)| m)
}

pub(crate) fn mass_without_end_releases(
    m_elem: &LocalMat,
    properties: SectionMassProperties,
    li: f64,
    lj: f64,
) -> LocalMat {
    let mut mass = crate::frame::rigid_arm::transform_mass(m_elem, li, lj);
    let rigid = crate::frame::rigid_arm::rigid_zone_mass(properties, li, lj);
    for i in 0..12 {
        for j in 0..12 {
            mass.set(i, j, mass.get(i, j) + rigid.get(i, j));
        }
    }
    mass
}

fn condense_expanded(
    k_elem: &LocalMat,
    m_elem: Option<&LocalMat>,
    properties: SectionMassProperties,
    li: f64,
    lj: f64,
    releases: &[(usize, f64)],
) -> Option<(Option<LocalMat>, LocalMat)> {
    const NA: usize = 12;

    if ((m_elem.is_none() || !releases.is_empty()) && k_elem.data.iter().any(|v| !v.is_finite()))
        || releases.iter().any(|&(_, k)| !k.is_finite())
    {
        return None;
    }

    if releases.is_empty() {
        let k = LocalMat {
            n: NA,
            data: k_elem.data.clone(),
        };
        let m = m_elem.map_or_else(
            || LocalMat::zeros(NA),
            |m| mass_without_end_releases(m, properties, li, lj),
        );
        return Some((m_elem.map(|_| m), k));
    }

    let nb = releases.len();
    let n = NA + nb;
    debug_assert!(
        n <= 18,
        "condense_end_releases: 解放自由度は両端の回転 6 まで"
    );
    debug_assert!(
        releases.iter().all(|&(r, _)| r < NA),
        "condense_end_releases: 解放自由度が要素自由度の範囲外"
    );
    debug_assert!(
        releases
            .iter()
            .enumerate()
            .all(|(i, &(r, _))| releases[..i].iter().all(|&(q, _)| q != r)),
        "condense_end_releases: 解放自由度が重複している"
    );
    let mut tr = [[0.0_f64; 12]; 12];
    for i in 0..NA {
        tr[i][i] = 1.0;
    }
    tr[1][5] = li;
    tr[2][4] = -li;
    tr[7][11] = -lj;
    tr[8][10] = lj;

    let mut e = [[0.0_f64; 18]; 12];
    for row in 0..NA {
        if let Some((idx, _)) = releases.iter().enumerate().find(|(_, &(r, _))| r == row) {
            e[row][NA + idx] = 1.0;
        } else {
            for col in 0..NA {
                e[row][col] = tr[row][col];
            }
        }
    }

    let mut k = [0.0_f64; 324];
    let mut m = [0.0_f64; 324];
    for i in 0..n {
        for j in 0..n {
            for a in 0..NA {
                for b in 0..NA {
                    k[i * n + j] += e[a][i] * k_elem.get(a, b) * e[b][j];
                    if let Some(m_elem) = m_elem {
                        m[i * n + j] += e[a][i] * m_elem.get(a, b) * e[b][j];
                    }
                }
            }
        }
    }
    let rigid = crate::frame::rigid_arm::rigid_zone_mass(properties, li, lj);
    for i in 0..NA {
        for j in 0..NA {
            if m_elem.is_some() {
                m[i * n + j] += rigid.get(i, j);
            }
        }
    }

    for (idx, &(r, ks)) in releases.iter().enumerate() {
        let ir = NA + idx;
        k[r * n + r] += ks;
        k[ir * n + ir] += ks;
        k[r * n + ir] -= ks;
        k[ir * n + r] -= ks;
    }

    let gauge = if m_elem.is_none() {
        free_common_torsion_gauge(k_elem, releases)
    } else {
        None
    };
    let active: Vec<_> = (0..nb).filter(|index| Some(*index) != gauge).collect();
    let nf = active.len();
    let mut kbb = vec![0.0; nf * nf];
    let mut kba = vec![0.0; nf * NA];
    for (i, &a) in active.iter().enumerate() {
        for (j, &b) in active.iter().enumerate() {
            kbb[i * nf + j] = k[(NA + a) * n + NA + b];
        }
        for j in 0..NA {
            kba[i * NA + j] = k[(NA + a) * n + j];
        }
    }
    let lu = CheckedDenseLu::factorize(&kbb, nf).ok()?;
    let solution = lu.solve(&kba, NA).ok()?;
    let mut kstar = LocalMat::zeros(NA);
    for i in 0..NA {
        for j in 0..NA {
            let mut value = k[i * n + j];
            for (a, &index) in active.iter().enumerate() {
                value -= k[i * n + NA + index] * solution[a * NA + j];
            }
            if !value.is_finite() {
                return None;
            }
            kstar.set(i, j, value);
        }
    }
    let mut r = [[0.0_f64; NA]; 18];
    for i in 0..NA {
        r[i][i] = 1.0;
    }
    for (a, &index) in active.iter().enumerate() {
        for j in 0..NA {
            r[NA + index][j] = -solution[a * NA + j];
        }
    }
    let mut mstar = LocalMat::zeros(NA);
    for i in 0..NA {
        for j in 0..NA {
            let mut s = 0.0;
            for a in 0..n {
                for b in 0..n {
                    s += r[a][i] * m[a * n + b] * r[b][j];
                }
            }
            mstar.set(i, j, s);
        }
    }
    Some((Some(mstar), kstar))
}

fn free_common_torsion_gauge(k: &LocalMat, releases: &[(usize, f64)]) -> Option<usize> {
    let i = releases
        .iter()
        .position(|&(dof, ks)| dof == 3 && ks == 0.0)?;
    let j = releases
        .iter()
        .position(|&(dof, ks)| dof == 9 && ks == 0.0)?;
    let t = k.get(3, 3);
    if t <= 0.0 || !t.is_finite() || k.get(9, 9) != t || k.get(3, 9) != -t || k.get(9, 3) != -t {
        return None;
    }
    for dof in (0..12).filter(|&dof| dof != 3 && dof != 9) {
        if k.get(3, dof) != 0.0
            || k.get(9, dof) != 0.0
            || k.get(dof, 3) != 0.0
            || k.get(dof, 9) != 0.0
        {
            return None;
        }
    }
    // 両端共通回転は外部から駆動されず、差回転だけがエネルギーを持つ。
    Some(i.max(j))
}

/// 軸力 `n_axial` による幾何剛性を局所 12×12 で返す。
///
/// 可撓長 `flex_len`（= L − λi − λj）で組み、可撓端自由度を剛体アームで節点自由度へ写す。
/// 可撓長が実質ゼロの部材はゼロ行列を返す。
///
/// xz 面（uz・ry）では並進-回転の結合項の符号が xy 面（uy・rz）と逆になる。
pub(crate) fn geometric_stiffness(
    n_axial: f64,
    flex_len: f64,
    rigid_i: f64,
    rigid_j: f64,
) -> LocalMat {
    let l = flex_len;
    if l < 1e-12 {
        return LocalMat::zeros(12);
    }
    let c = n_axial / l;
    let mut kg = LocalMat::zeros(12);
    {
        let mut s = |i: usize, j: usize, v: f64| {
            kg.set(i, j, v);
            if i != j {
                kg.set(j, i, v);
            }
        };
        s(1, 1, c * 6.0 / 5.0);
        s(7, 7, c * 6.0 / 5.0);
        s(1, 7, -c * 6.0 / 5.0);
        s(1, 5, c * l / 10.0);
        s(1, 11, c * l / 10.0);
        s(5, 7, -c * l / 10.0);
        s(7, 11, -c * l / 10.0);
        s(5, 5, c * 2.0 * l * l / 15.0);
        s(11, 11, c * 2.0 * l * l / 15.0);
        s(5, 11, -c * l * l / 30.0);
        s(2, 2, c * 6.0 / 5.0);
        s(8, 8, c * 6.0 / 5.0);
        s(2, 8, -c * 6.0 / 5.0);
        s(2, 4, -c * l / 10.0);
        s(2, 10, -c * l / 10.0);
        s(4, 8, c * l / 10.0);
        s(8, 10, c * l / 10.0);
        s(4, 4, c * 2.0 * l * l / 15.0);
        s(10, 10, c * 2.0 * l * l / 15.0);
        s(4, 10, -c * l * l / 30.0);
    }
    crate::frame::rigid_arm::transform_stiffness(&kg, rigid_i, rigid_j)
}

/// 集中質量（対角）の局所 12×12。並進 3 成分へ `mass/2` ずつ配る。
///
/// 並進 3 成分が等しい対角行列は回転不変（Rᵀ·(m/2)I·R = (m/2)I）のため、
/// 呼び出し側で全体系へ回す必要はない。
pub(crate) fn lumped_mass(mass: f64) -> LocalMat {
    let mut mm = LocalMat::zeros(12);
    for d in [0, 1, 2, 6, 7, 8] {
        mm.set(d, d, mass / 2.0);
    }
    mm
}

/// 整合質量の局所 12×12。
///
/// `torsion_term` は部材軸まわりの回転慣性 [t·mm²]。0 を渡すと
/// ねじれ自由度（rx）の質量を持たない行列になる。
///
///   Uy-Rz 面: \[Uy_i=1, Rz_i=5, Uy_j=7, Rz_j=11\]
///   Uz-Ry 面: \[Uz_i=2, Ry_i=4, Uz_j=8, Ry_j=10\]（回転符号は逆）
///
/// 呼び出し側で要素局所系から全体系へ回すこと。
#[cfg(test)]
pub(crate) fn consistent_mass(mass: f64, l: f64, torsion_term: f64) -> LocalMat {
    if l <= 0.0 {
        return LocalMat::zeros(12);
    }
    let properties = SectionMassProperties {
        mass_per_length: mass / l,
        rotary_inertia_y_per_length: 0.0,
        rotary_inertia_z_per_length: 0.0,
    };
    consistent_mass_components(
        properties.mass_per_length,
        0.0,
        0.0,
        (torsion_term * 6.0 / l).max(0.0),
        l,
        0.0,
        0.0,
    )
}

/// Timoshenko 形状関数による局所整合質量 12×12。
///
/// `phi_xy`/`phi_xz` はそれぞれ `uy-rz`／`uz-ry` 曲げ面のせん断変形係数。
/// 各曲げ面の回転慣性は、断面の y／z 軸まわりの質量二次モーメントを要素局所軸へ
/// 対応付けて用いる。
pub(crate) fn consistent_mass_timoshenko(
    properties: SectionMassProperties,
    l: f64,
    phi_xy: f64,
    phi_xz: f64,
) -> LocalMat {
    consistent_mass_components(
        properties.mass_per_length,
        properties.rotary_inertia_z_per_length,
        properties.rotary_inertia_y_per_length,
        properties.polar_inertia_per_length(),
        l,
        phi_xy,
        phi_xz,
    )
}

fn consistent_mass_components(
    mass_per_length: f64,
    rotary_inertia_y_per_length: f64,
    rotary_inertia_z_per_length: f64,
    polar_inertia_per_length: f64,
    l: f64,
    phi_xy: f64,
    phi_xz: f64,
) -> LocalMat {
    if l <= 0.0 {
        return LocalMat::zeros(12);
    }

    let mut mm = LocalMat::zeros(12);
    let gauss = [
        (-0.8611363115940526, 0.3478548451374538),
        (-0.3399810435848563, 0.6521451548625461),
        (0.3399810435848563, 0.6521451548625461),
        (0.8611363115940526, 0.3478548451374538),
    ];
    for (xi, weight) in gauss {
        let s = (xi + 1.0) / 2.0;
        let weight = weight * l / 2.0;
        let n = [1.0 - s, s];
        for a in 0..2 {
            for b in 0..2 {
                let v = mass_per_length.max(0.0) * n[a] * n[b] * weight;
                mm.set(a * 6, b * 6, mm.get(a * 6, b * 6) + v);
                mm.set(
                    3 + a * 6,
                    3 + b * 6,
                    mm.get(3 + a * 6, 3 + b * 6)
                        + polar_inertia_per_length.max(0.0) * n[a] * n[b] * weight,
                );
            }
        }

        add_bending_mass(
            &mut mm,
            [1, 5, 7, 11],
            1.0,
            mass_per_length,
            rotary_inertia_y_per_length,
            l,
            phi_xy,
            s,
            weight,
        );
        add_bending_mass(
            &mut mm,
            [2, 4, 8, 10],
            -1.0,
            mass_per_length,
            rotary_inertia_z_per_length,
            l,
            phi_xz,
            s,
            weight,
        );
    }
    mm
}

#[allow(clippy::too_many_arguments)]
fn add_bending_mass(
    mm: &mut LocalMat,
    indices: [usize; 4],
    rotation_sign: f64,
    mass_per_length: f64,
    rotary_inertia_per_length: f64,
    l: f64,
    phi: f64,
    s: f64,
    weight: f64,
) {
    let (mut n_v, mut n_theta) = timoshenko_shape_functions(s, l, phi);
    for index in [1, 3] {
        n_v[index] *= rotation_sign;
    }
    for index in [0, 2] {
        n_theta[index] *= rotation_sign;
    }
    for a in 0..4 {
        for b in 0..4 {
            let value = (mass_per_length.max(0.0) * n_v[a] * n_v[b]
                + rotary_inertia_per_length.max(0.0) * n_theta[a] * n_theta[b])
                * weight;
            mm.set(
                indices[a],
                indices[b],
                mm.get(indices[a], indices[b]) + value,
            );
        }
    }
}

fn timoshenko_shape_functions(s: f64, l: f64, phi: f64) -> ([f64; 4], [f64; 4]) {
    let phi = phi.max(0.0);
    let inv = 1.0 / (1.0 + phi);
    let s2 = s * s;
    let s3 = s2 * s;
    let s_one_minus = s * (1.0 - s);
    let n_v = [
        (1.0 + phi - phi * s - 3.0 * s2 + 2.0 * s3) * inv,
        l * (s - s2 / 2.0 - (1.5 * s2 - s3) * inv - phi * s * inv / 2.0),
        (phi * s + 3.0 * s2 - 2.0 * s3) * inv,
        l * (s2 / 2.0 - (1.5 * s2 - s3) * inv - phi * s * inv / 2.0),
    ];
    let n_theta = [
        -6.0 * s_one_minus * inv / l,
        1.0 - s - 3.0 * s_one_minus * inv,
        6.0 * s_one_minus * inv / l,
        s - 3.0 * s_one_minus * inv,
    ];
    (n_v, n_theta)
}

#[cfg(test)]
mod tests {
    #[test]
    fn consistent_mass_rigid_rotation_matches_integrated_translational_and_rotary_energy() {
        let properties = SectionMassProperties {
            mass_per_length: 2.0,
            rotary_inertia_y_per_length: 3.0,
            rotary_inertia_z_per_length: 5.0,
        };
        for (translation, rotation, sign, rotary) in [(1, 5, 1.0, 5.0), (2, 4, -1.0, 3.0)] {
            let mass = consistent_mass_timoshenko(properties, 10.0, 0.3, 0.7);
            let mut velocity = [0.0; 12];
            velocity[translation + 6] = 10.0;
            velocity[rotation] = sign;
            velocity[rotation + 6] = sign;
            let twice_kinetic_energy: f64 = (0..12)
                .map(|i| {
                    (0..12)
                        .map(|j| velocity[i] * mass.get(i, j) * velocity[j])
                        .sum::<f64>()
                })
                .sum();
            let expected = 2.0 * 1000.0 / 3.0 + rotary * 10.0;
            assert!((twice_kinetic_energy - expected).abs() < 1e-10);
        }
    }
    use super::*;

    /// ばね剛性 0（ピン）で解放した自由度は、縮約後に行・列とも 0 になる。
    /// これが「厳密なモーメント解放」の実体で、ペナルティ近似ではない。
    #[test]
    fn ピン解放した自由度の行と列は零になる() {
        let k = sample_k();
        let kstar = condense_end_releases(&k, &[(5, 0.0)]).unwrap();
        for i in 0..12 {
            assert!(kstar.get(5, i).abs() < 1e-6, "行 5 の {i} 列が非零");
            assert!(kstar.get(i, 5).abs() < 1e-6, "列 5 の {i} 行が非零");
        }
    }

    /// 解放が空なら要素剛性をそのまま返す（剛接は厳密に扱う）。
    #[test]
    fn 解放が空なら要素剛性をそのまま返す() {
        let k = sample_k();
        let kstar = condense_end_releases(&k, &[]).unwrap();
        assert_eq!(kstar.n, 12);
        assert_eq!(kstar.data, k.data);
    }

    /// 縮約は「内部自由度を直接解いた結果」と一致する。
    /// 外部自由度に単位変位 e を与え、内部自由度を Kbb·ub = −Kba·ua で解いて
    /// 得た外部反力 Kaa·ua + Kab·ub が、K*·ua と一致することを確かめる。
    #[test]
    fn 縮約は内部自由度を直接解いた結果と一致する() {
        let k = sample_k();
        let releases = [(5usize, 0.0_f64), (11usize, 2.0e7_f64)];
        let kstar = condense_end_releases(&k, &releases).unwrap();

        let (na, nb) = (12usize, releases.len());
        let n = na + nb;
        let mut full = vec![0.0; n * n];
        let mut map: Vec<usize> = (0..12).collect();
        for (idx, &(r, _)) in releases.iter().enumerate() {
            map[r] = na + idx;
        }
        for i in 0..12 {
            for j in 0..12 {
                full[map[i] * n + map[j]] += k.get(i, j);
            }
        }
        for (idx, &(r, ks)) in releases.iter().enumerate() {
            let ir = na + idx;
            full[r * n + r] += ks;
            full[ir * n + ir] += ks;
            full[r * n + ir] -= ks;
            full[ir * n + r] -= ks;
        }

        for col in 0..na {
            let kbb = [
                full[(na) * n + na],
                full[(na) * n + na + 1],
                full[(na + 1) * n + na],
                full[(na + 1) * n + na + 1],
            ];
            let rhs = [-full[(na) * n + col], -full[(na + 1) * n + col]];
            let det = kbb[0] * kbb[3] - kbb[1] * kbb[2];
            assert!(det.abs() > 1e-30, "テスト用 Kbb が特異");
            let ub = [
                (rhs[0] * kbb[3] - kbb[1] * rhs[1]) / det,
                (kbb[0] * rhs[1] - rhs[0] * kbb[2]) / det,
            ];
            for row in 0..na {
                let direct = full[row * n + col]
                    + full[row * n + na] * ub[0]
                    + full[row * n + na + 1] * ub[1];
                let condensed = kstar.get(row, col);
                let tol = 1e-6 * (1.0 + direct.abs());
                assert!(
                    (direct - condensed).abs() < tol,
                    "K*[{row}][{col}] = {condensed}, 直接解 = {direct}"
                );
            }
        }
    }

    #[test]
    fn kbb特異時は剛性と質量の縮約に失敗する() {
        let k = LocalMat::zeros(12);
        let m = LocalMat::zeros(12);
        let releases = [(5, 0.0)];
        assert!(condense_end_releases(&k, &releases).is_none());
        assert!(condense_end_releases_with_mass(
            &k,
            &m,
            SectionMassProperties::default(),
            0.0,
            0.0,
            &releases,
        )
        .is_none());
    }

    #[test]
    fn 非駆動共通ねじりだけを除き実ばねと他の特異性は保持する() {
        let k = sample_k();
        let releases = [(3, 0.0), (4, 0.0), (5, 0.0), (9, 0.0), (10, 0.0), (11, 0.0)];
        let pinned = condense_end_releases(&k, &releases).unwrap();
        for i in 0..12 {
            for j in 0..12 {
                let expected = match (i, j) {
                    (0, 0) | (6, 6) => 2e6,
                    (0, 6) | (6, 0) => -2e6,
                    _ => 0.0,
                };
                assert!((pinned.get(i, j) - expected).abs() < 1e-6);
            }
        }
        assert!(condense_end_releases_with_mass(
            &k,
            &consistent_mass(12.5, 3000.0, 2.0),
            SectionMassProperties::default(),
            0.0,
            0.0,
            &releases,
        )
        .is_none());
        let torsion_only = condense_end_releases(&k, &[(9, 0.0), (3, 0.0)]).unwrap();
        for i in 0..12 {
            for j in 0..12 {
                let expected = if [3, 9].contains(&i) || [3, 9].contains(&j) {
                    0.0
                } else {
                    k.get(i, j)
                };
                assert!((torsion_only.get(i, j) - expected).abs() < 1e-6);
            }
        }
        let mut singular = k.clone();
        for j in 0..12 {
            singular.set(4, j, 0.0);
            singular.set(j, 4, 0.0);
        }
        assert!(condense_end_releases(&singular, &releases).is_none());
        let mut coupled = k.clone();
        coupled.set(3, 0, 1e-30);
        assert!(condense_end_releases(&coupled, &releases).is_none());
        let mut finite_spring = releases;
        finite_spring[0].1 = 1e-2;
        finite_spring[3].1 = 1e-2;
        let actual = condense_end_releases(&k, &finite_spring).unwrap();
        let expected = 1.0 / (2.0 / 1e-2 + 1.0 / 5e9);
        assert!((actual.get(3, 3) / expected - 1.0).abs() < 1e-3);
        let mut invalid = k;
        invalid.set(0, 0, f64::NAN);
        assert!(condense_end_releases(&invalid, &[]).is_none());
        assert!(condense_end_releases(&invalid, &releases).is_none());
        assert!(condense_end_releases_with_mass(
            &invalid,
            &consistent_mass(12.5, 3000.0, 2.0),
            SectionMassProperties::default(),
            0.0,
            0.0,
            &[],
        )
        .is_some());
    }

    /// 軸力ゼロなら幾何剛性はゼロ行列。可撓長が実質ゼロでもゼロ行列。
    #[test]
    fn 幾何剛性は軸力零と可撓長零で零行列() {
        let kg = geometric_stiffness(0.0, 3000.0, 0.0, 0.0);
        assert!(kg.data.iter().all(|v| *v == 0.0), "軸力零で非零");
        let kg = geometric_stiffness(1.0e5, 0.0, 0.0, 0.0);
        assert!(kg.data.iter().all(|v| *v == 0.0), "可撓長零で非零");
    }

    /// 幾何剛性は剛体並進に対して力を生じない（列和が零）。
    /// 剛域なしの場合に、横方向の剛体並進 uy（DOF 1・7）で確認する。
    #[test]
    fn 幾何剛性は剛体並進で力を生じない() {
        let kg = geometric_stiffness(1.0e5, 3000.0, 0.0, 0.0);
        for row in 0..12 {
            let f = kg.get(row, 1) + kg.get(row, 7);
            assert!(f.abs() < 1e-9, "uy 剛体並進で行 {row} に力 {f}");
        }
    }

    /// 集中質量・整合質量とも、並進自由度の総和が部材質量に一致する
    /// （質量の保存。整合質量は Hermite 形状関数の重み 156+54+54+156 = 420 による）。
    #[test]
    fn 質量行列の並進成分の総和は部材質量に等しい() {
        const M: f64 = 12.5;
        let lumped = lumped_mass(M);
        let sum: f64 = [0usize, 6].iter().map(|&d| lumped.get(d, d)).sum();
        assert!((sum - M).abs() < 1e-12, "集中質量の軸方向総和 {sum}");

        let cm = consistent_mass(M, 3000.0, 0.0);
        let mut total = 0.0;
        for i in [1usize, 7] {
            for j in [1usize, 7] {
                total += cm.get(i, j);
            }
        }
        assert!((total - M).abs() < 1e-9, "整合質量の並進総和 {total}");
    }

    /// `torsion_term` を 0 にすると、ねじれ自由度の質量が消える。
    /// ファイバー梁は 0 を渡してねじれ質量を持たない（弾性梁は ρ·J·l/6 を渡す）。
    #[test]
    fn ねじれ項零なら回転自由度の質量は零() {
        let cm = consistent_mass(12.5, 3000.0, 0.0);
        for d in [3usize, 9] {
            assert_eq!(cm.get(d, d), 0.0, "DOF {d} にねじれ質量が残る");
        }
        let cm = consistent_mass(12.5, 3000.0, 2.0);
        assert_eq!(cm.get(3, 3), 4.0);
        assert_eq!(cm.get(3, 9), 2.0);
    }

    #[test]
    fn ティモシェンコ形状関数はphi零で標準hermiteになる() {
        let l = 3000.0;
        for s in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let (v, theta) = timoshenko_shape_functions(s, l, 0.0);
            let s2 = s * s;
            let s3 = s2 * s;
            let expected_v = [
                1.0 - 3.0 * s2 + 2.0 * s3,
                l * (s - 2.0 * s2 + s3),
                3.0 * s2 - 2.0 * s3,
                l * (-s2 + s3),
            ];
            let expected_theta = [
                (-6.0 * s + 6.0 * s2) / l,
                1.0 - 4.0 * s + 3.0 * s2,
                (6.0 * s - 6.0 * s2) / l,
                -2.0 * s + 3.0 * s2,
            ];
            for i in 0..4 {
                assert!((v[i] - expected_v[i]).abs() < 1e-12);
                assert!((theta[i] - expected_theta[i]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn ティモシェンコ質量は回転慣性とphiを曲げ面へ反映する() {
        let properties = SectionMassProperties {
            mass_per_length: 2.0,
            rotary_inertia_y_per_length: 3.0,
            rotary_inertia_z_per_length: 4.0,
        };
        let actual = consistent_mass_timoshenko(properties, 1000.0, 0.8, 0.6);
        let expected = independent_timoshenko_mass(properties, 1000.0, 0.8, 0.6);
        for i in 0..12 {
            for j in 0..12 {
                assert!(
                    (actual.get(i, j) - expected[i][j]).abs()
                        < 2e-10 * (1.0 + expected[i][j].abs()),
                    "M({i},{j}) actual={} expected={}",
                    actual.get(i, j),
                    expected[i][j]
                );
            }
        }
        assert_mass_is_positive_semidefinite(&actual);
    }

    #[test]
    fn ティモシェンコ質量のphi零極限は全成分で標準係数に一致する() {
        let properties = SectionMassProperties {
            mass_per_length: 2.0,
            rotary_inertia_y_per_length: 3.0,
            rotary_inertia_z_per_length: 4.0,
        };
        let actual = consistent_mass_timoshenko(properties, 1000.0, 0.0, 0.0);
        let expected = independent_timoshenko_mass(properties, 1000.0, 0.0, 0.0);
        for i in 0..12 {
            for j in 0..12 {
                assert!(
                    (actual.get(i, j) - expected[i][j]).abs()
                        < 1e-10 * (1.0 + expected[i][j].abs()),
                    "M({i},{j}) actual={} expected={}",
                    actual.get(i, j),
                    expected[i][j]
                );
            }
        }
        assert_mass_is_positive_semidefinite(&actual);
    }

    fn assert_mass_is_positive_semidefinite(mass: &LocalMat) {
        let mut vectors = Vec::with_capacity(36);
        for i in 0..12 {
            let mut vector = [0.0; 12];
            vector[i] = 1.0;
            vectors.push(vector);
        }
        for shift in 0..24 {
            let mut vector = [0.0; 12];
            for i in 0..12 {
                vector[i] = ((i * 7 + shift * 11) % 13) as f64 - 6.0;
            }
            vectors.push(vector);
        }
        for vector in vectors {
            let quadratic = (0..12)
                .map(|i| vector[i] * (0..12).map(|j| mass.get(i, j) * vector[j]).sum::<f64>())
                .sum::<f64>();
            assert!(quadratic >= -1.0e-8, "質量行列の二次形式が負: {quadratic}");
        }
    }

    fn independent_timoshenko_mass(
        properties: SectionMassProperties,
        l: f64,
        phi_xy: f64,
        phi_xz: f64,
    ) -> [[f64; 12]; 12] {
        let gauss = [
            (-0.9602898564975363, 0.1012285362903763),
            (-0.7966664774136267, 0.2223810344533745),
            (-0.525532409916329, 0.3137066458778873),
            (-0.1834346424956498, 0.362683783378362),
            (0.1834346424956498, 0.362683783378362),
            (0.525532409916329, 0.3137066458778873),
            (0.7966664774136267, 0.2223810344533745),
            (0.9602898564975363, 0.1012285362903763),
        ];
        let mut expected = [[0.0; 12]; 12];
        for (xi, gauss_weight) in gauss {
            let s = (xi + 1.0) / 2.0;
            let weight = gauss_weight * l / 2.0;
            let n = [1.0 - s, s];
            for a in 0..2 {
                for b in 0..2 {
                    expected[a * 6][b * 6] += properties.mass_per_length * n[a] * n[b] * weight;
                    expected[3 + a * 6][3 + b * 6] +=
                        properties.polar_inertia_per_length() * n[a] * n[b] * weight;
                }
            }
            for (indices, sign, phi, rotary) in [
                (
                    [1, 5, 7, 11],
                    1.0,
                    phi_xy,
                    properties.rotary_inertia_z_per_length,
                ),
                (
                    [2, 4, 8, 10],
                    -1.0,
                    phi_xz,
                    properties.rotary_inertia_y_per_length,
                ),
            ] {
                let (mut nv, slope) = reference_timoshenko_shapes(s, l, phi);
                for index in [1, 3] {
                    nv[index] *= sign;
                }
                let ntheta = [sign * slope[0], slope[1], sign * slope[2], slope[3]];
                for a in 0..4 {
                    for b in 0..4 {
                        expected[indices[a]][indices[b]] +=
                            (properties.mass_per_length * nv[a] * nv[b]
                                + rotary * ntheta[a] * ntheta[b])
                                * weight;
                    }
                }
            }
        }
        expected
    }

    fn reference_timoshenko_shapes(s: f64, l: f64, phi: f64) -> ([f64; 4], [f64; 4]) {
        let phi = phi.max(0.0);
        let q = 1.0 / (1.0 + phi);
        let v_end = [[1.0, 0.0], [0.0, 0.0], [0.0, 1.0], [0.0, 0.0]];
        let v_slope = [
            [-phi * q, -phi * q],
            [1.0 - phi * q / 2.0, -phi * q / 2.0],
            [phi * q, phi * q],
            [-phi * q / 2.0, 1.0 - phi * q / 2.0],
        ];
        let nv: [f64; 4] =
            std::array::from_fn(|column| cubic_hermite(s, v_end[column], v_slope[column]));
        let theta_mid = [-1.5 * q, 0.5 - 0.75 * q, 1.5 * q, 0.5 - 0.75 * q];
        let theta_end = [[0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0]];
        let ntheta: [f64; 4] = std::array::from_fn(|column| {
            quadratic_lagrange(s, theta_end[column], theta_mid[column])
        });
        let nv = std::array::from_fn(|column| {
            if column % 2 == 0 {
                nv[column]
            } else {
                l * nv[column]
            }
        });
        let ntheta = [ntheta[0] / l, ntheta[1], ntheta[2] / l, ntheta[3]];
        (nv, ntheta)
    }

    fn cubic_hermite(s: f64, endpoints: [f64; 2], slopes: [f64; 2]) -> f64 {
        let a = endpoints[0];
        let b = slopes[0];
        let c = 3.0 * (endpoints[1] - endpoints[0]) - 2.0 * slopes[0] - slopes[1];
        let d = 2.0 * (endpoints[0] - endpoints[1]) + slopes[0] + slopes[1];
        a + b * s + c * s * s + d * s * s * s
    }

    fn quadratic_lagrange(s: f64, endpoints: [f64; 2], midpoint: f64) -> f64 {
        endpoints[0] * 2.0 * (s - 0.5) * (s - 1.0)
            + midpoint * 4.0 * s * (1.0 - s)
            + endpoints[1] * 2.0 * s * (s - 0.5)
    }

    /// 試験用の対称な要素剛性。単純梁の弾性剛性に近い値を素朴に置く
    /// （縮約の性質を確かめるためのもので、実際の断面性能ではない）。
    fn sample_k() -> LocalMat {
        let mut k = LocalMat::zeros(12);
        let (ea_l, ei, l) = (2.0e6_f64, 1.0e12_f64, 3000.0_f64);
        k.set(0, 0, ea_l);
        k.set(6, 6, ea_l);
        k.set(0, 6, -ea_l);
        k.set(6, 0, -ea_l);
        k.set(3, 3, 5.0e9);
        k.set(9, 9, 5.0e9);
        k.set(3, 9, -5.0e9);
        k.set(9, 3, -5.0e9);
        for (d0, r0, d1, r1, sign) in [
            (1usize, 5usize, 7usize, 11usize, 1.0_f64),
            (2, 4, 8, 10, -1.0),
        ] {
            let (c1, c2, c3) = (12.0 * ei / (l * l * l), 6.0 * ei / (l * l), 4.0 * ei / l);
            k.set(d0, d0, c1);
            k.set(d1, d1, c1);
            k.set(d0, d1, -c1);
            k.set(d1, d0, -c1);
            k.set(d0, r0, c2 * sign);
            k.set(r0, d0, c2 * sign);
            k.set(d0, r1, c2 * sign);
            k.set(r1, d0, c2 * sign);
            k.set(d1, r0, -c2 * sign);
            k.set(r0, d1, -c2 * sign);
            k.set(d1, r1, -c2 * sign);
            k.set(r1, d1, -c2 * sign);
            k.set(r0, r0, c3);
            k.set(r1, r1, c3);
            k.set(r0, r1, c3 / 2.0);
            k.set(r1, r0, c3 / 2.0);
        }
        k
    }
}
