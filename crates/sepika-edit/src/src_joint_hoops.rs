use crate::{EditCommand, Noop, RejectedEdit};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::NodeId;
use sepika_core::model::{
    same_src_joint_plane_axis, src_joint_columns, src_joint_planes, Model, SrcJointHoopAssignment,
    SrcJointHoops,
};

/// SRC柱梁接合部の指定構面に帯筋を登録する。None はその構面軸の登録解除。
pub struct SetSrcJointHoops {
    pub node: NodeId,
    pub axis: [f64; 3],
    pub hoops: Option<SrcJointHoops>,
}

impl EditCommand for SetSrcJointHoops {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let reject = |reason: &str| {
            Box::new(RejectedEdit(format!(
                "SRC柱梁接合部 節点 {} 構面 {:?}: {reason}",
                self.node.0, self.axis
            ))) as Box<dyn EditCommand>
        };
        let assignment = if let Some(hoops) = &self.hoops {
            if let Err(reason) = hoops.ratio() {
                return reject(&reason);
            }
            let adjacency = NodeAdjacency::primary_frame(model);
            let mut matches = src_joint_planes(model, &adjacency, self.node)
                .into_iter()
                .filter(|plane| same_src_joint_plane_axis(plane.axis, self.axis));
            let (Some(plane), None) = (matches.next(), matches.next()) else {
                return reject("指定軸でSRC柱梁接合部の構面を一意に確認できません");
            };
            Some(SrcJointHoopAssignment {
                node: self.node,
                axis: plane.axis,
                columns: src_joint_columns(model, &adjacency, self.node)
                    .map(|column| column.id)
                    .collect(),
                beams: plane.beams.iter().map(|beam| beam.element).collect(),
                hoops: hoops.clone(),
            })
        } else {
            None
        };
        let axis = assignment.as_ref().map_or(self.axis, |entry| entry.axis);
        let matches = |entry: &SrcJointHoopAssignment| {
            entry.node == self.node
                && (entry.axis.map(f64::to_bits) == axis.map(f64::to_bits)
                    || same_src_joint_plane_axis(entry.axis, axis))
        };
        let mut entries = model.src_joint_hoops.iter().filter(|entry| matches(entry));
        if entries.next() == assignment.as_ref() && entries.next().is_none() {
            return Box::new(Noop);
        }
        let mut updated = model.src_joint_hoops.clone();
        let position = updated.iter().position(matches).unwrap_or(updated.len());
        updated.retain(|entry| !matches(entry));
        if let Some(assignment) = assignment {
            updated.insert(position, assignment);
        }
        Box::new(RestoreSrcJointHoops(std::mem::replace(
            &mut model.src_joint_hoops,
            updated,
        )))
    }

    fn affects_calculation(&self) -> bool {
        false
    }

    fn label(&self) -> &str {
        "SRC柱梁接合部の帯筋指定"
    }
}

struct RestoreSrcJointHoops(Vec<SrcJointHoopAssignment>);

impl EditCommand for RestoreSrcJointHoops {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        Box::new(Self(std::mem::replace(
            &mut model.src_joint_hoops,
            self.0.clone(),
        )))
    }

    fn affects_calculation(&self) -> bool {
        false
    }

    fn label(&self) -> &str {
        "SRC柱梁接合部の帯筋指定"
    }
}
