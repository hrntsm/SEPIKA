use crate::{DesignCtx, MemberForcesAt, MemberKind};
use sepika_core::model::Section;
use sepika_core::section_shape::SectionShape;

fn supplied_mm(value: Option<f64>) -> String {
    match value {
        None => "不明（未指定）".into(),
        Some(v) if v.is_finite() && v > 0.0 => format!("{v} mm"),
        Some(v) => format!("不正/未確定（{v} mm）"),
    }
}

pub(super) fn unsupported_reason(
    forces: &MemberForcesAt,
    sec: &Section,
    ctx: &DesignCtx,
) -> String {
    let Some(SectionShape::SteelLipChannel {
        height,
        width,
        lip,
        thick,
    }) = sec.shape.as_ref()
    else {
        unreachable!("リップ溝形鋼の形状を確認済み")
    };
    let kind = match ctx.kind {
        MemberKind::Girder => "梁",
        MemberKind::Column => "柱",
        MemberKind::Brace => "ブレース",
    };
    let geometry = if [*height, *width, *lip, *thick]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
        && *height > 2.0 * *thick
        && *width > *thick
        && *lip > *thick
        && *height > *lip + *thick
        && *lip <= *height / 2.0
    {
        "寸法の耐力式適用は未確定"
    } else {
        "寸法不正（非有限/非正または寸法関係不成立）"
    };
    let load = if [forces.n, forces.qy, forces.qz, forces.my, forces.mz]
        .iter()
        .all(|v| v.is_finite())
    {
        "荷重符号を保持"
    } else {
        "荷重不正（非有限）"
    };
    let axial = if !forces.n.is_finite() {
        "軸力不正（非有限）"
    } else if forces.n < 0.0 {
        "圧縮"
    } else if forces.n > 0.0 {
        "引張"
    } else {
        "軸力0"
    };
    let braces = ctx
        .steel_attr
        .as_ref()
        .and_then(|attr| attr.lateral_brace_count)
        .map_or_else(|| "不明（未指定）".into(), |n| format!("{n}本"));
    let lb_direct = ctx
        .steel_attr
        .as_ref()
        .and_then(|attr| attr.lb_direct)
        .map_or_else(
            || "不明（未指定）".into(),
            |(i, m, j)| {
                format!(
                    "始端={}、中央={}、終端={}",
                    supplied_mm(Some(i)),
                    supplied_mm(Some(m)),
                    supplied_mm(Some(j))
                )
            },
        );
    let term = match ctx.term {
        crate::LoadTerm::Long => "長期",
        crate::LoadTerm::Short => "短期",
    };
    format!(
        "リップ溝形鋼: 断面 {}、H={height} mm、B={width} mm、C={lip} mm、t={thick} mm、{geometry}。部材検定未対応（引張・無荷重・せん断のみも未検定）。\
         [局部座屈] 接合線の移動を伴わない板要素変形: 算定未対応。\
         [ゆがみ座屈] フランジ–リップ接合線の移動等の断面形状変化: 算定未対応。\
         [全体座屈] 断面形状を保持する曲げ・ねじり・曲げねじり、梁の横座屈: 算定未対応。\
         分類は検討対象であり支配モード・合否・座屈の不適用を判定しない。連成は未評価。\
         [有効断面] 有効A[mm²]・有効Z[mm³]は未算定（総断面による代替なし）。\
         [検討条件] {kind}・{term}、{load}、{}、N={} N、Qy={} N、Qz={} N、My={} N·mm、Mz={} N·mm。\
         曲げ符号は入力値を保持（強軸Mz/Qy、弱軸My/Qz）。\
         材長={}、lk_y={}、lk_z={}、lb={}、lb直接入力={lb_direct}、等間隔横補剛={}。\
         [適用条件未確定] 平板部幅・R・リップ剛性、補剛剛性・端部/ねじり/反り拘束、\
         荷重作用点/せん断中心、国内耐力式・相関式の適用条件を確認できない。",
        sec.name,
        axial,
        forces.n,
        forces.qy,
        forces.qz,
        forces.my,
        forces.mz,
        supplied_mm(Some(ctx.length)),
        supplied_mm(ctx.lk_y),
        supplied_mm(ctx.lk_z),
        supplied_mm(ctx.lb),
        braces,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CheckOutcome, DesignCheck, MemberKind, SteelDesign};
    use sepika_core::{ids::SectionId, model::SteelDesignAttr, section_shape::SectionShape};

    fn section(width: f64, lip: f64) -> Section {
        SectionShape::SteelLipChannel {
            height: 150.0,
            width,
            lip,
            thick: 2.3,
        }
        .to_section(SectionId(0), "LipC".into())
    }

    #[test]
    fn all_member_kinds_load_signs_and_bracing_remain_unchecked() {
        let sec = section(75.0, 20.0);
        let gross = (sec.shape.clone(), sec.area, sec.iy, sec.iz, sec.j);
        for kind in [MemberKind::Column, MemberKind::Girder, MemberKind::Brace] {
            for [n, qy, qz, my, mz] in [
                [-100_000.0, 0.0, 0.0, 0.0, 0.0],
                [100_000.0, 0.0, 0.0, 0.0, 0.0],
                [0.0; 5],
                [0.0, 10_000.0, -10_000.0, 0.0, 0.0],
                [0.0, 0.0, 0.0, 0.0, 1_000_000.0],
                [0.0, 0.0, 0.0, 0.0, -1_000_000.0],
                [0.0, 0.0, 0.0, 1_000_000.0, 0.0],
                [0.0, 0.0, 0.0, -1_000_000.0, 0.0],
                [-100_000.0, 10_000.0, -20_000.0, -1_000_000.0, 2_000_000.0],
            ] {
                for count in [None, Some(0), Some(2)] {
                    let ctx = DesignCtx {
                        kind,
                        length: 3000.0,
                        lk_y: count.map(|_| 2500.0),
                        lk_z: count.map(|_| 2000.0),
                        lb: count.map(|_| 1000.0),
                        steel_attr: Some(SteelDesignAttr {
                            lateral_brace_count: count,
                            elem: sepika_core::ids::ElemId(0),
                            joint_flange_loss: 0.0,
                            joint_web_loss: 0.0,
                            scallop_web_loss: 0.0,
                            lb_direct: count.map(|_| (900.0, 1000.0, 1100.0)),
                            lk_y_direct: None,
                            lk_z_direct: None,
                            c_direct: None,
                        }),
                        ..Default::default()
                    };
                    let forces = MemberForcesAt {
                        pos: 0.5,
                        n,
                        qy,
                        qz,
                        my,
                        mz,
                    };
                    let CheckOutcome::Skipped { reason } = SteelDesign.check(
                        &forces,
                        &sec,
                        &super::super::test_support::mat("SN400"),
                        &ctx,
                    ) else {
                        panic!("リップ材は荷重・補剛によらず未検定")
                    };
                    for item in [
                        "リップ溝形鋼",
                        "[局部座屈]",
                        "[ゆがみ座屈]",
                        "[全体座屈]",
                        "曲げ・ねじり・曲げねじり",
                        "横座屈",
                        "有効A[mm²]・有効Z[mm³]は未算定",
                        "[適用条件未確定]",
                    ] {
                        assert!(reason.contains(item), "{reason}");
                    }
                    assert!(reason.contains(&format!("My={my} N·mm")));
                    assert!(reason.contains(&format!("Mz={mz} N·mm")));
                    if let Some(count) = count {
                        assert!(reason.contains(&format!("等間隔横補剛={count}本")));
                        assert!(reason.contains("lk_y=2500 mm"));
                        assert!(
                            reason.contains("lb直接入力=始端=900 mm、中央=1000 mm、終端=1100 mm")
                        );
                    } else {
                        assert!(reason.contains("lk_y=不明（未指定）"));
                        assert!(reason.contains("等間隔横補剛=不明（未指定）"));
                    }
                    assert_eq!(gross, (sec.shape.clone(), sec.area, sec.iy, sec.iz, sec.j));
                }
            }
        }
    }

    #[test]
    fn equal_gross_area_does_not_erase_different_plate_and_lip_dimensions() {
        let a = section(75.0, 20.0);
        let b = section(65.0, 30.0);
        assert!((a.area - b.area).abs() < 1e-9);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: -1.0,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx::default();
        let ra = unsupported_reason(&forces, &a, &ctx);
        let rb = unsupported_reason(&forces, &b, &ctx);
        assert_ne!(ra, rb);
        assert!(ra.contains("B=75 mm、C=20 mm"));
        assert!(rb.contains("B=65 mm、C=30 mm"));
    }

    #[test]
    fn invalid_lengths_and_bending_forces_are_visible_without_fallback() {
        let sec = section(75.0, 20.0);
        let mat = super::super::test_support::mat("SN400");
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let ctx = DesignCtx {
                length: invalid,
                lk_y: Some(invalid),
                lk_z: Some(invalid),
                lb: Some(invalid),
                ..Default::default()
            };
            let forces = MemberForcesAt {
                pos: 0.5,
                n: 0.0,
                qy: 0.0,
                qz: 0.0,
                my: f64::NAN,
                mz: f64::INFINITY,
            };
            let CheckOutcome::Skipped { reason } = SteelDesign.check(&forces, &sec, &mat, &ctx)
            else {
                panic!("不正条件で検定済み")
            };
            for item in [
                "材長=不正/未確定",
                "lk_y=不正/未確定",
                "lk_z=不正/未確定",
                "lb=不正/未確定",
                "荷重不正（非有限）",
                "My=NaN",
                "Mz=inf",
            ] {
                assert!(reason.contains(item), "{reason}");
            }
        }
    }

    #[test]
    fn nonfinite_force_is_not_described_as_zero_or_tension() {
        let forces = MemberForcesAt {
            pos: 0.5,
            n: f64::NAN,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let reason = unsupported_reason(&forces, &section(75.0, 20.0), &DesignCtx::default());
        assert!(reason.contains("軸力不正（非有限）"));
        assert!(reason.contains("N=NaN N"));
    }
}
