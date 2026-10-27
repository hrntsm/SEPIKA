use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::*;
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::{Ctx, LocalVec};
use sepika_element::factory::{build_behavior, StrengthBasis};
use sepika_element::wall::wall_element::WallElement;
use sepika_solver::statics::linear::{linear_static_once, StaticOnce};

#[path = "../../../test_fixtures/rc_wall617.rs"]
mod rc_wall617;

const KINDS: [ElementKind; 3] = [
    ElementKind::Beam,
    ElementKind::Fiber,
    ElementKind::MultiSpring,
];

#[test]
#[ignore = "Issue821の現行モデル監査。開口モデルの妥当性を保証する回帰試験ではない"]
fn audit_rc_wall_opening_linear_response() {
    let opening = |width, height, x, z| WallOpening {
        width,
        height,
        offset: Some([x, z]),
    };
    let cases = [
        ("solid", vec![]),
        ("center", vec![opening(1000.0, 1200.0, 1500.0, 900.0)]),
        ("left", vec![opening(1000.0, 1200.0, 300.0, 900.0)]),
        ("low", vec![opening(1000.0, 1200.0, 1500.0, 300.0)]),
        ("wide", vec![opening(2000.0, 600.0, 1000.0, 1200.0)]),
        ("tall", vec![opening(600.0, 2000.0, 1700.0, 500.0)]),
        (
            "double_close",
            vec![
                opening(500.0, 1200.0, 800.0, 900.0),
                opening(500.0, 1200.0, 1700.0, 900.0),
            ],
        ),
        (
            "double",
            vec![
                opening(500.0, 1200.0, 800.0, 900.0),
                opening(500.0, 1200.0, 2700.0, 900.0),
            ],
        ),
    ];
    for columns in [true, false] {
        for mode in [
            MultiOpeningMode::Equivalent,
            MultiOpeningMode::Envelope,
            MultiOpeningMode::Auto,
        ] {
            for (name, openings) in &cases {
                let mut horizontal = fixture(ElementKind::Beam, Dof::Ux, 100_000.0);
                horizontal.multi_opening_mode = mode;
                horizontal.wall_attrs = vec![WallAttr {
                    elem: ElemId(4),
                    opening_area: 0.0,
                    opening_weight: 0.0,
                    slit: Default::default(),
                    openings: openings.clone(),
                    finish_intensity: 0.0,
                }];
                if !columns {
                    horizontal.elements.retain(|e| e.id.0 >= 2);
                }
                let lateral = linear_static_once(&horizontal, LoadCaseId(0)).unwrap();
                let lateral_wall = wall_nodal_force(&horizontal, &lateral);
                let mut axial = horizontal.clone();
                for node in &mut axial.nodes {
                    node.restraint = Dof6Mask::FIXED;
                    if node.id.0 >= 2 {
                        node.restraint.set_free(Dof::Uz);
                    }
                }
                axial.constraints = vec![Constraint::RigidLink {
                    master: NodeId(2),
                    slaves: vec![NodeId(3)],
                    dofs: Dof6Mask(1 << Dof::Uz as u8),
                }];
                for load in &mut axial.load_cases[0].nodal {
                    load.values = [0.0, 0.0, 50_000.0, 0.0, 0.0, 0.0];
                }
                let vertical = linear_static_once(&axial, LoadCaseId(0)).unwrap();
                if columns {
                    for (direct_model, direct_result, dof) in
                        [(&horizontal, &lateral, 0), (&axial, &vertical, 2)]
                    {
                        let prepared = opening_audit_plate_expansion(direct_model);
                        let actual = linear_static_once(&prepared, LoadCaseId(0)).unwrap();
                        let actual_forces = element_nodal_forces(&prepared, &actual);
                        let wall_force = &actual_forces[4];
                        println!("prepared mode={mode:?}, case={name}, dof={:?}, displacement_mm={:.12}, wall_force_N={:.6}, column_force_N={:.6}",
                            dof,
                            actual.disp[2][dof],
                            wall_force[2][dof] + wall_force[3][dof],
                            actual_forces[0][2][dof] + actual_forces[1][3][dof]);
                        let wall = prepared
                            .elements
                            .iter()
                            .find(|e| e.id == ElemId(4))
                            .unwrap();
                        if !wall_is_seismic(wall, &prepared) {
                            let support_n: f64 = actual_forces
                                .iter()
                                .flat_map(|e| &e[..2])
                                .map(|f| f[dof])
                                .sum();
                            close(support_n, -100_000.0, 0.01);
                            for beam in &actual_forces[2..4] {
                                for node in beam {
                                    for (d, &force) in node.iter().enumerate() {
                                        close(force, 0.0, if d < 3 { 0.01 } else { 0.1 });
                                    }
                                }
                            }
                            continue;
                        }
                        for (actual, expected) in actual.disp.iter().zip(&direct_result.disp) {
                            for (&actual, &expected) in actual.iter().zip(expected) {
                                close(actual, expected, 1e-10);
                            }
                        }
                        let expected_forces = element_nodal_forces(direct_model, direct_result);
                        for (actual, expected) in actual_forces.iter().zip(&expected_forces) {
                            for (actual, expected) in actual.iter().zip(expected) {
                                for (d, (&actual, &expected)) in
                                    actual.iter().zip(expected).enumerate()
                                {
                                    close(actual, expected, if d < 3 { 0.01 } else { 0.1 });
                                }
                            }
                        }
                    }
                }
                let axial_wall = wall_nodal_force(&axial, &vertical);
                let lateral_forces = element_nodal_forces(&horizontal, &lateral);
                let axial_forces = element_nodal_forces(&axial, &vertical);
                let reaction_x_n: f64 = lateral_forces
                    .iter()
                    .flat_map(|e| &e[..2])
                    .map(|f| f[0])
                    .sum();
                let reaction_z_n: f64 = axial_forces
                    .iter()
                    .flat_map(|e| &e[..2])
                    .map(|f| f[2])
                    .sum();
                close(reaction_x_n, -100_000.0, 0.01);
                close(reaction_z_n, -100_000.0, 0.01);
                let column_q_n = lateral_forces[0][2][0] + lateral_forces[1][3][0];
                let column_n_n = axial_forces[0][2][2] + axial_forces[1][3][2];
                let full = DofMap::build(&horizontal).including_restrained_node_dofs();
                let wall = horizontal
                    .elements
                    .iter()
                    .find(|e| e.id == ElemId(4))
                    .unwrap();
                let seismic = wall_is_seismic(wall, &horizontal);
                let behavior = build_behavior(wall, &horizontal);
                let mode_u: Vec<_> = behavior
                    .global_dofs(&full)
                    .iter()
                    .map(|&active| {
                        let g = full.global(active as u32);
                        match (g / 6, g % 6) {
                            (2 | 3, 0) => 0.15,
                            (2, 2) => 0.2,
                            (3, 2) => -0.2,
                            (2 | 3, 4) => 1e-4,
                            _ => 0.0,
                        }
                    })
                    .collect();
                let stiffness = behavior.tangent_stiffness(&Ctx { model: &horizontal });
                let mut bending_work_n_mm = 0.0;
                for (i, &ui) in mode_u.iter().enumerate() {
                    for (j, &uj) in mode_u.iter().enumerate() {
                        bending_work_n_mm += ui * stiffness.get(i, j) * uj;
                    }
                }
                let axial_area_mm2 =
                    (axial_wall[2][2] + axial_wall[3][2]) / vertical.disp[2][2] * 3000.0 / 30_000.0;
                let bending_i_mm4 = bending_work_n_mm * 3000.0 / (30_000.0 * 1e-8);
                println!("plate_expansion_checked={columns}, inferred_wall_A_mm2={axial_area_mm2:.6}, inferred_wall_I_mm4={bending_i_mm4:.6}");
                println!("columns={columns}, mode={mode:?}, case={name}, seismic={seismic}, ux_mm={:.12}, uz_mm={:.12}, wall_Q_N={:.6}, wall_N_N={:.6}, column_Q_N={column_q_n:.6}, column_N_N={column_n_n:.6}, reaction_X_N={reaction_x_n:.6}, reaction_Z_N={reaction_z_n:.6}, axial_K_N_mm={:.6}, bending_uKu_N_mm={:.6}", lateral.disp[2][0], vertical.disp[2][2], lateral_wall[2][0]+lateral_wall[3][0], axial_wall[2][2]+axial_wall[3][2], 100_000.0/vertical.disp[2][2], bending_work_n_mm);
            }
        }
    }
}

