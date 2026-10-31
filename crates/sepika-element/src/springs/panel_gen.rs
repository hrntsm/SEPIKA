//! 仕口パネル要素の自動生成（準備計算の前処理）。
//!
//! S 造（CFT を除く）の柱梁接合節点を検出し、[`ElementKind::PanelZone`] の要素を
//! モデルへ生成する。解析に先立って 1 回適用する冪等な前処理である。
//!
//! # 生成条件
//!
//! 柱とはりが 1 本以上ずつ取り付き、それらがすべて S 系で、諸元を解決できる
//! 柱があり `Ve` が正の節点である。取り付く柱に CFT が 1 本でもある接合部は
//! 対象外とする。
//!
//! # パネル分のオフセットをモデルへ書き込む
//!
//! パネルを設けた接合部では、部材は節点ではなくパネルの面（柱フェース・梁フェース）
//! で接合する。この接合位置までのオフセットを、生成時に
//! 各部材の `rigid_zone.panel_offset_i/j` へ書き込む。
//!
//! 剛域長 `length_i/j` とは別のフィールドへ入れる。
//!
//! 剛体アーム長は `max(剛域長, パネルオフセット)` とする。
//!
//! 全要素の `panel_offset_i/j` を毎回求め直すため、パネルを OFF にすれば
//! 値は 0 へ戻り、繰り返し適用しても増えない（冪等）。
//!
//! # 要素 ID の扱い
//!
//! `Model` は「配列添字 == `ElemId`」を不変条件とする。本モジュールは
//!
//! 1. 既存のパネル要素をすべて取り除き、残った要素の ID を連番へ詰め直す
//!    （モデル内の全 `ElemId` 参照を同時に付け替える）
//! 2. 新しいパネル要素を末尾へ追加する
//!
//! の順で処理する。

use sepika_core::adjacency::NodeAdjacency;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Model, PanelZoneMode,
};
use sepika_core::panel_zone::{
    is_horizontal_or_vertical_member, member_orientation, panel_connection_offset,
    resolve_panel_analysis_joint, PanelJointError,
};

/// 1 つの接合部に生成するパネルの諸元。方向別の値は `[強軸, 弱軸]` の順。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeneratedPanel {
    /// 接合部の節点。
    pub node: NodeId,
    /// 方向別パネル寸法 `dc` `[mm]`。H 形の弱軸はフランジ幅。
    pub dc: [f64; 2],
    /// 梁フランジ板厚中心間距離 `db` `[mm]`。
    pub db: [f64; 2],
    /// 抵抗板 1 枚あたりの有効厚 `tp` `[mm]`。
    pub tp: [f64; 2],
    /// 実効体積 `Ve` `[mm³]`。
    pub ve: [f64; 2],
    /// 柱の強軸・弱軸に対応するせん断剛性 `G・Ve` [N·mm/rad]。
    pub k_panel: [f64; 2],
}

/// モデルから既存の仕口パネル要素をすべて取り除き、残った要素の ID を連番へ
/// 詰め直す（モデル内の全 `ElemId` 参照も同時に付け替える）。
fn remove_existing_panels(model: &mut Model) {
    if !model
        .elements
        .iter()
        .any(|e| matches!(e.kind, ElementKind::PanelZone))
    {
        return;
    }
    let mut remap: Vec<Option<u32>> = vec![None; model.elements.len()];
    let mut kept: Vec<ElementData> = Vec::with_capacity(model.elements.len());
    for mut e in std::mem::take(&mut model.elements) {
        if matches!(e.kind, ElementKind::PanelZone) {
            continue;
        }
        let new_id = kept.len() as u32;
        remap[e.id.index()] = Some(new_id);
        e.id = ElemId(new_id);
        kept.push(e);
    }
    model.elements = kept;

    let shift = |id: &mut ElemId| {
        if let Some(Some(new)) = remap.get(id.index()).copied() {
            *id = ElemId(new);
        }
    };
    for lc in &mut model.load_cases {
        for ml in &mut lc.member {
            shift(&mut ml.elem);
        }
    }
    model.shift_elem_attr_refs(shift);
}

/// 節点 `node` にパネルを設けるべきか判定し、設ける場合は諸元と接続節点を返す。
///
/// 適用条件・方向別諸元は [`resolve_panel_analysis_joint`] に委ね、剛性と接続節点を返す。
fn panel_at(
    model: &Model,
    adjacency: &NodeAdjacency,
    node: NodeId,
) -> Result<Option<(GeneratedPanel, Vec<NodeId>)>, String> {
    let joint = match resolve_panel_analysis_joint(model, node, adjacency.elements_at(model, node))
    {
        Ok(joint) => joint,
        Err(PanelJointError::BeamInapplicable(_)) => return Ok(None),
        Err(reason) => return Err(reason.to_string()),
    };
    let Some(joint) = joint else {
        return Ok(None);
    };
    let k_panel = joint.panels.map(|panel| joint.g * panel.ve);

    let connected: Vec<NodeId> = adjacency
        .elements_at(model, node)
        .filter_map(|e| {
            let far = far_end_at(e, node)?;
            member_orientation(model, e).map(|_| far)
        })
        .collect();

    Ok(Some((
        GeneratedPanel {
            node,
            dc: joint.panels.map(|panel| panel.geometry.dc),
            db: [joint.db; 2],
            tp: joint.panels.map(|panel| panel.geometry.tp),
            ve: joint.panels.map(|panel| panel.ve),
            k_panel,
        },
        connected,
    )))
}

