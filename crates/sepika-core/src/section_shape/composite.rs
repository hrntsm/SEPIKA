//! SRC/CFT の複合換算断面性能。
//!
//! - [`CompositeProps`] — 複合換算後の断面性能
//! - [`SectionShape::src_equivalent_props`] — SRC の等価断面性能
//! - [`SectionShape::cft_equivalent_props`] — CFT の等価断面性能

use super::constants::{E_STEEL, KAPPA_RC, NU_CONCRETE, NU_STEEL};
use super::core_torsion::{
    reused_rounded_rect_core_j, rounded_rect_core_torsion, CoreTorsionError, CoreTorsionSolution,
};
use super::geometry::{h_web_shear_area, rect_torsion_j};
use super::material::concrete_young_modulus_gamma;
use super::types::SectionShape;

/// SRC/CFT の複合換算断面性能（要素剛性用。各種合成構造設計指針）。
///
/// いずれも要素に割り当てた材料（SRC はコンクリート、CFT は鋼管）を基準とした
/// 等価値。質量算定用の断面積（`calc_area`）とは区別して用いること。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositeProps {
    /// 軸剛性用断面積 `[mm²]`
    pub area_ax: f64,
    pub iy: f64,
    pub iz: f64,
    pub j: f64,
    pub as_y: f64,
    pub as_z: f64,
}

