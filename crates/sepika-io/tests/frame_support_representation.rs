#[path = "../../sepika-core/tests/support/frame_support_model.rs"]
mod fixture;
use sepika_io::ovika::{load_ovika, save_ovika, OvikaExtras};

#[test]
fn 明示解析表現の床壁版支持境界は実ovika保存読込再構築で保持する() {
    for wall in [false, true] {
        for (variant, kinds) in fixture::representations().into_iter().enumerate() {
            let mut model = fixture::with_plate(wall, kinds);
            model.assign_stb_node_ids().unwrap();
            let path = std::env::temp_dir().join(format!(
                "sepika-597-{}-{wall}-{variant}.ovika",
                std::process::id()
            ));
            save_ovika(&path, &model, OvikaExtras::default()).unwrap();
            let mut loaded = load_ovika(&path).unwrap().model;
            std::fs::remove_file(path).unwrap();
            assert!(loaded.eq_ignoring_dofmap(&model));
            loaded.rebuild_assignment_regions().unwrap();
            fixture::assert_geometry(&loaded, wall);
            assert_eq!(
                loaded.floor_assignment_regions,
                model.floor_assignment_regions
            );
            assert_eq!(
                loaded.wall_assignment_regions,
                model.wall_assignment_regions
            );
        }
    }
}
