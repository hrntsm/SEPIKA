use sepika_core::dof::{Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    AnalysisKind, ElementData, ElementKind, EndCondition, ForceRegime, LoadCase, LoadCaseKind,
    LocalAxis, MassMethod, Material, MaterialCategory, Model, Node, Section,
};
use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};
use sepika_element::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use sepika_element::factory::StrengthBasis;
use sepika_element::frame::{
    beam::BeamElement, fiber::FiberBeam, multi_spring::MultiSpringElement,
};
use sepika_math::solver::SolveError;
use sepika_solver::common::{assemble::assemble_global_m, constraint::Reducer};
use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
use sepika_solver::dynamic::eigen::solve_eigen;
use sepika_solver::dynamic::timehistory::{
    linear_time_history_analysis, nonlinear_time_history_analysis, GroundMotion, NewmarkCfg,
    NonlinearThCfg,
};

const LENGTH_MM: f64 = 3000.0;
const SRC_MASS_T: f64 = 1.3766168875202032;

#[path = "support/wall_side_column.rs"]
mod wall_side_column_fixture;
use wall_side_column_fixture::wall_connection_fixture;

#[test]
fn mass_assembly_rejects_invalid_wall_side_column_frame_before_building_behavior() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_connection_fixture(kind);
        model.elements[0].local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let solver = sepika_math::solver::make_solver(
            sepika_math::solver::SolverBackend::DirectSparseCholesky,
        );
        let errors = [
            assemble_global_m(&model, &dofmap, MassOption::Consistent)
                .map(|_| ())
                .unwrap_err(),
            solve_eigen(&model, &dofmap, &reducer, 0)
                .map(|_| ())
                .unwrap_err(),
            sepika_solver::dynamic::eigen::solve_eigen_with_solver(
                &model,
                &dofmap,
                &reducer,
                0,
                solver.as_ref(),
            )
            .map(|_| ())
            .unwrap_err(),
        ];
        for error in errors {
            assert!(
                matches!(error, SolveError::InvalidInput(reason) if reason.contains("部材 ID 0") && reason.contains("断面参照方向")),
                "{kind:?}"
            );
        }
        let error = sepika_element::factory::ensure_wall_side_column_analysis(
            &model,
            None,
            "質量行列組立",
            false,
            Some(MassOption::Consistent),
        )
        .unwrap_err();
        assert!(error.contains("部材 ID 0") && error.contains("断面参照方向"));
    }
}

#[test]
fn wall_connection_options_gate_uses_actual_analysis_and_mass_path() {
    use sepika_element::factory::{build_behavior, ensure_wall_side_column_analysis};
    use sepika_element::frame::panel_offset::{PanelEnd, PanelOffsetMember};
    use sepika_solver::nonlinear::pushover::{
        pushover_analysis_recording, PushoverControl, PushoverTarget,
    };
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_connection_fixture(kind);
        let index = sepika_core::model::WallSideColumnIndex::build(&model);
        assert_eq!(index.replacement_bindings(ElemId(0)).unwrap().len(), 1);
        assert!(ensure_wall_side_column_analysis(&model, None, "線形解析", false, None).is_ok());
        assert!(ensure_wall_side_column_analysis(
            &model,
            Some(AnalysisKind::Incremental),
            "増分解析",
            false,
            None
        )
        .is_ok());
        let pdelta = ensure_wall_side_column_analysis(
            &model,
            Some(AnalysisKind::Incremental),
            "増分解析",
            true,
            None,
        )
        .unwrap_err();
        assert!(pdelta.contains("増分解析: 部材 ID 0・壁 ID [4]") && pdelta.contains("P-Δ"));
        assert!(ensure_wall_side_column_analysis(
            &model,
            None,
            "固有値解析",
            false,
            Some(MassOption::Consistent)
        )
        .is_ok());
        for element in &mut model.elements[..2] {
            element.end_cond = [EndCondition::Fixed, EndCondition::Pinned];
        }
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let actual = pushover_analysis_recording(
            &model,
            &dofmap,
            &reducer,
            sepika_solver::statics::analysis::SeismicDir::X,
            1,
            PushoverTarget::from_max_disp(1.0),
            PushoverControl::default(),
            false,
            true,
            Default::default(),
        )
        .unwrap_err();
        assert_eq!(actual, pdelta);
        assemble_global_m(&model, &dofmap, MassOption::Lumped).unwrap();
        let direct =
            build_behavior(&model.elements[0], &model).try_mass_matrix(MassOption::Consistent);
        let gate = ensure_wall_side_column_analysis(
            &model,
            None,
            "固有値解析",
            false,
            Some(MassOption::Consistent),
        );
        let direct_reason = direct
            .err()
            .expect("両端自由ねじりの共通ゲージは慣性を消せない");
        assert!(direct_reason.contains("慣性"));
        let wrapped = PanelOffsetMember::new(
            build_behavior(&model.elements[0], &model),
            [
                Some(PanelEnd {
                    node: NodeId(0),
                    rotation: [[0.5, 0.0], [0.0, 0.5], [0.0, 0.0]],
                }),
                None,
            ],
        );
        assert!(
            matches!(wrapped.try_mass_matrix(MassOption::Consistent), Err(reason) if reason == direct_reason)
        );
        assert!(gate.unwrap_err().ends_with(&direct_reason));
        let mass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assemble_global_m(&model, &dofmap, MassOption::Consistent)
        }));
        assert!(
            matches!(mass.unwrap(), Err(SolveError::InvalidInput(reason)) if reason.ends_with(&direct_reason))
        );
        for fixed in [false, true] {
            if fixed {
                for node in &mut model.nodes {
                    node.restraint = Dof6Mask::FIXED;
                }
            }
            let dofmap = DofMap::build(&model);
            let reducer = Reducer::build(&model, &dofmap);
            assert!(
                matches!(solve_eigen(&model, &dofmap, &reducer, 0), Err(SolveError::InvalidInput(reason)) if reason.starts_with("固有値解析:") && reason.ends_with(&direct_reason))
            );
            let solver = sepika_math::solver::make_solver(
                sepika_math::solver::SolverBackend::DirectSparseCholesky,
            );
            assert!(
                matches!(sepika_solver::dynamic::eigen::solve_eigen_with_solver(&model, &dofmap, &reducer, 0, solver.as_ref()), Err(SolveError::InvalidInput(reason)) if reason.starts_with("固有値解析:") && reason.ends_with(&direct_reason))
            );
            let wave = GroundMotion {
                dt: 0.01,
                accel_x: vec![0.0, 0.0],
                accel_y: None,
                accel_theta: None,
            };
            let result = linear_time_history_analysis(
                &model,
                &dofmap,
                &reducer,
                &wave,
                &NewmarkCfg::average_accel(),
                &Damping::MassProportional {
                    h: 0.05,
                    omega: 1.0,
                },
                &[],
                &[],
                false,
                None,
            );
            assert!(
                matches!(result, Err(SolveError::InvalidInput(reason)) if reason.starts_with("線形時刻歴解析:") && reason.ends_with(&direct_reason))
            );
            let state = sepika_solver::dynamic::timehistory::TimeStepState {
                step: 0,
                time: 0.0,
                disp_red: vec![],
                vel_red: vec![],
                accel_red: vec![],
            };
            let result = sepika_solver::dynamic::timehistory::linear_time_history_from_state(
                &model,
                &dofmap,
                &reducer,
                &wave,
                &NewmarkCfg::average_accel(),
                &Damping::MassProportional {
                    h: 0.05,
                    omega: 1.0,
                },
                &state,
                false,
                None,
            );
            assert!(
                matches!(result, Err(SolveError::InvalidInput(reason)) if reason.starts_with("線形時刻歴解析:") && reason.ends_with(&direct_reason))
            );
        }
        model.mass_method = MassMethod::LumpedOnly;
        assert!(ensure_wall_side_column_analysis(
            &model,
            None,
            "固有値解析",
            false,
            Some(MassOption::Consistent)
        )
        .is_ok());
        assert!(assemble_global_m(&model, &DofMap::build(&model), MassOption::Consistent).is_ok());
        model.mass_method = MassMethod::CorrectedLumped;
        for element in &mut model.elements[..2] {
            element.end_cond = [EndCondition::Pinned, EndCondition::Fixed];
        }
        assert!(build_behavior(&model.elements[0], &model)
            .try_mass_matrix(MassOption::Consistent)
            .is_ok());
        let wrapped = PanelOffsetMember::new(
            build_behavior(&model.elements[0], &model),
            [
                Some(PanelEnd {
                    node: NodeId(0),
                    rotation: [[0.5, 0.0], [0.0, 0.5], [0.0, 0.0]],
                }),
                None,
            ],
        );
        let checked = wrapped.try_mass_matrix(MassOption::Consistent).unwrap();
        assert_eq!(checked.n, 14);
        assert_eq!(
            checked.data,
            wrapped.mass_matrix(MassOption::Consistent).data
        );
        assert!(ensure_wall_side_column_analysis(
            &model,
            None,
            "固有値解析",
            false,
            Some(MassOption::Consistent)
        )
        .is_ok());
        assert!(assemble_global_m(&model, &DofMap::build(&model), MassOption::Consistent).is_ok());
    }
}

