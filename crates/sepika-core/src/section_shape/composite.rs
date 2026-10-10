//! SRC/CFT の複合換算断面性能。
//!
//! - [`CompositeProps`] — 複合換算後の断面性能
//! - [`SectionShape::src_equivalent_props`] — SRC の等価断面性能
//! - [`SectionShape::cft_equivalent_props`] — CFT の等価断面性能

use super::constants::{E_STEEL, KAPPA_RC, NU_CONCRETE, NU_STEEL};
use super::geometry::{h_web_shear_area, rect_torsion_j};
use super::material::concrete_young_modulus_gamma;
use super::types::SectionShape;

/// SRC/CFT の複合換算断面性能（要素剛性用。各種合成構造設計指針）。
///
/// いずれも要素に割り当てた材料（SRC はコンクリート、CFT は鋼管）を基準とした
/// 等価値。質量算定用の断面積（`calc_area`）とは区別して用いること。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositeProps {
    /// 軸剛性用断面積 [mm²]
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
            props.j,
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

    /// CFT 断面（CftBox/CftPipe）の等価断面性能を鋼管基準で算定する
    /// （各種合成構造設計指針: SRC 柱に準じる累加（CFT もこれに準じる）を鋼基準の
    /// 1/n 換算で適用。J は S 柱の J=(sG/cG)·sJ+cJ を鋼基準 J=sJ+cJ/ngs に換算）。
    ///
    /// `es`/`nu_s`: 要素材料（鋼管）のヤング係数・ポアソン比、
    /// `fc`: 充填コンクリート強度（`Material.fc`）、`gamma_c`: 無筋コンクリートの気乾単位体積重量。
    /// νc=0.2 とする。
    /// CftBox/CftPipe 以外、または Ec≤0 では None（鋼管のみの既定値へ
    /// フォールバック）。
    pub fn cft_equivalent_props(
        &self,
        es: f64,
        nu_s: f64,
        fc: f64,
        gamma_c: f64,
    ) -> Option<CompositeProps> {
        self.try_cft_equivalent_props(es, nu_s, fc, gamma_c)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// CFT の材料由来の等価性能。必要な角Rが未知・不正ならエラー。
    pub fn try_cft_equivalent_props(
        &self,
        es: f64,
        nu_s: f64,
        fc: f64,
        gamma_c: f64,
    ) -> Result<Option<CompositeProps>, String> {
        let Some(core) = self.try_cft_core_props()? else {
            return Ok(None);
        };
        let ec = concrete_young_modulus_gamma(fc, gamma_c);
        if !ec.is_finite() || ec <= 0.0 || !es.is_finite() || es <= 0.0 || !nu_s.is_finite() {
            return Ok(None);
        }
        if core.area <= 0.0 {
            return Ok(None);
        }
        let n = es / ec;
        let ngs = n * (1.0 + NU_CONCRETE) / (1.0 + nu_s);
        if !n.is_finite() || !ngs.is_finite() || ngs <= 0.0 {
            return Ok(None);
        }
        let (s_as_y, s_as_z) = match *self {
            SectionShape::CftBox { thick: t, .. } => {
                (2.0 * t * core.inner_width, 2.0 * t * core.inner_height)
            }
            SectionShape::CftPipe { .. } => {
                let a = self.calc_area() / 2.0;
                (a, a)
            }
            _ => unreachable!("cft_core_props が Some を返した非 CFT 断面"),
        };
        Ok(Some(CompositeProps {
            area_ax: self.try_calc_area()? + core.area / n,
            iy: self.try_calc_iy()? + core.iy / n,
            iz: self.try_calc_iz()? + core.iz / n,
            j: self.calc_j() + core.j / ngs,
            as_y: s_as_y + core.area / KAPPA_RC / ngs,
            as_z: s_as_z + core.area / KAPPA_RC / ngs,
        }))
    }
}

/// CFT 断面の**充填コンクリート部分**（鋼管の内法で囲まれる部分）の諸元。
///
/// 剛性側（[`SectionShape::cft_equivalent_props`]。等価断面性能の累加）と
/// 耐力側（`sepika-design-jp` の CFT 軸終局検定）が同じ値を使うために切り出す。
/// 片方だけ式を直せば静かに食い違うため、算定はここ 1 か所に置く。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CftCoreProps {
    /// 内法幅 [mm]（角形は `幅 − 2t`、円形は内法径 `外径 − 2t`）。
    pub inner_width: f64,
    /// 内法せい [mm]（角形は `せい − 2t`、円形は内法径）。
    pub inner_height: f64,
    /// 充填コンクリートの断面積 [mm²]。
    pub area: f64,
    /// 強軸（せい方向まわり）の断面二次モーメント [mm⁴]。
    pub iy: f64,
    /// 弱軸（幅方向まわり）の断面二次モーメント [mm⁴]。円形は `iy` と同値。
    pub iz: f64,
    /// St.Venant ねじり定数 [mm⁴]。
    pub j: f64,
}

impl SectionShape {
    /// CFT の充填コンクリート性能。対象外は `None`。
    /// 必要な角Rが未知・不正なら panic。入力値の検証には `try_cft_core_props` を用いる。
    pub fn cft_core_props(&self) -> Option<CftCoreProps> {
        self.try_cft_core_props()
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// CFT コア性能。必要な角Rが未知・不正ならエラー。対象外は `Ok(None)`。
    pub fn try_cft_core_props(&self) -> Result<Option<CftCoreProps>, String> {
        let rounded = self.rounded_core_properties()?;
        let mut core = self.rectangular_cft_core_props();
        if let (Some(p), Some(core)) = (rounded, &mut core) {
            core.area = p.area;
            core.iy = p.iy;
            core.iz = p.iz;
        }
        Ok(core)
    }

    fn rectangular_cft_core_props(&self) -> Option<CftCoreProps> {
        match *self {
            SectionShape::CftBox {
                height: h,
                width: w,
                thick: t,
                ..
            } => {
                let bi = (w - 2.0 * t).max(0.0);
                let hi = (h - 2.0 * t).max(0.0);
                Some(CftCoreProps {
                    inner_width: bi,
                    inner_height: hi,
                    area: bi * hi,
                    iy: bi * hi.powi(3) / 12.0,
                    iz: hi * bi.powi(3) / 12.0,
                    j: rect_torsion_j(bi, hi),
                })
            }
            SectionShape::CftPipe { outer_dia, thick } => {
                let di = (outer_dia - 2.0 * thick).max(0.0);
                let i = std::f64::consts::PI * di.powi(4) / 64.0;
                Some(CftCoreProps {
                    inner_width: di,
                    inner_height: di,
                    area: std::f64::consts::PI * di * di / 4.0,
                    iy: i,
                    iz: i,
                    j: std::f64::consts::PI * di.powi(4) / 32.0,
                })
            }
            _ => None,
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
