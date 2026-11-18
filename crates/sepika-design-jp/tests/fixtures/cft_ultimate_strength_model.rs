use super::cft_column_buckling_input::{cantilever, direct_lengths};
use sepika_core::ids::ElemId;
use sepika_core::model::Model;

pub fn circular_column(fc_n_mm2: f64) -> Model {
    let mut model = cantilever(true, 100_000.0);
    model.materials[0].fc = Some(fc_n_mm2);
    model.steel_design_attrs = vec![direct_lengths(ElemId(0), Some(1200.0), Some(1200.0))];
    model
}
