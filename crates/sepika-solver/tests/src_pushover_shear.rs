use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node, Story,
};
use sepika_core::section_shape::{
    BeamStirrup, CircleColumnHoop, RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar, RebarSize,
    RectColumnHoop, SectionShape, SrcHManufacture,
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
            steel_manufacture: SrcHManufacture::BuiltUp,
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
            steel_manufacture: SrcHManufacture::BuiltUp,
        }
    };
    restrained_member_with_shape(frame_use, shape, Some(MaterialId(3)))
}

fn restrained_member_with_shape(
    frame_use: FrameSectionUse,
    shape: SectionShape,
    steel_material: Option<MaterialId>,
) -> Model {
    let beam = frame_use == FrameSectionUse::Girder;
    let mut sec = shape.input_section(SectionId(0), "断面".into()).unwrap();
    sec.frame_use = Some(frame_use);
    sec.material = Some(MaterialId(0));
    sec.rebar_material = Some(MaterialId(1));
    sec.shear_rebar_material = Some(MaterialId(2));
    sec.steel_material = steel_material;
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
fn src_column_same_step_face_actions_include_distributed_load_and_reject_point_at_face() {
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination, MemberLoad, MemberLoadKind};

    let mut model = restrained_member(FrameSectionUse::Column, 10.0, 13.0);
    model.elements[0].rigid_zone.face_i = Some(5.0);
    model.elements[0].rigid_zone.face_j = Some(7.0);
    model.nodes[1].restraint.set_free(Dof::Ux);
    model.nodes[1].restraint.set_free(Dof::Uz);
    model.stories[1].seismic_weight = Some(5_000.0);
    let distribute = |dir, w| MemberLoad::full_length_uniform(ELEMENT, dir, LENGTH_MM, w);
    model.load_cases = vec![
        LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "自重".into(),
            kind: LoadCaseKind::Dead,
            nodal: vec![],
            member: vec![
                distribute([0.0, 0.0, -1.0], 20.0),
                distribute([1.0, 0.0, 0.0], 11.0),
            ],
        },
        LoadCase {
            origin: Default::default(),
            id: LoadCaseId(1),
            name: "架構積載".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        },
    ];
    model.combinations = vec![LoadCombination {
        name: "G+P".into(),
        terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
    }];
    model.ultimate_gravity_combination = Some(0);
    let run = |model: &Model| {
        let map = DofMap::build(model);
        let reducer = Reducer::build(model, &map);
        pushover_analysis_recording(
            model,
            &map,
            &reducer,
            SeismicDir::X,
            2,
            PushoverTarget::from_max_disp(0.0),
            PushoverControl::LoadOnly,
            true,
            Default::default(),
        )
        .unwrap()
        .confirmed_history
        .unwrap()
        .remove(0)
        .src_frame_faces
        .remove(0)
    };
    let recorded = run(&model);
    assert_eq!(recorded.elem, ELEMENT);
    let [i, j] = recorded.faces.map(Result::unwrap);
    assert!((j[0] - i[0] - 760.0).abs() < 1e-7);
    assert!((j[1] - i[1] - 418.0).abs() < 1e-7);
    assert!((j[5] - i[5] - i[1] * 38.0 - 7942.0).abs() < 1e-7);

    model.load_cases[0].member.push(MemberLoad::manual(
        ELEMENT,
        [1.0, 0.0, 0.0],
        MemberLoadKind::Point { a: 5.0, p: 50.0 },
    ));
    let [i, j] = run(&model).faces;
    assert!(i.unwrap_err().contains("集中部材荷重"));
    assert!(j.is_ok());
}

fn shear_reinforced_members() -> [(&'static str, Model, SeismicDir); 5] {
    let src_beam = restrained_member(FrameSectionUse::Girder, 10.0, 13.0);
    let src_column = restrained_member(FrameSectionUse::Column, 10.0, 13.0);
    let rc_beam = SectionShape::RcBeamRect {
        b: 400.0,
        d: 600.0,
        rebar: src_beam.sections[0]
            .shape
            .as_ref()
            .and_then(SectionShape::beam_rebar)
            .unwrap()
            .clone(),
    };
    let rc_rect_column = SectionShape::RcColumnRect {
        b: 400.0,
        d: 600.0,
        rebar: src_column.sections[0]
            .shape
            .as_ref()
            .and_then(SectionShape::rect_column_rebar)
            .unwrap()
            .clone(),
    };
    let rc_circle_column = SectionShape::RcColumnCircle {
        d: 600.0,
        rebar: RcCircleColumnRebar {
            main_dia: RebarSize::DiameterMm(25.0),
            count: 16,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: RebarSize::DiameterMm(10.0),
                pitch: 100.0,
            },
        },
    };
    [
        (
            "RC矩形梁",
            restrained_member_with_shape(FrameSectionUse::Girder, rc_beam, None),
            SeismicDir::Y,
        ),
        (
            "RC矩形柱",
            restrained_member_with_shape(FrameSectionUse::Column, rc_rect_column, None),
            SeismicDir::X,
        ),
        (
            "RC円形柱",
            restrained_member_with_shape(FrameSectionUse::Column, rc_circle_column, None),
            SeismicDir::X,
        ),
        ("SRC矩形梁", src_beam, SeismicDir::Y),
        ("SRC矩形柱", src_column, SeismicDir::X),
    ]
}

