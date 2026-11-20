use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node, StbSectionStrength, StbSteelStrength, Story,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape,
};
use sepika_math::solver::SolveError;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
use sepika_solver::dynamic::timehistory::{
    nonlinear_time_history_analysis, GroundMotion, NewmarkCfg, NonlinearThCfg, ResponseResult,
};
use sepika_solver::nonlinear::pushover::{
    pushover_analysis_recording, PushoverControl, PushoverResult, PushoverTarget,
};
use sepika_solver::statics::analysis::SeismicDir;

const ELEMENT: ElemId = ElemId(0);
const LENGTH_MM: f64 = 50.0;

fn story(id: u32, elevation_mm: f64, node_ids: Vec<NodeId>, seismic_weight_n: f64) -> Story {
    Story {
        id: StoryId(id),
        name: format!("階{id}"),
        elevation: Some(elevation_mm),
        sl_offset_mm: Some(0.0),
        source_story: None,
        node_ids,
        seismic_weight: Some(seismic_weight_n),
        weight_override: None,
        structure: Default::default(),
        level_kind: Default::default(),
        dynamic_mass: None,
        standard_floor_load: None,
        column_finish_area_weight: 0.0,
        fireproof: Default::default(),
        wall_weights: vec![],
    }
}

fn restrained_member(frame_use: FrameSectionUse, web_mm: f64, flange_mm: f64) -> Model {
    let beam = frame_use == FrameSectionUse::Girder;
    let main_dia = RebarSize::DiameterMm(25.0);
    let hoop_dia = RebarSize::DiameterMm(10.0);
    let shape = if beam {
        SectionShape::SrcBeamRect {
            b: 400.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia,
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: hoop_dia,
                    pitch: 100.0,
                    legs: 2,
                },
            },
            steel_height: 300.0,
            steel_width: 150.0,
            steel_web_thick: web_mm,
            steel_flange_thick: flange_mm,
        }
    } else {
        SectionShape::SrcColumnRect {
            b: 400.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia,
                x: vec![5],
                y: vec![3],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: hoop_dia,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
            steel_height: 300.0,
            steel_width: 150.0,
            steel_web_thick: web_mm,
            steel_flange_thick: flange_mm,
        }
    };
    let mut sec = shape.input_section(SectionId(0), "SRC".into()).unwrap();
    sec.frame_use = Some(frame_use);
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(2));
    sec.steel_material = Some(MaterialId(3));
    let concrete = Material {
        id: MaterialId(0),
        name: "Fc24".into(),
        category: MaterialCategory::Concrete,
        young: 23000.0,
        poisson: 0.2,
        density: 0.0,
        shear: None,
        fc: Some(24.0),
        fy: None,
        strength_factor: None,
        concrete_class: Default::default(),
    };
    let steel = Material {
        id: MaterialId(3),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        fc: None,
        fy: Some(80.0),
        ..concrete.clone()
    };
    let main = Material {
        id: MaterialId(1),
        name: "SD345".into(),
        category: MaterialCategory::Rebar,
        fy: Some(345.0),
        ..steel.clone()
    };
    let hoop = Material {
        id: MaterialId(2),
        name: "SD295A".into(),
        fy: Some(295.0),
        ..main.clone()
    };
    let (fixed_coord, free_coord, local_y) = if beam {
        (
            [0.0, 0.0, LENGTH_MM],
            [LENGTH_MM, 0.0, LENGTH_MM],
            [0.0, 1.0, 0.0],
        )
    } else {
        ([0.0; 3], [0.0, 0.0, LENGTH_MM], [1.0, 0.0, 0.0])
    };
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: fixed_coord,
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: free_coord,
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(StoryId(1)),
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: None,
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ELEMENT,
            kind: ElementKind::Fiber,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(sec.id),
            local_axis: LocalAxis {
                ref_vector: local_y,
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![sec],
        materials: vec![concrete, main, hoop, steel],
        stories: vec![
            story(0, 0.0, vec![], 0.0),
            story(1, LENGTH_MM, vec![NodeId(1)], 0.0),
        ],
        ..Default::default()
    }
}

fn run(
    model: &mut Model,
    direction: SeismicDir,
    target_shear_n: f64,
) -> Result<PushoverResult, String> {
    let dof = match direction {
        SeismicDir::X => Dof::Ux,
        SeismicDir::Y => Dof::Uy,
    };
    let member_axis = if model.sections[0].frame_use == Some(FrameSectionUse::Girder) {
        Dof::Ux
    } else {
        Dof::Uz
    };
    let mut restraint = Dof6Mask::FIXED;
    restraint.set_free(dof);
    restraint.set_free(member_axis);
    model.nodes[1].restraint = restraint;
    model.stories[1].seismic_weight = Some(target_shear_n / 0.2);
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    pushover_analysis_recording(
        model,
        &dofmap,
        &reducer,
        direction,
        8,
        PushoverTarget::from_max_disp(0.0),
        PushoverControl::LoadOnly,
        false,
        Default::default(),
    )
}