#[test]
fn wall_connection_gate_preserves_ordinary_and_pure_steel_options() {
    use sepika_element::factory::ensure_wall_side_column_analysis;
    for representation in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut ordinary = wall_connection_fixture(representation);
        ordinary
            .elements
            .retain(|element| element.kind != ElementKind::Wall);
        ordinary.generated_wall_origins.clear();
        ordinary.wall_plates.clear();
        ordinary.wall_regions.clear();
        ordinary.wall_assignment_regions = Default::default();
        let mut steel = wall_connection_fixture(representation);
        for index in 0..2 {
            let mut section = SectionShape::SteelBox {
                height: 600.0,
                width: 600.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(0.0),
            }
            .to_section(SectionId(index as u32), "鋼材".into());
            section.frame_use = steel.sections[index].frame_use;
            section.material = Some(MaterialId(2));
            steel.sections[index] = section;
        }
        steel.sections[2].shape = None;
        steel.sections[2].material = Some(MaterialId(2));
        steel.sections[2].thickness = Some(6.0);
        for model in [ordinary, steel] {
            for kind in [
                None,
                Some(AnalysisKind::Incremental),
                Some(AnalysisKind::TimeHistory),
            ] {
                assert!(ensure_wall_side_column_analysis(
                    &model,
                    kind,
                    "実解析",
                    true,
                    Some(MassOption::Consistent)
                )
                .is_ok());
            }
        }
    }
}

fn rebar(count: u32) -> RcRectColumnRebar {
    RcRectColumnRebar {
        main_dia: if count == 0 { 0.0 } else { 19.0 },
        x: if count == 0 { vec![] } else { vec![count] },
        y: if count == 0 { vec![] } else { vec![count] },
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: 10.0,
            pitch: 100.0,
            legs_x: 2,
            legs_y: 2,
        },
    }
}

fn src(count: u32, steel_height: f64) -> SectionShape {
    SectionShape::SrcColumnRect {
        b: 300.0,
        d: 600.0,
        rebar: rebar(count),
        steel_height,
        steel_width: 100.0,
        steel_web_thick: 6.0,
        steel_flange_thick: 9.0,
    }
}

