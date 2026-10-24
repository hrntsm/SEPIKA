use sepika_core::{dof::Dof6Mask, ids::*, model::*, section_shape::*};
use sepika_io::results::ResultStore;
use sepika_mcp::{apply_edit, parse_edit_command, JobKind, ServerState};
use serde_json::{json, Value};

fn shapes() -> [SectionShape; 3] {
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

fn section(shape: &SectionShape, id: u32, usage: Option<FrameSectionUse>) -> Section {
    assert!(shape.validate_rebar().is_ok());
    let mut section = shape
        .try_to_section(SectionId(id), format!("RC{id}"))
        .unwrap();
    section.frame_use = usage;
    section.material = Some(MaterialId(0));
    section.rebar_material = Some(MaterialId(1));
    section.shear_rebar_material = Some(MaterialId(1));
    section
}

fn materials() -> Vec<Material> {
    [MaterialCategory::Concrete, MaterialCategory::Rebar]
        .into_iter()
        .enumerate()
        .map(|(id, category)| {
            let concrete = category == MaterialCategory::Concrete;
            Material {
                id: MaterialId(id as u32),
                name: if concrete { "Fc24" } else { "SD345" }.into(),
                category,
                young: if concrete { 25000. } else { 205000. },
                poisson: if concrete { 0.2 } else { 0.3 },
                density: if concrete { 2.4e-9 } else { 7.85e-9 },
                shear: None,
                fc: concrete.then_some(24.),
                fy: (!concrete).then_some(345.),
                concrete_class: Default::default(),
                strength_factor: None,
            }
        })
        .collect()
}

fn frame(shape: &SectionShape, usage: FrameSectionUse) -> Model {
    let mut model = Model {
        materials: materials(),
        ..Default::default()
    };
    model.sections.push(section(shape, 0, Some(usage)));
    for (id, coord) in [
        [0.; 3],
        if usage == FrameSectionUse::Column {
            [0., 0., 3000.]
        } else {
            [3000., 0., 0.]
        },
    ]
    .into_iter()
    .enumerate()
    {
        model.nodes.push(Node {
            id: NodeId(id as u32),
            coord,
            restraint: if id == 0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            mass: None,
            story: None,
            support_spring: None,
        });
    }
    model.elements.push(ElementData {
        id: ElemId(0),
        kind: ElementKind::Beam,
        nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
        section: Some(SectionId(0)),
        local_axis: LocalAxis {
            ref_vector: [0., 1., 0.],
        },
        end_cond: [EndCondition::Fixed; 2],
        force_regime: ForceRegime::Auto,
        rigid_zone: Default::default(),
        vertical_eccentricity: Some(0.0),
        plastic_zone: None,
        spring: None,
    });
    model
}

fn secondary_base(kind: SecondaryMemberKind) -> Model {
    let [beam, column, _] = shapes();
    let mut model = frame(&column, FrameSectionUse::Column);
    model
        .sections
        .push(section(&beam, 1, Some(FrameSectionUse::Girder)));
    let wall = kind == SecondaryMemberKind::Post;
    let coordinates = if wall {
        [
            [0., 0., 0.],
            [3000., 0., 0.],
            [3000., 0., 3000.],
            [0., 0., 3000.],
        ]
    } else {
        [
            [0., 0., 3000.],
            [3000., 0., 3000.],
            [3000., 3000., 3000.],
            [0., 3000., 3000.],
        ]
    };
    let node = model.nodes[0].clone();
    model.nodes = coordinates
        .into_iter()
        .enumerate()
        .map(|(id, coord)| Node {
            id: NodeId(id as u32),
            coord,
            restraint: if id == 0 {
                Dof6Mask::FIXED
            } else {
                Dof6Mask::FREE
            },
            ..node.clone()
        })
        .collect();
    let element = model.elements[0].clone();
    model.elements = (0..4)
        .map(|id| ElementData {
            id: ElemId(id),
            nodes: smallvec::smallvec![NodeId(id), NodeId((id + 1) % 4)],
            section: Some(SectionId(if wall && id % 2 == 1 { 0 } else { 1 })),
            local_axis: LocalAxis {
                ref_vector: if wall { [0., 1., 0.] } else { [0., 0., 1.] },
            },
            ..element.clone()
        })
        .collect();
    model.girder_torsion = GirderTorsionMode::Keep;
    let boundary = (0..4).map(NodeId).collect();
    if wall {
        model
            .wall_regions
            .push(WallRegion::new(WallRegionId(0), boundary));
    } else {
        model
            .floor_regions
            .push(FloorRegion::new(FloorRegionId(0), boundary));
    }
    model
}

fn member(kind: SecondaryMemberKind, section: u32) -> SecondaryMember {
    SecondaryMember {
        id: SecondaryMemberId(0),
        kind,
        ends: SecondaryMemberEnds::Supported([
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(0)),
                position: 0.5,
            },
            SecondaryMemberAnchor {
                support: SupportMemberId::Primary(ElemId(2)),
                position: 0.5,
            },
        ]),
        section: Some(SectionId(section)),
        name: "受入二次材".into(),
        vertical_eccentricity: Some(0.0),
        gravity_end_shares: Some([1., 0.]),
    }
}

