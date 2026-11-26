use std::collections::BTreeMap;

use sepika_core::model::{ElementData, ElementKind, Material, Model, Section, SteelDesignAttr};
use sepika_core::structure_kind::{structure_kind_of, StructureKind};
use sepika_design_jp::steel::{
    column_lateral_buckling_length, is_symmetric_h_section, SteelDesign,
};
use sepika_design_jp::{DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_solver::dynamic::timehistory::{
    ColumnBendingMaterialFailure, ColumnBendingPeak, CommittedBendingResponse,
    SymmetricColumnBendingHistory,
};

pub(crate) struct Column<'a> {
    pub index: usize,
    length_mm: f64,
    section: &'a Section,
    material: Option<&'a Material>,
    ctx: DesignCtx,
}

pub(crate) fn targets(model: &Model, elem: &ElementData) -> bool {
    matches!(
        elem.kind,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
    ) && MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
        && model.element_section(elem).is_some_and(|sec| {
            is_symmetric_h_section(sec)
                && structure_kind_of(
                    Some(sec),
                    model.element_material(elem).map(|mat| mat.category),
                ) == StructureKind::S
        })
}

fn confirmed_span_length(
    model: &Model,
    elem: &ElementData,
    attr: Option<&SteelDesignAttr>,
) -> Result<f64, &'static str> {
    let length_mm = column_lateral_buckling_length(model, elem, attr)?;
    if attr.is_none_or(|a| a.column_lateral_buckling_end_supports.is_none()) {
        return Err("両実材端の支持条件が未確認です。節点間距離から補剛区間を特定できません");
    }
    Ok(length_mm)
}

pub(crate) fn columns<'a>(
    model: &'a Model,
    histories: &mut BTreeMap<usize, SymmetricColumnBendingHistory>,
) -> Vec<Column<'a>> {
    model
        .elements
        .iter()
        .enumerate()
        .filter(|(_, elem)| targets(model, elem))
        .filter_map(|(index, elem)| {
            let section = model.element_section(elem)?;
            let material = model.element_material(elem);
            let steel_attr = model.steel_design_attrs.iter().find(|a| a.elem == elem.id);
            let item = histories.entry(index).or_default();
            match confirmed_span_length(model, elem, steel_attr) {
                Ok(lb_mm) => item.lb_mm = Some(lb_mm),
                Err(reason) => {
                    item.unavailable.insert(reason.into());
                }
            }
            if material.is_none() {
                item.unavailable
                    .insert("断面に材料が割り当てられていません".into());
            }
            Some(Column {
                index,
                length_mm: model.member_length(elem),
                section,
                material,
                ctx: DesignCtx {
                    term: LoadTerm::Short,
                    kind: MemberKind::Column,
                    steel_attr: steel_attr.cloned(),
                    ..Default::default()
                },
            })
        })
        .collect()
}

