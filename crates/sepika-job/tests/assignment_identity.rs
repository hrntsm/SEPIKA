use sepika_core as core_fixture_api;
use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{FrameSectionUse, Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_job::prepare::prepare_model_for_analysis;
use sepika_job::settings::AnalysisSettings;
#[path = "../../sepika-core/tests/support/assignment_identity_model.rs"]
mod fixture;

#[test]
fn 解析準備は孤立版と入力荷重を候補で診断しモデルを確定しない() {
    for wall in [false, true] {
        let mut model = fixture::with_plate_metadata(wall);
        model.materials.push(Material {
            id: MaterialId(0),
            name: "鋼材".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        });
        for (id, frame_use) in [FrameSectionUse::Girder, FrameSectionUse::Column]
            .into_iter()
            .enumerate()
        {
            let mut section = SectionShape::SteelFlatBar {
                width: 200.0,
                thick: 400.0,
            }
            .to_section(SectionId(id as u32), format!("支持材{id}"));
            section.material = Some(MaterialId(0));
            section.frame_use = Some(frame_use);
            model.sections.push(section);
        }
        for element in &mut model.elements {
            let vertical = model.nodes[element.nodes[0].index()].coord[2]
                != model.nodes[element.nodes[1].index()].coord[2];
            element.section = Some(SectionId(u32::from(vertical)));
        }
        if wall {
            model.unassigned_posts.push(fixture::divider(true));
        } else {
            model.unassigned_beams.push(fixture::divider(false));
        }
        let before = model.clone();
        let error = match prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
        {
            Ok(_) => panic!("孤立版は拒否"),
            Err(e) => e.to_string(),
        };
        assert!(error.contains("孤立版"), "{error}");
        assert!(error.contains("0.0025"));
        assert!(error.contains("未更新"));
        fixture::assert_inputs_eq(&model, &before);
    }
}
