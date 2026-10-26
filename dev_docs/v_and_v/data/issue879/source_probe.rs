use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, Section};
use sepika_design_jp::material_strength::{
    big_lambda, section_steel_f, steel_fc, steel_fs, steel_ft,
};
use sepika_design_jp::{
    CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind, SteelDesign,
};

fn material(name: &str, young: f64) -> Material {
    Material {
        id: MaterialId(0),
        name: name.into(),
        category: MaterialCategory::Steel,
        young,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(999.0),
        strength_factor: None,
        concrete_class: Default::default(),
    }
}

fn section() -> Section {
    Section {
        area: 10_000.0,
        iy: 400_000_000.0,
        iz: 100_000_000.0,
        depth: 400.0,
        width: 200.0,
        thickness: Some(12.0),
        ..Section::zero(SectionId(0), "明示断面性能".into())
    }
}

#[test]
fn material_table_and_allowables() {
    for (name, thickness, expected) in [
        ("SS400", 40.0, 235.0),
        ("SS400", 40.0001, 215.0),
        ("SS400", 100.0, 215.0),
        ("SN400B", 40.0, 235.0),
        ("SN400B", 40.0001, 215.0),
        ("SS490", 40.0, 275.0),
        ("SS490", 40.0001, 255.0),
        ("SN490B", 40.0, 325.0),
        ("SN490B", 40.0001, 295.0),
        ("SM490A", 40.0, 325.0),
        ("SM490A", 40.0001, 295.0),
        ("SM520B", 40.0, 355.0),
        ("SM520B", 40.0001, 335.0),
        ("SM520B", 75.0, 335.0),
        ("SM520B", 75.0001, 325.0),
        ("SM520B", 100.0, 325.0),
    ] {
        let s = Section {
            thickness: Some(thickness),
            ..section()
        };
        let f = section_steel_f(&s, Some(&material(name, 205_000.0))).unwrap();
        assert_eq!(f, expected);
        println!("F,{name},{thickness},{f}");
    }
    for name in ["SS400", "SN400B", "SS490", "SN490B", "SM490A", "SM520B"] {
        let s = Section {
            thickness: Some(100.0001),
            ..section()
        };
        assert!(section_steel_f(&s, Some(&material(name, 205_000.0))).is_err());
    }
    for (f, ft, fs, short_fs) in [
        (
            235.0,
            156.66666666666667,
            90.45154217304137,
            135.67731325956205,
        ),
        (
            215.0,
            143.33333333333333,
            82.75353858384636,
            124.13030787576954,
        ),
        (
            325.0,
            216.66666666666667,
            125.09255832441892,
            187.63883748662837,
        ),
    ] {
        assert!((steel_ft(f, LoadTerm::Long) - ft).abs() < 1e-10);
        assert_eq!(steel_ft(f, LoadTerm::Short), f);
        assert!((steel_fs(f, LoadTerm::Long) - fs).abs() < 1e-10);
        assert!((steel_fs(f, LoadTerm::Short) - short_fs).abs() < 1e-10);
        println!("allowable,{f},{ft},{fs},{short_fs}");
    }
}

#[test]
fn compression_and_actual_member_entry() {
    for (young, lambda, expected) in [
        (205_000.0, 0.0, 156.66666666666667),
        (205_000.0, 59.92015964527209, 126.877080583895),
        (205_000.0, 119.78908480518268, 65.07692307692308),
        (205_000.0, 150.0, 41.514480438212),
        (206_000.0, 150.0, 41.716990098886),
        (210_000.0, 150.0, 42.527028741583),
    ] {
        assert!((steel_fc(235.0, young, lambda, LoadTerm::Long) - expected).abs() < 1e-9);
    }
    for young in [205_000.0, 206_000.0, 210_000.0] {
        let boundary = big_lambda(235.0, young);
        for lambda in [
            0.0,
            59.92015964527209,
            boundary - 1e-6,
            boundary,
            boundary + 1e-6,
            119.84031929054418,
            150.0,
            200.0,
            200.0001,
            250.0,
            250.0001,
        ] {
            let fc = steel_fc(235.0, young, lambda, LoadTerm::Long);
            for term in [LoadTerm::Long, LoadTerm::Short] {
                let mat = material("SS400", young);
                let sec = section();
                let forces = MemberForcesAt {
                    pos: 0.5,
                    n: -100_000.0,
                    qy: 0.0,
                    qz: 0.0,
                    my: 0.0,
                    mz: 0.0,
                };
                for kind in [MemberKind::Column, MemberKind::Girder, MemberKind::Brace] {
                    let ctx = DesignCtx {
                        kind,
                        term,
                        length: 5000.0,
                        lk_y: Some(lambda * 200.0),
                        lk_z: Some(lambda * 100.0),
                        ..Default::default()
                    };
                    let result = SteelDesign.check(&forces, &sec, &mat, &ctx);
                    let CheckOutcome::Checked(result) = result else {
                        panic!("{result:?}")
                    };
                    let adopted = 10.0 / result.ratio();
                    let expected = if term == LoadTerm::Long { fc } else { fc * 1.5 };
                    assert!((adopted - expected).abs() < 1e-9);
                    println!(
                        "entry,{young},{kind:?},{term:?},{lambda:.12},{adopted:.12},ok={}",
                        result.ok()
                    );
                }
            }
        }
    }
}

#[test]
fn axis_lengths_and_tension_entry() {
    assert_eq!(
        sepika_design_jp::effective_slenderness(
            400_000_000.0,
            100_000_000.0,
            10_000.0,
            5000.0,
            Some(6000.0),
            Some(8000.0)
        ),
        80.0
    );
    let forces = MemberForcesAt {
        pos: 0.5,
        n: 1_500_000.0,
        qy: 0.0,
        qz: 0.0,
        my: 0.0,
        mz: 0.0,
    };
    let sec = Section {
        thickness: Some(50.0),
        ..section()
    };
    let ctx = DesignCtx {
        kind: MemberKind::Brace,
        ..Default::default()
    };
    let CheckOutcome::Checked(result) =
        SteelDesign.check(&forces, &sec, &material("SS400", 205_000.0), &ctx)
    else {
        panic!()
    };
    assert!((result.ratio() - 1.0465116279069768).abs() < 1e-12);
}
