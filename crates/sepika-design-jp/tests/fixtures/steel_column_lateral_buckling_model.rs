use sepika_core::dof::Dof;
use sepika_core::model::{Model, SteelDesignAttr};
use sepika_element::frame::beam::MemberForces;

#[path = "steel_h_column_combined_model.rs"]
mod input;

pub fn column(offset_mm: f64, support_confirmed: bool) -> Model {
    let mut model = input::cantilever(0.0, 0.0, 0.0);
    model.load_cases.clear();
    model.nodes[1].restraint.set_fixed(Dof::Uy);
    model.nodes[1].restraint.set_fixed(Dof::Rz);
    model.elements[0].end_offsets_mm[1][2] = offset_mm;
    model.steel_design_attrs.push(SteelDesignAttr {
        elem: model.elements[0].id,
        joint_flange_loss: 0.0,
        joint_web_loss: 0.0,
        scallop_web_loss: 0.0,
        lb_direct: None,
        lateral_brace_count: None,
        column_lateral_buckling_end_supports_confirmed: support_confirmed,
        lk_y_direct: Some(3500.0),
        lk_z_direct: Some(3500.0),
        c_direct: None,
    });
    model
}

pub fn moments(mz_nmm: f64) -> MemberForces {
    MemberForces {
        at: [0.0, 0.5, 1.0]
            .map(|pos| (pos, [0.0, 0.0, 0.0, 0.0, 0.0, mz_nmm]))
            .to_vec(),
    }
}