#[test]
fn pushover_requires_rebar_material_category_for_rc_and_src_shear_reinforcement() {
    for (name, mut model, direction) in shear_reinforced_members() {
        model.materials[2].name = "SD345".into();
        model.materials[2].fy = Some(345.0);
        for (category, label) in [
            (MaterialCategory::Steel, "鋼材"),
            (MaterialCategory::Concrete, "コンクリート"),
        ] {
            model.materials[2].category = category;
            let error = run(&mut model, direction, 1_000.0).unwrap_err();
            for expected in [
                "部材 ID 0",
                "せん断補強筋",
                "材料「SD345」",
                label,
                "せん断降伏耐力",
                "断面タブ",
                "区分「鉄筋」",
                "再解析",
            ] {
                assert!(error.contains(expected), "{name} {expected}: {error}");
            }
        }
        model.materials[2].category = MaterialCategory::Rebar;
        let result =
            run(&mut model, direction, 1_000.0).unwrap_or_else(|error| panic!("{name}: {error}"));
        let shear_n = result.member_response[0].shear_strong;
        assert!((shear_n - 1_000.0).abs() < 1.0, "{name}: {shear_n}");
    }
}

#[test]
fn pushover_requires_finite_positive_shear_rebar_fy() {
    for (name, mut model, direction) in shear_reinforced_members() {
        let material = model.sections[0].shear_rebar_material.unwrap().index();
        for fy in [
            Some(f64::INFINITY),
            Some(f64::NAN),
            Some(f64::NEG_INFINITY),
            Some(0.0),
            Some(-1.0),
        ] {
            model.materials[material].fy = fy;
            let Err(error) = run(&mut model, direction, 1_000.0) else {
                panic!("{name} {fy:?}: 解析を停止する必要があります");
            };
            for expected in [
                "部材 ID 0",
                "せん断補強筋材料「SD295A」",
                "降伏強度 Fy",
                "材料タブ",
                "正の有限値",
            ] {
                assert!(
                    error.contains(expected),
                    "{name} {fy:?} {expected}: {error}"
                );
            }
        }

        model.materials[material].fy = None;
        let Err(error) = run(&mut model, direction, 1_000.0) else {
            panic!("{name}: 降伏強度が未入力のときも解析を停止する必要があります");
        };
        assert!(
            error.contains("部材 ID 0")
                && error.contains("せん断補強筋材料「SD295A」に降伏強度 fy が設定されていません")
                && error.contains("材料タブで fy を設定"),
            "{name}: {error}"
        );
        model.materials[material].fy = Some(295.0);
        let result =
            run(&mut model, direction, 1_000.0).unwrap_or_else(|error| panic!("{name}: {error}"));
        let shear_n = result.member_response[0].shear_strong;
        assert!((shear_n - 1_000.0).abs() < 1.0, "{name}: {shear_n}");
    }
}

#[test]
fn src_pushover_recovers_shear_in_both_directions_without_a_shear_yield_event() {
    for (frame_use, direction, web_mm, flange_mm, load_n) in [
        (
            FrameSectionUse::Column,
            SeismicDir::X,
            10.0,
            45.0,
            981_000.0,
        ),
        (
            FrameSectionUse::Column,
            SeismicDir::Y,
            10.0,
            45.0,
            2_396_000.0,
        ),
        (
            FrameSectionUse::Girder,
            SeismicDir::Y,
            10.0,
            45.0,
            956_000.0,
        ),
        (
            FrameSectionUse::Column,
            SeismicDir::X,
            45.0,
            13.0,
            2_351_000.0,
        ),
        (
            FrameSectionUse::Column,
            SeismicDir::Y,
            45.0,
            13.0,
            1_135_000.0,
        ),
    ] {
        let mut model = restrained_member(frame_use, web_mm, flange_mm);
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
        assert!(
            result.shear_yields.is_empty(),
            "{frame_use:?} {direction:?} {response:?}"
        );
        assert!(result.member_history.is_empty());
        assert!(result.fiber_states.is_empty());
    }
}