struct Fixture {
    state: ServerState,
    dir: std::path::PathBuf,
}
impl Fixture {
    fn new(model: Model) -> Self {
        let dir = std::env::temp_dir().join(format!("sepika674-mcp-{}", uuid::Uuid::new_v4()));
        Self {
            state: ServerState::with_fs_store(model, &dir).unwrap(),
            dir,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}

fn edit(section: u32, shape: &SectionShape, usage: Option<FrameSectionUse>) -> Value {
    json!({"command":"EditSectionShape", "section":section, "new_shape":shape, "frame_use":usage})
}

fn apply_success(state: &mut ServerState, body: &Value) {
    parse_edit_command(body).unwrap();
    let result = apply_edit(state, body).unwrap();
    assert!(result.applied && result.undoable, "{}", result.summary);
}

fn assert_rejection_preserves_state(
    state: &mut ServerState,
    body: &Value,
    section_id: u32,
    beam: bool,
) {
    parse_edit_command(body).unwrap();
    let seed = edit(
        0,
        &state.model.sections[0].shape.clone().unwrap(),
        state.model.sections[0].frame_use,
    );
    apply_success(state, &seed);
    let redo_model = serde_json::to_value(&state.model).unwrap();
    state.undo.undo(&mut state.model);
    assert!(state.undo.can_redo());
    let before = serde_json::to_value(&state.model).unwrap();
    let revision = state.undo.revision();
    let undo_label = state.undo.undo_label().map(str::to_owned);
    let redo_label = state.undo.redo_label().map(str::to_owned);
    let manifest = serde_json::to_value(state.results.manifest()).unwrap();
    let job = state.jobs.lock().unwrap().register(JobKind::LinearStatic);
    let job_before = serde_json::to_value(state.jobs.lock().unwrap().get(&job)).unwrap();
    let reason = apply_edit(state, body).unwrap_err();
    assert!(
        reason.contains(&format!("SectionId({section_id})")),
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
    assert!(reason.contains("専用"), "{reason}");
    assert_eq!(serde_json::to_value(&state.model).unwrap(), before);
    assert_eq!(state.undo.revision(), revision);
    assert_eq!(state.undo.undo_label(), undo_label.as_deref());
    assert_eq!(state.undo.redo_label(), redo_label.as_deref());
    assert_eq!(
        serde_json::to_value(state.results.manifest()).unwrap(),
        manifest
    );
    assert_eq!(
        serde_json::to_value(state.jobs.lock().unwrap().get(&job)).unwrap(),
        job_before
    );
    state.undo.redo(&mut state.model);
    assert!(state.undo.last_error().is_none());
    assert_eq!(serde_json::to_value(&state.model).unwrap(), redo_model);
}

#[test]
fn mcpの主架構断面編集は逆用途三型を理由付き拒否しredoとモデルを保持する() {
    let shapes = shapes();
    for (index, shape) in shapes.iter().enumerate() {
        let beam = index == 0;
        let usage = if beam {
            FrameSectionUse::Column
        } else {
            FrameSectionUse::Girder
        };
        let original = &shapes[if beam { 1 } else { 0 }];
        let mut fixture = Fixture::new(frame(original, usage));
        let state = &mut fixture.state;
        state.model.validate().unwrap();
        let body = json!({"body":edit(0, shape, Some(usage))});
        assert_rejection_preserves_state(state, &body, 0, beam);
    }
}

#[test]
fn mcpの配置と領域参照更新は逆用途三型を理由付き拒否する() {
    for (index, shape) in shapes().iter().enumerate() {
        let beam = index == 0;
        let kind = if beam {
            SecondaryMemberKind::Post
        } else {
            SecondaryMemberKind::Beam
        };
        for usage in [
            None,
            Some(if beam {
                FrameSectionUse::Girder
            } else {
                FrameSectionUse::Column
            }),
        ] {
            let mut model = secondary_base(kind);
            model.sections.push(section(shape, 2, usage));
            model.validate().unwrap();
            let mut fixture = Fixture::new(model);
            let sm = member(kind, 2);
            let place = json!({"command":"PlaceSecondaryMember", "parent":if beam {"wall"} else {"floor"},
                "region":0,"kind":kind,"ends":sm.ends,"section":2});
            assert_rejection_preserves_state(&mut fixture.state, &place, 2, beam);
            let bulk = if beam {
                json!({"command":"SetWallRegionPosts","wall_region":0,"posts":[sm]})
            } else {
                json!({"command":"SetFloorRegionSecondaryBeams","id":0,"secondary_beams":[sm]})
            };
            assert_rejection_preserves_state(&mut fixture.state, &bulk, 2, beam);
        }
    }
}

#[test]
fn mcpの正常三型は同型共有と二次用途未設定で配置と参照中編集を許可する() {
    for (index, shape) in shapes().iter().enumerate() {
        let beam = index == 0;
        let kind = if beam {
            SecondaryMemberKind::Beam
        } else {
            SecondaryMemberKind::Post
        };
        let usage = if beam {
            FrameSectionUse::Girder
        } else {
            FrameSectionUse::Column
        };
        for shared in [true, false] {
            let mut model = secondary_base(kind);
            let sid = if shared {
                if beam {
                    1
                } else {
                    0
                }
            } else {
                2
            };
            if shared {
                model.sections[sid as usize] = section(shape, sid, Some(usage));
            } else {
                model.sections.push(section(shape, sid, None));
            }
            model.validate().unwrap();
            let mut fixture = Fixture::new(model);
            let state = &mut fixture.state;
            let sm = member(kind, sid);
            let place = json!({"command":"PlaceSecondaryMember","parent":if beam {"floor"} else {"wall"},
                "region":0,"kind":kind,"ends":sm.ends,"section":sid});
            apply_success(state, &place);
            if !beam {
                apply_success(
                    state,
                    &json!({"command":"SetPostGravityEndShares","member":0,"shares":[1.,0.]}),
                );
            }
            let mut changed = shape.clone();
            match &mut changed {
                SectionShape::RcBeamRect { d, .. }
                | SectionShape::RcColumnRect { d, .. }
                | SectionShape::RcColumnCircle { d, .. } => *d += 20.,
                _ => unreachable!(),
            }
            apply_success(state, &edit(sid, &changed, shared.then_some(usage)));
            let referenced = if beam {
                state.model.beams().next().unwrap()
            } else {
                state.model.posts().next().unwrap()
            };
            assert_eq!(referenced.section, Some(SectionId(sid)));
            assert_eq!(
                state.model.sections[sid as usize].frame_use,
                shared.then_some(usage)
            );
            assert_eq!(
                state.model.sections[sid as usize].shape.as_ref(),
                Some(&changed)
            );
            state.model.validate().unwrap();
            assert!(state.results.manifest().entries.is_empty());
        }
    }
}

#[test]
fn mcpは二次専用none断面の参照中逆型変更も理由付き拒否する() {
    let shapes = shapes();
    for (index, target) in shapes.iter().enumerate() {
        let beam = index == 0;
        let kind = if beam {
            SecondaryMemberKind::Post
        } else {
            SecondaryMemberKind::Beam
        };
        let original = &shapes[if beam { 1 } else { 0 }];
        let mut model = secondary_base(kind);
        model.sections.push(section(original, 2, None));
        let sm = member(kind, 2);
        if beam {
            model.wall_regions[0].posts.push(sm);
        } else {
            model.floor_regions[0].secondary_beams.push(sm);
        }
        model.next_secondary_member_id = 1;
        model.validate().unwrap();
        let mut fixture = Fixture::new(model);
        assert_rejection_preserves_state(&mut fixture.state, &edit(2, target, None), 2, beam);
    }
}
