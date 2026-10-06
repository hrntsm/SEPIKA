//! 断面形状から全塑性計算用のファイバ/バネ配置を生成する。

use sepika_core::error::RebarGeometryError;
use sepika_core::section_shape::{one_bar_area, RebarPoint, SectionShape};

use super::types::{concrete_young, FiberRegion, PlasticFiber, StrengthParams, YieldModelKind};

const NOMINAL_SLAB_WIDTH_MM: f64 = 1000.0;

/// ファイバ材料（限界応力と弾性係数、材料領域区分）。
#[derive(Clone, Copy)]
pub(crate) struct FiberMat {
    pub sigma_t: f64,
    pub sigma_c: f64,
    pub young: f64,
    pub region: FiberRegion,
}

/// 円環領域の分割解像度。
#[derive(Clone, Copy, Debug)]
pub struct AnnulusRes {
    /// 周方向分割数。
    pub n_theta: usize,
    /// 薄肉円環（鋼管壁）の径方向分割数。
    pub n_r_thin: usize,
    /// 中実円（丸鋼・RC 円形・CFT 充填部）の径方向分割数。
    pub n_r_solid: usize,
}

/// 矩形領域を目標寸法 `target` 以下のファイバに等分割して追加する。
pub(crate) fn mesh_rect(
    fibers: &mut Vec<PlasticFiber>,
    center: [f64; 2],
    w: f64,
    h: f64,
    target: f64,
    mat: FiberMat,
) {
    mesh_rect_grid(fibers, center, [w, h], target, mat, false);
}

fn mesh_symmetric_rect(
    fibers: &mut Vec<PlasticFiber>,
    center: [f64; 2],
    w: f64,
    h: f64,
    target: f64,
    mat: FiberMat,
) {
    mesh_rect_grid(fibers, center, [w, h], target, mat, true);
}

fn mesh_rect_grid(
    fibers: &mut Vec<PlasticFiber>,
    center: [f64; 2],
    dimensions: [f64; 2],
    target: f64,
    mat: FiberMat,
    symmetric: bool,
) {
    let [w, h] = dimensions;
    if w == 0.0 || h == 0.0 {
        return;
    }
    let [cy, cz] = center;
    let FiberMat {
        sigma_t,
        sigma_c,
        young,
        region,
    } = mat;
    let mut ny = (w / target).ceil().max(1.0) as usize;
    let mut nz = (h / target).ceil().max(1.0) as usize;
    if symmetric {
        if cy == 0.0 {
            ny += ny % 2;
        }
        if cz == 0.0 {
            nz += nz % 2;
        }
    }
    let dy = w / ny as f64;
    let dz = h / nz as f64;
    for i in 0..ny {
        for j in 0..nz {
            fibers.push(PlasticFiber {
                y: cy - w / 2.0 + (i as f64 + 0.5) * dy,
                z: cz - h / 2.0 + (j as f64 + 0.5) * dz,
                area: dy * dz,
                sigma_t,
                sigma_c,
                young,
                region,
            });
        }
    }
}

/// 円環領域を周方向・径方向に分割して追加する。
fn mesh_annulus(
    fibers: &mut Vec<PlasticFiber>,
    outer_dia: f64,
    thick: f64,
    n_theta: usize,
    n_r: usize,
    mat: FiberMat,
) {
    let FiberMat {
        sigma_t,
        sigma_c,
        young,
        region,
    } = mat;
    let ro = outer_dia / 2.0;
    let ri = (ro - thick).max(0.0);
    let dr = (ro - ri) / n_r as f64;
    for ir in 0..n_r {
        let r_mid = ri + (ir as f64 + 0.5) * dr;
        let r_in = ri + ir as f64 * dr;
        let r_out = r_in + dr;
        let ring_area = std::f64::consts::PI * (r_out * r_out - r_in * r_in);
        let a = ring_area / n_theta as f64;
        for it in 0..n_theta {
            let th = 2.0 * std::f64::consts::PI * (it as f64 + 0.5) / n_theta as f64;
            fibers.push(PlasticFiber {
                y: r_mid * th.cos(),
                z: r_mid * th.sin(),
                area: a,
                sigma_t,
                sigma_c,
                young,
                region,
            });
        }
    }
}

/// H 形を板ごとにメッシュ化して追加する。
fn mesh_h_plates(
    fibers: &mut Vec<PlasticFiber>,
    height: f64,
    width: f64,
    web_thick: f64,
    flange_thick: f64,
    target: f64,
    mat: FiberMat,
) {
    let hw = height - 2.0 * flange_thick;
    mesh_rect(
        fibers,
        [0.0, (height - flange_thick) / 2.0],
        width,
        flange_thick,
        target,
        mat,
    );
    mesh_rect(
        fibers,
        [0.0, -(height - flange_thick) / 2.0],
        width,
        flange_thick,
        target,
        mat,
    );
    mesh_rect(fibers, [0.0, 0.0], web_thick, hw, target, mat);
}