#[test]
fn src_pushover_event_uses_web_or_flange_f_at_each_independent_shear_threshold() {
    // Fc24、軸力0、SN400B のウェブ10 mmは235、フランジ45 mmは215 N/mm²。
    for (frame_use, direction, qy_n) in [
        (FrameSectionUse::Column, SeismicDir::X, 980_076.778_015_748),
        (
            FrameSectionUse::Column,
            SeismicDir::Y,
            2_395_602.442_002_425,
        ),
        (FrameSectionUse::Girder, SeismicDir::Y, 955_335.210_331_358),
    ] {
        for (load_n, yields) in [(qy_n - 1_000.0, false), (qy_n + 1_000.0, true)] {
            let mut model = restrained_member(frame_use, 10.0, 45.0);
            let result = run(&mut model, direction, load_n)
                .unwrap_or_else(|error| panic!("{frame_use:?} {direction:?} {error}"));
            let response = &result.member_response[0];
            assert!(result.hinges.is_empty(), "{frame_use:?} {direction:?}");
            let (shear_n, other_n) =
                if frame_use == FrameSectionUse::Column && direction == SeismicDir::Y {
                    (response.shear_weak, response.shear_strong)
                } else {
                    (response.shear_strong, response.shear_weak)
                };
            assert!(
                (shear_n - load_n).abs() < 1.0,
                "{frame_use:?} {direction:?} {response:?}"
            );
            assert!(
                other_n.abs() < 1.0 && response.axial.abs() < 1.0,
                "{response:?}"
            );
            if yields {
                let last_step = result.capacity_curve.last().unwrap().step;
                assert_eq!(result.shear_yields.len(), 1);
                let event = &result.shear_yields[0];
                assert_eq!((event.step, event.elem), (last_step, ELEMENT));
                let history = result
                    .member_history
                    .iter()
                    .find(|h| h.elem == ELEMENT)
                    .unwrap();
                assert_eq!(history.records.len(), result.steps.len());
                assert_eq!(result.fiber_states[0].0, ELEMENT);
            } else {
                assert!(
                    result.shear_yields.is_empty(),
                    "{frame_use:?} {direction:?} {response:?}"
                );
                assert!(result.member_history.is_empty());
                assert!(result.fiber_states.is_empty());
            }
        }
    }
}

#[test]
fn src_pushover_reports_why_the_assigned_steel_cannot_supply_a_shear_f() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (name, fy, category, reason) in [
            (None, None, MaterialCategory::Steel, "未割当"),
            (
                Some("独自H"),
                None,
                MaterialCategory::Steel,
                "明示 fy も未入力",
            ),
            (
                Some("独自H"),
                Some(400.0),
                MaterialCategory::Steel,
                "明示 fy が入力",
            ),
            (
                Some("SN400B"),
                Some(235.0),
                MaterialCategory::Rebar,
                "鋼材ではありません",
            ),
            (Some("STK400"), None, MaterialCategory::Steel, "鋼管用"),
            (
                Some("SN400"),
                None,
                MaterialCategory::Steel,
                "種類記号が不完全",
            ),
            (
                Some("TMCP325"),
                Some(325.0),
                MaterialCategory::Steel,
                "個別認定の番号・対象形状・板厚・基準強度",
            ),
            (
                Some("SS490"),
                None,
                MaterialCategory::Steel,
                "溶接を使用する部分",
            ),
            (
                Some("SN400C"),
                None,
                MaterialCategory::Steel,
                "両方に 16 mm 以上",
            ),
        ] {
            let mut model = restrained_member(frame_use, 10.0, 45.0);
            if let Some(name) = name {
                model.materials[3].name = name.into();
            } else {
                model.sections[0].steel_material = None;
            }
            model.materials[3].category = category;
            model.materials[3].fy = fy;
            let error = run(&mut model, SeismicDir::Y, 1_000.0).unwrap_err();
            assert!(
                error.contains("部材 ID 0") && error.contains("せん断降伏耐力"),
                "{error}"
            );
            assert!(name.is_none_or(|name| error.contains(name)), "{error}");
            assert!(error.contains(reason), "{error}");
        }
    }
}

