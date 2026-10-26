use super::*;

#[test]
fn rc_beam_shape_has_section_properties() {
    let shape = SectionShape::RcBeamRect {
        b: 300.0,
        d: 500.0,
        rebar: RcBeamRebar {
            main_dia: 22.0,
            top: vec![3],
            bottom: vec![4],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 150.0,
                legs: 2,
            },
        },
    };
    assert_eq!(shape.calc_area(), 150_000.0);
    let section = shape.to_section(crate::ids::SectionId(0), "RCB".into());
    assert!(section.area > 0.0);
    assert_eq!(section.frame_use, None);
}

#[test]
fn rc_column_shapes_are_concrete_like() {
    let shape = SectionShape::RcColumnCircle {
        d: 600.0,
        rebar: RcCircleColumnRebar {
            main_dia: 19.0,
            count: 12,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 150.0,
            },
        },
    };
    assert!(shape.is_concrete_like());
    assert!(shape.validate_rebar().is_ok());
}

#[test]
fn cft_initial_shear_rigidity_distinguishes_directional_plates_and_published_coefficients() {
    let gs_mpa = 205_000.0 / 2.6;
    let gc_mpa = 33_500.0 / 2.4;
    let ks = 20.0 * 1.3 / (48.0 + 39.0 * 0.3);
    let kc = 10.0 * 1.2 / (12.0 + 11.0 * 0.2);
    assert!((ks / 0.4355108877721943_f64 - 1.0).abs() < 1e-10);
    assert!((kc / 0.8450704225352113_f64 - 1.0).abs() < 1e-10);
    // Fc60・γ24 では Ec=33500。比較値は全材料面積に対する各文献の係数で独立算定。
    for (
        height,
        width,
        thick,
        radius,
        steel_mm2,
        core_mm2,
        current_n,
        reference1979_n,
        reference2003_n,
    ) in [
        (
            100.0,
            100.0,
            3.0,
            0.0,
            1164.0,
            8836.0,
            [147249091.8803419, 147249091.8803419],
            148668322.64957267,
            144197314.03496355,
        ),
        (
            100.0,
            100.0,
            3.0,
            3.0,
            1156.2743338823075,
            8836.0,
            [147249091.8803419, 147249091.8803419],
            148363753.11993286,
            143932027.34247997,
        ),
        (
            100.0,
            100.0,
            3.0,
            7.5,
            1133.0973355292335,
            8818.617251235193,
            [147046896.71241793, 147046896.71241793],
            147247849.36308962,
            142931124.27783886,
        ),
        (
            400.0,
            300.0,
            12.0,
            0.0,
            16224.0,
            103776.0,
            [1729393589.7435899, 1918624358.974359],
            1846716666.6666667,
            1781223837.497346,
        ),
        (
            400.0,
            300.0,
            12.0,
            12.0,
            16100.389342116934,
            103776.0,
            [1729393589.7435899, 1918624358.974359],
            1841843554.1924305,
            1776979250.4176092,
        ),
        (
            400.0,
            300.0,
            12.0,
            30.0,
            15729.557368467722,
            103497.87601976309,
            [1726158467.0568063, 1915389236.2875755],
            1823989094.0829377,
            1760964801.3833504,
        ),
    ] {
        for rotated in [false, true] {
            let (height, width) = if rotated {
                (width, height)
            } else {
                (height, width)
            };
            let shape = SectionShape::CftBox {
                height,
                width,
                thick,
                horizontal_thick: thick,
                corner_r: Some(radius),
            };
            let props = shape
                .try_cft_equivalent_props(
                    205_000.0,
                    0.3,
                    205_000.0 / (2.0 * (1.0 + 0.3)),
                    60.0,
                    24.0,
                    shape.try_calc_j().unwrap(),
                )
                .unwrap()
                .unwrap();
            let expected = if rotated {
                [current_n[1], current_n[0]]
            } else {
                current_n
            };
            for (actual, expected) in [props.as_y, props.as_z].into_iter().zip(expected) {
                assert!((gs_mpa * actual / expected - 1.0).abs() < 1e-10);
            }
            assert!((shape.try_calc_area().unwrap() / steel_mm2 - 1.0).abs() < 1e-10);
            assert!(
                (shape.try_cft_core_props().unwrap().unwrap().area / core_mm2 - 1.0).abs() < 1e-10
            );
        }
        assert!(
            ((gs_mpa * steel_mm2 / 2.0 + gc_mpa * core_mm2 / 1.2) / reference1979_n - 1.0).abs()
                < 1e-10
        );
        assert!(
            ((gs_mpa * ks * steel_mm2 + gc_mpa * kc * core_mm2) / reference2003_n - 1.0).abs()
                < 1e-10
        );
        assert!(current_n[0] < reference1979_n);
        if height == width {
            assert!(current_n[0] > reference2003_n);
        } else {
            assert!(current_n[0] < reference2003_n);
            assert!(current_n[1] > reference1979_n);
            assert!(current_n[1] > reference2003_n);
        }
    }
}
