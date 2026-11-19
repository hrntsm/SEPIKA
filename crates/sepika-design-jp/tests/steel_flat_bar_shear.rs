use sepika_core::ids::{MaterialId, SectionId};
use sepika_core::model::{Material, MaterialCategory, PropertyBasis, Section};
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::{
    CheckKind, CheckOutcome, CheckResult, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt,
    MemberKind, SteelDesign,
};

const SHEAR_N: f64 = 100_000.0;
const FS_MPA: f64 = 135.677313259562;
const FLAT_BAR: SectionShape = SectionShape::SteelFlatBar {
    width: 100.0,
    thick: 10.0,
};

fn section(shape: SectionShape) -> Section {
    shape.to_section(SectionId(0), "FB-100x10".into())
}

fn forces(qy: f64, qz: f64) -> MemberForcesAt {
    MemberForcesAt {
        pos: 0.5,
        n: 0.0,
        qy,
        qz,
        mx: 0.0,
        my: 0.0,
        mz: 0.0,
    }
}

fn check(
    section: &Section,
    forces: &MemberForcesAt,
    kind: MemberKind,
    length: f64,
) -> CheckOutcome {
    let material = Material {
        id: MaterialId(0),
        name: "直接鋼材".into(),
        category: MaterialCategory::Steel,
        young: 205_000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(235.0),
        strength_factor: None,
        concrete_class: Default::default(),
    };
    SteelDesign.check(
        forces,
        section,
        &material,
        &DesignCtx {
            kind,
            term: LoadTerm::Short,
            length,
            ..Default::default()
        },
    )
}

fn assert_local_shear_unchecked(outcome: CheckOutcome) {
    assert!(
        matches!(&outcome, CheckOutcome::Skipped { reason }
            if reason.contains("平鋼のせん断応力度") && reason.contains("許容値の組合せが未確定")),
        "{outcome:?}"
    );
}

fn checked(outcome: CheckOutcome) -> CheckResult {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    result
}

#[test]
fn pure_shear_on_either_flat_bar_axis_is_unchecked_in_girder_and_column() {
    let section = section(FLAT_BAR);
    // 中立軸の VQ/(Ib) に対する各方向の独立寸法。材端・載荷点から離れた弾性梁を想定。
    for (qy, qz, b_mm, h_mm) in [(SHEAR_N, 0.0, 100.0, 10.0_f64), (0.0, SHEAR_N, 10.0, 100.0)] {
        let first_moment_mm3 = b_mm * h_mm.powi(2) / 8.0;
        let inertia_mm4 = b_mm * h_mm.powi(3) / 12.0;
        let tau_max_mpa = SHEAR_N * first_moment_mm3 / (inertia_mm4 * b_mm);
        assert!((tau_max_mpa - 150.0).abs() < 1e-12);
        assert!((tau_max_mpa / FS_MPA - 1.105_564_345_256_73).abs() < 1e-12);
        let stiffness_transfer_area_mm2 = if qy != 0.0 {
            section.as_z
        } else {
            section.as_y
        };
        assert!((stiffness_transfer_area_mm2 - 833.3333333333334).abs() < 1e-10);
        assert!((SHEAR_N / stiffness_transfer_area_mm2 / FS_MPA - 0.884451476205384).abs() < 1e-12);
        for kind in [MemberKind::Girder, MemberKind::Column] {
            assert_local_shear_unchecked(check(&section, &forces(qy, qz), kind, 0.0));
        }
    }
}

#[test]
fn supplied_and_nonpositive_areas_do_not_enable_flat_bar_shear_checks() {
    let mut section = section(FLAT_BAR);
    section.property_basis.as_y = PropertyBasis::Supplied;
    section.property_basis.as_z = PropertyBasis::Supplied;
    for (as_y, as_z) in [(4000.0, 6000.0), (0.0, -1.0)] {
        section.as_y = as_y;
        section.as_z = as_z;
        for shear_n in [
            -SHEAR_N,
            f64::MIN_POSITIVE,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            for f in [forces(shear_n, 0.0), forces(0.0, shear_n)] {
                for kind in [MemberKind::Girder, MemberKind::Column] {
                    assert_local_shear_unchecked(check(&section, &f, kind, 0.0));
                }
            }
        }
    }
}

