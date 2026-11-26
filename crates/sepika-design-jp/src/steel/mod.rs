//! 鋼構造の許容応力度と断面検定。
//! 形状は `Section.shape` 優先、なければ `Section.name` から推定し、合わなければ `Other` とする。

use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx, MemberForcesAt,
    MemberKind,
};
use sepika_core::model::{Material, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;

pub use crate::material_strength::{
    big_lambda, plate_thickness, steel_f_value, steel_f_value_prefix, steel_fc, steel_fs, steel_ft,
};

mod beam;
mod brace;
mod column;
pub(crate) use column::check_symmetric_h_plate_slenderness;
pub(crate) use column::column_allowables;
pub mod lateral_bracing;
mod lip_channel;
pub mod panel_zone;
mod section;
#[cfg(test)]
mod shear_contract_tests;
#[cfg(test)]
mod torsion_contract_tests;

pub(crate) use section::column_span_parameters;
pub use section::{resolve_lb, steel_fb_h_asd2019, steel_h_z_with_loss};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeCategory {
    H,
    Box,
    Pipe,
    Other,
}

/// `Section.name` の先頭アルファベットトークンから形状カテゴリを推定する。
/// 例: "H-300x300x10x15"→H、"BOX-200x200x12"→Box、"PIPE-216.3x8.2"→Pipe。
/// 該当しない場合は `Other`（一般断面フォールバック）。
fn classify_shape(name: &str) -> ShapeCategory {
    let token: String = name
        .trim()
        .to_uppercase()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    match token.as_str() {
        "H" => ShapeCategory::H,
        "BOX" => ShapeCategory::Box,
        "PIPE" | "P" => ShapeCategory::Pipe,
        _ => ShapeCategory::Other,
    }
}

/// 形状カテゴリと板厚 `(カテゴリ, tf, tw)` を解決する。
/// `Section.shape` があれば実寸、なければ `Section.thickness` を `tf ≈ tw` とする。
fn shape_of(sec: &Section) -> (ShapeCategory, f64, f64) {
    if let Some(shape) = &sec.shape {
        match *shape {
            SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            } => return (ShapeCategory::H, flange_thick, web_thick),
            SectionShape::SteelBuiltH {
                web_thick,
                upper_thick,
                lower_thick,
                ..
            } => return (ShapeCategory::H, upper_thick.min(lower_thick), web_thick),
            SectionShape::SteelBox { thick, .. } => return (ShapeCategory::Box, thick, thick),
            SectionShape::SteelPipe { thick, .. } => return (ShapeCategory::Pipe, thick, thick),
            SectionShape::SteelChannel {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelTee {
                web_thick,
                flange_thick,
                ..
            } => return (ShapeCategory::Other, flange_thick, web_thick),
            SectionShape::SteelAngle { thick, .. } => return (ShapeCategory::Other, thick, thick),
            SectionShape::SteelFlatBar { thick, .. } => {
                return (ShapeCategory::Other, thick, thick)
            }
            SectionShape::SteelRoundBar { dia } => return (ShapeCategory::Other, dia, dia),
            SectionShape::SteelLipChannel { thick, .. } => {
                return (ShapeCategory::Other, thick, thick)
            }
            SectionShape::CftBox { thick, .. } => return (ShapeCategory::Box, thick, thick),
            SectionShape::CftPipe { thick, .. } => return (ShapeCategory::Pipe, thick, thick),
            SectionShape::RcBeamRect { .. }
            | SectionShape::RcColumnRect { .. }
            | SectionShape::RcColumnCircle { .. }
            | SectionShape::SrcBeamRect { .. }
            | SectionShape::SrcColumnRect { .. }
            | SectionShape::RcWall { .. }
            | SectionShape::RcSlab { .. } => return (ShapeCategory::Other, 0.0, 0.0),
        }
    }
    let t = sec.thickness.unwrap_or(0.0);
    (classify_shape(&sec.name), t, t)
}

/// 断面検定上、上下対称のH形鋼として扱う断面か。
pub fn is_symmetric_h_section(sec: &Section) -> bool {
    shape_of(sec).0 == ShapeCategory::H
        && !sec
            .shape
            .as_ref()
            .is_some_and(SectionShape::is_asymmetric_built_h)
}

/// 形状を持つ対称H形鋼の（全せい、ウェブ内法せい、ウェブ厚）[mm]。
pub(crate) fn symmetric_h_web(sec: &Section) -> Option<(f64, f64, f64)> {
    match sec.shape {
        Some(SectionShape::SteelH {
            height,
            flange_thick,
            web_thick,
            ..
        })
        | Some(SectionShape::SteelBuiltH {
            height,
            upper_thick: flange_thick,
            web_thick,
            ..
        }) if is_symmetric_h_section(sec) => Some((height, height - 2.0 * flange_thick, web_thick)),
        _ => None,
    }
}

fn strong_axis_h_tension(forces: &MemberForcesAt, sec: &Section) -> bool {
    symmetric_h_web(sec).is_some() && forces.n > 0.0 && forces.my == 0.0
}

struct StrongAxisTensionBendingRatios {
    tension: f64,
    compression: f64,
}

impl StrongAxisTensionBendingRatios {
    fn governing(&self) -> f64 {
        self.tension.max(self.compression.max(0.0))
    }
}

fn strong_axis_tension_bending_ratios(
    sigma_t_mpa: f64,
    sigma_b_mpa: f64,
    ft_mpa: f64,
    fb_mpa: f64,
) -> StrongAxisTensionBendingRatios {
    StrongAxisTensionBendingRatios {
        tension: (sigma_t_mpa + sigma_b_mpa) / safe_denom(ft_mpa),
        compression: (sigma_b_mpa - sigma_t_mpa) / safe_denom(fb_mpa),
    }
}

pub(crate) const INVALID_NODE_SPAN_FOR_LATERAL_BUCKLING: &str =
    "横座屈長さの既定値には有限な正の節点間距離が必要です";

/// 一般H形鋼柱の横座屈長さ [mm]。支点確認が未入力の場合は節点間距離を用いる。
pub fn column_lateral_buckling_length(
    model: &sepika_core::model::Model,
    elem: &sepika_core::model::ElementData,
    attr: Option<&sepika_core::model::SteelDesignAttr>,
) -> Result<f64, &'static str> {
    if attr
        .and_then(|a| a.lateral_brace_count)
        .is_some_and(|count| count > 0)
    {
        return Err(
            "中間横補剛が入力されています。補剛位置と拘束を確認した区間の横座屈長さは未算定です",
        );
    }
    let Some(confirmed) = attr.and_then(|a| a.column_lateral_buckling_end_supports.as_ref()) else {
        let span_mm = frame_node_span_mm(model, elem);
        return (span_mm.is_finite() && span_mm > 0.0)
            .then_some(span_mm)
            .ok_or(INVALID_NODE_SPAN_FOR_LATERAL_BUCKLING);
    };
    let current = model
        .column_end_supports(elem)
        .ok_or("実材軸長を有限な正値で取得できないため、横座屈長さは未算定です")?;
    if *confirmed != current {
        return Err("実材端の条件が確認時から変わりました。両実材端の拘束と中間横補剛の有無を再確認してください");
    }
    Ok(current.length_mm())
}

