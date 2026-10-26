use sepika_design_jp::secondary::holding_capacity::{
    check_holding_capacity_at_state, qud_by_story,
};

#[test]
fn source880_qun_product_and_inclusive_boundary() {
    for (ds, fes, qud, expected) in [
        (0.25, 1.0, 1_000_000.0, 250_000.0),
        (0.35, 1.2, 800_000.0, 336_000.0),
        (0.4, 1.5, 500_000.0, 300_000.0),
    ] {
        for (qu, ok) in [
            (expected - 1.0, false),
            (expected, true),
            (expected + 1.0, true),
        ] {
            let result = check_holding_capacity_at_state(
                &[qu],
                &[15.0],
                &[qud],
                &[ds],
                &[fes],
                &[1.0],
                &[0.0],
                &[3000.0],
                vec![],
            )
            .unwrap();
            let story = &result.stories[0];
            assert!((story.qun - expected).abs() < 1e-8);
            assert_eq!(story.ok, ok);
            println!(
                "Ds={ds} Fes={fes} Qud_N={qud} Qu_N={qu} Qun_N={} ok={}",
                story.qun, story.ok
            );
        }
    }
}

#[test]
fn source880_single_story_c0_one_force() {
    let qud = qud_by_story(&[1_000_000.0], 0.8, 0.9, 0.3);
    assert!((qud[0] - 720_000.0).abs() < 1e-8);
    println!(
        "single_story W_N=1000000 Z=0.8 Rt=0.9 C0=1 Qud_N={}",
        qud[0]
    );
}
