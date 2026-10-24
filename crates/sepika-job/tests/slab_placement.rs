use sepika_core::{frame_gen::*, ids::*, model::*};
use sepika_element::{frame::member_load::FrameDeadLoadMap, transform::LocalFrame};

#[test]
fn 床平面荷重の実材長換算は傾斜梁の三次元等価節点力と仮想仕事を保つ() {
    let mut model = frame_model(&FrameSpec {
        x_spans: vec![4000.],
        y_spans: vec![3000.],
        ..Default::default()
    })
    .unwrap();
    model.slabs[0].plate.method = DistributionMethod::OneWay;
    model.slabs[0].plate.one_way = Some(OneWayDir::Y);
    let boundary = model.slabs[0].boundary_nodes(&model).unwrap();
    model.nodes[boundary[0].index()].coord[2] = -3000.;
    let support = model.slab_assignment_region(SlabId(0)).unwrap().boundary[0].support;
    let SupportMemberId::Primary(elem) = support else {
        panic!("大梁支持")
    };
    let loads =
        sepika_load::floor::distribute_slab_resolved(&model, &model.slabs[0], 0.01).unwrap();
    let cmq = loads.iter().find(|l| l.elem == elem).unwrap().cmq;
    assert!((cmq.c_i - 20000000.).abs() < 1e-7);
    assert!((cmq.c_j + 20000000.).abs() < 1e-7);
    assert!((cmq.q_i - 24000.).abs() < 1e-7);
    assert!((cmq.q_j - 24000.).abs() < 1e-7);
    let (_, member) = sepika_load::slab_load::slab_load_case_content(&model, &loads);
    let member: Vec<_> = member.into_iter().filter(|l| l.elem == elem).collect();
    assert_eq!(member.len(), 1);
    assert_eq!(member[0].dir, [0., 0., -1.]);
    let MemberLoadKind::Distributed { a, b, w1, w2 } = member[0].kind else {
        panic!("等分布")
    };
    assert_eq!((a, b, w1, w2), (0., 5000., 12., 12.));
    let frame = LocalFrame {
        rot: [[0.8, 0., 0.6], [-0.6, 0., 0.8], [0., -1., 0.]],
    };
    let map = FrameDeadLoadMap::new(&member, &frame, 5000., 0., 0.).unwrap();
    let local = map.flex_load();
    let global: [f64; 12] = std::array::from_fn(|i| {
        let base = i / 3 * 3;
        (0..3).map(|k| frame.rot[k][i % 3] * local[base + k]).sum()
    });
    let expected = [
        0., 0., -30000., 0., 20000000., 0., 0., 0., -30000., 0., -20000000., 0.,
    ];
    for (actual, expected) in global.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-7, "{actual} != {expected}");
    }
    let rigid_vertical_work = 2. * (global[2] + global[8]);
    assert!((rigid_vertical_work + 120000.).abs() < 1e-7);
    let i_rotation_work = global[4] * 0.001;
    assert!((i_rotation_work - 20000.).abs() < 1e-7);
}

#[test]
fn 新入力の配置不備は古い有効配置から自動荷重を再生成しない() {
    let mut model = frame_model(&FrameSpec::default()).unwrap();
    let mut material = model.materials[0].clone();
    material.id = MaterialId(model.materials.len() as u32);
    material.name = "自重0の鋼材".into();
    material.category = MaterialCategory::Steel;
    material.young = 205000.;
    material.density = 0.;
    material.fc = None;
    material.fy = Some(235.);
    let mut section = sepika_core::section_shape::SectionShape::SteelFlatBar {
        width: 200.,
        thick: 400.,
    }
    .to_section(
        SectionId(model.sections.len() as u32),
        "自重0の支持梁".into(),
    );
    section.material = Some(material.id);
    section.frame_use = Some(FrameSectionUse::Girder);
    for element in &mut model.elements {
        element.section = Some(section.id);
    }
    model.materials.push(material);
    model.sections.push(section);
    sepika_job::auto_loads::compute_gravity_auto_load_cases(&model).unwrap();
    model.slabs[0].vertical_eccentricity_mm = None;
    let reason = sepika_job::auto_loads::compute_gravity_auto_load_cases(&model)
        .err()
        .unwrap()
        .to_string();
    assert!(
        reason.contains("床板 0") && reason.contains("鉛直偏心"),
        "{reason}"
    );
    assert!(model.slabs[0].surface_coords(&model).is_none());
}

#[test]
fn 水平床の先端荷重は傾斜支持材長ではなく床先端の実長を使う() {
    let mut model = frame_model(&FrameSpec {
        x_spans: vec![4000.],
        y_spans: vec![3000.],
        ..Default::default()
    })
    .unwrap();
    let boundary = model.slabs[0].boundary_nodes(&model).unwrap();
    model.nodes[boundary[0].index()].coord[2] = -3000.;
    model.slabs[0].shape = SlabShape::Attached {
        anchor: RegionAnchor::Line {
            nodes: [boundary[0], boundary[1]],
            span: [0., 1.],
            transfer: LoadTransfer::Anchor,
        },
        extent: [2000.; 2],
    };
    model.slabs[0].tip_loads.push(SlabTipLoad {
        case: LoadCaseId(0),
        intensity: 2.,
        direction: TipLoadDirection::NegZ,
    });
    let cases = sepika_job::auto_loads::compute_tip_loads(&model).unwrap();
    let member = &cases.iter().find(|(id, _)| *id == LoadCaseId(0)).unwrap().1;
    assert_eq!(member.len(), 1);
    let MemberLoadKind::Distributed { a, b, w1, w2 } = member[0].kind else {
        panic!("先端分布荷重")
    };
    assert_eq!((a, b, w1, w2), (0., 5000., 1.6, 1.6));
    assert!(((b - a) * (w1 + w2) / 2. - 8000.).abs() < 1e-7);
}
