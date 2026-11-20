use crate::adjacency::NodeAdjacency;
use crate::ids::{ElemId, NodeId};
use crate::panel_zone::{member_orientation, panel_joint_planes, MemberOrientation, PanelPlane};
use crate::section_shape::StandardRebar;
use crate::structure_kind::{member_structure_kind, StructureKind};
use serde::{Deserialize, Serialize};

use super::{ElementData, Model};

/// SRC柱梁接合部の1構面で有効と確認した帯筋。間隔と対象幅の単位は mm。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SrcJointHoops {
    pub effective_legs: u32,
    pub diameter: StandardRebar,
    pub spacing_mm: f64,
    pub width_mm: f64,
    pub grade: String,
}

impl SrcJointHoops {
    /// 選択可能な異形鉄筋の材種。
    pub const GRADES: [&str; 6] = ["SD295", "SD295A", "SD295B", "SD345", "SD390", "SD490"];

    /// 入力を検査し、0より大きく1未満の無次元の帯筋比を返す。
    pub fn ratio(&self) -> Result<f64, String> {
        if self.effective_legs == 0 {
            return Err("有効脚本数は1本以上が必要です".into());
        }
        if !self.spacing_mm.is_finite() || self.spacing_mm <= 0.0 {
            return Err("間隔には正の有限値 [mm] が必要です".into());
        }
        if !self.width_mm.is_finite() || self.width_mm <= 0.0 {
            return Err("対象幅には正の有限値 [mm] が必要です".into());
        }
        if !Self::GRADES.contains(&self.grade.as_str()) {
            return Err(format!(
                "帯筋材種「{}」は未対応です。対応材種は {} です",
                self.grade,
                Self::GRADES.join("・")
            ));
        }
        let area_mm2 = f64::from(self.effective_legs) * self.diameter.area_mm2();
        let denominator_mm2 = self.width_mm * self.spacing_mm;
        let ratio = area_mm2 / denominator_mm2;
        if !ratio.is_finite() || ratio <= 0.0 || ratio >= 1.0 {
            return Err("帯筋比には0より大きく1未満の有限値が必要です".into());
        }
        Ok(ratio)
    }
}

/// 節点・正向き水平単位軸・接続柱・構面内の梁に結び付けた帯筋の指定。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SrcJointHoopAssignment {
    pub node: NodeId,
    pub axis: [f64; 3],
    pub columns: Vec<ElemId>,
    pub beams: Vec<ElemId>,
    pub hoops: SrcJointHoops,
}

/// 節点に接続する主架構の柱を返す。構造種別は問わない。
pub fn src_joint_columns<'a>(
    model: &'a Model,
    adjacency: &'a NodeAdjacency,
    node: NodeId,
) -> impl Iterator<Item = &'a ElementData> + 'a {
    adjacency
        .elements_at(model, node)
        .filter(|element| member_orientation(model, element) == Some(MemberOrientation::Column))
}

/// SRC柱が取り付き、正向き水平単位軸を解決できる柱梁接合部の構面を返す。
pub fn src_joint_planes(model: &Model, adjacency: &NodeAdjacency, node: NodeId) -> Vec<PanelPlane> {
    if !src_joint_columns(model, adjacency, node)
        .any(|column| member_structure_kind(model, column) == StructureKind::Src)
    {
        return Vec::new();
    }
    panel_joint_planes(model, node, adjacency.elements_at(model, node))
        .into_iter()
        .filter(|plane| is_horizontal_unit_axis(plane.axis))
        .collect()
}

fn is_horizontal_unit_axis(axis: [f64; 3]) -> bool {
    let [x, y, z] = axis;
    z == 0.0 && (x.hypot(y) - 1.0).abs() <= 1e-12
}

/// 構面軸を成分ごとの数値公差で比較する。
pub fn same_src_joint_plane_axis(a: [f64; 3], b: [f64; 3]) -> bool {
    is_horizontal_unit_axis(a)
        && is_horizontal_unit_axis(b)
        && a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 1e-12)
}
