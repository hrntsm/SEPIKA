#[path = "fixtures/high_density_steel.rs"]
mod fixture;

use sepika_core::{ids::*, model::*};
use sepika_job::{prepare::prepare_model, prepare_model_for_analysis, AnalysisSettings};

fn load(kind: MemberLoadKind, dir: [f64; 3], source: LoadSource) -> MemberLoad {
    let mut load = MemberLoad::manual(ElemId(0), dir, kind);
    load.name = "保持する部材荷重".into();
    load.source = source;
    load
}

#[test]
fn invalid_direction_rejects_all_cases_before_manual_auto_or_weight_mutation() {
    for kind in [
        MemberLoadKind::Point {
            a: 1000.0,
            p: 100.0,
        },
        MemberLoadKind::Point { a: 1000.0, p: 0.0 },
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 2000.0,
            w1: 7.0,
            w2: 7.0,
        },
        MemberLoadKind::Distributed {
            a: 0.0,
            b: 2000.0,
            w1: 0.0,
            w2: 0.0,
        },
    ] {
        for dir in [
            [0.0; 3],
            [1e-13, 0.0, 0.0],
            [1e-300, 0.0, 0.0],
            [1e300, 0.0, 0.0],
            [f64::from_bits(1e-12_f64.to_bits() - 1), 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [0.0, f64::INFINITY, 0.0],
            [0.0, 0.0, f64::NEG_INFINITY],
        ] {
            for source in [
                LoadSource::Manual,
                LoadSource::Auto,
                LoadSource::SlabTip,
                LoadSource::WallSelfWeight,
            ] {
                let mut model = fixture::steel_frame();
                model.load_cases.push(LoadCase {
                    id: LoadCaseId(9),
                    name: "非選択ケース".into(),
                    nodal: vec![],
                    member: vec![load(kind.clone(), dir, source)],
                    kind: LoadCaseKind::Other,
                });
                let before = format!("{model:?}");
                for initialize in [false, true] {
                    let error =
                        prepare_model(&mut model, &AnalysisSettings::default(), None, initialize)
                            .err()
                            .expect("方向不備を準備前に拒否する")
                            .to_string();
                    for token in [
                        "荷重ケース 9",
                        "非選択ケース",
                        "member[0]",
                        "保持する部材荷重",
                        "部材 0",
                        "作用方向",
                    ] {
                        assert!(error.contains(token), "{error}");
                    }
                    assert_eq!(format!("{model:?}"), before);
                }
            }
        }
    }
}

#[test]
fn valid_nonunit_zero_and_opposing_manual_loads_survive_preparation() {
    for dir in [
        [0.0, 0.0, -2.0],
        [0.0, 3.0, -4.0],
        [1e-10, 0.0, -2.0],
        [1e-12, 0.0, 0.0],
        [f64::from_bits(1e-12_f64.to_bits() + 1), 0.0, 0.0],
        [0.0, 2.0, 0.0],
        [2.0, 0.0, 0.0],
    ] {
        let mut model = fixture::steel_frame();
        let member = vec![
            load(
                MemberLoadKind::Point {
                    a: 1000.0,
                    p: 100.0,
                },
                dir,
                LoadSource::Manual,
            ),
            load(
                MemberLoadKind::Point {
                    a: 1000.0,
                    p: -100.0,
                },
                dir,
                LoadSource::Manual,
            ),
            load(
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 2000.0,
                    w1: 7.0,
                    w2: -7.0,
                },
                dir,
                LoadSource::Manual,
            ),
            load(
                MemberLoadKind::Point { a: 1000.0, p: 0.0 },
                dir,
                LoadSource::Manual,
            ),
            load(
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 2000.0,
                    w1: 0.0,
                    w2: 0.0,
                },
                dir,
                LoadSource::Manual,
            ),
        ];
        model.load_cases.push(LoadCase {
            id: LoadCaseId(9),
            name: "保持対照".into(),
            nodal: vec![],
            member: member.clone(),
            kind: LoadCaseKind::Other,
        });
        prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
        assert_eq!(
            model
                .load_cases
                .iter()
                .find(|case| case.id == LoadCaseId(9))
                .unwrap()
                .member,
            member
        );
    }
    let mut model = fixture::steel_frame();
    model.load_cases.clear();
    prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None).unwrap();
}