#[test]
fn src_pushover_reports_why_internal_fiber_steel_cannot_be_used() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (name, fy, category, reason) in [
            (None, None, MaterialCategory::Steel, "設定されていません"),
            (
                Some("独自H"),
                None,
                MaterialCategory::Steel,
                "適用根拠が未確定",
            ),
            (
                Some("SN400B"),
                Some(235.0),
                MaterialCategory::Rebar,
                "材料区分は「鉄筋」",
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
                error.contains("部材 ID 0") && error.contains("内蔵鉄骨"),
                "{error}"
            );
            assert!(name.is_none_or(|name| error.contains(name)), "{error}");
            assert!(error.contains(reason), "{error}");
        }
    }
}

#[test]
fn src_pushover_requires_strength_of_the_explicitly_assigned_internal_steel() {
    let mut model = restrained_member(FrameSectionUse::Column, 10.0, 12.0);
    model.materials[0].fy = Some(235.0);
    model.materials.push(Material {
        id: MaterialId(4),
        name: "独自H".into(),
        young: 103_000.0,
        fy: Some(400.0),
        ..model.materials[3].clone()
    });
    model.sections[0].steel_material = Some(MaterialId(4));
    let error = run(&mut model, SeismicDir::X, 1_000.0).unwrap_err();
    for expected in ["部材 ID 0", "内蔵鉄骨", "独自H", "適用根拠が未確定"] {
        assert!(error.contains(expected), "{error}");
    }
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
fn time_history_accepts_src_member_without_using_shear_reinforcement_strength() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        let mut model = restrained_member(frame_use, 10.0, 13.0);
        model.materials[2].name = "SD345".into();
        model.materials[2].fy = Some(345.0);
        model.materials[2].category = MaterialCategory::Steel;
        run_time_history(&mut model).unwrap_or_else(|error| panic!("{frame_use:?}: {error}"));
        model.materials[2].category = MaterialCategory::Rebar;
        model.materials[2].fy = Some(f64::INFINITY);
        run_time_history(&mut model).unwrap_or_else(|error| panic!("{frame_use:?}: {error}"));
    }
}

#[test]
fn src_nonlinear_analysis_identifies_unconfirmed_manufacture_and_plate_thickness() {
    for frame_use in [FrameSectionUse::Column, FrameSectionUse::Girder] {
        for (manufacture, web_mm, flange_mm, reason) in [
            (SrcHManufacture::Unknown, 10.0, 12.0, "製造区分が不明"),
            (
                SrcHManufacture::Rolled,
                10.0,
                12.0,
                "圧延H形鋼のウェブ・フランジに適用する降伏強度の根拠が未確認",
            ),
            (SrcHManufacture::BuiltUp, 5.0, 12.0, "ウェブの実板厚 5 mm"),
            (
                SrcHManufacture::BuiltUp,
                10.0,
                101.0,
                "フランジ: 鋼材「SN400B」の実板厚 101 mm",
            ),
        ] {
            let mut model = restrained_member(frame_use, web_mm, flange_mm);
            match model.sections[0].shape.as_mut().unwrap() {
                SectionShape::SrcBeamRect {
                    steel_manufacture, ..
                }
                | SectionShape::SrcColumnRect {
                    steel_manufacture, ..
                } => *steel_manufacture = manufacture,
                _ => unreachable!(),
            }
            model.materials[3].fy = Some(235.0);
            let Err(SolveError::InvalidInput(error)) = run_time_history(&mut model.clone()) else {
                panic!("適用条件を確認できないSRC断面は入力エラーが必要です");
            };
            let pushover_error = run(&mut model, SeismicDir::Y, 1_000.0).unwrap_err();
            for error in [error, pushover_error] {
                for expected in ["部材 ID 0", "内蔵鉄骨", "SN400B", reason] {
                    assert!(error.contains(expected), "{frame_use:?}: {error}");
                }
            }
        }
    }
}

#[test]
fn rigid_diaphragm_src_girder_keeps_auto_spring_but_explicit_fiber_requires_manufacture() {
    use sepika_core::model::Constraint;

    let mut model = restrained_member(FrameSectionUse::Girder, 10.0, 13.0);
    model.elements[0].kind = ElementKind::Beam;
    model
        .constraints
        .push(Constraint::rigid_diaphragm(StoryId(1), NodeId(1), vec![]));
    let Some(SectionShape::SrcBeamRect {
        steel_manufacture, ..
    }) = model.sections[0].shape.as_mut()
    else {
        unreachable!();
    };
    *steel_manufacture = SrcHManufacture::Unknown;

    let result = run(&mut model, SeismicDir::Y, 1_000.0).unwrap();
    assert!((result.member_response[0].shear_strong - 1_000.0).abs() < 1.0);
    assert!(result.fiber_states.is_empty());

    model.elements[0].force_regime = ForceRegime::AxialBendingInteract;
    let error = run(&mut model, SeismicDir::Y, 1_000.0).unwrap_err();
    assert!(
        error.contains("部材 ID 0") && error.contains("製造区分が不明"),
        "{error}"
    );
}