fn shear_component(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
    fs_mpa: f64,
) -> Option<CheckComponent> {
    let (tau_mpa, basis) = match symmetric_h_web(sec) {
        Some((height, web_depth, web_thick))
            if (forces.qy != 0.0 || forces.qz == 0.0) && !has_section_loss(ctx) =>
        {
            let web_depth_mm = if ctx.kind == MemberKind::Girder {
                height
            } else {
                web_depth
            };
            let web_area_mm2 = web_depth_mm * web_thick;
            if !web_area_mm2.is_finite() || web_area_mm2 <= 0.0 || web_depth_mm <= 0.0 {
                return None;
            }
            (forces.qy.abs() / web_area_mm2, "H形鋼のウェブ面内せん断")
        }
        _ if matches!(sec.shape, Some(SectionShape::SteelPipe { .. }))
            && sec.area.is_finite()
            && sec.area > 0.0 =>
        {
            (
                forces.qy.hypot(forces.qz) / (sec.area / 2.0),
                "円形鋼管の最大せん断",
            )
        }
        _ => return None,
    };
    Some(CheckComponent {
        kind: CheckKind::Shear,
        ratio: tau_mpa / fs_mpa,
        detail: format!(
            "{basis}: τ={tau_mpa:.4} N/mm², fs={fs_mpa:.4} N/mm², せん断比={:.4}",
            tau_mpa / fs_mpa
        ),
    })
}

fn independent_h_web_shear_failure(
    forces: &MemberForcesAt,
    sec: &Section,
    mat: &Material,
    ctx: &DesignCtx,
) -> Option<CheckComponent> {
    if !matches!(ctx.kind, MemberKind::Girder | MemberKind::Column)
        || symmetric_h_web(sec).is_none()
        || forces.mx != 0.0
        || ![forces.n, forces.qy, forces.qz]
            .iter()
            .all(|v| v.is_finite())
    {
        return None;
    }
    let f = crate::material_strength::section_steel_f(sec, Some(mat)).ok()?;
    shear_component(forces, sec, ctx, steel_fs(f, ctx.term))
        .filter(|component| component.ratio.is_finite() && component.ratio > 1.0)
}

pub(crate) fn buckling_lengths_with_defaults(
    kind: MemberKind,
    node_span_mm: f64,
    strong_mm: Option<f64>,
    weak_mm: Option<f64>,
) -> (f64, f64) {
    if kind == MemberKind::Column {
        sepika_core::model::column_buckling_lengths_mm(node_span_mm, strong_mm, weak_mm)
    } else {
        (
            strong_mm.unwrap_or(node_span_mm),
            weak_mm.unwrap_or(node_span_mm),
        )
    }
}

/// 通常S部材・SRC柱の座屈長さ（強軸、弱軸）[mm]。欠落した軸に節点間距離から得る既定値を用いる。
pub fn frame_buckling_lengths(
    model: &sepika_core::model::Model,
    elem: &sepika_core::model::ElementData,
    kind: MemberKind,
    attr: Option<&sepika_core::model::SteelDesignAttr>,
) -> (f64, f64) {
    let (strong_mm, weak_mm) = attr
        .map(|a| (a.lk_y_direct, a.lk_z_direct))
        .unwrap_or_default();
    buckling_lengths_with_defaults(kind, frame_node_span_mm(model, elem), strong_mm, weak_mm)
}

/// 部材両端の節点座標間の距離 [mm]。両端の参照を解決できない場合は0。
pub fn frame_node_span_mm(
    model: &sepika_core::model::Model,
    elem: &sepika_core::model::ElementData,
) -> f64 {
    match elem.nodes.as_slice() {
        [i, j] => model.node(*i).zip(model.node(*j)).map_or(0.0, |(a, b)| {
            sepika_core::geom::vec3::dist(a.coord, b.coord)
        }),
        _ => 0.0,
    }
}

const WEAK_H_SHEAR_UNAVAILABLE: &str = "H形鋼に作用する弱軸せん断力Qzの照査条件を確定できません";
const NONFINITE_SHEAR_FORCE: &str = "せん断力が非有限です";

fn check_same_point_shear(
    result: &mut CheckResult,
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
    f: f64,
) -> Option<&'static str> {
    if ctx.kind == MemberKind::Brace || (forces.qy == 0.0 && forces.qz == 0.0) {
        return None;
    }
    if forces.qz != 0.0 && symmetric_h_web(sec).is_some() {
        return Some(WEAK_H_SHEAR_UNAVAILABLE);
    }
    let Some(shear) = result
        .components
        .iter_mut()
        .find(|c| c.kind == CheckKind::Shear)
    else {
        return Some("作用するせん断力の断面形状・方向に対応する照査条件を確定できません");
    };
    if !shear.ratio.is_finite() {
        return Some("せん断力の照査比を有限値で算定できません");
    }
    if forces.n == 0.0 && forces.my == 0.0 && forces.mz == 0.0 {
        return None;
    }
    let stresses = if let Some(SectionShape::SteelPipe { .. }) = sec.shape {
        (forces.my == 0.0 && forces.mz == 0.0 && !has_section_loss(ctx)).then(|| {
            (
                forces.n.abs() / sec.area,
                forces.qy.hypot(forces.qz) / (sec.area / 2.0),
            )
        })
    } else if let Some((_, web_depth_mm, tw)) = symmetric_h_web(sec) {
        (!has_section_loss(ctx)
            && [web_depth_mm, tw, sec.area, sec.iy, sec.iz]
                .iter()
                .all(|v| v.is_finite() && *v > 0.0))
        .then(|| {
            (
                forces.n.abs() / sec.area
                    + forces.mz.abs() / sec.iy * (web_depth_mm / 2.0)
                    + forces.my.abs() / sec.iz * (tw / 2.0),
                forces.qy.abs() / (web_depth_mm * tw),
            )
        })
    } else {
        None
    };
    if let Some((sigma, tau)) = stresses.filter(|(sigma, tau)| {
        sigma.is_finite() && tau.is_finite() && sec.area.is_finite() && sec.area > 0.0
    }) {
        let ft = crate::material_strength::steel_ft(f, ctx.term);
        let fs = crate::material_strength::steel_fs(f, ctx.term);
        let ratio = (sigma / ft).hypot(tau / fs);
        if ratio.is_finite() {
            shear.ratio = shear.ratio.max(ratio);
            shear.detail.push_str(&format!(
                "。式(5.24): 同一点 σ={sigma:.4} N/mm², τ={tau:.4} N/mm², 組合せ応力度比={ratio:.4}"
            ));
            return None;
        }
    }
    Some("式(5.24)の同一点の垂直応力度とせん断応力度を確定できません")
}

/// 分母が極小の場合に安全側デフォルトへ逃がすヘルパー。
fn safe_denom(x: f64) -> f64 {
    if x.abs() > 1e-9 {
        x
    } else {
        1e-9
    }
}

/// 断面係数 Z = I / (半せい)。半せいが極小なら 0（呼び出し側で 1.0 にフォールバック）。
fn section_modulus(i: f64, half_dim: f64) -> f64 {
    if half_dim > 1e-9 {
        i / half_dim
    } else {
        0.0
    }
}

fn nonzero(z: f64) -> f64 {
    if z.abs() > 1e-9 {
        z
    } else {
        1.0
    }
}

fn has_section_loss(ctx: &DesignCtx) -> bool {
    ctx.steel_attr.as_ref().is_some_and(|attr| {
        [
            attr.joint_flange_loss,
            attr.joint_web_loss,
            attr.scallop_web_loss,
        ]
        .iter()
        .any(|&loss| loss != 0.0)
    })
}