impl SectionShape {
    /// SRC 等価性能。Ec [N/mm²] は有限正、νc は −1 < νc < 0.5 を要する。
    /// 対象外は `Ok(None)`、材料・必要剛性が不正なら理由付きエラーを返す。
    pub fn src_equivalent_props(
        &self,
        ec: f64,
        nu_c: f64,
    ) -> Result<Option<CompositeProps>, String> {
        let (SectionShape::SrcBeamRect {
            b,
            d,
            steel_height: sh,
            steel_width: sw,
            steel_web_thick: tw,
            steel_flange_thick: tf,
            ..
        }
        | SectionShape::SrcColumnRect {
            b,
            d,
            steel_height: sh,
            steel_width: sw,
            steel_web_thick: tw,
            steel_flange_thick: tf,
            ..
        }) = *self
        else {
            return Ok(None);
        };
        self.validate_src_geometry()
            .map_err(|error| error.to_string())?;
        if !ec.is_finite() || ec <= 0.0 {
            return Err("SRC の Ec は正の有限値が必要です".into());
        }
        if !nu_c.is_finite() || nu_c <= -1.0 || nu_c >= 0.5 {
            return Err("SRC の νc は有限値かつ −1 < νc < 0.5 が必要です".into());
        }
        let ns = E_STEEL / ec;
        let ngs = ns * (1.0 + nu_c) / (1.0 + NU_STEEL);

        if !ns.is_finite() || ns <= 0.0 || !ngs.is_finite() || ngs <= 0.0 {
            return Err("SRC の解析用ヤング係数比・せん断弾性係数比が不正です".into());
        }
        let s_a = 2.0 * sw * tf + (sh - 2.0 * tf) * tw;
        let hw = sh - 2.0 * tf;
        let s_iy = (sw * sh.powi(3) - (sw - tw) * hw.powi(3)) / 12.0;
        let s_iz = (2.0 * tf * sw.powi(3) + hw * tw.powi(3)) / 12.0;
        let s_j = (2.0 * sw * tf.powi(3) + hw * tw.powi(3)) / 3.0;

        let rc_as = b * d / KAPPA_RC;
        let props = CompositeProps {
            area_ax: b * d + (ns - 1.0) * s_a,
            iy: b * d.powi(3) / 12.0 + (ns - 1.0) * s_iy,
            iz: d * b.powi(3) / 12.0 + (ns - 1.0) * s_iz,
            j: rect_torsion_j(b, d) + ngs * s_j,
            as_y: rc_as + (ngs - 1.0) * 2.0 * sw * tf,
            as_z: rc_as + (ngs - 1.0) * h_web_shear_area(sh, tw),
        };
        let gc = ec / (2.0 * (1.0 + nu_c));
        for value in [
            props.area_ax,
            props.iy,
            props.iz,
            props.as_y,
            props.as_z,
            ec * props.area_ax,
            ec * props.iy,
            ec * props.iz,
            gc * props.j,
            gc * props.as_y,
            gc * props.as_z,
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err("SRC の等価断面性能または必要剛性が正の有限値ではありません".into());
            }
        }
        Ok(Some(props))
    }

    /// CFT の鋼管基準等価性能。材料不正・コアJ未確定ならpanicする。
    /// Es・実Gs・FcはN/mm²、γcはkN/m³、供給鋼管Jsはmm⁴。
    pub fn cft_equivalent_props(
        &self,
        es: f64,
        nu_s: f64,
        gs_mpa: f64,
        fc: f64,
        gamma_c: f64,
        steel_j_mm4: f64,
    ) -> Option<CompositeProps> {
        self.try_cft_equivalent_props(es, nu_s, gs_mpa, fc, gamma_c, steel_j_mm4)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// CFTの鋼管基準等価性能。Es・実Gs・Fc [N/mm²]、γc [kN/m³]、鋼管Js `[mm⁴]` は有限正。
    /// −1 < νs < 0.5を要する。対象外のみ`Ok(None)`、材料・幾何・コアJ未確定はエラー。
    pub fn try_cft_equivalent_props(
        &self,
        es: f64,
        nu_s: f64,
        gs_mpa: f64,
        fc: f64,
        gamma_c: f64,
        steel_j_mm4: f64,
    ) -> Result<Option<CompositeProps>, String> {
        if !matches!(self, Self::CftBox { .. } | Self::CftPipe { .. }) {
            return Ok(None);
        }
        for (name, value) in [
            ("Es", es),
            ("Fc", fc),
            ("γc", gamma_c),
            ("鋼管Js", steel_j_mm4),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("CFT の {name} は正の有限値が必要です"));
            }
        }
        if !nu_s.is_finite() || nu_s <= -1.0 || nu_s >= 0.5 {
            return Err("CFT の νs は有限値かつ −1 < νs < 0.5 が必要です".into());
        }
        if !gs_mpa.is_finite() || gs_mpa <= 0.0 {
            return Err("CFT の Gs は正の有限値が必要です".into());
        }
        let ec = concrete_young_modulus_gamma(fc, gamma_c);
        if !ec.is_finite() || ec <= 0.0 {
            return Err("CFT の Ec は正の有限値が必要です".into());
        }
        let gs = gs_mpa;
        let gc = ec / (2.0 * (1.0 + NU_CONCRETE));
        let n = es / ec;
        let ngs = gs / gc;
        if [gs, gc, n, ngs, 1.0 / n, 1.0 / ngs]
            .into_iter()
            .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err("CFT の解析用弾性係数・換算比が正の有限値ではありません".into());
        }
        if self.has_unequal_box_thickness() {
            return Err(
                "異厚BOXのせん断性能の採用式・適用条件が未確認のため、CFT等価剛性は未算定です"
                    .into(),
            );
        }
        let core = self.try_cft_core_props()?.expect("CFT コア幾何");
        let steel_area = self.try_calc_area()?;
        let steel_iy = self.try_calc_iy()?;
        let steel_iz = self.try_calc_iz()?;
        let (s_as_y, s_as_z) = match *self {
            Self::CftBox { thick: t, .. } => {
                (2.0 * t * core.inner_width, 2.0 * t * core.inner_height)
            }
            Self::CftPipe { .. } => (steel_area / 2.0, steel_area / 2.0),
            _ => unreachable!("CFT コア幾何"),
        };
        let mut props = CompositeProps {
            area_ax: steel_area + core.area / n,
            iy: steel_iy + core.iy / n,
            iz: steel_iz + core.iz / n,
            j: steel_j_mm4,
            as_y: s_as_y + core.area / KAPPA_RC / ngs,
            as_z: s_as_z + core.area / KAPPA_RC / ngs,
        };
        validate_cft_non_torsional_stiffness(props, es, gs)?;
        if !(gs * steel_j_mm4).is_finite() {
            return Err("CFT の鋼管ねじり剛性が有限値ではありません".into());
        }
        let (_, _, inner_r_mm) = self.cft_core_dimensions()?.expect("CFT コア幾何");
        let core_j_mm4 = if let Some(inner_r_mm) = inner_r_mm {
            reused_rounded_rect_core_j(core.inner_width, core.inner_height, inner_r_mm)?
        } else {
            self.try_cft_core_torsion()
                .map_err(|error| error.to_string())?
                .expect("CFT コアJ")
                .j_mm4()
        };
        props.j += core_j_mm4 / ngs;
        if [props.j, gs * props.j]
            .into_iter()
            .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err("CFT の等価Jまたはねじり剛性が正の有限値ではありません".into());
        }
        Ok(Some(props))
    }
}

