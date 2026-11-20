use sepika_core::dof::{Dof, Dof6Mask, DofMap};
use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
    MaterialCategory, Model, Node, Story,
};
use sepika_core::section_shape::SectionShape;
use sepika_math::solver::SolveError;
use sepika_solver::common::constraint::Reducer;
use sepika_solver::dynamic::damping::{Damping, DampingAccumulation};
use sepika_solver::dynamic::timehistory::{
    nonlinear_time_history_analysis, GroundMotion, NewmarkCfg, NonlinearThCfg, ResponseResult,
};
use sepika_solver::nonlinear::pushover::{
    pushover_analysis_recording, PushoverControl, PushoverResult, PushoverTarget,
    PushoverTermination,
};
use sepika_solver::statics::analysis::SeismicDir;

const HEIGHT_MM: f64 = 3000.0;

fn column_shapes() -> [(SectionShape, &'static str); 2] {
    [
        (
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
            "STK400",
        ),
        (
            SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(30.0),
            },
            "STKR400",
        ),
    ]
}

fn column(shape: SectionShape, steel_name: &str, steel_fy: Option<f64>) -> Model {
    let mut section = shape.input_section(SectionId(0), "CFT柱".into()).unwrap();
    section.frame_use = Some(FrameSectionUse::Column);
    section.steel_material = Some(MaterialId(0));
    section.material = Some(MaterialId(1));
    let mut top_restraint = Dof6Mask::FIXED;
    top_restraint.set_free(Dof::Ux);
    top_restraint.set_free(Dof::Ry);
    let base = Story::new(StoryId(0), "1F".into(), Some(0.0), Some(0.0));
    let mut top = Story::new(StoryId(1), "2F".into(), Some(HEIGHT_MM), Some(0.0));
    top.node_ids = vec![NodeId(1)];
    top.seismic_weight = Some(10_000.0);
    Model {
        nodes: vec![
            Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(StoryId(0)),
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, HEIGHT_MM],
                restraint: top_restraint,
                mass: None,
                story: Some(StoryId(1)),
                support_spring: None,
            },
        ],
        elements: vec![ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(section.id),
            local_axis: LocalAxis {
                ref_vector: [1.0, 0.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        }],
        sections: vec![section],
        materials: vec![
            Material {
                id: MaterialId(0),
                name: steel_name.into(),
                category: MaterialCategory::Steel,
                young: 205_000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: steel_fy,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 25_000.0,
                poisson: 0.2,
                density: 2.3e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ],
        stories: vec![base, top],
        ..Default::default()
    }
}

fn time_history(model: &Model, accel_x: [f64; 3]) -> Result<ResponseResult, SolveError> {
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    nonlinear_time_history_analysis(
        model,
        &dofmap,
        &reducer,
        &GroundMotion {
            dt: 0.01,
            accel_x: accel_x.to_vec(),
            accel_y: None,
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

fn pushover(model: &Model) -> Result<PushoverResult, String> {
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    pushover_analysis_recording(
        model,
        &dofmap,
        &reducer,
        SeismicDir::X,
        8,
        PushoverTarget::from_max_disp(0.0),
        PushoverControl::LoadOnly,
        false,
        Default::default(),
    )
}

#[test]
fn public_nonlinear_analyses_report_unresolved_cft_tube_fy() {
    for (shape, _) in column_shapes() {
        for (fy, reason) in [
            (None, "明示 fy も未入力"),
            (Some(0.0), "正の有限値"),
            (Some(f64::INFINITY), "正の有限値"),
        ] {
            let mut model = column(shape.clone(), "独自鋼管", fy);
            model.materials[1].fy = Some(235.0);
            let SolveError::InvalidInput(error) =
                time_history(&model, [0.0, 10.0, 0.0]).unwrap_err()
            else {
                panic!("鋼管の fy を解決できないときは入力エラーが必要です");
            };
            for error in [error, pushover(&model).unwrap_err()] {
                for expected in ["部材 ID 0", "鋼管材料「独自鋼管」", "降伏強度 fy", reason]
                {
                    assert!(error.contains(expected), "{shape:?}: {error}");
                }
            }
        }
    }
}

#[test]
fn public_time_history_rejects_infinite_cft_tube_fy_at_zero_acceleration() {
    let (shape, _) = column_shapes().into_iter().next().unwrap();
    let model = column(shape, "独自鋼管", Some(f64::INFINITY));
    match time_history(&model, [0.0; 3]) {
        Err(SolveError::InvalidInput(reason)) => assert!(reason.contains("正の有限値"), "{reason}"),
        Err(error) => panic!("入力診断を期待しましたが解析は別の理由で失敗しました: {error}"),
        Ok(_) => panic!("鋼管の非有限の降伏強度 fy で解析が成功しました"),
    }
}

#[test]
fn public_nonlinear_analyses_accept_cft_tube_grade_or_explicit_fy() {
    for (shape, grade) in column_shapes() {
        for (name, fy) in [(grade, None), ("独自鋼管", Some(235.0))] {
            let model = column(shape.clone(), name, fy);
            let response = time_history(&model, [0.0, 10.0, 0.0]).unwrap();
            assert!(response.nonlinear);
            assert_eq!(response.time.len(), 3);
            assert!(response.history.node_disp.iter().all(|u| u.is_finite()));
            assert!(response.history.node_disp.iter().any(|&u| u.abs() > 0.0));
            let response = pushover(&model).unwrap();
            assert_eq!(
                response.termination,
                PushoverTermination::LambdaCap { lambda: 1.0 }
            );
            let top_disp_mm = response.steps.last().unwrap().top_disp;
            assert!(top_disp_mm.is_finite() && top_disp_mm > 0.0);
        }
    }
}
