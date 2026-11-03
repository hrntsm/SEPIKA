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
        origin: Default::default(),
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
