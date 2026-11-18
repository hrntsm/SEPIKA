use std::f64::consts::PI;

use sepika_core::{ids::ElemId, section_shape::SectionShape};
use sepika_design_jp::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberDesignCheckOptions,
    MemberForcesAt, MemberKind, QdMethod, SeismicQd,
    cft::CftDesign,
    run_member_design_checks,
    ultimate::{
        cft::{CftAxialInput, CftColumnClass, cft_axial_ultimate},
        cft_mu_nm, collect_cft_ultimate_checks,
    },
};

#[path = "../tests/fixtures/cft_column_buckling_model.rs"]
mod model_input;

const COLUMN: ElemId = ElemId(0);

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() < tolerance,
        "actual={actual}, expected={expected}"
    );
}

fn circle(diameter_mm: f64, thickness_mm: f64) -> SectionShape {
    SectionShape::CftPipe {
        outer_dia: diameter_mm,
        thick: thickness_mm,
    }
}

fn circle_axial(diameter_mm: f64, thickness_mm: f64, fy: f64, lk_mm: f64) -> CftAxialInput {
    let outer_radius_mm = diameter_mm / 2.0;
    let core_radius_mm = outer_radius_mm - thickness_mm;
    CftAxialInput {
        circular: true,
        d_section: diameter_mm,
        c_area: PI * core_radius_mm.powi(2),
        s_area: PI * (outer_radius_mm.powi(2) - core_radius_mm.powi(2)),
        c_inertia: PI / 4.0 * core_radius_mm.powi(4),
        s_inertia: PI / 4.0 * (outer_radius_mm.powi(4) - core_radius_mm.powi(4)),
        fc: 30.0,
        fy,
        s_young: 205_000.0,
        lk: lk_mm,
    }
}

fn measured_mu(shape: &SectionShape, fy: f64, n_n: f64, lk_mm: f64) -> f64 {
    cft_mu_nm(shape, 30.0, fy, n_n, lk_mm, false).unwrap() / 1e6
}

