#[path = "fixtures/rc_column_axial_boundary_model.rs"]
mod column;
#[path = "fixtures/rc_column_mechanism_model.rs"]
mod fixture;

use fixture::{COLUMN, GIRDER_SECTION, REBAR};
use sepika_core::model::Model;
use sepika_design_jp::{
    run_member_design_checks, CheckKind, CheckOutcome, LoadTerm, MemberDesignCheckOptions, QdMethod,
};
use sepika_element::frame::beam::MemberForces;

fn check(
    model: &Model,
    method: QdMethod,
    gravity_n: Option<f64>,
    forces: [f64; 6],
) -> Vec<CheckOutcome> {
    let dist = |force| {
        [(
            COLUMN,
            MemberForces {
                at: vec![(0.5, force)],
            },
        )]
    };
    let gravity = gravity_n.map(|n| dist([n, 0.0, 0.0, 0.0, 0.0, 0.0]));
    run_member_design_checks(
        model,
        &dist(forces),
        &[],
        &MemberDesignCheckOptions {
            term: LoadTerm::Short,
            qd_method: method,
            long_member_forces: gravity.as_ref().map(|forces| &forces[..]),
            ..Default::default()
        },
    )
    .unwrap()
    .member_checks
    .into_iter()
    .map(|(elem, pos, row)| {
        assert_eq!((elem, pos), (COLUMN, 0.5));
        row
    })
    .collect()
}

#[test]
fn invalid_girder_rebar_category_blocks_only_its_assigned_column_shear_direction() {
    for (girder_xy, axis) in [
        ([4000.0, 0.0], "Qy"),
        ([0.0, 4000.0], "Qz"),
        ([4000.0, 4000.0], "Qy"),
    ] {
        let mut model = fixture::column_with_girder(column::column_model(false, 0.0));
        model.nodes[2].coord[..2].copy_from_slice(&girder_xy);
        let demand = [-100_000.0, 1000.0, 1000.0, 0.0, 0.0, 0.0];
        for method in [QdMethod::Qd1, QdMethod::Min] {
            let rows = check(&model, method, Some(demand[0]), demand);
            let [CheckOutcome::Skipped { reason }] = &rows[..] else {
                panic!("{rows:?}");
            };
            for expected in [
                &format!("RC柱 ID {} の {axis} 方向", COLUMN.0),
                &format!("大梁 ID {}", fixture::GIRDER.0),
                "断面「接続大梁」",
                "主筋材料「SD345」",
                "材料区分は「鋼材」",
                "断面タブで主筋に「鉄筋」区分の材料を割り当て",
            ] {
                assert!(reason.contains(expected), "{reason}");
            }
            assert_eq!(reason.matches("方向のせん断検定").count(), 1, "{reason}");
        }

        let qd2 = check(&model, QdMethod::Qd2, Some(demand[0]), demand);
        let non_seismic = check(&model, QdMethod::Qd1, None, demand);
        model.sections[GIRDER_SECTION.index()].rebar_material = Some(REBAR);
        for (method, gravity) in [
            (QdMethod::Qd1, Some(demand[0])),
            (QdMethod::Min, Some(demand[0])),
            (QdMethod::Qd2, Some(demand[0])),
            (QdMethod::Qd1, None),
        ] {
            let rows = check(&model, method, gravity, demand);
            assert!(
                matches!(&rows[..], [CheckOutcome::Checked(result)] if result.ok()
                    && result.components.iter().any(|c| c.kind == CheckKind::Shear && c.ratio > 0.0)),
                "{rows:?}"
            );
            match (method, gravity) {
                (QdMethod::Qd2, _) => assert_eq!(rows, qd2),
                (_, None) => assert_eq!(rows, non_seismic),
                _ => {}
            }
        }
    }
}

#[test]
fn invalid_girder_rebar_category_preserves_independent_column_failures() {
    let model = fixture::column_with_girder(column::column_model(false, 0.0));
    let mut repaired = model.clone();
    repaired.sections[GIRDER_SECTION.index()].rebar_material = Some(REBAR);
    for (gravity, demand, failed_kind) in [
        (
            -100_000.0,
            [-100_000.0, 0.0, 0.0, 0.0, 0.0, 600.0e6],
            CheckKind::AxialBending,
        ),
        (
            -2_000_000.0,
            [-2_000_000.0, 0.0, 1_000_000.0, 0.0, 0.0, 0.0],
            CheckKind::Shear,
        ),
    ] {
        let rows = check(&model, QdMethod::Min, Some(gravity), demand);
        let [CheckOutcome::Checked(result), CheckOutcome::Skipped { reason }] = &rows[..] else {
            panic!("{rows:?}");
        };
        assert!(reason.contains("Qy 方向"), "{reason}");
        let [failed] = &result.components[..] else {
            panic!("{rows:?}");
        };
        assert_eq!(failed.kind, failed_kind);
        assert!(failed.ratio > 1.0);
        let rows = check(&repaired, QdMethod::Min, Some(gravity), demand);
        let [CheckOutcome::Checked(result)] = &rows[..] else {
            panic!("{rows:?}");
        };
        let baseline = result
            .components
            .iter()
            .find(|c| c.kind == failed_kind)
            .unwrap();
        assert_eq!(failed.ratio, baseline.ratio);
    }
}
