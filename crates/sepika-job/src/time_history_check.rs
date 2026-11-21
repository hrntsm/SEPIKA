//! 確定時刻ごとの断面単独照査と支配値の集計。

use std::collections::BTreeMap;

use sepika_core::model::{Material, Model, Section};
use sepika_design_jp::steel::{buckling::resolve_steel_buckling_lengths, SteelDesign};
use sepika_design_jp::{DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_element::frame::beam::{eval_sections_of, MemberForces};
use sepika_solver::dynamic::timehistory::{
    uses_signed_bending_peaks, AsymmetricColumnSectionHistory, CommittedMemberForcesObserver,
    SectionForceCheckPeak, SectionForceCheckPosition,
};

struct Column<'a> {
    index: usize,
    section: &'a Section,
    material: Option<&'a Material>,
    ctx: DesignCtx,
}

pub(crate) fn collect_asymmetric_column_sections<T, E>(
    model: &Model,
    mut history: BTreeMap<usize, AsymmetricColumnSectionHistory>,
    first_step: u64,
    run: impl FnOnce(Option<&mut CommittedMemberForcesObserver<'_>>) -> Result<T, E>,
) -> Result<(T, BTreeMap<usize, AsymmetricColumnSectionHistory>), E> {
    let columns: Vec<_> = model
        .elements
        .iter()
        .enumerate()
        .filter(|(_, elem)| uses_signed_bending_peaks(model, elem))
        .filter_map(|(index, elem)| {
            let section = model.element_section(elem)?;
            let material = model.element_material(elem);
            let length = model.member_length(elem);
            let kind = MemberKind::Column;
            let steel_attr = model
                .steel_design_attrs
                .iter()
                .find(|attr| attr.elem == elem.id);
            let item = history.entry(index).or_default();
            let (lk_y, lk_z) = resolve_steel_buckling_lengths(elem.id, kind, length, steel_attr)
                .map(|(y, z)| (Some(y), Some(z)))
                .unwrap_or_else(|reason| {
                    item.unavailable.insert(reason);
                    (None, None)
                });
            if item.positions.is_empty() {
                item.positions = eval_sections_of(elem, model, length)
                    .into_iter()
                    .map(|xi| SectionForceCheckPosition {
                        xi,
                        ..Default::default()
                    })
                    .collect();
            }
            if material.is_none() {
                item.unavailable
                    .insert("断面に材料が割り当てられていません".into());
            }
            if !length.is_finite() || length <= 0.0 {
                item.unavailable
                    .insert("有効な材軸長を取得できません".into());
                return None;
            }
            Some(Column {
                index,
                section,
                material,
                ctx: DesignCtx {
                    term: LoadTerm::Short,
                    kind,
                    length,
                    lk_y,
                    lk_z,
                    steel_attr: steel_attr.cloned(),
                    ..Default::default()
                },
            })
        })
        .collect();
    let result = if history.is_empty() {
        run(None)?
    } else {
        run(Some(
            &mut |step, time_s, forces: &[Option<MemberForces>]| {
                // 再開点の内力は前半結果で照査済みなので二重計上しない。
                if step >= first_step {
                    for item in history.values_mut() {
                        item.committed_count += 1;
                    }
                    for column in &columns {
                        if let Some(member) = forces.get(column.index).and_then(Option::as_ref) {
                            record_section_forces(
                                &mut history.get_mut(&column.index).unwrap().positions,
                                column,
                                time_s,
                                member,
                            );
                        }
                    }
                }
            },
        ))?
    };
    Ok((result, history))
}

fn record_section_forces(
    positions: &mut [SectionForceCheckPosition],
    column: &Column<'_>,
    time_s: f64,
    member: &MemberForces,
) {
    for position in positions {
        let mut matching = member.at.iter().filter(|(xi, _)| *xi == position.xi);
        let Some(&(_, local)) = matching.next() else {
            continue;
        };
        if matching.next().is_some() {
            position
                .unavailable
                .insert("同一評価位置の断面力が重複しています".into());
            continue;
        }
        position.observed_count += 1;
        let Some(material) = column.material else {
            continue;
        };
        let forces = MemberForcesAt::from_local(position.xi, local);
        match SteelDesign.asymmetric_column_section_check(
            &forces,
            column.section,
            material,
            &column.ctx,
        ) {
            Ok((ratio, unavailable)) => {
                for reason in unavailable {
                    if !position.unavailable.contains(reason) {
                        position.unavailable.insert(reason.into());
                    }
                }
                if position.peak.as_ref().is_none_or(|peak| ratio > peak.ratio) {
                    position.peak = Some(SectionForceCheckPeak {
                        time_s,
                        n_n: forces.n,
                        mz_nmm: forces.mz,
                        ratio,
                    });
                }
            }
            Err(reason) => {
                position.unavailable.insert(reason);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/fixtures/asymmetric_column_model.rs"]
mod asymmetric_column_model;

#[cfg(test)]
mod tests {
    use super::asymmetric_column_model::column_model;
    use super::*;

    fn run(
        model: &Model,
        prior: BTreeMap<usize, AsymmetricColumnSectionHistory>,
        first_step: u64,
        samples: &[(u64, Option<MemberForces>)],
    ) -> BTreeMap<usize, AsymmetricColumnSectionHistory> {
        collect_asymmetric_column_sections(model, prior, first_step, |observer| {
            let observer = observer.unwrap();
            for (step, member) in samples {
                observer(*step, *step as f64, std::slice::from_ref(member));
            }
            Ok::<_, ()>(())
        })
        .unwrap()
        .1
    }

    #[test]
    fn missing_and_unavailable_section_forces_do_not_erase_confirmed_failures() {
        let model = column_model();
        let sample = |step, n, mz| {
            (
                step,
                Some(MemberForces {
                    at: vec![
                        (0.0, [n, 0.0, 0.0, 0.0, 0.0, mz]),
                        (0.5, [n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    ],
                }),
            )
        };
        let mut samples = [
            sample(0, -800e3, 0.0),
            sample(1, -400e3, 125e6),
            sample(2, -1e3, 130e6),
            sample(3, -1e3, -145e6),
            sample(4, 1e3, 1e12),
            sample(5, -1e3, 1e12),
            (6, None),
            sample(7, -1e3, f64::NAN),
        ];
        samples[2].1.as_mut().unwrap().at.reverse();
        samples[5].1.as_mut().unwrap().at[0].1[3] = 1e6;
        let full = run(&model, Default::default(), 0, &samples);
        let prefix = run(&model, Default::default(), 0, &samples[..5]);
        let resumed = run(&model, prefix, 5, &samples[4..]);
        assert_eq!(resumed, full);

        let column = &resumed[&0];
        assert_eq!(column.committed_count, samples.len());
        let [end, middle, missing] = column.positions.as_slice() else {
            panic!("{column:?}")
        };
        assert_eq!(end.observed_count, 7);
        let peak = end.peak.as_ref().unwrap();
        assert_eq!((peak.time_s, peak.n_n, peak.mz_nmm), (1.0, -400e3, 125e6));
        assert!((peak.ratio - 1.065_016_34).abs() < 1e-8);
        for reason in [
            "引張軸力と強軸曲げ",
            "ねじりモーメントMx",
            "断面力が非有限",
            "曲げねじり座屈",
        ] {
            assert!(
                end.unavailable.iter().any(|r| r.contains(reason)),
                "{reason}: {end:?}"
            );
        }
        assert_eq!(
            (middle.xi, middle.peak.as_ref().unwrap().time_s),
            (0.5, 0.0)
        );
        assert_eq!(
            (missing.xi, missing.observed_count, &missing.peak),
            (1.0, 0, &None)
        );
    }

    #[test]
    fn section_loss_and_unresolved_strength_prevent_section_ratios() {
        use sepika_core::model::SteelDesignAttr;

        let mut model = column_model();
        let forces = [(
            0,
            Some(MemberForces {
                at: vec![(0.5, [-400e3, 0.0, 0.0, 0.0, 0.0, 125e6])],
            }),
        )];
        let assert_skipped = |reason: &str, model: &Model| {
            let checks = run(model, Default::default(), 0, &forces);
            let middle = &checks[&0].positions[1];
            assert!(middle.peak.is_none(), "{middle:?}");
            assert!(
                middle.unavailable.iter().any(|r| r.contains(reason)),
                "{middle:?}"
            );
        };
        model.steel_design_attrs.push(SteelDesignAttr {
            elem: model.elements[0].id,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 15.0,
            lb_direct: None,
            lateral_brace_count: None,
            column_lateral_buckling_end_supports: None,
            lk_y_direct: None,
            lk_z_direct: None,
            c_direct: None,
        });
        assert_skipped("断面欠損", &model);
        model.steel_design_attrs.clear();
        model.materials[0].fy = None;
        assert_skipped("基準強度", &model);
    }
}
