use sepika_core::ids::{ElemId, NodeId, SectionId};
use sepika_core::model::{
    Constraint, ElementKind, EndCondition, FrameSectionUse, StoryDynamicMass,
};
use sepika_core::units::GRAVITY_MM_S2;
use sepika_job::compute::compute_linear_static;
use sepika_job::{build_lumped_mass, LumpedMassBuildInput};
use sepika_solver::dynamic::lumped_mass::{LumpedMassType, LumpedStiffnessSource, StickDim};
use sepika_solver::statics::analysis::SeismicDir;

#[allow(dead_code)]
#[path = "../../sepika-design-jp/tests/fixtures/eccentricity_model.rs"]
mod eccentricity_model;
use eccentricity_model::{
    add_case, oppositely_rotated_columns_rigid_floor, three_column_rigid_floor, TOP,
};

#[test]
fn both_spatial_mass_models_hold_the_layer_with_directional_coupling() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let x = add_case(&mut model, "X", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| compute_linear_static(model.clone(), id).unwrap());
    assert_eq!(x.story_rigidity, y.story_rigidity);
    assert!(matches!(x.story_rigidity.as_slice(), [(TOP, Ok(Some(_)))]));

    for model_type in [
        LumpedMassType::EquivalentShear,
        LumpedMassType::EquivalentBendingShear,
    ] {
        for source in [
            LumpedStiffnessSource::ColumnKi,
            LumpedStiffnessSource::StoryQd,
        ] {
            for nonlinear in [false, true] {
                let reason = build_lumped_mass(LumpedMassBuildInput {
                    model_type,
                    model: &model,
                    dim: StickDim::Spatial,
                    source,
                    dir: SeismicDir::X,
                    nonlinear,
                    secant_ratio: 0.75,
                    target_drift_angle: 1.0 / 200.0,
                    res_x: Some(&x),
                    res_y: Some(&y),
                    po_x: None,
                    po_y: None,
                })
                .unwrap_err()
                .to_string();
                for expected in ["階1", "方向間連成", "独立したX/Yばね"] {
                    assert!(reason.contains(expected), "{reason}");
                }
            }
        }
    }
}

#[test]
fn spatial_bending_shear_reports_invalid_diaphragm_node_references() {
    let mut model = three_column_rigid_floor([2400.0, 3200.0]);
    let x = add_case(&mut model, "X", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| compute_linear_static(model.clone(), id).unwrap());
    let Constraint::RigidDiaphragm { master, .. } = &mut model.constraints[0] else {
        panic!("試験モデルの層上端に剛床がありません");
    };
    *master = NodeId(99);
    let reason = build_lumped_mass(LumpedMassBuildInput {
        model_type: LumpedMassType::EquivalentBendingShear,
        model: &model,
        dim: StickDim::Spatial,
        source: LumpedStiffnessSource::StoryQd,
        dir: SeismicDir::X,
        nonlinear: false,
        secant_ratio: 0.75,
        target_drift_angle: 1.0 / 200.0,
        res_x: Some(&x),
        res_y: Some(&y),
        po_x: None,
        po_y: None,
    })
    .unwrap_err()
    .to_string();
    assert!(reason.contains("階1"), "{reason}");
    assert_eq!(
        reason.matches("節点拘束1が参照する節点ID 99").count(),
        1,
        "{reason}"
    );
}

