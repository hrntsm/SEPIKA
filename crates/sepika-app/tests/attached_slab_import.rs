use sepika_core::geom::polygon;
use sepika_core::ids::{ElemId, NodeId, SlabId};
use sepika_core::model::{
    ElementKind, LoadTransfer, MemberLoadKind, Model, RegionAnchor, SlabShape,
};
use sepika_core::region_rebuild::rebuild_floor_regions;
use sepika_io::stbridge::import_stbridge_with_report;
use sepika_job::auto_loads::slab_load_case_content;
use sepika_load::floor::distribute_slab_resolved;

#[test]
fn imported_boundary_rectangle_preserves_support_interval_area_and_actual_load_resultant() {
    let xml = include_str!("../../sepika-io/tests/fixtures/cantilever_slab.stb");
    let (model, _) = import_stbridge_with_report(xml).unwrap();
    assert_eq!(model.slabs.len(), 1);
    let slab = &model.slabs[0];
    assert!(slab.is_attached());
    let before_xy = [[0.0, 0.0], [4000.0, 0.0], [4000.0, 1500.0], [0.0, 1500.0]];
    assert_eq!(polygon::area(&before_xy), 6_000_000.0);
    assert_eq!(
        polygon::area_xy(&slab.boundary_coords(&model).unwrap()),
        6_000_000.0
    );
    let supports = model.attached_slab_supports(slab).unwrap();
    assert_eq!(supports.len(), 1);
    let beam = &model.elements[0];
    assert_eq!(beam.kind, ElementKind::Beam);
    assert_eq!(supports[0].elem, beam.id);
    assert_eq!(supports[0].span, [0.0, 1.0]);
    assert_eq!(supports[0].fraction, 1.0);

    let loads = distribute_slab_resolved(&model, slab, 0.003).unwrap();
    let (nodal, member) = slab_load_case_content(&model, &loads);
    assert!(nodal.is_empty());
    assert_eq!(member.len(), 1);
    assert_eq!(member[0].elem, beam.id);
    assert_eq!(member[0].dir, [0.0, 0.0, -1.0]);
    let MemberLoadKind::Distributed { a, b, w1, w2 } = member[0].kind else {
        panic!("取付き梁へ等分布荷重を渡すこと");
    };
    assert_eq!([a, b], [0.0, 4000.0]);
    assert_eq!([w1, w2], [4.5, 4.5]);
    assert_eq!((w1 + w2) / 2.0 * (b - a), 18_000.0);
}

fn assert_actual_supports_and_loads(model: &Model, expected: &[([f64; 2], f64)]) {
    assert_eq!(model.slabs.len(), expected.len());
    let mut total_n = 0.0;
    for (slab, (span, area_mm2)) in model.slabs.iter().zip(expected) {
        assert_eq!(
            polygon::area_xy(&slab.boundary_coords(model).unwrap()),
            *area_mm2
        );
        let supports = model.attached_slab_supports(slab).unwrap();
        assert_eq!(supports.len(), 1);
        assert_eq!(supports[0].elem, ElemId(0));
        assert_eq!(supports[0].span, *span);
        assert_eq!(supports[0].fraction, 1.0);
        let loads = distribute_slab_resolved(model, slab, 0.003).unwrap();
        let (nodal, member) = slab_load_case_content(model, &loads);
        assert!(nodal.is_empty());
        assert_eq!(member.len(), 1);
        assert_eq!(member[0].elem, ElemId(0));
        assert_eq!(member[0].dir, [0.0, 0.0, -1.0]);
        let MemberLoadKind::Distributed { a, b, w1, w2 } = member[0].kind else {
            panic!("取付き梁への分布荷重");
        };
        assert_eq!([a, b], [span[0] * 4000.0, span[1] * 4000.0]);
        let resultant_n = (w1 + w2) / 2.0 * (b - a);
        assert!((resultant_n - 0.003 * area_mm2).abs() < 1e-6);
        total_n += resultant_n;
    }
    assert!((total_n - 9000.0).abs() < 1e-6);
}

#[test]
fn existing_split_preserves_same_geometry_support_intervals_and_actual_resultant() {
    for sign in [1.0, -1.0] {
        let xml = include_str!("../../sepika-io/tests/fixtures/cantilever_slab.stb");
        let (mut model, _) = import_stbridge_with_report(xml).unwrap();
        model.slabs[0].shape = SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.25, 0.75],
                transfer: LoadTransfer::Anchor,
            },
            extent: [sign * 1000.0, sign * 2000.0],
        };
        let mut root = model.nodes[0].clone();
        root.id = NodeId(4);
        root.coord = [2000.0, 0.0, 0.0];
        let mut tip = root.clone();
        tip.id = NodeId(5);
        tip.coord[1] = sign * 1500.0;
        model.nodes.extend([root, tip]);
        let mut cross_beam = model.elements[0].clone();
        cross_beam.id = ElemId(1);
        cross_beam.nodes = vec![NodeId(4), NodeId(5)].into();
        model.elements.push(cross_beam);
        let original_members = format!("{:?}", model.elements);
        assert_actual_supports_and_loads(&model, &[([0.25, 0.75], 3_000_000.0)]);
        rebuild_floor_regions(&mut model);
        assert_eq!(format!("{:?}", model.elements), original_members);
        assert_actual_supports_and_loads(
            &model,
            &[([0.25, 0.5], 1_250_000.0), ([0.5, 0.75], 1_750_000.0)],
        );
    }
}

#[test]
fn existing_merge_preserves_same_geometry_support_intervals_and_actual_resultant() {
    for sign in [1.0, -1.0] {
        let xml = include_str!("../../sepika-io/tests/fixtures/cantilever_slab.stb");
        let (mut model, _) = import_stbridge_with_report(xml).unwrap();
        let mut right = model.slabs[0].clone();
        right.id = SlabId(1);
        for (slab, span, extent) in [
            (&mut model.slabs[0], [0.25, 0.5], [1000.0, 1500.0]),
            (&mut right, [0.5, 0.75], [1500.0, 2000.0]),
        ] {
            slab.shape = SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span,
                    transfer: LoadTransfer::Anchor,
                },
                extent: extent.map(|value| sign * value),
            };
        }
        model.slabs.push(right);
        let original_members = format!("{:?}", model.elements);
        assert_actual_supports_and_loads(
            &model,
            &[([0.25, 0.5], 1_250_000.0), ([0.5, 0.75], 1_750_000.0)],
        );
        rebuild_floor_regions(&mut model);
        assert_eq!(format!("{:?}", model.elements), original_members);
        assert_actual_supports_and_loads(&model, &[([0.25, 0.75], 3_000_000.0)]);
    }
}
