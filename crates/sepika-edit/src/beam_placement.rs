use crate::{EditCommand, Noop, RejectedEdit};
use sepika_core::model::{FrameSectionUse, Model, SecondaryMemberKind, SupportMemberId};

/// 大梁・小梁の一律鉛直偏心 [mm] を設定する。None は未指定へ戻し、非有限値は拒否する。
pub struct SetBeamVerticalEccentricity {
    pub member: SupportMemberId,
    pub value: Option<f64>,
}

impl EditCommand for SetBeamVerticalEccentricity {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if self.value.is_some_and(|v| !v.is_finite()) {
            return Box::new(RejectedEdit(format!(
                "梁 {:?}: 鉛直偏心には有限値が必要です",
                self.member
            )));
        }
        let slot = match self.member {
            SupportMemberId::Primary(id) => {
                if !model.element(id).is_some_and(|e| {
                    e.kind.is_primary_frame_support()
                        && model
                            .element_section(e)
                            .is_some_and(|s| s.frame_use == Some(FrameSectionUse::Girder))
                }) {
                    return Box::new(Noop);
                }
                &mut model.elements[id.index()].vertical_eccentricity
            }
            SupportMemberId::Secondary(id) => {
                let Some(sm) = crate::secondary::find_secondary_mut(model, id) else {
                    return Box::new(Noop);
                };
                if sm.kind != SecondaryMemberKind::Beam {
                    return Box::new(Noop);
                }
                &mut sm.vertical_eccentricity
            }
        };
        if *slot == self.value {
            return Box::new(Noop);
        }
        let value = std::mem::replace(slot, self.value);
        Box::new(Self {
            member: self.member,
            value,
        })
    }
    fn label(&self) -> &str {
        "梁の一律鉛直偏心変更"
    }
}
