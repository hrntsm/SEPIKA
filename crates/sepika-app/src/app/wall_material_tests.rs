use super::*;
use sepika_core::ids::{WallPlateId, WallRegionId};
use sepika_core::model::{
    MaterialCategory, Model, PropertyBasis, WallPlate, WallPlateShape, WallRegion,
};
use sepika_core::section_shape::{
    BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop, SectionShape,
};

fn portal_with_wall_plate() -> sepika_core::model::Model {
    let mut model =
        crate::app::tests::with_explicit_pushover_gravity(crate::sample::portal_frame());
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    let mut bottom = model.elements[2].clone();
    bottom.id = ElemId(3);
    bottom.nodes = [NodeId(0), NodeId(1)].into_iter().collect();
    model.elements.push(bottom);
    let mut section = model.sections[1].clone();
    section.id = SectionId(2);
    section.name = "壁".into();
    section.frame_use = None;
    section.shape = None;
    section.property_basis.area = PropertyBasis::Supplied;
    section.property_basis.j = PropertyBasis::Supplied;
    section.thickness = Some(6.0);
    model.sections.push(section);
    let boundary = vec![NodeId(0), NodeId(1), NodeId(3), NodeId(2)];
    model.add_enclosed_wall_plate_from_nodes(
        &boundary,
        WallPlate {
            src_wall_detailing_confirmed: false,
            src_steel_plate: None,
            id: WallPlateId(0),
            shape: WallPlateShape::Enclosed,
            section: Some(SectionId(2)),
            dl_support: Some(sepika_core::model::WallDlSupport::LowerBeam),
            self_weight_shares: Vec::new(),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: Vec::new(),
            slit: Default::default(),
        },
    );
    model.wall_regions.push(WallRegion {
        id: WallRegionId(0),
        name: "壁構面".into(),
        boundary,
        wall_plate_ids: vec![WallPlateId(0)],
        posts: Vec::new(),
    });
    model.wall_weight_generation =
        Some(sepika_core::model::WallWeightGenerationMode::GravityCasesOnly);
    model
}

pub(super) fn rc_wall_model() -> Model {
    let mut model = portal_with_wall_plate();
    model.panel_zone = sepika_core::model::PanelZoneMode::None;
    model.materials.truncate(1);
    let concrete = &mut model.materials[0];
    concrete.name = "Fc24".into();
    concrete.category = MaterialCategory::Concrete;
    concrete.fc = Some(24.0);
    concrete.fy = None;
    concrete.young = 23000.0;
    concrete.poisson = 0.2;
    concrete.density = 2.4e-9;
    concrete.shear = None;
    let mut vertical = concrete.clone();
    vertical.id = MaterialId(1);
    vertical.name = "SD345".into();
    vertical.category = MaterialCategory::Rebar;
    vertical.fc = None;
    vertical.fy = Some(345.0);
    vertical.young = 205000.0;
    vertical.strength_factor = Some(1.0);
    let mut horizontal = vertical.clone();
    horizontal.id = MaterialId(2);
    horizontal.name = "SD295A".into();
    horizontal.fy = Some(295.0);
    model.materials.extend([vertical, horizontal]);
    let column = SectionShape::RcColumnRect {
        b: 600.0,
        d: 600.0,
        rebar: RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![8],
            y: vec![8],
            cover: 50.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        },
    };
    let beam = SectionShape::RcBeamRect {
        b: 600.0,
        d: 600.0,
        rebar: RcBeamRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            top: vec![8],
            bottom: vec![8],
            cover: 50.0,
            stirrup: BeamStirrup {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs: 2,
            },
        },
    };
    for (index, old) in model.sections.iter_mut().enumerate().take(3) {
        let shape = match index {
            0 => column.clone(),
            1 => beam.clone(),
            _ => SectionShape::RcWall {
                thickness: 200.0,
                ps: 0.0025,
                pwh_ratio: Some(0.006),
                pwv_ratio: Some(0.002),
            },
        };
        let mut section = shape.to_section(old.id, old.name.clone());
        section.frame_use = old.frame_use;
        section.material = Some(MaterialId(0));
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(2));
        *old = section;
    }
    for element in &mut model.elements {
        element.force_regime = sepika_core::model::ForceRegime::AxialBendingInteract;
    }
    model
}

#[cfg(feature = "gui")]
#[test]
fn 壁版の断面候補は鋼材主材料を除外する() {
    use crate::tables::wall_plates::wall_section_selectable;

    let steel = portal_with_wall_plate();
    assert!(!wall_section_selectable(&steel, &steel.sections[2]));
    let mut rc = rc_wall_model();
    assert!(wall_section_selectable(&rc, &rc.sections[2]));
    let embedded = steel.materials[0].clone();
    let id = MaterialId(rc.materials.len() as u32);
    rc.materials
        .push(sepika_core::model::Material { id, ..embedded });
    rc.sections[2].steel_material = Some(id);
    assert!(wall_section_selectable(&rc, &rc.sections[2]));
}

#[test]
fn 鋼板壁のモデルを読み込んでも現在のモデルを差し替えない() {
    let mut app = App::default();
    assert!(app.load_model(crate::sample::portal_frame()));
    let before = app.core.model.nodes.clone();
    assert!(!app.load_model(portal_with_wall_plate()));
    assert_eq!(app.core.model.nodes, before);
    assert!(app.core.model.wall_plates.is_empty());
    let error = app.core.scoped.last_error.as_deref().unwrap();
    assert!(
        error.contains("鋼板壁版 ID 0") && error.contains("重量は荷重として入力"),
        "{error}"
    );
}

#[test]
fn 時刻歴と増分解析では鋼板壁を理由に停止する() {
    for background in [false, true] {
        for time_history_nonlinear in [None, Some(false), Some(true)] {
            let mut app = App::default();
            app.core.model = portal_with_wall_plate();
            for node in &mut app.core.model.nodes {
                node.mass = Some([1.0; 6]);
            }
            if let Some(nonlinear) = time_history_nonlinear {
                app.core.analysis_cfg.th_nonlinear = nonlinear;
                app.core.analysis_cfg.th_duration = 0.05;
                let wave = App::sample_wave(&app.core.analysis_cfg).unwrap();
                if background {
                    app.start_time_history_job(wave);
                } else {
                    app.run_time_history(wave);
                }
            } else if background {
                app.start_pushover_job();
            } else {
                app.run_pushover();
            }
            assert!(app.core.scoped.job.is_none());
            let error = app.core.scoped.last_error.as_deref().unwrap();
            assert!(
                error.contains("鋼板壁版 ID 0") && error.contains("重量は荷重として入力"),
                "{error}"
            );
            if let Some(results) = &app.core.scoped.results {
                assert!(results.time_history.is_none());
                assert!(results.pushover_x.is_none());
            }
        }
    }
}