pub(crate) fn record(
    item: &mut SymmetricColumnBendingHistory,
    column: &Column<'_>,
    step: u64,
    time_s: f64,
    bending: &CommittedBendingResponse<'_>,
) {
    item.committed_count += 1;
    let Some(material) = column.material else {
        return;
    };
    let span = match bending(column.index, item.lb_mm.unwrap_or(column.length_mm)) {
        Ok(span) => span,
        Err(reason) => {
            item.unavailable.insert(reason);
            return;
        }
    };
    let material_excess = span
        .demand_peaks
        .iter()
        .max_by(|a, b| a.left[5].abs().total_cmp(&b.left[5].abs()))
        .and_then(|demand| {
            SteelDesign
                .time_history_column_bending_material_failure(
                    demand.left[5],
                    column.section,
                    material,
                    &column.ctx,
                )
                .map(|ratio| (demand, ratio))
        });
    if let Some((demand, ratio)) = material_excess.filter(|&(_, ratio)| {
        item.material_failure
            .as_ref()
            .is_none_or(|prior| ratio > prior.ratio)
    }) {
        item.material_failure = Some(ColumnBendingMaterialFailure {
            step,
            time_s,
            x_mm: demand.x_mm,
            mz_nmm: demand.left[5],
            ratio,
        });
    }
    let Some(lb_mm) = item.lb_mm else {
        return;
    };
    let mut complete = !span.demand_peaks.is_empty();
    if !complete {
        item.unavailable
            .insert("強軸曲げの区間最大を取得できません".into());
    }
    for peak in &span.demand_peaks {
        if peak.left[5] == 0.0 {
            continue;
        }
        let left = (!peak.is_i_face || matches!(peak.right, Ok(None))).then_some(peak.left);
        let right = match &peak.right {
            Ok(right) => *right,
            Err(reason) => {
                item.unavailable.insert(reason.clone());
                complete = false;
                None
            }
        };
        for force in [left, right].into_iter().flatten() {
            let result = SteelDesign.time_history_column_strong_bending(
                column.section,
                material,
                &column.ctx,
                lb_mm,
                &span,
                &MemberForcesAt::from_local(peak.xi, force),
            );
            match result {
                Ok(check) => {
                    if item
                        .peak
                        .as_ref()
                        .is_none_or(|prior| check.ratio > prior.ratio)
                    {
                        item.peak = Some(ColumnBendingPeak {
                            step,
                            time_s,
                            max_x_mm: peak.x_mm,
                            max_mz_nmm: force[5],
                            end_moments_nmm: span.end_moments_nmm,
                            monotonic: !span.interior_maximum,
                            c: check.c,
                            p_lambda_b: check.p_lambda_b,
                            fb_mpa: check.fb_mpa,
                            ratio: check.ratio,
                        });
                    }
                }
                Err(reason) => {
                    item.unavailable.insert(reason);
                    complete = false;
                }
            }
        }
    }
    item.evaluated_count += usize::from(complete);
}

