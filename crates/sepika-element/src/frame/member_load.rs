//! 部材（梁）スパン荷重の等価節点力と固定端内力を計算する。
//!
//! ローカル 12 自由度の並びは i 端 [N, Vy, Vz, Mx, My, Mz] = index 0..6、j 端 = index 6..12。

use crate::transform::LocalFrame;
use sepika_core::geom::vec3;
use sepika_core::model::{MemberLoad, MemberLoadKind};

#[derive(Clone, Copy, Debug)]
enum Comp {
    Point { a: f64, p: f64 },
    Dist { a: f64, b: f64, w1: f64, w2: f64 },
}

/// 返り値 [Option<Comp>;3]: index 0=軸(x), 1=曲げ面y, 2=曲げ面z。
fn resolve(load: &MemberLoad, frame: &LocalFrame) -> [Option<Comp>; 3] {
    let d = load.dir;
    let dl = vec3::norm(d);
    if dl < 1e-12 {
        return [None, None, None];
    }
    let d = [d[0] / dl, d[1] / dl, d[2] / dl];
    let c = [
        vec3::dot(d, frame.rot[0]),
        vec3::dot(d, frame.rot[1]),
        vec3::dot(d, frame.rot[2]),
    ];
    let mut out = [None, None, None];
    for (axis, out_slot) in out.iter_mut().enumerate() {
        let ck = c[axis];
        if ck.abs() < 1e-15 {
            continue;
        }
        *out_slot = Some(match load.kind {
            MemberLoadKind::Point { a, p } => Comp::Point { a, p: p * ck },
            MemberLoadKind::Distributed { a, b, w1, w2 } => Comp::Dist {
                a,
                b,
                w1: w1 * ck,
                w2: w2 * ck,
            },
        });
    }
    out
}

fn n_vi(xi: f64) -> f64 {
    1.0 - 3.0 * xi * xi + 2.0 * xi * xi * xi
}
fn n_ti(xi: f64, l: f64) -> f64 {
    l * (xi - 2.0 * xi * xi + xi * xi * xi)
}
fn n_vj(xi: f64) -> f64 {
    3.0 * xi * xi - 2.0 * xi * xi * xi
}
fn n_tj(xi: f64, l: f64) -> f64 {
    l * (-xi * xi + xi * xi * xi)
}

fn gauss_dist<F: Fn(f64) -> f64>(a: f64, b: f64, w1: f64, w2: f64, f: F) -> f64 {
    if b == a {
        return 0.0;
    }
    const G: [f64; 3] = [-0.7745966692414834, 0.0, 0.7745966692414834];
    const W: [f64; 3] = [0.5555555555555556, 0.8888888888888888, 0.5555555555555556];
    let mid = 0.5 * (a + b);
    let half = 0.5 * (b - a);
    let mut s = 0.0;
    for k in 0..3 {
        let x = mid + half * G[k];
        let t = 0.5 * (1.0 + G[k]);
        let w = w1 * (1.0 - t) + w2 * t;
        s += W[k] * w * f(x);
    }
    s * half
}

/// 元の全長荷重と、可撓端・剛域節点の局所12自由度荷重 [N, N·mm] を保持する。
#[derive(Clone, Debug)]
pub struct FrameDeadLoadMap {
    loads: Vec<MemberLoad>,
    frame: LocalFrame,
    length_mm: f64,
    li_mm: f64,
    lj_mm: f64,
    flex_load: [f64; 12],
    rigid_node_load: [f64; 12],
}

impl FrameDeadLoadMap {
    /// 解決済み剛域長 `[mm]` と全長座標の荷重を受け取る。フェイス上の集中荷重は可撓側に属する。
    /// 非有限値、不正な長さ・荷重区間・方向、非有限な計算結果はエラーを返す。
    pub fn new(
        loads: &[MemberLoad],
        frame: &LocalFrame,
        length_mm: f64,
        li_mm: f64,
        lj_mm: f64,
    ) -> Result<Self, String> {
        if ![length_mm, li_mm, lj_mm].iter().all(|v| v.is_finite())
            || length_mm <= 0.0
            || li_mm < 0.0
            || lj_mm < 0.0
            || length_mm - li_mm - lj_mm <= 0.0
            || !frame.rot.iter().flatten().all(|v| v.is_finite())
        {
            return Err("部材荷重の全長・解決済み剛域長・局所座標系が不正です".into());
        }
        let mut map = Self {
            loads: loads.to_vec(),
            frame: *frame,
            length_mm,
            li_mm,
            lj_mm,
            flex_load: [0.0; 12],
            rigid_node_load: [0.0; 12],
        };
        let face_j = length_mm - lj_mm;
        let flex_length = face_j - li_mm;
        for load in loads {
            load.validate_direction()
                .map_err(|reason| format!("部材 {:?}: {reason}", load.elem))?;
            let valid = match load.kind {
                MemberLoadKind::Point { a, p } => {
                    a.is_finite() && p.is_finite() && a >= 0.0 && a <= length_mm
                }
                MemberLoadKind::Distributed { a, b, w1, w2 } => {
                    [a, b, w1, w2].iter().all(|v| v.is_finite())
                        && a >= 0.0
                        && b <= length_mm
                        && b > a
                }
            };
            if !valid {
                return Err(format!(
                    "部材 {:?}: 部材荷重の位置・区間・強度が不正です",
                    load.elem
                ));
            }
            for (axis, comp) in resolve(load, frame).into_iter().enumerate() {
                let Some(comp) = comp else { continue };
                match comp {
                    Comp::Point { a, p } => {
                        if a < li_mm {
                            add_rigid_load(&mut map.rigid_node_load, axis, comp, 0, 0.0);
                        } else if a > face_j {
                            add_rigid_load(&mut map.rigid_node_load, axis, comp, 6, length_mm);
                        } else {
                            add_flex_load(
                                &mut map.flex_load,
                                axis,
                                &Comp::Point { a: a - li_mm, p },
                                flex_length,
                            );
                        }
                    }
                    Comp::Dist { a, b, w1, w2 } => {
                        for (lo, hi, rigid_end) in [
                            (0.0, li_mm, Some((0, 0.0))),
                            (li_mm, face_j, None),
                            (face_j, length_mm, Some((6, length_mm))),
                        ] {
                            let start = a.max(lo);
                            let end = b.min(hi);
                            if start >= end {
                                continue;
                            }
                            let intensity = |x| w1 * ((b - x) / (b - a)) + w2 * ((x - a) / (b - a));
                            let (w1, w2) = (intensity(start), intensity(end));
                            if let Some((base, origin)) = rigid_end {
                                add_rigid_load(
                                    &mut map.rigid_node_load,
                                    axis,
                                    Comp::Dist {
                                        a: start,
                                        b: end,
                                        w1,
                                        w2,
                                    },
                                    base,
                                    origin,
                                );
                            } else {
                                add_flex_load(
                                    &mut map.flex_load,
                                    axis,
                                    &Comp::Dist {
                                        a: start - li_mm,
                                        b: end - li_mm,
                                        w1,
                                        w2,
                                    },
                                    flex_length,
                                );
                            }
                        }
                    }
                }
            }
        }
        if !map
            .flex_load
            .iter()
            .chain(&map.rigid_node_load)
            .all(|v| v.is_finite())
        {
            return Err("部材荷重の可撓端・剛域節点荷重が非有限です".into());
        }
        Ok(map)
    }