fn opening_audit_plate_expansion(direct: &Model) -> Model {
    use sepika_core::ids::{WallPlateId, WallRegionId};
    use sepika_core::wall_expand::expand_wall_elements;

    let mut input = direct.clone();
    let wall = input.elements.pop().unwrap();
    assert_eq!(wall.id, ElemId(4));
    let attr = input.wall_attrs.pop().unwrap();
    let boundary = wall.nodes.to_vec();
    let plate = input.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: wall.section,
            dl_support: Some(WallDlSupport::LowerBeam),
            self_weight_shares: Vec::new(),
            opening_area: attr.opening_area,
            opening_weight: attr.opening_weight,
            openings: attr.openings,
            loads: Vec::new(),
            slit: attr.slit,
        },
    );
    input.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "開口監査".into(),
        boundary,
        wall_plate_ids: vec![plate],
        posts: Vec::new(),
    });
    assert_eq!(input.elements.len(), 4);
    let (expanded, _, report) = expand_wall_elements(&input);
    assert_eq!(report.generated, 1);
    assert_eq!(
        expanded.generated_wall_origins.get(&ElemId(4)),
        Some(&plate)
    );
    expanded
}

#[test]
fn rc_wall_capacity_uses_both_physical_columns_for_each_representation() {
    for kind in KINDS {
        let mut model = fixture(kind, Dof::Ux, 100_000.0);
        let index = WallSideColumnIndex::build(&model);
        let bindings = index.wall_replacement_bindings(ElemId(4)).unwrap();
        assert_eq!(bindings.len(), 2);
        for side in 0..2 {
            let binding = bindings.iter().find(|b| b.side == side).unwrap();
            assert_eq!(binding.column, ElemId(side as u32));
            assert_eq!(binding.role, WallSideColumnRole::RcSrcReplacement);
            assert_eq!(binding.frame_use, Some(FrameSectionUse::Column));
        }
        // D=4400、d=4200、j=3675、A=1040000、te=236.36363636363637、左右8D20、N=0。
        for (method, expected_n) in [
            (RcWallShearMethod::ArakawaMin, 1_841_981.398_546_544_4),
            (RcWallShearMethod::ArakawaMean, 2_292_376.310_855_940_4),
        ] {
            model.stress_cfg.rc_wall_shear_method = method;
            let qu_n = WallElement::directional_shear_capacity_at_reference(
                &model.elements[4],
                &model,
                StrengthBasis::Nominal,
                0.0,
                [1.0, 1.0],
            )
            .unwrap();
            close(qu_n[0], expected_n, 1e-6);
            close(qu_n[1], expected_n, 1e-6);
        }
    }
}

