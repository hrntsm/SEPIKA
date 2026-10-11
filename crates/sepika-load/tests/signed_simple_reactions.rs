use sepika_core::model::MemberLoadKind;
use sepika_load::floor::{simple_beam_extremes, simple_beam_moment_at, simple_reactions};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected} (tol {tolerance})"
    );
}

// Simpson則でw(s)を直接評価する。製品のW/S閉形式とは独立した積分。
fn integrate(f: impl Fn(f64) -> f64, a: f64, b: f64) -> f64 {
    let n = 100;
    let h = (b - a) / n as f64;
    let mut sum = f(a) + f(b);
    for k in 1..n {
        sum += if k % 2 == 0 { 2.0 } else { 4.0 } * f(a + k as f64 * h);
    }
    sum * h / 3.0
}

#[test]
fn signed_reactions_and_moments_match_independent_equilibrium() {
    let l = 6000.0;
    for (a, b, w1, w2, expected_i, expected_j) in [
        (0.0, l, 1.0, -1.0, 1000.0, -1000.0),
        (1000.0, 4000.0, 1.0, -1.0, 250.0, -250.0),
        (0.0, l, 1.0, -1.0 + 1e-10, 1000.0000001, -999.9999998),
        (0.0, l, 1.0, -0.9, 1100.0, -800.0),
        (0.0, l, 1.0, 1.0, 3000.0, 3000.0),
        (0.0, l, 0.0, 1.0, 1000.0, 2000.0),
    ] {
        let w = |s: f64| w1 + (w2 - w1) * (s - a) / (b - a);
        let total_n = integrate(w, a, b);
        let first_nmm = integrate(|s| s * w(s), a, b);
        let kind = MemberLoadKind::Distributed { a, b, w1, w2 };
        let (ri, rj) = simple_reactions(&kind, l);
        close(ri, expected_i, 1e-8); // N
        close(rj, expected_j, 1e-8);
        close(ri + rj, total_n, 1e-8);
        close(rj * l, first_nmm, 1e-5); // N mm
        for x in [0.0, 1500.0, 3000.0, 4500.0, l] {
            let end = x.min(b).max(a);
            let expected = (total_n - first_nmm / l) * x - integrate(|s| w(s) * (x - s), a, end);
            close(
                simple_beam_moment_at(std::slice::from_ref(&kind), l, x),
                expected,
                1e-5,
            );
        }
        close(
            simple_beam_moment_at(std::slice::from_ref(&kind), l, l),
            0.0,
            1e-5,
        );
        // 以下は独立値ではなく、符号・i/j対称性の自己比較。
        let reversed = MemberLoadKind::Distributed {
            a: l - b,
            b: l - a,
            w1: w2,
            w2: w1,
        };
        let negative = MemberLoadKind::Distributed {
            a,
            b,
            w1: -w1,
            w2: -w2,
        };
        let (rev_i, rev_j) = simple_reactions(&reversed, l);
        close(rev_i, rj, 1e-8);
        close(rev_j, ri, 1e-8);
        let (neg_i, neg_j) = simple_reactions(&negative, l);
        close(neg_i, -ri, 1e-8);
        close(neg_j, -rj, 1e-8);
        for x in [1500.0, 4500.0] {
            let m = simple_beam_moment_at(std::slice::from_ref(&kind), l, x);
            close(
                simple_beam_moment_at(std::slice::from_ref(&negative), l, x),
                -m,
                1e-5,
            );
            close(
                simple_beam_moment_at(std::slice::from_ref(&reversed), l, l - x),
                m,
                1e-5,
            );
        }
    }
    let loads = [MemberLoadKind::Distributed {
        a: 0.0,
        b: l,
        w1: 1.0,
        w2: -1.0,
    }];
    for (x, m) in [
        (0.0, 0.0),
        (1500.0, 562500.0),
        (3000.0, 0.0),
        (4500.0, -562500.0),
        (l, 0.0),
    ] {
        close(simple_beam_moment_at(&loads, l, x), m, 1e-6);
    }
    let forces = simple_beam_extremes(&loads, l, 200000.0, 1e8);
    close(forces.q_max, 1000.0, 1e-8);
    assert!(forces.m_max >= 562500.0 && forces.m_max < 580000.0);
    // EI*y''=1000x-x²/2+x³/18000 の独立積分。既存80分割近似の誤差を許容。
    let deflection = |x: f64| {
        (1000.0 * x.powi(3) / 6.0 - x.powi(4) / 24.0 + x.powi(5) / 360000.0 - 600000000.0 * x)
            / 2e13
    };
    let expected_mm = (0..=10000)
        .map(|i| deflection(l * i as f64 / 10000.0).abs())
        .fold(0.0_f64, f64::max);
    close(forces.deflection, expected_mm, 4e-5);
}

#[test]
fn signed_point_superposition_and_floor_range_adapter() {
    let l = 6000.0;
    for (a, expected) in [
        (0.0, (1000.0, 0.0)),
        (1500.0, (750.0, 250.0)),
        (4500.0, (250.0, 750.0)),
        (l, (0.0, 1000.0)),
        (-100.0, (1000.0, 0.0)),
        (7000.0, (0.0, 1000.0)),
    ] {
        assert_eq!(
            simple_reactions(&MemberLoadKind::Point { a, p: 1000.0 }, l),
            expected
        );
    }
    let loads = [
        MemberLoadKind::Point {
            a: 1500.0,
            p: 1000.0,
        },
        MemberLoadKind::Point {
            a: 1500.0,
            p: -1000.0,
        },
    ];
    for x in [0.0, 1500.0, 3000.0, l] {
        close(simple_beam_moment_at(&loads, l, x), 0.0, 1e-8);
    }
    assert_eq!(
        simple_reactions(&loads[0], l).0 + simple_reactions(&loads[1], l).0,
        0.0
    );
    for (a, b) in [(1.0, 1.0), (2.0, 1.0), (0.0, 1e-10)] {
        assert_eq!(
            simple_reactions(
                &MemberLoadKind::Distributed {
                    a,
                    b,
                    w1: 1.0,
                    w2: 1.0
                },
                l
            ),
            (0.0, 0.0)
        );
    }
    assert_eq!(
        simple_reactions(
            &MemberLoadKind::Distributed {
                a: 0.0,
                b: 2.0,
                w1: 1.0,
                w2: 1.0
            },
            1e-10
        ),
        (1.0, 1.0)
    );
    assert_eq!(
        simple_reactions(&MemberLoadKind::Point { a: 0.0, p: 10.0 }, 0.0),
        (5.0, 5.0)
    );
    // 原区間を保持する入力adapter。支間外の数学保証を意味しない。
    assert_eq!(
        simple_reactions(
            &MemberLoadKind::Distributed {
                a: -1000.0,
                b: 7000.0,
                w1: 1.0,
                w2: 1.0
            },
            l
        ),
        (4000.0, 4000.0)
    );
}

#[test]
#[should_panic]
fn negative_span_point_keeps_existing_floor_adapter_panic() {
    simple_reactions(&MemberLoadKind::Point { a: 0.0, p: 1000.0 }, -1.0);
}
