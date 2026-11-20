use sepika_core::dof::Dof6Mask;
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId};
use sepika_core::model::{
    ElementData, FrameSectionUse, GirderTorsionMode, LoadCase, MaterialCategory, MemberLoad,
    MemberLoadKind, Model, NodalLoad, PanelZoneMode, ZoneSource,
};
use sepika_core::section_shape::{RcRectColumnRebar, RebarSize, RectColumnHoop, SectionShape};
use sepika_job::compute::compute_linear_static;
use sepika_job::prepare_model_for_analysis;
use sepika_job::settings::AnalysisSettings;
use sepika_solver::statics::linear::{superpose_static, GirderColumnFaceEnds, StaticOnce};

#[path = "fixtures/steel_panel_model.rs"]
mod fixture;

const GIRDER: ElemId = ElemId(2);
const TIP: NodeId = NodeId(3);
const LENGTH_MM: f64 = 6000.0;
const TIP_LOAD_N: f64 = 10_000.0;

fn box_shape(depth_mm: f64, width_mm: f64) -> SectionShape {
    SectionShape::SteelBox {
        height: depth_mm,
        width: width_mm,
        thick: 16.0,
        horizontal_thick: 16.0,
        corner_r: Some(0.0),
    }
}

fn column_section(model: &mut Model, id: usize, shape: SectionShape, material: MaterialId) {
    let mut section = shape.to_section(SectionId(id as u32), format!("柱{id}"));
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(material);
    model.sections[id] = section;
}

fn closed_and_known_flat_column_shapes() -> [SectionShape; 3] {
    [
        box_shape(600.0, 400.0),
        SectionShape::SteelH {
            height: 600.0,
            width: 400.0,
            web_thick: 16.0,
            flange_thick: 25.0,
            root_r: Some(12.0),
        },
        SectionShape::SteelBox {
            height: 600.0,
            width: 400.0,
            thick: 16.0,
            horizontal_thick: 16.0,
            corner_r: None,
        },
    ]
}

fn set_column_shape(model: &mut Model, id: usize, shape: &SectionShape) {
    if matches!(shape, SectionShape::SteelBox { corner_r: None, .. }) {
        model.sections[id].property_basis = Default::default();
        model.sections[id].shape = Some(shape.clone());
    } else {
        column_section(model, id, shape.clone(), MaterialId(0));
    }
}

fn model() -> Model {
    let mut model = fixture::steel_panel_model();
    model.stories.clear();
    model.panel_zone = PanelZoneMode::None;
    model.girder_torsion = GirderTorsionMode::Keep;
    model.nodes[1].restraint = Dof6Mask::FIXED;
    model.elements[1].nodes = [TIP, NodeId(2)].into_iter().collect();
    model.nodes[2].coord = [LENGTH_MM, 0.0, 6000.0];
    for column in &mut model.elements[..2] {
        column.local_axis.ref_vector = [1.0, 0.0, 0.0];
    }
    column_section(&mut model, 0, box_shape(600.0, 400.0), MaterialId(0));
    column_section(&mut model, 1, box_shape(500.0, 400.0), MaterialId(0));
    model.load_cases.push(LoadCase {
        id: LoadCaseId(0),
        name: "梁先端荷重".into(),
        origin: Default::default(),
        kind: Default::default(),
        nodal: vec![NodalLoad::manual(
            TIP,
            [0.0, 0.0, -TIP_LOAD_N, 0.0, 0.0, 0.0],
        )],
        member: vec![],
    });
    model
}

fn add_branch(model: &mut Model, template: usize, start: NodeId, tip_mm: [f64; 3]) {
    let mut node = model.nodes[TIP.index()].clone();
    node.coord = tip_mm;
    node.id = NodeId(model.nodes.len() as u32);
    let mut elem: ElementData = model.elements[template].clone();
    elem.id = ElemId(model.elements.len() as u32);
    elem.nodes = [start, node.id].into_iter().collect();
    model.nodes.push(node);
    model.elements.push(elem);
}

fn solve(mut model: Model) -> StaticOnce {
    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
    compute_linear_static(model, LoadCaseId(0)).unwrap()
}

fn faces(result: &StaticOnce) -> &GirderColumnFaceEnds {
    &result
        .girder_column_face_forces
        .iter()
        .find(|(id, _)| *id == GIRDER)
        .expect("大梁の実柱面結果")
        .1
}