#[test]
fn rc_wall_capacity_keeps_explicit_local_directional_span_ratios() {
    for kind in KINDS {
        let mut model = fixture(kind, Dof::Ux, 100_000.0);
        for (method, expected_n) in [
            (
                RcWallShearMethod::ArakawaMin,
                [1_841_981.398_546_544_4, 1_248_245.154_081_532],
            ),
            (
                RcWallShearMethod::ArakawaMean,
                [2_292_376.310_855_940_4, 1_825_520.720_255_426],
            ),
        ] {
            model.stress_cfg.rc_wall_shear_method = method;
            let qu_n = WallElement::directional_shear_capacity_at_reference(
                &model.elements[4],
                &model,
                StrengthBasis::Nominal,
                0.0,
                [1.0, 2.0],
            )
            .unwrap();
            close(qu_n[0], expected_n[0], 1e-6);
            close(qu_n[1], expected_n[1], 1e-6);
        }
    }
}

fn fixture(kind: ElementKind, dof: Dof, total_force_n: f64) -> Model {
    let mut model = rc_wall617::expanded_model();
    model.materials[0].young = 30_000.0;
    model.materials[0].shear = Some(12_500.0);
    model.materials[1].name = "SD295A".into();
    model.materials[1].fy = Some(295.0);
    for section in &mut model.sections {
        let mut shape = section.shape.clone().unwrap();
        match &mut shape {
            SectionShape::RcColumnRect { b, d, rebar } => {
                *b = 400.0;
                *d = 400.0;
                rebar.main_dia = 20.0;
                rebar.x = vec![3];
                rebar.y = vec![3];
            }
            SectionShape::RcBeamRect { b, d, .. } => {
                *b = 400.0;
                *d = 400.0;
            }
            SectionShape::RcWall {
                thickness,
                ps,
                pwh_ratio,
                pwv_ratio,
            } => {
                *thickness = 200.0;
                *ps = 0.0025;
                *pwh_ratio = Some(0.0025);
                *pwv_ratio = Some(0.0025);
            }
            _ => unreachable!(),
        }
        let mut resized = shape.to_section(section.id, section.name.clone());
        resized.frame_use = section.frame_use;
        resized.material = section.material;
        resized.rebar_material = section.rebar_material;
        resized.shear_rebar_material = section.shear_rebar_material;
        *section = resized;
    }
    for element in &mut model.elements {
        if element.id.0 < 2 {
            element.kind = kind;
        }
        element.rigid_zone = RigidZone {
            length_i: 0.0,
            length_j: 0.0,
            source_i: ZoneSource::Manual,
            source_j: ZoneSource::Manual,
            panel_offset_i: 0.0,
            panel_offset_j: 0.0,
            face_i: Some(200.0),
            face_j: Some(200.0),
        };
    }
    for node in &mut model.nodes {
        node.mass = None;
        node.restraint = Dof6Mask::FIXED;
        if node.id.0 >= 2 {
            node.restraint.set_free(dof);
        }
    }
    for story in &mut model.stories {
        story.seismic_weight = None;
        story.weight_override = None;
    }
    model.panel_zone = PanelZoneMode::None;
    model.stress_cfg.no_long_axial_column = false;
    model.stress_cfg.no_long_axial_brace = false;
    model.constraints = vec![Constraint::RigidLink {
        master: NodeId(2),
        slaves: vec![NodeId(3)],
        dofs: Dof6Mask(1 << dof as u8),
    }];
    let mut force = [0.0; 6];
    force[dof as usize] = total_force_n / 2.0;
    model.load_cases = vec![LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: "独立1自由度参照".into(),
        kind: LoadCaseKind::Other,
        nodal: vec![
            NodalLoad::manual(NodeId(2), force),
            NodalLoad::manual(NodeId(3), force),
        ],
        member: vec![],
    }];
    model.combinations.clear();
    model
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        actual.is_finite() && (actual - expected).abs() <= tolerance,
        "actual={actual:.16e}, expected={expected:.16e}, tolerance={tolerance:.3e}"
    );
}

