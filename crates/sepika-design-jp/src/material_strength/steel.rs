//! 鋼材の許容応力度（鋼構造設計規準 1973・構造規定。板厚 [mm] 区分対応の F 値）。
//! F 値対応表は `sepika_core::material_grade` に一本化する。

use crate::LoadTerm;
use sepika_core::model::{Material, Section};
use sepika_core::section_shape::SectionShape;

/// 鋼材の F 値 [N/mm²]（完全一致、板厚 `[mm]` 区分対応）。
pub use sepika_core::material_grade::steel_f_value;

/// 鋼材の F 値 [N/mm²]（前方一致、板厚 `[mm]` 区分対応）。
pub use sepika_core::material_grade::steel_f_value_prefix;

/// 通常鋼部材・鋼小梁の実断面最大板厚から F `[N/mm²]` を求める。
/// 実形状がなければ明示板厚を用い、未知名の直接入力材料には F 用の板厚区分を要求しない。
pub fn section_steel_f(sec: &Section, mat: Option<&Material>) -> Result<f64, String> {
    if let Some(shape) = &sec.shape {
        shape
            .validate_surface_radius()
            .and_then(|()| shape.validate_lip_channel())
            .map_err(|reason| format!("断面「{}」: {reason}", sec.name))?;
        let (dimensions_mm, inner_dimensions_valid) = match *shape {
            SectionShape::SteelBuiltH {
                height,
                upper_width,
                upper_thick,
                lower_width,
                lower_thick,
                web_thick,
            } => (
                vec![height, upper_width, lower_width],
                height > upper_thick + lower_thick && upper_width.min(lower_width) >= web_thick,
            ),
            SectionShape::SteelChannel {
                height,
                width,
                web_thick,
                flange_thick,
            } => (
                vec![height, width],
                height > 2.0 * flange_thick && width >= web_thick,
            ),
            SectionShape::SteelTee {
                height,
                width,
                web_thick,
                flange_thick,
            } => (
                vec![height, width],
                height > flange_thick && width >= web_thick,
            ),
            SectionShape::SteelAngle {
                leg_a,
                leg_b,
                thick,
            } => (vec![leg_a, leg_b], leg_a > thick && leg_b > thick),
            SectionShape::SteelPipe { outer_dia, thick }
            | SectionShape::CftPipe { outer_dia, thick } => {
                (vec![outer_dia], outer_dia > 2.0 * thick)
            }
            SectionShape::SteelFlatBar { width, .. } => (vec![width], true),
            _ => (Vec::new(), true),
        };
        if !inner_dimensions_valid || dimensions_mm.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(format!("断面「{}」の実形状寸法が不正です。寸法は正の有限値 [mm] とし、内法が成立する値を入力してください", sec.name));
        }
    }
    let thicknesses = match sec.shape.as_ref() {
        Some(
            SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelChannel {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelTee {
                web_thick,
                flange_thick,
                ..
            },
        ) => vec![*web_thick, *flange_thick],
        Some(SectionShape::SteelBuiltH {
            web_thick,
            upper_thick,
            lower_thick,
            ..
        }) => vec![*web_thick, *upper_thick, *lower_thick],
        Some(
            SectionShape::SteelBox { thick, .. }
            | SectionShape::SteelAngle { thick, .. }
            | SectionShape::SteelPipe { thick, .. }
            | SectionShape::SteelFlatBar { thick, .. }
            | SectionShape::SteelLipChannel { thick, .. }
            | SectionShape::CftBox { thick, .. }
            | SectionShape::CftPipe { thick, .. },
        ) => vec![*thick],
        Some(SectionShape::SteelRoundBar { dia }) => vec![*dia],
        Some(
            SectionShape::SrcBeamRect {
                steel_web_thick,
                steel_flange_thick,
                ..
            }
            | SectionShape::SrcColumnRect {
                steel_web_thick,
                steel_flange_thick,
                ..
            },
        ) => vec![*steel_web_thick, *steel_flange_thick],
        _ => sec.thickness.into_iter().collect(),
    };
    if thicknesses.iter().any(|t| !t.is_finite() || *t <= 0.0) {
        return Err(format!(
            "断面「{}」の実板厚が不正です。正の有限値 [mm] を入力してください",
            sec.name
        ));
    }
    let thickness_mm = thicknesses.into_iter().reduce(f64::max);
    sepika_core::material_grade::resolve_steel_f(mat, thickness_mm)
        .map_err(|reason| format!("断面「{}」: {reason}", sec.name))
}

