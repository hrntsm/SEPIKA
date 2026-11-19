//! 鉄筋コンクリート造梁の断面検定（RC 規準13条: 曲げ・せん断・付着）。
//! 強軸曲げ（`mz`）とそれに対のせん断（`qy`）のみを検定する。

use super::{
    axis_props_from_shape, main_rebar_grade, rc_allow, rc_beam_bond_check_1991_with_rebar,
    rc_beam_bond_check_with_rebar, rebar_allowable_tension, rebar_info_from_shape,
    rebar_sigma_y_of, seismic_design_shear, shear_alpha, shear_capacity, shear_rebar_grade,
    AxisProps,
};
use crate::ultimate::rc_props::RcDirection;
use crate::{
    BondMethod, CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm,
    MemberForcesAt,
};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::{RcBeamRebar, SectionShape};

pub(crate) struct BeamMoment {
    /// 引張鉄筋支配の許容曲げモーメント MA_t = at・ft・j。
    pub(crate) ma_t: f64,
    /// 圧縮縁コンクリート支配の許容曲げモーメント MA_c。
    pub(crate) ma_c: f64,
    /// 圧縮縁から中立軸までの距離 [mm]。
    xn_mm: f64,
    /// MA = min(MA_t, MA_c)。
    pub(crate) ma: f64,
}

/// 梁の許容曲げモーメント MA を算定する（RC 規準 13条）。
pub(crate) fn beam_moment_capacity(
    props: &AxisProps,
    ft: f64,
    fc: f64,
    n_ratio: f64,
) -> Option<BeamMoment> {
    let ma_t = props.at * ft * props.j;

    let dc = props.dt;
    let d = props.d;
    let b = props.b;
    let ac = props.ac;
    let at = props.at;

    let b_coef = (n_ratio - 1.0) * ac + n_ratio * at;
    let c_coef = (n_ratio - 1.0) * ac * dc + n_ratio * at * d;
    let xn_mm = ((b_coef * b_coef + 2.0 * b * c_coef).sqrt() - b_coef) / b;
    if !xn_mm.is_finite() || xn_mm <= 0.0 || xn_mm >= d {
        return None;
    }
    let icr = b * xn_mm.powi(3) / 3.0
        + (n_ratio - 1.0) * ac * (xn_mm - dc).powi(2)
        + n_ratio * at * (d - xn_mm).powi(2);
    let ma_c = fc * icr / xn_mm;
    if !ma_t.is_finite() || !ma_c.is_finite() || ma_t <= 0.0 || ma_c <= 0.0 {
        return None;
    }
    Some(BeamMoment {
        ma_t,
        ma_c,
        xn_mm,
        ma: ma_t.min(ma_c),
    })
}

fn layer_count_and_first_moment(layers: &[u32]) -> (u128, u128) {
    layers
        .iter()
        .enumerate()
        .fold((0, 0), |(total, first_moment), (layer, &count)| {
            let count = u128::from(count);
            (total + count, first_moment + (layer as u128) * count)
        })
}