    /// 可撓端局所12自由度の等価荷重 [N, N·mm]。
    pub fn flex_load(&self) -> &[f64; 12] {
        &self.flex_load
    }

    /// 剛域の実荷重を節点へ移した局所12自由度荷重 [N, N·mm]。
    pub fn rigid_node_load(&self) -> &[f64; 12] {
        &self.rigid_node_load
    }

    /// 分割前の全長座標の荷重。
    pub fn loads(&self) -> &[MemberLoad] {
        &self.loads
    }

    /// 入力荷重の投影に使用した初期局所座標系。
    pub fn frame(&self) -> &LocalFrame {
        &self.frame
    }

    /// 節点間の全長 `[mm]`。
    pub fn length_mm(&self) -> f64 {
        self.length_mm
    }

    /// 解決済み剛域長 (i端, j端) `[mm]`。
    pub fn rigid_lengths_mm(&self) -> (f64, f64) {
        (self.li_mm, self.lj_mm)
    }
}

/// 全長座標の実荷重と元節点端力から断面力 [N, Nmm] を回復する。
#[derive(Clone, Debug)]
pub struct LoadedFrameSection {
    map: FrameDeadLoadMap,
    node_force_local: [f64; 12],
}

impl LoadedFrameSection {
    pub(crate) fn new(map: FrameDeadLoadMap, node_force_local: [f64; 12]) -> Self {
        Self {
            map,
            node_force_local,
        }
    }

    /// 部材荷重の元節点における等価並進力を全体系 `[N]` で返す。
    pub fn applied_nodal_forces_global(&self) -> [[f64; 3]; 2] {
        let (li, lj) = self.map.rigid_lengths_mm();
        let flex = crate::frame::rigid_arm::to_node_force(self.map.flex_load(), li, lj);
        let local = std::array::from_fn(|i| flex[i] + self.map.rigid_node_load()[i]);
        let global = self.map.frame().rotate_to_global(&local);
        std::array::from_fn(|end| std::array::from_fn(|d| global[end * 6 + d]))
    }

    /// 水平切断面の上側へ作用する部材荷重の全体並進合力 `[N]`。切断位置の集中荷重は断面力の左極限に合わせる。
    pub fn applied_force_above_elevation(
        &self,
        z_i: f64,
        z_j: f64,
        z: f64,
    ) -> Result<[f64; 3], String> {
        if ![z_i, z_j, z].iter().all(|value| value.is_finite()) {
            return Err("部材荷重の切断高さが非有限です".into());
        }
        let dz = z_j - z_i;
        let x = if dz == 0.0 {
            0.0
        } else {
            (z - z_i) / dz * self.map.length_mm
        };
        let mut force = [0.0; 3];
        for load in &self.map.loads {
            let magnitude = match load.kind {
                MemberLoadKind::Point { a, p } => {
                    if (dz > 0.0 && a >= x) || (dz < 0.0 && a < x) || (dz == 0.0 && z_i > z) {
                        p
                    } else {
                        0.0
                    }
                }
                MemberLoadKind::Distributed { a, b, w1, w2 } => {
                    let (lo, hi) = if dz > 0.0 {
                        (a.max(x).min(b), b)
                    } else if dz < 0.0 {
                        (a, b.min(x).max(a))
                    } else if z_i > z {
                        (a, b)
                    } else {
                        (a, a)
                    };
                    comp_resultant(&Comp::Dist { a, b, w1, w2 }, lo, hi)
                }
            };
            let norm = sepika_core::geom::vec3::norm(load.dir);
            for (d, value) in force.iter_mut().enumerate() {
                *value += magnitude * load.dir[d] / norm;
            }
        }
        if force.iter().any(|value| !value.is_finite()) {
            return Err("壁側柱の切断片荷重が非有限です".into());
        }
        Ok(force)
    }

    /// i端からの全長比xi∈`[0,1]`における`[N,Qy,Qz,Mx,My,Mz]`。集中荷重位置では左極限を返す。
    pub fn at(&self, xi: f64) -> Result<[f64; 6], String> {
        if !xi.is_finite() || !(0.0..=1.0).contains(&xi) {
            return Err("部材断面位置は有限の全長比0以上1以下が必要です".into());
        }
        let x = xi * self.map.length_mm;
        let mut force = [0.0; 3];
        let mut moment = [0.0; 3];
        for load in &self.map.loads {
            for (axis, comp) in resolve(load, &self.map.frame).into_iter().enumerate() {
                match comp {
                    Some(Comp::Point { a, p }) if a < x => {
                        force[axis] += p;
                        moment[axis] += p * (x - a);
                    }
                    Some(comp @ Comp::Dist { a, .. }) if a < x => {
                        force[axis] += comp_resultant(&comp, 0.0, x);
                        moment[axis] += comp_moment(&comp, 0.0, x, x);
                    }
                    _ => {}
                }
            }
        }
        let g = &self.node_force_local;
        let result = [
            -g[0] - force[0],
            g[1] + force[1],
            g[2] + force[2],
            -g[3],
            -g[4] - g[2] * x - moment[2],
            -g[5] + g[1] * x + moment[1],
        ];
        if result.iter().any(|v| !v.is_finite()) {
            return Err("荷重込み断面力が非有限です".into());
        }
        Ok(result)
    }

    /// 指定された全長比の順で荷重込み断面力を返す。
    pub fn recover(&self, positions: &[f64]) -> Result<crate::frame::beam::MemberForces, String> {
        let at = positions
            .iter()
            .map(|&xi| self.at(xi).map(|force| (xi, force)))
            .collect::<Result<_, _>>()?;
        Ok(crate::frame::beam::MemberForces { at })
    }
}

fn add_flex_load(q: &mut [f64; 12], axis: usize, comp: &Comp, length: f64) {
    if axis == 0 {
        add_axial_consistent(q, comp, length);
    } else {
        add_bending_consistent(q, comp, length, axis);
    }
}

