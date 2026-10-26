use sepika_core::section_shape::SectionShape;
use sepika_design_jp::ultimate::{
    cft::{cft_axial_ultimate, CftAxialInput, CftColumnClass},
    cft_mu_nm,
};

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "actual={actual}, expected={expected}"
    );
}

fn square(d: f64, t: f64, r: Option<f64>) -> SectionShape {
    SectionShape::CftBox {
        height: d,
        width: d,
        thick: t,
        horizontal_thick: t,
        corner_r: r,
    }
}

fn main() {
    let shape = square(400.0, 16.0, Some(0.0));
    for weak in [false, true] {
        close(
            cft_mu_nm(&shape, 30.0, 325.0, 2031360.0, 1600.0, weak).unwrap(),
            1337707520.0,
        );
        close(
            cft_mu_nm(&shape, 30.0, 325.0, -7987200.0, 1600.0, weak).unwrap(),
            0.0,
        );
        close(
            cft_mu_nm(&shape, 30.0, 325.0, 12049920.0, 1600.0, weak).unwrap(),
            0.0,
        );
    }
    let pipe = SectionShape::CftPipe {
        outer_dia: 400.0,
        thick: 16.0,
    };
    close(
        cft_mu_nm(&pipe, 30.0, 325.0, 2171770.4350560093, 1600.0, false).unwrap(),
        971406336.0,
    );
    let small = square(150.0, 4.5, Some(0.0));
    for lk in [599.999, 600.0, 600.001, 1799.999, 1800.0, 1800.001] {
        let mu = cft_mu_nm(&small, 30.0, 235.0, 800000.0, lk, false).unwrap();
        println!("分類境界の観測（独立期待値照合に含めない）: Lk={lk}, Mu={mu}");
    }
    for weak in [false, true] {
        close(
            cft_mu_nm(&small, 30.0, 235.0, 774319.9534426274, 2700.0, weak).unwrap(),
            12313672.630184587,
        );
    }
    for (fc, lk) in [(58.84, 2700.0), (30.0, 4500.001)] {
        assert!(cft_mu_nm(&small, fc, 235.0, 1e6, lk, false)
            .unwrap_err()
            .contains("適用域外"));
    }
    assert!(cft_mu_nm(
        &square(150.0, 4.5, Some(1.0)),
        30.0,
        235.0,
        800000.0,
        2700.0,
        false
    )
    .unwrap_err()
    .contains("未対応"));
    assert!(cft_mu_nm(
        &square(150.0, 4.5, None),
        30.0,
        235.0,
        800000.0,
        2700.0,
        false
    )
    .is_err());
    for (lk, class) in [
        (1600.0, CftColumnClass::Short),
        (1600.001, CftColumnClass::Medium),
        (4800.0, CftColumnClass::Medium),
        (4800.001, CftColumnClass::Long),
    ] {
        let r = cft_axial_ultimate(&CftAxialInput {
            circular: false,
            d_section: 400.0,
            c_area: 135424.0,
            s_area: 24576.0,
            c_inertia: 1528310442.6666667,
            s_inertia: 605022890.6666666,
            fc: 30.0,
            fy: 325.0,
            s_young: 205000.0,
            lk,
        });
        assert_eq!(r.class, class);
        close(r.ntu, 7987200.0);
        if lk == 1600.0 {
            close(r.ncu, 12049920.0);
        }
    }
    println!("Issue888: 固定独立期待値・境界・拒否条件の公開入口Verification成功");
}