fn close(actual: f64, expected: f64, tol: f64) {
    assert!((actual - expected).abs() <= tol, "{actual} ≠ {expected}");
}

fn tip_cut(values: [f64; 6], remaining_mm: f64) {
    for (component, (actual, expected)) in values
        .into_iter()
        .zip([0.0, TIP_LOAD_N, 0.0, 0.0, 0.0, -TIP_LOAD_N * remaining_mm])
        .enumerate()
    {
        close(actual, expected, if component < 3 { 1e-6 } else { 1e-2 });
    }
}

#[test]
fn 直交梁による危険断面の移動と左右の実柱面力を公開解析で区別する() {
    for (cross_girder, legacy_mm) in [(false, 300.0), (true, 400.0)] {
        let mut model = model();
        if cross_girder {
            add_branch(&mut model, 2, NodeId(0), [0.0, 3500.0, 3000.0]);
            let mut section = box_shape(800.0, 400.0).to_section(SectionId(3), "直交梁".into());
            section.frame_use = Some(FrameSectionUse::Girder);
            section.material = Some(MaterialId(0));
            model.sections.push(section);
            model.elements[3].section = Some(SectionId(3));
        }
        let result = solve(model);
        let normal = &result
            .member_forces
            .iter()
            .find(|(id, _)| *id == GIRDER)
            .unwrap()
            .1;
        let legacy = normal
            .at
            .iter()
            .filter(|(xi, _)| 0.0 < *xi && *xi < 0.5)
            .collect::<Vec<_>>();
        assert_eq!(legacy.len(), 1);
        close(legacy[0].0 * LENGTH_MM, legacy_mm, 1e-9);
        tip_cut(legacy[0].1, LENGTH_MM - legacy_mm);
        for ((face, expected_mm), column) in faces(&result).iter().zip([300.0, 5750.0]).zip([0, 1])
        {
            let face = face.as_ref().unwrap();
            assert_eq!(face.face.position_mm, expected_mm);
            assert_eq!(face.face.length_mm, LENGTH_MM);
            assert_eq!(face.face.columns, vec![ElemId(column)]);
            tip_cut(*face.forces.as_ref().unwrap(), LENGTH_MM - expected_mm);
        }
    }
}

#[test]
fn 非正方柱の方位とコンクリート外周から実柱面力を取得する() {
    let rebar = RcRectColumnRebar {
        main_dia: RebarSize::DiameterMm(19.0),
        x: vec![4],
        y: vec![4],
        cover: 40.0,
        hoop: RectColumnHoop {
            dia: RebarSize::DiameterMm(10.0),
            pitch: 150.0,
            legs_x: 2,
            legs_y: 2,
        },
    };
    for (shape, is_concrete) in [
        (box_shape(600.0, 400.0), false),
        (
            SectionShape::RcColumnRect {
                b: 400.0,
                d: 600.0,
                rebar: rebar.clone(),
            },
            true,
        ),
        (
            SectionShape::SrcColumnRect {
                b: 400.0,
                d: 600.0,
                rebar,
                steel_height: 400.0,
                steel_width: 200.0,
                steel_web_thick: 10.0,
                steel_flange_thick: 16.0,
            },
            true,
        ),
    ] {
        for (reference, expected_mm) in [
            ([1.0, 0.0, 0.0], 300.0),
            ([0.0, 1.0, 0.0], 200.0),
            ([1.0, 1.0, 0.0], 200.0 * 2.0_f64.sqrt()),
        ] {
            let mut model = model();
            let material = if is_concrete {
                model.stories = fixture::steel_panel_model().stories;
                let mut concrete = model.materials[0].clone();
                concrete.id = MaterialId(1);
                concrete.name = "Fc30".into();
                concrete.category = MaterialCategory::Concrete;
                concrete.young = 25_000.0;
                concrete.poisson = 0.2;
                concrete.density = 2.45e-9;
                concrete.fc = Some(30.0);
                concrete.fy = None;
                let mut steel_rebar = model.materials[0].clone();
                steel_rebar.id = MaterialId(2);
                steel_rebar.name = "SD345".into();
                steel_rebar.category = MaterialCategory::Rebar;
                steel_rebar.fy = Some(345.0);
                model.materials.push(concrete);
                model.materials.push(steel_rebar);
                MaterialId(1)
            } else {
                MaterialId(0)
            };
            column_section(&mut model, 0, shape.clone(), material);
            if is_concrete {
                model.sections[0].rebar_material = Some(MaterialId(2));
                model.sections[0].shear_rebar_material = Some(MaterialId(2));
            }
            if matches!(shape, SectionShape::SrcColumnRect { .. }) {
                model.sections[0].steel_material = Some(MaterialId(0));
            }
            model.elements[0].local_axis.ref_vector = reference;
            let result = solve(model);
            let face = faces(&result)[0].as_ref().unwrap();
            close(face.face.position_mm, expected_mm, 1e-9);
            tip_cut(*face.forces.as_ref().unwrap(), LENGTH_MM - expected_mm);
        }
    }
}

