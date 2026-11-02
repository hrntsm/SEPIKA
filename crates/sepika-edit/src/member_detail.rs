//! 部材の付帯情報（ハンチ・継手位置、`MemberDetailAttr`）の編集コマンド。

use super::*;
use sepika_core::ids::*;

/// 部材の付帯情報を追加・更新する。寸法・継手位置の単位は mm。
/// ハンチ寸法・継手位置に非有限値があれば、理由付きで拒否しモデルを変更しない。
/// 対象部材が存在しない場合は何も変更しない。
pub struct SetMemberDetailAttr {
    pub attr: sepika_core::model::MemberDetailAttr,
}

impl EditCommand for SetMemberDetailAttr {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
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
        if !crate::refs::elem_exists(model, self.attr.elem) {
            return Box::new(Noop);
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

/// 部材付帯情報エントリを削除する（`elem` に一致するものを削除）。一致する
/// エントリがなければ Noop。逆操作は削除前の値を復元する
/// [`SetMemberDetailAttr`]（このエントリの `elem` は削除時点で存在しないため、
/// `SetMemberDetailAttr` は「既存エントリなし→末尾追加」の枝を通り、元の位置
/// には戻らないが、`member_detail_attrs` は `ElemId` をキーとする集合的な
/// データであり配列順に意味はないため問題ない）。
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