/// 箱形の 4 枚板をメッシュ化して追加する。
fn mesh_box_plates(
    fibers: &mut Vec<PlasticFiber>,
    height: f64,
    width: f64,
    thick: f64,
    target: f64,
    mat: FiberMat,
) {
    let hw = height - 2.0 * thick;
    mesh_rect(
        fibers,
        [0.0, (height - thick) / 2.0],
        width,
        thick,
        target,
        mat,
    );
    mesh_rect(
        fibers,
        [0.0, -(height - thick) / 2.0],
        width,
        thick,
        target,
        mat,
    );
    for ysign in [1.0, -1.0] {
        mesh_rect(
            fibers,
            [ysign * (width - thick) / 2.0, 0.0],
            thick,
            hw,
            target,
            mat,
        );
    }
}

fn mesh_quarter_annulus(
    fibers: &mut Vec<PlasticFiber>,
    center: [f64; 2],
    radii: [f64; 2],
    signs: [f64; 2],
    target: f64,
    mat: FiberMat,
) {
    let [ri, ro] = radii;
    if ro == 0.0 {
        return;
    }
    let nr = ((ro - ri) / target).ceil().max(1.0) as usize;
    let nt = (std::f64::consts::FRAC_PI_2 * ro / target).ceil().max(1.0) as usize;
    let dr = (ro - ri) / nr as f64;
    let dt = std::f64::consts::FRAC_PI_2 / nt as f64;
    for ir in 0..nr {
        let a = ri + ir as f64 * dr;
        let b = ri + (ir + 1) as f64 * dr;
        let area = (b * b - a * a) * dt / 2.0;
        let rho = 4.0 * (dt / 2.0).sin() * (b.powi(3) - a.powi(3)) / (3.0 * dt * (b * b - a * a));
        for it in 0..nt {
            let theta = (it as f64 + 0.5) * dt;
            push_geometry_fiber(
                fibers,
                [
                    center[0] + signs[0] * rho * theta.cos(),
                    center[1] + signs[1] * rho * theta.sin(),
                ],
                area,
                mat,
            );
        }
    }
}

fn push_geometry_fiber(fibers: &mut Vec<PlasticFiber>, center: [f64; 2], area: f64, mat: FiberMat) {
    fibers.push(PlasticFiber {
        y: center[0],
        z: center[1],
        area,
        sigma_t: mat.sigma_t,
        sigma_c: mat.sigma_c,
        young: mat.young,
        region: mat.region,
    });
}

fn mesh_rounded_rect(
    fibers: &mut Vec<PlasticFiber>,
    h: f64,
    b: f64,
    r: f64,
    target: f64,
    mat: FiberMat,
) {
    if r == 0.0 {
        mesh_rect(fibers, [0.0, 0.0], b, h, target, mat);
        return;
    }
    mesh_symmetric_rect(fibers, [0.0, 0.0], b - 2.0 * r, h, target, mat);
    for sy in [-1.0, 1.0] {
        mesh_symmetric_rect(
            fibers,
            [sy * (b - r) / 2.0, 0.0],
            r,
            h - 2.0 * r,
            target,
            mat,
        );
        for sz in [-1.0, 1.0] {
            mesh_quarter_annulus(
                fibers,
                [sy * (b / 2.0 - r), sz * (h / 2.0 - r)],
                [0.0, r],
                [sy, sz],
                target,
                mat,
            );
        }
    }
}

fn mesh_rounded_box(
    fibers: &mut Vec<PlasticFiber>,
    h: f64,
    b: f64,
    t: f64,
    r: f64,
    target: f64,
    mat: FiberMat,
) {
    if r == 0.0 {
        mesh_box_plates(fibers, h, b, t, target, mat);
        return;
    }
    if r >= t {
        for sz in [-1.0, 1.0] {
            mesh_rect(
                fibers,
                [0.0, sz * (h - t) / 2.0],
                b - 2.0 * r,
                t,
                target,
                mat,
            );
        }
        for sy in [-1.0, 1.0] {
            mesh_rect(
                fibers,
                [sy * (b - t) / 2.0, 0.0],
                t,
                h - 2.0 * r,
                target,
                mat,
            );
            for sz in [-1.0, 1.0] {
                mesh_quarter_annulus(
                    fibers,
                    [sy * (b / 2.0 - r), sz * (h / 2.0 - r)],
                    [r - t, r],
                    [sy, sz],
                    target,
                    mat,
                );
            }
        }
    } else {
        for sz in [-1.0, 1.0] {
            mesh_rect(fibers, [0.0, sz * (h - t - r) / 2.0], b, t - r, target, mat);
            mesh_rect(
                fibers,
                [0.0, sz * (h - r) / 2.0],
                b - 2.0 * r,
                r,
                target,
                mat,
            );
        }
        for sy in [-1.0, 1.0] {
            mesh_rect(
                fibers,
                [sy * (b - t) / 2.0, 0.0],
                t,
                h - 2.0 * t,
                target,
                mat,
            );
            for sz in [-1.0, 1.0] {
                mesh_quarter_annulus(
                    fibers,
                    [sy * (b / 2.0 - r), sz * (h / 2.0 - r)],
                    [0.0, r],
                    [sy, sz],
                    target,
                    mat,
                );
            }
        }
    }
}