fn check_displacements(result: &StaticOnce, dof: Dof, displacement_mm: f64) {
    assert_eq!(result.disp.len(), 4);
    for (node, disp) in result.disp.iter().enumerate() {
        for (component, value) in disp.iter().enumerate() {
            let expected = if node >= 2 && component == dof as usize {
                displacement_mm
            } else {
                0.0
            };
            close(*value, expected, 1e-10);
        }
    }
}

fn projected_stiffness(model: &Model, dof: Dof) -> f64 {
    let full = DofMap::build(model).including_restrained_node_dofs();
    let mut total = 0.0;
    for element in &model.elements {
        let behavior = build_behavior(element, model);
        let global_dofs = behavior.global_dofs(&full);
        let mode: Vec<_> = global_dofs
            .iter()
            .map(|&active| {
                let global = full.global(active as u32);
                f64::from(global / 6 >= 2 && global % 6 == dof as usize)
            })
            .collect();
        let k = behavior.tangent_stiffness(&Ctx { model });
        for (i, &ui) in mode.iter().enumerate() {
            for (j, &uj) in mode.iter().enumerate() {
                total += ui * k.get(i, j) * uj;
            }
        }
    }
    total
}

fn wall_nodal_force(model: &Model, result: &StaticOnce) -> [[f64; 6]; 4] {
    let full = DofMap::build(model).including_restrained_node_dofs();
    let wall = model.elements.iter().find(|e| e.id == ElemId(4)).unwrap();
    let normalized = match wall.nodes.as_slice() {
        [NodeId(0), NodeId(1), NodeId(3), NodeId(2)] => [0, 1, 2, 3],
        [NodeId(1), NodeId(0), NodeId(2), NodeId(3)] => [1, 0, 3, 2],
        other => panic!("未定義の壁周回: {other:?}"),
    };
    let mut behavior = build_behavior(wall, model);
    let global_dofs = behavior.global_dofs(&full);
    let physical: Vec<_> = global_dofs
        .iter()
        .map(|&active| full.global(active as u32))
        .collect();
    let expected: Vec<_> = normalized
        .iter()
        .flat_map(|node| (0..6).map(move |d| node * 6 + d))
        .collect();
    assert_eq!(physical, expected);
    let u = LocalVec {
        data: physical
            .iter()
            .map(|&g| result.disp[g / 6][g % 6])
            .collect(),
    };
    behavior.update_state(&u, false, &Ctx { model }).unwrap();
    let force = behavior.internal_force(&Ctx { model });
    assert_eq!(force.data.len(), physical.len());
    let mut nodal = [[0.0; 6]; 4];
    for (&global, &value) in physical.iter().zip(force.data.iter()) {
        nodal[global / 6][global % 6] += value;
    }
    nodal
}

fn element_nodal_forces(model: &Model, result: &StaticOnce) -> [[[f64; 6]; 4]; 5] {
    let full = DofMap::build(model).including_restrained_node_dofs();
    let mut forces = [[[0.0; 6]; 4]; 5];
    for element in &model.elements {
        if element.id == ElemId(4) {
            forces[4] = wall_nodal_force(model, result);
            continue;
        }
        let mut behavior = build_behavior(element, model);
        let physical: Vec<_> = behavior
            .global_dofs(&full)
            .iter()
            .map(|&active| full.global(active as u32))
            .collect();
        let expected: Vec<_> = element
            .nodes
            .iter()
            .flat_map(|node| (0..6).map(move |d| node.index() * 6 + d))
            .collect();
        assert_eq!(physical, expected);
        let u: Vec<_> = physical
            .iter()
            .map(|&g| result.disp[g / 6][g % 6])
            .collect();
        let nodal_force = if let Some(response) = behavior.frame_load_response(&u, &[]).unwrap() {
            for (i, &global) in physical.iter().enumerate() {
                close(
                    response.nodal_residual.data[i],
                    response.physical_frame_end_force_global[i],
                    if global % 6 < 3 { 1e-5 } else { 0.01 },
                );
            }
            response.physical_frame_end_force_global.to_vec()
        } else {
            behavior
                .update_state(&LocalVec { data: u.into() }, false, &Ctx { model })
                .unwrap();
            behavior.internal_force(&Ctx { model }).data.to_vec()
        };
        for (i, &global) in physical.iter().enumerate() {
            forces[element.id.index()][global / 6][global % 6] += nodal_force[i];
        }
    }
    forces
}

