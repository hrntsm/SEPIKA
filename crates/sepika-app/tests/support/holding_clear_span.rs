use super::App;
use sepika_core::model::Model;
use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
use sepika_solver::statics::analysis::SeismicDir;

#[path = "frame_gravity.rs"]
mod frame_gravity;

pub fn rc_rank_portal(
    mut model: Model,
    circular_column: bool,
    story_height_mm: f64,
    beam_faces: Option<(Option<f64>, Option<f64>)>,
) -> App {
    use sepika_core::model::ElementKind;
    use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar, SectionShape};

    model.elements.pop();
    model.sections.truncate(2);
    for node in &mut model.nodes[2..] {
        node.coord[2] = story_height_mm;
    }
    model.stories[1].elevation = Some(story_height_mm);
    if let Some(faces) = beam_faces {
        let beam = &mut model.elements[2];
        // 梁要素では準備計算がフェース距離を更新する。
        beam.kind = ElementKind::Fiber;
        (beam.rigid_zone.face_i, beam.rigid_zone.face_j) = faces;
    }
    if circular_column {
        let section = &mut model.sections[0];
        let old = section.clone();
        *section = SectionShape::RcColumnCircle {
            d: 600.0,
            rebar: RcCircleColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                count: 8,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                },
            },
        }
        .to_section(old.id, old.name);
        section.frame_use = old.frame_use;
        section.material = old.material;
        section.rebar_material = old.rebar_material;
        section.shear_rebar_material = old.shear_rebar_material;
    }

    let mut app = App::default();
    frame_gravity::select_frame_gravity(&mut model);
    app.load_model(model);
    app.core.design_frame = sepika_design_jp::secondary::holding_capacity::FrameType::RcFrame;
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.push_apply_long_term = false;
    app.core.analysis_cfg.push_steps = 2;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.core.analysis_cfg.push_max_disp = 0.2;
    app.run_preparation();
    app.run_seismic(SeismicDir::X);
    app.run_seismic(SeismicDir::Y);
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "{:?}",
        app.core.scoped.last_error
    );
    app.core.design_rank_auto = true;
    let po = app
        .core
        .scoped
        .results
        .as_mut()
        .unwrap()
        .pushover_x
        .as_mut()
        .unwrap();
    let step = po.capacity_curve.last().unwrap().step;
    let mut ds = po
        .evaluation_point(
            EvaluationPurpose::Ds,
            SeismicDir::X,
            step,
            "終端確定ステップ".into(),
        )
        .unwrap();
    ds.member_capacities_n = po
        .member_response
        .iter()
        .map(|member| (member.elem, 1_000_000.0))
        .collect();
    po.ds_evaluation = Some(ds);
    po.capacity_evaluation = Some(
        po.evaluation_point(
            EvaluationPurpose::HoldingCapacity,
            SeismicDir::X,
            step,
            "終端確定ステップ".into(),
        )
        .unwrap(),
    );
    app
}