fn bending_component(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
    rebar: &RcBeamRebar,
    props: &AxisProps,
    tension_is_top: bool,
    ft: f64,
    allow: &super::RcAllow,
) -> Result<CheckComponent, &'static str> {
    let demand_nmm = forces.mz.abs();
    let (ratio, detail) = if demand_nmm == 0.0 {
        (0.0, "強軸曲げ需要=0 N·mm".into())
    } else if props.at <= 0.0 {
        return Err("引張側の主筋がありません");
    } else if (forces.pos - 0.5).abs() < 1e-6 && ctx.beam_has_slab && forces.mz > 0.0 {
        let ma_t = props.at * ft * props.j;
        (
            demand_nmm / ma_t,
            format!("MA_t={ma_t:.1} N·mm, MA={ma_t:.1} N·mm, |mz|={demand_nmm:.1} N·mm, 中央+スラブ正曲げ: MA=at·ft·j"),
        )
    } else {
        let (tension, compression) = if tension_is_top {
            (&rebar.top, &rebar.bottom)
        } else {
            (&rebar.bottom, &rebar.top)
        };
        let (tension_count, tension_moment) = layer_count_and_first_moment(tension);
        let (compression_count, compression_moment) = layer_count_and_first_moment(compression);
        // 共通の段間隔を消去すれば、浮動小数点で重心の一致を判定する必要がない。
        if compression_moment * tension_count != tension_moment * compression_count {
            return Err(
                "圧縮縁から圧縮筋重心までの距離と引張縁から引張筋重心までの距離が一致しません",
            );
        }
        let bm = beam_moment_capacity(props, ft, allow.fc, allow.n_ratio)
            .ok_or("矩形断面の許容曲げを求められません")?;
        let (top_inner, bottom_inner) = rebar.innermost_bar_distances_from_edges_mm();
        let (tension_inner, compression_inner) = if tension_is_top {
            (top_inner, bottom_inner)
        } else {
            (bottom_inner, top_inner)
        };
        if compression_inner.is_some_and(|distance_mm| distance_mm >= bm.xn_mm)
            || tension_inner.is_some_and(|distance_mm| props.d_full - distance_mm <= bm.xn_mm)
        {
            return Err("主筋の全段が中立軸の想定側に収まりません（中立軸上の段を含む）");
        }
        (
            demand_nmm / bm.ma,
            format!(
                "MA_t={:.1} N·mm, MA_c={:.1} N·mm, MA={:.1} N·mm, |mz|={demand_nmm:.1} N·mm",
                bm.ma_t, bm.ma_c, bm.ma,
            ),
        )
    };
    Ok(CheckComponent {
        kind: CheckKind::Bending,
        ratio,
        detail,
    })
}

