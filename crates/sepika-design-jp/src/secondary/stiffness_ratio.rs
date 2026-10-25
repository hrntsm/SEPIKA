//! 上下階SL節点の柱層間変位と、Rs用の重心位置変位を算定する。

use sepika_core::ids::{ElemId, StoryId};
use sepika_core::model::{Model, Node};

use super::story_columns::checked_story_columns;

/// 柱 1 本の層間変位（加力方向）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnDrift {
    pub elem: ElemId,
    /// 柱頭の平面位置 (x, y) `[mm]`。
    pub pos: [f64; 2],
    /// 層間変位 δ = |上端床変位 − 下端床変位| `[mm]`（加力方向の水平成分）。
    pub drift: f64,
}

/// 上下階SLを結ぶ非分岐柱の水平変位差の絶対値 `[mm]` を返す。
/// `disp` は節点ID順、`dir` は0=X・1=Y。対象変位の欠測・非有限値はErr。
pub fn column_drifts(
    model: &Model,
    disp: &[[f64; 6]],
    dir: usize,
    story: StoryId,
) -> Result<Vec<ColumnDrift>, String> {
    if dir > 1 {
        return Err(format!("層間変形角: 水平方向{dir}が不正です"));
    }
    checked_story_columns(model, story)?
        .into_iter()
        .map(|col| {
            let top = &model.nodes[col.top.index()];
            let displacement = |node: sepika_core::ids::NodeId| {
                disp.get(node.index())
                    .map(|u| u[dir])
                    .filter(|u| u.is_finite())
                    .ok_or_else(|| {
                        format!(
                            "柱{} 節点{}の変位が欠落又は非有限です",
                            col.top_elem.0, node.0
                        )
                    })
            };
            let drift = (displacement(col.top)? - displacement(col.bottom)?).abs();
            if !drift.is_finite() {
                return Err(format!("柱{}の変位差が非有限です", col.top_elem.0));
            }
            Ok(ColumnDrift {
                elem: col.top_elem,
                pos: [top.coord[0], top.coord[1]],
                drift,
            })
        })
        .collect()
}

/// 最大柱層間変位 `[mm]` と同じ対象のID・柱頭位置を返す。
/// 対象なしはOk(None)、対象変位・接続の不整合はErr。
pub fn max_column_drift(
    model: &Model,
    disp: &[[f64; 6]],
    dir: usize,
    story: StoryId,
) -> Result<Option<ColumnDrift>, String> {
    Ok(column_drifts(model, disp, dir, story)?
        .into_iter()
        .max_by(|a, b| a.drift.total_cmp(&b.drift)))
}

/// 当該層の重心位置の水平変位 `[mm]`（加力方向）。
///
/// 剛床の並進＋回転を仮定すると質量重み付き平均変位は重心位置の変位に一致する
/// （u_i = u_g + θ×(r_i − r_g) の質量加重平均は u_g）。質量未定義の節点は重み 0、
/// 層の全質量が 0 の場合は単純平均で代用する。
pub fn cog_horizontal_disp(
    model: &Model,
    disp: &[[f64; 6]],
    dir: usize,
    story: StoryId,
) -> Result<f64, String> {
    if dir > 1 || !model.stories.iter().any(|s| s.id == story) {
        return Err(format!("Rs: 階{}又は加力方向{}が不正です", story.0, dir));
    }
    let nodes: Vec<&Node> = model
        .nodes
        .iter()
        .filter(|n| n.story == Some(story) && model.on_diaphragm_level(story, n.coord[2]))
        .collect();
    if nodes.is_empty() {
        return Err(format!("Rs: 階{}の床レベルに節点がありません", story.0));
    }
    let mut total_mass = 0.0;
    let mut weighted_disp = 0.0;
    let mut sum_disp = 0.0;
    for node in &nodes {
        let mass = node.mass.map(|m| m[0]).unwrap_or(0.0);
        let u = disp
            .get(node.id.index())
            .ok_or_else(|| format!("Rs: 階{}の節点{}の変位がありません", story.0, node.id.0))?[dir];
        if !mass.is_finite() || mass < 0.0 || !u.is_finite() {
            return Err(format!(
                "Rs: 階{}の節点{}の質量又は変位が不正です",
                story.0, node.id.0
            ));
        }
        total_mass += mass;
        weighted_disp += mass * u;
        sum_disp += u;
    }
    let result = if total_mass > 0.0 {
        weighted_disp / total_mass
    } else {
        sum_disp / nodes.len() as f64
    };
    if !total_mass.is_finite() || !weighted_disp.is_finite() || !result.is_finite() {
        return Err(format!("Rs: 階{}の重心位置変位を算定できません", story.0));
    }
    Ok(result)
}

