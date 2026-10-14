pub fn input_model(mut model: sepika_core::model::Model) -> sepika_core::model::Model {
    use sepika_core::ids::{LoadCaseId, NodeId};
    use sepika_core::model::{LoadCase, LoadCaseKind, NodalLoad};
    for node in &mut model.nodes {
        if node.coord[0] > 0.0 {
            node.coord[0] = 4200.0;
        }
    }
    if let Some(sepika_core::section_shape::SectionShape::RcWall {
        thickness,
        pwh_ratio,
        ..
    }) = &mut model.sections[2].shape
    {
        *thickness = 200.0;
        *pwh_ratio = Some(0.002);
    }
    model.materials[0].density = 1e-30;
    model.materials[0].shear = Some(3339.437618147448);
    // AwQu=1440000、te=300、d=4500、10-D22、M/QD<=1のmean式からQu=4000000となる圧縮N。
    model.load_cases = vec![LoadCase {
        id: LoadCaseId(0),
        name: "Qc独立fixture".into(),
        kind: LoadCaseKind::Dead,
        nodal: vec![
            NodalLoad::manual(
                NodeId(2),
                [50_000.0, 0.0, -6_025_275.701739306, 0.0, 0.0, 0.0],
            ),
            NodalLoad::manual(
                NodeId(3),
                [50_000.0, 0.0, -6_025_275.701739306, 0.0, 0.0, 0.0],
            ),
        ],
        member: Vec::new(),
    }];
    model
}

pub fn assert_reference(reference: &sepika_design_jp::wall_check::WallShearReference) {
    let close = |actual: f64, expected: f64| {
        assert!(
            (actual - expected).abs() <= expected.abs() * 1e-9,
            "actual={actual:.15}, expected={expected:.15}"
        );
    };
    close(reference.aw_qc_mm2, 840_000.0);
    close(reference.beta_s, 0.15130833333333333);
    assert_eq!(reference.displacement_points[0], (0.0, 0.0));
    assert_eq!(reference.angle_points[0], (0.0, 0.0));
    for (actual, expected) in reference
        .displacement_points
        .iter()
        .skip(1)
        .zip([(2.016, 2_016_000.0), (26.43608525637538, 4_000_000.0)])
    {
        close(actual.0, expected.0);
        close(actual.1, expected.1);
    }
    for (actual, expected) in reference
        .angle_points
        .iter()
        .skip(1)
        .zip([(0.000672, 2_016_000.0), (0.008812028418791793, 4_000_000.0)])
    {
        close(actual.0, expected.0);
        close(actual.1, expected.1);
    }
    close(reference.second_stiffness_n_per_mm, 81244.59759951428);
}