/// 要素 `e` の端点に `node` が現れるなら、その反対端の節点を返す。
fn far_end_at(e: &ElementData, node: NodeId) -> Option<NodeId> {
    match e.nodes.iter().take(2).position(|n| *n == node)? {
        0 => e.nodes.get(1).copied(),
        _ => e.nodes.first().copied(),
    }
}

/// 部材の `rigid_zone.panel_offset_i/j` を、現在のパネル配置から求め直す。
///
/// パネルが 1 つもなければ全要素の値が 0 になるため、モデル化を OFF にすると
/// オフセットは消える（冪等）。
fn apply_panel_offsets(
    model: &mut Model,
    adjacency: &NodeAdjacency,
    panels: &[GeneratedPanel],
) -> Result<(), String> {
    let mut offsets = vec![[0.0; 2]; model.elements.len()];
    for panel in panels {
        let joint = resolve_panel_analysis_joint(
            model,
            panel.node,
            adjacency.elements_at(model, panel.node),
        )
        .map_err(|reason| reason.to_string())?
        .expect("生成対象の接合部");
        let column = &model.elements[joint.column.index()];
        for member in adjacency.elements_at(model, panel.node) {
            if !is_horizontal_or_vertical_member(model, member)
                || member_orientation(model, member).is_none()
            {
                continue;
            }
            let end = usize::from(member.nodes[0] != panel.node);
            offsets[member.id.index()][end] =
                panel_connection_offset(model, panel.node, column, member)
                    .map_err(|reason| reason.to_string())?;
        }
    }
    for (element, ends) in model.elements.iter_mut().zip(offsets) {
        element.rigid_zone.panel_offset_i = ends[0];
        element.rigid_zone.panel_offset_j = ends[1];
    }
    Ok(())
}