fn add_rigid_load(q: &mut [f64; 12], axis: usize, comp: Comp, base: usize, origin: f64) {
    let (force, moment) = match comp {
        Comp::Point { a, p } => (p, (a - origin) * p),
        Comp::Dist { a, b, .. } => (
            comp_resultant(&comp, a, b),
            -comp_moment(&comp, a, b, origin),
        ),
    };
    q[base + axis] += force;
    match axis {
        1 => q[base + 5] += moment,
        2 => q[base + 4] -= moment,
        _ => {}
    }
}

/// スパン荷重を材端へ配る方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpanLoadTransfer {
    #[default]
    Consistent,
    StaticallyEquivalent,
}

/// 部材の全スパン荷重に対する等価節点力ベクトル（local 12）。
pub fn consistent_load_local(
    loads: &[MemberLoad],
    frame: &LocalFrame,
    length: f64,
    transfer: SpanLoadTransfer,
) -> [f64; 12] {
    let l = length.max(1e-9);
    let mut q = [0.0; 12];
    for load in loads {
        let comps = resolve(load, frame);
        for (axis, comp) in comps.iter().enumerate() {
            let Some(comp) = comp else { continue };
            match (axis, transfer) {
                (0, _) => add_axial_consistent(&mut q, comp, l),
                (1, SpanLoadTransfer::Consistent) => add_bending_consistent(&mut q, comp, l, 1),
                (1, SpanLoadTransfer::StaticallyEquivalent) => {
                    add_bending_static(&mut q, comp, l, 1)
                }
                (_, SpanLoadTransfer::Consistent) => add_bending_consistent(&mut q, comp, l, 2),
                (_, SpanLoadTransfer::StaticallyEquivalent) => {
                    add_bending_static(&mut q, comp, l, 2)
                }
            }
        }
    }
    q
}

/// q[0]=N_i, q[6]=N_j。
fn add_axial_consistent(q: &mut [f64; 12], comp: &Comp, l: f64) {
    match *comp {
        Comp::Point { a, p } => {
            let xi = (a / l).clamp(0.0, 1.0);
            q[0] += p * (1.0 - xi);
            q[6] += p * xi;
        }
        Comp::Dist { a, b, w1, w2 } => {
            q[0] += gauss_dist(a, b, w1, w2, |s| 1.0 - s / l);
            q[6] += gauss_dist(a, b, w1, w2, |s| s / l);
        }
    }
}

/// 曲げ面（plane=1: y面 → Vy,Mz / plane=2: z面 → Vz,My）。
fn add_bending_consistent(q: &mut [f64; 12], comp: &Comp, l: f64, plane: usize) {
    let (iv, im, jv, jm, msign) = if plane == 1 {
        (1usize, 5usize, 7usize, 11usize, 1.0)
    } else {
        (2usize, 4usize, 8usize, 10usize, -1.0)
    };
    match *comp {
        Comp::Point { a, p } => {
            let xi = (a / l).clamp(0.0, 1.0);
            q[iv] += p * n_vi(xi);
            q[im] += msign * p * n_ti(xi, l);
            q[jv] += p * n_vj(xi);
            q[jm] += msign * p * n_tj(xi, l);
        }
        Comp::Dist { a, b, w1, w2 } => {
            q[iv] += gauss_dist(a, b, w1, w2, |s| n_vi(s / l));
            q[im] += msign * gauss_dist(a, b, w1, w2, |s| n_ti(s / l, l));
            q[jv] += gauss_dist(a, b, w1, w2, |s| n_vj(s / l));
            q[jm] += msign * gauss_dist(a, b, w1, w2, |s| n_tj(s / l, l));
        }
    }
}

/// 曲げ面成分を材端モーメント無しで両端へ配る。
fn add_bending_static(q: &mut [f64; 12], comp: &Comp, l: f64, plane: usize) {
    let (iv, jv) = if plane == 1 { (1usize, 7usize) } else { (2, 8) };
    let (r_i, r_j) = match *comp {
        Comp::Point { a, p } => {
            let xi = (a / l).clamp(0.0, 1.0);
            (p * (1.0 - xi), p * xi)
        }
        Comp::Dist { a, b, .. } => {
            if b <= a {
                return;
            }
            (
                comp_moment(comp, a, b, l) / l,
                -comp_moment(comp, a, b, 0.0) / l,
            )
        }
    };
    q[iv] += r_i;
    q[jv] += r_j;
}

fn comp_resultant(comp: &Comp, lo: f64, hi: f64) -> f64 {
    if hi <= lo {
        return 0.0;
    }
    match *comp {
        Comp::Point { a, p } => {
            if a >= lo && a < hi {
                p
            } else {
                0.0
            }
        }
        Comp::Dist { a, b, w1, w2 } => {
            let l = lo.max(a);
            let h = hi.min(b);
            if h <= l {
                return 0.0;
            }
            let wl = w1 * ((b - l) / (b - a)) + w2 * ((l - a) / (b - a));
            let wh = w1 * ((b - h) / (b - a)) + w2 * ((h - a) / (b - a));
            (h - l) * (wl * 0.5 + wh * 0.5)
        }
    }
}

fn comp_moment(comp: &Comp, lo: f64, hi: f64, xref: f64) -> f64 {
    if hi <= lo {
        return 0.0;
    }
    match *comp {
        Comp::Point { a, p } => {
            if a >= lo && a < hi {
                p * (xref - a)
            } else {
                0.0
            }
        }
        Comp::Dist { a, b, w1, w2 } => {
            let l = lo.max(a);
            let h = hi.min(b);
            if h <= l {
                return 0.0;
            }
            let wl = w1 * ((b - l) / (b - a)) + w2 * ((l - a) / (b - a));
            let wh = w1 * ((b - h) / (b - a)) + w2 * ((h - a) / (b - a));
            let width = h - l;
            let resultant = width * (wl * 0.5 + wh * 0.5);
            if (xref - l).abs() <= (xref - h).abs() {
                (xref - l) * resultant - width * (width * (wl / 6.0 + wh / 3.0))
            } else {
                (xref - h) * resultant + width * (width * (wl / 3.0 + wh / 6.0))
            }
        }
    }
}

