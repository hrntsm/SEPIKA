//! 鉄骨造柱の断面検定（鋼構造設計規準の
//! 鉄骨造柱の許容応力度検定）。

use crate::material_strength::{steel_fc, steel_fs, steel_ft};
use crate::{
    CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm, MemberForcesAt,
    SteelFbBasis,
};
use sepika_core::model::{Material, PropertyBasis, Section};
use sepika_core::section_shape::{face_geometry::SectionGeometry, SectionShape};

use super::section::{
    steel_c_factor, steel_fb_h, steel_fb_h_asd2019, steel_i_t, steel_p_lambda_b,
    steel_warping_constant,
};
use super::{
    has_section_loss, nonzero, safe_denom, section_modulus, shape_of, shear_area_2d, ShapeCategory,
};

pub(super) fn asymmetric_built_h_shape(sec: &Section) -> Option<&SectionShape> {
    let Some(
        shape @ SectionShape::SteelBuiltH {
            upper_width,
            upper_thick,
            lower_width,
            lower_thick,
            ..
        },
    ) = sec.shape.as_ref()
    else {
        return None;
    };
    (upper_width != lower_width || upper_thick != lower_thick).then_some(shape)
}

pub(super) fn asymmetric_built_h_section_reason(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
) -> Option<&'static str> {
    if ![forces.n, forces.mz, forces.my, forces.qy, forces.qz]
        .iter()
        .all(|value| value.is_finite())
    {
        return Some("断面力が非有限です");
    }
    if has_section_loss(ctx) {
        return Some("断面欠損を控除した断面係数の適用条件が未確定です");
    }
    let basis = sec.property_basis;
    [basis.area, basis.iy, basis.iz]
        .iter()
        .any(|&property| property != PropertyBasis::Shape)
        .then_some("図心と断面性能の対応を確定するには、形状から求めた断面積と両軸の断面二次モーメントが必要です")
}

pub(super) fn asymmetric_built_h_combination_reasons(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
) -> impl Iterator<Item = &'static str> {
    let has_nonzero_length = [
        ctx.length,
        ctx.lk_y.unwrap_or(0.0),
        ctx.lk_z.unwrap_or(0.0),
        ctx.lb.unwrap_or(0.0),
    ]
    .iter()
    .any(|&length| length != 0.0)
        || ctx.steel_attr.as_ref().is_some_and(|attr| {
            let (i, mid, j) = attr.lb_direct.unwrap_or_default();
            [
                i,
                mid,
                j,
                attr.lk_y_direct.unwrap_or(0.0),
                attr.lk_z_direct.unwrap_or(0.0),
            ]
            .iter()
            .any(|&length| length != 0.0)
        });
    [
        has_nonzero_length.then_some("上下非対称断面の横座屈・曲げねじり座屈の算定条件が未確定です"),
        (forces.my != 0.0).then_some("弱軸曲げMyが同時に作用する場合の上下非対称断面の二軸曲げ照査は未算定です"),
        (forces.qy != 0.0).then_some("強軸せん断Qyが同時に作用する場合の上下非対称断面のウェブ内同一点の組合せ応力度は未算定です"),
    ]
    .into_iter()
    .flatten()
}

