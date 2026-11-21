use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
    Model, Node, Section, Story,
};
use sepika_core::section_shape::SectionShape;

/// 基部の床（階の列の先頭）。階は床であり、その先頭が基部であることは
/// `Model::layers` が依拠する不変条件のため、テストのモデルにも必ず置く。
pub(super) fn base_story(node_ids: Vec<NodeId>) -> Story {
    Story {
        wall_weights: Vec::new(),
        level_kind: Default::default(),
        structure: Default::default(),
        id: StoryId(0),
        name: "1F".to_string(),
        elevation: Some(0.0),
        sl_offset_mm: Some(0.0),
        node_ids,
        seismic_weight: None,
        weight_override: None,
        dynamic_mass: None,
        standard_floor_load: None,
        column_finish_area_weight: 0.0,
        fireproof: Default::default(),
    }
}

/// 1層・鉛直ファイバ柱の片持ちプッシュオーバー（最小統合テスト）。
/// 配線済み非線形要素（FiberBeam）＋座標変換＋NR 反復＋降伏追跡が
/// エンドツーエンドで動作することを検証する。
pub(crate) fn single_column_model(fy: f64, seismic_weight: f64) -> Model {
    Model {
        load_cases: sepika_core::model::default_load_cases(),
        combinations: sepika_core::model::default_combinations(),
        ultimate_gravity_combination: Some(0),
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask(0b100000),
                mass: None,
                story: Some(StoryId(1)),
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Fiber,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![Section {
            frame_use: None,
            id: SectionId(0),
            name: "col".to_string(),
            area: 10000.0,
            iy: 8.333e6,
            iz: 8.333e6,
            j: 1.0e6,
            depth: 100.0,
            width: 100.0,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        }],
        materials: vec![Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: "steel".to_string(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(fy),
        }],
        stories: vec![
            base_story(vec![]),
            Story {
                wall_weights: Vec::new(),
                level_kind: Default::default(),
                structure: Default::default(),
                id: StoryId(1),
                name: "2F".to_string(),
                elevation: Some(3000.0),
                sl_offset_mm: Some(0.0),
                node_ids: vec![NodeId(1)],
                seismic_weight: Some(seismic_weight),
                weight_override: None,
                dynamic_mass: None,
                standard_floor_load: None,
                column_finish_area_weight: 0.0,
                fireproof: Default::default(),
            },
        ],
        ..Default::default()
    }
}

/// RC 矩形断面と SD345 の主筋を持つ片持ち柱モデル。
pub(super) fn rc_hinge_model() -> (
    Model,
    sepika_core::section_shape::RcRectColumnRebar,
    f64,
    f64,
) {
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
    let rebar = RcRectColumnRebar {
        main_dia: sepika_core::section_shape::RebarSize::DiameterMm(25.0),
        x: vec![5],
        y: vec![3],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    let (b, d) = (400.0, 600.0);
    let shape = SectionShape::RcColumnRect {
        b,
        d,
        rebar: rebar.clone(),
    };
    let mut sec = shape.to_section(SectionId(0), "RC-400x600".into());
    sec.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(2));
    let mat = Material {
        strength_factor: None,
        concrete_class: Default::default(),
        id: MaterialId(0),
        name: "rc".to_string(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 0.0,
        shear: None,
        fc: Some(24.0),
        fy: None,
    };
    let rebar_mat = Material {
        id: MaterialId(1),
        name: "SD345".to_string(),
        category: MaterialCategory::Rebar,
        young: 205000.0,
        poisson: 0.3,
        fc: None,
        fy: Some(345.0),
        ..mat.clone()
    };
    let shear_mat = Material {
        id: MaterialId(2),
        name: "SD295A".to_string(),
        fy: Some(295.0),
        ..rebar_mat.clone()
    };
    let model = Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 3000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Fiber,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![sec],
        materials: vec![mat, rebar_mat, shear_mat],
        ..single_column_model(235.0, 1_000_000.0)
    };
    (model, rebar, b, d)
}