fn independent_axial_failure(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
    f: f64,
) -> Option<CheckComponent> {
    if !forces.qy.is_finite()
        || !forces.qz.is_finite()
        || forces.my != 0.0
        || forces.mz != 0.0
        || sec.property_basis.area != PropertyBasis::Shape
        || has_section_loss(ctx)
    {
        return None;
    }
    let area_mm2 = sec.shape.as_ref()?.try_calc_area().ok()?;
    axial_material_limit_failure(forces.n, area_mm2, ctx.term, f)
}

fn axial_material_limit_failure(
    axial_n: f64,
    area_mm2: f64,
    term: crate::LoadTerm,
    f: f64,
) -> Option<CheckComponent> {
    axial_limit_failure(axial_n, area_mm2, steel_ft(f, term), "材料許容上限")
}

fn axial_limit_failure(
    axial_n: f64,
    area_mm2: f64,
    limit_mpa: f64,
    limit_label: &str,
) -> Option<CheckComponent> {
    if ![area_mm2, limit_mpa]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        return None;
    }
    let stress_mpa = axial_n.abs() / area_mm2;
    let ratio = stress_mpa / limit_mpa;
    (ratio.is_finite() && ratio > 1.0).then(|| CheckComponent {
        kind: CheckKind::Axial,
        ratio,
        detail: format!(
            "N={axial_n:.4} N, A={area_mm2:.4} mm², |N|/A={stress_mpa:.4} N/mm², {limit_label}={limit_mpa:.4} N/mm²",
        ),
    })
}

fn independent_h_column_compression_failure(
    axial_n: f64,
    area_mm2: f64,
    mat: &Material,
    term: crate::LoadTerm,
    f_mpa: f64,
    slenderness: &column::HColumnCompressionSlenderness,
) -> Option<CheckComponent> {
    let Some((fc_mpa, label)) = slenderness.allowable_compression_limit(f_mpa, mat.young, term)
    else {
        return axial_material_limit_failure(axial_n, area_mm2, term, f_mpa);
    };
    let mut check = axial_limit_failure(axial_n, area_mm2, fc_mpa, label)?;
    if slenderness.reason().is_some() {
        check
            .detail
            .push_str(&format!("、軸力比の下限={:.4}", check.ratio));
    }
    Some(check)
}

fn compression_buckling_lengths_mm(ctx: &DesignCtx, secondary_post: bool) -> [f64; 2] {
    if secondary_post || ctx.length == 0.0 {
        [
            ctx.lk_y.unwrap_or(ctx.length),
            ctx.lk_z.unwrap_or(ctx.length),
        ]
    } else {
        let (strong_mm, weak_mm) = buckling_lengths_with_defaults(
            ctx.kind,
            ctx.node_span_mm.unwrap_or(ctx.length),
            ctx.lk_y,
            ctx.lk_z,
        );
        [strong_mm, weak_mm]
    }
}

fn check_compression_slenderness(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
    torsion_reason: Option<&str>,
    secondary_post: bool,
) -> Result<Option<f64>, CheckOutcome> {
    if forces.n.is_finite() && forces.n >= 0.0 {
        return Ok(None);
    }
    if forces.n < 0.0 && matches!(sec.shape, Some(SectionShape::SteelAngle { .. })) {
        return Err(CheckOutcome::Skipped {
            reason: "圧縮山形鋼は幾何軸と主軸が異なり、主軸に対応する有効座屈長さが未確認のため、有効細長比の法定上限を含む鋼部材検定は未算定です".into(),
        });
    }
    let lengths_mm = compression_buckling_lengths_mm(ctx, secondary_post);
    if !secondary_post
        && ctx.length != 0.0
        && lengths_mm
            .iter()
            .any(|&length| !length.is_finite() || length <= 0.0)
    {
        return Err(CheckOutcome::Skipped {
            reason: "圧縮材の強軸・弱軸の座屈長さには有限な正値が必要です".into(),
        });
    }
    if !forces.n.is_finite()
        || [sec.area, sec.iy, sec.iz]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        || lengths_mm.iter().any(|v| !v.is_finite() || *v < 0.0)
    {
        return Err(CheckOutcome::Skipped {
            reason: "有効細長比の法定上限を判定できません。軸力・断面性能・有効座屈長さを確認してください".into(),
        });
    }
    let radii_mm = [(sec.iy / sec.area).sqrt(), (sec.iz / sec.area).sqrt()];
    let lambdas = [lengths_mm[0] / radii_mm[0], lengths_mm[1] / radii_mm[1]];
    if radii_mm.iter().any(|v| !v.is_finite() || *v <= 0.0)
        || lambdas.iter().any(|v| !v.is_finite())
    {
        return Err(CheckOutcome::Skipped {
            reason: "有効細長比を有限値で算定できないため法定上限を判定できません".into(),
        });
    }
    let lambda = lambdas[0].max(lambdas[1]);
    let limit = if ctx.kind == MemberKind::Column {
        200.0
    } else {
        250.0
    };
    if lambda <= limit {
        return Ok(Some(lambda));
    }
    Err(compression_slenderness_failure(
        lambda,
        limit,
        false,
        torsion_reason,
    ))
}

fn compression_slenderness_failure(
    lambda: f64,
    limit: f64,
    lower_bound: bool,
    torsion_reason: Option<&str>,
) -> CheckOutcome {
    let label = if lower_bound {
        "有効細長比の下限"
    } else {
        "有効細長比"
    };
    let mut detail = format!(
        "{label}λ={lambda} > 法定上限{limit}。λ/上限={ratio}（応力度比ではありません）",
        ratio = lambda / limit
    );
    if let Some(reason) = torsion_reason {
        detail.push('。');
        detail.push_str(reason);
    }
    CheckOutcome::Checked(CheckResult {
        basis: "建築基準法施行令第65条 有効細長比上限".into(),
        detail: String::new(),
        components: vec![CheckComponent {
            kind: CheckKind::Provision,
            ratio: lambda / limit,
            detail,
        }],
    })
}

/// 鋼部材の許容応力度検定。
/// 平鋼の柱・大梁で片方向でもせん断力が非零なら、理由付きの未算定とする。
/// 上下のフランジ厚が異なる組立H形鋼の柱・大梁では、強軸せん断力が非零または非有限の位置のせん断検定を理由付きで未算定とする。
/// 上下非対称組立H形鋼の柱・大梁では、弱軸せん断力が非零または非有限の位置のせん断検定を理由付きで未算定とする。
/// H形鋼柱で弱軸せん断と強軸曲げが同時に作用する位置は理由付きで未算定とする。
/// 圧縮・軸力なしでは確認済みの不適合を保持し、組合せの未算定理由を併記する。
/// 上下非対称組立H形鋼柱の圧縮・強軸曲げは、無欠損の断面単独照査と独立して確定できる不適合を返す。
/// 軸力ゼロでの同柱の強軸曲げは、材長と各座屈長さが0で局部座屈の影響を省ける無欠損断面を照査する。
/// 上下非対称組立H形鋼大梁の単独強軸曲げは、明示された横座屈長さ0で照査する。
/// 非零または非有限のねじりモーメントでは応力度検定を未算定とし、法定細長比の不適合時も理由を併記する。
pub struct SteelDesign;

#[derive(Clone, Copy)]
enum SteelCheckScope {
    Ordinary,
    StaticColumn(Option<(f64, f64)>),
    TimeHistory,
    SecondaryPost,
}