pub(super) fn check_asymmetric_built_h(
    forces: &MemberForcesAt,
    sec: &Section,
    shape: &SectionShape,
    ctx: &DesignCtx,
    f: f64,
    h_combined_reason: Option<&'static str>,
    built_h_shear_reason: Option<&'static str>,
) -> CheckOutcome {
    let check = || {
        if let Some(reason) = asymmetric_built_h_section_reason(forces, sec, ctx) {
            return Err(reason.into());
        }
        let [y_lower_mm, y_upper_mm] = SectionGeometry::of(shape)?.projected_range([1.0, 0.0])?;
        let area_mm2 = shape.try_calc_area()?;
        let inertia_mm4 = shape.try_calc_iy()?;
        let z_upper_mm3 = inertia_mm4 / y_upper_mm;
        let z_lower_mm3 = inertia_mm4 / -y_lower_mm;
        let ft_mpa = steel_ft(f, ctx.term);
        if ![area_mm2, z_upper_mm3, z_lower_mm3, ft_mpa]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return Err("全断面積・上下の断面係数・許容応力度を有限な正値で算定できません".into());
        }

        let sigma_c_mpa = -forces.n / area_mm2;
        let sigma_b_upper_mpa = -forces.mz / z_upper_mm3;
        let sigma_b_lower_mpa = forces.mz / z_lower_mm3;
        let (compression_side, tension_side, sigma_bc_mpa, sigma_bt_mpa) = if forces.mz > 0.0 {
            ("上", "下", -sigma_b_upper_mpa, sigma_b_lower_mpa)
        } else {
            ("下", "上", -sigma_b_lower_mpa, sigma_b_upper_mpa)
        };
        // 有限長に対称断面の fc・fb を流用すると適合を誤判定しうるため、各許容値の上限で不適合だけを確定する。
        let compression_ratio = sigma_c_mpa / ft_mpa + sigma_bc_mpa / ft_mpa;
        let tension_ratio = (sigma_bt_mpa - sigma_c_mpa) / ft_mpa;
        if !compression_ratio.is_finite() || !tension_ratio.is_finite() {
            return Err("上下縁の応力度比を有限値で算定できません".into());
        }
        let ratio = compression_ratio.max(tension_ratio);

        let unavailable: Vec<_> = asymmetric_built_h_combination_reasons(forces, ctx)
            .chain(h_combined_reason)
            .chain(built_h_shear_reason)
            .collect();
        if !unavailable.is_empty() && ratio <= 1.0 {
            return Ok(CheckOutcome::Skipped {
                reason: unavailable.join("。"),
            });
        }

        let term = match ctx.term {
            LoadTerm::Long => "長期",
            LoadTerm::Short => "短期",
        };
        let mut basis =
            format!("鋼構造許容応力度設計規準 {term} 上下非対称組立H形鋼柱 式(6.1)・(6.2)");
        let ratio_label = if unavailable.is_empty() {
            basis.push_str("（座屈長さ0の断面照査）");
            "軸曲げ比"
        } else {
            basis.push_str("（軸力・強軸曲げの照査による不適合）。");
            basis.push_str(&unavailable.join("。"));
            "軸曲げ比の下限"
        };
        Ok(CheckOutcome::Checked(CheckResult {
            basis,
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::AxialBending,
                ratio,
                detail: format!(
                    "A={area_mm2:.4} mm², Z上={z_upper_mm3:.4} mm³, Z下={z_lower_mm3:.4} mm³, \
                     σc={sigma_c_mpa:.4} N/mm², 曲げの圧縮側={compression_side}、引張側={tension_side}, \
                     σb上={sigma_b_upper_mpa:.4} N/mm², σb下={sigma_b_lower_mpa:.4} N/mm², \
                     ft={ft_mpa:.4} N/mm², {ratio_label}（圧縮側）={compression_ratio:.4}, \
                     {ratio_label}（引張側）={tension_ratio:.4}"
                ),
            }],
        }))
    };
    check().unwrap_or_else(|reason: String| {
        let mut reason = format!("上下非対称組立H形鋼柱は未算定です。{reason}");
        for shear_reason in [h_combined_reason, built_h_shear_reason]
            .into_iter()
            .flatten()
        {
            reason.push('。');
            reason.push_str(shear_reason);
        }
        CheckOutcome::Skipped { reason }
    })
}

