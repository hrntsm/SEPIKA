use sepika_core::{frame_gen::*, ids::*, model::*};
use sepika_load::{floor::*, slab_load::slab_load_case_content};

#[test]
fn 床上面と梁が離れても明示支持へ同じ平面合力を実材長あたりで渡す() {
    let mut model = frame_model(&FrameSpec {
        x_spans: vec![4000.],
        y_spans: vec![3000.],
        ..Default::default()
    })
    .unwrap();
    let id = SlabId(0);
    let expected_targets = distribute_slab_resolved(&model, &model.slabs[0], 0.01)
        .unwrap()
        .iter()
        .map(|l| l.target)
        .collect::<Vec<_>>();
    let supports = model.slab_assignment_region(id).unwrap().clone();
    model.slabs[0].vertical_eccentricity_mm = Some(1000.);
    let first = model.slabs[0].boundary_nodes(&model).unwrap()[0];
    model.nodes[first.index()].coord[2] = -3000.;
    for element in &mut model.elements {
        element.vertical_eccentricity = Some(-500.);
    }
    model.rebuild_assignment_regions().unwrap();
    assert_eq!(*model.slab_assignment_region(id).unwrap(), supports);
    let placement = model.resolved_slab_placement(id).unwrap();
    assert_eq!((placement.top_z_mm, placement.bottom_z_mm), (1000., 850.));
    let loads = distribute_slab_resolved(&model, &model.slabs[0], 0.01).unwrap();
    assert_eq!(
        loads.iter().map(|l| l.target).collect::<Vec<_>>(),
        expected_targets
    );
    let (nodal, member) = slab_load_case_content(&model, &loads);
    assert!(nodal.is_empty());
    let mut force = 0.;
    let mut first_x = 0.;
    let mut first_y = 0.;
    for load in member {
        let MemberLoadKind::Distributed { a, b, w1, w2 } = load.kind else {
            panic!("分布荷重")
        };
        let e = model.element(load.elem).unwrap();
        let p = model.node(e.nodes[0]).unwrap().coord;
        let q = model.node(e.nodes[1]).unwrap().coord;
        let length = model.member_length(e);
        let total = (b - a) * (w1 + w2) / 2.;
        let first = a * total + (b - a).powi(2) * (w1 + 2. * w2) / 6.;
        force += total;
        first_x += total * p[0] + first * (q[0] - p[0]) / length;
        first_y += total * p[1] + first * (q[1] - p[1]) / length;
    }
    assert!((force - 120000.).abs() < 1e-7);
    assert!((first_x - 240000000.).abs() < 1e-5);
    assert!((first_y - 180000000.).abs() < 1e-5);
}