#[test]
fn 梁偏心時は実材端からの位置と荷重のつり合いを返す() {
    let mut model = model();
    model.elements[GIRDER.index()].end_offsets_mm = [[50.0, 0.0, 0.0]; 2];
    let result = solve(model.clone());
    for ((face, position_mm), global_x_mm) in faces(&result)
        .iter()
        .zip([250.0, 5700.0])
        .zip([300.0, 5750.0])
    {
        let face = face.as_ref().unwrap();
        close(face.face.position_mm, position_mm, 1e-9);
        tip_cut(*face.forces.as_ref().unwrap(), LENGTH_MM - global_x_mm);
    }

    for shape in closed_and_known_flat_column_shapes() {
        let mut model = model.clone();
        set_column_shape(&mut model, 0, &shape);
        model.elements[GIRDER.index()].end_offsets_mm = [[300.0, 0.0, 0.0]; 2];
        let result = solve(model);
        let reason = faces(&result)[0].as_ref().unwrap_err();
        assert!(reason.contains("境界だけ"), "{shape:?}: {reason}");
    }
}

#[test]
fn 梁の鉛直偏心で実材軸が届く下柱の側面を用いる() {
    let mut model = model();
    model.elements[GIRDER.index()].end_offsets_mm = [[0.0, 0.0, -100.0]; 2];
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    let result = solve(model);
    let [i, j] = faces(&result);
    let face = i.as_ref().unwrap();
    assert_eq!(face.face.columns, [ElemId(0)]);
    close(face.face.position_mm, 300.0, 1e-9);
    tip_cut(*face.forces.as_ref().unwrap(), 5700.0);
    let reason = j.as_ref().unwrap_err();
    assert!(
        reason.contains("大梁 ID 2 j端") && reason.contains("側面を横切りません"),
        "{reason}"
    );
}

#[test]
fn 水平偏心で実材軸と交わらない柱を除く() {
    for shape in closed_and_known_flat_column_shapes() {
        let mut model = model();
        add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
        let mut upper_section = model.sections[0].clone();
        upper_section.id = SectionId(3);
        upper_section.name = "上柱".into();
        model.sections.push(upper_section);
        set_column_shape(&mut model, 3, &shape);
        model.elements[3].section = Some(SectionId(3));
        if matches!(shape, SectionShape::SteelBox { corner_r: None, .. }) {
            // 偏心上柱の角Rは従来の剛域準備で必要なので、角Rが既知の下柱と梁を偏心させる。
            model.elements[0].end_offsets_mm = [[0.0, -250.0, 0.0]; 2];
            model.elements[GIRDER.index()].end_offsets_mm[0][1] = -250.0;
            model.nodes[TIP.index()].coord[1] = -250.0;
            model.nodes[2].coord[1] = -250.0;
        } else {
            model.elements[3].end_offsets_mm = [[0.0, 250.0, 0.0]; 2];
        }
        let result = solve(model);
        let face = faces(&result)[0]
            .as_ref()
            .unwrap_or_else(|reason| panic!("{shape:?}: {reason}"));
        assert_eq!(face.face.columns, [ElemId(0)]);
        close(face.face.position_mm, 300.0, 1e-9);
        tip_cut(*face.forces.as_ref().unwrap(), 5700.0);
    }
}

#[test]
fn 傾斜梁が柱端面から入って側面で出るときは側面を用いる() {
    for shape in closed_and_known_flat_column_shapes() {
        assert_cap_then_side_exit(&shape);
    }
}

