use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::cft::CftDesign;
use sepika_design_jp::{CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt};

fn material(steel: bool, fc: f64) -> Material {
    Material {
        id: MaterialId(0),
        name: if steel { "SN400B" } else { "concrete" }.into(),
        category: if steel {
            MaterialCategory::Steel
        } else {
            MaterialCategory::Concrete
        },
        young: if steel { 205000.0 } else { 25000.0 },
        poisson: 0.3,
        density: 0.0,
        shear: None,
        fc: if steel { None } else { Some(fc) },
        fy: Some(999.0),
        concrete_class: Default::default(),
        strength_factor: None,
    }
}
fn main() {
    let section = SectionShape::CftBox {
        height: 400.0,
        width: 300.0,
        thick: 12.0,
        horizontal_thick: 12.0,
        corner_r: Some(0.0),
    }
    .to_section(SectionId(0), "CFT887".into());
    let force = MemberForcesAt {
        pos: 0.0,
        n: -1000.0,
        my: 0.0,
        mz: 0.0,
        qy: 0.0,
        qz: 0.0,
    };
    let mut ctx = DesignCtx {
        length: 3000.0,
        steel_material: Some(material(true, 0.0)),
        ..Default::default()
    };
    for fc in [21.0, 23.999, 24.0, 24.001] {
        match CftDesign.check(&force, &section, &material(false, fc), &ctx) {
            CheckOutcome::Checked(r) => {
                assert!(r.ok());
                println!("Fc={fc}: Checked ok={} ratio={}", r.ok(), r.ratio());
            }
            CheckOutcome::Skipped { reason } => panic!("Fc={fc}: {reason}"),
        }
    }
    for (term, expected) in [
        (
            LoadTerm::Long,
            [0.12251377939458445, 0.1669028298998687, 0.3382782749327795],
        ),
        (
            LoadTerm::Short,
            [0.08167585292972297, 0.11126855326657913, 0.2255188499551863],
        ),
    ] {
        ctx.term = term;
        for (axis, expected) in expected.into_iter().enumerate() {
            let mut f = MemberForcesAt {
                pos: 0.0,
                n: 0.0,
                my: 0.0,
                mz: 0.0,
                qy: 0.0,
                qz: 0.0,
            };
            f.n = 0.0;
            match axis {
                0 => f.qy = 100000.0,
                1 => f.qz = 100000.0,
                _ => f.mz = 100000000.0,
            }
            let CheckOutcome::Checked(r) =
                CftDesign.check(&f, &section, &material(false, 24.0), &ctx)
            else {
                panic!("未算定");
            };
            assert!(
                (r.ratio() - expected).abs() < 1e-12,
                "{:?} axis={axis}: {}",
                term,
                r.ratio()
            );
            println!("{:?} axis={axis} ratio={:.15}", term, r.ratio());
        }
    }
}
