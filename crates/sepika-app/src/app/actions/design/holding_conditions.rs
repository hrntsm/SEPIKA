use super::super::*;
use sepika_core::model::ElementKind;
use sepika_core::structure_kind::{member_structure_kind, StructureKind};
use sepika_design_jp::MemberKind;

impl App {
    pub(crate) fn holding_ds_conditions_unavailable_reason(&self) -> Option<String> {
        let model = &self.core.model;
        let members: Vec<_> = model
            .elements
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
                ) && e.nodes.len() == 2
                    && matches!(
                        MemberKind::try_of_element(e, model),
                        Ok(MemberKind::Column | MemberKind::Girder)
                    )
            })
            .collect();
        let mut by_node = std::collections::BTreeMap::<_, Vec<_>>::new();
        for &member in &members {
            for &node in &member.nodes {
                by_node.entry(node).or_default().push(member);
            }
        }
        let mut reasons = Vec::new();
        for wall in model
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Wall)
        {
            if member_structure_kind(model, wall) == StructureKind::Rc {
                reasons.push(format!(
                    "RC壁 ID {}: Ds算定階の崩壊時応力・破壊形式の確認情報がありません。",
                    wall.id.0
                ));
            }
        }
        for plate in model
            .wall_plates
            .iter()
            .filter(|p| model.wall_plate_becomes_element(p))
        {
            if model
                .wall_plate_material(plate)
                .is_some_and(|m| m.category == sepika_core::model::MaterialCategory::Concrete)
            {
                reasons.push(format!(
                    "RC壁版 ID {}: Ds算定階の崩壊時応力・破壊形式の確認情報がありません。",
                    plate.id.0
                ));
            }
        }
        let axis = match self.core.scoped.pushover_view_dir {
            SeismicDir::X => 0,
            SeismicDir::Y => 1,
        };
        let steel_columns: Vec<_> = members
            .iter()
            .filter(|e| {
                MemberKind::of_element(e, model) == MemberKind::Column
                    && member_structure_kind(model, e) == StructureKind::S
            })
            .collect();
        for member in &members {
            let structure = member_structure_kind(model, member);
            let concrete = structure == StructureKind::Rc;
            let steel = structure == StructureKind::S;
            if !concrete && !steel {
                continue;
            }
            let kind = MemberKind::of_element(member, model);
            if concrete {
                reasons.push(format!("RC部材 ID {}: Ds算定階の崩壊時応力・破壊形式の確認情報がありません。選択step・解析ヒンジ履歴だけでは確定できません。", member.id.0));
            }
            if kind == MemberKind::Column {
                let beams: std::collections::BTreeSet<_> = member
                    .nodes
                    .iter()
                    .flat_map(|n| by_node.get(n).into_iter().flatten())
                    .filter(|beam| {
                        MemberKind::of_element(beam, model) == MemberKind::Girder
                            && model
                                .node(beam.nodes[0])
                                .zip(model.node(beam.nodes[1]))
                                .is_some_and(|(i, j)| j.coord[axis] != i.coord[axis])
                    })
                    .map(|e| e.id.0.to_string())
                    .collect();
                reasons.push(format!("{}非ヒンジ柱種別: 柱 ID {}、計算方向 {:?} の接続梁 ID [{}]（直交梁を除く）。非ヒンジ柱に該当するか、該当時の接続梁・接合部材の採用種別が未確認です。", if concrete { "RC" } else { "S" }, member.id.0, self.core.scoped.pushover_view_dir, beams.into_iter().collect::<Vec<_>>().join("、")));
            }
            if concrete && kind == MemberKind::Girder && !steel_columns.is_empty() {
                reasons.push(format!("S柱を含む架構のRC基礎梁候補: RC梁 ID {}（層外を含む）。基礎梁用途・塑性ヒンジ・限定換算の適用条件・RCのFD回避が未確認です。地域限定のRC→S種別換算を一般適用しません。", member.id.0));
            }
        }
        for column in steel_columns {
            reasons.push(format!("S柱 ID {}: 柱脚の有無・形式、露出柱脚アンカーボルトの伸び能力・保有耐力接合・認定条件が未確認です。伸び能力あり非保有耐力接合の1階Ds割増等、保有耐力接合の柱ヒンジ仮定と上部構造Ds、伸び能力なし非保有耐力接合の群Dを選択できません。群Dを0.05割増対象にしません。", column.id.0));
        }
        for (node, connected_members) in by_node {
            let connected: Vec<_> = connected_members
                .iter()
                .filter(|e| {
                    matches!(
                        member_structure_kind(model, e),
                        StructureKind::Rc | StructureKind::S
                    )
                })
                .map(|e| e.id.0.to_string())
                .collect();
            if connected.len() >= 2 {
                reasons.push(format!("接合部 節点 ID {}・部材 ID [{}]: 崩壊時に塑性ヒンジ又は脆性破壊を生じる対象部材の識別と、その対象だけの最低種別が未確認です。", node.0, connected.join("、")));
            }
        }
        (!reasons.is_empty()).then(|| format!("Ds判定保留（必要保有水平耐力・合否は算定不能）: 自動・手動ランクのいずれも次の確認不足を補えません。\n{}", reasons.join("\n")))
    }
}