fn check_lateral_reactions(
    model: &Model,
    forces: &[[[f64; 6]; 4]; 5],
    total_force_n: f64,
    wall_force_n: f64,
    column_force_n: f64,
) {
    let mut reactions = [[0.0; 6]; 4];
    for element in forces {
        for (reaction, force) in reactions.iter_mut().zip(element) {
            for (r, f) in reaction.iter_mut().zip(force) {
                *r += f;
            }
        }
    }
    for load in &model.load_cases[0].nodal {
        for (r, value) in reactions[load.node.index()].iter_mut().zip(load.values) {
            *r -= value;
        }
    }
    let mut balance = [total_force_n, 0.0, 0.0, 0.0, total_force_n * 3000.0, 0.0];
    for (node, (reaction, data)) in reactions.iter().zip(&model.nodes).enumerate() {
        let mut expected = [0.0; 6];
        if node < 2 {
            expected[0] = -total_force_n / 2.0;
        }
        expected[2] =
            if node % 2 == 0 { -1.0 } else { 1.0 } * wall_force_n * 3000.0 / (2.0 * 4000.0);
        expected[4] = -column_force_n * 3000.0 / 2.0;
        for d in 0..6 {
            close(reaction[d], expected[d], if d < 3 { 1e-5 } else { 0.01 });
            balance[d] += reaction[d];
        }
        let [x, y, z] = data.coord;
        balance[3] += y * reaction[2] - z * reaction[1];
        balance[4] += z * reaction[0] - x * reaction[2];
        balance[5] += x * reaction[1] - y * reaction[0];
    }
    for (d, value) in balance.iter().enumerate() {
        close(*value, 0.0, if d < 3 { 1e-5 } else { 0.01 });
    }
}

fn lateral_work(kind: ElementKind, steel: bool) -> [f64; 5] {
    let mut previous_disp = [[0.0; 6]; 4];
    let mut previous_forces = [[[0.0; 6]; 4]; 5];
    let mut previous_load = [[0.0; 6]; 4];
    let mut work_n_mm = [0.0; 5];
    let mut external_work_n_mm = 0.0;
    for total_force_n in [0.0, 50_000.0, 100_000.0] {
        let model = if steel {
            steel_fixture(kind, total_force_n)
        } else {
            fixture(kind, Dof::Ux, total_force_n)
        };
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        let forces = element_nodal_forces(&model, &result);
        let mut nodal_load = [[0.0; 6]; 4];
        for load in &model.load_cases[0].nodal {
            for (value, force) in nodal_load[load.node.index()].iter_mut().zip(load.values) {
                *value += force;
            }
        }
        if total_force_n == 0.0 {
            for element in &forces {
                for node in element {
                    for value in node {
                        close(*value, 0.0, 1e-10);
                    }
                }
            }
        }
        for e in 0..5 {
            for node in 0..4 {
                for d in 0..6 {
                    work_n_mm[e] += 0.5
                        * (previous_forces[e][node][d] + forces[e][node][d])
                        * (result.disp[node][d] - previous_disp[node][d]);
                }
            }
        }
        for node in 0..4 {
            for d in 0..6 {
                external_work_n_mm += 0.5
                    * (previous_load[node][d] + nodal_load[node][d])
                    * (result.disp[node][d] - previous_disp[node][d]);
            }
        }
        previous_disp.copy_from_slice(&result.disp);
        previous_forces = forces;
        previous_load = nodal_load;
    }
    close(work_n_mm.iter().sum(), external_work_n_mm, 1e-6);
    work_n_mm
}

fn steel_fixture(kind: ElementKind, total_force_n: f64) -> Model {
    let mut model = fixture(kind, Dof::Ux, total_force_n);
    let steel = &mut model.materials[0];
    steel.name = "人工鋼材".into();
    steel.category = MaterialCategory::Steel;
    steel.young = 205_000.0;
    steel.shear = Some(80_000.0);
    steel.poisson = 0.28125;
    steel.density = 7.85e-9;
    steel.fc = None;
    steel.fy = Some(235.0);
    for section in &mut model.sections {
        let mut replacement = SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 400.0,
        }
        .to_section(section.id, section.name.clone());
        replacement.frame_use = section.frame_use;
        replacement.material = section.material;
        if section.id.index() == 2 {
            replacement.shape = None;
            replacement.thickness = Some(10.0);
            replacement.width = 10.0;
            replacement.depth = 0.0;
            replacement.area = 0.0;
            replacement.iy = 0.0;
            replacement.iz = 0.0;
            replacement.j = 0.0;
            replacement.as_y = 0.0;
            replacement.as_z = 0.0;
            replacement.property_basis = Default::default();
        }
        *section = replacement;
    }
    model
}

fn rotate_z90(values: [f64; 6], rotated: bool) -> [f64; 6] {
    if rotated {
        [
            -values[1], values[0], values[2], -values[4], values[3], values[5],
        ]
    } else {
        values
    }
}