fn assert_cap_then_side_exit(shape: &SectionShape) {
    let mut model = model();
    set_column_shape(&mut model, 0, shape);
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    model.elements[GIRDER.index()].end_offsets_mm = [[0.0, 0.0, -10.0], [0.0, 0.0, 590.0]];
    let result = solve(model.clone());
    let face = faces(&result)[0]
        .as_ref()
        .unwrap_or_else(|reason| panic!("{shape:?}: {reason}"));
    assert_eq!(face.face.columns, [ElemId(3)]);
    let slope_factor = 1.01_f64.sqrt();
    close(face.face.position_mm, 300.0 * slope_factor, 1e-9);
    for (component, (actual, expected)) in face
        .forces
        .as_ref()
        .unwrap()
        .iter()
        .zip([
            -1000.0 / slope_factor,
            10_000.0 / slope_factor,
            0.0,
            0.0,
            0.0,
            -57_000_000.0,
        ])
        .enumerate()
    {
        close(*actual, expected, if component < 3 { 1e-6 } else { 1e-2 });
    }

    model.elements[GIRDER.index()].end_offsets_mm = [[0.0, 0.0, -30.0], [0.0, 0.0, 570.0]];
    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("境界だけ") || reason.contains("側面と端面"),
        "{shape:?}: {reason}"
    );
}

#[test]
fn 上柱の側面から入って端面から出る場合は未算定を返す() {
    let mut model = model();
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    model.elements[GIRDER.index()].end_offsets_mm = [[-400.0, 0.0, 20.0], [-400.0, 0.0, -580.0]];

    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("大梁 ID 2 i端")
            && reason.contains("柱 ID 3")
            && reason.contains("実材端と離れた位置で")
            && reason.contains("接合詳細"),
        "{reason}"
    );
}

#[test]
fn 梁実材端が側面と端面の交線から入る場合は未算定を返す() {
    let mut model = model();
    model.nodes[1].coord[2] = 6000.0;
    model.elements[GIRDER.index()].end_offsets_mm = [[-300.0, 0.0, 0.0], [-300.0, 0.0, 600.0]];
    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("大梁 ID 2 i端")
            && reason.contains("柱 ID 0")
            && reason.contains("側面と端面"),
        "{reason}"
    );
}

#[test]
fn 柱断面の外にある梁端から端面を経て外フランジ広面へ達する() {
    let mut model = model();
    model.nodes[1].coord[2] = 6000.0;
    let [lower_box, upper_h, _] = closed_and_known_flat_column_shapes();
    set_column_shape(&mut model, 0, &upper_h);
    model.elements[GIRDER.index()].end_offsets_mm = [[-310.0, 0.0, -10.0], [-310.0, 0.0, 590.0]];
    let result = solve(model.clone());
    let face = faces(&result)[0].as_ref().unwrap();
    assert_eq!(face.face.columns, [ElemId(0)]);
    let slope_factor = 1.01_f64.sqrt();
    close(face.face.position_mm, 610.0 * slope_factor, 1e-9);
    let force = face.forces.as_ref().unwrap();
    close(force[0], -1000.0 / slope_factor, 1e-6);
    close(force[1], 10_000.0 / slope_factor, 1e-6);
    close(force[5], -57_000_000.0, 1e-2);

    let mut with_lower_box = model.clone();
    add_branch(&mut with_lower_box, 0, NodeId(0), [0.0, 0.0, 0.0]);
    let mut lower = with_lower_box.sections[0].clone();
    lower.id = SectionId(3);
    with_lower_box.sections.push(lower);
    set_column_shape(&mut with_lower_box, 3, &lower_box);
    with_lower_box.elements[3].section = Some(SectionId(3));
    let result = solve(with_lower_box);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("柱 ID 3") && reason.contains("実材端と離れた位置で"),
        "{reason}"
    );

    model.nodes[TIP.index()].coord[1] = 100.0;
    model.nodes[2].coord[1] = 100.0;
    model.elements[GIRDER.index()].end_offsets_mm[0][1] = 100.0;
    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("柱 ID 0") && reason.contains("実材端と離れた位置で"),
        "{reason}"
    );
}