fn fixture(shape: Option<SectionShape>, category: MaterialCategory) -> Model {
    let mut section = shape.map_or_else(
        || Section {
            area: 12345.0,
            width: 300.0,
            depth: 600.0,
            iy: 8.0e8,
            iz: 2.0e8,
            j: 1.0e6,
            as_y: 10000.0,
            as_z: 10000.0,
            ..Section::zero(SectionId(0), "保存AとbDが異なる".into())
        },
        |shape| shape.to_section(SectionId(0), "独立質量fixture".into()),
    );
    section.frame_use = match section.shape {
        Some(SectionShape::RcBeamRect { .. }) => Some(sepika_core::model::FrameSectionUse::Girder),
        Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. }) => {
            Some(sepika_core::model::FrameSectionUse::Column)
        }
        _ => section.frame_use,
    };
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section.steel_material = Some(MaterialId(2));
    let material = |id, category, density, young, fc, fy| Material {
        id: MaterialId(id),
        name: format!("material-{id}"),
        category,
        density,
        young,
        poisson: 0.3,
        fc,
        fy,
        shear: None,
        strength_factor: None,
        concrete_class: Default::default(),
    };
    Model {
        nodes: [[0.0, 0.0, 0.0], [LENGTH_MM, 0.0, 0.0]]
            .into_iter()
            .enumerate()
            .map(|(i, coord)| Node {
                id: NodeId(i as u32),
                coord,
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect(),
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![
            material(
                0,
                category,
                if category == MaterialCategory::Steel {
                    7.85e-9
                } else {
                    2.4e-9
                },
                205000.0,
                if category == MaterialCategory::Steel {
                    None
                } else {
                    Some(24.0)
                },
                Some(235.0),
            ),
            material(
                1,
                MaterialCategory::Rebar,
                7.85e-9,
                200000.0,
                None,
                Some(400.0),
            ),
            material(
                2,
                MaterialCategory::Steel,
                7.85e-9,
                205000.0,
                None,
                Some(235.0),
            ),
        ],
        ..Default::default()
    }
}

fn assert_mass(matrix: &LocalMat, expected_t: f64, opt: MassOption) {
    for d in 0..3 {
        let translation = matrix.get(d, d)
            + matrix.get(d, d + 6)
            + matrix.get(d + 6, d)
            + matrix.get(d + 6, d + 6);
        assert!(
            (translation - expected_t).abs() < 1e-10,
            "direction {d}: {translation} vs {expected_t}"
        );
        if matches!(opt, MassOption::Lumped) {
            assert!((matrix.get(d, d) - expected_t / 2.0).abs() < 1e-12);
            assert!((matrix.get(d + 6, d + 6) - expected_t / 2.0).abs() < 1e-12);
            assert_eq!(matrix.get(d + 3, d + 3), 0.0);
            assert_eq!(matrix.get(d + 9, d + 9), 0.0);
        }
    }
}

fn verify_direct_and_public(model: &Model, mass_t: f64) {
    let data = &model.elements[0];
    let beam = BeamElement::try_new(data, model).unwrap();
    let fiber = FiberBeam::try_new(
        data,
        model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    )
    .unwrap_or_else(|reason| panic!("{:?}: {reason}", model.sections[0].shape));
    let ms = MultiSpringElement::new(
        data,
        model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    for opt in [MassOption::Lumped, MassOption::Consistent] {
        for elem in [&beam as &dyn ElementBehavior, &fiber, &ms] {
            assert_mass(&elem.mass_matrix(opt), mass_t, opt);
        }
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let mut model = model.clone();
            model.elements[0].kind = kind;
            let dofmap = DofMap::build(&model);
            let global = assemble_global_m(&model, &dofmap, opt).unwrap().to_dense();
            for d in 0..3 {
                let indices = [
                    dofmap.active(d).unwrap() as usize,
                    dofmap.active(d + 6).unwrap() as usize,
                ];
                let total: f64 = indices
                    .iter()
                    .flat_map(|&i| indices.iter().map(move |&j| (i, j)))
                    .map(|(i, j)| global[(i, j)])
                    .sum();
                assert!((total - mass_t).abs() < 1e-10);
            }
        }
    }
}

#[test]
fn geometry_and_adopted_density_are_independent_mass_oracles_for_all_frame_forms() {
    let s = fixture(
        Some(SectionShape::SteelFlatBar {
            width: 200.0,
            thick: 20.0,
        }),
        MaterialCategory::Steel,
    );
    verify_direct_and_public(&s, 4000.0 * 7.85e-9 * LENGTH_MM);
    let rc = fixture(
        Some(SectionShape::RcColumnRect {
            b: 300.0,
            d: 600.0,
            rebar: rebar(4),
        }),
        MaterialCategory::Concrete,
    );
    verify_direct_and_public(&rc, 180000.0 * 24.0 / 9.80665e9 * LENGTH_MM);
    let src = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
    verify_direct_and_public(&src, SRC_MASS_T);
    assert!((SRC_MASS_T - 1.296).abs() > 0.01);
    assert!((SRC_MASS_T - 1.3522176).abs() > 0.01);
    for thick in [10.0, 20.0] {
        let cft = fixture(
            Some(SectionShape::CftBox {
                height: 600.0,
                width: 300.0,
                thick,
                horizontal_thick: thick,
                corner_r: Some(0.0),
            }),
            MaterialCategory::Concrete,
        );
        let core_area = (600.0 - 2.0 * thick) * (300.0 - 2.0 * thick);
        let steel_area = 180000.0 - core_area;
        let mass_t = (steel_area * 7.85e-9 + core_area * 23.0 / 9.80665e9) * LENGTH_MM;
        verify_direct_and_public(&cft, mass_t);
    }
    let shapeless = fixture(None, MaterialCategory::Steel);
    verify_direct_and_public(&shapeless, 12345.0 * 7.85e-9 * LENGTH_MM);
}

#[test]
fn reinforcement_and_src_steel_area_do_not_change_inclusive_mass() {
    for count in [0, 2, 4] {
        let rc = fixture(
            Some(SectionShape::RcColumnRect {
                b: 300.0,
                d: 600.0,
                rebar: rebar(count),
            }),
            MaterialCategory::Concrete,
        );
        verify_direct_and_public(&rc, 180000.0 * 24.0 / 9.80665e9 * LENGTH_MM);
        for h in [200.0, 400.0] {
            verify_direct_and_public(
                &fixture(Some(src(count, h)), MaterialCategory::Concrete),
                SRC_MASS_T,
            );
        }
    }
}

#[test]
fn fiber_gauss_points_tangents_history_and_missing_mesh_do_not_supply_mass() {
    let model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
    let data = &model.elements[0];
    let mut fiber = FiberBeam::new(
        data,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    let ctx = Ctx { model: &model };
    let before = [
        fiber.mass_matrix(MassOption::Lumped),
        fiber.mass_matrix(MassOption::Consistent),
    ];
    fiber
        .update_state(
            &LocalVec {
                data: smallvec::smallvec![
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.0, 0.0, -0.01
                ],
            },
            true,
            &ctx,
        )
        .unwrap();
    for gp in &mut fiber.gauss_points {
        gp.xi = 0.0;
        gp.weight = 100.0;
        gp.trial_stress.fill(1.0e8);
        gp.trial_et.fill(0.0);
        for fiber in &mut gp.section.fibers {
            fiber.area *= 100.0;
        }
        for material in &mut gp.mats {
            material.trial(0.2);
            material.commit();
        }
    }
    for (i, opt) in [MassOption::Lumped, MassOption::Consistent]
        .into_iter()
        .enumerate()
    {
        assert_eq!(fiber.mass_matrix(opt).data, before[i].data);
    }
    fiber.gauss_points.clear();
    let mut ms = MultiSpringElement::new(
        data,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    ms.inner.gauss_points.clear();
    for opt in [MassOption::Lumped, MassOption::Consistent] {
        assert_mass(&fiber.mass_matrix(opt), SRC_MASS_T, opt);
        assert_mass(&ms.mass_matrix(opt), SRC_MASS_T, opt);
    }
}

#[test]
fn global_mass_projects_fixed_dofs_and_adds_node_mass() {
    let mut model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
    model.nodes[0].restraint = Dof6Mask::FIXED;
    model.nodes[1].mass = Some([0.1, 0.2, 0.3, 0.0, 0.0, 0.0]);
    let dofmap = DofMap::build(&model);
    let global = assemble_global_m(&model, &dofmap, MassOption::Lumped)
        .unwrap()
        .to_dense();
    for d in 0..3 {
        let active = dofmap.active(6 + d).unwrap() as usize;
        assert!((global[(active, active)] - SRC_MASS_T / 2.0 - 0.1 * (d + 1) as f64).abs() < 1e-12);
    }
}

#[test]
fn both_mass_options_reject_invalid_density_with_element_id_and_lumped_only_bypasses_it() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
        ElementKind::Brace {
            tension_only: false,
        },
    ] {
        for density in [-1.0, f64::NAN, f64::INFINITY] {
            let mut model = fixture(None, MaterialCategory::Steel);
            model.elements[0].kind = kind;
            model.materials[0].density = density;
            let dofmap = DofMap::build(&model);
            for opt in [MassOption::Lumped, MassOption::Consistent] {
                let error = assemble_global_m(&model, &dofmap, opt).unwrap_err();
                assert!(matches!(error, SolveError::InvalidInput(_)));
                let reason = error.to_string();
                assert!(
                    reason.contains("要素 ID 0") && reason.contains("密度"),
                    "{reason}"
                );
            }
            model.mass_method = MassMethod::LumpedOnly;
            assert!(assemble_global_m(&model, &dofmap, MassOption::Lumped).is_ok());
            assert!(assemble_global_m(&model, &dofmap, MassOption::Consistent).is_ok());
        }
    }
    let mut zero = fixture(None, MaterialCategory::Steel);
    zero.materials[0].density = 0.0;
    verify_direct_and_public(&zero, 0.0);
}

fn constrained(model: &mut Model) {
    model.nodes[0].restraint = Dof6Mask::FIXED;
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id: LoadCaseId(0),
        name: "質量不要".into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![],
        member: vec![],
    });
}