/// 断面 xi（i 端からの正規化位置 0..1）における固定端内力を
/// local 内力 [N, Qy, Qz, Mx, My, Mz] で返す。
///
/// `transfer` が [`SpanLoadTransfer::StaticallyEquivalent`] の場合、
/// 曲げ・せん断・ねじりの固定端内力は零を返し、軸力成分のみを返す。
pub fn fixed_internal_local(
    loads: &[MemberLoad],
    frame: &LocalFrame,
    length: f64,
    xi: f64,
    transfer: SpanLoadTransfer,
) -> [f64; 6] {
    let l = length.max(1e-9);
    let x = xi * l;
    let xr = (1.0 - xi) * l;
    let q = consistent_load_local(loads, frame, l, transfer);
    let ff = q.map(|v| -v);

    let mut comps_x: Vec<Comp> = Vec::new();
    let mut comps_y: Vec<Comp> = Vec::new();
    let mut comps_z: Vec<Comp> = Vec::new();
    for load in loads {
        let r = resolve(load, frame);
        if let Some(c) = r[0] {
            comps_x.push(c);
        }
        if let Some(c) = r[1] {
            comps_y.push(c);
        }
        if let Some(c) = r[2] {
            comps_z.push(c);
        }
    }
    let res_i = |comps: &[Comp]| comps.iter().map(|c| comp_resultant(c, 0.0, x)).sum::<f64>();
    let mom_i = |comps: &[Comp]| comps.iter().map(|c| comp_moment(c, 0.0, x, x)).sum::<f64>();
    let mom_jx = |comps: &[Comp]| comps.iter().map(|c| comp_moment(c, x, l, x)).sum::<f64>();

    let sy_i = res_i(&comps_y);
    let sz_i = res_i(&comps_z);

    let mut f = [0.0; 6];
    f[0] = -ff[0] - res_i(&comps_x);
    if transfer == SpanLoadTransfer::StaticallyEquivalent {
        return f;
    }
    f[1] = ff[1] + sy_i;
    f[2] = ff[2] + sz_i;

    if xi < 0.5 {
        f[3] = -ff[3];
        f[5] = -ff[5] + ff[1] * x + mom_i(&comps_y);
        f[4] = -ff[4] - ff[2] * x - mom_i(&comps_z);
    } else {
        f[3] = ff[9];
        f[5] = ff[11] + ff[7] * xr - mom_jx(&comps_y);
        f[4] = ff[10] - ff[8] * xr + mom_jx(&comps_z);
    }
    f
}

