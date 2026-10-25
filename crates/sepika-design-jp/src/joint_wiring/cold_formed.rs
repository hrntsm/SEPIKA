//! 冷間成形角形鋼管（BCR/BCP）柱の柱梁耐力比検定配線。

use super::common::{ForcesAt, MemberInfo};
use crate::steel::cold_formed::{
    box_zp, cold_formed_column_ratio_check, panel_mpp, ColdFormedInput,
};
use crate::CheckOutcome;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::section_shape::SectionShape;

/// 冷間成形角形鋼管（BCR/BCP）判定。
fn is_cold_formed(name: &str) -> bool {
    let upper = name.to_uppercase();
    upper.starts_with("BCR") || upper.starts_with("BCP")
}

/// H 形鋼の塑性断面係数（強軸）Zp = B·tf·(H−tf) + tw·(H−2tf)²/4。
fn h_zp(h: f64, b: f64, tw: f64, tf: f64) -> f64 {
    b * tf * (h - tf) + tw * (h - 2.0 * tf).powi(2) / 4.0
}

/// 冷間成形角形鋼管（BCR/BCP）柱の柱梁耐力比検定を `out` へ追加する。
pub(super) fn check_cold_formed(
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
    long_member_forces: Option<&[(ElemId, ForcesAt<'_>)]>,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    let cf_cols: Vec<&&MemberInfo> = cols
        .iter()
        .filter(|c| {
            is_cold_formed(&c.mat.name)
                && matches!(
                    c.sec.shape,
                    Some(SectionShape::SteelBox { .. }) | Some(SectionShape::CftBox { .. })
                )
        })
        .collect();
    if cf_cols.iter().any(|c| {
        c.sec
            .shape
            .as_ref()
            .is_some_and(SectionShape::has_unequal_box_thickness)
    }) {
        out.push((
            nid,
            "冷間成形耐力比".into(),
            CheckOutcome::Skipped {
                reason: "異厚BOXは一様厚を前提とする冷間成形耐力比の適用外です".into(),
            },
        ));
        return;
    }
    if !cf_cols.is_empty() {
        for beam in beams.iter().filter(|b| b.kind.is_steel_like()) {
            if let Some(SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            }) = beam.sec.shape
            {
                if let Some(reason) = sepika_core::material_grade::steel_f_value_issue(
                    &beam.mat.name,
                    web_thick.max(flange_thick),
                ) {
                    out.push((
                        nid,
                        "冷間成形耐力比".into(),
                        CheckOutcome::Skipped { reason },
                    ));
                    return;
                }
            }
        }
        let long_end_n = |c: &MemberInfo, nid: NodeId| -> Option<f64> {
            let list = long_member_forces?;
            let (_, forces) = list.iter().find(|(id, _)| *id == c.elem.id)?;
            let pos = if c.elem.nodes.first() == Some(&nid) {
                0.0
            } else if c.elem.nodes.get(1) == Some(&nid) {
                1.0
            } else {
                return None;
            };
            forces
                .iter()
                .min_by(|a, b| {
                    (a.0 - pos)
                        .abs()
                        .partial_cmp(&(b.0 - pos).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(_, f)| f[0])
        };
        let zp_f_n = |c: &MemberInfo| -> Option<(f64, f64, f64)> {
            let (h, b, t) = match c.sec.shape {
                Some(SectionShape::SteelBox {
                    height,
                    width,
                    thick,
                    ..
                })
                | Some(SectionShape::CftBox {
                    height,
                    width,
                    thick,
                    ..
                }) => (height, width, thick),
                _ => return None,
            };
            let f = crate::steel::steel_f_value_prefix(&c.mat.name, t)
                .or(c.mat.fy)
                .unwrap_or(295.0);
            let n = c
                .end_forces(nid)
                .map(|fr| {
                    let n_cur = -fr[0];
                    let n_exist = match long_end_n(c, nid) {
                        Some(nl_signed) => {
                            let nl = -nl_signed;
                            nl + 1.5 * (n_cur - nl)
                        }
                        None => n_cur,
                    };
                    n_exist.max(0.0) / (f * c.sec.area.max(1e-9))
                })
                .unwrap_or(0.0);
            Some((box_zp(h, b, t), f, n))
        };
        let upper = zp_f_n(cf_cols[0]);
        let lower = cf_cols.get(1).and_then(|c| zp_f_n(c)).or(upper);
        if let (Some((zp_u, f_u, n_u)), Some((zp_l, f_l, n_l))) = (upper, lower) {
            let sum_beam_mp: f64 = beams
                .iter()
                .filter(|b| b.kind.is_steel_like())
                .filter_map(|b| match b.sec.shape {
                    Some(SectionShape::SteelH {
                        height,
                        width,
                        web_thick,
                        flange_thick,
                        ..
                    }) => {
                        let fb = crate::steel::steel_f_value_prefix(
                            &b.mat.name,
                            flange_thick.max(web_thick),
                        )
                        .unwrap_or(235.0);
                        Some(fb * h_zp(height, width, web_thick, flange_thick))
                    }
                    _ => None,
                })
                .sum();
            if sum_beam_mp > 0.0 {
                let (dc, tp) = match cf_cols[0].sec.shape {
                    Some(SectionShape::SteelBox { height, thick, .. })
                    | Some(SectionShape::CftBox { height, thick, .. }) => (height - thick, thick),
                    _ => (0.0, 0.0),
                };
                let db = beams
                    .iter()
                    .map(|b| match b.sec.shape {
                        Some(SectionShape::SteelH { flange_thick, .. }) => {
                            b.sec.depth - flange_thick
                        }
                        _ => 0.9 * b.sec.depth,
                    })
                    .fold(0.0, f64::max);
                let n_panel = (n_u + n_l) / 2.0;
                let mpp = panel_mpp(dc, db, tp, f_l, n_panel);
                let inp = ColdFormedInput {
                    zp_col_upper: zp_u,
                    zp_col_lower: zp_l,
                    f_col_upper: f_u,
                    f_col_lower: f_l,
                    n_upper: n_u,
                    n_lower: n_l,
                    sum_beam_mp,
                    panel_mpp: mpp,
                };
                out.push((
                    nid,
                    "冷間成形耐力比".to_string(),
                    CheckOutcome::Checked(cold_formed_column_ratio_check(&inp)),
                ));
            }
        }
    }
}

#[cfg(test)]
mod sm520_tests {
    use super::*;
    use sepika_core::ids::SectionId;
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis,
    };
    #[test]
    fn out_of_range_h_beam_does_not_disappear_from_cold_formed_capacity_sum() {
        let make_elem = |id, nodes: [NodeId; 2]| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: nodes.into_iter().collect(),
            section: Some(SectionId(id)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        };
        let column = make_elem(0, [NodeId(0), NodeId(1)]);
        let beam = make_elem(1, [NodeId(1), NodeId(2)]);
        let good_beam = make_elem(2, [NodeId(1), NodeId(3)]);
        let mut csec = SectionShape::SteelBox {
            height: 1000.0,
            width: 1000.0,
            thick: 20.0,
            horizontal_thick: 20.0,
            corner_r: Some(0.0),
        }
        .to_section(SectionId(0), "BCR柱".into());
        csec.frame_use = Some(FrameSectionUse::Column);
        let bsec = crate::steel::test_support::h_section(1000.0, 400.0, 100.001, 80.0);
        let good_sec = crate::steel::test_support::h_section(1000.0, 400.0, 20.0, 20.0);
        let cmat = crate::steel::test_support::mat("BCR295");
        let good_mat = crate::steel::test_support::mat("SS400");
        let f = [(0.0, [0.0; 6]), (1.0, [0.0; 6])];
        for name in ["SM520", "SM520B", "SM520C"] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let mut bmat = crate::steel::test_support::mat(name);
                bmat.fy = fy;
                let make_info = |elem, sec, mat, member_kind| MemberInfo {
                    elem,
                    sec,
                    mat,
                    rebar_mat: None,
                    shear_mat: None,
                    steel_mat: None,
                    forces: &f,
                    kind: sepika_core::structure_kind::StructureKind::S,
                    member_kind,
                    length: 3000.0,
                };
                let col = make_info(&column, &csec, &cmat, crate::MemberKind::Column);
                let bad = make_info(&beam, &bsec, &bmat, crate::MemberKind::Girder);
                let good = make_info(&good_beam, &good_sec, &good_mat, crate::MemberKind::Girder);
                for beams in [vec![&bad], vec![&bad, &good]] {
                    let mut out = vec![];
                    check_cold_formed(&[&col], &beams, NodeId(1), None, &mut out);
                    assert_eq!(out.len(), 1);
                    assert!(
                        matches!(&out[0].2, CheckOutcome::Skipped { reason } if reason.contains("適用範囲"))
                    );
                }
            }
        }
    }
}