/// 梁の断面検定（RC 規準 13条）。強軸曲げ mz とそれに対のせん断 qy のみを扱う。
pub(crate) fn beam_check(
    forces: &MemberForcesAt,
    mat: &Material,
    ctx: &DesignCtx,
    shape: &SectionShape,
    rebar: &RcBeamRebar,
    fc_raw: f64,
) -> CheckOutcome {
    let long_term = ctx.term == LoadTerm::Long;
    let grade = main_rebar_grade(ctx.rebar_material.as_ref());
    let deformed_rebar = matches!(
        rebar.main_dia,
        sepika_core::section_shape::RebarSize::Standard(_)
    ) || ctx.rebar_material.as_ref().is_some_and(|m| {
        m.category == MaterialCategory::Rebar && !matches!(m.name.trim(), "SR235" | "SR295")
    });
    let allow = rc_allow(
        fc_raw,
        mat.concrete_class,
        shear_rebar_grade(ctx.shear_rebar_material.as_ref()),
        long_term,
    );

    let tension_is_top = if forces.mz < 0.0 {
        true
    } else if forces.mz > 0.0 {
        false
    } else {
        let at_top = axis_props_from_shape(shape, RcDirection::Strong, true).map(|p| p.at);
        let at_bottom = axis_props_from_shape(shape, RcDirection::Strong, false).map(|p| p.at);
        match (at_top, at_bottom) {
            (Some(t), Some(b)) => t < b,
            (Some(_), None) => true,
            _ => false,
        }
    };
    let props = axis_props_from_shape(shape, RcDirection::Strong, tension_is_top)
        .expect("梁の断面諸元を算定できる形状のみ来る");
    let info =
        rebar_info_from_shape(shape, tension_is_top).expect("梁の鉄筋情報を算定できる形状のみ来る");
    let ft = rebar_allowable_tension(grade, info.main_dia.designation_or_diameter_mm(), long_term);

    let bending = match bending_component(forces, ctx, rebar, &props, tension_is_top, ft, &allow) {
        Ok(bending) => bending,
        Err(reason) => {
            return CheckOutcome::Skipped {
                reason: format!(
                    "RC 梁曲げ検定不能（{}曲げ）: {reason}",
                    if tension_is_top { "負" } else { "正" }
                ),
            };
        }
    };

    let (m_for_alpha, q_for_alpha) = ctx.shear_span.unwrap_or((forces.mz.abs(), forces.qy.abs()));
    let alpha = shear_alpha(m_for_alpha, q_for_alpha, props.d, 2.0);
    let damage_control = ctx.rc_damage_control;
    let qa = shear_capacity(&props, &allow, alpha, ctx.term, damage_control, false);
    let q_design = if ctx.seismic_qd.is_some() {
        let mu_inp = sepika_core::rc_capacity::RcCapacityInput {
            b: props.b,
            d: props.d_full,
            at: props.at,
            d_eff: props.d,
            sigma_y: rebar_sigma_y_of(ctx.rebar_material.as_ref()),
            fc: fc_raw,
            pw: props.pw,
            sigma_wy: 0.0,
            clear_span: 0.0,
            sigma_0: 0.0,
        };
        let sum_mu = 2.0 * sepika_core::rc_capacity::rc_mu_simple(&mu_inp);
        seismic_design_shear(ctx, forces.pos, forces.qy, 1, sum_mu, false)
    } else {
        forces.qy.abs()
    };
    let ratio_q = if qa > 0.0 { q_design / qa } else { 0.0 };

    let (ratio_bond, bond_detail) =
        match ctx.bond_method {
            BondMethod::Rc1999 => {
                let bond = rc_beam_bond_check_with_rebar(
                    forces.pos,
                    ctx.length,
                    props.b,
                    props.d,
                    props.j,
                    props.at,
                    forces.mz.abs(),
                    &info,
                    fc_raw,
                    long_term,
                    tension_is_top,
                    deformed_rebar,
                );
                let ratio = bond.as_ref().map(|b| b.ratio).unwrap_or(0.0);
                let detail =
                    bond.as_ref().map(|b| {
                        format!(
                    "1999: ld={:.1} mm, ldb={:.1} mm, K={:.3}, W={:.3} mm, fb={:.3} N/mm², \
                     σt={:.1} N/mm², 付着検定比={:.3}（{}・{}）",
                    b.ld,
                    b.ldb,
                    b.k,
                    b.w,
                    b.fb,
                    b.sigma_t,
                    b.ratio,
                    if b.is_end { "端部" } else { "中央" },
                    if tension_is_top { "上端筋" } else { "下端筋" }
                )
                    });
                (ratio, detail)
            }
            BondMethod::Rc1991 => {
                let n_t = info.tension_count_1991;
                let phi = n_t * info.main_dia.perimeter_mm();
                let is_end = !(0.25 < forces.pos && forces.pos < 0.75);
                let bond = rc_beam_bond_check_1991_with_rebar(
                    q_design,
                    props.j,
                    phi,
                    fc_raw,
                    tension_is_top,
                    long_term,
                    deformed_rebar,
                );
                let ratio = bond.as_ref().map(|b| b.ratio).unwrap_or(0.0);
                let detail = bond.as_ref().map(|b| {
                    format!(
                        "1991: τa={:.3} N/mm², fa={:.3} N/mm², ψ={:.1} mm, \
                     Q={:.1} N, 付着検定比={:.3}（{}・{}）",
                        b.tau,
                        b.fa,
                        phi,
                        q_design,
                        b.ratio,
                        if is_end { "端部" } else { "中央" },
                        if tension_is_top {
                            "上端筋"
                        } else {
                            "下端筋"
                        }
                    )
                });
                (ratio, detail)
            }
        };

    let basis = "RC 規準13条（梁の曲げ・せん断・付着）".to_string();
    let shear_detail = format!(
        "QA={:.1} N, |qy|={:.1} N, α={:.3}, pw={:.5}",
        qa, forces.qy, alpha, props.pw,
    );
    let detail = format!(
        "at={:.1} mm², d={:.1} mm, j={:.1} mm{}",
        props.at,
        props.d,
        props.j,
        if bond_detail.is_some() {
            ""
        } else {
            ", 付着検定: 部材長(柱面間距離)が未設定、または入力不足のため省略"
        },
    );

    let mut components = vec![
        bending,
        CheckComponent {
            kind: CheckKind::Shear,
            ratio: ratio_q,
            detail: shear_detail,
        },
    ];
    if let Some(bond_detail) = bond_detail {
        components.push(CheckComponent {
            kind: CheckKind::Bond,
            ratio: ratio_bond,
            detail: bond_detail,
        });
    }

    let mut detail = detail;
    if super::provisions::is_member_level_station(forces.pos) {
        let mz_for_at = {
            let mut m = forces.mz.abs();
            if let Some((mi, mj)) = ctx.end_moments_z {
                m = m.max(mi.abs()).max(mj.abs());
            }
            if let Some(mc) = ctx.mid_moment_z {
                m = m.max(mc.abs());
            }
            m
        };
        let prov = super::provisions::beam_provisions(
            props.d_full,
            &props,
            &info,
            long_term,
            mz_for_at,
            ft,
        );
        if let Some(c) = prov.provision_component() {
            components.push(c);
        }
        detail.push_str(&prov.warning_suffix());
    }

    CheckOutcome::Checked(CheckResult {
        basis,
        detail,
        components,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rc::tests::{ctx_beam, make_material, make_section, rc_beam_shape};
    use crate::DesignCheck;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    use sepika_core::units::ConcreteClass;

    fn make_material_class(fc: f64, grade: &str, class: ConcreteClass) -> Material {
        Material {
            concrete_class: class,
            ..make_material(fc, grade)
        }
    }

    fn checked_beam(
        forces: &MemberForcesAt,
        mat: &Material,
        ctx: &DesignCtx,
        shape: &SectionShape,
    ) -> CheckResult {
        let SectionShape::RcBeamRect { rebar, .. } = shape else {
            unreachable!()
        };
        beam_check(forces, mat, ctx, shape, rebar, 24.0).unwrap_checked()
    }

    #[test]
    fn test_beam_check_seismic_qd_increases_shear_ratio() {
        use crate::{QdMethod, SeismicQd};
        // 短期・地震時: QL=20kN、当該組合せ Q=60kN → QE=40kN、
        // QD2 = 20+1.5×40 = 80kN（QD1 は ΣMy が大きく効かないよう長スパン）。
        let mat = make_material(24.0, "SD345");
        let shape = rc_beam_shape(400.0, 700.0, 6, 22.0, 1, 40.0, 10.0, 100.0, 2);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 60_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 10.0e6,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Short,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            ..Default::default()
        };
        let base = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let ctx_qd = DesignCtx {
            term: LoadTerm::Short,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            seismic_qd: Some(SeismicQd {
                long_at: vec![(0.0, [0.0, 20_000.0, 0.0, 0.0, 0.0, 0.0])],
                n_factor: 1.5,
                n_mechanism: 1.0,
                q_simple: None,
                clear_length: 6000.0,
                method: QdMethod::Qd2,
            }),
            ..Default::default()
        };
        let with_qd = checked_beam(&forces, &mat, &ctx_qd, &shape);
        // QD=80kN > 解析値 60kN なのでせん断検定比が 4/3 倍になる
        // （曲げ・付着が支配しない前提の内力設定）。
        assert!(
            with_qd.ratio() > base.ratio(),
            "with_qd={} <= base={}",
            with_qd.ratio(),
            base.ratio()
        );
    }

    #[test]
    fn test_beam_moment_light_reinforcement_tension_governs() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let props = crate::rc::section_props::axis_props_from_shape(
            &shape,
            crate::ultimate::rc_props::RcDirection::Strong,
            false,
        )
        .unwrap();
        let ft = rebar_allowable_tension("SD345", 19.0, true);
        let fc = super::super::concrete_allowable_compression(24.0, true);
        let n_ratio = super::super::young_ratio_n(24.0);
        let bm = beam_moment_capacity(&props, ft, fc, n_ratio).unwrap();

        let expected_ma_t = props.at * ft * props.j;
        assert!((bm.ma_t - expected_ma_t).abs() < 1e-6);
        assert!(bm.ma_t <= bm.ma_c, "軽配筋では MA_t が支配するはず");
        assert!((bm.ma - bm.ma_t).abs() < 1e-6);
    }

    #[test]
    fn test_beam_moment_heavy_reinforcement_compression_governs() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let props = crate::rc::section_props::axis_props_from_shape(
            &shape,
            crate::ultimate::rc_props::RcDirection::Strong,
            false,
        )
        .unwrap();
        let ft = rebar_allowable_tension("SD345", 32.0, true);
        let fc = super::super::concrete_allowable_compression(24.0, true);
        let n_ratio = super::super::young_ratio_n(24.0);
        let bm = beam_moment_capacity(&props, ft, fc, n_ratio).unwrap();

        assert!(bm.ma_c < bm.ma_t, "過大配筋では MA_c が支配するはず");
        assert!((bm.ma - bm.ma_c).abs() < 1e-6);
    }

    #[test]
    fn test_beam_mid_with_slab_uses_ma_t_only() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let mat = make_material(24.0, "SD345");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 1_000.0, // 曲げ支配にするためせん断を小さく
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 50_000_000.0,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: false,
            ..Default::default()
        };
        let ctx_slab = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: true,
            ..Default::default()
        };
        let r0 = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let r1 = checked_beam(&forces, &mat, &ctx_slab, &shape);
        let bend0 = r0
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        let bend1 = r1
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        assert!(
            bend1 < bend0 - 1e-9,
            "中央+スラブは MA_t 採用で検定比が下がるはず: plain={bend0}, slab={bend1}"
        );
        assert!(r1.components[0].detail.contains("中央+スラブ"));
    }

    #[test]
    fn test_beam_check_trait_reports_ratio_basis_components_and_details() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(result.ratio() > 0.0);
        assert!(result.basis.contains("13条"));

        assert!(!result.components.is_empty());
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Bending));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Shear));

        let bending = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .expect("Bending component が存在するはず");
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Shear)
            .expect("Shear component が存在するはず");
        assert!(bending.detail.contains("MA_t="));
        assert!(!shear.detail.contains("MA_t="));
        assert!(shear.detail.contains("QA="));
        assert!(!bending.detail.contains("QA="));
        assert!(result.detail.contains("at="));
    }

    /// 軽量1種の RcDesign 検定は、普通コンクリートより検定比が大きくなる
    /// （許容せん断応力度 fs の 0.9 倍低減が `mat.concrete_class` 経由で効いている）。
    #[test]
    fn test_beam_check_lightweight_reduces_capacity() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat_n = make_material(24.0, "SD345");
        let mat_l = make_material_class(24.0, "SD345", ConcreteClass::Lightweight1);
        let ctx = ctx_beam(LoadTerm::Short);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 100_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let r_n = design.check(&forces, &sec, &mat_n, &ctx).unwrap_checked();
        let r_l = design.check(&forces, &sec, &mat_l, &ctx).unwrap_checked();
        assert!(
            r_l.ratio() > r_n.ratio(),
            "軽量1種は許容応力度低減により検定比が大きくなるはず: normal={}, light={}",
            r_n.ratio(),
            r_l.ratio()
        );
    }

    #[test]
    fn test_beam_check_provision_error_on_cover() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 20.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1_000_000.0,
        };
        let result = crate::rc::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Provision));
        assert!(!result.ok());
    }

    #[test]
    fn test_beam_mid_negative_with_slab_keeps_min_ma() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let mat = make_material(24.0, "SD345");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 1_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: -50_000_000.0,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: false,
            ..Default::default()
        };
        let ctx_slab = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: true,
            ..Default::default()
        };
        let r0 = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let r1 = checked_beam(&forces, &mat, &ctx_slab, &shape);
        let bend0 = r0
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        let bend1 = r1
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        assert!(
            (bend0 - bend1).abs() < 1e-12,
            "負曲げではスラブの有無で曲げ検定比が変わらないはず: {bend0} vs {bend1}"
        );
    }

    #[test]
    fn test_beam_check_trait_reports_ratio_basis_components_and_details_includes_bond_detail() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Short);
        ctx.length = 3000.0;
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(crate::full_detail(&result).contains("ld="));
        assert!(crate::full_detail(&result).contains("付着検定比"));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Bond));
    }

    #[test]
    fn test_beam_bond_1991_phi_uses_total_tension_count_for_new_model() {
        let shape = SectionShape::RcBeamRect {
            b: 400.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                top: vec![4, 2],
                bottom: vec![2, 1],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Short);
        ctx.bond_method = BondMethod::Rc1991;
        ctx.length = 3000.0;

        let bond_detail = |mz: f64| {
            let forces = MemberForcesAt {
                pos: 0.0,
                n: 0.0,
                qy: 20_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz,
            };
            let r = crate::rc::RcDesign
                .check(&forces, &sec, &mat, &ctx)
                .unwrap_checked();
            r.components
                .iter()
                .find(|c| c.kind == crate::CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone()
        };

        let dia = 22.0;
        let expected_top = format!("ψ={:.1}", 6.0 * std::f64::consts::PI * dia);
        let expected_bottom = format!("ψ={:.1}", 3.0 * std::f64::consts::PI * dia);
        assert!(
            bond_detail(-30_000_000.0).contains(&expected_top),
            "上端引張の φ は 6 本分: {}",
            bond_detail(-30_000_000.0)
        );
        assert!(
            bond_detail(30_000_000.0).contains(&expected_bottom),
            "下端引張の φ は 3 本分: {}",
            bond_detail(30_000_000.0)
        );
    }

    #[test]
    fn test_beam_check_bond_skipped_without_length_regression() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(crate::full_detail(&result).contains("省略"));
    }

    #[test]
    fn test_beam_bond_detail_uses_bending_sign_and_position_independently() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        for (pos, mz, expected) in [
            (0.0, -30.0e6, "端部・上端筋"),
            (0.5, -30.0e6, "中央・上端筋"),
            (0.0, 30.0e6, "端部・下端筋"),
            (0.5, 30.0e6, "中央・下端筋"),
        ] {
            let forces = MemberForcesAt {
                pos,
                n: 0.0,
                qy: 20_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz,
            };
            let result = crate::RcDesign
                .check(&forces, &sec, &mat, &ctx)
                .unwrap_checked();
            let detail = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone();
            assert!(detail.contains(expected), "{detail}");
        }
    }

    #[test]
    fn test_beam_bond_1991_corrected_allowable_rejects_excess_demand() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        // 4本D19、j=7/8×540.5 mmに対してτa=2.3125 N/mm²。
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 261125.3034629518,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30.0e6,
        };
        let result = crate::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        let bond = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Bond)
            .unwrap();
        assert!(
            (bond.ratio - 1.001082251082251).abs() < 1e-12,
            "{}",
            bond.ratio
        );
        assert!(bond.ratio > 1.0);
        assert!(bond.detail.contains("fa=2.310"));
    }

    #[test]
    fn test_beam_bond_selects_rebar_category_and_grade() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30.0e6,
        };
        let mut detail = |grade: &str| {
            ctx.rebar_material = Some(crate::rc::tests::make_rebar_material(grade, 235.0));
            crate::RcDesign
                .check(&forces, &sec, &make_material(24.0, grade), &ctx)
                .unwrap_checked()
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone()
        };
        assert!(detail("SD345").contains("fa=2.310"));
        assert!(detail("SR235").contains("fa=1.350"));
        assert!(detail("任意名称").contains("fa=2.310"));
    }
}