fn mesh_root_fillets(
    fibers: &mut Vec<PlasticFiber>,
    hw: f64,
    tw: f64,
    r: f64,
    target: f64,
    mat: FiberMat,
) {
    use std::f64::consts::FRAC_PI_4;
    if r == 0.0 {
        return;
    }
    let nr = ((2.0_f64.sqrt() - 1.0) * r / target).ceil().max(1.0) as usize;
    let nt = (FRAC_PI_4 * r / target).ceil().max(1.0) as usize;
    for ir in 0..nr {
        let ua = ir as f64 / nr as f64;
        let ub = (ir + 1) as f64 / nr as f64;
        let pa = 1.0 - ua;
        let pb = 1.0 - ub;
        for it in 0..nt {
            let a = it as f64 * FRAC_PI_4 / nt as f64;
            let b = (it + 1) as f64 * FRAC_PI_4 / nt as f64;
            let sec = |theta: f64| 1.0 / theta.cos();
            let lsec = (sec(b) + b.tan()).ln() - (sec(a) + a.tan()).ln();
            let area = r.powi(2) / 2.0
                * ((pb * pb - pa * pa) * (b - a)
                    + 2.0 * (pb * ub - pa * ua) * lsec
                    + (ub * ub - ua * ua) * (b.tan() - a.tan()));
            let c0 = pb.powi(3) - pa.powi(3);
            let c1 = 3.0 * (pb * pb * ub - pa * pa * ua);
            let c2 = 3.0 * (pb * ub * ub - pa * ua * ua);
            let c3 = ub.powi(3) - ua.powi(3);
            let mx = r.powi(3) / 3.0
                * (c0 * (b.sin() - a.sin()) + c1 * (b - a) + c2 * lsec + c3 * (b.tan() - a.tan()));
            let mz = r.powi(3) / 3.0
                * (c0 * (a.cos() - b.cos())
                    + c1 * (a.cos() / b.cos()).ln()
                    + c2 * (sec(b) - sec(a))
                    + c3 * (sec(b).powi(2) - sec(a).powi(2)) / 2.0);
            for (u, v) in [(mx / area, mz / area), (mz / area, mx / area)] {
                for sy in [-1.0, 1.0] {
                    for sz in [-1.0, 1.0] {
                        push_geometry_fiber(
                            fibers,
                            [sy * (tw / 2.0 + r - u), sz * (hw / 2.0 - r + v)],
                            area,
                            mat,
                        );
                    }
                }
            }
        }
    }
}

/// 実配筋座標 `RebarPoint{x,y}` をファイバ `PlasticFiber{y,z}` へ写して追加する
/// （`fiber.y = point.x`, `fiber.z = point.y`）。
fn rebar_fibers_from_points(
    fibers: &mut Vec<PlasticFiber>,
    points: &[RebarPoint],
    main_dia: f64,
    fy: f64,
    young: f64,
) {
    let a = one_bar_area(main_dia);
    for p in points {
        fibers.push(PlasticFiber {
            y: p.x,
            z: p.y,
            area: a,
            sigma_t: fy,
            sigma_c: -fy,
            young,
            region: FiberRegion::Rebar,
        });
    }
}

/// 断面形状からファイバ/バネ配置を生成する。
/// `kind` により解像度が変わる（細分割と粗い配置）。
/// 実配筋を生成できない場合は [`RebarGeometryError`] を返す。
pub fn plastic_fibers(
    shape: &SectionShape,
    strength: &StrengthParams,
    kind: YieldModelKind,
) -> Result<Vec<PlasticFiber>, RebarGeometryError> {
    let fine = !matches!(kind, YieldModelKind::MultiSpring);
    let target = if fine {
        max_dimension(shape) / 40.0
    } else {
        max_dimension(shape) / 4.0
    };
    let ring = if fine {
        AnnulusRes {
            n_theta: 48,
            n_r_thin: 4,
            n_r_solid: 12,
        }
    } else {
        AnnulusRes {
            n_theta: 8,
            n_r_thin: 1,
            n_r_solid: 2,
        }
    };
    plastic_fibers_at(shape, strength, target, ring)
}

/// 断面外形の最大寸法 [mm]（目標ファイバ寸法の基準）。
pub fn max_dimension(shape: &SectionShape) -> f64 {
    match *shape {
        SectionShape::SteelH { height, width, .. }
        | SectionShape::SteelBox { height, width, .. }
        | SectionShape::SteelChannel { height, width, .. }
        | SectionShape::SteelTee { height, width, .. } => height.max(width),
        SectionShape::SteelAngle { leg_a, leg_b, .. } => leg_a.max(leg_b),
        SectionShape::SteelPipe { outer_dia, .. } => outer_dia,
        SectionShape::SteelFlatBar { width, thick } => width.max(thick),
        SectionShape::SteelRoundBar { dia } => dia,
        SectionShape::SteelLipChannel { height, width, .. } => height.max(width),
        SectionShape::SteelBuiltH {
            height,
            upper_width,
            lower_width,
            ..
        } => height.max(upper_width).max(lower_width),
        SectionShape::RcBeamRect { b, d, .. }
        | SectionShape::RcColumnRect { b, d, .. }
        | SectionShape::SrcBeamRect { b, d, .. }
        | SectionShape::SrcColumnRect { b, d, .. } => b.max(d),
        SectionShape::RcColumnCircle { d, .. } => d,
        SectionShape::CftBox { height, width, .. } => height.max(width),
        SectionShape::CftPipe { outer_dia, .. } => outer_dia,
        SectionShape::RcWall { thickness, .. } | SectionShape::RcSlab { thickness } => {
            thickness.max(1000.0)
        }
    }
}

