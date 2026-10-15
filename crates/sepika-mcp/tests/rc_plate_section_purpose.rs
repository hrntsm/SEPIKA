use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::*};
use sepika_io::results::{nodal_disp_batch, ResultKind, ResultQuery, ResultStore};
use sepika_mcp::{apply_edit, parse_edit_command, JobKind, ServerState};
use serde_json::{json, Value};

fn dedicated_shapes() -> [SectionShape; 3] {
    [
        SectionShape::RcBeamRect {
            b: 300.,
            d: 500.,
            rebar: RcBeamRebar {
                main_dia: 22.,
                top: vec![3],
                bottom: vec![4],
                cover: 40.,
                stirrup: BeamStirrup {
                    dia: 10.,
                    pitch: 150.,
                    legs: 2,
                },
            },
        },
        SectionShape::RcColumnRect {
            b: 600.,
            d: 600.,
            rebar: RcRectColumnRebar {
                main_dia: 19.,
                x: vec![4],
                y: vec![4],
                cover: 40.,
                hoop: RectColumnHoop {
                    dia: 10.,
                    pitch: 150.,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        },
        SectionShape::RcColumnCircle {
            d: 600.,
            rebar: RcCircleColumnRebar {
                main_dia: 19.,
                count: 12,
                cover: 40.,
                hoop: CircleColumnHoop {
                    dia: 10.,
                    pitch: 150.,
                },
            },
        },
    ]
}

fn section(shape: &SectionShape, id: u32) -> Section {
    assert!(shape.validate_rebar().is_ok());
    let mut section = shape
        .try_to_section(SectionId(id), format!("RC{id}"))
        .unwrap();
    section.material = Some(MaterialId(0));
    if id != 1 {
        section.rebar_material = Some(MaterialId(1));
        section.shear_rebar_material = Some(MaterialId(1));
    }
    section
}

fn base(wall: bool, dedicated: &SectionShape) -> Model {
    let mut beam = section(&dedicated_shapes()[0], 0);
    beam.frame_use = Some(FrameSectionUse::Girder);
    let mut column = section(&dedicated_shapes()[1], 4);
    column.frame_use = Some(FrameSectionUse::Column);
    let plate_shape = if wall {
        SectionShape::RcWall {
            thickness: 180.,
            ps: 0.002,
            pwh_ratio: Some(0.002),
            pwv_ratio: Some(0.002),
        }
    } else {
        SectionShape::RcSlab { thickness: 180. }
    };
    let mut plate = section(&plate_shape, 1);
    if wall {
        plate.rebar_material = Some(MaterialId(1));
        plate.shear_rebar_material = Some(MaterialId(1));
    }
    let mut numeric = plate.clone();
    numeric.id = SectionId(3);
    numeric.name = "数値入力板".into();
    numeric.shape = None;
    let mut model = Model {
        materials: [MaterialCategory::Concrete, MaterialCategory::Rebar]
            .into_iter()
            .enumerate()
            .map(|(id, category)| {
                let concrete = category == MaterialCategory::Concrete;
                Material {
                    id: MaterialId(id as u32),
                    name: format!("材料{id}"),
                    category,
                    young: if concrete { 25000. } else { 205000. },
                    poisson: 0.2,
                    density: if concrete { 2.4e-9 } else { 7.85e-9 },
                    shear: None,
                    fc: concrete.then_some(24.),
                    fy: (!concrete).then_some(345.),
                    concrete_class: Default::default(),
                    strength_factor: None,
                }
            })
            .collect(),
        sections: vec![beam, plate, section(dedicated, 2), numeric, column],
        girder_torsion: GirderTorsionMode::Keep,
        ..Default::default()
    };
    for (id, xy) in [[0., 0.], [3000., 0.], [3000., 3000.], [0., 3000.]]
        .into_iter()
        .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(id as u32),
            coord: if wall {
                [xy[0], 0., xy[1]]
            } else {
                [xy[0], xy[1], 0.]
            },
            restraint: Dof6Mask::FIXED,
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    for id in 0..4 {
        model.elements.push(ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: [NodeId(id), NodeId((id + 1) % 4)].into_iter().collect(),
            section: Some(SectionId(if wall && id % 2 == 1 { 4 } else { 0 })),
            local_axis: LocalAxis {
                ref_vector: if wall { [0., 1., 0.] } else { [0., 0., 1.] },
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
    }
    if wall {
        model.wall_regions.push(WallRegion::new(
            WallRegionId(0),
            (0..4).map(NodeId).collect(),
        ));
        assert_eq!(model.rebuild_wall_assignment_regions().regions, 1);
    } else {
        model.floor_regions.push(FloorRegion::new(
            FloorRegionId(0),
            (0..4).map(NodeId).collect(),
        ));
        assert_eq!(model.rebuild_floor_assignment_regions().regions, 1);
    }
    model.validate().unwrap();
    model
}

struct Fixture {
    state: ServerState,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new(wall: bool, shape: &SectionShape) -> Self {
        let dir = std::env::temp_dir().join(format!("sepika677-mcp-{}", uuid::Uuid::new_v4()));
        let mut state = ServerState::with_fs_store(base(wall, shape), &dir).unwrap();
        success(&mut state, &assign(wall, Some(1)));
        state.model.validate().unwrap();
        Self { state, dir }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn assign(wall: bool, section: Option<u32>) -> Value {
    json!({"command":if wall {"AssignWallPlateToRegion"} else {"AssignSlabToFloorPlateRegion"},
        "region":0,"section":section,"method":"TriTrapezoid"})
}
fn unset(wall: bool) -> Value {
    json!({"command":if wall {"UnsetWallPlateRegion"} else {"UnsetFloorPlateRegion"}, "region":0})
}
fn set(wall: bool, section: Option<u32>) -> Value {
    json!({"command":if wall {"SetWallPlateSection"} else {"SetSlabSection"}, "id":0,"section":section})
}
fn attached(wall: bool, section: Option<u32>) -> Value {
    json!({"command":if wall {"AddAttachedWallPlate"} else {"AddAttachedSlab"},
        "anchor":{"Line":{"nodes":[0,1],"span":[0.,1.],"transfer":"Anchor"}},
        "extent":[900.,900.],"section":section})
}
fn shape_edit(shape: &SectionShape) -> Value {
    json!({"body":{"command":"EditSectionShape","section":1,"new_shape":shape,"frame_use":null}})
}
fn success(state: &mut ServerState, body: &Value) {
    parse_edit_command(body).unwrap();
    let result = apply_edit(state, body).unwrap();
    assert!(result.applied && result.undoable, "{}", result.summary);
}

fn rejection_preserves_state(
    fixture: &mut Fixture,
    wall: bool,
    body: &Value,
    section: u32,
    plate: u32,
    beam: bool,
) {
    let state = &mut fixture.state;
    parse_edit_command(body).unwrap();
    let mut seed_shape = state.model.sections[0].shape.clone().unwrap();
    for increment in [20., 20.] {
        if let SectionShape::RcBeamRect { d, .. } = &mut seed_shape {
            *d += increment;
        }
        success(
            state,
            &json!({"command":"EditSectionShape","section":0,"new_shape":seed_shape,"frame_use":"Girder"}),
        );
    }
    let redo_model = serde_json::to_value(&state.model).unwrap();
    state.undo.undo(&mut state.model);
    assert!(state.undo.can_undo() && state.undo.can_redo());
    state.model.validate().unwrap();
    let before = serde_json::to_value(&state.model).unwrap();
    let revision = state.undo.revision();
    let undo_label = state.undo.undo_label().map(str::to_owned);
    let redo_label = state.undo.redo_label().map(str::to_owned);
    let mut writer = state
        .results
        .writer(7, ResultKind::NodalDisp, None)
        .unwrap();
    writer
        .write_rows(&nodal_disp_batch(&[0], &[[0.125, 0., 0., 0., 0., 0.]]).unwrap())
        .unwrap();
    writer.finish().unwrap();
    state.results.sync().unwrap();
    let query = ResultQuery {
        case: 7,
        kind: ResultKind::NodalDisp,
        node_filter: None,
        member_filter: None,
        step_range: None,
    };
    let result_batch = state.results.query(&query).unwrap().batch;
    let manifest = serde_json::to_value(state.results.manifest()).unwrap();
    assert_eq!(state.results.manifest().entries.len(), 1);
    let result_path = fixture.dir.join(&state.results.manifest().entries[0].path);
    let result_bytes = std::fs::read(&result_path).unwrap();
    let job = state.jobs.lock().unwrap().register(JobKind::LinearStatic);
    let job_before = serde_json::to_value(state.jobs.lock().unwrap().get(&job)).unwrap();
    let reason = apply_edit(state, body).unwrap_err();
    let plate_type = if wall { "WallPlateId" } else { "SlabId" };
    assert!(
        reason.contains(&format!("{plate_type}({plate})")),
        "{reason}"
    );
    assert!(
        reason.contains(&format!("SectionId({section})")),
        "{reason}"
    );
    assert!(
        reason.contains(if beam {
            "RC 梁用断面"
        } else {
            "RC 柱用断面"
        }),
        "{reason}"
    );
    assert!(
        reason.contains(if beam {
            "大梁・小梁専用"
        } else {
            "柱・間柱専用"
        }),
        "{reason}"
    );
    assert_eq!(serde_json::to_value(&state.model).unwrap(), before);
    assert_eq!(state.undo.revision(), revision);
    assert_eq!(state.undo.undo_label(), undo_label.as_deref());
    assert_eq!(state.undo.redo_label(), redo_label.as_deref());
    assert_eq!(
        serde_json::to_value(state.results.manifest()).unwrap(),
        manifest
    );
    assert_eq!(std::fs::read(result_path).unwrap(), result_bytes);
    assert_eq!(state.results.query(&query).unwrap().batch, result_batch);
    assert_eq!(
        serde_json::to_value(state.jobs.lock().unwrap().get(&job)).unwrap(),
        job_before
    );
    state.undo.redo(&mut state.model);
    assert!(state.undo.last_error().is_none());
    assert_eq!(serde_json::to_value(&state.model).unwrap(), redo_model);
    state.undo.undo(&mut state.model);
    assert!(state.undo.last_error().is_none());
    assert_eq!(serde_json::to_value(&state.model).unwrap(), before);
    state.undo.undo(&mut state.model);
    assert!(state.undo.last_error().is_none());
    assert_ne!(serde_json::to_value(&state.model).unwrap(), before);
    state.undo.redo(&mut state.model);
    assert!(state.undo.last_error().is_none());
    assert_eq!(serde_json::to_value(&state.model).unwrap(), before);
}

#[test]
fn mcpの床壁断面設定と割当と取付追加はrc専用三型を理由付き拒否し状態を保持する() {
    for (index, shape) in dedicated_shapes().iter().enumerate() {
        for wall in [false, true] {
            for (body, plate) in [
                (set(wall, Some(2)), 0),
                (assign(wall, Some(2)), 0),
                (attached(wall, Some(2)), 1),
            ] {
                let mut fixture = Fixture::new(wall, shape);
                if body["command"] == "AssignSlabToFloorPlateRegion"
                    || body["command"] == "AssignWallPlateToRegion"
                {
                    success(&mut fixture.state, &unset(wall));
                }
                rejection_preserves_state(&mut fixture, wall, &body, 2, plate, index == 0);
            }
        }
    }
}

#[test]
fn mcpの床壁参照中断面編集はrc専用三型への変更を理由付き拒否する() {
    for (index, shape) in dedicated_shapes().iter().enumerate() {
        for wall in [false, true] {
            let mut fixture = Fixture::new(wall, shape);
            rejection_preserves_state(&mut fixture, wall, &shape_edit(shape), 1, 0, index == 0);
        }
    }
}

#[test]
fn mcpの正常rc板と数値断面と未割当は床壁の設定と割当と取付追加を許可する() {
    for wall in [false, true] {
        for section in [Some(1), Some(3), None] {
            let mut fixture = Fixture::new(wall, &dedicated_shapes()[0]);
            let state = &mut fixture.state;
            success(state, &set(wall, section));
            state.model.validate().unwrap();
            success(state, &unset(wall));
            success(state, &assign(wall, section));
            state.model.validate().unwrap();
            success(state, &attached(wall, section));
            state.model.validate().unwrap();
            if wall {
                assert_eq!(state.model.wall_plates.len(), 2);
                assert!(state
                    .model
                    .wall_plates
                    .iter()
                    .all(|p| p.section == section.map(SectionId)));
            } else {
                assert_eq!(state.model.slabs.len(), 2);
                assert!(state
                    .model
                    .slabs
                    .iter()
                    .all(|p| p.plate.section == section.map(SectionId)));
            }
            let shape = if wall {
                SectionShape::RcWall {
                    thickness: 200.,
                    ps: 0.002,
                    pwh_ratio: Some(0.002),
                    pwv_ratio: Some(0.002),
                }
            } else {
                SectionShape::RcSlab { thickness: 200. }
            };
            success(state, &shape_edit(&shape));
            state.model.validate().unwrap();
            assert_eq!(state.model.sections[1].shape.as_ref(), Some(&shape));
        }
    }
}