#[test]
fn cft_mass_only_failure_allows_static_but_public_dynamic_analyses_return_diagnostics() {
    let mut model = fixture(
        Some(SectionShape::CftBox {
            height: 600.0,
            width: 300.0,
            thick: 10.0,
            horizontal_thick: 10.0,
            corner_r: Some(0.0),
        }),
        MaterialCategory::Concrete,
    );
    constrained(&mut model);
    model.materials[0].density = f64::NAN;
    assert!(BeamElement::try_new(&model.elements[0], &model).is_ok());
    assert!(sepika_solver::statics::linear::linear_static_once(&model, LoadCaseId(0)).is_ok());
    assert_public_dynamic_mass_error(&model);
}

fn assert_public_dynamic_mass_error(model: &Model) {
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    let check = |error: SolveError| {
        assert!(matches!(error, SolveError::InvalidInput(_)));
        assert!(error.to_string().contains("要素 ID 0"), "{error}");
    };
    check(solve_eigen(model, &dofmap, &reducer, 1).unwrap_err());
    let wave = GroundMotion {
        dt: 0.01,
        accel_x: vec![0.0; 3],
        accel_y: None,
        accel_theta: None,
    };
    let newmark = NewmarkCfg::average_accel();
    let damping = Damping::Rayleigh {
        h1: 0.02,
        w1: 1.0,
        h2: 0.02,
        w2: 2.0,
    };
    let init = vec![0.0; reducer.n_indep];
    check(
        linear_time_history_analysis(
            model, &dofmap, &reducer, &wave, &newmark, &damping, &init, &init, false, None,
        )
        .unwrap_err(),
    );
    check(
        nonlinear_time_history_analysis(
            model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            DampingAccumulation::default(),
            &init,
            &init,
            NonlinearThCfg::default(),
        )
        .unwrap_err(),
    );
}

#[test]
fn angled_rotated_rigid_and_released_members_preserve_geometry_mass() {
    let mut model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
    model.nodes[1].coord = [1000.0, 2000.0, 2000.0];
    model.elements[0].local_axis.ref_vector = [0.2, 0.7, -0.3];
    model.elements[0].rigid_zone.length_i = 200.0;
    model.elements[0].rigid_zone.length_j = 300.0;
    for condition in [
        EndCondition::Fixed,
        EndCondition::Pinned,
        EndCondition::SemiRigid { k_theta: 2.0e8 },
    ] {
        model.elements[0].end_cond[0] = condition;
        verify_direct_and_public(&model, SRC_MASS_T);
    }
}