/// 一般H形鋼梁の補剛区間全体に対して成立する符号付きの強軸曲げ分布。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SteelBeamMomentDiagram {
    /// 区間内で符号付き曲げが単調。材軸 i/j 順の端部曲げモーメント [N·mm]。
    Monotonic([f64; 2]),
    /// 強軸曲げの絶対値最大が区間内部にある。端部と同値で確認できた平坦区間を含む。
    InteriorMaximum,
    /// 単調または内部最大を確認できていない。
    #[default]
    Unverified,
    /// 一本部材の補剛区間が未確認。要素単体の長さや部分区間指定で代替しない。
    UnverifiedGroup,
    /// 中間横補剛の位置または区間分布を確認できていない。
    UnverifiedBracing,
}

pub(crate) use beam::beam_lateral_buckling_length;
pub use column::ColumnStrongBendingCheck;

impl DesignCheck for SteelDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        let (outcome, incomplete) = self.check_with_incomplete(forces, sec, mat, ctx);
        let mut outcomes =
            crate::member_design_check::strength_outcomes(outcome, incomplete.as_deref());
        let mut first = outcomes.next().expect("強度検定結果は少なくとも1件");
        if let (CheckOutcome::Checked(result), Some(incomplete)) = (&mut first, incomplete) {
            result.basis.push('。');
            result.basis.push_str(&incomplete);
        }
        first
    }
}

impl SteelDesign {
    /// 条件を確定できない位置では、未評価理由と独立した不適合を別の行で返す。
    pub fn check_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> impl Iterator<Item = CheckOutcome> {
        self.check_outcomes_in_scope(forces, sec, mat, ctx, SteelCheckScope::Ordinary)
    }

    fn check_outcomes_in_scope(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        scope: SteelCheckScope,
    ) -> impl Iterator<Item = CheckOutcome> {
        let mut incomplete = None;
        let outcome = self.check_section(forces, sec, mat, ctx, scope, &mut incomplete);
        crate::member_design_check::strength_outcomes(outcome, incomplete.as_deref())
    }

    /// 確認済み単一区間の同一時刻の最大強軸曲げを短期照査する。
    /// 補剛長は mm、最大曲げ位置の断面力は同じ区間分布の応答を指定する。適用外は理由付きエラー。
    pub fn time_history_column_strong_bending(
        &self,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        lb_mm: f64,
        span: &sepika_element::frame::member_load::FrameStrongBendingSpan,
        peak: &MemberForcesAt,
    ) -> Result<ColumnStrongBendingCheck, String> {
        column::time_history_column_strong_bending(sec, mat, ctx, lb_mm, span, peak)
    }

