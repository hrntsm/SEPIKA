#[path = "fixtures/src_h_steel_product_model.rs"]
mod src_model;

use std::collections::HashMap;

use sepika_core::ids::{ElemId, MaterialId};
use sepika_core::model::{MaterialCategory, Model};
use sepika_design_jp::{
    run_member_design_checks, CheckComponent, CheckKind, CheckOutcome, LoadTerm,
    MemberDesignCheckOptions, SrcColumnHeadLimit, SrcColumnRoute21RcShear, SrcColumnRoute21Shear,
    SrcColumnRoute21ShearAxis, SrcColumnSeismicSense,
};
use sepika_element::frame::beam::MemberForces;

const ID: ElemId = ElemId(0);
const STEEL_QY: f64 = 269_837_202.133_333_3;
const STEEL_QZ: f64 = 40_770_833.066_666_67;

fn column() -> Model {
    let mut model = src_model::model(true, "SN400B", 0.0, 0.0);
    let mut hoop = model.materials[1].clone();
    hoop.id = MaterialId(3);
    hoop.name = "SD295".into();
    hoop.fy = Some(295.0);
    model.materials.push(hoop);
    model.sections[0].shear_rebar_material = Some(MaterialId(3));
    model
}

fn bending(ultimate_n_mm: [f64; 2], head_limit: SrcColumnHeadLimit) -> SrcColumnRoute21RcShear {
    SrcColumnRoute21RcShear::Bending {
        ultimate_n_mm,
        head_limit,
    }
}

fn confirmed() -> SrcColumnRoute21Shear {
    SrcColumnRoute21Shear {
        clear_height_mm: 4_000.0,
        qy: Some(SrcColumnRoute21ShearAxis {
            sense: SrcColumnSeismicSense::Positive,
            steel_allowable_n_mm: [STEEL_QY; 2],
            rc: bending(
                [937_029_839.646_543_6, 590_432_617.424_321_4],
                SrcColumnHeadLimit::BothEndsYield,
            ),
            evidence: "正加力組合せの強軸単段・両端曲げ降伏の独立計算".into(),
        }),
        qz: Some(SrcColumnRoute21ShearAxis {
            sense: SrcColumnSeismicSense::Negative,
            steel_allowable_n_mm: [STEEL_QZ; 2],
            rc: bending(
                [753_887_231.892_659_7, 479_789_651.155_705_9],
                SrcColumnHeadLimit::BothEndsYield,
            ),
            evidence: "負加力組合せの弱軸多段・両端曲げ降伏の独立計算".into(),
        }),
    }
}

fn response(head_n: f64, base_n: f64, qy_n: f64) -> [(ElemId, MemberForces); 1] {
    [(
        ID,
        MemberForces {
            at: [(0.0, base_n), (0.5, (base_n + head_n) / 2.0), (1.0, head_n)]
                .map(|(pos, n)| (pos, [n, qy_n, -800_000.0, 0.0, 0.0, 0.0]))
                .to_vec(),
        },
    )]
}