pub(crate) fn valid_prefix(
    items: &BTreeMap<usize, SymmetricColumnBendingHistory>,
    times: &[f64],
    model: &Model,
) -> bool {
    items.iter().all(|(&index, item)| {
        let Some(elem) = model
            .elements
            .get(index)
            .filter(|elem| targets(model, elem))
        else {
            return false;
        };
        let attr = model
            .steel_design_attrs
            .iter()
            .find(|attr| attr.elem == elem.id);
        let lb = confirmed_span_length(model, elem, attr).ok();
        let valid_demand = |step, time_s, x_mm, mz_nmm: f64| {
            let length_mm = model.member_length(elem);
            (0.0..=length_mm + 32.0 * f64::EPSILON * length_mm).contains(&x_mm)
                && usize::try_from(step).ok().and_then(|i| times.get(i)) == Some(&time_s)
                && mz_nmm.is_finite()
                && mz_nmm != 0.0
        };
        item.lb_mm == lb
            && item.committed_count == times.len()
            && item.evaluated_count <= item.committed_count
            && (item.evaluated_count == item.committed_count) == item.unavailable.is_empty()
            && item.material_failure.as_ref().is_none_or(|failure| {
                valid_demand(failure.step, failure.time_s, failure.x_mm, failure.mz_nmm)
                    && failure.ratio.is_finite()
                    && failure.ratio > 1.0
            })
            && item.peak.as_ref().is_none_or(|peak| {
                lb.is_some()
                    && valid_demand(peak.step, peak.time_s, peak.max_x_mm, peak.max_mz_nmm)
                    && peak.end_moments_nmm.iter().all(|moment| moment.is_finite())
                    && (1.0..=2.3).contains(&peak.c)
                    && (0.3..=0.9 + f64::EPSILON).contains(&peak.p_lambda_b)
                    && peak.fb_mpa.is_finite()
                    && peak.fb_mpa > 0.0
                    && peak.ratio.is_finite()
                    && peak.ratio > 0.0
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time_history_check::collect_time_history_checks;
    use sepika_core::model::{MemberLoad, MemberLoadKind};
    use sepika_element::frame::beam::MemberForces;
    use sepika_element::frame::member_load::frame_strong_bending_span;
    use sepika_element::transform::LocalFrame;

    use crate::time_history_check::asymmetric_column_model::steel_column as combined_column;

    fn model() -> Model {
        let mut model = combined_column::cantilever(0.0, 0.0, 0.0);
        model.steel_design_attrs.push(SteelDesignAttr {
            elem: model.elements[0].id,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            column_lateral_buckling_end_supports: model.column_end_supports(&model.elements[0]),
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: Some(2.3),
        });
        model
    }

    fn collect(
        model: &Model,
        steps: usize,
        bending: impl Fn(
            usize,
            usize,
            f64,
        )
            -> Result<sepika_element::frame::member_load::FrameStrongBendingSpan, String>,
    ) -> SymmetricColumnBendingHistory {
        let (_, mut checks) =
            collect_time_history_checks(model, Default::default(), 0, |observer| {
                let observer = observer.unwrap();
                for step in 0..steps {
                    observer(step as u64, step as f64 * 0.1, &[], &|i, lb| {
                        bending(step, i, lb)
                    });
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        checks.symmetric_columns.remove(&0).unwrap()
    }

    fn frame() -> LocalFrame {
        LocalFrame::from_nodes([0.0, 0.0, 0.0], [0.0, 0.0, 3000.0], [1.0, 0.0, 0.0]).unwrap()
    }

    fn m_and_q(moment_nmm: f64, shear_n: f64) -> [f64; 6] {
        [0.0, shear_n, 0.0, 0.0, 0.0, moment_nmm]
    }

    #[test]
    fn opposite_ends_from_different_steps_do_not_hide_strong_bending_failure() {
        let model = model();
        let frame = frame();
        let samples = [[259e6, 0.0], [0.0, -259e6], [260e6, -260e6]];
        let history = collect(&model, 4, |step, _, lb| {
            let Some(&[i, j]) = samples.get(step) else {
                return Err("同じ確定時刻の荷重込み部材応答がありません".into());
            };
            let q = -86_333.333_333_333_33;
            let q = if step == 2 {
                -173_333.333_333_333_34
            } else {
                q
            };
            let forces = MemberForces {
                at: vec![(0.0, m_and_q(i, q)), (1.0, m_and_q(j, q))],
            };
            frame_strong_bending_span(&frame, lb, [0.0, lb], &[], &forces)
        });
        let peak = history.peak.unwrap();
        assert_eq!(history.lb_mm, Some(3000.0));
        assert_eq!((history.committed_count, history.evaluated_count), (4, 3));
        assert!(history
            .unavailable
            .iter()
            .any(|reason| reason.contains("部材応答がありません")));
        assert!((peak.c - 1.75).abs() < 1e-10, "{peak:?}");
        assert!((peak.p_lambda_b - 0.6).abs() < 1e-10, "{peak:?}");
        assert!((peak.fb_mpa - 223.833_12).abs() < 1e-5, "{peak:?}");
        assert!((peak.ratio - 1.007_72).abs() < 1e-5, "{peak:?}");
        assert!(
            peak.step < 2 && peak.ratio > 1.0 && peak.monotonic,
            "{peak:?}"
        );
        assert_eq!(peak.end_moments_nmm, samples[peak.step as usize]);
    }

    #[test]
    fn off_center_and_plateau_maxima_are_not_treated_as_end_moment_gradients() {
        let model = model();
        let frame = frame();
        for (load_n, end_moment_nmm, end_shear_n) in [(-300e3, 100e6, -100e3), (-200e3, 300e6, 0.0)]
        {
            let point = MemberLoad::manual(
                model.elements[0].id,
                [1.0, 0.0, 0.0],
                MemberLoadKind::Point {
                    a: 1000.0,
                    p: load_n,
                },
            );
            let response = MemberForces {
                at: vec![
                    (0.0, m_and_q(100e6, 200e3)),
                    (1.0, m_and_q(end_moment_nmm, end_shear_n)),
                ],
            };
            let history = collect(&model, 1, |_, _, lb| {
                frame_strong_bending_span(&frame, lb, [0.0, lb], &[(1.0, &point)], &response)
            });
            assert!(history.unavailable.is_empty(), "{history:?}");
            let peak = history.peak.unwrap();
            assert!(!peak.monotonic, "{peak:?}");
            assert_eq!((peak.c, peak.p_lambda_b), (1.0, 0.3));
            assert_eq!((peak.max_x_mm, peak.max_mz_nmm), (1000.0, 300e6));
        }
    }

    #[test]
    fn weak_axis_load_inside_a_flat_maximum_cannot_be_hidden_by_zero_end_moments() {
        let model = model();
        let loads = MemberLoad::manual(
            model.elements[0].id,
            [0.0, 1.0, 0.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: -1.0,
                w2: -1.0,
            },
        );
        for weak_load in [false, true] {
            let qz = if weak_load { 1500.0 } else { 0.0 };
            let force = MemberForces {
                at: vec![
                    (0.0, [0.0, 0.0, qz, 0.0, 0.0, 200e6]),
                    (1.0, [0.0, 0.0, -qz, 0.0, 0.0, 200e6]),
                ],
            };
            let terms = [(1.0, &loads)];
            let history = collect(&model, 1, |_, _, lb| {
                let applied = if weak_load { terms.as_slice() } else { &[] };
                frame_strong_bending_span(&frame(), lb, [0.0, lb], applied, &force)
            });
            assert!(history.material_failure.is_none(), "{history:?}");
            if weak_load {
                assert_eq!(history.evaluated_count, 0);
                assert!(history.peak.is_none(), "{history:?}");
                assert!(
                    history
                        .unavailable
                        .iter()
                        .any(|reason| reason.contains("最大となる平坦区間")),
                    "{history:?}"
                );
            } else {
                assert_eq!(history.evaluated_count, 1);
                let peak = history.peak.unwrap();
                assert_eq!((peak.c, peak.p_lambda_b), (1.0, 0.3));
                assert!(peak.ratio < 1.0, "{peak:?}");
            }
        }
    }

    #[test]
    fn internal_bending_excess_survives_unavailable_lateral_buckling() {
        let mut model = model();
        let load = MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: -140.0,
                w2: -140.0,
            },
        );
        let force = MemberForces {
            at: vec![
                (0.0, [0.0, 140e3, 0.0, 0.0, 1e6, 210e6]),
                (0.5, [0.0, -70e3, 0.0, 0.0, 1e6, 262.5e6]),
                (1.0, [0.0, -280e3, 0.0, 0.0, 1e6, 0.0]),
            ],
        };
        for reason in ["弱軸曲げと強軸曲げ", "両実材端の支持条件が未確認"] {
            let history = collect(&model, 1, |_, _, length| {
                frame_strong_bending_span(&frame(), length, [0.0, length], &[(1.0, &load)], &force)
            });
            assert_eq!(history.evaluated_count, 0);
            assert!(history.peak.is_none(), "{history:?}");
            assert!(
                history.unavailable.iter().any(|item| item.contains(reason)),
                "{history:?}"
            );
            let failure = history.material_failure.unwrap();
            assert!((failure.x_mm - 1000.0).abs() < 1e-9, "{failure:?}");
            assert!((failure.mz_nmm - 280e6).abs() < 1e-6, "{failure:?}");
            assert!((failure.ratio - 1.03766).abs() < 5e-6, "{failure:?}");
            model.steel_design_attrs[0].column_lateral_buckling_end_supports = None;
        }
    }

    #[test]
    fn verified_eccentric_supports_use_the_actual_length() {
        let mut model = model();
        model.elements[0].end_offsets_mm[1][2] = 500.0;
        model.steel_design_attrs[0].column_lateral_buckling_end_supports =
            model.column_end_supports(&model.elements[0]);
        let frame = LocalFrame::from_nodes([0.0; 3], [0.0, 0.0, 3500.0], [1.0, 0.0, 0.0]).unwrap();
        let response = MemberForces {
            at: vec![(0.0, m_and_q(215e6, 0.0)), (1.0, m_and_q(215e6, 0.0))],
        };
        let history = collect(&model, 1, |_, _, lb| {
            frame_strong_bending_span(&frame, lb, [0.0, lb], &[], &response)
        });
        assert_eq!(history.lb_mm, Some(3500.0));
        assert!(history.unavailable.is_empty(), "{history:?}");
        let peak = history.peak.unwrap();
        assert_eq!((peak.c, peak.p_lambda_b), (1.0, 0.3));
        assert!((peak.ratio - 1.039).abs() < 0.0005, "{peak:?}");
    }

    #[test]
    fn support_length_alone_cannot_establish_a_bending_interval() {
        let mut model = model();
        for (confirm, count, move_after_confirmation, reason) in [
            (false, None, false, "両実材端の支持条件が未確認"),
            (true, Some(1), false, "中間横補剛"),
            (true, None, true, "条件が確認時から変わりました"),
        ] {
            model.elements[0].end_offsets_mm[1][2] = 500.0;
            model.steel_design_attrs[0].column_lateral_buckling_end_supports =
                confirm.then(|| model.column_end_supports(&model.elements[0]).unwrap());
            model.steel_design_attrs[0].lateral_brace_count = count;
            if move_after_confirmation {
                model.elements[0].end_offsets_mm[1][2] = 0.0;
            }
            let zero = MemberForces {
                at: vec![(0.0, [0.0; 6]), (1.0, [0.0; 6])],
            };
            let history = collect(&model, 1, |_, _, length| {
                frame_strong_bending_span(&frame(), length, [0.0, length], &[], &zero)
            });
            assert!(history.lb_mm.is_none() && history.peak.is_none());
            assert!(
                history.unavailable.iter().any(|item| item.contains(reason)),
                "{history:?}"
            );
        }
    }

    #[test]
    fn no_bending_requires_a_verified_zero_diagram_and_other_nonmonotonic_cases_remain_unknown() {
        let model = model();
        let frame = frame();
        let load = MemberLoad::manual(
            model.elements[0].id,
            [1.0, 0.0, 0.0],
            MemberLoadKind::Point { a: 900.0, p: 100e3 },
        );
        let unclassified = MemberForces {
            at: vec![(0.0, m_and_q(0.0, -20e3)), (1.0, m_and_q(150e6, 80e3))],
        };
        let zero = MemberForces {
            at: vec![(0.0, [0.0; 6]), (1.0, [0.0; 6])],
        };
        let history = collect(&model, 3, |step, _, lb| match step {
            0 => frame_strong_bending_span(&frame, lb, [0.0, lb], &[], &zero),
            1 => frame_strong_bending_span(&frame, lb, [0.0, lb], &[(1.0, &load)], &unclassified),
            _ => frame_strong_bending_span(&frame, lb, [0.0, lb], &[], &unclassified),
        });
        assert_eq!((history.committed_count, history.evaluated_count), (3, 1));
        assert!(history.peak.is_none(), "{history:?}");
        for expected in ["単調変化または区間内部", "釣合いを確認できません"] {
            assert!(
                history
                    .unavailable
                    .iter()
                    .any(|reason| reason.contains(expected)),
                "{history:?}"
            );
        }
    }

    #[test]
    fn material_end_uses_only_the_interval_side_of_a_point_load() {
        let model = model();
        let frame = frame();
        let load = MemberLoad::manual(
            model.elements[0].id,
            [0.0, 0.0, 1.0],
            MemberLoadKind::Point { a: 0.0, p: -200e3 },
        );
        let q = -86_333.333_333_333_33;
        let force = MemberForces {
            at: vec![
                (0.0, [-100e3, q, 0.0, 0.0, 0.0, 259e6]),
                (1.0, [100e3, q, 0.0, 0.0, 0.0, 0.0]),
            ],
        };
        let history = collect(&model, 1, |_, _, lb| {
            frame_strong_bending_span(&frame, lb, [0.0, lb], &[(1.0, &load)], &force)
        });
        assert_eq!(history.evaluated_count, 0);
        assert!(history.peak.is_none(), "{history:?}");
        assert!(
            history
                .unavailable
                .iter()
                .any(|reason| reason.contains("引張軸力")),
            "{history:?}"
        );
    }

    #[test]
    fn tensile_axial_force_at_the_maximum_does_not_become_a_lateral_buckling_failure() {
        let model = model();
        let frame = frame();
        let end_forces = [100e3, -86_333.333_333_333_33, 0.0, 0.0, 0.0, 259e6];
        let force = MemberForces {
            at: vec![
                (0.0, end_forces),
                (1.0, [100e3, end_forces[1], 0.0, 0.0, 0.0, 0.0]),
            ],
        };
        let history = collect(&model, 1, |_, _, lb| {
            frame_strong_bending_span(&frame, lb, [0.0, lb], &[], &force)
        });
        assert_eq!(history.evaluated_count, 0);
        assert!(history.peak.is_none(), "{history:?}");
        assert!(
            history
                .unavailable
                .iter()
                .any(|reason| reason.contains("引張軸力")),
            "{history:?}"
        );
    }
}