    /// 上下対称H形鋼柱の強軸曲げ [N·mm] が材料の許容上限を超える場合だけ、独立した不適合の比を返す。
    pub fn time_history_column_bending_material_failure(
        &self,
        mz_nmm: f64,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> Option<f64> {
        column::strong_bending_material_excess(mz_nmm, sec, mat, ctx).map(|(_, _, ratio)| ratio)
    }

    /// 対称H形鋼柱の強軸横座屈は別途区間で照査する。
    /// 強軸曲げを含む行では既知の不適合のみを返し、包絡は成分を組み合わせない。
    pub fn check_time_history_h_column_section(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &mut DesignCtx,
        envelope: bool,
    ) -> Vec<CheckOutcome> {
        if forces.mz == 0.0 {
            return self
                .check_outcomes_in_scope(forces, sec, mat, ctx, SteelCheckScope::TimeHistory)
                .collect();
        }
        let mut outcomes = Vec::new();
        let mut known_checks = |sample: MemberForcesAt, ctx: &DesignCtx| {
            outcomes.extend(
                self.check_outcomes_in_scope(&sample, sec, mat, ctx, SteelCheckScope::TimeHistory)
                    .filter_map(|outcome| {
                        let CheckOutcome::Checked(mut result) = outcome else {
                            return None;
                        };
                        result.components.retain(|c| c.ratio > 1.0);
                        (!result.components.is_empty()).then_some(CheckOutcome::Checked(result))
                    }),
            );
        };

        let strong_issue = if envelope {
            None
        } else {
            column::confirmed_time_history_strong_modulus(sec).err()
        };
        if envelope || strong_issue.is_some() {
            for (component, value) in [forces.n, forces.qy, forces.qz, 0.0, forces.my]
                .into_iter()
                .enumerate()
                .filter(|(_, value)| *value != 0.0)
            {
                let mut isolated = [0.0; 6];
                isolated[component] = value;
                known_checks(MemberForcesAt::from_local(forces.pos, isolated), ctx);
            }
            if let Some(failure) =
                column::strong_bending_material_failure(forces, sec, mat, ctx, None)
            {
                outcomes.push(failure);
            }
        } else {
            let previous_length = ctx.column_lateral_buckling_length.replace(Err(
                "横座屈を考慮する組合せはこの評価位置だけから確定できません",
            ));
            known_checks(forces.clone(), ctx);
            ctx.column_lateral_buckling_length = previous_length;
        }

        if let Some(reason) = strong_issue {
            outcomes.push(CheckOutcome::Skipped {
                reason: reason.into(),
            });
        }
        if let Some(reason) = steel_torsion_reason(forces, sec) {
            outcomes.push(CheckOutcome::Skipped {
                reason: reason.into(),
            });
        }
        if forces.n != 0.0 || forces.my != 0.0 || forces.qz != 0.0 {
            let reason = if envelope {
                "時刻歴の成分別包絡から、横座屈を考慮する組合せの適否は確定できません"
            } else {
                "軸力・弱軸曲げ・弱軸せん断との横座屈を考慮する組合せは未算定です"
            };
            outcomes.push(CheckOutcome::Skipped {
                reason: reason.into(),
            });
        }
        outcomes
    }

    /// 成分別の時刻歴包絡では、単一補剛区間の強軸曲げと組合せは未算定とする。
    /// その場合は各成分だけから確定する不適合を別行で返す。
    pub fn check_time_history_envelope_outcomes(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &mut DesignCtx,
    ) -> Vec<CheckOutcome> {
        let (lb, partial) = beam_lateral_buckling_length(ctx, forces.pos);
        if ctx.kind != MemberKind::Girder
            || !is_symmetric_h_section(sec)
            || !lb.is_finite()
            || lb <= 1e-9
            || partial
            || forces.mz == 0.0
        {
            return self
                .check_outcomes_in_scope(forces, sec, mat, ctx, SteelCheckScope::TimeHistory)
                .collect();
        }
        let diagram = std::mem::take(&mut ctx.steel_beam_moment_diagram);
        let mut outcomes = Vec::new();
        for (axis, force) in [
            forces.n, forces.qy, forces.qz, forces.mx, forces.my, forces.mz,
        ]
        .into_iter()
        .enumerate()
        .filter(|(_, force)| *force != 0.0)
        {
            let mut isolated = [0.0; 6];
            isolated[axis] = force;
            let single = MemberForcesAt::from_local(forces.pos, isolated);
            if let CheckOutcome::Checked(mut result) = self.check(&single, sec, mat, ctx) {
                result
                    .components
                    .retain(|c| c.ratio.is_finite() && c.ratio > 1.0);
                if !result.components.is_empty() {
                    outcomes.push(CheckOutcome::Checked(result));
                }
            }
        }
        ctx.steel_beam_moment_diagram = diagram;
        let mut reason = "単一補剛区間の強軸曲げ分布を確認できず、横座屈を考慮した強軸曲げは未算定です。時刻歴の成分別包絡から同時に作用する組合せを確定できません".to_owned();
        if let Some(issue) = steel_torsion_reason(forces, sec) {
            reason.push('。');
            reason.push_str(issue);
        }
        outcomes.push(CheckOutcome::Skipped { reason });
        outcomes
    }

    pub(crate) fn check_with_incomplete(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> (CheckOutcome, Option<String>) {
        let mut incomplete = None;
        let outcome = self.check_section(
            forces,
            sec,
            mat,
            ctx,
            SteelCheckScope::Ordinary,
            &mut incomplete,
        );
        (outcome, incomplete)
    }

    pub(crate) fn check_static_column_section(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        parameters: Option<(f64, f64)>,
        section_issue: Option<&str>,
    ) -> (CheckOutcome, Option<String>) {
        let section_issue = section_issue.or_else(|| {
            if has_section_loss(ctx) {
                Some("断面欠損の位置と断面性能を確定できないため、上下対称H形鋼柱の強軸曲げ・横座屈は未算定です")
            } else if [sec.area, sec.iy, sec.iz, sec.depth, sec.width]
                .into_iter()
                .any(|value| !value.is_finite() || value <= 0.0)
            {
                Some("軸曲げ検定に必要な断面性能を有限な正値で確定できません")
            } else {
                None
            }
        });
        let mut incomplete = None;
        let outcome = match section_issue {
            Some(reason) => CheckOutcome::Skipped {
                reason: reason.into(),
            },
            None => self.check_section(
                forces,
                sec,
                mat,
                ctx,
                SteelCheckScope::StaticColumn(parameters),
                &mut incomplete,
            ),
        };
        if let CheckOutcome::Skipped { reason } = &outcome {
            let mut components = Vec::new();
            let shape_confirmed = symmetric_h_web(sec).is_some();
            if !has_section_loss(ctx) && forces.n.is_finite() && forces.n < 0.0 {
                let verified = column::HColumnCompressionSlenderness::resolve(sec, ctx);
                if let Some(lambda) = verified.lower_bound().filter(|lambda| *lambda > 200.0) {
                    if let CheckOutcome::Checked(slenderness) = compression_slenderness_failure(
                        lambda,
                        200.0,
                        verified.reason().is_some(),
                        steel_torsion_reason(forces, sec),
                    ) {
                        components.extend(slenderness.components);
                    }
                }
                if shape_confirmed {
                    components.extend(
                        sec.resolved_area()
                            .ok()
                            .zip(crate::material_strength::section_steel_f(sec, Some(mat)).ok())
                            .filter(|(_, f)| {
                                verified
                                    .allowable_compression_limit(*f, mat.young, ctx.term)
                                    .is_some()
                            })
                            .and_then(|(area, f)| {
                                independent_h_column_compression_failure(
                                    forces.n, area, mat, ctx.term, f, &verified,
                                )
                            }),
                    );
                }
            }
            if shape_confirmed {
                let (_, flange_thick_mm, web_thick_mm) = shape_of(sec);
                let plate_f = |thick_mm| {
                    sepika_core::material_grade::resolve_steel_f(Some(mat), Some(thick_mm)).ok()
                };
                if let Some(f_cap_mpa) = plate_f(flange_thick_mm)
                    .zip(plate_f(web_thick_mm))
                    .map(|(flange, web)| flange.max(web))
                {
                    if sec.property_basis.area == PropertyBasis::Shape
                        && !components
                            .iter()
                            .any(|component| component.kind == CheckKind::Axial)
                    {
                        components.extend(sec.resolved_area().ok().and_then(|area| {
                            axial_material_limit_failure(forces.n, area, ctx.term, f_cap_mpa)
                        }));
                    }
                    if let Some(CheckOutcome::Checked(bending)) =
                        column::strong_bending_material_failure(
                            forces,
                            sec,
                            mat,
                            ctx,
                            Some(f_cap_mpa),
                        )
                    {
                        components.extend(bending.components);
                    }
                }
            }
            components.extend(independent_h_web_shear_failure(forces, sec, mat, ctx));
            if !components.is_empty() {
                return (
                    CheckOutcome::Checked(CheckResult {
                        basis: "鋼構造許容応力度設計規準 H形鋼柱の独立照査".into(),
                        detail: String::new(),
                        components,
                    }),
                    crate::member_design_check::combine_incomplete_reasons(
                        incomplete,
                        Some(reason),
                    ),
                );
            }
        }
        (outcome, incomplete)
    }

    /// 上下非対称組立H形鋼柱の圧縮・強軸曲げに対し、上下縁の支配比と併記する未算定理由を返す。
    /// 検定比は座屈等の未算定時には下限となり、1以下でも適合を確定できない。
    pub fn asymmetric_column_section_check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> Result<(f64, Vec<&'static str>), String> {
        let shape = sec
            .shape
            .as_ref()
            .filter(|shape| ctx.kind == MemberKind::Column && shape.is_asymmetric_built_h())
            .ok_or("上下非対称組立H形鋼柱の断面が必要です")?;
        if let Some(reason) = steel_torsion_reason(forces, sec) {
            return Err(reason.into());
        }
        if ![forces.n, forces.mz, forces.my, forces.qy, forces.qz]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err("断面力が非有限です".into());
        }
        if forces.n > 0.0 {
            return Err(if forces.mz != 0.0 {
                "引張軸力と強軸曲げが同時に作用する場合の上下非対称断面の適用条件が未確定です"
            } else {
                "引張軸力のみの位置は軸力極値の照査で確認してください"
            }
            .into());
        }
        let f = crate::material_strength::section_steel_f(sec, Some(mat))?;
        let [strong, weak] = built_h_shear_reasons(forces, sec, true, true, None);
        let combined = h_column_weak_shear_combination_reason(forces, sec, true, None);
        let stress = column::asymmetric_built_h_section_stress(
            forces,
            sec,
            shape,
            mat,
            ctx,
            f,
            [combined, strong, weak],
        )?;
        Ok((stress.ratio, stress.unavailable))
    }

    pub(crate) fn check_secondary_post(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> (CheckOutcome, Option<String>) {
        let mut incomplete = None;
        let outcome = self.check_section(
            forces,
            sec,
            mat,
            ctx,
            SteelCheckScope::SecondaryPost,
            &mut incomplete,
        );
        (outcome, incomplete)
    }

    fn check_section(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
        scope: SteelCheckScope,
        incomplete: &mut Option<String>,
    ) -> CheckOutcome {
        let secondary_post = matches!(scope, SteelCheckScope::SecondaryPost);
        let lateral_buckling = if secondary_post {
            column::ColumnLateralBuckling::OutsideScope
        } else if matches!(scope, SteelCheckScope::StaticColumn(None)) && forces.mz != 0.0 {
            column::ColumnLateralBuckling::Unconfirmed("単一補剛区間の強軸曲げ・横座屈は未算定です")
        } else {
            column::ColumnLateralBuckling::resolve(ctx, sec, forces)
        };
        if let column::ColumnLateralBuckling::Unconfirmed(reason) = lateral_buckling {
            *incomplete = Some(reason.to_owned());
        }
        let torsion_reason = steel_torsion_reason(forces, sec);
        let primary_column_or_girder =
            !secondary_post && matches!(ctx.kind, MemberKind::Column | MemberKind::Girder);
        let asymmetric_built_h = sec
            .shape
            .as_ref()
            .filter(|shape| primary_column_or_girder && shape.is_asymmetric_built_h());
        let [built_h_strong_shear_reason, built_h_weak_shear_reason] = built_h_shear_reasons(
            forces,
            sec,
            primary_column_or_girder,
            asymmetric_built_h.is_some(),
            torsion_reason,
        );
        let built_h_shear_reasons = [built_h_strong_shear_reason, built_h_weak_shear_reason];
        let built_h_shear_unavailable = built_h_shear_reasons.iter().any(Option::is_some);
        let h_combined_reason = h_column_weak_shear_combination_reason(
            forces,
            sec,
            !secondary_post && ctx.kind == MemberKind::Column,
            torsion_reason,
        );
        let built_h_unavailable_reasons = || {
            built_h_shear_reasons
                .into_iter()
                .flatten()
                .chain(h_combined_reason)
                .collect::<Vec<_>>()
                .join("。")
        };
        let asymmetric_shape = asymmetric_built_h
            .filter(|_| ctx.kind == MemberKind::Column && forces.n <= 0.0 && forces.mz != 0.0);
        let asymmetric_girder =
            asymmetric_built_h.filter(|_| ctx.kind == MemberKind::Girder && forces.mz != 0.0);
        let symmetric_h_column = ctx.kind == MemberKind::Column && symmetric_h_web(sec).is_some();
        let ordinary_weak_h_column = symmetric_h_column
            && matches!(
                scope,
                SteelCheckScope::Ordinary | SteelCheckScope::StaticColumn(_)
            )
            && forces.my != 0.0;
        let check_plate_slenderness = if forces.my == 0.0 {
            forces.n < 0.0 || forces.mz != 0.0
        } else {
            ordinary_weak_h_column
        };
        let plate_slenderness_reason = (!secondary_post
            && symmetric_h_column
            && torsion_reason.is_none()
            && check_plate_slenderness)
            .then(|| column::check_symmetric_h_plate_slenderness(forces, sec, mat).err())
            .flatten();
        let weak_h_slenderness = (ordinary_weak_h_column && forces.n.is_finite() && forces.n < 0.0)
            .then(|| column::HColumnCompressionSlenderness::resolve(sec, ctx));
        let weak_h_slenderness_reason = weak_h_slenderness
            .as_ref()
            .and_then(column::HColumnCompressionSlenderness::reason);
        let incomplete_weak_h_column = ordinary_weak_h_column
            && (plate_slenderness_reason.is_some() || weak_h_slenderness_reason.is_some());
        let h_combined_reason =
            h_combined_reason.filter(|_| !incomplete_weak_h_column || forces.qz.is_finite());
        let weak_shear_reason = (incomplete_weak_h_column
            && torsion_reason.is_none()
            && forces.qz.is_finite()
            && forces.qz != 0.0
            && h_combined_reason.is_none())
        .then_some(WEAK_H_SHEAR_UNAVAILABLE);
        let invalid_shear = !forces.qy.is_finite() || !forces.qz.is_finite();
        let invalid_shear_reason =
            (incomplete_weak_h_column && invalid_shear).then_some(NONFINITE_SHEAR_FORCE);
        let slenderness = if let Some(verified) = &weak_h_slenderness {
            let lower_bound = verified.lower_bound();
            let limit = 200.0;
            if let Some(lambda) = lower_bound.filter(|&lambda| lambda > limit) {
                *incomplete = crate::member_design_check::combine_incomplete_reasons(
                    incomplete.take(),
                    weak_h_slenderness_reason,
                );
                Err(compression_slenderness_failure(
                    lambda,
                    limit,
                    weak_h_slenderness_reason.is_some(),
                    torsion_reason,
                ))
            } else if let Some(reason) = weak_h_slenderness_reason {
                Err(CheckOutcome::Skipped {
                    reason: reason.into(),
                })
            } else {
                Ok(lower_bound)
            }
        } else {
            check_compression_slenderness(forces, sec, ctx, torsion_reason, secondary_post)
        };
        let lambda = match slenderness {
            Ok(Some(lambda)) if !secondary_post => lambda,
            Ok(_) => crate::effective_slenderness(
                sec.iy,
                sec.iz,
                nonzero(sec.area),
                ctx.length,
                ctx.lk_y,
                ctx.lk_z,
            ),
            Err(mut outcome) => {
                if let (Some(torsion_reason), CheckOutcome::Skipped { reason }) =
                    (torsion_reason, &mut outcome)
                {
                    reason.push('。');
                    reason.push_str(torsion_reason);
                }
                for shear_reason in h_combined_reason
                    .filter(|_| forces.n <= 0.0)
                    .into_iter()
                    .chain(built_h_shear_reasons.into_iter().flatten())
                    .chain(weak_shear_reason)
                    .chain(invalid_shear_reason)
                    .chain(plate_slenderness_reason.as_deref())
                {
                    match &mut outcome {
                        CheckOutcome::Checked(_) => {
                            *incomplete = crate::member_design_check::combine_incomplete_reasons(
                                incomplete.take(),
                                Some(shear_reason),
                            );
                        }
                        CheckOutcome::Skipped { reason } => {
                            reason.push('。');
                            reason.push_str(shear_reason);
                        }
                    }
                }
                if let (Some(_), CheckOutcome::Checked(result)) = (asymmetric_shape, &mut outcome) {
                    for reason in column::asymmetric_built_h_section_reason(forces, sec, ctx)
                        .into_iter()
                        .chain(column::asymmetric_built_h_combination_reasons(forces, ctx))
                    {
                        result.basis.push('。');
                        result.basis.push_str(reason);
                    }
                }
                if let (Some(_), None, CheckOutcome::Checked(result)) =
                    (asymmetric_girder, torsion_reason, &mut outcome)
                {
                    for reason in beam::asymmetric_built_h_combination_reasons(forces) {
                        result.basis.push('。');
                        result.basis.push_str(reason);
                    }
                }
                let independent_axial = if incomplete_weak_h_column
                    || (plate_slenderness_reason.is_some()
                        && matches!(&outcome, CheckOutcome::Skipped { .. }))
                {
                    sec.resolved_area()
                        .ok()
                        .zip(crate::material_strength::section_steel_f(sec, Some(mat)).ok())
                        .and_then(|(area_mm2, f)| {
                            if let Some(verified) = &weak_h_slenderness {
                                independent_h_column_compression_failure(
                                    forces.n, area_mm2, mat, ctx.term, f, verified,
                                )
                            } else {
                                axial_material_limit_failure(forces.n, area_mm2, ctx.term, f)
                            }
                        })
                } else {
                    None
                };
                let independent_checks: Vec<_> = independent_axial
                    .into_iter()
                    .chain(independent_h_web_shear_failure(forces, sec, mat, ctx))
                    .collect();
                if !independent_checks.is_empty() {
                    match &mut outcome {
                        CheckOutcome::Checked(result) => {
                            result.components.extend(independent_checks)
                        }
                        CheckOutcome::Skipped { reason } => {
                            *incomplete = crate::member_design_check::combine_incomplete_reasons(
                                incomplete.take(),
                                Some(reason),
                            );
                            outcome = CheckOutcome::Checked(CheckResult {
                                basis: "鋼構造許容応力度設計規準 H形鋼の独立照査".into(),
                                detail: String::new(),
                                components: independent_checks,
                            });
                        }
                    }
                }
                return outcome;
            }
        };
        if let Some(reason) = torsion_reason {
            return CheckOutcome::Skipped {
                reason: reason.into(),
            };
        }
        if matches!(sec.shape, Some(SectionShape::SteelLipChannel { .. })) {
            return CheckOutcome::Skipped {
                reason: lip_channel::unsupported_reason(forces, sec, ctx),
            };
        }
        if matches!(ctx.kind, MemberKind::Girder | MemberKind::Column)
            && matches!(sec.shape, Some(SectionShape::SteelFlatBar { .. }))
            && (forces.qy != 0.0 || forces.qz != 0.0)
        {
            return CheckOutcome::Skipped {
                reason: "平鋼のせん断応力度の評価方法と適用する許容値の組合せが未確定のため、鋼部材検定は未算定です".into(),
            };
        }
        if h_combined_reason.is_some() && forces.n > 0.0 && symmetric_h_web(sec).is_none() {
            return CheckOutcome::Skipped {
                reason: built_h_unavailable_reasons(),
            };
        }
        if let Some(shape) = sec
            .shape
            .as_ref()
            .filter(|shape| shape.has_unequal_box_thickness())
        {
            if let Err(reason) = shape.validate_surface_radius() {
                return CheckOutcome::Skipped { reason };
            }
            return CheckOutcome::Skipped {
                reason: "異厚BOXのせん断性能の採用式・適用条件が未確認のため鋼部材検定は未算定です"
                    .into(),
            };
        }
        if let Some(shape @ SectionShape::SteelBox { .. }) = &sec.shape {
            if let Err(reason) = shape.validate_surface_radius() {
                return CheckOutcome::Skipped { reason };
            }
        }
        let f = match crate::material_strength::section_steel_f(sec, Some(mat)) {
            Ok(f) => f,
            Err(reason) => {
                let mut reason = reason;
                for issue in h_combined_reason
                    .into_iter()
                    .chain(built_h_shear_reasons.into_iter().flatten())
                    .chain(weak_shear_reason)
                    .chain(invalid_shear_reason)
                    .chain(plate_slenderness_reason.as_deref())
                {
                    reason.push('。');
                    reason.push_str(issue);
                }
                return CheckOutcome::Skipped { reason };
            }
        };
        let term = ctx.term;
        if ctx.kind != MemberKind::Brace
            && strong_axis_h_tension(forces, sec)
            && has_section_loss(ctx)
        {
            return CheckOutcome::Skipped {
                reason: "断面欠損がある引張材の有効断面積と断面係数を確定できません".into(),
            };
        }

        if let Some(shape) = asymmetric_girder {
            return beam::check_asymmetric_built_h(
                forces,
                sec,
                shape,
                mat,
                ctx,
                f,
                built_h_shear_reasons,
            );
        }
        if let Some(shape) = asymmetric_shape {
            let outcome = column::check_asymmetric_built_h(
                forces,
                sec,
                shape,
                mat,
                ctx,
                f,
                [
                    h_combined_reason,
                    built_h_strong_shear_reason,
                    built_h_weak_shear_reason,
                ],
            );
            if matches!(outcome, CheckOutcome::Checked(_)) {
                for reason in h_combined_reason
                    .into_iter()
                    .chain(built_h_shear_reasons.into_iter().flatten())
                {
                    *incomplete = crate::member_design_check::combine_incomplete_reasons(
                        incomplete.take(),
                        Some(reason),
                    );
                }
            }
            return outcome;
        }
        if built_h_shear_unavailable && h_combined_reason.is_none() {
            let reason = built_h_unavailable_reasons();
            return match independent_axial_failure(forces, sec, ctx, f) {
                Some(component) => {
                    *incomplete = crate::member_design_check::combine_incomplete_reasons(
                        incomplete.take(),
                        Some(&reason),
                    );
                    CheckOutcome::Checked(CheckResult {
                        basis: "軸応力度が材料の許容上限を超えるため不適合".into(),
                        detail: String::new(),
                        components: vec![component],
                    })
                }
                None => CheckOutcome::Skipped { reason },
            };
        }
        if invalid_shear && !incomplete_weak_h_column {
            return CheckOutcome::Skipped {
                reason: NONFINITE_SHEAR_FORCE.into(),
            };
        }
        let combined_and_shear = built_h_shear_unavailable.then(built_h_unavailable_reasons);
        let h_combined_reason = combined_and_shear.as_deref().or(h_combined_reason);
        if let Some(reason) = plate_slenderness_reason {
            for other_reason in [
                h_combined_reason.or(weak_shear_reason),
                invalid_shear_reason,
            ]
            .into_iter()
            .flatten()
            {
                *incomplete = crate::member_design_check::combine_incomplete_reasons(
                    incomplete.take(),
                    Some(other_reason),
                );
            }
            let components: Vec<_> = sec
                .resolved_area()
                .ok()
                .and_then(|area_mm2| {
                    if let Some(verified) = &weak_h_slenderness {
                        independent_h_column_compression_failure(
                            forces.n, area_mm2, mat, term, f, verified,
                        )
                    } else if forces.n < 0.0 {
                        // 局部座屈で有効面積が減っても、全断面で算定する許容圧縮応力度は変わらない。
                        let fc_mpa = steel_fc(f, mat.young, lambda, term);
                        axial_limit_failure(forces.n, area_mm2, fc_mpa, "fc")
                    } else {
                        axial_material_limit_failure(forces.n, area_mm2, term, f)
                    }
                })
                .into_iter()
                .chain(independent_h_web_shear_failure(forces, sec, mat, ctx))
                .collect();
            if components.is_empty() {
                return CheckOutcome::Skipped { reason };
            }
            *incomplete = crate::member_design_check::combine_incomplete_reasons(
                incomplete.take(),
                Some(&reason),
            );
            return CheckOutcome::Checked(CheckResult {
                basis: "鋼構造許容応力度設計規準 H形鋼柱の独立照査".into(),
                detail: String::new(),
                components,
            });
        }
        let cr = match ctx.kind {
            MemberKind::Girder => beam::check_beam(forces, sec, mat, ctx, f, lambda, incomplete)
                .map(CheckOutcome::Checked),
            MemberKind::Column => column_allowables(
                sec,
                mat,
                ctx,
                f,
                Some(lambda),
                lateral_buckling.length_mm(ctx.length).map(|length_mm| {
                    let parameters = match scope {
                        SteelCheckScope::StaticColumn(parameters) => parameters,
                        _ => None,
                    };
                    (length_mm, parameters)
                }),
            )
            .map(|allowables| {
                column::check_column(forces, sec, ctx, allowables, lambda, &lateral_buckling)
            }),
            MemberKind::Brace => Ok(CheckOutcome::Checked(brace::check_brace(
                forces, sec, mat, ctx, f, term, lambda,
            ))),
        };
        match cr {
            Ok(CheckOutcome::Checked(mut result)) => {
                let shear_reason = check_same_point_shear(&mut result, forces, sec, ctx, f);
                if let Some(reason) = h_combined_reason.or(shear_reason) {
                    result
                        .components
                        .retain(|component| component.ratio.is_finite());
                    *incomplete = crate::member_design_check::combine_incomplete_reasons(
                        incomplete.take(),
                        Some(reason),
                    );
                }
                CheckOutcome::Checked(result)
            }
            Ok(outcome) => outcome,
            Err(reason) => {
                let reason = match h_combined_reason {
                    Some(combined) => format!("鋼横座屈検定: {reason}。{combined}"),
                    None => format!("鋼横座屈検定: {reason}"),
                };
                let Some(component) = independent_h_web_shear_failure(forces, sec, mat, ctx) else {
                    return CheckOutcome::Skipped { reason };
                };
                *incomplete = crate::member_design_check::combine_incomplete_reasons(
                    incomplete.take(),
                    Some(&reason),
                );
                CheckOutcome::Checked(CheckResult {
                    basis: "強軸ウェブせん断が許容値を超えるため不適合".into(),
                    detail: String::new(),
                    components: vec![component],
                })
            }
        }
    }
}

fn steel_torsion_reason(forces: &MemberForcesAt, sec: &Section) -> Option<&'static str> {
    if !forces.mx.is_finite() {
        Some("ねじりモーメントMxが非有限です")
    } else if forces.mx != 0.0 {
        let pure_pipe_torsion = matches!(sec.shape, Some(SectionShape::SteelPipe { .. }))
            && [forces.n, forces.qy, forces.qz, forces.my, forces.mz]
                .iter()
                .all(|&force| force == 0.0);
        Some(if pure_pipe_torsion {
            "円形鋼管に純ねじり（Mx）が作用しています。規準に基づくねじり耐力と適用条件を確定できないため、鋼部材の応力度検定は未算定です"
        } else {
            "ねじりモーメントMxが作用しています。この断面形状・断面力に対するねじりの採用式と、他の断面力がある場合の断面内同一点での組合せ条件が未確定のため、鋼部材の応力度検定は未算定です"
        })
    } else {
        None
    }
}

fn built_h_shear_reasons(
    forces: &MemberForcesAt,
    sec: &Section,
    primary_column_or_girder: bool,
    asymmetric_built_h: bool,
    torsion_reason: Option<&str>,
) -> [Option<&'static str>; 2] {
    let built_h_strong_shear_reason = (primary_column_or_girder
        && torsion_reason.is_none()
        && forces.qy != 0.0
        && matches!(
            sec.shape,
            Some(SectionShape::SteelBuiltH {
                upper_thick,
                lower_thick,
                ..
            }) if upper_thick != lower_thick
        ))
    .then_some(if forces.qy.is_finite() {
        "上下のフランジ厚が異なる組立H形鋼では、実ウェブの内法寸法に対する規準上の公称せん断応力度の評価方法と許容値の適用条件を確定できないため、強軸せん断検定は未算定です"
    } else {
        "上下のフランジ厚が異なる組立H形鋼の強軸せん断力Qyが非有限のため、強軸せん断検定は未算定です"
    });
    let built_h_weak_shear_reason =
        (asymmetric_built_h && torsion_reason.is_none() && forces.qz != 0.0)
            .then_some(if forces.qz.is_finite() {
                "上下非対称組立H形鋼では、弱軸せん断に対する規準上の公称せん断応力度の評価方法と材料の許容せん断応力度の適用条件を確定できないため、弱軸せん断検定は未算定です"
            } else {
                "上下非対称組立H形鋼の弱軸せん断力Qzが非有限のため、弱軸せん断検定は未算定です"
            });
    [built_h_strong_shear_reason, built_h_weak_shear_reason]
}

fn h_column_weak_shear_combination_reason(
    forces: &MemberForcesAt,
    sec: &Section,
    primary_column: bool,
    torsion_reason: Option<&str>,
) -> Option<&'static str> {
    (primary_column
        && torsion_reason.is_none()
        && !forces.n.is_nan()
        && forces.qz != 0.0
        && forces.mz != 0.0
        && shape_of(sec).0 == ShapeCategory::H)
    .then_some(if forces.n > 0.0 {
        "H形鋼柱に弱軸せん断・強軸曲げ・引張軸力が同時に作用しています。弱軸せん断に対するフランジ内の設計用公称応力度の分布を規準から確定できず、同一点の組合せ応力度は未算定です"
    } else {
        "圧縮軸力または軸力なしのH形鋼柱に弱軸せん断と強軸曲げが同時に作用しています。弱軸せん断に対するフランジ内の設計用公称応力度の分布を規準から確定できず、同一点の組合せ応力度は未算定です"
    })
}