#[test]
fn direct_mass_api_retains_errors_instead_of_density_or_zero_fallbacks() {
    let mut model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
    model.materials[0].density = f64::NAN;
    let data = &model.elements[0];
    let beam = BeamElement::try_new(data, &model).unwrap();
    let fiber = FiberBeam::try_new(
        data,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    )
    .unwrap();
    let ms = MultiSpringElement::new(
        data,
        &model,
        StrengthBasis::Nominal,
        AnalysisKind::Incremental,
    );
    for elem in [&beam as &dyn ElementBehavior, &fiber, &ms] {
        for opt in [MassOption::Lumped, MassOption::Consistent] {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| elem.mass_matrix(opt)));
            assert!(result.is_err());
        }
    }
}

#[test]
fn public_mass_rejects_invalid_dimensions_radii_and_required_materials_before_factory() {
    for mutation in 0..5 {
        let mut model = fixture(
            Some(SectionShape::CftBox {
                height: 600.0,
                width: 300.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: Some(0.0),
            }),
            MaterialCategory::Concrete,
        );
        match mutation {
            0 => {
                if let Some(SectionShape::CftBox { thick, .. }) = &mut model.sections[0].shape {
                    *thick = 200.0;
                }
            }
            1 => {
                if let Some(SectionShape::CftBox { corner_r, .. }) = &mut model.sections[0].shape {
                    *corner_r = None;
                }
            }
            2 => model.sections[0].steel_material = None,
            3 => model.materials[0].fc = None,
            _ => model.sections[0].material = None,
        }
        let dofmap = DofMap::build(&model);
        for opt in [MassOption::Lumped, MassOption::Consistent] {
            let error = assemble_global_m(&model, &dofmap, opt).unwrap_err();
            assert!(matches!(error, SolveError::InvalidInput(_)));
            assert!(error.to_string().contains("要素 ID 0"));
        }
        assert_public_dynamic_mass_error(&model);
    }
}

#[test]
fn public_eigen_and_timehistories_use_projected_common_mass_for_elastic_axial_response() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(None, MaterialCategory::Steel);
        model.elements[0].kind = kind;
        model.sections[0].area = 20000.0;
        model.sections[0].width = 100.0;
        model.sections[0].depth = 200.0;
        model.nodes[0].restraint = Dof6Mask::FIXED;
        model.nodes[1].restraint = Dof6Mask(0b111110);
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let mass_t = 20000.0 * 7.85e-9 * LENGTH_MM / 3.0;
        let stiffness_n_mm = 205000.0 * 20000.0 / LENGTH_MM;
        let omega_sq = stiffness_n_mm / mass_t;
        let eigen = solve_eigen(&model, &dofmap, &reducer, 1).unwrap();
        assert!((eigen.omega2[0] / omega_sq - 1.0).abs() < 1e-10);
        let dt = 0.0001;
        let wave = GroundMotion {
            dt,
            accel_x: vec![0.0; 3],
            accel_y: None,
            accel_theta: None,
        };
        let newmark = NewmarkCfg::average_accel();
        let damping = Damping::Rayleigh {
            h1: 0.0,
            w1: 1.0,
            h2: 0.0,
            w2: 2.0,
        };
        let u0 = [0.001];
        let v0 = [0.0];
        // 一自由度平均加速度法の自由振動2step理論解。
        let step_cos = (1.0 - omega_sq * dt * dt / 4.0) / (1.0 + omega_sq * dt * dt / 4.0);
        let expected = u0[0] * (2.0 * step_cos * step_cos - 1.0);
        let linear = linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            &u0,
            &v0,
            false,
            Some(1),
        )
        .unwrap();
        let nonlinear = nonlinear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            DampingAccumulation::default(),
            &u0,
            &v0,
            NonlinearThCfg {
                apply_long_term: false,
                ..Default::default()
            },
        )
        .unwrap();
        for result in [linear, nonlinear] {
            assert_eq!(result.non_converged_steps, 0);
            assert_eq!(result.time, vec![0.0, dt, 2.0 * dt]);
            let recording = result.recording.unwrap();
            let last = recording.node_disp.last().unwrap()[1][0];
            assert!((last - expected).abs() < 1e-10, "{last} vs {expected}");
        }
    }
}

#[test]
fn beam_delegates_share_both_mass_forms_and_reject_the_same_resolver_failure() {
    use sepika_element::frame::concentrated::ConcentratedSpringBeam;
    use sepika_element::frame::panel_offset::{PanelEnd, PanelOffsetMember};
    use sepika_element::wall::side_column::{InPlaneReleasedColumn, ReleaseAxis};
    use sepika_material::uniaxial::Bilinear;
    for invalid in [false, true] {
        let mut model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
        if invalid {
            model.materials[0].density = f64::NAN;
        }
        let beam = BeamElement::try_new(&model.elements[0], &model).unwrap();
        let elements: Vec<Box<dyn ElementBehavior>> = vec![
            Box::new(InPlaneReleasedColumn::new(
                beam.clone(),
                ReleaseAxis::LocalY,
            )),
            Box::new(PanelOffsetMember::new(
                Box::new(beam.clone()),
                [
                    Some(PanelEnd {
                        node: NodeId(0),
                        rotation: [[-0.5, 0.0], [0.0, -0.5], [0.0, 0.0]],
                    }),
                    None,
                ],
            )),
            Box::new(ConcentratedSpringBeam::new(
                beam,
                Box::new(Bilinear::new(1.0e12, 1.0e20, 0.01)),
                Box::new(Bilinear::new(1.0e12, 1.0e20, 0.01)),
            )),
        ];
        for element in elements {
            for opt in [MassOption::Lumped, MassOption::Consistent] {
                if invalid {
                    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                        || element.mass_matrix(opt)
                    ))
                    .is_err());
                } else {
                    assert_mass(&element.mass_matrix(opt), SRC_MASS_T, opt);
                }
            }
        }
    }
}