#[test]
fn src_pushover_checks_both_plates_and_respects_external_steel_assignments() {
    for (name, web_mm, flange_mm, reason) in [
        ("SN490C", 16.0, 10.0, "両方に 16 mm 以上"),
        ("SN490C", 16.0, 100.001, "適用範囲"),
        ("SN490C", 100.001, 16.0, "適用範囲"),
        ("SM520B", 16.0, 100.001, "適用範囲"),
        ("SM520C", 100.001, 16.0, "適用範囲"),
    ] {
        let mut model = restrained_member(FrameSectionUse::Column, web_mm, flange_mm);
        model.materials[3].name = name.into();
        let error = run(&mut model, SeismicDir::X, 1_000.0).unwrap_err();
        assert!(
            error.contains("せん断降伏耐力") && error.contains(name) && error.contains(reason),
            "{error}"
        );
    }

    let mut model = restrained_member(FrameSectionUse::Column, 10.0, 12.0);
    model.materials[0].fy = Some(235.0);
    model.materials[3].name = "独自H".into();
    model.materials[3].fy = None;
    let error = run(&mut model, SeismicDir::X, 1_000.0).unwrap_err();
    assert!(
        error.contains("部材 ID 0")
            && error.contains("せん断降伏耐力")
            && error.contains("独自H")
            && error.contains("明示 fy も未入力"),
        "{error}"
    );

    let mut model = restrained_member(FrameSectionUse::Column, 10.0, 45.0);
    model.materials.push(Material {
        id: MaterialId(4),
        name: "STK400".into(),
        fy: Some(235.0),
        ..model.materials[3].clone()
    });
    let steel_input = |material, part, grade: &str| StbSteelStrength {
        native_material: Some(MaterialId(material)),
        element: "StbSecSteelColumn_S_Same".into(),
        part,
        position: None,
        strength: grade.into(),
    };
    model.stb_strengths.sections.push(StbSectionStrength {
        built_box: None,
        native_rebar: false,
        rebar: None,
        section: SectionId(0),
        concrete: None,
        native_material: None,
        reinforcement: vec![],
        steel: vec![steel_input(4, "shape".into(), "STK400")],
    });
    let error = run(&mut model, SeismicDir::X, 1_000.0).unwrap_err();
    assert!(
        error.contains("せん断降伏耐力") && error.contains("STK400") && error.contains("鋼管用"),
        "{error}"
    );

    model.stb_strengths.sections[0]
        .steel
        .push(steel_input(3, "flange".into(), "SN400B"));
    let error = run(&mut model, SeismicDir::X, 1_000.0).unwrap_err();
    assert!(
        error.contains("単一の参照先") && error.contains("STK400") && error.contains("SN400B"),
        "{error}"
    );
}

fn run_time_history(model: &mut Model) -> Result<ResponseResult, SolveError> {
    model.nodes[1].restraint.set_free(Dof::Uy);
    model.nodes[1].mass = Some([1.0; 6]);
    model.materials[0].density = 2.4e-9;
    model.materials[3].density = 7.85e-9;
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    nonlinear_time_history_analysis(
        model,
        &dofmap,
        &reducer,
        &GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 3],
            accel_y: Some(vec![0.0, 10.0, 0.0]),
            accel_theta: None,
        },
        &NewmarkCfg::average_accel(),
        &Damping::MassProportional { h: 0.0, omega: 1.0 },
        DampingAccumulation::default(),
        &[],
        &[],
        NonlinearThCfg::default(),
    )
}

#[test]
fn src_time_history_reports_invalid_fiber_steel_fy() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (fy, reason) in [
            (None, "未入力"),
            (Some(0.0), "正の有限値"),
            (Some(f64::INFINITY), "正の有限値"),
        ] {
            let mut model = restrained_member(frame_use, 10.0, 12.0);
            model.materials[0].fy = Some(235.0);
            model.materials[3].name = "独自H".into();
            model.materials[3].fy = fy;
            let Err(SolveError::InvalidInput(error)) = run_time_history(&mut model) else {
                panic!("内蔵鉄骨の fy を解決できないときは入力エラーが必要です");
            };
            for expected in ["部材 ID 0", "内蔵鉄骨", "独自H", "降伏強度 fy", reason] {
                assert!(error.contains(expected), "{frame_use:?}: {error}");
            }
        }
    }
}

#[test]
fn src_time_history_rejects_nonsteel_fiber_materials() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (name, fy, category, category_label) in [
            (
                "独自H",
                Some(235.0),
                MaterialCategory::Concrete,
                "コンクリート",
            ),
            ("SN400B", None, MaterialCategory::Rebar, "鉄筋"),
        ] {
            let mut model = restrained_member(frame_use, 10.0, 12.0);
            model.materials[3].name = name.into();
            model.materials[3].fy = fy;
            model.materials[3].category = category;
            let error = match run_time_history(&mut model) {
                Err(SolveError::InvalidInput(error)) => error,
                _ => panic!(
                    "{frame_use:?}: 内蔵鉄骨の材料区分が {category_label} なら入力エラーが必要です"
                ),
            };
            for expected in [
                "部材 ID 0",
                "内蔵鉄骨",
                name,
                "材料区分",
                category_label,
                "鋼材",
            ] {
                assert!(error.contains(expected), "{frame_use:?}: {error}");
            }
        }
    }
}

#[test]
fn src_time_history_accepts_assigned_fiber_steel_fy() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (name, fy) in [("独自H", Some(235.0)), ("SN400B", None)] {
            let mut model = restrained_member(frame_use, 10.0, 12.0);
            model.materials[3].name = name.into();
            model.materials[3].fy = fy;
            let result = run_time_history(&mut model).unwrap();
            assert!(result.nonlinear);
            assert_eq!(result.time.len(), 3);
            assert!(result.history.node_disp.iter().any(|&u| u.abs() > 0.0));
            assert!(result.history.node_disp.iter().all(|u| u.is_finite()));
        }
    }
}