#[cfg(test)]
pub(crate) mod test_support {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{Material, MaterialCategory, Section};
    use sepika_core::section_shape::SectionShape;

    pub(crate) fn mat(name: &str) -> Material {
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: name.to_string(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: None,
        }
    }

    pub(crate) fn rect_section(b: f64, d: f64, name: &str) -> Section {
        Section {
            frame_use: Some(sepika_core::model::FrameSectionUse::Girder),
            id: SectionId(0),
            name: name.to_string(),
            area: b * d,
            iy: b * d.powi(3) / 12.0,
            iz: d * b.powi(3) / 12.0,
            j: 0.0,
            depth: d,
            width: b,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }
    }

    /// `SectionShape::SteelH` 付きの断面（実寸 tf/tw を持つ正規経路の検証用）。
    pub(crate) fn h_section(h: f64, b: f64, tw: f64, tf: f64) -> Section {
        let shape = SectionShape::SteelH {
            root_r: Some(0.0),
            height: h,
            width: b,
            web_thick: tw,
            flange_thick: tf,
        };
        shape.to_section(SectionId(0), format!("H-{}x{}x{}x{}", h, b, tw, tf))
    }
}

#[cfg(test)]
mod sm520_tests {
    use super::*;
    use crate::LoadTerm;
    use test_support::{h_section, mat, rect_section};
    fn forces() -> MemberForcesAt {
        MemberForcesAt {
            pos: 0.5,
            n: 2_200_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        }
    }
    #[test]
    fn sm520_out_of_range_is_skipped_for_every_steel_member_and_explicit_fy() {
        let sec = h_section(1000.0, 400.0, 100.001, 80.0);
        for name in ["SM520", "SM520B", "SM520C"] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let mut material = mat(name);
                material.fy = fy;
                for kind in [MemberKind::Brace, MemberKind::Girder, MemberKind::Column] {
                    let ctx = DesignCtx {
                        kind,
                        ..Default::default()
                    };
                    assert!(
                        matches!(SteelDesign.check(&forces(), &sec, &material, &ctx), CheckOutcome::Skipped { reason } if reason.contains("適用範囲") && reason.contains("100.001"))
                    );
                }
                if fy.is_some() {
                    material.name = "任意鋼材".into();
                    assert!(matches!(
                        SteelDesign.check(
                            &forces(),
                            &sec,
                            &material,
                            &DesignCtx {
                                kind: MemberKind::Brace,
                                ..Default::default()
                            }
                        ),
                        CheckOutcome::Checked(_)
                    ));
                }
            }
        }
    }
    #[test]
    fn sm520_325_fails_fixed_tension_demand_which_335_passes() {
        let mut sec = rect_section(100.0, 100.0, "固定面積ブレース");
        sec.thickness = Some(80.0);
        let ctx = DesignCtx {
            kind: MemberKind::Brace,
            term: LoadTerm::Long,
            ..Default::default()
        };
        let checked = SteelDesign
            .check(&forces(), &sec, &mat("SM520B"), &ctx)
            .unwrap_checked();
        assert!((checked.ratio() - 1.0153846153846153).abs() < 1e-12);
        assert!(!checked.ok());
        let mut old = mat("任意鋼材");
        old.fy = Some(335.0);
        let old_checked = SteelDesign
            .check(&forces(), &sec, &old, &ctx)
            .unwrap_checked();
        assert!((old_checked.ratio() - 0.9850746268656716).abs() < 1e-12);
        assert!(old_checked.ok());
        assert!((2_200_000.0 / checked.ratio() - 2_166_666.6666666665).abs() < 1e-8);
        assert!((2_200_000.0 / old_checked.ratio() - 2_233_333.3333333335).abs() < 1e-8);
    }
}
