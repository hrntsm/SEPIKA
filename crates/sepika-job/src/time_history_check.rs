//! 確定時刻ごとの簡易検定と支配値の集計。

pub(crate) mod column_bending;
pub use column_bending::targets as targets_symmetric_column_bending;

use std::collections::BTreeMap;

use sepika_core::ids::ElemId;
use sepika_core::model::{ElementKind, Material, Model, Section};
use sepika_design_jp::steel::SteelDesign;
use sepika_design_jp::{DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_element::frame::beam::eval_sections_of;
use sepika_solver::dynamic::timehistory::{
    uses_signed_bending_peaks, AsymmetricColumnSectionHistory, CommittedMemberForcesObserver,
    ResponseResult, SectionForceCheckPeak, SectionForceCheckPosition,
    SymmetricColumnBendingHistory, ThRecording,
};

/// 全確定時刻を使うH形鋼柱照査の記録。対象外と、対象だが記録がない場合を区別する。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeHistoryColumnCheck<'a> {
    OutsideScope(&'static str),
    AsymmetricSection(Result<&'a AsymmetricColumnSectionHistory, &'static str>),
    SymmetricBending(Result<&'a SymmetricColumnBendingHistory, &'static str>),
}

/// 時刻歴解析の結果と、その記録の部材順を確定した解析モデル。
#[derive(Debug)]
pub struct ConfirmedTimeHistoryResult {
    pub(crate) model: Model,
    pub(crate) response: ResponseResult,
}

impl ConfirmedTimeHistoryResult {
    /// この実行の応答。記録添字はこの実行の解析モデルの要素順。
    pub fn response(&self) -> &ResponseResult {
        &self.response
    }

    /// 応答を取り出す。生成元の部材との結び付きは引き継がない。
    pub(crate) fn into_response(self) -> ResponseResult {
        self.response
    }

    /// この実行の部材 ID で照査を参照する。対象外・記録欠落・部分的な未算定理由を区別する。
    ///
    /// ```compile_fail
    /// use sepika_core::{ids::ElemId, model::Model};
    /// use sepika_job::time_history_check::ConfirmedTimeHistoryResult;
    /// fn lookup(result: &ConfirmedTimeHistoryResult, reordered: &Model) {
    ///     result.column_check(reordered, ElemId(0));
    /// }
    /// ```
    pub fn column_check(&self, id: ElemId) -> TimeHistoryColumnCheck<'_> {
        column_check_history(&self.model, id, self.response.recording.as_ref())
    }
}

fn column_check_history<'a>(
    model: &Model,
    id: ElemId,
    recording: Option<&'a ThRecording>,
) -> TimeHistoryColumnCheck<'a> {
    use TimeHistoryColumnCheck::*;
    let mut elements = model
        .elements
        .iter()
        .enumerate()
        .filter(|(_, e)| e.id == id);
    let Some((index, elem)) = elements.next() else {
        return OutsideScope("指定した部材が解析モデルにありません。");
    };
    if elements.next().is_some() {
        return OutsideScope("部材 ID が重複し、照査対象を特定できません。");
    }
    if uses_signed_bending_peaks(model, elem) {
        AsymmetricSection(
            recording
                .and_then(|rec| rec.asymmetric_column_sections.get(&index))
                .ok_or("全確定時刻の断面照査の記録を取得できません。確認するには解析を再実行してください。"),
        )
    } else if targets_symmetric_column_bending(model, elem) {
        SymmetricBending(
            recording
                .and_then(|rec| rec.symmetric_column_bending.get(&index))
                .ok_or("確定時刻ごとの補剛区間の照査記録がありません。先頭から解析を再実行してください。"),
        )
    } else {
        OutsideScope(match elem.kind {
            ElementKind::Beam | ElementKind::Fiber | ElementKind::Brace { .. } => {
                "対象のH形鋼柱ではないため、全確定時刻の柱照査の対象外です。"
            }
            _ => "この要素種別は断面検定の対象外です（ダンパー・節点ばね要素等）。",
        })
    }
}

struct Column<'a> {
    index: usize,
    id: ElemId,
    section: &'a Section,
    material: Option<&'a Material>,
    ctx: DesignCtx,
}

#[derive(Default)]
pub(crate) struct TimeHistoryChecks {
    asymmetric_columns: BTreeMap<usize, AsymmetricColumnSectionHistory>,
    symmetric_columns: BTreeMap<usize, SymmetricColumnBendingHistory>,
}

impl TimeHistoryChecks {
    pub(crate) fn take(recording: &mut ThRecording) -> Self {
        Self {
            asymmetric_columns: std::mem::take(&mut recording.asymmetric_column_sections),
            symmetric_columns: std::mem::take(&mut recording.symmetric_column_bending),
        }
    }

    pub(crate) fn store(self, recording: &mut ThRecording) {
        recording.asymmetric_column_sections = self.asymmetric_columns;
        recording.symmetric_column_bending = self.symmetric_columns;
    }
}

