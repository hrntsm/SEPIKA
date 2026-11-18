use super::{cft_column_buckling_input, column_model::column_stories};
use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{Model, NodalLoad, PanelZoneMode};

pub(crate) fn cft_long_case2_model(dead_n: f64, live_n: f64) -> Model {
    let mut model = cft_column_buckling_input::cantilever(false, dead_n);
    model.nodes[1].coord[2] = 7200.0;
    model.nodes[1].story = Some(StoryId(1));
    model.stories = column_stories();
    model.stories[1].elevation = Some(7200.0);
    model.materials[0].fc = Some(30.0);
    model.load_cases[1].name = "架構用積載荷重".into();
    model.load_cases[1].nodal.push(tip_compression(live_n));
    model.panel_zone = PanelZoneMode::None;
    model.steel_design_attrs = vec![cft_column_buckling_input::direct_lengths(
        ElemId(0),
        Some(7200.0),
        Some(7200.0),
    )];
    model
}

pub(crate) fn tip_compression(n: f64) -> NodalLoad {
    NodalLoad::manual(NodeId(1), [0.0, 0.0, -n, 0.0, 0.0, 0.0])
}