/// 全層の重心位置の層間変位 `[mm]` を下層から上層の順で返す。
/// dir は 0=X・1=Y。階対応・必要節点変位の欠損、不正値・演算不能は Err。
pub fn cog_story_drifts(model: &Model, disp: &[[f64; 6]], dir: usize) -> Result<Vec<f64>, String> {
    if dir > 1 || model.layers().is_empty() {
        return Err("Rs: 層又は水平加力方向がありません".into());
    }
    model
        .layers()
        .iter()
        .map(|layer| {
            let top = cog_horizontal_disp(model, disp, dir, layer.top)?;
            let bottom = cog_horizontal_disp(model, disp, dir, layer.bottom)?;
            let drift = (top - bottom).abs();
            if !drift.is_finite() {
                Err(format!(
                    "Rs: 上端階{}の層間変位を算定できません",
                    layer.top.0
                ))
            } else {
                Ok(drift)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId, SectionId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Node,
        RigidZone, Section, Story,
    };
    use smallvec::SmallVec;

    /// 1 層 3 本柱（2 本鉛直・1 本斜め）のテストモデル。
    /// 柱1: (0,0), 柱2: (6000,0), 斜め柱: 脚(3000,0)→頭(3000+1500,0)。
    /// 1 層モデル。層の上端の階は `StoryId(1)`（`StoryId(0)` は基部の床。
    /// 階は床であり、その先頭が基部であることが `Model::layers` の不変条件）。
    fn build_model() -> (Model, StoryId) {
        let base = StoryId(0);
        let s0 = StoryId(1);
        let mut nodes = Vec::new();
        // 柱脚（z=0, 拘束）
        let feet = [[0.0, 0.0], [6000.0, 0.0], [3000.0, 0.0]];
        for (i, &[x, y]) in feet.iter().enumerate() {
            nodes.push(Node {
                id: NodeId(i as u32),
                coord: [x, y, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(base),
                support_spring: None,
            });
        }
        // 柱頭（z=3000, story=S0）。斜め柱の頭は x を 1500 ずらす。
        let heads = [[0.0, 0.0], [6000.0, 0.0], [4500.0, 0.0]];
        let masses = [2.0, 1.0, 1.0]; // 重心を柱1側へ寄せる
        for (i, &[x, y]) in heads.iter().enumerate() {
            nodes.push(Node {
                id: NodeId((i + 3) as u32),
                coord: [x, y, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: Some([masses[i], masses[i], masses[i], 0.0, 0.0, 0.0]),
                story: Some(s0),
                support_spring: None,
            });
        }
        let elements: Vec<ElementData> = (0..3)
            .map(|i| ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: {
                    let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
                    v.push(NodeId(i as u32));
                    v.push(NodeId((i + 3) as u32));
                    v
                },
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [0.0, 1.0, 0.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: RigidZone::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            })
            .collect();
        let base_story = Story {
            wall_weights: Vec::new(),
            id: base,
            name: "1F".to_string(),
            elevation: Some(0.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(0), NodeId(1), NodeId(2)],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        };
        let story = Story {
            wall_weights: Vec::new(),
            id: s0,
            name: "2F".to_string(),
            elevation: Some(3000.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(3), NodeId(4), NodeId(5)],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        };
        let model = Model {
            nodes,
            elements,
            sections: vec![{
                let mut section = Section::zero(SectionId(0), "C".into());
                section.frame_use = Some(FrameSectionUse::Column);
                section
            }],
            stories: vec![base_story, story],
            ..Default::default()
        };
        (model, s0)
    }

    /// 変位場: 柱頭 X 変位 = [10, 20, 15]、柱脚 0。
    fn disp_field() -> Vec<[f64; 6]> {
        let mut d = vec![[0.0; 6]; 6];
        d[3][0] = 10.0;
        d[4][0] = 20.0;
        d[5][0] = 15.0;
        d
    }

    #[test]
    fn test_max_column_drift_includes_inclined() {
        let (model, s0) = build_model();
        let disp = disp_field();
        let drifts = column_drifts(&model, &disp, 0, s0).unwrap();

        assert_eq!(drifts.len(), 3);
        let max = max_column_drift(&model, &disp, 0, s0).unwrap().unwrap();
        assert_eq!(max.elem, ElemId(1));
        assert!((max.drift - 20.0).abs() < 1e-12);
    }

    #[test]
    fn test_cog_disp_mass_weighted() {
        let (model, s0) = build_model();
        let disp = disp_field();
        // δg = (2·10 + 1·20 + 1·15) / 4 = 13.75（斜め柱の節点も質量として重心には寄与）
        let dg = cog_horizontal_disp(&model, &disp, 0, s0).unwrap();
        assert!((dg - 13.75).abs() < 1e-12, "δg={dg}");
        let drifts = cog_story_drifts(&model, &disp, 0).unwrap();
        assert_eq!(drifts.len(), 1);
        assert!((drifts[0] - 13.75).abs() < 1e-12);
    }

    #[test]
    fn test_cog_disp_unmassed_falls_back_to_average() {
        let (mut model, s0) = build_model();
        for n in &mut model.nodes {
            n.mass = None;
        }
        let disp = disp_field();
        let dg = cog_horizontal_disp(&model, &disp, 0, s0).unwrap();
        assert!((dg - 15.0).abs() < 1e-12, "単純平均 (10+20+15)/3, got {dg}");
    }

    #[test]
    fn fes718_cog_drift_rejects_missing_nonfinite_and_invalid_direction() {
        let (mut model, story) = build_model();
        let mut disp = disp_field();
        assert!(cog_story_drifts(&model, &disp[..5], 0).is_err());
        assert!(cog_story_drifts(&model, &disp, 2).is_err());
        assert!(cog_horizontal_disp(&model, &disp, 0, StoryId(99)).is_err());
        disp[3][0] = f64::NAN;
        assert!(cog_story_drifts(&model, &disp, 0).is_err());
        disp[3][0] = f64::INFINITY;
        assert!(cog_story_drifts(&model, &disp, 0).is_err());
        model.nodes.retain(|node| node.story != Some(story));
        assert!(cog_story_drifts(&model, &disp, 0).is_err());
    }

    /// 中間節点（z=2000）で 2 分割された 1 本柱（z=0, 2000, 4000）。
    fn build_split_column_model() -> (Model, StoryId) {
        let base = StoryId(0);
        let top_story = StoryId(1);
        let nodes = vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(base),
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 2000.0],
                restraint: Dof6Mask::FREE,
                mass: Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
                story: Some(top_story),
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0, 0.0, 4000.0],
                restraint: Dof6Mask::FREE,
                mass: Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
                story: Some(top_story),
                support_spring: None,
            },
        ];
        let mk_elem = |id: u32, n0: u32, n1: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: {
                let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
                v.push(NodeId(n0));
                v.push(NodeId(n1));
                v
            },
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        };
        let elements = vec![mk_elem(0, 1, 2), mk_elem(1, 0, 1)];
        let base_story = Story {
            wall_weights: Vec::new(),
            id: base,
            name: "1F".to_string(),
            elevation: Some(0.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(0)],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        };
        let top = Story {
            wall_weights: Vec::new(),
            id: top_story,
            name: "2F".to_string(),
            elevation: Some(4000.0),
            sl_offset_mm: Some(0.0),
            source_story: None,
            node_ids: vec![NodeId(2)],
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
        };
        let model = Model {
            nodes,
            elements,
            sections: vec![{
                let mut section = Section::zero(SectionId(0), "C".into());
                section.frame_use = Some(FrameSectionUse::Column);
                section
            }],
            stories: vec![base_story, top],
            ..Default::default()
        };
        (model, top_story)
    }

    /// 分割柱の層間変位は上下端床節点間 |u(上端)−u(下端)| であること。
    #[test]
    fn test_column_drift_uses_full_story_height_for_split_column() {
        let (model, top) = build_split_column_model();
        let mut disp = vec![[0.0; 6]; 3];
        disp[0][0] = 0.0; // 下端 z=0
        disp[1][0] = 6.0; // 中間 z=2000（セグメント δ=4 や 6 ではない）
        disp[2][0] = 10.0; // 上端 z=4000
        let drifts = column_drifts(&model, &disp, 0, top).unwrap();
        assert_eq!(drifts.len(), 1);
        assert!(
            (drifts[0].drift - 10.0).abs() < 1e-12,
            "層間変位は |10−0|=10、got {}",
            drifts[0].drift
        );
    }

    /// 重心変位は中間節点（床レベル外）の変位を混ぜないこと。
    #[test]
    fn test_cog_disp_excludes_mid_height_nodes() {
        let (model, top) = build_split_column_model();
        let mut disp = vec![[0.0; 6]; 3];
        disp[0][0] = 0.0;
        disp[1][0] = 100.0; // 中間: 床レベル外なので除外
        disp[2][0] = 10.0; // 上端床
        let dg = cog_horizontal_disp(&model, &disp, 0, top).unwrap();
        assert!(
            (dg - 10.0).abs() < 1e-12,
            "床レベル節点のみの平均 10、got {dg}"
        );
    }
}