fn validate_cft_non_torsional_stiffness(
    props: CompositeProps,
    es: f64,
    gs: f64,
) -> Result<(), String> {
    for value in [
        props.area_ax,
        props.iy,
        props.iz,
        props.as_y,
        props.as_z,
        es * props.area_ax,
        es * props.iy,
        es * props.iz,
        gs * props.as_y,
        gs * props.as_z,
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err("CFT の等価断面性能または必要剛性が正の有限値ではありません".into());
        }
    }
    Ok(())
}

/// CFT充填コンクリートの幾何性能。ねじり性能を含まない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CftCoreProps {
    /// 内法幅 `[mm]`。円形は内法径。
    pub inner_width: f64,
    /// 内法せい `[mm]`。円形は内法径。
    pub inner_height: f64,
    /// 断面積 `[mm²]`。
    pub area: f64,
    /// 強軸断面二次モーメント `[mm⁴]`。
    pub iy: f64,
    /// 弱軸断面二次モーメント `[mm⁴]`。
    pub iz: f64,
}

/// CFTコア単体の自由ねじり性能。円の解析値と数値解の品質診断を区別する。
#[derive(Debug)]
pub enum CftCoreTorsion {
    AnalyticCircle { j_mm4: f64 },
    Numerical(CoreTorsionSolution),
}

impl CftCoreTorsion {
    /// コア単体のSt.Venantねじり定数 `[mm⁴]`。
    pub fn j_mm4(&self) -> f64 {
        match self {
            Self::AnalyticCircle { j_mm4 } => *j_mm4,
            Self::Numerical(solution) => solution.j_mm4,
        }
    }
}

fn core_input_error(reason: String) -> CoreTorsionError {
    CoreTorsionError {
        reason,
        levels: Vec::new(),
        failed_attempt: None,
        total_seconds: 0.0,
    }
}

