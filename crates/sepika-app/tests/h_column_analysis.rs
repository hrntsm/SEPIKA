use sepika_core::dof::Dof6Mask;
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{LoadCase, NodalLoad};
#[path = "support/h_column_model.rs"]
mod fixture;
use sepika_job::compute::compute_linear_static;

#[test]
fn 鉄骨向きの入力とovika往復で片持ち柱の強弱軸の曲げ応答を保つ() {
    // 組立Hの三矩形の断面二次モーメントから、純曲げ時の柱頭角 ML/EI [rad] を求める。
    let weak_rotation = 100_000_000.0 * 3000.0 / (205_000.0 * 90_080_640.0);
    let strong_rotation = 100_000_000.0 * 3000.0 / (205_000.0 * 1_185_216_000.0);
    for (direction, expected) in [
        ([1.0, 0.0, 0.0], [weak_rotation, strong_rotation]),
        ([0.0, 1.0, 0.0], [strong_rotation, weak_rotation]),
    ] {
        let model = fixture::cantilever(sepika_app::sample::portal_frame(), direction);
        let path =
            std::env::temp_dir().join(format!("sepika-h-column-{}.ovika", uuid::Uuid::new_v4()));
        sepika_io::ovika::save_ovika(&path, &model, Default::default()).unwrap();
        let reread = sepika_io::ovika::load_ovika(&path).unwrap().model;
        std::fs::remove_file(path).unwrap();
        for mut model in [model, reread] {
            let [base, tip] = [model.elements[0].nodes[0], model.elements[0].nodes[1]];
            model.nodes[base.index()].restraint = Dof6Mask::FIXED;
            model.nodes[tip.index()].restraint = Dof6Mask::FREE;
            let case = LoadCaseId(0);
            model.load_cases = vec![LoadCase {
                id: case,
                name: "柱頭の純曲げ".into(),
                origin: Default::default(),
                nodal: vec![NodalLoad {
                    node: tip,
                    values: [0.0, 0.0, 0.0, 100_000_000.0, 100_000_000.0, 0.0],
                    name: String::new(),
                    source: Default::default(),
                }],
                member: Vec::new(),
                kind: Default::default(),
            }];
            let result = compute_linear_static(model, case).unwrap();
            for (actual, expected) in result.disp[tip.index()][3..5].iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 1e-10,
                    "{direction:?}: {actual} != {expected}"
                );
            }
        }
    }
}