fn report(
    model: &Model,
    response: &[(ElemId, MemberForces)],
    inputs: Option<&HashMap<ElemId, SrcColumnRoute21Shear>>,
    term: LoadTerm,
) -> Vec<CheckOutcome> {
    run_member_design_checks(
        model,
        response,
        &[],
        &MemberDesignCheckOptions {
            term,
            src_column_route21_shear: inputs,
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .map(|(_, _, outcome)| outcome)
    .collect()
}

fn shear(outcome: &CheckOutcome) -> &CheckComponent {
    let CheckOutcome::Checked(result) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(result.basis.contains("ルート2-1、付1.4-6"), "{result:?}");
    result
        .components
        .iter()
        .find(|component| component.kind == CheckKind::Shear)
        .unwrap()
}

fn quantity(row: &CheckComponent, direction: &str, label: &str) -> f64 {
    detail_quantity(&row.detail, direction, label)
}

fn detail_quantity(detail: &str, direction: &str, label: &str) -> f64 {
    detail
        .split_once(direction)
        .unwrap()
        .1
        .split_once(label)
        .unwrap()
        .1
        .split([' ', ',', '、'])
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
}

#[test]
fn common_runner_uses_confirmed_end_moments_for_single_and_multiple_rows() {
    let model = column();
    // #953の600 mm角独立値。圧縮2端と引張2端で単段式・多段式を照合する。
    let mut inputs = HashMap::from([(ID, confirmed())]);
    let compression = response(-3_300_000.0, -1_000_000.0, 800_000.0);
    let rows = report(&model, &compression, Some(&inputs), LoadTerm::Short);
    assert_eq!(rows.len(), 3, "{rows:?}");
    for row in &rows {
        let shear = shear(row);
        close(quantity(shear, "Qy方向", "rQD="), 381_865.614_267_716_3);
        close(quantity(shear, "Qz方向", "rQD="), 308_419.220_762_091_4);
        close(quantity(shear, "Qy方向", "sQD="), 134_918.601_066_666_66);
        close(quantity(shear, "Qz方向", "sQD="), 20_385.416_533_333_337);
        close(shear.ratio, 0.913_634_597_685_867_3);
        assert!(shear.detail.contains("採用方法=曲げ方式（Qd1、付1.4-1）"));
        assert!(shear
            .detail
            .contains("柱頭条件=両端曲げ降伏、局所加力=負、根拠=負加力組合せ"));
        assert!(matches!(row, CheckOutcome::Checked(result) if result.ok()));
    }

    let [qy, qz] = {
        let entry = inputs.get_mut(&ID).unwrap();
        [entry.qy.as_mut().unwrap(), entry.qz.as_mut().unwrap()]
    };
    qy.rc = bending(
        [32_515_483.964_654_326; 2],
        SrcColumnHeadLimit::BothEndsYield,
    );
    qz.rc = bending(
        [21_451_187.337_792_844; 2],
        SrcColumnHeadLimit::BothEndsYield,
    );
    let tension = response(1_219_330.648_674_539, 1_219_330.648_674_539, 800_000.0);
    let rows = report(&model, &tension, Some(&inputs), LoadTerm::Short);
    close(
        quantity(shear(&rows[1]), "Qy方向", "rQD="),
        16_257.741_982_327_163,
    );
    close(
        quantity(shear(&rows[1]), "Qz方向", "rQD="),
        10_725.593_668_896_421,
    );
}

#[test]
fn common_runner_uses_the_beam_head_limit_and_shows_the_supported_story_condition() {
    let model = column();
    let mut input = confirmed();
    let forces = response(-3_300_000.0, -3_300_000.0, 800_000.0);
    for (top_story, story, expected_n) in [
        (false, "一般階", 279_298.159_378_302_6),
        (true, "最上階", 391_798.159_378_302_6),
    ] {
        input.qy.as_mut().unwrap().rc = bending(
            [937_029_839.646_543_6; 2],
            SrcColumnHeadLimit::ConnectedBeams {
                sum_n_mm: 900_000_000.0,
                top_story,
            },
        );
        let inputs = HashMap::from([(ID, input.clone())]);
        let rows = report(&model, &forces, Some(&inputs), LoadTerm::Short);
        let shear = shear(&rows[1]);
        close(quantity(shear, "Qy方向", "rQD="), expected_n);
        assert!(shear
            .detail
            .contains(&format!("柱頭条件=接続梁（{story}）")));
    }
}

#[test]
fn alternative_uses_only_confirmed_primary_earthquake_shear_and_the_shared_steel_demand() {
    let model = column();
    let mut input = confirmed();
    let axis = input.qy.as_mut().unwrap();
    axis.rc = SrcColumnRoute21RcShear::PrimaryEarthquake { shear_n: 400_000.0 };
    axis.evidence = "一次設計用地震力のみの強軸せん断 400 kN の独立計算".into();
    let inputs = HashMap::from([(ID, input)]);
    for combination_shear_n in [800_000.0, -1_600_000.0] {
        let response = response(-3_300_000.0, -1_000_000.0, combination_shear_n);
        let rows = report(&model, &response, Some(&inputs), LoadTerm::Short);
        let shear = shear(&rows[1]);
        close(quantity(shear, "Qy方向", "sQD="), 134_918.601_066_666_66);
        close(quantity(shear, "Qy方向", "rQD="), 530_162.797_866_666_7);
        close(quantity(shear, "Qz方向", "rQD="), 308_419.220_762_091_4);
        close(shear.ratio, 1.268_443_809_652_216_4);
        assert!(shear
            .detail
            .contains("採用方法=代替式（Qd2、付1.4-2）、一次設計用地震せん断力Q=4e5 N"));
        assert!(shear.detail.contains("採用方法=曲げ方式（Qd1、付1.4-1）"));
        assert!(matches!(&rows[1], CheckOutcome::Checked(result) if !result.ok()));
    }
}

#[test]
fn alternative_keeps_known_rc_shear_failure_with_invalid_main_rebar_material() {
    let mut input = confirmed();
    let axis = input.qy.as_mut().unwrap();
    axis.rc = SrcColumnRoute21RcShear::PrimaryEarthquake { shear_n: 400_000.0 };
    axis.evidence = "一次設計用地震力のみの強軸せん断 400 kN の独立計算".into();
    let inputs = HashMap::from([(ID, input)]);
    let response = response(-3_300_000.0, -1_000_000.0, 800_000.0);
    for (assignment, expected_reason) in [
        (
            Some((MaterialCategory::Steel, "SD345")),
            "主筋材料「SD345」の材料区分は「鋼材」",
        ),
        (
            Some((MaterialCategory::Rebar, "不明材")),
            "主筋材料「不明材」はSRC規準の異形主筋に対応する材種ではありません",
        ),
        (None, "主筋の材料が未割当"),
    ] {
        let mut model = column();
        if let Some((category, name)) = assignment {
            model.materials[1].category = category;
            model.materials[1].name = name.into();
        } else {
            model.sections[0].rebar_material = None;
            for term in [LoadTerm::Short, LoadTerm::Long] {
                let ordinary_rows = report(&model, &response, None, term);
                assert_eq!(ordinary_rows.len(), 3, "{ordinary_rows:?}");
                assert!(ordinary_rows.iter().all(|outcome| matches!(
                    outcome, CheckOutcome::Skipped { reason } if reason.contains(expected_reason)
                )), "{ordinary_rows:?}");
            }
        }
        let rows = report(&model, &response, Some(&inputs), LoadTerm::Short);
        assert_eq!(rows.len(), 6, "{rows:?}");
        for [checked, skipped] in rows.as_chunks::<2>().0 {
            let shear = shear(checked);
            close(shear.ratio, 1.268_443_809_652_216_4);
            close(quantity(shear, "Qy方向", "rQD="), 530_162.797_866_666_7);
            let weak = shear.detail.split_once("Qz方向").unwrap().1;
            assert!(weak.contains("RC rQD=未算定、指定方法=曲げ方式"));
            assert!(
                matches!(skipped, CheckOutcome::Skipped { reason }
                if reason.contains(expected_reason)),
                "{rows:?}"
            );
        }
    }
}

#[test]
fn alternative_at_q_equal_to_qs_keeps_the_confirmed_zero_rc_demand() {
    let model = column();
    let mut input = confirmed();
    let axis = input.qy.as_mut().unwrap();
    axis.steel_allowable_n_mm = [280_000_000.0, 250_000_000.0];
    axis.rc = SrcColumnRoute21RcShear::PrimaryEarthquake { shear_n: 132_500.0 };
    let inputs = HashMap::from([(ID, input)]);
    let response = response(0.0, 0.0, 900_000.0);
    let rows = report(&model, &response, Some(&inputs), LoadTerm::Short);
    let shear = shear(&rows[1]);
    assert_eq!(quantity(shear, "Qy方向", "sQD="), 132_500.0);
    assert_eq!(quantity(shear, "Qy方向", "rQD="), 0.0);
    assert_eq!(quantity(shear, "Qy方向", "rQD/rQA="), 0.0);
    assert!(shear.detail.contains("採用方法=代替式（Qd2、付1.4-2）"));
}

#[test]
fn available_direction_is_reported_without_a_pass_when_the_other_direction_is_missing() {
    let model = column();
    let mut input = confirmed();
    input.qz = None;
    let inputs = HashMap::from([(ID, input)]);
    let rows = report(
        &model,
        &response(-3_300_000.0, -1_000_000.0, 800_000.0),
        Some(&inputs),
        LoadTerm::Short,
    );
    assert_eq!(rows.len(), 3, "{rows:?}");
    for row in rows {
        let CheckOutcome::Skipped { reason } = row else {
            panic!("{row:?}");
        };
        for needle in [
            "Qz方向のルート2-1の確認済み入力がありません",
            "Qy方向 鉄骨 sQD=",
            "sQA=",
            "sQD/sQA=",
            "RC rQD=",
            "rQA=",
            "rQD/rQA=",
            "採用方法=曲げ方式（Qd1",
            "根拠=正加力組合せの強軸単段・両端曲げ降伏の独立計算",
        ] {
            assert!(reason.contains(needle), "{needle}: {reason}");
        }
        close(
            detail_quantity(&reason, "Qy方向", "sQD="),
            134_918.601_066_666_66,
        );
        close(
            detail_quantity(&reason, "Qy方向", "rQD="),
            381_865.614_267_716_3,
        );
    }
}

#[test]
fn alternative_does_not_hide_known_steel_failure_if_the_rc_input_cannot_be_used() {
    let model = column();
    let response = response(0.0, 0.0, 1_600_000.0);
    let mut input = confirmed();
    input.clear_height_mm = 1_000.0;
    input.qz = None;
    for (q_n, expected) in [
        (1.0, "Q<Qs"),
        (
            f64::NAN,
            "一次設計用地震力による柱せん断力Qには有限な非負値",
        ),
        (f64::MAX, "RC設計用せん断力が非有限"),
    ] {
        input.qy.as_mut().unwrap().rc = SrcColumnRoute21RcShear::PrimaryEarthquake { shear_n: q_n };
        let inputs = HashMap::from([(ID, input.clone())]);
        let rows = report(&model, &response, Some(&inputs), LoadTerm::Short);
        assert_eq!(rows.len(), 6, "{rows:?}");
        assert!(shear(&rows[0]).ratio > 1.0, "{rows:?}");
        assert!(shear(&rows[0]).detail.contains("RC rQD=未算定"));
        assert!(
            matches!(&rows[1], CheckOutcome::Skipped { reason }
                if reason.contains(expected) && reason.contains("Qz方向のルート2-1の確認済み入力")),
            "{rows:?}",
        );
    }
}

#[test]
fn bending_keeps_known_rc_failure_when_an_independent_steel_end_is_unavailable() {
    let model = column();
    let forces = response(-3_300_000.0, -3_300_000.0, 800_000.0);
    let mut input = confirmed();

    for (steel_end, head_limit) in [
        (0, SrcColumnHeadLimit::BothEndsYield),
        (
            1,
            SrcColumnHeadLimit::ConnectedBeams {
                sum_n_mm: 3_000_000_000.0,
                top_story: false,
            },
        ),
    ] {
        let qy = input.qy.as_mut().unwrap();
        qy.steel_allowable_n_mm = [STEEL_QY; 2];
        qy.steel_allowable_n_mm[steel_end] = f64::NAN;
        qy.rc = bending([937_029_839.646_543_6; 2], head_limit);
        let inputs = HashMap::from([(ID, input.clone())]);
        let rows = report(&model, &forces, Some(&inputs), LoadTerm::Short);
        let shear = shear(&rows[0]);
        assert!(shear.ratio > 1.0);
        assert!(shear.detail.contains("Qy方向 鉄骨 sQD=未算定"));
        close(quantity(shear, "Qy方向", "rQD="), 468_514.919_823_271_8);
        assert!(
            matches!(&rows[1], CheckOutcome::Skipped { reason } if reason.contains("鉄骨部の柱頭・柱脚")),
            "{rows:?}"
        );
    }
}

#[test]
fn requested_route21_never_uses_ordinary_short_shear_for_missing_or_invalid_inputs() {
    let model = column();
    let response = response(-40_000_000.0, -40_000_000.0, 0.0);
    let mut bad_length = confirmed();
    bad_length.clear_height_mm = f64::NAN;
    let mut no_evidence = confirmed();
    no_evidence.qy.as_mut().unwrap().evidence = "  ".into();
    for (inputs, reason) in [
        (HashMap::new(), "確認済み入力が対象SRC柱にありません"),
        (HashMap::from([(ID, bad_length)]), "有限で正の内法高さ"),
        (HashMap::from([(ID, no_evidence)]), "確認根拠がありません"),
    ] {
        let rows = report(&model, &response, Some(&inputs), LoadTerm::Short);
        assert!(
            matches!(&rows[..2], [CheckOutcome::Checked(known), CheckOutcome::Skipped { reason: actual }]
                if !known.ok() && known.components.iter().all(|c| c.kind != CheckKind::Shear)
                   && actual.contains(reason)),
            "{rows:?}",
        );
    }

    let input = HashMap::from([(ID, confirmed())]);
    let rows = report(&model, &[], Some(&input), LoadTerm::Short);
    assert!(
        matches!(rows.as_slice(), [CheckOutcome::Skipped { reason }] if reason.contains("解析断面力がありません")),
        "{rows:?}"
    );
}

#[test]
fn missing_or_duplicate_response_does_not_appear_as_a_complete_check() {
    let model = column();
    let input = HashMap::from([(ID, confirmed())]);
    let mut response = response(-3_300_000.0, -1_000_000.0, 800_000.0);
    let duplicates = [response[0].clone(), response[0].clone()];
    let rows = report(&model, &duplicates, Some(&input), LoadTerm::Short);
    assert!(
        matches!(rows.as_slice(), [CheckOutcome::Skipped { reason }] if reason.contains("断面力が重複")),
        "{rows:?}"
    );

    response[0].1.at.remove(1);
    let rows = report(&model, &response, Some(&input), LoadTerm::Short);
    assert_eq!(rows.len(), 3, "{rows:?}");
    assert!(rows.iter().any(|row| matches!(row, CheckOutcome::Skipped { reason } if reason.contains("検定位置の解析断面力がありません"))), "{rows:?}");
}

#[test]
fn repeated_response_at_one_design_station_retains_a_failure_at_another_station() {
    let model = column();
    let input = HashMap::from([(ID, confirmed())]);
    for repeated_pos in [0.5, 0.5 + 0.5e-6] {
        let mut response = response(-3_300_000.0, -8_000_000.0, 800_000.0);
        response[0].1.at.push((
            repeated_pos,
            [-1_000_000.0, 800_000.0, -800_000.0, 0.0, 0.0, 0.0],
        ));
        let rows = run_member_design_checks(
            &model,
            &response,
            &[],
            &MemberDesignCheckOptions {
                term: LoadTerm::Short,
                src_column_route21_shear: Some(&input),
                ..Default::default()
            },
        )
        .unwrap()
        .member_checks;
        assert_eq!(rows.len(), 3, "{rows:?}");
        let duplicate: Vec<_> = rows
            .iter()
            .filter(|(_, pos, _)| (*pos - 0.5).abs() < 1e-6)
            .collect();
        assert!(
            matches!(duplicate.as_slice(), [(_, _, CheckOutcome::Skipped { reason })]
            if reason.contains("検定位置で解析断面力が重複")),
            "{rows:?}"
        );
        assert!(rows.iter().any(|(_, pos, outcome)| *pos == 0.0 && matches!(
            outcome, CheckOutcome::Checked(result) if !result.ok() &&
                result.components.iter().any(|c| c.kind == CheckKind::AxialBending && c.ratio > 1.0)
        )), "{rows:?}");
    }
}

#[test]
fn short_route21_keeps_shear_failure_with_unknown_evaluation_axial_force() {
    let mut response = response(0.0, 0.0, 500_000.0);
    response[0].1.at[1].1[0] = f64::NAN;
    let mut input = confirmed();
    input.qy.as_mut().unwrap().rc = bending(
        [937_029_839.646_543_6; 2],
        SrcColumnHeadLimit::BothEndsYield,
    );
    let rows = report(
        &column(),
        &response,
        Some(&HashMap::from([(ID, input)])),
        LoadTerm::Short,
    );
    assert_eq!(rows.len(), 4, "{rows:?}");
    assert!(shear(&rows[1]).ratio > 1.0);
    assert!(
        matches!(&rows[2], CheckOutcome::Skipped { reason } if reason.contains("軸力Nが非有限"))
    );
}

#[test]
fn route21_input_is_ignored_for_long_term_and_src_girders() {
    let inputs = Some(&HashMap::from([(ID, confirmed())]));
    let girder = src_model::model(false, "SN400B", 0.0, 0.0);
    let response = response(0.0, 0.0, 90_000.0);
    for (model, term) in [(column(), LoadTerm::Long), (girder, LoadTerm::Short)] {
        assert_eq!(
            report(&model, &response, inputs, term),
            report(&model, &response, None, term)
        );
    }
}