/// 鉄骨造柱の断面検定（鋼構造設計規準）。
///
/// 組合せ応力を算定できない場合は、独立した軸曲げの不適合のみを検定比として返す。
pub(crate) fn check_column(
    forces: &MemberForcesAt,
    sec: &Section,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    lambda: f64,
    h_combined_reason: Option<&str>,
) -> Result<CheckOutcome, String> {
    let h = sec.depth;
    let (shape, tf, _) = shape_of(sec);
    let a = column_allowables(sec, mat, ctx, f, Some(lambda))?;
    let ColumnAllowables {
        area,
        z_strong,
        z_weak,
        ft: ft_val,
        fc: fc_val,
        fb_strong,
        fb_weak,
        fs: fs_val,
        shear_areas: [as_y, as_z],
        ..
    } = a;
    let sigma_bx = forces.mz.abs() / z_strong;
    let sigma_by = forces.my.abs() / z_weak;
    let buckling_note =
        if ctx.lk_y.unwrap_or(ctx.length) == 0.0 && ctx.lk_z.unwrap_or(ctx.length) == 0.0 {
            "（座屈長さ0のため座屈無視 λ=0）"
        } else {
            ""
        };

    let sigma_b_pipe = (forces.mz.powi(2) + forces.my.powi(2)).sqrt() / z_strong;

    let (axial_stress, ratio_axial_bend, axial_basis) = if forces.n < 0.0 {
        let sigma_c = forces.n.abs() / area;
        let ratio = match shape {
            ShapeCategory::Pipe => {
                sigma_c / safe_denom(fc_val) + sigma_b_pipe / safe_denom(fb_strong)
            }
            _ => {
                sigma_c / safe_denom(fc_val)
                    + sigma_bx / safe_denom(fb_strong)
                    + sigma_by / safe_denom(fb_weak)
            }
        };
        (sigma_c, ratio, "圧縮+曲げ: σc/fc(座屈考慮)+ΣσB/fb")
    } else {
        let sigma_t = forces.n / area;
        let ratio = match shape {
            ShapeCategory::Pipe => {
                sigma_t / safe_denom(ft_val) + sigma_b_pipe / safe_denom(fb_strong)
            }
            _ => {
                sigma_t / safe_denom(ft_val)
                    + sigma_bx / safe_denom(fb_strong)
                    + sigma_by / safe_denom(fb_weak)
            }
        };
        (sigma_t, ratio, "引張+曲げ: σt/ft+ΣσB/fb")
    };

    let term_label = match ctx.term {
        LoadTerm::Long => "長期",
        LoadTerm::Short => "短期",
    };
    let basis = format!("鋼構造設計規準 {term_label} 柱: {axial_basis}{buckling_note}");
    let axial_bending = CheckComponent {
        kind: CheckKind::AxialBending,
        ratio: ratio_axial_bend,
        detail: format!(
            "σax={:.4} N/mm², σbX={:.4} N/mm², σbY={:.4} N/mm², fc={:.4} N/mm², fbX={:.4} N/mm², \
fbY={:.4} N/mm², λ={:.3}, 軸曲げ比={:.4}",
            axial_stress, sigma_bx, sigma_by, fc_val, fb_strong, fb_weak, lambda, ratio_axial_bend,
        ),
    };
    if let Some(reason) = h_combined_reason {
        if ratio_axial_bend.is_finite() && ratio_axial_bend > 1.0 {
            return Ok(CheckOutcome::Checked(CheckResult {
                basis: format!("{basis}。{reason}"),
                detail: String::new(),
                components: vec![axial_bending],
            }));
        }
        return Ok(CheckOutcome::Skipped {
            reason: reason.into(),
        });
    }

    let (ay, az) = if matches!(
        sec.shape,
        Some(sepika_core::section_shape::SectionShape::SteelBox { .. })
    ) {
        (as_y, as_z)
    } else {
        (safe_denom(as_y), safe_denom(as_z))
    };
    let tau_y = forces.qy.abs() / ay;
    let tau_z = forces.qz.abs() / az;
    let tau = match shape {
        ShapeCategory::Pipe => (forces.qy.powi(2) + forces.qz.powi(2)).sqrt() / safe_denom(as_y),
        _ => tau_y + tau_z,
    };
    let sigma_total = match shape {
        ShapeCategory::H => axial_stress + sigma_bx * (h - 2.0 * tf).max(0.0) / safe_denom(h),
        ShapeCategory::Pipe => axial_stress + sigma_b_pipe,
        _ => axial_stress + sigma_bx + sigma_by,
    };
    let ratio_shear = ((sigma_total.powi(2) + 3.0 * tau.powi(2)).sqrt() / safe_denom(ft_val))
        .max(tau / safe_denom(fs_val));

    let shear_detail = if shape == ShapeCategory::Pipe {
        format!(
            "τ={:.4} N/mm², fs={:.4} N/mm², せん断比={:.4}",
            tau, fs_val, ratio_shear
        )
    } else {
        format!(
            "τy={:.4} N/mm², τz={:.4} N/mm², τ={:.4} N/mm², fs={:.4} N/mm², せん断比={:.4}",
            tau_y, tau_z, tau, fs_val, ratio_shear
        )
    };
    Ok(CheckOutcome::Checked(CheckResult {
        basis: format!("{basis}, せん断 von Mises"),
        detail: String::new(),
        components: vec![
            axial_bending,
            CheckComponent {
                kind: CheckKind::Shear,
                ratio: ratio_shear,
                detail: shear_detail,
            },
        ],
    }))
}