/// F 値の板厚区分判定に用いる最大板厚 `[mm]`。
///
/// `Section.shape` があれば形状の最大板厚、なければ `Section.thickness`、
/// いずれもなければ 40mm 以下区分として扱う。
pub fn plate_thickness(sec: &Section) -> f64 {
    if let Some(shape) = &sec.shape {
        match *shape {
            SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelChannel {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelTee {
                web_thick,
                flange_thick,
                ..
            } => return web_thick.max(flange_thick),
            SectionShape::SteelBox { thick, .. }
            | SectionShape::SteelAngle { thick, .. }
            | SectionShape::SteelPipe { thick, .. }
            | SectionShape::SteelFlatBar { thick, .. }
            | SectionShape::SteelLipChannel { thick, .. }
            | SectionShape::CftBox { thick, .. }
            | SectionShape::CftPipe { thick, .. } => return thick,
            SectionShape::SteelRoundBar { dia } => return dia,
            SectionShape::SteelBuiltH {
                web_thick,
                upper_thick,
                lower_thick,
                ..
            } => return web_thick.max(upper_thick).max(lower_thick),
            SectionShape::SrcBeamRect {
                steel_web_thick,
                steel_flange_thick,
                ..
            }
            | SectionShape::SrcColumnRect {
                steel_web_thick,
                steel_flange_thick,
                ..
            } => return steel_web_thick.max(steel_flange_thick),
            SectionShape::RcBeamRect { .. }
            | SectionShape::RcColumnRect { .. }
            | SectionShape::RcColumnCircle { .. }
            | SectionShape::RcWall { .. }
            | SectionShape::RcSlab { .. } => {}
        }
    }
    sec.thickness.unwrap_or(40.0)
}

/// 長期・短期許容引張／曲げ（座屈無視）応力度 ft [N/mm²]。
///
/// `長期 = F/1.5`、`短期 = F`。
pub fn steel_ft(f: f64, term: LoadTerm) -> f64 {
    match term {
        LoadTerm::Long => f / 1.5,
        LoadTerm::Short => f,
    }
}

/// 長期・短期許容せん断応力度 fs [N/mm²]。
///
/// `長期 = F/(1.5·√3)`、`短期 = F/√3`。
pub fn steel_fs(f: f64, term: LoadTerm) -> f64 {
    match term {
        LoadTerm::Long => f / (1.5 * 3.0_f64.sqrt()),
        LoadTerm::Short => f / 3.0_f64.sqrt(),
    }
}

/// 限界細長比 `Λ = √(π²E/(0.6F))`。`e` はヤング係数 [N/mm²]、`f` は基準強度 F [N/mm²]。
pub fn big_lambda(f: f64, e: f64) -> f64 {
    (std::f64::consts::PI.powi(2) * e / (0.6 * f)).sqrt()
}

/// 長期・短期許容圧縮応力度 fc [N/mm²]（座屈考慮、鋼構造設計規準 1973）。
/// `λ = lk/i`、`Λ = √(π²E/(0.6F))` で 2 分岐（`λ ≤ Λ` と `λ > Λ`）。短期は長期の 1.5 倍。
/// `e` は材料のヤング係数 [N/mm²]。
pub fn steel_fc(f: f64, e: f64, lambda: f64, term: LoadTerm) -> f64 {
    let big_l = big_lambda(f, e);
    let r = if big_l > 1e-9 { lambda / big_l } else { 0.0 };
    let fc_long = if lambda <= big_l {
        let nu = 1.5 + (2.0 / 3.0) * r * r;
        f * (1.0 - 0.4 * r * r) / nu
    } else {
        0.277 * f / (r * r)
    };
    match term {
        LoadTerm::Long => fc_long,
        LoadTerm::Short => fc_long * 1.5,
    }
}
