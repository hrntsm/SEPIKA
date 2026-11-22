use super::App;
use sepika_core::model::{Model, NodalLoad, EY_CASE_NAME};
use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::linear_static_once;

#[path = "frame_gravity.rs"]
mod frame_gravity;

pub fn rc_rank_portal(
    mut model: Model,
    circular_column: bool,
    story_height_mm: f64,
    beam_faces: Option<(Option<f64>, Option<f64>)>,
) -> App {
    use sepika_core::dof::{Dof, Dof6Mask};
    use sepika_core::ids::{NodeId, SectionId};
    use sepika_core::model::{Constraint, ElementKind, EndCondition, Node, ZoneSource};
    use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar, SectionShape};

    model.elements.pop();
    model.sections.truncate(2);
    for node in &mut model.nodes[2..] {
        node.coord[2] = story_height_mm;
        for dof in [Dof::Rx, Dof::Ry] {
            node.restraint.set_fixed(dof);
        }
    }
    model.stories[1].elevation = Some(story_height_mm);
    let story = model.stories[1].id;
    let [left, right] = [model.nodes[2].id, model.nodes[3].id];
    let floor = if circular_column {
        Constraint::rigid_diaphragm(story, right, vec![left])
    } else {
        let master = NodeId(model.nodes.len() as u32);
        model.nodes.push(Node {
            id: master,
            coord: [2000.0, 0.0, story_height_mm],
            restraint: Dof6Mask(0b011100),
            mass: None,
            story: Some(story),
            support_spring: None,
        });
        Constraint::rigid_diaphragm(story, master, vec![left, right])
    };
    model.constraints.push(floor);
    model.elements[2].end_cond = [EndCondition::Pinned; 2];
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
        model.elements[0].end_cond = [EndCondition::Pinned; 2];
        for column in &mut model.elements[..2] {
            column.rigid_zone.source_i = ZoneSource::Manual;
            column.rigid_zone.source_j = ZoneSource::Manual;
        }
        model.elements[1].section = Some(SectionId(2));
        model.sections.push(sepika_core::model::Section {
            id: SectionId(2),
            name: "S2".into(),
            ..old
        });
    }

    let mut app = App::default();
    frame_gravity::select_frame_gravity(&mut model);
    app.load_model(model);
    app.core.design_frame = sepika_design_jp::secondary::holding_capacity::FrameType::RcFrame;
    app.core.analysis_cfg.threads = 1;
    app.core.analysis_cfg.push_apply_long_term = true;
    app.core.analysis_cfg.push_steps = 2;
    app.core.analysis_cfg.push_use_max_disp = true;
    app.core.analysis_cfg.push_use_drift_angle = false;
    app.core.analysis_cfg.push_max_disp = 0.002;
    app.run_preparation();
    if circular_column {
        balance_y_floor_rotation(&mut app.core.model);
    }
    app.run_seismic(SeismicDir::X);
    app.run_seismic(SeismicDir::Y);
    app.run_pushover();
    assert!(
        app.core.scoped.last_error.is_none(),
        "高さ {story_height_mm} mm・円柱 {circular_column}・梁フェース {beam_faces:?}: {:?}",
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

fn balance_y_floor_rotation(model: &mut Model) {
    let story = model.layers()[0].top;
    let master = model.diaphragms_of(story).next().unwrap().master;
    let ey = model
        .load_cases
        .iter()
        .find(|case| case.name == EY_CASE_NAME)
        .unwrap()
        .id;
    let response = linear_static_once(model, ey).unwrap();
    let mut torsion_model = model.clone();
    let moment_nmm = 1_000_000.0;
    let case = &mut torsion_model.load_cases[ey.index()];
    case.member.clear();
    case.nodal = vec![NodalLoad::manual(
        master,
        [0.0, 0.0, 0.0, 0.0, 0.0, moment_nmm],
    )];
    let torsion = linear_static_once(&torsion_model, ey).unwrap();
    let balance_nmm =
        -moment_nmm * response.disp[master.index()][5] / torsion.disp[master.index()][5];
    model.load_cases[ey.index()].nodal.push(NodalLoad::manual(
        master,
        [0.0, 0.0, 0.0, 0.0, 0.0, balance_nmm],
    ));
}