fn main() {
    let pipe = circle(400.0, 16.0);
    let ntu_n = PI * (200.0_f64.powi(2) - 184.0_f64.powi(2)) * 325.0;
    for (lk_mm, class) in [
        (1600.0, CftColumnClass::Short),
        (1600.001, CftColumnClass::Medium),
        (4800.0, CftColumnClass::Medium),
        (4800.001, CftColumnClass::Long),
    ] {
        let axial = cft_axial_ultimate(&circle_axial(400.0, 16.0, 325.0, lk_mm));
        assert_eq!(axial.class, class);
        close(axial.ntu, ntu_n, 1e-7);
        let mu_ntu = measured_mu(&pipe, 325.0, -ntu_n, lk_mm);
        let mu_paper = measured_mu(&pipe, 325.0, -1.08 * ntu_n, lk_mm);
        println!(
            "円形引張 {class:?} Lk={lk_mm} Ntu={:.9} N Mu(-Ntu)={mu_ntu:.9} kN·m Mu(-1.08Ntu)={mu_paper:.9} kN·m",
            axial.ntu
        );
    }

    let square = SectionShape::CftBox {
        height: 150.0,
        width: 150.0,
        thick: 4.5,
        horizontal_thick: 4.5,
        corner_r: Some(0.0),
    };
    for lk_mm in [599.999, 600.0, 600.001, 1799.999, 1800.0, 1800.001, 1200.0] {
        let mu = measured_mu(&square, 235.0, 800_000.0, lk_mm);
        println!("角形 N=800000 N Lk={lk_mm} mm Mu={mu:.9} kN·m");
    }
    let mu_pure_bending = measured_mu(&square, 235.0, 0.0, 1200.0);
    close(mu_pure_bending, 32.186_759_001_547_294, 1e-9);
    println!("角形 Lk=1200 mm N=0: Mu={mu_pure_bending:.9} kN·m");
    for (diameter_mm, thickness_mm, lk_mm) in [(600.0, 12.0, 2400.0), (400.0, 16.0, 7200.0)] {
        let shape = circle(diameter_mm, thickness_mm);
        let axial = cft_axial_ultimate(&circle_axial(diameter_mm, thickness_mm, 325.0, lk_mm));
        for n_n in [
            axial.ncu - 1.0,
            axial.ncu,
            axial.ncu + 1.0,
            1.05 * axial.ncu,
        ] {
            println!(
                "円形 {diameter_mm}×{thickness_mm} Lk={lk_mm} mm Ncu={:.9} N N={n_n:.9} N Mu={:.9} kN·m",
                axial.ncu,
                measured_mu(&shape, 325.0, n_n, lk_mm),
            );
        }
    }

    let mut model = model_input::cantilever(true, 100_000.0);
    model.sections[0].shape = Some(circle(400.0, 16.0));
    model.materials[0].fc = Some(30.0);
    model.materials[1].name = "SN490B".into();
    model.materials[1].fy = Some(325.0);
    model.steel_design_attrs = vec![model_input::direct_lengths(
        COLUMN,
        Some(7200.0),
        Some(7200.0),
    )];
    let rows = collect_cft_ultimate_checks(&model, &[(COLUMN, 1.05 * 7_342_847.61111)]).unwrap();
    let row = &rows[0];
    println!(
        "円形長柱一括検定 N={:.9} N Ncu={:.9} N 軸適合={} Mu={:?} N·mm",
        row.n_design, row.ncu, row.ok, row.mu_nm,
    );

    let forces = MemberForcesAt {
        pos: 0.0,
        n: -row.n_design,
        qy: 200_000.0,
        qz: 0.0,
        my: 0.0,
        mz: 0.0,
    };
    for method in [QdMethod::Qd1, QdMethod::Min, QdMethod::Qd2] {
        let ctx = DesignCtx {
            term: LoadTerm::Short,
            kind: MemberKind::Column,
            length: 3000.0,
            lk_y: Some(7200.0),
            lk_z: Some(7200.0),
            steel_material: Some(model.materials[1].clone()),
            seismic_qd: Some(SeismicQd {
                long_at: vec![(0.0, [0.0; 6])],
                n_factor: 1.5,
                n_mechanism: 1.0,
                q_simple: None,
                clear_length: 3000.0,
                method,
            }),
            ..Default::default()
        };
        let CheckOutcome::Checked(output) =
            CftDesign.check(&forces, &model.sections[0], &model.materials[0], &ctx)
        else {
            panic!("円形長柱の短期せん断が未算定");
        };
        let shear = output
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap();
        println!("円形長柱 {method:?} せん断検定比={:.9}", shear.ratio);
    }

    let mut model = model_input::cantilever(true, 100_000.0);
    model.steel_design_attrs = vec![model_input::direct_lengths(
        COLUMN,
        Some(1200.0),
        Some(1200.0),
    )];
    let solved =
        sepika_solver::statics::linear::linear_static_once(&model, sepika_core::ids::LoadCaseId(0))
            .unwrap();
    for fc in [23.999, 24.0] {
        model.materials[0].fc = Some(fc);
        let ultimate = collect_cft_ultimate_checks(&model, &[(COLUMN, 100_000.0)]).unwrap();
        let allowable = run_member_design_checks(
            &model,
            &solved.member_forces,
            &solved.panel_moments,
            &MemberDesignCheckOptions {
                term: LoadTerm::Long,
                ..Default::default()
            },
        )
        .unwrap();
        println!(
            "Fc={fc} N/mm² 終局軸適合={} Mu={:?} N·mm 一次={:?}",
            ultimate[0].ok,
            ultimate[0].mu_nm,
            allowable
                .member_checks
                .iter()
                .map(|(_, _, outcome)| {
                    match outcome {
                        CheckOutcome::Skipped { reason } => reason.as_str(),
                        CheckOutcome::Checked(_) => "算定",
                    }
                })
                .collect::<Vec<_>>(),
        );
    }
}
