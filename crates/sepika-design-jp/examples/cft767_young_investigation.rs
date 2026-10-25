use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{
    CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
};

fn material(category: MaterialCategory, young: f64, fc: Option<f64>) -> Material {
    Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category,
        young,
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc,
        fy: Some(235.0),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}

fn main() {
    for (name, shape, radius_mm, long_ratio, short_ratio) in [
        (
            "box",
            SectionShape::CftBox {
                height: 400.0,
                width: 400.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: Some(0.0),
            },
            159.2691642053372,
            3.198638508546,
            1.847918106058,
        ),
        (
            "pipe",
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 10.0,
            },
            137.93114224133723,
            4.072633038394,
            2.352842408065,
        ),
    ] {
        let section = shape.to_section(SectionId(0), "CFT767".into());
        let concrete = material(MaterialCategory::Concrete, 25000.0, Some(24.0));
        for (term, expected) in [(LoadTerm::Long, long_ratio), (LoadTerm::Short, short_ratio)] {
            for steel_e in [100000.0, 205000.0, 300000.0] {
                let ctx = DesignCtx {
                    term,
                    kind: MemberKind::Column,
                    length: 100.0 * radius_mm,
                    steel_material: Some(material(MaterialCategory::Steel, steel_e, None)),
                    ..Default::default()
                };
                let forces = MemberForcesAt {
                    pos: 0.0,
                    n: -8.0e6,
                    qy: 0.0,
                    qz: 0.0,
                    my: 0.0,
                    mz: 0.0,
                };
                let result = match CftDesign.check(&forces, &section, &concrete, &ctx) {
                    CheckOutcome::Checked(result) => result,
                    CheckOutcome::Skipped { reason } => panic!("{reason}"),
                };
                println!(
                    "{name} {term:?} E={steel_e}: ratio={:.12}; {}",
                    result.ratio(),
                    result.components[0].detail
                );
                println!("fixed_candidate_independent_ratio={expected:.12}");
            }
        }
    }
}