/// 目標ファイバ寸法 `target` [mm] と円環解像度 `ring` を明示して配置を生成する。
/// [`plastic_fibers`]（MN 曲面・M-φ 用）と要素ファイバ生成
/// （`sepika-element` の `build_gauss_fibers`）が同じ配置規則を共用するための実体。
/// 実配筋を生成できない場合は [`RebarGeometryError`] を返す。
pub fn plastic_fibers_at(
    shape: &SectionShape,
    strength: &StrengthParams,
    target: f64,
    ring: AnnulusRes,
) -> Result<Vec<PlasticFiber>, RebarGeometryError> {
    if !target.is_finite() || target <= 0.0 {
        return Err(RebarGeometryError::InvalidDimension {
            field: "ファイバー目標寸法",
        });
    }
    shape
        .rounded_steel_properties()
        .map_err(RebarGeometryError::SectionGeometry)?;
    let fy = strength.steel_fy;
    let fc = strength.concrete_fc;
    let steel = FiberMat {
        sigma_t: fy,
        sigma_c: -fy,
        young: strength.steel_e,
        region: FiberRegion::Steel,
    };
    let conc = FiberMat {
        sigma_t: 0.0,
        sigma_c: -fc,
        young: concrete_young(fc),
        region: FiberRegion::Concrete,
    };
    let mut fibers = Vec::new();

    match *shape {
        SectionShape::SteelH {
            height,
            width,
            web_thick,
            flange_thick,
            root_r,
        } => {
            mesh_h_plates(
                &mut fibers,
                height,
                width,
                web_thick,
                flange_thick,
                target,
                steel,
            );
            mesh_root_fillets(
                &mut fibers,
                height - 2.0 * flange_thick,
                web_thick,
                root_r.ok_or_else(|| {
                    RebarGeometryError::SectionGeometry("フィレット半径が未知です".into())
                })?,
                target,
                steel,
            );
        }
        SectionShape::SteelBox {
            height,
            width,
            thick,
            corner_r,
        } => {
            mesh_rounded_box(
                &mut fibers,
                height,
                width,
                thick,
                corner_r
                    .ok_or_else(|| RebarGeometryError::SectionGeometry("角Rが未知です".into()))?,
                target,
                steel,
            );
        }
        SectionShape::SteelAngle {
            leg_a,
            leg_b,
            thick,
        } => {
            mesh_rect(
                &mut fibers,
                [thick / 2.0, leg_a / 2.0],
                thick,
                leg_a,
                target,
                steel,
            );
            mesh_rect(
                &mut fibers,
                [thick + (leg_b - thick) / 2.0, thick / 2.0],
                leg_b - thick,
                thick,
                target,
                steel,
            );
        }
        SectionShape::SteelChannel {
            height,
            width,
            web_thick,
            flange_thick,
        } => {
            let hw = height - 2.0 * flange_thick;
            mesh_rect(
                &mut fibers,
                [web_thick / 2.0, 0.0],
                web_thick,
                hw,
                target,
                steel,
            );
            for zsign in [1.0, -1.0] {
                mesh_rect(
                    &mut fibers,
                    [width / 2.0, zsign * (height - flange_thick) / 2.0],
                    width,
                    flange_thick,
                    target,
                    steel,
                );
            }
        }
        SectionShape::SteelTee {
            height,
            width,
            web_thick,
            flange_thick,
        } => {
            let hw = height - flange_thick;
            mesh_rect(
                &mut fibers,
                [0.0, (height - flange_thick) / 2.0],
                width,
                flange_thick,
                target,
                steel,
            );
            mesh_rect(
                &mut fibers,
                [
                    0.0,
                    (height - flange_thick) / 2.0 - flange_thick / 2.0 - hw / 2.0,
                ],
                web_thick,
                hw,
                target,
                steel,
            );
        }
        SectionShape::SteelPipe { outer_dia, thick } => {
            mesh_annulus(
                &mut fibers,
                outer_dia,
                thick,
                ring.n_theta,
                ring.n_r_thin,
                steel,
            );
        }
        SectionShape::SteelFlatBar { width, thick } => {
            mesh_rect(&mut fibers, [0.0, 0.0], width, thick, target, steel);
        }
        SectionShape::SteelRoundBar { dia } => {
            mesh_annulus(
                &mut fibers,
                dia,
                dia / 2.0,
                ring.n_theta,
                ring.n_r_solid,
                steel,
            );
        }
        SectionShape::SteelLipChannel {
            height,
            width,
            lip,
            thick,
        } => {
            let t = thick;
            mesh_rect(
                &mut fibers,
                [t / 2.0, height / 2.0],
                t,
                height,
                target,
                steel,
            );
            for ysign in [1.0, -1.0] {
                mesh_rect(
                    &mut fibers,
                    [(t + width) / 2.0, height / 2.0 + ysign * (height - t) / 2.0],
                    width - t,
                    t,
                    target,
                    steel,
                );
                mesh_rect(
                    &mut fibers,
                    [
                        width - t / 2.0,
                        height / 2.0 + ysign * (height - lip - t) / 2.0,
                    ],
                    t,
                    lip - t,
                    target,
                    steel,
                );
            }
        }
        SectionShape::SteelBuiltH {
            height,
            upper_width,
            upper_thick,
            lower_width,
            lower_thick,
            web_thick,
        } => {
            let hw = (height - upper_thick - lower_thick).max(0.0);
            mesh_rect(
                &mut fibers,
                [0.0, height - upper_thick / 2.0],
                upper_width,
                upper_thick,
                target,
                steel,
            );
            mesh_rect(
                &mut fibers,
                [0.0, lower_thick / 2.0],
                lower_width,
                lower_thick,
                target,
                steel,
            );
            mesh_rect(
                &mut fibers,
                [0.0, lower_thick + hw / 2.0],
                web_thick,
                hw,
                target,
                steel,
            );
        }
        SectionShape::RcBeamRect { b, d, ref rebar } => {
            mesh_rect(&mut fibers, [0.0, 0.0], b, d, target, conc);
            let positions = rebar.bar_positions(b, d)?;
            rebar_fibers_from_points(
                &mut fibers,
                &positions,
                rebar.main_dia,
                strength.rebar_fy,
                strength.steel_e,
            );
        }
        SectionShape::RcColumnRect { b, d, ref rebar } => {
            mesh_rect(&mut fibers, [0.0, 0.0], b, d, target, conc);
            let positions = rebar.bar_positions(b, d)?;
            rebar_fibers_from_points(
                &mut fibers,
                &positions,
                rebar.main_dia,
                strength.rebar_fy,
                strength.steel_e,
            );
        }
        SectionShape::RcColumnCircle { d, ref rebar } => {
            mesh_annulus(&mut fibers, d, d / 2.0, ring.n_theta, ring.n_r_solid, conc);
            let positions = rebar.bar_positions(d)?;
            rebar_fibers_from_points(
                &mut fibers,
                &positions,
                rebar.main_dia,
                strength.rebar_fy,
                strength.steel_e,
            );
        }
        SectionShape::SrcBeamRect {
            b,
            d,
            ref rebar,
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
        } => {
            mesh_rect(&mut fibers, [0.0, 0.0], b, d, target, conc);
            let positions = rebar.bar_positions(b, d)?;
            rebar_fibers_from_points(
                &mut fibers,
                &positions,
                rebar.main_dia,
                strength.rebar_fy,
                strength.steel_e,
            );
            mesh_h_plates(
                &mut fibers,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
                target,
                steel,
            );
        }
        SectionShape::SrcColumnRect {
            b,
            d,
            ref rebar,
            steel_height,
            steel_width,
            steel_web_thick,
            steel_flange_thick,
        } => {
            mesh_rect(&mut fibers, [0.0, 0.0], b, d, target, conc);
            let positions = rebar.bar_positions(b, d)?;
            rebar_fibers_from_points(
                &mut fibers,
                &positions,
                rebar.main_dia,
                strength.rebar_fy,
                strength.steel_e,
            );
            mesh_h_plates(
                &mut fibers,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
                target,
                steel,
            );
        }
        SectionShape::CftBox {
            height,
            width,
            thick,
            corner_r,
        } => {
            let r = corner_r
                .ok_or_else(|| RebarGeometryError::SectionGeometry("角Rが未知です".into()))?;
            mesh_rounded_box(&mut fibers, height, width, thick, r, target, steel);
            mesh_rounded_rect(
                &mut fibers,
                height - 2.0 * thick,
                width - 2.0 * thick,
                (r - thick).max(0.0),
                target,
                conc,
            );
        }
        SectionShape::CftPipe { outer_dia, thick } => {
            mesh_annulus(
                &mut fibers,
                outer_dia,
                thick,
                ring.n_theta,
                ring.n_r_thin,
                steel,
            );
            let di = outer_dia - 2.0 * thick;
            if di > 0.0 {
                mesh_annulus(
                    &mut fibers,
                    di,
                    di / 2.0,
                    ring.n_theta,
                    ring.n_r_solid,
                    conc,
                );
            }
        }
        SectionShape::RcWall { thickness, .. } | SectionShape::RcSlab { thickness } => {
            mesh_rect(
                &mut fibers,
                [0.0, 0.0],
                NOMINAL_SLAB_WIDTH_MM,
                thickness,
                target,
                conc,
            );
        }
    }

    if matches!(
        shape,
        SectionShape::SteelAngle { .. }
            | SectionShape::SteelChannel { .. }
            | SectionShape::SteelTee { .. }
            | SectionShape::SteelLipChannel { .. }
            | SectionShape::SteelBuiltH { .. }
    ) {
        let a_sum: f64 = fibers.iter().map(|f| f.area).sum();
        if a_sum > 0.0 {
            let cy: f64 = fibers.iter().map(|f| f.area * f.y).sum::<f64>() / a_sum;
            let cz: f64 = fibers.iter().map(|f| f.area * f.z).sum::<f64>() / a_sum;
            for f in &mut fibers {
                f.y -= cy;
                f.z -= cz;
            }
        }
    }

    if fibers
        .iter()
        .any(|f| !f.y.is_finite() || !f.z.is_finite() || !f.area.is_finite() || f.area <= 0.0)
    {
        return Err(RebarGeometryError::SectionGeometry(
            "ファイバーの面積・図心を有限値で算定できません。寸法と分割を確認してください".into(),
        ));
    }
    Ok(fibers)
}