pub(crate) fn collect_time_history_checks<T, E>(
    model: &Model,
    mut checks: TimeHistoryChecks,
    first_step: u64,
    run: impl FnOnce(Option<&mut CommittedMemberForcesObserver<'_>>) -> Result<T, E>,
) -> Result<(T, TimeHistoryChecks), E> {
    let history = &mut checks.asymmetric_columns;
    let bending_histories = &mut checks.symmetric_columns;
    let bending_columns = column_bending::columns(model, bending_histories);
    let columns = asymmetric_columns(model, history);
    let result = if history.is_empty() && bending_histories.is_empty() {
        run(None)?
    } else {
        run(Some(&mut |snapshot| {
            // 再開点の内力は前半結果で照査済みなので二重計上しない。
            if snapshot.step() < first_step {
                return;
            }
            for column in &bending_columns {
                let item = bending_histories.get_mut(&column.index).unwrap();
                column_bending::record(
                    item,
                    column,
                    snapshot.step(),
                    snapshot.time_s(),
                    |length_mm| {
                        let bounds_mm = [0.0, length_mm];
                        snapshot
                            .member(column.id)?
                            .strong_bending_span(bounds_mm, bounds_mm)
                    },
                );
            }
            for item in history.values_mut() {
                item.committed_count += 1;
            }
            for column in &columns {
                let member = snapshot.member(column.id);
                record_section_forces(
                    &mut history.get_mut(&column.index).unwrap().positions,
                    column,
                    snapshot.time_s(),
                    |xi| {
                        member
                            .as_ref()
                            .map_err(|&reason| reason)?
                            .recorded_section_forces(xi)
                    },
                );
            }
        }))?
    };
    Ok((result, checks))
}

fn asymmetric_columns<'a>(
    model: &'a Model,
    history: &mut BTreeMap<usize, AsymmetricColumnSectionHistory>,
) -> Vec<Column<'a>> {
    model
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
            let (lk_y, lk_z) =
                sepika_design_jp::steel::frame_buckling_lengths(model, elem, kind, steel_attr);
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
                id: elem.id,
                section,
                material,
                ctx: DesignCtx {
                    term: LoadTerm::Short,
                    kind,
                    length,
                    lk_y: Some(lk_y),
                    lk_z: Some(lk_z),
                    steel_attr: steel_attr.cloned(),
                    ..Default::default()
                },
            })
        })
        .collect()
}

fn record_section_forces(
    positions: &mut [SectionForceCheckPosition],
    column: &Column<'_>,
    time_s: f64,
    section_forces: impl Fn(f64) -> Result<[f64; 6], &'static str>,
) {
    for position in positions {
        let local = match section_forces(position.xi) {
            Ok(local) => local,
            Err(reason) => {
                if !position.unavailable.contains(reason) {
                    position.unavailable.insert(reason.into());
                }
                continue;
            }
        };
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
    use sepika_core::model::ElementData;
    use sepika_element::frame::beam::MemberForces;

    #[test]
    fn missing_column_record_and_nodal_spring_have_different_reasons() {
        let mut model = column_model();
        let id = model.elements[0].id;
        let spring_id = ElemId(17);
        model.elements.push(ElementData {
            id: spring_id,
            kind: ElementKind::NodalSpring,
            ..model.elements[0].clone()
        });
        let recording = ThRecording::default();
        assert!(matches!(column_check_history(&model, id, Some(&recording)),
            TimeHistoryColumnCheck::AsymmetricSection(Err(reason)) if reason.contains("断面照査の記録を取得できません")));
        assert!(
            matches!(column_check_history(&model, spring_id, Some(&recording)),
            TimeHistoryColumnCheck::OutsideScope(reason) if reason.contains("節点ばね"))
        );
    }

    fn run(
        model: &Model,
        mut prior: BTreeMap<usize, AsymmetricColumnSectionHistory>,
        first_step: u64,
        samples: &[(u64, Option<MemberForces>)],
    ) -> BTreeMap<usize, AsymmetricColumnSectionHistory> {
        let columns = asymmetric_columns(model, &mut prior);
        let [column] = columns.as_slice() else {
            panic!("対象柱が1本ではない")
        };
        let item = prior.get_mut(&column.index).unwrap();
        for (step, member) in samples.iter().filter(|(step, _)| *step >= first_step) {
            item.committed_count += 1;
            record_section_forces(&mut item.positions, column, *step as f64, |xi| {
                let member = member.as_ref().ok_or("部材の解析断面力がありません")?;
                let (_, force) = member
                    .at
                    .iter()
                    .find(|(pos, _)| *pos == xi)
                    .ok_or("指定した評価位置の解析断面力がありません")?;
                if force.iter().any(|f| !f.is_finite()) {
                    return Err("断面力が非有限です");
                }
                Ok(*force)
            });
        }
        prior
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
        assert_eq!(end.observed_count, 6);
        let peak = end.peak.as_ref().unwrap();
        assert_eq!((peak.time_s, peak.n_n, peak.mz_nmm), (1.0, -400e3, 125e6));
        assert!((peak.ratio - 1.065_016_34).abs() < 1e-8);
        for reason in [
            "引張軸力と強軸曲げ",
            "ねじりモーメントMx",
            "断面力が非有限",
            "曲げねじり座屈",
            "部材の解析断面力がありません",
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
        assert!(missing
            .unavailable
            .contains("指定した評価位置の解析断面力がありません"));
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