#[derive(Clone, Copy)]
pub(crate) struct ColumnAllowables {
    pub area: f64,
    pub z_strong: f64,
    pub z_weak: f64,
    pub ft: f64,
    pub fc: f64,
    pub fb_strong: f64,
    pub fb_weak: f64,
    pub fs: f64,
    pub shear_areas: [f64; 2],
    pub h_web_factor: f64,
}

pub(crate) fn column_allowables(
    sec: &Section,
    mat: &Material,
    ctx: &DesignCtx,
    f: f64,
    checked_slenderness: Option<f64>,
) -> Result<ColumnAllowables, String> {
    let term = ctx.term;
    let h = sec.depth;
    let b = sec.width;
    let area = nonzero(sec.area);
    let z_strong = nonzero(section_modulus(sec.iy, h / 2.0));
    let z_weak = nonzero(section_modulus(sec.iz, b / 2.0));

    let (shape, tf, tw) = shape_of(sec);

    let ft_val = steel_ft(f, term);
    let fs_val = steel_fs(f, term);

    let lambda = checked_slenderness.unwrap_or_else(|| {
        crate::effective_slenderness(sec.iy, sec.iz, area, ctx.length, ctx.lk_y, ctx.lk_z)
    });
    let fc_val = steel_fc(f, mat.young, lambda, term);

    let c = steel_c_factor(ctx, false);
    let fb_strong = match shape {
        ShapeCategory::H => match ctx.steel_fb_basis {
            SteelFbBasis::Standard1973 => {
                let af = b * tf;
                let i_t = steel_i_t(b, tf, h, tw);
                steel_fb_h(f, term, ctx.length, i_t, h, af, c)
            }
            SteelFbBasis::Asd2019 if ctx.length <= 1e-9 => ft_val,
            SteelFbBasis::Asd2019 => {
                let iz = sec.iz;
                let iw = steel_warping_constant(sec, tf);
                let j = sec.resolved_j()?;
                let e = mat.young;
                let g = mat.shear.unwrap_or(e / (2.0 * (1.0 + mat.poisson)));
                let p_lambda_b = steel_p_lambda_b(ctx, false);
                steel_fb_h_asd2019(
                    f, term, ctx.length, iz, iw, j, e, g, z_strong, c, p_lambda_b,
                )
            }
        },
        _ => ft_val,
    };
    let fb_weak = ft_val;

    let (ay, az) = shear_area_2d(shape, sec, tf, tw);
    Ok(ColumnAllowables {
        area,
        z_strong,
        z_weak,
        ft: ft_val,
        fc: fc_val,
        fb_strong,
        fb_weak,
        fs: fs_val,
        shear_areas: [ay, az],
        h_web_factor: if shape == ShapeCategory::H {
            (h - 2.0 * tf).max(0.0) / safe_denom(h)
        } else {
            1.0
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steel::test_support::{h_section, mat, rect_section};
    use crate::steel::SteelDesign;
    use crate::{DesignCheck, MemberKind};

    /// 断片が意図した component に配置されていることの確認
    /// （AxialBending の detail に "軸曲げ比" が含まれ、Shear の detail には
    /// 含まれない。逆に Shear 固有の "せん断比" は AxialBending に含まれない）。
    #[test]
    fn test_column_check_detail_fragments_assigned_to_intended_components() {
        let mut sec = rect_section(300.0, 300.0, "H-300x300x10x15");
        sec.thickness = Some(15.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -500_000.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 20e6,
            mz: 50e6,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3500.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let axial_bending = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::AxialBending)
            .expect("AxialBending component が存在するはず");
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Shear)
            .expect("Shear component が存在するはず");
        assert!(axial_bending.detail.contains("軸曲げ比"));
        assert!(!shear.detail.contains("軸曲げ比"));
        assert!(shear.detail.contains("せん断比"));
        assert!(!axial_bending.detail.contains("せん断比"));
    }

    #[test]
    fn test_column_check_pipe_combines_biaxial_sigma_b() {
        let mut sec = rect_section(300.0, 300.0, "PIPE-300x12");
        sec.iz = sec.iy;
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces_x_only = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30e6,
        };
        let forces_biaxial = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 30e6 / std::f64::consts::SQRT_2,
            mz: 30e6 / std::f64::consts::SQRT_2,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            ..Default::default()
        };
        let r1 = SteelDesign
            .check(&forces_x_only, &sec, &mat_v, &ctx)
            .unwrap_checked();
        let r2 = SteelDesign
            .check(&forces_biaxial, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(
            (r1.ratio() - r2.ratio()).abs() < 1e-6,
            "pipe combined sigma_b mismatch: {} vs {}",
            r1.ratio(),
            r2.ratio()
        );
    }

    #[test]
    fn test_column_shear_von_mises_hand_calc() {
        // 純せん断（N=0, M=0）で von Mises 式 sqrt(3)*τ/ft と τ/fs を手計算照合。
        let sec = sepika_core::section_shape::SectionShape::SteelBox {
            height: 300.0,
            width: 300.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        }
        .to_section(sepika_core::ids::SectionId(0), "BOX-300x300x12".into());
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 300_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let ay = 2.0 * 12.0 * (300.0 - 2.0 * 12.0);
        let tau = 300_000.0_f64.abs() / ay;
        let ft = steel_ft(235.0, LoadTerm::Long);
        let fs = steel_fs(235.0, LoadTerm::Long);
        // σ=0（純せん断）なので von Mises 側は sqrt(3)*τ/ft。
        let expected = (3.0_f64.sqrt() * tau / ft).max(tau / fs);
        assert!(
            (result.ratio() - expected).abs() < 1e-6,
            "ratio={} expected={}",
            result.ratio(),
            expected
        );
    }

    #[test]
    fn test_column_length_zero_ignores_buckling() {
        let mut sec = rect_section(300.0, 300.0, "一般断面");
        sec.thickness = Some(20.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 0.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();
        assert!(result.basis.contains("座屈無視"));
        // λ=0 なので fc=ft、単純圧縮比 = σc/ft と一致するはず。
        let ft = steel_ft(235.0, LoadTerm::Long);
        let expected = (100_000.0 / sec.area) / ft;
        assert!((result.ratio() - expected).abs() < 1e-6);
    }

    /// lk_y=lk_z=None（=length 共通）の場合、λ=max(lk_y/i_y, lk_z/i_z) が
    /// λ=length/i_min（i_min=√(min(Iy,Iz)/A)）と一致することを、
    /// fc（λ に応じて単調減少）を介して確認する。
    #[test]
    fn test_column_lambda_both_axes_none_matches_legacy_i_min() {
        let mut sec = rect_section(300.0, 200.0, "H-300x200x8x12");
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -100_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 4000.0,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let area = sec.area;
        let i_min = (sec.iy.min(sec.iz) / area).sqrt();
        let lambda = 4000.0 / i_min;
        let fc = steel_fc(235.0, mat_v.young, lambda, LoadTerm::Long);
        let expected = (100_000.0 / area) / fc;
        assert!(
            (result.ratio() - expected).abs() < 1e-9,
            "ratio={} expected(legacy i_min)={}",
            result.ratio(),
            expected
        );
    }

    /// lk_z（弱軸）のみを長くすると弱軸細長比が支配し fc が下がる（検定比が
    /// 上がる）ことを確認する。矩形断面 B=300 > D=100 とし、iz（弱軸=B方向）
    /// を十分小さくして弱軸が支配しやすい断面とする。
    #[test]
    fn test_column_lk_z_only_governs_weak_axis_slenderness() {
        let mut sec = rect_section(100.0, 300.0, "H-300x100x8x12");
        sec.thickness = Some(12.0);
        let mat_v = mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx_base = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(3000.0),
            ..Default::default()
        };
        let base = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx_base)
            .unwrap_checked();

        let ctx_long_z = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(3000.0),
            lk_z: Some(9000.0),
            ..Default::default()
        };
        let long_z = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx_long_z)
            .unwrap_checked();

        assert!(
            long_z.ratio() > base.ratio(),
            "lk_z を伸ばすと検定比が上がるはず: base={} long_z={}",
            base.ratio(),
            long_z.ratio()
        );
    }

    /// `Standard1973` を指定すると 1973 年版の式を用いる。
    #[test]
    fn test_column_check_standard1973_basis_matches_1973_formula() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3500.0,
            steel_fb_basis: SteelFbBasis::Standard1973,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let af = 200.0 * 13.0;
        let i_t = steel_i_t(200.0, 13.0, 400.0, 8.0);
        let c = steel_c_factor(&ctx, false);
        let fb_expected = steel_fb_h(f, LoadTerm::Long, 3500.0, i_t, 400.0, af, c);
        assert!(
            crate::full_detail(&result).contains(&format!("fbX={:.4}", fb_expected)),
            "detail={}",
            crate::full_detail(&result)
        );
    }

    /// AIJ-ASD19 fb（`SteelFbBasis::Asd2019`）: H形柱の fbX がその式の
    /// 手計算と一致することを確認する（柱の階高 `ctx.length` を lb として
    /// 用いる）。
    #[test]
    fn test_column_check_fb_basis_asd2019_matches_hand_calc() {
        let sec = h_section(400.0, 200.0, 8.0, 13.0);
        let mat_v = mat("SN400B");
        let forces = MemberForcesAt {
            pos: 0.0,
            n: -50_000.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5e7,
        };
        let ctx = DesignCtx {
            term: LoadTerm::Long,
            kind: MemberKind::Column,
            length: 3500.0,
            steel_fb_basis: SteelFbBasis::Asd2019,
            ..Default::default()
        };
        let result = SteelDesign
            .check(&forces, &sec, &mat_v, &ctx)
            .unwrap_checked();

        let f = 235.0;
        let z_strong = sec.iy / (sec.depth / 2.0);
        let iw = steel_warping_constant(&sec, 13.0);
        let e = mat_v.young;
        let g = e / (2.0 * (1.0 + mat_v.poisson));
        let p_lambda_b = steel_p_lambda_b(&ctx, false);
        let c = steel_c_factor(&ctx, false);
        let fb_expected = steel_fb_h_asd2019(
            f,
            LoadTerm::Long,
            3500.0,
            sec.iz,
            iw,
            sec.j,
            e,
            g,
            z_strong,
            c,
            p_lambda_b,
        );
        assert!(
            crate::full_detail(&result).contains(&format!("fbX={:.4}", fb_expected)),
            "detail={}",
            crate::full_detail(&result)
        );
    }
}
