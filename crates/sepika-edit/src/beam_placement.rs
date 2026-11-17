use crate::{EditCommand, Noop, RejectedEdit};
use sepika_core::model::{FrameSectionUse, Model, SecondaryMemberKind, SupportMemberId};

/// 大梁の解析偏心・小梁の表示偏心を全体XYZ `[mm]`、i端・j端の順で設定する。
pub struct SetBeamEndOffsets {
    pub member: SupportMemberId,
    pub value: [[f64; 3]; 2],
}

impl EditCommand for SetBeamEndOffsets {
    fn affects_calculation(&self) -> bool {
        matches!(self.member, SupportMemberId::Primary(_))
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let reject = |reason: String| -> Box<dyn EditCommand> {
            Box::new(RejectedEdit(format!(
                "梁 {:?} のXYZ偏心: {reason}",
                self.member
            )))
        };
        if !self.value.iter().flatten().all(|v| v.is_finite()) {
            return reject("各成分に有限値が必要です".into());
        }
        let (ends, old) = match self.member {
            SupportMemberId::Primary(id) => {
                let Some(e) = model.element(id) else {
                    return reject("部材がありません".into());
                };
                if !e.kind.is_primary_frame_support()
                    || !model
                        .element_section(e)
                        .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Girder))
                {
                    return reject("大梁ではありません".into());
                }
                if let Err(reason) = model.primary_beam_axis_ends(e) {
                    return reject(reason);
                }
                let ends = e
                    .nodes
                    .iter()
                    .map(|id| model.node(*id).map(|n| n.coord))
                    .collect::<Option<Vec<_>>>();
                let Some(ends) = ends.filter(|ends| ends.len() == 2) else {
                    return reject("基準節点が不正です".into());
                };
                ([ends[0], ends[1]], e.end_offsets_mm)
            }
            SupportMemberId::Secondary(id) => {
                let Some(sm) = model.secondary_member(id) else {
                    return reject("小梁がありません".into());
                };
                if sm.kind != SecondaryMemberKind::Beam {
                    return reject("間柱の非0偏心は未対応です".into());
                }
                let Some((a, b)) = model.secondary_member_end_points(sm) else {
                    return reject("支持アンカーを解決できません".into());
                };
                ([a, b], sm.end_offsets_mm)
            }
        };
        if old == self.value {
            return Box::new(Noop);
        }
        let actual = std::array::from_fn::<_, 2, _>(|side| {
            sepika_core::geom::vec3::add(ends[side], self.value[side])
        });
        let length = sepika_core::geom::vec3::dist(actual[0], actual[1]);
        if !actual.iter().flatten().all(|v| v.is_finite()) || !length.is_finite() || length <= 0.0 {
            return reject("実材軸長は有限な正値でなければなりません".into());
        }
        let mut loads = Vec::new();
        if let SupportMemberId::Primary(id) = self.member {
            for (case_index, case) in model.load_cases.iter().enumerate() {
                for (load_index, load) in case.member.iter().enumerate() {
                    if load.elem != id || load.source != sepika_core::model::LoadSource::Manual {
                        continue;
                    }
                    let mut next = load.clone();
                    if next.extent == sepika_core::model::MemberLoadExtent::FullLengthUniform {
                        if let sepika_core::model::MemberLoadKind::Distributed { a, b, .. } =
                            &mut next.kind
                        {
                            *a = 0.0;
                            *b = length;
                        }
                    }
                    if let Err(reason) = next.validate_extent(length) {
                        return reject(format!(
                            "荷重ケース {} member[{}] 荷重「{}」: {reason}",
                            case.id.0, load_index, load.name
                        ));
                    }
                    loads.push((case_index, load_index, load.clone(), next));
                }
            }
        }
        match self.member {
            SupportMemberId::Primary(id) => model.elements[id.index()].end_offsets_mm = self.value,
            SupportMemberId::Secondary(id) => {
                crate::secondary::find_secondary_mut(model, id)
                    .unwrap()
                    .end_offsets_mm = self.value
            }
        }
        let saved = loads
            .iter()
            .map(|(c, i, old, _)| (*c, *i, old.clone()))
            .collect();
        for (case, index, _, load) in loads {
            model.load_cases[case].member[index] = load;
        }
        Box::new(RestoreBeamEndOffsets {
            command: Self {
                member: self.member,
                value: old,
            },
            loads: saved,
        })
    }
    fn label(&self) -> &str {
        "梁の端別XYZ偏心変更"
    }
}

struct RestoreBeamEndOffsets {
    command: SetBeamEndOffsets,
    loads: Vec<(usize, usize, sepika_core::model::MemberLoad)>,
}

impl EditCommand for RestoreBeamEndOffsets {
    fn affects_calculation(&self) -> bool {
        self.command.affects_calculation()
    }
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let inverse = self.command.apply(model);
        if inverse.rejection().is_some() || inverse.is_noop() {
            return inverse;
        }
        for (case, index, load) in &self.loads {
            model.load_cases[*case].member[*index] = load.clone();
        }
        inverse
    }
    fn label(&self) -> &str {
        self.command.label()
    }
}