#[test]
fn flat_bar_without_shear_and_axial_brace_keep_existing_ratios() {
    let section = section(FLAT_BAR);
    let mut f = forces(0.0, -0.0);
    // N/A=10 N/mm²、My/Zweak=3 N/mm²、Mz/Zstrong=6 N/mm²。
    f.n = 10_000.0;
    f.my = 50_000.0;
    f.mz = 10_000.0;
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let result = checked(check(&section, &f, kind, 0.0));
        assert!((result.ratio() - 19.0 / 235.0).abs() < 1e-12, "{result:?}");
    }
    f.qy = SHEAR_N;
    let result = checked(check(&section, &f, MemberKind::Brace, 0.0));
    assert_eq!(result.components.len(), 1);
    assert_eq!(result.components[0].kind, CheckKind::Axial);
    assert!((result.ratio() - 10.0 / 235.0).abs() < 1e-12, "{result:?}");
}

#[test]
fn statutory_slenderness_failure_takes_precedence_for_flat_bar_in_compression() {
    let mut f = forces(SHEAR_N, 0.0);
    f.n = -10_000.0;
    for kind in [MemberKind::Girder, MemberKind::Column] {
        let result = checked(check(&section(FLAT_BAR), &f, kind, 1000.0));
        assert_eq!(result.components.len(), 1);
        assert_eq!(result.components[0].kind, CheckKind::Provision);
        assert!(result.ratio() > 1.0, "{result:?}");
    }
}

#[test]
fn other_section_shapes_and_name_only_sections_keep_existing_shear_area_selection() {
    let cases = [
        (
            SectionShape::SteelRoundBar { dia: 40.0 },
            [360.0 * std::f64::consts::PI; 2],
        ),
        (
            SectionShape::SteelChannel {
                height: 150.0,
                width: 75.0,
                web_thick: 6.0,
                flange_thick: 9.0,
            },
            [900.0, 1350.0],
        ),
        (
            SectionShape::SteelTee {
                height: 150.0,
                width: 75.0,
                web_thick: 6.0,
                flange_thick: 9.0,
            },
            [900.0, 675.0],
        ),
        (
            SectionShape::SteelAngle {
                leg_a: 100.0,
                leg_b: 75.0,
                thick: 10.0,
            },
            [1000.0, 750.0],
        ),
    ];
    let mut sections: Vec<_> = cases
        .into_iter()
        .map(|(shape, areas)| (section(shape), areas))
        .collect();
    let mut name_only = section(FLAT_BAR);
    name_only.shape = None;
    name_only.property_basis.as_y = PropertyBasis::Supplied;
    name_only.property_basis.as_z = PropertyBasis::Supplied;
    name_only.as_y = 4000.0;
    name_only.as_z = 6000.0;
    sections.push((name_only.clone(), [6000.0, 4000.0]));
    name_only.as_y = -1.0;
    name_only.as_z = 0.0;
    sections.push((name_only, [1000.0, 1000.0]));
    for (section, [ay_mm2, az_mm2]) in sections {
        for (f, area_mm2) in [
            (forces(SHEAR_N, 0.0), ay_mm2),
            (forces(0.0, SHEAR_N), az_mm2),
        ] {
            for kind in [MemberKind::Girder, MemberKind::Column] {
                let result = checked(check(&section, &f, kind, 0.0));
                let expected = SHEAR_N / area_mm2 / FS_MPA;
                assert!(
                    (result.ratio() - expected).abs() < 1e-12,
                    "{:?} {result:?}",
                    section.shape
                );
            }
        }
    }
}