#[test]
fn src_physical_self_weight_shares_inertial_mass_while_design_dl_distribution_is_preserved() {
    use sepika_core::model::{LoadCfg, MemberLoadKind};
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = fixture(Some(src(0, 400.0)), MaterialCategory::Concrete);
        model.elements[0].kind = kind;
        let (nodal, member) =
            sepika_load::self_weight::self_weight_case_content(&model, &LoadCfg::default())
                .unwrap();
        assert!(nodal.is_empty());
        assert_eq!(member.len(), 1);
        assert_eq!(member[0].dir, [0.0, 0.0, -1.0]);
        let MemberLoadKind::Distributed { a, b, w1, w2 } = member[0].kind else {
            panic!("水平線材の等分布自重を期待する")
        };
        assert_eq!(a, 0.0);
        assert_eq!(b, LENGTH_MM);
        assert!((w1 - 4.2364728).abs() < 1e-12 && (w2 - 4.2364728).abs() < 1e-12);
        for node in &mut model.nodes {
            node.coord[2] = 3000.0;
        }
        model.nodes.push(Node {
            id: NodeId(2),
            coord: [0.0, 0.0, 0.0],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        model.stories = [0.0, 3000.0]
            .into_iter()
            .enumerate()
            .map(|(i, fl)| {
                sepika_core::model::Story::new(
                    sepika_core::ids::StoryId(i as u32),
                    format!("{}F", i + 1),
                    Some(fl),
                    Some(0.0),
                )
            })
            .collect();
        let generated = sepika_load::story_gen::generate_stories_with_opts(
            &model,
            &[],
            true,
            MassMethod::CorrectedLumped,
        )
        .unwrap();
        let physical_n: f64 = generated
            .stories
            .iter()
            .filter_map(|story| story.dynamic_mass)
            .map(|mass| mass.mass_equiv_weight_n)
            .sum();
        assert!((physical_n - 13500.0).abs() < 1e-8);
        assert!((physical_n / 9806.65 - SRC_MASS_T).abs() < 1e-12);
    }
}

#[test]
fn lip_channel_geometry_dynamic_results_reject_all_mass_methods_before_early_success() {
    use sepika_math::solver::{make_solver, SolverBackend};
    use sepika_math::sparse::{assemble_csc, Triplet};
    use sepika_solver::dynamic::eigen::solve_eigen_with_solver;
    use sepika_solver::dynamic::timehistory::{
        linear_time_history_from_state, linear_time_history_with_state, TimeStepState,
    };

    let mut failures = vec![];
    for mass_method in [MassMethod::LumpedOnly, MassMethod::CorrectedLumped] {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            for lip in [75.0, 80.0, 100.0] {
                for fixed in [false, true] {
                    let mut model = fixture(
                        Some(SectionShape::SteelLipChannel {
                            height: 150.0,
                            width: 75.0,
                            lip: 20.0,
                            thick: 2.3,
                        }),
                        MaterialCategory::Steel,
                    );
                    model.mass_method = mass_method;
                    model.elements[0].kind = kind;
                    model.sections[0].name = "LC544".into();
                    model.sections[0].shape = Some(SectionShape::SteelLipChannel {
                        height: 150.0,
                        width: 75.0,
                        lip,
                        thick: 2.3,
                    });
                    model.nodes[0].restraint = Dof6Mask::FIXED;
                    for node in &mut model.nodes {
                        node.mass = Some([1.0; 6]);
                        if fixed {
                            node.restraint = Dof6Mask::FIXED;
                        }
                    }
                    let dofmap = DofMap::build(&model);
                    let reducer = Reducer::build(&model, &dofmap);
                    let n = reducer.n_indep.max(1);
                    let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
                    solver
                        .factorize(&assemble_csc(
                            n,
                            (0..n)
                                .map(|i| Triplet {
                                    row: i,
                                    col: i,
                                    val: 1.0,
                                })
                                .collect(),
                        ))
                        .unwrap();
                    let wave = GroundMotion {
                        dt: 0.01,
                        accel_x: vec![0.0, 0.0],
                        accel_y: None,
                        accel_theta: None,
                    };
                    let newmark = NewmarkCfg {
                        dt: 0.01,
                        ..NewmarkCfg::average_accel()
                    };
                    let damping = Damping::MassProportional {
                        h: 0.05,
                        omega: 1.0,
                    };
                    let state = TimeStepState {
                        step: 0,
                        time: 0.0,
                        disp_red: vec![0.0; reducer.n_indep],
                        vel_red: vec![0.0; reducer.n_indep],
                        accel_red: vec![0.0; reducer.n_indep],
                    };
                    let before = format!("{model:?}");
                    let reason = model.validate_lip_channels().unwrap_err().to_string();
                    let mut check =
                        |entry: &str, outcome: std::thread::Result<Result<(), SolveError>>| {
                            match outcome {
                            Ok(Err(SolveError::InvalidInput(error))) if error == reason => {}
                            Ok(Err(error)) => failures.push(format!("{mass_method:?}/{kind:?}/C{lip}/fixed{fixed}/{entry}: {error}")),
                            Ok(Ok(())) => failures.push(format!("{mass_method:?}/{kind:?}/C{lip}/fixed{fixed}/{entry}: 不成立形状で成功")),
                            Err(_) => failures.push(format!("{mass_method:?}/{kind:?}/C{lip}/fixed{fixed}/{entry}: Result入口でpanic")),
                        }
                        };
                    check(
                        "assemble_global_m",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            assemble_global_m(&model, &dofmap, MassOption::Consistent).map(|_| ())
                        })),
                    );
                    for n_modes in [0, 1] {
                        check(
                            &format!("solve_eigen({n_modes})"),
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                solve_eigen(&model, &dofmap, &reducer, n_modes).map(|_| ())
                            })),
                        );
                    }
                    check(
                        "solve_eigen_with_solver",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            solve_eigen_with_solver(&model, &dofmap, &reducer, 1, solver.as_ref())
                                .map(|_| ())
                        })),
                    );
                    check(
                        "linear_time_history_analysis",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            linear_time_history_analysis(
                                &model,
                                &dofmap,
                                &reducer,
                                &wave,
                                &newmark,
                                &damping,
                                &[],
                                &[],
                                false,
                                None,
                            )
                            .map(|_| ())
                        })),
                    );
                    check(
                        "linear_time_history_with_state",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            linear_time_history_with_state(
                                &model,
                                &dofmap,
                                &reducer,
                                &wave,
                                &newmark,
                                &damping,
                                &[],
                                &[],
                                false,
                                None,
                            )
                            .map(|_| ())
                        })),
                    );
                    check(
                        "linear_time_history_from_state",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            linear_time_history_from_state(
                                &model, &dofmap, &reducer, &wave, &newmark, &damping, &state,
                                false, None,
                            )
                            .map(|_| ())
                        })),
                    );
                    check(
                        "nonlinear_time_history_analysis",
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            nonlinear_time_history_analysis(
                                &model,
                                &dofmap,
                                &reducer,
                                &wave,
                                &newmark,
                                &damping,
                                DampingAccumulation::default(),
                                &[],
                                &[],
                                NonlinearThCfg {
                                    apply_long_term: false,
                                    ..Default::default()
                                },
                            )
                            .map(|_| ())
                        })),
                    );
                    assert_eq!(format!("{model:?}"), before);
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
    for mass_method in [MassMethod::LumpedOnly, MassMethod::CorrectedLumped] {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let mut model = fixture(
                Some(SectionShape::SteelLipChannel {
                    height: 150.0,
                    width: 75.0,
                    lip: 20.0,
                    thick: 2.3,
                }),
                MaterialCategory::Steel,
            );
            model.mass_method = mass_method;
            model.elements[0].kind = kind;
            model.nodes[0].restraint = Dof6Mask::FIXED;
            model.nodes[1].mass = Some([1.0; 6]);
            let dofmap = DofMap::build(&model);
            let reducer = Reducer::build(&model, &dofmap);
            let before = format!("{model:?}");
            assert!(assemble_global_m(&model, &dofmap, MassOption::Consistent).is_ok());
            let modal = solve_eigen(&model, &dofmap, &reducer, 1).unwrap();
            assert_eq!(modal.period.len(), 1);
            assert!(modal.period[0].is_finite() && modal.period[0] > 0.0);
            let wave = GroundMotion {
                dt: 0.01,
                accel_x: vec![0.0, 0.0],
                accel_y: None,
                accel_theta: None,
            };
            let newmark = NewmarkCfg {
                dt: 0.01,
                ..NewmarkCfg::average_accel()
            };
            let damping = Damping::MassProportional {
                h: 0.05,
                omega: 1.0,
            };
            assert!(linear_time_history_analysis(
                &model,
                &dofmap,
                &reducer,
                &wave,
                &newmark,
                &damping,
                &[],
                &[],
                false,
                None
            )
            .is_ok());
            assert_eq!(format!("{model:?}"), before);
        }
    }
}