#[test]
fn 既知の鋼材に届かない上柱だけを除く() {
    let mut model = model();
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    let mut section =
        closed_and_known_flat_column_shapes()[1].to_section(SectionId(3), "H形上柱".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    model.sections.push(section);
    model.elements[3].section = Some(SectionId(3));
    model.nodes[TIP.index()].coord[1] = 100.0;
    model.nodes[2].coord[1] = 100.0;
    model.elements[GIRDER.index()].end_offsets_mm = [[0.0, 100.0, 10.0], [0.0, 0.0, -590.0]];

    let result = solve(model.clone());
    let face = faces(&result)[0].as_ref().unwrap();
    assert_eq!(face.face.columns, [ElemId(0)]);
    let slope_factor = 1.01_f64.sqrt();
    close(face.face.position_mm, 300.0 * slope_factor, 1e-9);
    let force = face.forces.as_ref().unwrap();
    close(force[0], 1000.0 / slope_factor, 1e-6);
    close(force[1], 10_000.0 / slope_factor, 1e-6);
    close(force[5], -57_000_000.0, 1e-2);

    let mut boundary = model.clone();
    boundary.nodes[TIP.index()].coord[1] = 8.0;
    boundary.nodes[2].coord[1] = 8.0;
    boundary.elements[GIRDER.index()].end_offsets_mm[0][1] = 8.0;
    let result = solve(boundary);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("柱 ID 3") && reason.contains("境界付近"),
        "{reason}"
    );

    let SectionShape::SteelH { root_r, .. } = model.sections[3].shape.as_mut().unwrap() else {
        unreachable!()
    };
    *root_r = None;
    model.sections[3].property_basis = Default::default();
    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("柱 ID 3") && reason.contains("外周を確定できません"),
        "{reason}"
    );
}

#[test]
fn 有限な柱で鋼材区間が連続しない端には接合詳細を求める() {
    let mut model = model();
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    let mut upper = model.sections[0].clone();
    upper.id = SectionId(3);
    model.sections.push(upper);
    set_column_shape(
        &mut model,
        3,
        &SectionShape::SteelH {
            height: 600.0,
            width: 400.0,
            web_thick: 16.0,
            flange_thick: 20.0,
            root_r: Some(0.0),
        },
    );
    model.elements[3].section = Some(SectionId(3));
    model.nodes[TIP.index()].coord[1] = 0.1 * LENGTH_MM;
    model.nodes[2].coord[1] = 0.1 * LENGTH_MM;
    model.elements[GIRDER.index()].end_offsets_mm =
        [[0.0, 0.0, 29.0], [0.0, 0.0, 29.0 - 0.1 * LENGTH_MM]];

    for root_radius_unknown in [false, true] {
        if root_radius_unknown {
            let SectionShape::SteelH { root_r, .. } = model.sections[3].shape.as_mut().unwrap()
            else {
                unreachable!()
            };
            *root_r = None;
            model.sections[3].property_basis = Default::default();
        }
        let result = solve(model.clone());
        let reason = faces(&result)[0].as_ref().unwrap_err();
        assert!(
            reason.contains("柱 ID 3")
                && reason.contains("鋼材区間が連続せず")
                && reason.contains("接合詳細"),
            "{reason}"
        );
    }
}

#[test]
fn 角r未知で梁実材端が保証外周の外にある場合は端面通過だけと判断しない() {
    let mut model = model();
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    let mut upper = model.sections[0].clone();
    upper.id = SectionId(3);
    model.sections.push(upper);
    let [_, _, unknown_box] = closed_and_known_flat_column_shapes();
    set_column_shape(&mut model, 3, &unknown_box);
    model.elements[3].section = Some(SectionId(3));
    // t=150 mmでせい方向300 mmの平面に達するが、先にt=140 mmで柱端面を通る。
    let width_slope = -199.0 / 150.0;
    let tip_xy = [150.0 + LENGTH_MM, 199.0 + width_slope * LENGTH_MM];
    model.nodes[TIP.index()].coord[..2].copy_from_slice(&tip_xy);
    model.nodes[2].coord[..2].copy_from_slice(&tip_xy);
    model.elements[GIRDER.index()].end_offsets_mm =
        [[150.0, 199.0, 14.0], [0.0, 0.0, 14.0 - 0.1 * LENGTH_MM]];

    let result = solve(model);
    let reason = faces(&result)[0].as_ref().unwrap_err();
    assert!(
        reason.contains("柱 ID 3")
            && reason.contains("実材端と離れた位置で")
            && reason.contains("接合詳細"),
        "{reason}"
    );
}