#[test]
fn spatial_mass_models_require_a_column_cut_even_with_independent_rigid_floor_rigidity() {
    let mut model = oppositely_rotated_columns_rigid_floor();
    model.elements[2].end_offsets_mm = [[0.0, 0.0, 150.0]; 2];
    let x = add_case(&mut model, "X", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| compute_linear_static(model.clone(), id).unwrap());
    let [(TOP, Ok(Some(rigidity)))] = x.story_rigidity.as_slice() else {
        panic!("物理的な層剛性が同定されていません");
    };
    assert!(rigidity.translational_stiffness_n_per_mm[1].abs() < 1e-9);
    for model_type in [
        LumpedMassType::EquivalentShear,
        LumpedMassType::EquivalentBendingShear,
    ] {
        let reason = build_lumped_mass(LumpedMassBuildInput {
            model_type,
            model: &model,
            dim: StickDim::Spatial,
            source: LumpedStiffnessSource::StoryQd,
            dir: SeismicDir::X,
            nonlinear: false,
            secant_ratio: 0.75,
            target_drift_angle: 1.0 / 200.0,
            res_x: Some(&x),
            res_y: Some(&y),
            po_x: None,
            po_y: None,
        })
        .unwrap_err()
        .to_string();
        for expected in ["階1", "柱2", "切断位置"] {
            assert!(reason.contains(expected), "{reason}");
        }
    }
}
#[test]
fn spatial_shear_without_columns_uses_story_qd_when_axes_are_independent() {
    let mut model = three_column_rigid_floor([0.0; 2]);
    for (base, [dx, dy]) in
        model
            .nodes
            .iter_mut()
            .step_by(2)
            .zip([[1000.0, 1000.0], [1000.0, -1000.0], [1000.0, 0.0]])
    {
        base.coord[0] -= dx;
        base.coord[1] -= dy;
    }
    model.sections[0].frame_use = Some(FrameSectionUse::Brace);
    for element in &mut model.elements {
        element.kind = ElementKind::Brace {
            tension_only: false,
        };
    }
    model.stories[TOP.index()].dynamic_mass = Some(StoryDynamicMass {
        mass_equiv_weight_n: GRAVITY_MM_S2,
        center_xy_mm: [2000.0, 2000.0],
        inertia_t_mm2: 1.0e6,
        horizontal_inertia_t_mm2: [0.0; 3],
        lumped_mass: None,
    });
    let x = add_case(&mut model, "X", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| compute_linear_static(model.clone(), id).unwrap());
    let [(TOP, Ok(Some(rigidity)))] = x.story_rigidity.as_slice() else {
        panic!(
            "柱を持たない層の剛性が同定されていません: {:?}",
            x.story_rigidity
        );
    };
    assert!(rigidity.translational_stiffness_n_per_mm[1].abs() < 1e-9);
    let input = LumpedMassBuildInput {
        model_type: LumpedMassType::EquivalentShear,
        model: &model,
        dim: StickDim::Spatial,
        source: LumpedStiffnessSource::StoryQd,
        dir: SeismicDir::X,
        nonlinear: false,
        secant_ratio: 0.75,
        target_drift_angle: 1.0 / 200.0,
        res_x: Some(&x),
        res_y: Some(&y),
        po_x: None,
        po_y: None,
    };
    let result = build_lumped_mass(input).unwrap();
    let floor = &result.spatial[0];
    assert_eq!(floor.rigidity_xy, rigidity.center_xy_mm);
    assert_eq!(floor.kr, rigidity.torsional_stiffness_nmm);
    assert!(floor.k1_x > 0.0 && floor.k1_y > 0.0);
    let error = build_lumped_mass(LumpedMassBuildInput {
        source: LumpedStiffnessSource::ColumnKi,
        ..input
    })
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("柱 ki") && error.contains("対象柱がありません"),
        "{error}"
    );
}

#[test]
fn spatial_shear_uses_story_qd_but_holds_column_ki_when_a_brace_contributes() {
    let mut model = oppositely_rotated_columns_rigid_floor();
    let mut base = model.nodes[0].clone();
    base.id = NodeId(6);
    base.coord[0] = -2000.0;
    model.nodes.push(base);
    let mut section = model.sections[0].clone();
    section.id = SectionId(2);
    section.name = "斜材断面".into();
    section.frame_use = Some(FrameSectionUse::Brace);
    model.sections.push(section);
    let mut brace = model.elements[0].clone();
    brace.id = ElemId(3);
    brace.nodes[0] = NodeId(6);
    brace.section = Some(SectionId(2));
    brace.kind = ElementKind::Brace {
        tension_only: false,
    };
    brace.end_cond = [EndCondition::Pinned; 2];
    model.elements.push(brace);
    model.stories[TOP.index()].dynamic_mass = Some(StoryDynamicMass {
        mass_equiv_weight_n: GRAVITY_MM_S2,
        center_xy_mm: [2000.0, 2000.0],
        inertia_t_mm2: 1.0e6,
        horizontal_inertia_t_mm2: [0.0; 3],
        lumped_mass: None,
    });
    let x = add_case(&mut model, "X", [1.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    let y = add_case(&mut model, "Y", [0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    let [x, y] = [x, y].map(|id| compute_linear_static(model.clone(), id).unwrap());
    let [(TOP, Ok(Some(rigidity)))] = x.story_rigidity.as_slice() else {
        panic!("層剛性が同定されていません");
    };
    assert!(rigidity.translational_stiffness_n_per_mm[1].abs() < 1e-9);
    let input = LumpedMassBuildInput {
        model_type: LumpedMassType::EquivalentShear,
        model: &model,
        dim: StickDim::Spatial,
        source: LumpedStiffnessSource::StoryQd,
        dir: SeismicDir::X,
        nonlinear: false,
        secant_ratio: 0.75,
        target_drift_angle: 1.0 / 200.0,
        res_x: Some(&x),
        res_y: Some(&y),
        po_x: None,
        po_y: None,
    };
    let result = build_lumped_mass(input).unwrap();
    let floor = &result.spatial[0];
    let brace_length_mm = 2000.0_f64.hypot(3000.0);
    let brace_kx = 200_000.0 * 2000.0 / brace_length_mm * (2000.0 / brace_length_mm).powi(2);
    let columns_kx = 2_775.251_189_846_642;
    assert!(
        (floor.k1_x - (columns_kx + brace_kx)).abs() < 1e-6,
        "{}",
        floor.k1_x
    );
    let reason = build_lumped_mass(LumpedMassBuildInput {
        source: LumpedStiffnessSource::ColumnKi,
        ..input
    })
    .unwrap_err()
    .to_string();
    for expected in ["階 基礎", "柱だけの水平剛性", "柱 ki 方式"] {
        assert!(reason.contains(expected), "{reason}");
    }
}