#[cfg(test)]
mod rounded_tests {
    use super::*;
    use std::f64::consts::PI;

    fn rounded_width(h: f64, b: f64, r: f64, z: f64) -> f64 {
        if z.abs() >= h / 2.0 {
            0.0
        } else if z.abs() <= h / 2.0 - r {
            b
        } else {
            b - 2.0 * r + 2.0 * (r * r - (z.abs() - h / 2.0 + r).powi(2)).sqrt()
        }
    }

    fn intervals(shape: &SectionShape, z: f64, region: FiberRegion) -> Vec<[f64; 2]> {
        match *shape {
            SectionShape::SteelH {
                height: h,
                width: b,
                web_thick: tw,
                flange_thick: tf,
                root_r: Some(r),
            } => {
                if region != FiberRegion::Steel {
                    return vec![];
                }
                let d = h / 2.0 - tf - z.abs();
                let width = if d <= 0.0 {
                    b
                } else if d >= r {
                    tw
                } else {
                    tw + 2.0 * (r - (r * r - (r - d).powi(2)).sqrt())
                };
                vec![[-width / 2.0, width / 2.0]]
            }
            SectionShape::CftBox {
                height: h,
                width: b,
                thick: t,
                corner_r: Some(r),
            } => {
                let outer = rounded_width(h, b, r, z);
                let inner = rounded_width(h - 2.0 * t, b - 2.0 * t, (r - t).max(0.0), z);
                if region == FiberRegion::Concrete {
                    if inner == 0.0 {
                        vec![]
                    } else {
                        vec![[-inner / 2.0, inner / 2.0]]
                    }
                } else if inner == 0.0 {
                    vec![[-outer / 2.0, outer / 2.0]]
                } else {
                    vec![[-outer / 2.0, -inner / 2.0], [inner / 2.0, outer / 2.0]]
                }
            }
            _ => unreachable!(),
        }
    }