#[test]
fn 柱端面だけから出る場合は側面未確定の柱を候補から外す() {
    let [_, known_h, unknown_box] = closed_and_known_flat_column_shapes();
    for (shape, start, tip_xy, horizontal_factor, face_mm, unknown_fillet) in [
        (
            known_h.clone(),
            [0.0, 0.0, 0.4],
            [0.0, LENGTH_MM],
            1.0,
            200.0,
            false,
        ),
        (known_h, [0.0, 0.0, 0.4], [0.0, LENGTH_MM], 1.0, 200.0, true),
        (
            unknown_box,
            [0.0, 0.0, 10.0],
            [LENGTH_MM, LENGTH_MM],
            2.0_f64.sqrt(),
            200.0,
            false,
        ),
        (
            SectionShape::SteelH {
                height: 600.0,
                width: 400.0,
                web_thick: 16.0,
                flange_thick: 20.0,
                root_r: Some(0.0),
            },
            [290.0, 10.0, 1.5],
            [290.0 - LENGTH_MM, 10.0 - LENGTH_MM],
            2.0_f64.sqrt(),
            210.0,
            false,
        ),
        (
            SectionShape::SteelH {
                height: 600.0,
                width: 400.0,
                web_thick: 16.0,
                flange_thick: 20.0,
                root_r: Some(12.0),
            },
            [280.0, 12.0, 1.0],
            [280.0, 12.0 + 0.1 * LENGTH_MM],
            0.1,
            1880.0,
            false,
        ),
    ] {
        let mut model = model();
        add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
        let mut upper = model.sections[0].clone();
        upper.id = SectionId(3);
        upper.name = "上柱".into();
        model.sections.push(upper);
        set_column_shape(&mut model, 3, &shape);
        if unknown_fillet {
            let SectionShape::SteelH { root_r, .. } = model.sections[3].shape.as_mut().unwrap()
            else {
                unreachable!()
            };
            *root_r = None;
            model.sections[3].property_basis = Default::default();
        }
        model.elements[3].section = Some(SectionId(3));
        model.nodes[TIP.index()].coord[..2].copy_from_slice(&tip_xy);
        model.nodes[2].coord[..2].copy_from_slice(&tip_xy);
        model.elements[GIRDER.index()].end_offsets_mm = [start, [0.0, 0.0, start[2] - 600.0]];

        let result = solve(model);
        let face = faces(&result)[0]
            .as_ref()
            .unwrap_or_else(|reason| panic!("{shape:?}, {start:?}, {unknown_fillet}: {reason}"));
        assert_eq!(face.face.columns, [ElemId(0)]);
        let slope_factor = horizontal_factor.hypot(0.1);
        close(face.face.position_mm, face_mm * slope_factor, 1e-9);
        let force = face.forces.as_ref().unwrap();
        close(force[0], 1000.0 / slope_factor, 1e-6);
        close(force[1], 10_000.0 * horizontal_factor / slope_factor, 1e-6);
        close(
            force[5],
            -TIP_LOAD_N * (LENGTH_MM - face_mm) * horizontal_factor,
            1e-2,
        );
    }
}

#[test]
fn 角形鋼管の既知の角の曲面までの断面力を返す() {
    let mut model = model();
    let mut shape = box_shape(600.0, 600.0);
    let SectionShape::SteelBox { corner_r, .. } = &mut shape else {
        unreachable!()
    };
    *corner_r = Some(60.0);
    column_section(&mut model, 0, shape, MaterialId(0));
    model.elements[0].local_axis.ref_vector = [1.0, 1.0, 0.0];
    let result = solve(model);
    let face = faces(&result)[0].as_ref().unwrap();
    let expected_mm = 240.0 * 2.0_f64.sqrt() + 60.0;
    close(face.face.position_mm, expected_mm, 1e-9);
    tip_cut(*face.forces.as_ref().unwrap(), LENGTH_MM - expected_mm);
}

#[test]
fn 支持候補が一致する端は柱識別子を保持し異なる端や未支持端には理由を返す() {
    let mut model = model();
    add_branch(&mut model, 0, NodeId(0), [0.0, 0.0, 6000.0]);
    let result = solve(model.clone());
    assert_eq!(
        faces(&result)[0].as_ref().unwrap().face.columns,
        [ElemId(0), ElemId(3)]
    );
    model.elements[3].section = Some(SectionId(1));
    model.elements[1].nodes = [NodeId(1), NodeId(2)].into_iter().collect();
    model.nodes[2].coord = [0.0, 0.0, -3000.0];
    let result = solve(model);
    let i = faces(&result)[0].as_ref().unwrap_err();
    for fragment in [
        "大梁 ID 2 i端",
        "柱 ID 0",
        "300 mm",
        "柱 ID 3",
        "250 mm",
        "一致しません",
    ] {
        assert!(i.contains(fragment), "{i}");
    }
    let j = faces(&result)[1].as_ref().unwrap_err();
    assert!(
        j.contains("大梁 ID 2 j端") && j.contains("接続柱がありません"),
        "{j}"
    );
}

