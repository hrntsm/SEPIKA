use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::*;
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape,
    StandardRebar,
};
use sepika_job::apply_rigid_zones_and_panels;

pub(super) const CONCRETE_E_MPA: f64 = 23_000.0;
pub(super) const CONCRETE_POISSON: f64 = 0.2;
pub(super) const BEAM_D_MM: f64 = 650.0;
pub(super) const COLUMN_B_MM: f64 = 450.0;
pub(super) const COLUMN_H_MM: f64 = 3_000.0;

fn material(id: u32, name: &str, category: MaterialCategory) -> Material {
    let standard = sepika_core::standard_material::standard_material_properties(name).unwrap();
    Material {
        id: MaterialId(id),
        name: name.into(),
        category,
        young: standard.young,
        poisson: standard.poisson,
        density: standard.density,
        shear: None,
        fc: standard.fc,
        fy: standard.fy,
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn section(id: u32, name: &str, shape: SectionShape) -> Section {
    let mut section = shape.to_section(SectionId(id), name.into());
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.frame_use = section.rc_frame_use();
    section
}

pub(super) fn frame(
    total_n: f64,
    spans_mm: [f64; 2],
    beam_b_mm: f64,
    bars: [StandardRebar; 2],
) -> Model {
    let right_x_mm = spans_mm.iter().sum();
    let nodes = [
        [0.0, 0.0, 0.0],
        [right_x_mm, 0.0, 0.0],
        [0.0, 0.0, COLUMN_H_MM],
        [spans_mm[0], 0.0, COLUMN_H_MM],
        [right_x_mm, 0.0, COLUMN_H_MM],
    ]
    .into_iter()
    .enumerate()
    .map(|(index, coord)| {
        let mut restraint = Dof6Mask::FIXED;
        if index >= 2 {
            restraint.set_free(Dof::Uz);
        }
        Node {
            id: NodeId(index as u32),
            coord,
            restraint,
            mass: (index >= 2).then_some([0.0, 0.0, 2.0, 0.0, 0.0, 0.0]),
            story: None,
            support_spring: None,
        }
    })
    .collect();
    let beam = |main_dia| SectionShape::RcBeamRect {
        b: beam_b_mm,
        d: BEAM_D_MM,
        rebar: RcBeamRebar {
            main_dia: RebarSize::Standard(main_dia),
            top: vec![4],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: RebarSize::Standard(StandardRebar::D13),
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    let column = SectionShape::RcColumnRect {
        b: COLUMN_B_MM,
        d: COLUMN_B_MM,
        rebar: RcRectColumnRebar {
            main_dia: RebarSize::Standard(StandardRebar::D25),
            x: vec![4],
            y: vec![4],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: RebarSize::Standard(StandardRebar::D13),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let mut concrete = material(0, "FC24", MaterialCategory::Concrete);
    concrete.young = CONCRETE_E_MPA;
    concrete.poisson = CONCRETE_POISSON;
    let mut model = Model {
        nodes,
        elements: [[2, 3], [3, 4], [0, 2], [1, 4]]
            .into_iter()
            .enumerate()
            .map(|(i, ends)| ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: ends.map(NodeId).into_iter().collect(),
                section: Some(SectionId(i.min(2) as u32)),
                local_axis: LocalAxis {
                    ref_vector: if i < 2 {
                        [0.0, 0.0, 1.0]
                    } else {
                        [1.0, 0.0, 0.0]
                    },
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::UniaxialBendingShear,
                rigid_zone: RigidZone {
                    source_i: ZoneSource::Manual,
                    source_j: ZoneSource::Manual,
                    ..Default::default()
                },
                end_offsets_mm: [[0.0; 3]; 2],
                plastic_zone: None,
                spring: None,
            })
            .collect(),
        sections: vec![
            section(0, "G1", beam(bars[0])),
            section(1, "G2", beam(bars[1])),
            section(2, "C1", column),
        ],
        materials: vec![concrete, material(1, "SD345", MaterialCategory::Rebar)],
        mass_method: MassMethod::LumpedOnly,
        panel_zone: PanelZoneMode::None,
        ..Default::default()
    };
    for id in [ElemId(0), ElemId(1)] {
        model.set_member_rc_beam_reference(
            id,
            Some(RcBeamReference::AntisymmetricHalfMemberWithoutAxialForce),
        );
    }
    apply_rigid_zones_and_panels(&mut model).unwrap();
    // 自動生成の梁自重は材間荷重を生じ、等せん断・逆対称曲げの接続条件を外れる。
    model.load_cases = [(LoadCaseKind::Dead, 0.8), (LoadCaseKind::Live, 0.2)]
        .into_iter()
        .enumerate()
        .map(|(i, (kind, fraction))| LoadCase {
            id: LoadCaseId(i as u32),
            name: [DL_CASE_NAME, LL_FRAME_CASE_NAME][i].into(),
            kind,
            origin: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(3),
                [0.0, 0.0, -total_n * fraction, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        })
        .collect();
    model.combinations = vec![LoadCombination {
        name: "常時".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
    }];
    model.validate().unwrap();
    model
}