    fn independent(shape: &SectionShape, region: FiberRegion, theta: f64, offset: f64) -> [f64; 6] {
        let n = 262_144;
        let dt = PI / n as f64;
        let h = match *shape {
            SectionShape::SteelH { height, .. } | SectionShape::CftBox { height, .. } => height,
            _ => unreachable!(),
        };
        let mut result = [0.0; 6];
        for i in 0..n {
            let th = -PI / 2.0 + (i as f64 + 0.5) * dt;
            let z = h / 2.0 * th.sin();
            let dz = h / 2.0 * th.cos() * dt;
            for [lo, hi] in intervals(shape, z, region) {
                let width = hi - lo;
                result[0] += width * dz;
                result[1] += width * z * z * dz;
                result[2] += (hi.powi(3) - lo.powi(3)) / 3.0 * dz;
                let mut breaks = vec![lo, hi];
                if theta.cos().abs() > 1.0e-12 {
                    let neutral = (offset - theta.sin() * z) / theta.cos();
                    if neutral > lo && neutral < hi {
                        breaks.insert(1, neutral);
                    }
                }
                for pair in breaks.windows(2) {
                    let tension =
                        theta.cos() * (pair[0] + pair[1]) / 2.0 + theta.sin() * z >= offset;
                    let sigma = match (region, tension) {
                        (FiberRegion::Steel, true) => 235.0,
                        (FiberRegion::Steel, false) => -235.0,
                        (FiberRegion::Concrete, true) => 0.0,
                        (FiberRegion::Concrete, false) => -24.0,
                        _ => unreachable!(),
                    };
                    let force = sigma * (pair[1] - pair[0]) * dz;
                    result[3] += force;
                    result[4] += force * z;
                    result[5] -= sigma * (pair[1].powi(2) - pair[0].powi(2)) / 2.0 * dz;
                }
            }
        }
        result
    }