#[test]
fn src_material_strength_rejects_factor_above_the_notice_limit() {
    let mut model = restrained_member(FrameSectionUse::Column, 45.0, 13.0);
    model.materials[3].strength_factor = Some(1.11);
    let error = run(&mut model, SeismicDir::Y, 1_000.0).unwrap_err();
    for expected in ["部材 ID 0", "内蔵鉄骨", "材料強度割増 1.11", "1.1以下"] {
        assert!(error.contains(expected), "{error}");
    }
    assert!(run_time_history(&mut model).is_ok());
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
        for (name, fy, web_mm, flange_mm) in [
            ("SN400B", Some(80.0), 10.0, 45.0),
            ("SN400B", None, 10.0, 12.0),
            ("SN400B", None, 45.0, 13.0),
        ] {
            let mut model = restrained_member(frame_use, web_mm, flange_mm);
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

#[test]
fn src_fiber_response_uses_each_plate_strength_for_axial_and_bending() {
    use sepika_core::model::AnalysisKind;
    use sepika_element::behavior::{Ctx, LocalVec};
    use sepika_element::factory::{
        build_nonlinear_behavior, ensure_nonlinear_input_with_basis, StrengthBasis,
    };

    for (basis, kind, factor) in [
        (StrengthBasis::Nominal, AnalysisKind::TimeHistory, 1.0),
        (
            StrengthBasis::MaterialStrength,
            AnalysisKind::Incremental,
            1.1,
        ),
    ] {
        for (
            web_mm,
            flange_mm,
            expected_fy,
            expected_area_mm2,
            expected_plastic_axial_n,
            expected_plastic_moment_n_mm,
        ) in [
            (
                45.0,
                13.0,
                [215.0, 235.0],
                [12_330.0, 3_900.0],
                3_567_450.0,
                313_107_825.0,
            ),
            (
                10.0,
                45.0,
                [235.0, 215.0],
                [2_100.0, 13_500.0],
                3_396_000.0,
                395_977_500.0,
            ),
            (
                10.0,
                40.0,
                [235.0, 235.0],
                [2_200.0, 12_000.0],
                3_337_000.0,
                395_035_000.0,
            ),
        ] {
            let mut model = restrained_member(FrameSectionUse::Column, web_mm, flange_mm);
            model.nodes[1].coord[2] = 3_000.0;
            model.materials[3].fy = Some(80.0);
            let context = Ctx { model: &model };
            model.validate_local_frames().unwrap();
            ensure_nonlinear_input_with_basis(&model, kind, basis).unwrap();

            let mut axial = LocalVec {
                data: smallvec::smallvec![0.0; 12],
            };
            axial.data[8] = 3_000.0 * 225.0 * factor / 205_000.0;
            let mut bending = LocalVec {
                data: smallvec::smallvec![0.0; 12],
            };
            bending.data[4] = 0.02;
            bending.data[10] = -0.02;

            for (displacement, force_index) in [(axial, 8), (bending, 4)] {
                let mut member =
                    build_nonlinear_behavior(&model.elements[0], &model, basis, kind, None);
                member.update_state(&displacement, true, &context).unwrap();
                let response = member.internal_force(&context).data[force_index];
                assert!(response.is_finite() && response.abs() > 1.0, "{response}");

                for section in member.fiber_section_states().unwrap() {
                    let mut area_mm2 = [0.0; 2];
                    let mut plastic_axial_n = 0.0;
                    let mut plastic_moment_n_mm = 0.0;
                    for fiber in section.fibers.iter().filter(|fiber| fiber.material == 2) {
                        let plate = usize::from(fiber.y.abs() > 150.0 - flange_mm);
                        area_mm2[plate] += fiber.area;
                        let fy = if fiber.strain.abs() > 1e-12 {
                            205_000.0 * fiber.strain.abs() / fiber.yield_ratio
                        } else {
                            assert_eq!(force_index, 4);
                            assert_eq!(fiber.y, 0.0);
                            expected_fy[plate] * factor
                        };
                        assert!((fy - expected_fy[plate] * factor).abs() < 1e-8, "{fiber:?}");
                        plastic_axial_n += fy * fiber.area;
                        plastic_moment_n_mm += fy * fiber.area * fiber.y.abs();
                    }
                    for i in 0..2 {
                        assert!((area_mm2[i] - expected_area_mm2[i]).abs() < 1e-8);
                    }
                    assert!((plastic_axial_n / factor - expected_plastic_axial_n).abs() < 1.0);
                    assert!(
                        (plastic_moment_n_mm / factor / expected_plastic_moment_n_mm - 1.0).abs()
                            < 0.002
                    );
                }
            }
        }
    }
}