#[test]
fn linear_time_history_with_state_and_analysis_preserve_actual_wall_options_reason() {
    use sepika_element::factory::ensure_wall_side_column_analysis;
    use sepika_solver::dynamic::timehistory::linear_time_history_with_state;
    use sepika_solver::statics::analysis::Analysis;
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        let mut model = wall_connection_fixture(kind);
        model.wall_plates[0].dl_support = Some(sepika_core::model::WallDlSupport::LowerBeam);
        for element in &mut model.elements[..2] {
            element.end_cond = [EndCondition::Fixed, EndCondition::Pinned];
        }
        for node in &mut model.nodes {
            node.restraint = if node.coord[2] == 0.0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask(0b111110)
            };
            node.mass = Some([1.0; 6]);
        }
        let dofmap = DofMap::build(&model);
        let reducer = Reducer::build(&model, &dofmap);
        let wave = GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0, 0.0],
            accel_y: None,
            accel_theta: None,
        };
        let damping = Damping::MassProportional { h: 0.0, omega: 1.0 };
        for use_kg in [false, true] {
            let expected = ensure_wall_side_column_analysis(
                &model,
                None,
                "線形時刻歴解析",
                use_kg,
                Some(MassOption::Consistent),
            )
            .unwrap_err();
            let error = linear_time_history_with_state(
                &model,
                &dofmap,
                &reducer,
                &wave,
                &NewmarkCfg::average_accel(),
                &damping,
                &[],
                &[],
                use_kg,
                None,
            )
            .unwrap_err();
            assert!(matches!(error, SolveError::InvalidInput(reason) if reason == expected));
            if !use_kg {
                let error = Analysis::prepare(&model)
                    .unwrap()
                    .time_history(
                        &wave,
                        NewmarkCfg::average_accel(),
                        Damping::MassProportional { h: 0.0, omega: 1.0 },
                        None,
                    )
                    .unwrap_err();
                assert!(matches!(error, SolveError::InvalidInput(reason) if reason == expected));
            }
        }
    }
}

#[test]
fn beam_736_wall_column_mass_gate_returns_finite_input_stiffness_diagnostics() {
    use sepika_solver::dynamic::eigen::solve_eigen_with_solver;
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for g in [80_000.0, f64::MAX] {
            let mut model = wall_connection_fixture(kind);
            for node in &mut model.nodes {
                node.restraint = Dof6Mask::FIXED;
            }
            model.materials[0].shear = Some(g);
            let dofmap = DofMap::build(&model);
            let reducer = Reducer::build(&model, &dofmap);
            let solver = sepika_math::solver::make_solver(
                sepika_math::solver::SolverBackend::DirectSparseCholesky,
            );
            let eigen = solve_eigen(&model, &dofmap, &reducer, 0).map(|_| ());
            let eigen_with_solver =
                solve_eigen_with_solver(&model, &dofmap, &reducer, 0, solver.as_ref()).map(|_| ());
            let mass = assemble_global_m(&model, &dofmap, MassOption::Consistent).map(|_| ());
            for (entry, result) in [
                ("eigen", eigen),
                ("eigen_with_solver", eigen_with_solver),
                ("mass", mass),
            ] {
                println!("{kind:?}, G={g:e}, {entry}: {result:?}");
                if g == 80_000.0 {
                    result.unwrap();
                } else {
                    let error = result.unwrap_err().to_string();
                    assert!(
                        error.contains("部材 ID 0・壁 ID [4]")
                            && error.contains("GJ/L")
                            && error.contains("非有限"),
                        "{entry}: {error}"
                    );
                }
            }
            let pdelta = sepika_element::factory::ensure_wall_side_column_analysis(
                &model,
                None,
                "固有値解析",
                true,
                Some(MassOption::Consistent),
            )
            .unwrap_err();
            assert!(
                pdelta.contains("部材 ID 0・壁 ID [4]") && pdelta.contains("P-Δ"),
                "{pdelta}"
            );
        }
    }
}