/// モデルの仕口パネル要素を再生成する（冪等）。
///
/// `Model::panel_zone` が [`PanelZoneMode::None`] のときは既存のパネルを取り除く
/// だけで、新しいパネルは生成しない。
///
/// 戻り値は生成したパネルの諸元（節点 index の昇順）。採用柱の解決失敗は接合部 ID と理由付きエラー。
pub fn apply_auto_panel_zones(model: &mut Model) -> Result<Vec<GeneratedPanel>, String> {
    if model.panel_zone == PanelZoneMode::Model {
        let adjacency = NodeAdjacency::build(model);
        for node in &model.nodes {
            panel_at(model, &adjacency, node.id)?;
        }
    }
    remove_existing_panels(model);
    if model.panel_zone != PanelZoneMode::Model {
        for e in &mut model.elements {
            e.rigid_zone.panel_offset_i = 0.0;
            e.rigid_zone.panel_offset_j = 0.0;
        }
        return Ok(Vec::new());
    }
    let adjacency = NodeAdjacency::build(model);

    let mut generated = Vec::new();
    let mut new_elements = Vec::new();
    let node_ids: Vec<NodeId> = model.nodes.iter().map(|n| n.id).collect();
    for node in node_ids {
        let Some((panel, connected)) = panel_at(model, &adjacency, node)? else {
            continue;
        };
        let mut nodes: smallvec::SmallVec<[NodeId; 8]> = smallvec::smallvec![node];
        nodes.extend(connected);
        new_elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::PanelZone,
            nodes,
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
        generated.push(panel);
    }

    for mut e in new_elements {
        e.id = ElemId(model.elements.len() as u32);
        model.elements.push(e);
    }

    apply_panel_offsets(model, &adjacency, &generated)?;
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::MaterialCategory;
    use sepika_core::model::{FrameSectionUse, Material, Node, Section};
    use sepika_core::panel_zone::resolve_panel_joint;
    use sepika_core::panel_zone::PanelGeometry;
    use sepika_core::section_shape::SectionShape;

    fn h_shape(height: f64, width: f64, tw: f64, tf: f64) -> SectionShape {
        SectionShape::SteelH {
            root_r: Some(0.0),
            height,
            width,
            web_thick: tw,
            flange_thick: tf,
        }
    }

    /// 断面（材料 0 = 鋼材を割り当てる。材料は断面が持つ）。
    fn section(id: u32, shape: SectionShape, depth: f64) -> Section {
        section_with_mat(id, shape, depth, 0)
    }

    /// 主材料を指定して断面を作る。
    fn section_with_mat(id: u32, shape: SectionShape, depth: f64, mat: u32) -> Section {
        Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(id),
            material: Some(MaterialId(mat)),
            name: String::new(),
            area: 1.0e4,
            iy: 1.0e8,
            iz: 1.0e8,
            j: 1.0e7,
            depth,
            width: depth,
            as_y: 4.0e3,
            as_z: 4.0e3,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: Some(shape),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }
    }

    /// 材料 0 = 鋼材、材料 1 = コンクリート。
    fn test_material(id: u32, category: MaterialCategory) -> Material {
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(id),
            name: if category == MaterialCategory::Steel {
                "SN400B"
            } else {
                "Fc24"
            }
            .into(),
            category,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
        }
    }

    fn member(id: u32, n0: u32, n1: u32, sec: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
            section: Some(SectionId(sec)),
            local_axis: LocalAxis {
                ref_vector: if sec == 1 {
                    [1.0, 0.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }
    }

    /// 柱 1 本・梁 1 本の S 造 L 型接合部モデル。`col_shape` で柱断面を差し替える。
    fn l_frame(col_shape: SectionShape) -> Model {
        l_frame_with_beam(col_shape, h_shape(600.0, 200.0, 11.0, 17.0), 600.0)
    }

    /// [`l_frame`] の梁断面も差し替えられる版。
    fn l_frame_with_beam(
        col_shape: SectionShape,
        beam_shape: SectionShape,
        beam_depth: f64,
    ) -> Model {
        let node = |id: u32, coord: [f64; 3]| Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        Model {
            nodes: vec![
                node(0, [0.0, 0.0, 3000.0]),
                node(1, [6000.0, 0.0, 3000.0]),
                node(2, [0.0, 0.0, 0.0]),
            ],
            sections: vec![
                {
                    let mut section = section(0, beam_shape, beam_depth);
                    section.frame_use = Some(FrameSectionUse::Girder);
                    section
                },
                {
                    let mut section = section(1, col_shape, 400.0);
                    section.frame_use = Some(FrameSectionUse::Column);
                    section
                },
            ],
            materials: vec![
                test_material(0, MaterialCategory::Steel),
                test_material(1, MaterialCategory::Concrete),
            ],
            elements: vec![member(0, 0, 1, 0), member(1, 2, 0, 1)],
            ..Default::default()
        }
    }

    #[test]
    fn test_panel_generation_offsets_and_disabled_mode_lifecycle() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].node, NodeId(0));
        assert!((panels[0].dc[0] - (400.0 - 21.0)).abs() < 1e-9);
        assert!((panels[0].db[0] - (600.0 - 17.0)).abs() < 1e-9);
        assert!((panels[0].tp[0] - 13.0).abs() < 1e-9);
        assert!(panels[0].k_panel[0] > 0.0);
        let panel_elems: Vec<_> = model
            .elements
            .iter()
            .filter(|e| matches!(e.kind, ElementKind::PanelZone))
            .collect();
        assert_eq!(panel_elems.len(), 1);
        assert_eq!(panel_elems[0].nodes[0], NodeId(0), "先頭は接合部の節点");
        assert!(panel_elems[0].nodes.len() >= 2);
        assert_eq!(model.elements.len(), 3);
        let beam = &model.elements[0].rigid_zone;
        assert!((beam.panel_offset_i - 200.0).abs() < 1e-9);
        assert_eq!(beam.panel_offset_j, 0.0, "接合部でない端は 0");
        assert_eq!(beam.length_i, 0.0, "剛域長そのものは変えない");
        assert!((beam.rigid_length_i() - 200.0).abs() < 1e-9);
        let col = &model.elements[1].rigid_zone;
        assert!((col.panel_offset_j - 300.0).abs() < 1e-9);
        assert_eq!(col.panel_offset_i, 0.0);
        assert!((col.rigid_length_j() - 300.0).abs() < 1e-9);
        model.validate().expect("配列添字 == ElemId が保たれる");

        let first = model.elements.clone();
        apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(model.elements.len(), first.len());
        for (a, b) in model.elements.iter().zip(first.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.nodes, b.nodes);
            assert_eq!(a.rigid_zone.panel_offset_i, b.rigid_zone.panel_offset_i);
            assert_eq!(a.rigid_zone.panel_offset_j, b.rigid_zone.panel_offset_j);
        }
        model.validate().expect("不変条件");

        model.panel_zone = PanelZoneMode::None;
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert!(panels.is_empty());
        assert_eq!(model.elements.len(), 2);
        assert!(!model
            .elements
            .iter()
            .any(|e| matches!(e.kind, ElementKind::PanelZone)));
        assert!(
            model
                .elements
                .iter()
                .all(|e| e.rigid_zone.panel_offset_i == 0.0 && e.rigid_zone.panel_offset_j == 0.0),
            "モデル化を OFF にするとオフセットは消える"
        );
        model.validate().expect("不変条件");
    }

    /// パネル生成後に追加した部材があっても、再生成で ID 参照の整合が保たれる
    /// （パネルを除いて詰め直したうえで末尾へ再追加する）。
    #[test]
    fn test_regeneration_keeps_id_references_consistent() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(model.elements.len(), 3, "梁・柱・パネル");

        let mut far = model.nodes[1].clone();
        far.id = NodeId(3);
        far.coord[1] = 4000.0;
        model.nodes.push(far);
        let mut added = member(3, 1, 3, 0);
        added.id = ElemId(3);
        model.elements.push(added);
        model.girder_groups = vec![vec![ElemId(3)]];

        apply_auto_panel_zones(&mut model).unwrap();
        model.validate().expect("配列添字 == ElemId が保たれる");

        let added_idx = model
            .elements
            .iter()
            .position(|e| e.nodes.as_slice() == [NodeId(1), NodeId(3)])
            .expect("追加した部材");
        assert_eq!(model.elements[added_idx].id.index(), added_idx);
        assert_eq!(model.girder_groups[0][0], model.elements[added_idx].id);
    }

    /// RC 矩形柱断面。
    fn rc_shape(b: f64, d: f64) -> SectionShape {
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
        SectionShape::RcColumnRect {
            b,
            d,
            rebar: RcRectColumnRebar {
                main_dia: 25.0,
                x: vec![4],
                y: vec![4],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        }
    }

    /// RC 柱の接合部にはパネルを設けない（剛域と RC 柱梁接合部検定で扱う）。
    #[test]
    fn test_rc_column_gets_no_panel() {
        let rc = rc_shape(700.0, 700.0);
        let mut model = l_frame(rc);
        model.sections[1].material = Some(MaterialId(1));
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert!(panels.is_empty(), "RC 柱は対象外");
        assert_eq!(model.elements.len(), 2);
    }

    /// CFT 柱の接合部にはパネルを設けない（充填コンクリートと通しダイアフラムが
    /// 接合部のせん断挙動に関与し、鋼管のみの実効体積では剛性を表せないため）。
    /// 断面検定は CFT も対象に含めるため、検定側の判定とは分かれる。
    #[test]
    fn test_cft_column_is_not_modeling_target() {
        for shape in [
            SectionShape::CftBox {
                corner_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                thick: 16.0,
                horizontal_thick: 16.0,
            },
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
        ] {
            let mut model = l_frame(shape);
            let panels = apply_auto_panel_zones(&mut model).unwrap();
            assert!(panels.is_empty(), "CFT 柱はモデル化の対象外");
            assert_eq!(model.elements.len(), 2, "パネル要素は生成されない");

            let geom = PanelGeometry::from_column(&model.sections[1]).expect("諸元は解決できる");
            assert!(geom.filled, "モデル化対象ではない");
            assert!(geom.effective_volume(500.0) > 0.0, "検定用の Ve は求まる");
        }
    }

    /// 角形鋼管・円形鋼管（CFT でない S 造）はモデル化の対象になる。
    #[test]
    fn test_steel_tube_columns_are_modeling_targets() {
        for shape in [
            SectionShape::SteelBox {
                height: 400.0,
                width: 400.0,
                thick: 16.0,
                horizontal_thick: 16.0,
                corner_r: Some(0.0),
            },
            SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
        ] {
            let mut model = l_frame(shape);
            let panels = apply_auto_panel_zones(&mut model).unwrap();
            assert_eq!(panels.len(), 1, "S 造の鋼管柱はモデル化の対象");
        }
    }

    /// RC 梁が取り付く接合部にはパネルを設けない。柱が S でも接合部は RC になり、
    /// 鋼柱のウェブだけの実効体積では挙動を表せない。
    #[test]
    fn test_rc_beam_gets_no_panel_even_with_steel_column() {
        let mut model = l_frame_with_beam(
            h_shape(400.0, 400.0, 13.0, 21.0),
            rc_shape(400.0, 700.0),
            700.0,
        );
        model.sections[0].material = Some(MaterialId(1));
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert!(panels.is_empty(), "RC 梁の接合部は対象外");
        assert_eq!(model.elements.len(), 2, "パネル要素は生成されない");
    }

    /// S 梁と RC 梁が混在する接合部も対象外とする（1 本でも非 S があれば設けない）。
    #[test]
    fn test_mixed_beams_get_no_panel() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.nodes.push(Node {
            id: NodeId(3),
            coord: [0.0, 6000.0, 3000.0],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        model
            .sections
            .push(section_with_mat(2, rc_shape(400.0, 700.0), 700.0, 1));
        model.elements.push(member(2, 0, 3, 2));

        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert!(panels.is_empty(), "RC 梁が 1 本でも混じれば対象外");
    }

    #[test]
    fn panel_759_offsets_preserve_independent_inputs_and_lifecycle() {
        let mut model = l_frame(SectionShape::SteelBox {
            height: 400.0,
            width: 200.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        });
        model.nodes[1].coord = [0.0, 6000.0, 3000.0];
        model.elements[0].rigid_zone.length_i = 20.0;
        model.elements[0].rigid_zone.source_i = sepika_core::model::ZoneSource::Manual;
        model.elements[0].rigid_zone.face_i = Some(777.0);
        for _ in 0..2 {
            apply_auto_panel_zones(&mut model).unwrap();
            let zone = model.elements[0].rigid_zone;
            assert_eq!(zone.panel_offset_i, 100.0);
            assert_eq!(zone.rigid_length_i(), 100.0);
            assert_eq!(zone.length_i, 20.0);
            assert_eq!(zone.face_i, Some(777.0));
            model.validate().unwrap();
        }
        model.elements[1].local_axis.ref_vector = [0.0, 1.0, 0.0];
        assert!(model
            .validate()
            .unwrap_err()
            .to_string()
            .contains("必要値 200"));
        apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(model.elements[0].rigid_zone.panel_offset_i, 200.0);
        for disabled in [false, true] {
            let mut candidate = model.clone();
            if disabled {
                candidate.panel_zone = PanelZoneMode::None;
            } else {
                candidate.sections[1].shape = Some(SectionShape::CftBox {
                    height: 400.0,
                    width: 200.0,
                    thick: 12.0,
                    horizontal_thick: 12.0,
                    corner_r: Some(0.0),
                });
            }
            apply_auto_panel_zones(&mut candidate).unwrap();
            assert!(candidate
                .elements
                .iter()
                .all(|element| element.rigid_zone.panel_offset_i == 0.0
                    && element.rigid_zone.panel_offset_j == 0.0));
            assert_eq!(candidate.elements[0].rigid_zone.length_i, 20.0);
            assert_eq!(candidate.elements[0].rigid_zone.face_i, Some(777.0));
        }
        model.sections[1].shape = Some(h_shape(400.0, 200.0, 12.0, 20.0));
        model.elements[1].local_axis.ref_vector = [1.0, 0.0, 0.0];
        let before = bincode::serialize(&model).unwrap();
        let error = apply_auto_panel_zones(&mut model).unwrap_err();
        for text in ["接合部 ID 0", "柱 ID 1", "部材 ID 0", "接合詳細"] {
            assert!(error.contains(text), "{error}");
        }
        assert_eq!(bincode::serialize(&model).unwrap(), before);
    }

    #[test]
    fn panel_759_directional_rigid_connection_preserves_original_load_resultants_and_work() {
        use crate::behavior::ElementBehavior;
        use sepika_core::model::{MemberLoad, MemberLoadKind};
        let mut model = l_frame(SectionShape::SteelBox {
            height: 400.0,
            width: 200.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        });
        model.nodes[1].coord = [0.0, 6000.0, 3000.0];
        apply_auto_panel_zones(&mut model).unwrap();
        let data = &model.elements[0];
        assert_eq!(data.rigid_zone.panel_offset_i, 100.0);
        use crate::wall::side_column::{InPlaneReleasedColumn, ReleaseAxis};
        let wrapped = crate::frame::panel_offset::PanelOffsetMember::new(
            Box::new(InPlaneReleasedColumn::new(
                crate::frame::beam::BeamElement::new(data, &model),
                ReleaseAxis::LocalY,
            )),
            crate::frame::panel_offset::resolve(data, &model).unwrap(),
        );
        let loads = [
            MemberLoad::manual(
                data.id,
                [0.0, 0.0, -1.0],
                MemberLoadKind::Point { a: 50.0, p: 1000.0 },
            ),
            MemberLoad::manual(
                data.id,
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 6000.0,
                    w1: 2.0,
                    w2: 4.0,
                },
            ),
        ];
        let zero = wrapped
            .frame_load_response(&vec![0.0; wrapped.n_dof()], &loads)
            .unwrap()
            .unwrap();
        let residual = &zero.nodal_residual.data;
        assert!((residual[2] + residual[8] - 19000.0).abs() < 1e-8);
        assert!((residual[3] + residual[9] + 6000.0 * residual[8] - 60_050_000.0).abs() < 1e-6);
        let mut reference_data = data.clone();
        reference_data.rigid_zone.panel_offset_i = 0.0;
        reference_data.rigid_zone.length_i = 100.0;
        let reference = InPlaneReleasedColumn::new(
            crate::frame::beam::BeamElement::new(&reference_data, &model),
            ReleaseAxis::LocalY,
        );
        let mut displacement = vec![0.0; wrapped.n_dof()];
        displacement[12] = 0.001;
        displacement[13] = -0.002;
        let mut reference_displacement = [0.0; 12];
        reference_displacement[3] = -0.0005;
        reference_displacement[4] = 0.001;
        let actual = wrapped
            .frame_load_response(&displacement, &loads)
            .unwrap()
            .unwrap();
        let expected = reference
            .frame_load_response(&reference_displacement, &loads)
            .unwrap()
            .unwrap();
        for (actual, expected) in actual.nodal_residual.data[..12]
            .iter()
            .zip(&expected.nodal_residual.data)
        {
            assert!((actual - expected).abs() < 1e-9 * expected.abs().max(1.0));
        }
        let work: f64 = displacement
            .iter()
            .zip(&actual.nodal_residual.data)
            .map(|(u, f)| u * f)
            .sum();
        let reference_work: f64 = reference_displacement
            .iter()
            .zip(&expected.nodal_residual.data)
            .map(|(u, f)| u * f)
            .sum();
        assert!((work - reference_work).abs() < 1e-9 * reference_work.abs().max(1.0));
    }

    /// 斜材はパネルの接合面を持たないため、剛域長へパネルオフセットを加えない。
    #[test]
    fn test_diagonal_member_does_not_get_panel_offset() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.nodes.push(Node {
            id: NodeId(3),
            coord: [3000.0, 0.0, 6000.0],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        let mut brace_section = model.sections[0].clone();
        brace_section.id = SectionId(2);
        brace_section.frame_use = Some(FrameSectionUse::Brace);
        model.sections.push(brace_section);
        model.elements.push(member(2, 0, 3, 2));

        apply_auto_panel_zones(&mut model).unwrap();

        let diagonal = model
            .elements
            .iter()
            .find(|e| e.nodes.as_slice() == [NodeId(0), NodeId(3)])
            .expect("斜材");
        assert_eq!(
            diagonal.rigid_zone.panel_offset_i, 0.0,
            "斜材の i 端へパネルオフセットを設定しない"
        );
        assert_eq!(
            diagonal.rigid_zone.panel_offset_j, 0.0,
            "斜材の j 端へパネルオフセットを設定しない"
        );
        assert_eq!(diagonal.rigid_zone.rigid_length_i(), 0.0);
        assert_eq!(diagonal.rigid_zone.rigid_length_j(), 0.0);
    }

    /// 剛体アーム長は `max(剛域長, オフセット)`。手動指定が大きければそちらが効き、
    /// 小さくてもオフセットの分は確保される（接合位置は幾何的事実のため）。
    #[test]
    fn test_rigid_length_takes_larger_of_zone_and_offset() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.elements[0].rigid_zone.length_i = 500.0;
        model.elements[0].rigid_zone.source_i = sepika_core::model::ZoneSource::Manual;
        model.elements[1].rigid_zone.length_j = 10.0;
        model.elements[1].rigid_zone.source_j = sepika_core::model::ZoneSource::Manual;

        apply_auto_panel_zones(&mut model).unwrap();
        let beam = &model.elements[0].rigid_zone;
        assert_eq!(beam.length_i, 500.0, "手動指定はそのまま残る");
        assert!((beam.panel_offset_i - 200.0).abs() < 1e-9);
        assert_eq!(beam.rigid_length_i(), 500.0, "大きい方が剛体アーム長");

        let col = &model.elements[1].rigid_zone;
        assert_eq!(col.length_j, 10.0, "手動指定はそのまま残る");
        assert!(
            (col.rigid_length_j() - 300.0).abs() < 1e-9,
            "オフセットの方が大きければそちらが効く"
        );
    }

    /// 剛域の自動算定を単独で走らせてもオフセットは消えない。
    ///
    /// 増分解析・時刻歴・MCP のジョブは `apply_auto_rigid_zones` だけを呼ぶ経路が
    /// あるため、剛域長と同じフィールドへ入れると「パネル要素は残るのに剛体アームだけ
    /// 消えたモデル」で解析が走る。別フィールドに保持することで順序に依存しない。
    #[test]
    fn test_offsets_survive_rigid_zone_recomputation() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        apply_auto_panel_zones(&mut model).unwrap();
        let before: Vec<_> = model
            .elements
            .iter()
            .map(|e| (e.rigid_zone.panel_offset_i, e.rigid_zone.panel_offset_j))
            .collect();

        crate::frame::beam::apply_auto_rigid_zones(
            &mut model,
            &crate::frame::beam::RigidZoneRule::default(),
        );

        let after: Vec<_> = model
            .elements
            .iter()
            .map(|e| (e.rigid_zone.panel_offset_i, e.rigid_zone.panel_offset_j))
            .collect();
        assert_eq!(before, after, "剛域の再算定でオフセットが消えてはいけない");
        assert!((model.elements[0].rigid_zone.rigid_length_i() - 200.0).abs() < 1e-9);
    }

    #[test]
    fn lower_column_is_used_regardless_of_element_order() {
        let thin = h_shape(400.0, 400.0, 9.0, 21.0);
        let thick = h_shape(400.0, 400.0, 22.0, 21.0);

        let build = |upper_first: bool| {
            let mut model = l_frame(thick.clone());
            model.nodes.push(Node {
                id: NodeId(3),
                coord: [0.0, 0.0, 6000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
            let mut thin_section = section(2, thin.clone(), 400.0);
            thin_section.frame_use = Some(FrameSectionUse::Column);
            model.sections.push(thin_section);
            let upper = member(2, 0, 3, 2);
            if upper_first {
                model.elements.insert(0, upper);
                for (i, e) in model.elements.iter_mut().enumerate() {
                    e.id = ElemId(i as u32);
                }
            } else {
                model.elements.push(upper);
            }
            model
        };

        let mut a = build(true);
        let mut b = build(false);
        let pa = apply_auto_panel_zones(&mut a).unwrap();
        let pb = apply_auto_panel_zones(&mut b).unwrap();
        assert_eq!(pa.len(), 1);
        assert_eq!(pb.len(), 1);
        assert!(
            (pa[0].tp[0] - 22.0).abs() < 1e-9,
            "下側の柱を採る: tp={}",
            pa[0].tp[0]
        );
        assert_eq!(pa[0], pb[0], "要素の並び順に依存しない");
    }

    /// 梁が取り付かない節点（柱だけ）にはパネルを設けない。
    #[test]
    fn test_column_only_node_gets_no_panel() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.elements.remove(0);
        model.elements[0].id = ElemId(0);
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert!(panels.is_empty());
    }
    #[test]
    fn panel_504_shape_depth_and_manual_reason_survive_generation() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.sections[0].depth = 650.0;
        model.sections[0].shape = Some(h_shape(600.0, 200.0, 11.0, 20.0));
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(panels[0].db[0], 580.0);
        assert_eq!(panels[0].ve[0], 2_857_660.0);
        let data = model.elements.last().unwrap().clone();
        let manual = crate::springs::panel::PanelZone::new(&data, &model);
        assert_eq!(manual.db[0], 580.0);
        assert_eq!(manual.ve[0], 2_857_660.0);
        assert!((manual.k_panel[0] - 225_315_500_000.0).abs() < 1.0);
        model.sections[0].shape = Some(SectionShape::SteelPipe {
            outer_dia: 600.0,
            thick: 20.0,
        });
        let reason = resolve_panel_joint(&model, NodeId(0), &model.elements)
            .unwrap_err()
            .to_string();
        let manual = crate::springs::panel::PanelZone::new(&data, &model);
        assert_eq!(manual.resolution_error, Some(reason.clone()));
        assert!(model.validate().unwrap_err().to_string().contains(&reason));
        assert!(apply_auto_panel_zones(&mut model).unwrap().is_empty());
    }
    #[test]
    fn panel_504_built_h_beam_generates_575_mm_depth() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        model.sections[0].depth = 650.0;
        model.sections[0].shape = Some(SectionShape::SteelBuiltH {
            height: 600.0,
            upper_width: 200.0,
            upper_thick: 20.0,
            lower_width: 250.0,
            lower_thick: 30.0,
            web_thick: 12.0,
        });
        let panels = apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(panels[0].db[0], 575.0);
        assert_eq!(panels[0].ve[0], 2_833_025.0);
        let manual = crate::springs::panel::PanelZone::new(model.elements.last().unwrap(), &model);
        assert_eq!(manual.db[0], 575.0);
        assert_eq!(manual.ve[0], 2_833_025.0);
        assert!((manual.k_panel[0] - 223_373_125_000.0).abs() < 1.0);
    }
    #[test]
    fn panel_505_old_panel_is_rejected_and_regeneration_removes_unsupported_panel() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        apply_auto_panel_zones(&mut model).unwrap();
        let old = model.elements.last().unwrap().clone();
        let mut other = model.sections[0].clone();
        other.id = SectionId(2);
        other.shape = Some(h_shape(500.0, 200.0, 11.0, 17.0));
        model.sections.push(other);
        let mut far = model.nodes[1].clone();
        far.id = NodeId(3);
        far.coord[0] = -6000.0;
        model.nodes.push(far);
        let id = model.elements.len() as u32;
        model.elements.push(member(id, 3, 0, 2));
        let reason = resolve_panel_joint(&model, NodeId(0), &model.elements)
            .unwrap_err()
            .to_string();
        assert!(reason.contains("上位置差=50 mm"));
        assert!(model.validate().unwrap_err().to_string().contains(&reason));
        for panel in [
            crate::springs::panel::PanelZone::new(&old, &model),
            crate::springs::panel::PanelZone::new_nonlinear(&old, &model),
        ] {
            assert_eq!(panel.resolution_error, Some(reason.clone()));
        }
        assert!(apply_auto_panel_zones(&mut model).unwrap().is_empty());
        assert!(!model
            .elements
            .iter()
            .any(|e| e.kind == ElementKind::PanelZone));
        assert!(model
            .elements
            .iter()
            .all(|e| e.rigid_zone.panel_offset_i == 0.0 && e.rigid_zone.panel_offset_j == 0.0));
        model.validate().unwrap();
    }
    #[test]
    fn invalid_lower_column_fails_public_generation_without_changing_existing_panels() {
        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        let mut upper_node = model.nodes[2].clone();
        upper_node.id = NodeId(3);
        upper_node.coord[2] = 6000.0;
        model.nodes.push(upper_node);
        let mut upper_section = model.sections[1].clone();
        upper_section.id = SectionId(2);
        model.sections.push(upper_section);
        model.elements.push(member(2, 0, 3, 2));
        apply_auto_panel_zones(&mut model).unwrap();
        for missing_section in [false, true] {
            let mut invalid = model.clone();
            if missing_section {
                invalid.elements[1].section = None;
            } else {
                invalid.sections[1].material = None;
            }
            let before = format!("{invalid:?}");
            let reason = apply_auto_panel_zones(&mut invalid).unwrap_err();
            assert!(reason.contains("接合部 ID 0"), "{reason}");
            assert!(reason.contains("ID 1"), "{reason}");
            assert_eq!(format!("{invalid:?}"), before);
        }
    }

    #[test]
    fn panel_757_invalid_column_reference_rejects_generation_without_mutation() {
        use sepika_core::ids::LoadCaseId;
        use sepika_core::model::{LoadCase, LoadCaseKind, MemberLoad, MemberLoadKind};

        let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
        apply_auto_panel_zones(&mut model).unwrap();
        assert!(model.elements.iter().any(|element| {
            element.rigid_zone.panel_offset_i > 0.0 || element.rigid_zone.panel_offset_j > 0.0
        }));
        let mut far = model.nodes[1].clone();
        far.id = NodeId(3);
        far.coord[1] = 4_000.0;
        model.nodes.push(far);
        model.elements.push(member(3, 1, 3, 0));
        model.girder_groups = vec![vec![ElemId(3)]];
        model.set_member_hysteresis(ElemId(3), sepika_core::model::HysteresisModel::Standard);
        model.load_cases.push(LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "追加部材の参照".into(),
            kind: LoadCaseKind::Dead,
            nodal: Vec::new(),
            member: vec![MemberLoad::manual(
                ElemId(3),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Point {
                    a: 1_000.0,
                    p: 100.0,
                },
            )],
        });
        let nan = f64::from_bits(0x7ff8_0000_0000_0757);
        let mut references = Vec::new();
        for value in [nan, f64::INFINITY, f64::NEG_INFINITY] {
            for component in 0..3 {
                let mut reference = [1.0, 0.0, 0.0];
                reference[component] = value;
                references.push(reference);
            }
        }
        references.extend([[f64::MAX, 0.0, 0.0], [0.0, f64::MAX, 0.0]]);
        for reference in references {
            let mut invalid = model.clone();
            invalid.elements[1].local_axis.ref_vector = reference;
            let before = bincode::serialize(&invalid).unwrap();
            let reason = apply_auto_panel_zones(&mut invalid).unwrap_err();
            assert!(reason.contains("接合部 ID 0"), "{reason}");
            assert!(reason.contains("柱 ID 1"), "{reason}");
            assert!(reason.contains("参照方向"), "{reason}");
            assert_eq!(bincode::serialize(&invalid).unwrap(), before);
        }
    }

    #[test]
    fn beam_inapplicability_is_omitted_but_never_hides_invalid_lower_column() {
        for shape in [
            None,
            Some(h_shape(0.0, 200.0, 11.0, 20.0)),
            Some(SectionShape::SteelPipe {
                outer_dia: 600.0,
                thick: 20.0,
            }),
        ] {
            let mut model = l_frame(h_shape(400.0, 400.0, 13.0, 21.0));
            let mut upper_node = model.nodes[2].clone();
            upper_node.id = NodeId(3);
            upper_node.coord[2] = 6000.0;
            model.nodes.push(upper_node);
            let mut upper_section = model.sections[1].clone();
            upper_section.id = SectionId(2);
            model.sections.push(upper_section);
            model.elements.push(member(2, 0, 3, 2));
            model.sections[0].shape = shape;
            assert!(apply_auto_panel_zones(&mut model).unwrap().is_empty());
            assert!(!model
                .elements
                .iter()
                .any(|e| e.kind == ElementKind::PanelZone));
            model.sections[1].material = None;
            let reason = apply_auto_panel_zones(&mut model).unwrap_err();
            assert!(
                reason.contains("接合部 ID 0") && reason.contains("柱 ID 1"),
                "{reason}"
            );
        }
    }
}