/// モデルの剛域設定・局所軸・材端解放に従う、節点変位ゼロの梁CMQ応答 [N, N·mm]。
/// 荷重・材端幾何が不正、または線材の荷重応答を取得できない場合はエラー。
pub fn cmq_with_rigid_zone(
    model: &sepika_core::model::Model,
    data: &sepika_core::model::ElementData,
    loads: &[MemberLoad],
) -> Result<crate::behavior::LoadedFrameResponse, String> {
    if data.nodes.len() != 2
        || !matches!(
            data.kind,
            sepika_core::model::ElementKind::Beam
                | sepika_core::model::ElementKind::Fiber
                | sepika_core::model::ElementKind::MultiSpring
        )
    {
        return Err("CMQは2節点梁要素に限ります".into());
    }
    if let Some(reason) = crate::factory::wall_side_column_input_issue(data, model, None) {
        return Err(reason);
    }
    crate::frame::beam::BeamElement::try_new(data, model)?;
    if sepika_core::panel_zone::is_horizontal_or_vertical_member(model, data)
        && sepika_core::panel_zone::member_orientation(model, data).is_some()
    {
        for &node in &data.nodes {
            if model.elements.iter().any(|element| {
                element.kind == sepika_core::model::ElementKind::PanelZone
                    && element.nodes.first() == Some(&node)
            }) {
                sepika_core::panel_zone::resolve_explicit_panel_analysis_joint(
                    model,
                    node,
                    &model.elements,
                )
                .map_err(|reason| reason.to_string())?;
            }
        }
    }
    let behavior = crate::factory::build_behavior(data, model);
    behavior
        .frame_load_response(&vec![0.0; behavior.n_dof()], loads)?
        .ok_or_else(|| "対象要素は梁の荷重応答を持ちません".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::ElemId;
    use sepika_core::model::{MemberLoad, MemberLoadKind};

    fn canonical_frame() -> LocalFrame {
        LocalFrame {
            rot: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        }
    }

    fn map_load(kind: MemberLoadKind, direction: [f64; 3]) -> FrameDeadLoadMap {
        FrameDeadLoadMap::new(
            &[MemberLoad::manual(ElemId(0), direction, kind)],
            &canonical_frame(),
            1000.0,
            200.0,
            100.0,
        )
        .unwrap()
    }

    fn assert_components(actual: &[f64; 12], entries: &[(usize, f64)]) {
        let mut expected = [0.0; 12];
        for &(index, value) in entries {
            expected[index] = value;
        }
        for (i, (&actual, &expected)) in actual.iter().zip(&expected).enumerate() {
            assert!(
                (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
                "component {i}: {actual} != {expected}"
            );
        }
    }

    fn mapped_node_load(map: &FrameDeadLoadMap) -> [f64; 12] {
        let (li, lj) = map.rigid_lengths_mm();
        let mut node = crate::frame::rigid_arm::to_node_force(map.flex_load(), li, lj);
        for (value, rigid) in node.iter_mut().zip(map.rigid_node_load()) {
            *value += rigid;
        }
        node
    }

    #[test]
    fn dead_load_rigid_point_has_no_flex_load_and_preserves_force_arm() {
        let map = map_load(
            MemberLoadKind::Point {
                a: 100.0,
                p: 1000.0,
            },
            [0.0, 1.0, 0.0],
        );
        assert_components(map.flex_load(), &[]);
        assert_components(map.rigid_node_load(), &[(1, 1000.0), (5, 100000.0)]);
        let d = crate::frame::rigid_arm::to_flex_force(map.rigid_node_load(), 200.0, 100.0);
        assert_components(&d, &[(1, 1000.0), (5, -100000.0)]);
        assert_eq!(
            crate::frame::rigid_arm::to_node_force(&d, 200.0, 100.0),
            *map.rigid_node_load()
        );
    }

    #[test]
    fn dead_load_flex_midpoint_uses_flex_length_hermite() {
        let map = map_load(
            MemberLoadKind::Point {
                a: 550.0,
                p: 1000.0,
            },
            [0.0, 1.0, 0.0],
        );
        assert_components(
            map.flex_load(),
            &[(1, 500.0), (5, 87500.0), (7, 500.0), (11, -87500.0)],
        );
        assert_components(map.rigid_node_load(), &[]);
        let node = mapped_node_load(&map);
        assert!((node[1] + node[7] - 1000.0).abs() < 1e-9);
        assert!((node[5] + node[11] + 1000.0 * node[7] - 550000.0).abs() < 1e-8);
    }

    #[test]
    fn dead_load_points_at_nodes_and_faces_belong_to_exactly_one_region() {
        for (a, flex, rigid) in [
            (0.0, vec![], vec![(1, 1000.0)]),
            (200.0, vec![(1, 1000.0)], vec![]),
            (900.0, vec![(7, 1000.0)], vec![]),
            (1000.0, vec![], vec![(7, 1000.0)]),
        ] {
            let map = map_load(MemberLoadKind::Point { a, p: 1000.0 }, [0.0, 1.0, 0.0]);
            assert_components(map.flex_load(), &flex);
            assert_components(map.rigid_node_load(), &rigid);
            let node = mapped_node_load(&map);
            assert!((node[1] + node[7] - 1000.0).abs() < 1e-9);
            assert!((node[5] + node[11] + 1000.0 * node[7] - a * 1000.0).abs() < 1e-8);
        }
        for a in [200.0 - 1e-10, 900.0 + 1e-10] {
            let map = map_load(MemberLoadKind::Point { a, p: 1000.0 }, [0.0, 1.0, 0.0]);
            assert_components(map.flex_load(), &[]);
            assert!((map.rigid_node_load()[1] + map.rigid_node_load()[7] - 1000.0).abs() < 1e-9);
        }
    }

    #[test]
    fn dead_load_full_udl_preserves_rigid_flex_resultants_moments_and_virtual_work() {
        let map = map_load(
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 2.0,
                w2: 2.0,
            },
            [0.0, 1.0, 0.0],
        );
        assert_components(
            map.flex_load(),
            &[
                (1, 700.0),
                (5, 245000.0 / 3.0),
                (7, 700.0),
                (11, -245000.0 / 3.0),
            ],
        );
        assert_components(
            map.rigid_node_load(),
            &[(1, 400.0), (5, 40000.0), (7, 200.0), (11, -10000.0)],
        );
        assert_components(
            &crate::frame::rigid_arm::to_flex_force(map.rigid_node_load(), 200.0, 100.0),
            &[(1, 400.0), (5, -40000.0), (7, 200.0), (11, 10000.0)],
        );
        let node = mapped_node_load(&map);
        assert!((node[1] + node[7] - 2000.0).abs() < 1e-9);
        assert!((node[5] + node[11] + 1000.0 * node[7] - 1000000.0).abs() < 1e-8);
        let mut u_node = [0.0; 12];
        u_node[1] = 3.0 / 7.0;
        u_node[5] = 1.0 / 350.0;
        u_node[7] = 90.0 / 7.0;
        u_node[11] = 1.0 / 35.0;
        let u_flex = crate::frame::rigid_arm::to_flex_disp(&u_node, 200.0, 100.0);
        let work: f64 = map
            .flex_load()
            .iter()
            .zip(u_flex)
            .map(|(q, u)| q * u)
            .sum::<f64>()
            + map
                .rigid_node_load()
                .iter()
                .zip(u_node)
                .map(|(q, u)| q * u)
                .sum::<f64>();
        // 可撓の仮想変位1+2t+3t²+4t³と両剛域の線形変位を実荷重に対して積分。
        assert!((work - 57200.0 / 7.0).abs() < 1e-9);
    }

    #[test]
    fn dead_load_partial_trapezoid_reevaluates_strength_at_cut_face() {
        let map = map_load(
            MemberLoadKind::Distributed {
                a: 100.0,
                b: 500.0,
                w1: 1.0,
                w2: 5.0,
            },
            [0.0, 1.0, 0.0],
        );
        assert_components(map.rigid_node_load(), &[(1, 150.0), (5, 70000.0 / 3.0)]);
        assert_components(
            map.flex_load(),
            &[
                (1, 297645.0 / 343.0),
                (5, 4356000.0 / 49.0),
                (7, 62505.0 / 343.0),
                (11, -1786500.0 / 49.0),
            ],
        );
        let node = mapped_node_load(&map);
        assert!((node[1] + node[7] - 1200.0).abs() < 1e-9);
        assert!((node[5] + node[11] + 1000.0 * node[7] - 1240000.0 / 3.0).abs() < 1e-8);
    }

    #[test]
    fn dead_load_axial_udl_splits_both_rigid_ends_without_moments() {
        for sign in [-1.0, 1.0] {
            let map = map_load(
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 1000.0,
                    w1: 2.0 * sign,
                    w2: 2.0 * sign,
                },
                [1.0, 0.0, 0.0],
            );
            assert_components(map.flex_load(), &[(0, 700.0 * sign), (6, 700.0 * sign)]);
            assert_components(
                map.rigid_node_load(),
                &[(0, 400.0 * sign), (6, 200.0 * sign)],
            );
            let unrestrained =
                FrameDeadLoadMap::new(map.loads(), map.frame(), 1000.0, 0.0, 0.0).unwrap();
            assert_components(
                unrestrained.flex_load(),
                &[(0, 1000.0 * sign), (6, 1000.0 * sign)],
            );
        }
    }

    #[test]
    fn dead_load_rotation_reversal_and_sign_preserve_physical_nodal_load() {
        let kinds = [
            MemberLoadKind::Point {
                a: 100.0,
                p: 1000.0,
            },
            MemberLoadKind::Point {
                a: 550.0,
                p: 1000.0,
            },
            MemberLoadKind::Distributed {
                a: 100.0,
                b: 500.0,
                w1: 1.0,
                w2: 5.0,
            },
        ];
        let rotated = LocalFrame {
            rot: [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
        };
        let reversed = LocalFrame {
            rot: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        };
        for kind in kinds {
            let original = map_load(kind.clone(), [0.0, 1.0, 0.0]);
            let negative = map_load(kind.clone(), [0.0, -1.0, 0.0]);
            let z_plane = map_load(kind.clone(), [0.0, 0.0, 1.0]);
            let rot = FrameDeadLoadMap::new(
                &[MemberLoad::manual(
                    ElemId(0),
                    [-1.0, 0.0, 0.0],
                    kind.clone(),
                )],
                &rotated,
                1000.0,
                200.0,
                100.0,
            )
            .unwrap();
            let reversed_kind = match kind {
                MemberLoadKind::Point { a, p } => MemberLoadKind::Point { a: 1000.0 - a, p },
                MemberLoadKind::Distributed { a, b, w1, w2 } => MemberLoadKind::Distributed {
                    a: 1000.0 - b,
                    b: 1000.0 - a,
                    w1: w2,
                    w2: w1,
                },
            };
            let rev = FrameDeadLoadMap::new(
                &[MemberLoad::manual(
                    ElemId(0),
                    [0.0, 1.0, 0.0],
                    reversed_kind,
                )],
                &reversed,
                1000.0,
                100.0,
                200.0,
            )
            .unwrap();
            for (actual, expected) in [
                (negative.flex_load(), original.flex_load()),
                (negative.rigid_node_load(), original.rigid_node_load()),
            ] {
                for (a, e) in actual.iter().zip(expected) {
                    assert!((a + e).abs() < 1e-8);
                }
            }
            assert_components(
                rot.flex_load(),
                &original
                    .flex_load()
                    .iter()
                    .copied()
                    .enumerate()
                    .collect::<Vec<_>>(),
            );
            assert_components(
                rot.rigid_node_load(),
                &original
                    .rigid_node_load()
                    .iter()
                    .copied()
                    .enumerate()
                    .collect::<Vec<_>>(),
            );
            let node = mapped_node_load(&original);
            let global_rot = rotated.rotate_to_global(&mapped_node_load(&rot));
            let global_rev = reversed.rotate_to_global(&mapped_node_load(&rev));
            let node_z = mapped_node_load(&z_plane);
            for base in [0, 6] {
                assert!((global_rot[base] + node[base + 1]).abs() < 1e-8);
                assert!((global_rot[base + 5] - node[base + 5]).abs() < 1e-8);
                for axis in 0..6 {
                    assert!((global_rev[base + axis] - node[6 - base + axis]).abs() < 1e-8);
                }
                assert!((node_z[base + 2] - node[base + 1]).abs() < 1e-8);
                assert!((node_z[base + 4] + node[base + 5]).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn dead_load_map_retains_original_input_and_rejects_invalid_geometry_or_loads() {
        let frame = canonical_frame();
        let loads = [MemberLoad::manual(
            ElemId(0),
            [0.0, 1.0, 0.0],
            MemberLoadKind::Point {
                a: 200.0,
                p: 1000.0,
            },
        )];
        let map = FrameDeadLoadMap::new(&loads, &frame, 1000.0, 200.0, 100.0).unwrap();
        assert_eq!(map.loads(), &loads);
        assert_eq!(map.frame().rot, frame.rot);
        assert_eq!(map.length_mm(), 1000.0);
        assert_eq!(map.rigid_lengths_mm(), (200.0, 100.0));
        for (length, li, lj) in [
            (0.0, 0.0, 0.0),
            (1000.0, -1.0, 0.0),
            (1000.0, 600.0, 400.0),
            (f64::NAN, 0.0, 0.0),
        ] {
            assert!(FrameDeadLoadMap::new(&loads, &frame, length, li, lj).is_err());
        }
        for kind in [
            MemberLoadKind::Point { a: -1.0, p: 1.0 },
            MemberLoadKind::Point { a: 1001.0, p: 1.0 },
            MemberLoadKind::Point {
                a: 100.0,
                p: f64::INFINITY,
            },
            MemberLoadKind::Distributed {
                a: 100.0,
                b: 100.0,
                w1: 1.0,
                w2: 1.0,
            },
        ] {
            let load = MemberLoad::manual(ElemId(0), [0.0, 1.0, 0.0], kind);
            assert!(FrameDeadLoadMap::new(&[load], &frame, 1000.0, 200.0, 100.0).is_err());
        }
        let invalid_direction = MemberLoad::manual(ElemId(0), [0.0; 3], loads[0].kind.clone());
        assert!(FrameDeadLoadMap::new(&[invalid_direction], &frame, 1000.0, 200.0, 100.0).is_err());
        let empty = FrameDeadLoadMap::new(&[], &frame, 1000.0, 200.0, 100.0).unwrap();
        assert_components(empty.flex_load(), &[]);
        assert_components(empty.rigid_node_load(), &[]);
    }

    #[test]
    fn dead_load_short_distributed_interval_is_not_dropped() {
        let map = FrameDeadLoadMap::new(
            &[MemberLoad::manual(
                ElemId(0),
                [1.0, 0.0, 0.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 1e-13,
                    w1: 1e16,
                    w2: 1e16,
                },
            )],
            &canonical_frame(),
            1000.0,
            200.0,
            100.0,
        )
        .unwrap();
        assert_components(map.rigid_node_load(), &[(0, 1000.0)]);
        let (a, b) = (100.0, 100.0 + 1e-13);
        let map = map_load(
            MemberLoadKind::Distributed {
                a,
                b,
                w1: 1e13,
                w2: 3e13,
            },
            [0.0, 1.0, 0.0],
        );
        let resultant = (b - a) * 2e13;
        let centroid = a + (b - a) * 7.0 / 12.0;
        assert!((map.rigid_node_load()[1] - resultant).abs() < 1e-12);
        assert!((map.rigid_node_load()[5] - resultant * centroid).abs() < 1e-10);
    }

    #[test]
    fn signed_static_transfer_preserves_independent_reaction_fixtures_and_adapter() {
        let frame = horiz_frame();
        for (a, b, w1, w2, ri, rj) in [
            (0.0, 6000.0, 1.0, -1.0, 1000.0, -1000.0),
            (1000.0, 4000.0, 1.0, -1.0, 250.0, -250.0),
            (0.0, 6000.0, 1.0, -1.0 + 1e-10, 1000.0000001, -999.9999998),
            (0.0, 6000.0, -1.0, 1.0, -1000.0, 1000.0),
            (2000.0, 5000.0, -1.0, 1.0, -250.0, 250.0),
            (0.0, 6000.0, 1.0, -0.9, 1100.0, -800.0),
            (-1000.0, 7000.0, 1.0, 1.0, 4000.0, 4000.0),
            (2.0, 1.0, 1.0, 1.0, 0.0, 0.0),
        ] {
            let load = MemberLoad::manual(
                ElemId(0),
                frame.rot[1],
                MemberLoadKind::Distributed { a, b, w1, w2 },
            );
            let q = consistent_load_local(
                &[load],
                &frame,
                6000.0,
                SpanLoadTransfer::StaticallyEquivalent,
            );
            assert!(
                (q[1] - ri).abs() < 1e-8 && (q[7] - rj).abs() < 1e-8,
                "{q:?}"
            );
            assert_eq!([q[5], q[11]], [0.0, 0.0]);
        }
        for (l, a, expected) in [
            (6000.0, -1.0, [1000.0, 0.0]),
            (6000.0, 7000.0, [0.0, 1000.0]),
            (0.0, 0.5e-9, [500.0, 500.0]),
        ] {
            let load = MemberLoad::manual(
                ElemId(0),
                frame.rot[1],
                MemberLoadKind::Point { a, p: 1000.0 },
            );
            let q =
                consistent_load_local(&[load], &frame, l, SpanLoadTransfer::StaticallyEquivalent);
            assert_eq!([q[1], q[7]], expected);
        }
    }

    fn horiz_frame() -> LocalFrame {
        LocalFrame::from_nodes([0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [0.0, 0.0, 1.0]).unwrap()
    }

    fn assert_relative(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 2e-14 * expected.abs(),
            "actual={actual:e}, expected={expected:e}"
        );
    }

    #[test]
    fn short_static_distribution_preserves_independent_resultant_and_first_moment() {
        for (width, scale) in [(1e-13, 1e16), (2_f64.powi(-43), 2_f64.powi(43))] {
            for (wi, wj, resultant, first_moment) in [
                (1.0, 3.0, 2.0, 7.0 / 6.0),
                (3.0, 1.0, 2.0, 5.0 / 6.0),
                (-1.0, -3.0, -2.0, -7.0 / 6.0),
                (1.0, -1.0, 0.0, -1.0 / 6.0),
            ] {
                let loads = [MemberLoad::manual(
                    ElemId(0),
                    [0.0, 1.0, 0.0],
                    MemberLoadKind::Distributed {
                        a: 0.0,
                        b: width,
                        w1: wi * scale,
                        w2: wj * scale,
                    },
                )];
                let q = consistent_load_local(
                    &loads,
                    &canonical_frame(),
                    1.0,
                    SpanLoadTransfer::StaticallyEquivalent,
                );
                assert_relative(q[1] + q[7], resultant * width * scale);
                assert_relative(q[7], first_moment * width * width * scale);
                assert_eq!([q[5], q[11]], [0.0; 2]);
            }
        }
    }

    #[test]
    fn short_cut_distribution_preserves_section_force_and_moment_near_both_ends() {
        let width = 2_f64.powi(-43);
        let scale = 1.0 / width;
        for a in [0.0, 1.0 - width] {
            for sign in [1.0, -1.0] {
                let loads = [MemberLoad::manual(
                    ElemId(0),
                    [0.0, sign, 0.0],
                    MemberLoadKind::Distributed {
                        a,
                        b: a + width,
                        w1: scale,
                        w2: 3.0 * scale,
                    },
                )];
                let section = LoadedFrameSection::new(
                    FrameDeadLoadMap::new(&loads, &canonical_frame(), 1.0, 0.0, 0.0).unwrap(),
                    [0.0; 12],
                );
                let cut = a + width / 2.0;
                let cut_force = section.at(cut).unwrap();
                assert_relative(cut_force[1], sign * 0.75);
                assert_relative(cut_force[5], sign * width / 6.0);
                let left = section
                    .applied_force_above_elevation(1.0, 0.0, 1.0 - cut)
                    .unwrap();
                assert_relative(left[1], sign * 0.75);
                let right = section
                    .applied_force_above_elevation(0.0, 1.0, cut)
                    .unwrap();
                assert_relative(right[1], sign * 1.25);
                let q = consistent_load_local(
                    &loads,
                    &canonical_frame(),
                    1.0,
                    SpanLoadTransfer::Consistent,
                );
                let f = fixed_internal_local(
                    &loads,
                    &canonical_frame(),
                    1.0,
                    cut,
                    SpanLoadTransfer::Consistent,
                );
                assert_relative(f[1] + q[1], sign * 0.75);
                if a == 0.0 {
                    assert_relative(f[5] - q[5] + q[1] * cut, sign * width / 6.0);
                } else {
                    assert_relative(-q[11] - q[7] * (1.0 - cut) - f[5], -sign * width / 3.0);
                }
            }
        }
    }

    #[test]
    fn zero_width_distribution_has_no_nodal_or_section_load() {
        let frame = canonical_frame();
        let loads = [MemberLoad::manual(
            ElemId(0),
            [1.0, 1.0, 1.0],
            MemberLoadKind::Distributed {
                a: 0.5,
                b: 0.5,
                w1: 1e16,
                w2: 3e16,
            },
        )];
        for transfer in [
            SpanLoadTransfer::Consistent,
            SpanLoadTransfer::StaticallyEquivalent,
        ] {
            assert_eq!(
                consistent_load_local(&loads, &frame, 1.0, transfer),
                [0.0; 12]
            );
            for xi in [0.0, 0.5, 1.0] {
                assert_eq!(
                    fixed_internal_local(&loads, &frame, 1.0, xi, transfer),
                    [0.0; 6]
                );
            }
        }
    }

    #[test]
    fn short_rigid_distribution_preserves_nodal_force_and_moment_at_both_ends() {
        let width = 2_f64.powi(-43);
        for (a, base, moment) in [
            (0.0, 0, width * 7.0 / 6.0),
            (1.0 - width, 6, -width * 5.0 / 6.0),
        ] {
            let load = MemberLoad::manual(
                ElemId(0),
                [0.0, 1.0, 0.0],
                MemberLoadKind::Distributed {
                    a,
                    b: a + width,
                    w1: 1.0 / width,
                    w2: 3.0 / width,
                },
            );
            let map = FrameDeadLoadMap::new(&[load], &canonical_frame(), 1.0, 0.25, 0.25).unwrap();
            assert_relative(map.rigid_node_load()[base + 1], 2.0);
            assert_relative(map.rigid_node_load()[base + 5], moment);
            assert_eq!(*map.flex_load(), [0.0; 12]);
        }
    }

    #[test]
    fn short_static_distribution_preserves_small_reaction_near_j_end() {
        let width = 2_f64.powi(-43);
        let load = MemberLoad::manual(
            ElemId(0),
            [0.0, 1.0, 0.0],
            MemberLoadKind::Distributed {
                a: 1.0 - width,
                b: 1.0,
                w1: 1.0 / width,
                w2: 3.0 / width,
            },
        );
        let q = consistent_load_local(
            &[load],
            &canonical_frame(),
            1.0,
            SpanLoadTransfer::StaticallyEquivalent,
        );
        assert_relative(q[1], width * 5.0 / 6.0);
        assert_relative(q[7], 2.0 - width * 5.0 / 6.0);
    }

    #[test]
    fn axial_fixed_internal_satisfies_section_equilibrium() {
        let frame = horiz_frame();
        let l = 1000.0;
        for transfer in [
            SpanLoadTransfer::Consistent,
            SpanLoadTransfer::StaticallyEquivalent,
        ] {
            for (kind, qi, samples) in [
                (
                    MemberLoadKind::Distributed {
                        a: 0.0,
                        b: l,
                        w1: 2.0,
                        w2: 2.0,
                    },
                    1000.0,
                    vec![(0.0, 0.0), (0.25, 500.0), (0.5, 1000.0), (1.0, 2000.0)],
                ),
                (
                    MemberLoadKind::Distributed {
                        a: 0.0,
                        b: l,
                        w1: 0.0,
                        w2: 6.0,
                    },
                    1000.0,
                    vec![(0.0, 0.0), (0.25, 187.5), (0.5, 750.0), (1.0, 3000.0)],
                ),
                (
                    MemberLoadKind::Point {
                        a: 250.0,
                        p: 2000.0,
                    },
                    1500.0,
                    vec![(0.0, 0.0), (0.2, 0.0), (0.3, 2000.0), (1.0, 2000.0)],
                ),
            ] {
                let loads = [MemberLoad::manual(ElemId(0), [1.0, 0.0, 0.0], kind)];
                for (xi, resultant) in samples {
                    // 左側切断片の釣合い: N = 等価節点力Qi - 切断位置までの外力合計。
                    let expected = qi - resultant;
                    let actual = fixed_internal_local(&loads, &frame, l, xi, transfer)[0];
                    assert!(
                        (actual - expected).abs() < 1e-9,
                        "xi={xi}: N={actual}, expected={expected}"
                    );
                }
            }
        }
    }

    fn udl(w: f64, l: f64) -> MemberLoad {
        MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: l,
                w1: w,
                w2: w,
            },
        )
    }

    #[test]
    fn point_mid_fixed_end_moment_is_pl_over_8() {
        let l = 1000.0;
        let p = 100.0;
        let frame = horiz_frame();
        let loads = vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Point { a: l / 2.0, p },
        )];
        let q = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::Consistent);
        let fem = p * l / 8.0;
        assert!((q[5].abs() - fem).abs() < 1e-6, "q5={} fem={}", q[5], fem);
        assert!((q[1].abs() - p / 2.0).abs() < 1e-6, "q1={}", q[1]);
    }

    /// 両端固定梁の UDL に対する固定端内力場が、i/j 分岐（ξ=0.5）をまたいで
    /// 連続かつ符号付き理論解と一致すること。下向き荷重（下端引張正の規約）で
    /// M(ξ) = wL²(6ξ−6ξ²−1)/12（端部 −wL²/12・中央 +wL²/24）、
    /// Q(ξ) = wL(1−2ξ)/2。
    #[test]
    fn udl_clamped_internal_field_continuous_and_signed() {
        let l = 1000.0;
        let w = 2.0;
        let frame = horiz_frame();
        let loads = vec![udl(w, l)];
        for &xi in &[0.0, 0.25, 0.45, 0.5, 0.55, 0.75, 1.0] {
            let f = fixed_internal_local(&loads, &frame, l, xi, SpanLoadTransfer::Consistent);
            let m_expected = w * l * l * (6.0 * xi - 6.0 * xi * xi - 1.0) / 12.0;
            let q_expected = w * l * (1.0 - 2.0 * xi) / 2.0;
            assert!(
                (f[5] - m_expected).abs() < 1e-2,
                "xi={xi} Mz={} expected={m_expected}",
                f[5]
            );
            assert!(
                (f[1] - q_expected).abs() < 1e-6,
                "xi={xi} Qy={} expected={q_expected}",
                f[1]
            );
        }
    }

    /// ブレース（トラス要素）の等分布荷重は、材端モーメントを生じずに
    /// 両端へ半分ずつ配られる。合力は等価節点力方式と一致する
    /// （建物総重量・支点反力が方式によって変わらないことの確認）。
    #[test]
    fn brace_udl_has_no_end_moment_and_preserves_resultant() {
        let l = 1000.0;
        let w = 2.0;
        let frame = horiz_frame();
        let loads = vec![udl(w, l)];
        let q = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::StaticallyEquivalent);
        for &k in &[3usize, 4, 5, 9, 10, 11] {
            assert!(q[k].abs() < 1e-9, "q[{k}]={} は 0 のはず", q[k]);
        }
        let half = w * l / 2.0;
        assert!((q[1].abs() - half).abs() < 1e-6, "q1={}", q[1]);
        assert!((q[7].abs() - half).abs() < 1e-6, "q7={}", q[7]);
        let qc = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::Consistent);
        assert!(
            ((q[1] + q[7]) - (qc[1] + qc[7])).abs() < 1e-6,
            "合力が方式で変わってはいけない"
        );
    }

    /// 材軸上の集中荷重は、作用位置によらず単純梁反力と同じ配分（てこの原理）で
    /// 両端へ分かれる。作用位置の 1 次モーメントが保存される。
    /// 材端ちょうど（a=0, a=L）も落とさずに配る境界ケースを含む。
    #[test]
    fn brace_point_load_splits_by_lever_rule() {
        let l = 1000.0;
        let p = 100.0;
        let frame = horiz_frame();
        for a in [0.0, 250.0, l] {
            let loads = vec![MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Point { a, p },
            )];
            let q =
                consistent_load_local(&loads, &frame, l, SpanLoadTransfer::StaticallyEquivalent);
            assert!(
                (q[1].abs() - p * (1.0 - a / l)).abs() < 1e-6,
                "a={a}: q1={}",
                q[1]
            );
            assert!((q[7].abs() - p * a / l).abs() < 1e-6, "a={a}: q7={}", q[7]);
            assert!(
                q[5].abs() < 1e-9 && q[11].abs() < 1e-9,
                "a={a}: 材端モーメントは 0"
            );
        }
    }

    /// 分布荷重が材長の一部に載る場合も、合力は等価節点力方式と一致する
    /// （静定分配が積分区間を材長で切り取っていないことの確認）。
    #[test]
    fn brace_partial_span_load_preserves_resultant() {
        let l = 1000.0;
        let frame = horiz_frame();
        let loads = vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Distributed {
                a: 200.0,
                b: 700.0,
                w1: 1.0,
                w2: 4.0,
            },
        )];
        let qs = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::StaticallyEquivalent);
        let qc = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::Consistent);
        assert!(
            ((qs[1] + qs[7]) - (qc[1] + qc[7])).abs() < 1e-6,
            "合力が方式で変わってはいけない: static={} consistent={}",
            qs[1] + qs[7],
            qc[1] + qc[7]
        );
        assert!(((qs[1] + qs[7]).abs() - 1250.0).abs() < 1e-6);
    }

    /// ブレースは材軸直交成分を負担しないため、固定端内力はどの断面でも
    /// 曲げ・せん断が 0 になる（荷重ベクトル側で両端へ流し切っている）。
    #[test]
    fn brace_fixed_internal_has_no_bending() {
        let l = 1000.0;
        let w = 2.0;
        let frame = horiz_frame();
        let loads = vec![udl(w, l)];
        for &xi in &[0.0, 0.25, 0.5, 0.75, 1.0] {
            let f = fixed_internal_local(
                &loads,
                &frame,
                l,
                xi,
                SpanLoadTransfer::StaticallyEquivalent,
            );
            for &k in &[1usize, 2, 3, 4, 5] {
                assert!(f[k].abs() < 1e-9, "xi={xi} f[{k}]={} は 0 のはず", f[k]);
            }
        }
    }

    #[test]
    fn triangle_via_trapezoid_matches_known_fem() {
        let l = 1000.0;
        let w = 3.0;
        let frame = horiz_frame();
        let loads = vec![MemberLoad::manual(
            ElemId(0),
            [0.0, 0.0, -1.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: l,
                w1: 0.0,
                w2: w,
            },
        )];
        let q = consistent_load_local(&loads, &frame, l, SpanLoadTransfer::Consistent);
        let fem_i = w * l * l / 30.0;
        let fem_j = w * l * l / 20.0;
        assert!(
            (q[5].abs() - fem_i).abs() < 1e-2,
            "q5={} fem_i={}",
            q[5],
            fem_i
        );
        assert!(
            (q[11].abs() - fem_j).abs() < 1e-2,
            "q11={} fem_j={}",
            q[11],
            fem_j
        );
    }
}