impl SectionShape {
    /// CFTコア幾何性能。対象外は`None`、必要寸法・角Rが不正ならpanicする。
    pub fn cft_core_props(&self) -> Option<CftCoreProps> {
        self.try_cft_core_props()
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// CFTコア幾何性能。対象外のみ`Ok(None)`、必要寸法・角Rが未知・不正ならエラー。
    pub fn try_cft_core_props(&self) -> Result<Option<CftCoreProps>, String> {
        let Some((b, h, r)) = self.cft_core_dimensions()? else {
            return Ok(None);
        };
        let core = if r.is_some() {
            let p = self.rounded_core_properties()?.expect("角形CFTコア");
            CftCoreProps {
                inner_width: b,
                inner_height: h,
                area: p.area,
                iy: p.iy,
                iz: p.iz,
            }
        } else {
            let i = std::f64::consts::PI * b.powi(4) / 64.0;
            CftCoreProps {
                inner_width: b,
                inner_height: h,
                area: std::f64::consts::PI * b * b / 4.0,
                iy: i,
                iz: i,
            }
        };
        if [core.area, core.iy, core.iz]
            .into_iter()
            .any(|v| !v.is_finite() || v <= 0.0)
        {
            return Err("CFT コア幾何性能が正の有限値ではありません".into());
        }
        Ok(Some(core))
    }

    /// CFTコア単体J `[mm⁴]`を毎回算定し、数値解では全品質履歴を返す。
    /// 対象外のみ`Ok(None)`。必要寸法・角R不正や数値未確定は履歴付きエラー。
    pub fn try_cft_core_torsion(&self) -> Result<Option<CftCoreTorsion>, CoreTorsionError> {
        let Some((b, h, r)) = self.cft_core_dimensions().map_err(core_input_error)? else {
            return Ok(None);
        };
        if let Some(r) = r {
            rounded_rect_core_torsion(b, h, r)
                .map(|solution| Some(CftCoreTorsion::Numerical(solution)))
        } else {
            let j_mm4 = std::f64::consts::PI * b.powi(4) / 32.0;
            if !j_mm4.is_finite() || j_mm4 <= 0.0 {
                return Err(core_input_error(
                    "CFT 円形コアJが正の有限値ではありません".into(),
                ));
            }
            Ok(Some(CftCoreTorsion::AnalyticCircle { j_mm4 }))
        }
    }

    fn cft_core_dimensions(&self) -> Result<Option<(f64, f64, Option<f64>)>, String> {
        match *self {
            Self::CftBox {
                height,
                width,
                thick,
                horizontal_thick,
                corner_r,
            } => {
                self.validate_surface_radius()?;
                let ro = corner_r.ok_or("CFT コア領域の算定に必要な角Rが未知です")?;
                Ok(Some((
                    width - 2.0 * thick,
                    height - 2.0 * horizontal_thick,
                    Some((ro - thick).max(0.0)),
                )))
            }
            Self::CftPipe { outer_dia, thick } => {
                if !outer_dia.is_finite()
                    || !thick.is_finite()
                    || outer_dia <= 0.0
                    || thick <= 0.0
                    || outer_dia <= 2.0 * thick
                {
                    return Err("CFT 円形コアの外径・板厚は有限正かつ外径 > 2tが必要です".into());
                }
                let d = outer_dia - 2.0 * thick;
                Ok(Some((d, d, None)))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod src_tests {
    use super::*;
    use crate::section_shape::{RcRectColumnRebar, RectColumnHoop};

    fn shape() -> SectionShape {
        SectionShape::SrcColumnRect {
            b: 300.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 0.0,
                x: vec![],
                y: vec![],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 0.0,
                    pitch: 100.0,
                    legs_x: 0,
                    legs_y: 0,
                },
            },
            steel_height: 400.0,
            steel_width: 200.0,
            steel_web_thick: 8.0,
            steel_flange_thick: 12.0,
        }
    }

    #[test]
    fn src_analysis_ratio_matches_independent_rectangular_h_section() {
        let p = shape().src_equivalent_props(20500.0, 0.2).unwrap().unwrap();
        assert_eq!(p.area_ax, 250272.0);
        assert!((p.iy - 7345337856.0).abs() < 1e-5);
        assert!((20500.0 * p.iy - 150579426048000.0).abs() < 0.1);
        let p15 = shape()
            .src_equivalent_props(205000.0 / 15.0, 0.2)
            .unwrap()
            .unwrap();
        assert_eq!(p15.area_ax, 289312.0);
        assert!((p15.iy - 8426081109.333333).abs() < 1e-5);
        assert_ne!(p.iy, p15.iy);
        assert_eq!(shape().calc_area(), 180000.0);
    }

    #[test]
    fn src_material_errors_are_distinct_from_non_src() {
        for ec in [
            0.0,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN_POSITIVE,
            f64::MAX,
        ] {
            assert!(shape().src_equivalent_props(ec, 0.2).is_err(), "Ec={ec}");
        }
        for nu in [
            -1.0,
            0.5,
            -2.0,
            1.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            assert!(shape().src_equivalent_props(20500.0, nu).is_err(), "ν={nu}");
        }
        for nu in [-0.5, 0.0, 0.2, 0.49] {
            assert!(shape().src_equivalent_props(20500.0, nu).unwrap().is_some());
        }
        let non_src = SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        };
        assert_eq!(non_src.src_equivalent_props(f64::NAN, f64::NAN), Ok(None));
        let mut invalid = shape();
        if let SectionShape::SrcColumnRect { d, .. } = &mut invalid {
            *d = f64::NAN;
        }
        assert!(invalid.src_equivalent_props(20500.0, 0.2).is_err());
    }
}