#[test]
fn 隅角半径や採用面が不明な場合と実形状不足を解析結果で識別する() {
    let mut unknown = model();
    unknown.sections[0].property_basis = Default::default();
    let Some(SectionShape::SteelBox { corner_r, .. }) = &mut unknown.sections[0].shape else {
        unreachable!()
    };
    *corner_r = None;
    let result = solve(unknown.clone());
    assert_eq!(faces(&result)[0].as_ref().unwrap().face.position_mm, 300.0);
    unknown.elements[0].local_axis.ref_vector = [1.0, 1.0, 0.0];

    let mut h_shape = model();
    column_section(
        &mut h_shape,
        0,
        SectionShape::SteelH {
            height: 600.0,
            width: 400.0,
            web_thick: 16.0,
            flange_thick: 25.0,
            root_r: Some(0.0),
        },
        MaterialId(0),
    );
    let result = solve(h_shape.clone());
    assert_eq!(faces(&result)[0].as_ref().unwrap().face.position_mm, 300.0);
    h_shape.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];

    let mut missing = model();
    missing.sections[0].property_basis = Default::default();
    missing.sections[0].shape = None;
    for (input, reason) in [
        (unknown, "角Rが未知"),
        (h_shape, "採用面"),
        (missing, "実断面形状が未指定"),
    ] {
        let result = solve(input);
        let error = faces(&result)[0].as_ref().unwrap_err();
        assert!(
            error.contains("大梁 ID 2 i端") && error.contains("柱 ID 0") && error.contains(reason),
            "{error}"
        );
        assert!(faces(&result)[1].is_ok());
    }
}

#[test]
fn 剛域内の実柱面で荷重のつり合いを満たし集中力の作用点は未算定を保持する() {
    let mut model = model();
    model.elements[GIRDER.index()].rigid_zone.source_i = ZoneSource::Manual;
    model.elements[GIRDER.index()].rigid_zone.length_i = 600.0;
    let load = |a, p| MemberLoad::manual(GIRDER, [0.0, 0.0, -1.0], MemberLoadKind::Point { a, p });
    model.load_cases[0].member.push(load(450.0, 2000.0));
    let continuous = solve(model.clone());
    let face = faces(&continuous)[0].as_ref().unwrap();
    assert_eq!(face.face.position_mm, 300.0);
    let force = face.forces.as_ref().unwrap();
    close(force[1], 12_000.0, 1e-6);
    close(force[5], -57_300_000.0, 1e-2);
    model.load_cases[0]
        .member
        .push(load(300.0 + f64::EPSILON * LENGTH_MM, 1000.0));
    let discontinuous = solve(model);
    let face = faces(&discontinuous)[0].as_ref().unwrap();
    assert_eq!(face.face.position_mm, 300.0);
    let error = face.forces.as_ref().unwrap_err();
    assert!(
        error.contains("大梁 ID 2 i端") && error.contains("切断側"),
        "{error}"
    );
    let combined = superpose_static(&[(&discontinuous, 0.0), (&continuous, 2.0)]).unwrap();
    let force = faces(&combined)[0]
        .as_ref()
        .unwrap()
        .forces
        .as_ref()
        .unwrap();
    close(force[1], 24_000.0, 1e-6);
    close(force[5], -114_600_000.0, 1e-2);
    let combined = superpose_static(&[(&continuous, 1.0), (&discontinuous, 2.0)]).unwrap();
    assert_eq!(
        faces(&combined)[0]
            .as_ref()
            .unwrap()
            .forces
            .as_ref()
            .unwrap_err(),
        error
    );
    let zero = superpose_static(&[(&discontinuous, 0.0)]).unwrap();
    assert_eq!(faces(&zero)[0].as_ref().unwrap().forces, Ok([0.0; 6]));
    let bytes = rmp_serde::to_vec_named(&discontinuous).unwrap();
    let restored: StaticOnce = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(
        restored.girder_column_face_forces,
        discontinuous.girder_column_face_forces
    );
}
