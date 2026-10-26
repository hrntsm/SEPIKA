use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt};

#[test]
fn cft_concrete_strength_statutory_lower_bound() {
    let steel = Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: None,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    };
    for shape in [
        SectionShape::CftBox {
            height: 400.0,
            width: 300.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        },
        SectionShape::CftPipe {
            outer_dia: 400.0,
            thick: 12.0,
        },
    ] {
        let section = shape.to_section(SectionId(0), "CFT916".into());
        for term in [LoadTerm::Long, LoadTerm::Short] {
            for fc in [21.0, 23.999, 24.0, 24.001] {
                let concrete = Material {
                    category: MaterialCategory::Concrete,
                    fc: Some(fc),
                    young: 25000.0,
                    ..steel.clone()
                };
                let outcome = CftDesign.check(
                    &MemberForcesAt {
                        pos: 0.0,
                        n: -1000.0,
                        qy: 0.0,
                        qz: 0.0,
                        my: 0.0,
                        mz: 0.0,
                    },
                    &section,
                    &concrete,
                    &DesignCtx {
                        term,
                        length: 3000.0,
                        steel_material: Some(steel.clone()),
                        ..Default::default()
                    },
                );
                match outcome {
                    CheckOutcome::Skipped { reason } if fc < 24.0 => {
                        assert!(reason.contains("告示464号第三2"), "{reason}");
                        assert!(reason.contains("下限24 N/mm²未満"), "{reason}");
                        assert!(reason.contains(&format!("Fc={fc} N/mm²")), "{reason}");
                    }
                    CheckOutcome::Checked(result) if fc >= 24.0 => assert!(result.ok()),
                    other => panic!("Fc={fc}, {term:?}, {other:?}"),
                }
                assert_eq!(concrete.fc, Some(fc));
            }
        }
    }
}