fn orient_fixture(model: &mut Model, rotated: bool, reversed_wall: bool, reversed_columns: bool) {
    if reversed_wall {
        model
            .elements
            .iter_mut()
            .find(|e| e.id == ElemId(4))
            .unwrap()
            .nodes = [NodeId(1), NodeId(0), NodeId(2), NodeId(3)]
            .into_iter()
            .collect();
        model.elements.swap(0, 1);
    }
    for element in &mut model.elements {
        if reversed_columns && element.id.index() < 2 {
            element.nodes.swap(0, 1);
            element.end_cond.swap(0, 1);
            let zone = &mut element.rigid_zone;
            std::mem::swap(&mut zone.length_i, &mut zone.length_j);
            std::mem::swap(&mut zone.source_i, &mut zone.source_j);
            std::mem::swap(&mut zone.panel_offset_i, &mut zone.panel_offset_j);
            std::mem::swap(&mut zone.face_i, &mut zone.face_j);
        }
        if rotated {
            let [x, y, z] = element.local_axis.ref_vector;
            element.local_axis.ref_vector = [-y, x, z];
        }
    }
    if rotated {
        for node in &mut model.nodes {
            let [x, y, z] = node.coord;
            node.coord = [-y, x, z];
            node.restraint = Dof6Mask::FIXED;
            if node.id.index() >= 2 {
                node.restraint.set_free(Dof::Uy);
            }
        }
        model.constraints = vec![Constraint::RigidLink {
            master: NodeId(2),
            slaves: vec![NodeId(3)],
            dofs: Dof6Mask(1 << Dof::Uy as u8),
        }];
        for case in &mut model.load_cases {
            for load in &mut case.nodal {
                load.values = rotate_z90(load.values, true);
            }
        }
    }
}