    #[test]
    fn exact_material_area_and_refined_geometry_and_capacity() {
        let mut shapes = vec![];
        for (h, b) in [(400.0_f64, 400.0_f64), (500.0, 300.0)] {
            for r in [0.0, 5.0, 10.0, 30.0, b / 2.0] {
                shapes.push(SectionShape::CftBox {
                    height: h,
                    width: b,
                    thick: 10.0,
                    corner_r: Some(r),
                });
            }
        }
        for r in [0.0, 13.0, 95.5] {
            shapes.push(SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 9.0,
                flange_thick: 12.0,
                root_r: Some(r),
            });
        }
        let strengths = StrengthParams::default();
        let ring = AnnulusRes {
            n_theta: 48,
            n_r_thin: 4,
            n_r_solid: 12,
        };
        for shape in shapes {
            let dim = max_dimension(&shape);
            let steel = shape.rounded_steel_properties().unwrap().unwrap();
            let core = shape.rounded_core_properties().unwrap();
            let loadings = [
                (0.0, 0.0),
                (PI / 2.0, 0.0),
                (PI / 4.0, 0.2 * dim),
                (-PI / 3.0, -0.3 * dim),
                (0.0, dim),
                (0.0, -dim),
            ];
            let references: Vec<_> = loadings
                .iter()
                .map(|&(theta, offset)| {
                    let s = independent(&shape, FiberRegion::Steel, theta, offset);
                    let c = if core.is_some() {
                        independent(&shape, FiberRegion::Concrete, theta, offset)
                    } else {
                        [0.0; 6]
                    };
                    [s[3] + c[3], s[4] + c[4], s[5] + c[5]]
                })
                .collect();
            let norm_n = steel.area * 235.0 + core.map_or(0.0, |p| p.area * 24.0);
            for divisions in [4, 40, 160, 640, 2560] {
                let fibers =
                    plastic_fibers_at(&shape, &strengths, dim / divisions as f64, ring).unwrap();
                let mut max_i_error = 0.0_f64;
                for (region, p) in [
                    (FiberRegion::Steel, Some(steel)),
                    (FiberRegion::Concrete, core),
                ] {
                    let Some(p) = p else {
                        continue;
                    };
                    let moments = fibers.iter().filter(|f| f.region == region).fold(
                        [0.0; 3],
                        |mut sum, f| {
                            sum[0] += f.area;
                            sum[1] += f.area * f.z * f.z;
                            sum[2] += f.area * f.y * f.y;
                            sum
                        },
                    );
                    assert!((moments[0] / p.area - 1.0).abs() <= 1.0e-10);
                    for (actual, expected) in [(moments[1], p.iy), (moments[2], p.iz)] {
                        let error = actual / expected - 1.0;
                        max_i_error = max_i_error.max(error.abs());
                        if divisions == 2560 {
                            assert!(error.abs() <= 0.001, "{shape:?}: I {error}");
                        }
                    }
                }
                let mut min_error = 0.0_f64;
                let mut max_error = 0.0_f64;
                for (&(theta, offset), expected) in loadings.iter().zip(&references) {
                    let actual = super::super::plastic::plastic_point(
                        &fibers,
                        -offset,
                        theta.sin(),
                        -theta.cos(),
                    );
                    for k in 0..3 {
                        let representative = if k == 0 { norm_n } else { norm_n * dim / 4.0 };
                        let denominator = expected[k].abs().max(representative * 1.0e-6);
                        let error = if expected[k].abs() > representative * 1.0e-6 {
                            (actual[k].abs() - expected[k].abs()) / denominator
                        } else {
                            (actual[k] - expected[k]) / representative
                        };
                        min_error = min_error.min(error);
                        max_error = max_error.max(error);
                        if divisions == 2560 {
                            assert!(
                                error.abs() <= 0.001,
                                "{shape:?}, {theta}, {offset}: capacity {k} {error}"
                            );
                        }
                    }
                }
                eprintln!("{shape:?}, divisions={divisions}, fibers={}, max_I={max_i_error:.9e}, capacity_signed=[{min_error:.9e},{max_error:.9e}]", fibers.len());
            }
        }
    }

    #[test]
    fn unknown_or_invalid_geometry_is_explicit_error() {
        let ring = AnnulusRes {
            n_theta: 48,
            n_r_thin: 4,
            n_r_solid: 12,
        };
        for r in [
            None,
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(201.0),
        ] {
            let shape = SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 10.0,
                corner_r: r,
            };
            assert!(plastic_fibers_at(&shape, &StrengthParams::default(), 10.0, ring).is_err());
        }
    }

    #[test]
    fn unrepresentable_fillet_cells_are_errors_not_nan_fibers() {
        let ring = AnnulusRes {
            n_theta: 48,
            n_r_thin: 4,
            n_r_solid: 12,
        };
        let tiny = SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 9.0,
            flange_thick: 12.0,
            root_r: Some(1.0e-200),
        };
        assert!(plastic_fibers_at(&tiny, &StrengthParams::default(), 10.0, ring).is_err());
    }

    #[test]
    fn published_moment_at_axial_force_and_zero_radius_baseline() {
        use super::super::plastic::{plastic_moment_at_n, plastic_point};
        let steel = FiberMat {
            sigma_t: 235.0,
            sigma_c: -235.0,
            young: 205000.0,
            region: FiberRegion::Steel,
        };
        let conc = FiberMat {
            sigma_t: 0.0,
            sigma_c: -24.0,
            young: concrete_young(24.0),
            region: FiberRegion::Concrete,
        };
        let ring = AnnulusRes {
            n_theta: 48,
            n_r_thin: 4,
            n_r_solid: 12,
        };
        for (shape, is_zero) in [
            (
                SectionShape::SteelH {
                    height: 400.0,
                    width: 200.0,
                    web_thick: 9.0,
                    flange_thick: 12.0,
                    root_r: Some(0.0),
                },
                true,
            ),
            (
                SectionShape::SteelH {
                    height: 400.0,
                    width: 200.0,
                    web_thick: 9.0,
                    flange_thick: 12.0,
                    root_r: Some(13.0),
                },
                false,
            ),
            (
                SectionShape::CftBox {
                    height: 400.0,
                    width: 400.0,
                    thick: 10.0,
                    corner_r: Some(0.0),
                },
                true,
            ),
            (
                SectionShape::CftBox {
                    height: 400.0,
                    width: 400.0,
                    thick: 10.0,
                    corner_r: Some(30.0),
                },
                false,
            ),
            (
                SectionShape::CftBox {
                    height: 500.0,
                    width: 300.0,
                    thick: 10.0,
                    corner_r: Some(0.0),
                },
                true,
            ),
            (
                SectionShape::CftBox {
                    height: 500.0,
                    width: 300.0,
                    thick: 10.0,
                    corner_r: Some(30.0),
                },
                false,
            ),
        ] {
            let dim = max_dimension(&shape);
            let loads = [
                (0.0, 0.0),
                (std::f64::consts::FRAC_PI_2, 0.2 * dim),
                (std::f64::consts::FRAC_PI_4, -0.3 * dim),
            ];
            let reference: Vec<_> = loads
                .iter()
                .map(|&(theta, offset)| {
                    let s = independent(&shape, FiberRegion::Steel, theta, offset);
                    let c = if matches!(shape, SectionShape::CftBox { .. }) {
                        independent(&shape, FiberRegion::Concrete, theta, offset)
                    } else {
                        [0.0; 6]
                    };
                    [s[3] + c[3], s[4] + c[4], s[5] + c[5]]
                })
                .collect();
            for divisions in [40, 160, 640, 2560] {
                let target = dim / divisions as f64;
                let fibers =
                    plastic_fibers_at(&shape, &StrengthParams::default(), target, ring).unwrap();
                let mut old = Vec::new();
                if is_zero {
                    match shape {
                        SectionShape::SteelH {
                            height,
                            width,
                            web_thick,
                            flange_thick,
                            ..
                        } => mesh_h_plates(
                            &mut old,
                            height,
                            width,
                            web_thick,
                            flange_thick,
                            target,
                            steel,
                        ),
                        SectionShape::CftBox {
                            height,
                            width,
                            thick,
                            ..
                        } => {
                            mesh_box_plates(&mut old, height, width, thick, target, steel);
                            mesh_rect(
                                &mut old,
                                [0.0, 0.0],
                                width - 2.0 * thick,
                                height - 2.0 * thick,
                                target,
                                conc,
                            );
                        }
                        _ => unreachable!(),
                    }
                }
                let mut worst = 0.0_f64;
                let mut old_worst = 0.0_f64;
                let mut support_diff = 0.0_f64;
                for (&(theta, offset), expected) in loads.iter().zip(&reference) {
                    let evaluate = |fs: &[PlasticFiber]| {
                        let m = plastic_moment_at_n(fs, theta.sin(), -theta.cos(), expected[0])
                            .unwrap();
                        let projection = theta.sin() * m[0] - theta.cos() * m[1];
                        let exact = theta.sin() * expected[1] - theta.cos() * expected[2];
                        (projection / exact - 1.0).abs()
                    };
                    worst = worst.max(evaluate(&fibers));
                    if is_zero {
                        old_worst = old_worst.max(evaluate(&old));
                        let current = plastic_point(&fibers, -offset, theta.sin(), -theta.cos());
                        let previous = plastic_point(&old, -offset, theta.sin(), -theta.cos());
                        let norm = shape.calc_area() * 235.0;
                        support_diff = support_diff.max((current[0] - previous[0]).abs() / norm);
                    }
                }
                eprintln!("公開 plastic_moment_at_n: {shape:?}, divisions={divisions}, error={worst:.9e}, zero_radius_previous={old_worst:.9e}, support_N_delta={support_diff:.9e}");
                if divisions == 2560 {
                    assert!(worst <= 0.001, "公開曲げ耐力: {shape:?}: {worst}");
                }
                if is_zero && matches!(shape, SectionShape::SteelH { .. }) {
                    assert_eq!(worst, old_worst);
                    assert_eq!(support_diff, 0.0);
                }
            }
        }
    }
}
