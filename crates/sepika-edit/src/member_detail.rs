//! 部材の付帯情報（ハンチ・継手位置、`MemberDetailAttr`）の編集コマンド。

use super::*;
use sepika_core::ids::*;
use sepika_core::model::{Haunch, HaunchQd1Face};

/// 部材の付帯情報を追加・更新する。寸法・継手位置の単位は mm。
/// 対象部材が存在しない、ハンチ寸法・継手位置が非有限、またはQD1用の入力値に矛盾があれば拒否する。
pub struct SetMemberDetailAttr {
    pub attr: sepika_core::model::MemberDetailAttr,
}

impl SetMemberDetailAttr {
    /// 既存ハンチのQD1用フェースだけを変える編集では、解析を陳腐化させないコマンドを返す。
    pub fn command_for(self, model: &Model) -> Box<dyn EditCommand> {
        let geometry =
            |haunch: Option<Haunch>| haunch.map(|h| [h.length, h.depth_increase, h.width_increase]);
        if model.member_detail(self.attr.elem).is_some_and(|before| {
            geometry(before.haunch_i) == geometry(self.attr.haunch_i)
                && geometry(before.haunch_j) == geometry(self.attr.haunch_j)
                && before.joints == self.attr.joints
        }) {
            return Box::new(SetMemberDetailQd1Faces {
                elem: self.attr.elem,
                faces: [self.attr.haunch_i, self.attr.haunch_j]
                    .map(|end| end.and_then(|h| h.qd1_face)),
            });
        }
        Box::new(self)
    }
}

impl EditCommand for SetMemberDetailAttr {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(error) = model.validate_member_detail_attr_ref(self.attr.elem) {
            return Box::new(RejectedEdit(error.to_string()));
        }
        for (end, haunch) in [("i端", &self.attr.haunch_i), ("j端", &self.attr.haunch_j)] {
            if let Some(haunch) = haunch {
                for (label, value) in [
                    ("ハンチ長", haunch.length),
                    ("せい増分", haunch.depth_increase),
                    ("幅増分", haunch.width_increase),
                ] {
                    if !value.is_finite() {
                        return Box::new(RejectedEdit(format!(
                            "{end}{label}は有限の値で入力してください"
                        )));
                    }
                }
                if let Some(reason) = haunch.qd1_face.and_then(|face| face.input_issue()) {
                    return Box::new(RejectedEdit(format!(
                        "{end}ハンチのQD1用フェース: {reason}"
                    )));
                }
            }
        }
        if self
            .attr
            .joints
            .iter()
            .any(|joint| !joint.distance.is_finite())
        {
            return Box::new(RejectedEdit("継手位置は有限の値で入力してください".into()));
        }
        if let Some(pos) = model
            .member_detail_attrs
            .iter()
            .position(|a| a.elem == self.attr.elem)
        {
            let old = model.member_detail_attrs[pos].clone();
            model.member_detail_attrs[pos] = self.attr.clone();
            Box::new(SetMemberDetailAttr { attr: old })
        } else {
            model.member_detail_attrs.push(self.attr.clone());
            Box::new(RemoveMemberDetailAttr {
                elem: self.attr.elem,
            })
        }
    }

    fn label(&self) -> &str {
        "部材付帯情報変更"
    }
}

struct SetMemberDetailQd1Faces {
    elem: ElemId,
    faces: [Option<HaunchQd1Face>; 2],
}

impl EditCommand for SetMemberDetailQd1Faces {
    fn affects_calculation(&self) -> bool {
        false
    }

    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Err(error) = model.validate_member_detail_attr_ref(self.elem) {
            return Box::new(RejectedEdit(error.to_string()));
        }
        let Some(detail) = model
            .member_detail_attrs
            .iter_mut()
            .find(|detail| detail.elem == self.elem)
        else {
            return Box::new(RejectedEdit(
                "QD1用フェースの対象となる部材付帯情報がありません".into(),
            ));
        };
        for (index, (haunch, face)) in [detail.haunch_i, detail.haunch_j]
            .into_iter()
            .zip(self.faces)
            .enumerate()
        {
            if let Some(face) = face {
                let end = ["i端", "j端"][index];
                if haunch.is_none() {
                    return Box::new(RejectedEdit(format!("{end}のハンチがありません")));
                }
                if let Some(reason) = face.input_issue() {
                    return Box::new(RejectedEdit(format!(
                        "{end}ハンチのQD1用フェース: {reason}"
                    )));
                }
            }
        }
        let previous = [detail.haunch_i, detail.haunch_j].map(|end| end.and_then(|h| h.qd1_face));
        if previous == self.faces {
            return Box::new(Noop);
        }
        for (haunch, face) in [&mut detail.haunch_i, &mut detail.haunch_j]
            .into_iter()
            .zip(self.faces)
        {
            if let Some(haunch) = haunch {
                haunch.qd1_face = face;
            }
        }
        Box::new(Self {
            elem: self.elem,
            faces: previous,
        })
    }

    fn label(&self) -> &str {
        "RC梁のハンチ付き材端フェース変更"
    }
}

/// `elem` に一致する最初の部材付帯情報を削除する。一致する情報がなければ Noop。
pub struct RemoveMemberDetailAttr {
    pub elem: ElemId,
}

impl EditCommand for RemoveMemberDetailAttr {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        if let Some(pos) = model
            .member_detail_attrs
            .iter()
            .position(|a| a.elem == self.elem)
        {
            let old = model.member_detail_attrs.remove(pos);
            Box::new(SetMemberDetailAttr { attr: old })
        } else {
            Box::new(Noop)
        }
    }

    fn label(&self) -> &str {
        "部材付帯情報削除"
    }
}