#[test]
fn eccentric_ground_motion_drives_free_rotation_with_restrained_translations() {
    for kind in [
        ElementKind::Beam,
        ElementKind::Fiber,
        ElementKind::MultiSpring,
    ] {
        for offset_mm in [-100.0_f64, 0.0, 100.0] {
            let mut model = fixture(None, MaterialCategory::Steel);
            model.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
            model.sections[0].area = 10000.0;
            model.sections[0].width = 100.0;
            model.sections[0].depth = 100.0;
            model.sections[0].iy = 10000.0 * 10000.0 / 12.0;
            model.sections[0].iz = 10000.0 * 10000.0 / 12.0;
            model.sections[0].as_y = 1e100;
            model.sections[0].as_z = 1e100;
            model.materials[0].fy = Some(1e10);
            model.elements[0].kind = kind;
            model.elements[0].vertical_eccentricity = Some(offset_mm);
            model.elements[0].plastic_zone = Some(300.0);
            model.nodes[0].restraint = Dof6Mask::FIXED;
            model.nodes[1].restraint = Dof6Mask(0b101111);
            let map = DofMap::build(&model);
            let reducer = Reducer::build(&model, &map);
            assert_eq!(reducer.n_indep, 1);
            let density = 7.85e-9;
            let area = 10000.0;
            let inertia = area * 10000.0 / 12.0;
            let length = LENGTH_MM;
            let stiffness =
                4.0 * 205000.0 * inertia / length + offset_mm.powi(2) * 205000.0 * area / length;
            let mass = density * area * length.powi(3) / 105.0
                + density * inertia * length * 2.0 / 15.0
                + offset_mm.powi(2) * density * area * length / 3.0;
            let actual_mass = assemble_global_m(&model, &map, MassOption::Consistent).unwrap();
            assert!((actual_mass.to_dense()[(0, 0)] / mass - 1.0).abs() < 1e-12);
            let eigen = solve_eigen(&model, &map, &reducer, 1).unwrap();
            assert!((eigen.omega2[0] / (stiffness / mass) - 1.0).abs() < 1e-10);
            let dt = 0.0001;
            let wave = GroundMotion {
                dt,
                accel_x: vec![0.0, 1.0, 1.0],
                accel_y: None,
                accel_theta: None,
            };
            let expected =
                -offset_mm * density * area * length / 2.0 / (stiffness + 4.0 * mass / (dt * dt));
            let linear = linear_time_history_analysis(
                &model,
                &map,
                &reducer,
                &wave,
                &NewmarkCfg::average_accel(),
                &Damping::MassProportional { h: 0.0, omega: 1.0 },
                &[],
                &[],
                false,
                Some(1),
            )
            .unwrap();
            let nonlinear = nonlinear_time_history_analysis(
                &model,
                &map,
                &reducer,
                &wave,
                &NewmarkCfg::average_accel(),
                &Damping::MassProportional { h: 0.0, omega: 1.0 },
                DampingAccumulation::default(),
                &[],
                &[],
                NonlinearThCfg {
                    apply_long_term: false,
                    record_every: Some(1),
                    ..Default::default()
                },
            )
            .unwrap();
            for result in [linear, nonlinear] {
                assert_eq!(result.non_converged_steps, 0);
                let first = result.recording.unwrap().node_disp[1][1][4];
                assert!(
                    (first - expected).abs() < 1e-10 * expected.abs().max(1e-12),
                    "{kind:?} e={offset_mm}: {first} != {expected}"
                );
            }
        }
    }
}

#[test]
fn consistent_ground_motion_keeps_fixed_end_mass_and_nodal_only_input_is_unchanged() {
    for nodal_only in [false, true] {
        let mut model = fixture(None, MaterialCategory::Steel);
        model.nodes[0].restraint = Dof6Mask::FIXED;
        model.nodes[1].restraint = Dof6Mask(0b111110);
        model.nodes[1].mass = Some([0.25, 0.0, 0.0, 0.0, 0.0, 0.0]);
        if nodal_only {
            model.mass_method = MassMethod::LumpedOnly;
        }
        let map = DofMap::build(&model);
        let reducer = Reducer::build(&model, &map);
        let distributed_mass = if nodal_only {
            0.0
        } else {
            12345.0 * 7.85e-9 * LENGTH_MM
        };
        let mass = 0.25 + distributed_mass / 3.0;
        let influence = 0.25 + distributed_mass / 2.0;
        let stiffness = 205000.0 * 12345.0 / LENGTH_MM;
        let dt = 0.0001;
        let expected = -influence / (stiffness + 4.0 * mass / (dt * dt));
        let wave = GroundMotion {
            dt,
            accel_x: vec![0.0, 1.0],
            accel_y: None,
            accel_theta: None,
        };
        let result = linear_time_history_analysis(
            &model,
            &map,
            &reducer,
            &wave,
            &NewmarkCfg::average_accel(),
            &Damping::MassProportional { h: 0.0, omega: 1.0 },
            &[],
            &[],
            false,
            Some(1),
        )
        .unwrap();
        let first = result.recording.unwrap().node_disp[1][1][0];
        assert!(
            (first / expected - 1.0).abs() < 1e-10,
            "{first} != {expected}"
        );
    }
}