#[test]
fn wall_system_signed_rotated_and_reordered_states_match_independent_reference() {
    let mut states = 0;
    for steel in [false, true] {
        let (stiffness, displacement, wall_q, wall_z, column_q, column_m) = if steel {
            (
                1_120_022.938_470_160_6,
                0.089_283_885_682_368_27,
                67_090.754_877_014_43,
                25_159.033_078_880_41,
                16_454.622_561_492_786,
                -24_681_933.842_239_18,
            )
        } else {
            (
                2_668_279.578_323_177_5,
                0.037_477_332_140_300_995,
                100_000.0,
                37_500.0,
                0.0,
                0.0,
            )
        };
        for kind in KINDS {
            for sign in [-1.0, 1.0] {
                for rotated in [false, true] {
                    for reversed_wall in [false, true] {
                        for reversed_columns in [false, true] {
                            println!("steel={steel}, kind={kind:?}, sign={sign}, rotated={rotated}, reversed_wall={reversed_wall}, reversed_columns={reversed_columns}");
                            let mut model = if steel {
                                steel_fixture(kind, sign * 100_000.0)
                            } else {
                                fixture(kind, Dof::Ux, sign * 100_000.0)
                            };
                            orient_fixture(&mut model, rotated, reversed_wall, reversed_columns);
                            let dof = if rotated { Dof::Uy } else { Dof::Ux };
                            let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
                            check_displacements(&result, dof, sign * displacement);
                            close(
                                sign * 100_000.0 / result.disp[2][dof as usize],
                                stiffness,
                                0.003,
                            );
                            close(projected_stiffness(&model, dof), stiffness, 0.003);
                            let forces = element_nodal_forces(&model, &result);
                            let mut reactions = [[0.0; 6]; 4];
                            for (e, element) in forces.iter().enumerate() {
                                for (node, force) in element.iter().enumerate() {
                                    let mut expected = [0.0; 6];
                                    if e == 4 {
                                        expected[0] = if node < 2 { -wall_q } else { wall_q } / 2.0;
                                        expected[2] = if node % 2 == 0 { -wall_z } else { wall_z };
                                    } else if e < 2 && node % 2 == e {
                                        expected[0] = if node < 2 { -column_q } else { column_q };
                                        expected[4] = column_m;
                                    }
                                    let expected = rotate_z90(expected.map(|v| sign * v), rotated);
                                    for d in 0..6 {
                                        close(
                                            force[d],
                                            expected[d],
                                            if d < 3 { 1e-5 } else { 0.01 },
                                        );
                                        reactions[node][d] += force[d];
                                    }
                                }
                            }
                            for load in &model.load_cases[0].nodal {
                                for (reaction, force) in
                                    reactions[load.node.index()].iter_mut().zip(load.values)
                                {
                                    *reaction -= force;
                                }
                            }
                            let mut balance = [0.0; 6];
                            for (node, reaction) in reactions.iter().enumerate() {
                                let mut expected = [0.0; 6];
                                if node < 2 {
                                    expected[0] = -50_000.0;
                                }
                                expected[2] = if node % 2 == 0 { -wall_z } else { wall_z };
                                expected[4] = column_m;
                                let expected = rotate_z90(expected.map(|v| sign * v), rotated);
                                for d in 0..6 {
                                    close(
                                        reaction[d],
                                        expected[d],
                                        if d < 3 { 1e-5 } else { 0.01 },
                                    );
                                }
                                let mut net = *reaction;
                                for load in model.load_cases[0]
                                    .nodal
                                    .iter()
                                    .filter(|l| l.node.index() == node)
                                {
                                    for (v, p) in net.iter_mut().zip(load.values) {
                                        *v += p;
                                    }
                                }
                                for d in 0..6 {
                                    balance[d] += net[d];
                                }
                                let [x, y, z] = model.nodes[node].coord;
                                balance[3] += y * net[2] - z * net[1];
                                balance[4] += z * net[0] - x * net[2];
                                balance[5] += x * net[1] - y * net[0];
                            }
                            for (d, value) in balance.into_iter().enumerate() {
                                close(value, 0.0, if d < 3 { 1e-5 } else { 0.01 });
                            }
                            let upper = std::array::from_fn::<_, 3, _>(|d| {
                                forces[4][2][d] + forces[4][3][d]
                            });
                            let winding = if reversed_wall { -1.0 } else { 1.0 };
                            let local_q = winding * upper[dof as usize];
                            close(local_q, winding * sign * wall_q, 1e-5);
                            close(upper[0], if rotated { 0.0 } else { sign * wall_q }, 1e-5);
                            close(upper[1], if rotated { sign * wall_q } else { 0.0 }, 1e-5);
                            states += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(states, 96);
}

#[test]
fn rc_wall_physical_reactions_and_linear_work_match_independent_reference() {
    for kind in KINDS {
        let model = fixture(kind, Dof::Ux, 100_000.0);
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        let forces = element_nodal_forces(&model, &result);
        for (node, force) in forces[4].iter().enumerate() {
            close(force[0], if node < 2 { -50_000.0 } else { 50_000.0 }, 1e-5);
            close(
                force[2],
                if node % 2 == 0 { -37_500.0 } else { 37_500.0 },
                1e-5,
            );
        }
        for element in &forces[..4] {
            for node in element {
                for (d, value) in node.iter().enumerate() {
                    close(*value, 0.0, if d < 3 { 1e-5 } else { 0.01 });
                }
            }
        }
        check_lateral_reactions(&model, &forces, 100_000.0, 100_000.0, 0.0);
        let work = lateral_work(kind, false);
        close(work[4], 1_873.866_607_015_049_8, 1e-6);
        for value in &work[..4] {
            close(*value, 0.0, 1e-6);
        }
        println!("{kind:?}: RC wall work={} Nmm", work[4]);
    }
}

#[test]
fn steel_plate_and_unreleased_boundary_columns_match_independent_reference() {
    // 400角の鋼平鋼は人工数値例。板・固定端柱の曲げ/せん断コンプライアンスから得た閉式基準。
    const PLATE_K_N_MM: f64 = 751_431.844_215_349_5;
    const COLUMN_K_N_MM: f64 = 184_295.547_127_405_53;
    const TOTAL_K_N_MM: f64 = 1_120_022.938_470_160_6;
    const DISPLACEMENT_MM: f64 = 0.089_283_885_682_368_27;
    const PLATE_Q_N: f64 = 67_090.754_877_014_43;
    const COLUMN_Q_N: f64 = 16_454.622_561_492_786;
    for kind in KINDS {
        let model = steel_fixture(kind, 100_000.0);
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        check_displacements(&result, Dof::Ux, DISPLACEMENT_MM);
        close(100_000.0 / result.disp[2][0], TOTAL_K_N_MM, 0.002);
        close(projected_stiffness(&model, Dof::Ux), TOTAL_K_N_MM, 0.002);
        let forces = element_nodal_forces(&model, &result);
        for (node, force) in forces[4].iter().enumerate() {
            let mut expected_wall = [0.0; 6];
            expected_wall[0] = if node < 2 { -1.0 } else { 1.0 } * PLATE_Q_N / 2.0;
            expected_wall[2] = if node % 2 == 0 { -1.0 } else { 1.0 } * 25_159.033_078_880_41;
            for (d, expected) in expected_wall.iter().enumerate() {
                close(force[d], *expected, if d < 3 { 1e-5 } else { 0.01 });
            }
        }
        close(
            (forces[4][2][0] + forces[4][3][0]) / result.disp[2][0],
            PLATE_K_N_MM,
            0.001,
        );
        for (e, element) in forces[..2].iter().enumerate() {
            for (node, force) in element.iter().enumerate() {
                let mut expected = [0.0; 6];
                if node % 2 == e {
                    expected[0] = if node < 2 { -COLUMN_Q_N } else { COLUMN_Q_N };
                    expected[4] = -24_681_933.842_239_18;
                }
                for (d, expected) in expected.iter().enumerate() {
                    close(force[d], *expected, if d < 3 { 1e-5 } else { 0.01 });
                }
            }
            close(element[e + 2][0] / result.disp[2][0], COLUMN_K_N_MM, 0.001);
        }
        check_lateral_reactions(&model, &forces, 100_000.0, PLATE_Q_N, COLUMN_Q_N);
        let work = lateral_work(kind, true);
        close(work.iter().sum(), 4_464.194_284_118_414, 1e-6);
        close(work[4], 0.5 * PLATE_K_N_MM * DISPLACEMENT_MM.powi(2), 1e-6);
        for value in &work[..2] {
            close(*value, 0.5 * COLUMN_K_N_MM * DISPLACEMENT_MM.powi(2), 1e-6);
        }
        close(work[2], 0.0, 1e-6);
        close(work[3], 0.0, 1e-6);
        println!(
            "{kind:?}: steel K={}, ux={}, plate Q={}, total work={} Nmm",
            100_000.0 / result.disp[2][0],
            result.disp[2][0],
            forces[4][2][0] + forces[4][3][0],
            work.iter().sum::<f64>()
        );
    }
}

#[test]
fn rc_wall_lateral_stiffness_and_force_partition_match_independent_reference() {
    // 区分多項式のせん断流積分As=785670.7448491786 mm²と壁板EIから得た閉式基準。
    const STIFFNESS_N_MM: f64 = 2_668_279.578_323_177_5;
    const DISPLACEMENT_MM: f64 = 0.037_477_332_140_300_995;
    for kind in KINDS {
        let model = fixture(kind, Dof::Ux, 100_000.0);
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        check_displacements(&result, Dof::Ux, DISPLACEMENT_MM);
        close(100_000.0 / result.disp[2][0], STIFFNESS_N_MM, 0.003);
        close(projected_stiffness(&model, Dof::Ux), STIFFNESS_N_MM, 0.003);
        assert_eq!(result.member_forces.len(), 5);
        for id in [ElemId(0), ElemId(1)] {
            let forces = &result
                .member_forces
                .iter()
                .find(|(i, _)| *i == id)
                .unwrap()
                .1;
            assert!(forces.at.len() >= 2);
            for (_, f) in &forces.at {
                close(f[1], 0.0, 1e-5);
                close(f[5], 0.0, 0.01);
            }
        }
        let virtual_column = &result
            .member_forces
            .iter()
            .find(|(id, _)| *id == ElemId(4))
            .unwrap()
            .1;
        assert!(virtual_column.at.len() >= 2);
        for (xi, f) in &virtual_column.at {
            close(f[0], 0.0, 1e-5);
            close(f[1], -100_000.0, 1e-5);
            close(f[5], 150_000_000.0 - 300_000_000.0 * xi, 0.01);
        }
        let wall_force = wall_nodal_force(&model, &result);
        close(wall_force[2][0] + wall_force[3][0], 100_000.0, 1e-5);
        close(wall_force[0][0] + wall_force[1][0], -100_000.0, 1e-5);
        println!(
            "{kind:?}: lateral K={}, ux={}, Qw={}",
            100_000.0 / result.disp[2][0],
            result.disp[2][0],
            wall_force[2][0] + wall_force[3][0]
        );
    }
}

#[test]
fn rc_wall_axial_stiffness_and_column_forces_match_independent_reference() {
    // 壁板のEA/H=8116666.666666667、側柱各EA/H=1600000 N/mmの並列和。
    const STIFFNESS_N_MM: f64 = 11_316_666.666_666_668;
    const TOTAL_FORCE_N: f64 = 113_166.666_666_666_68;
    const VIRTUAL_COLUMN_FORCE_N: f64 = 81_166.666_666_666_67;
    for kind in KINDS {
        let model = fixture(kind, Dof::Uz, TOTAL_FORCE_N);
        let result = linear_static_once(&model, LoadCaseId(0)).unwrap();
        check_displacements(&result, Dof::Uz, 0.01);
        close(TOTAL_FORCE_N / result.disp[2][2], STIFFNESS_N_MM, 0.012);
        close(projected_stiffness(&model, Dof::Uz), STIFFNESS_N_MM, 0.012);
        assert_eq!(result.member_forces.len(), 5);
        for id in [ElemId(0), ElemId(1), ElemId(4)] {
            let forces = &result
                .member_forces
                .iter()
                .find(|(i, _)| *i == id)
                .unwrap()
                .1;
            let expected_n = if id == ElemId(4) {
                VIRTUAL_COLUMN_FORCE_N
            } else {
                16_000.0
            };
            assert!(forces.at.len() >= 2);
            for (_, f) in &forces.at {
                close(f[0], expected_n, 1e-5);
                for &q in &f[1..3] {
                    close(q, 0.0, 1e-5);
                }
                for &moment in &f[3..6] {
                    close(moment, 0.0, 0.01);
                }
            }
        }
        let wall_force = wall_nodal_force(&model, &result);
        close(
            wall_force[2][2] + wall_force[3][2],
            VIRTUAL_COLUMN_FORCE_N,
            1e-5,
        );
        close(
            wall_force[0][2] + wall_force[1][2],
            -VIRTUAL_COLUMN_FORCE_N,
            1e-5,
        );
        println!(
            "{kind:?}: axial K={}, uz={}, wall N={}",
            TOTAL_FORCE_N / result.disp[2][2],
            result.disp[2][2],
            wall_force[2][2] + wall_force[3][2]
        );
    }
}
