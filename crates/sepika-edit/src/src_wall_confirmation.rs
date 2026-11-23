use crate::{CompositeCommand, EditCommand};
use sepika_core::ids::{SectionId, WallPlateId};
use sepika_core::model::{Model, SupportMemberId, WallPlate};

type Confirmation = (WallPlateId, bool, bool);

pub(super) fn invalidate_changed(
    before: &Model,
    after: &mut Model,
    inverse: Box<dyn EditCommand>,
) -> Box<dyn EditCommand> {
    let mut previous = Vec::new();
    let mut changed = Vec::new();
    for wall in &after.wall_plates {
        let transfer = wall
            .src_steel_plate
            .is_some_and(|steel| steel.force_transfer_confirmed);
        if !wall.src_wall_detailing_confirmed && !transfer {
            continue;
        }
        let previous_wall = after
            .wall_plate_assignment_region(wall.id)
            .and_then(|region| {
                before
                    .wall_assignment_regions
                    .regions
                    .iter()
                    .find(|old| old.id == region.id)
            })
            .and_then(|region| before.wall_plate(region.assignment.plate()?))
            .or_else(|| before.wall_plate(wall.id));
        let Some(old) = previous_wall else { continue };
        if same_wall_specification(before, old, after, wall) {
            continue;
        }
        previous.push((
            old.id,
            old.src_wall_detailing_confirmed,
            old.src_steel_plate
                .is_some_and(|steel| steel.force_transfer_confirmed),
        ));
        changed.push(wall.id);
    }
    if changed.is_empty() {
        return inverse;
    }
    for id in changed {
        let wall = &mut after.wall_plates[id.index()];
        wall.src_wall_detailing_confirmed = false;
        if let Some(steel) = &mut wall.src_steel_plate {
            steel.force_transfer_confirmed = false;
        }
    }
    Box::new(CompositeCommand {
        label: inverse.label().into(),
        children: vec![inverse, Box::new(RestoreConfirmations(previous))],
    })
}

fn same_wall_specification(a: &Model, old: &WallPlate, b: &Model, new: &WallPlate) -> bool {
    if let (Some(old), Some(new)) = (old.src_steel_plate, new.src_steel_plate) {
        if old.material == new.material
            && a.materials.get(old.material.index()) != b.materials.get(new.material.index())
        {
            return false;
        }
    }
    if old.shape != new.shape
        || old.slit != new.slit
        || old.opening_area != new.opening_area
        || old.openings != new.openings
        || old.boundary_coords(a) != new.boundary_coords(b)
        || !same_section(a, old.section, b, new.section)
    {
        return false;
    }
    let before = a
        .wall_plate_assignment_region(old.id)
        .map(|r| r.boundary.as_slice());
    let after = b
        .wall_plate_assignment_region(new.id)
        .map(|r| r.boundary.as_slice());
    if before != after {
        return false;
    }
    before.into_iter().flatten().all(|edge| {
        if let SupportMemberId::Primary(id) = edge.support {
            let before = a.element(id);
            let after = b.element(id);
            before.zip(after).is_some_and(|(before, after)| {
                before.kind == after.kind
                    && before.nodes == after.nodes
                    && before.local_axis == after.local_axis
                    && before.end_cond == after.end_cond
                    && before.end_offsets_mm == after.end_offsets_mm
                    && same_section(a, before.section, b, after.section)
            })
        } else {
            true
        }
    })
}

fn same_section(a: &Model, old: Option<SectionId>, b: &Model, new: Option<SectionId>) -> bool {
    let old = old.and_then(|id| a.section(id));
    let new = new.and_then(|id| b.section(id));
    let materials = |s: &sepika_core::model::Section| {
        [
            s.material,
            s.rebar_material,
            s.shear_rebar_material,
            s.steel_material,
        ]
    };
    match (old, new) {
        (Some(old), Some(new)) => {
            old.id == new.id
                && old.shape == new.shape
                && old.frame_use == new.frame_use
                && materials(old) == materials(new)
                && materials(old)
                    .into_iter()
                    .flatten()
                    .all(|id| a.materials.get(id.index()) == b.materials.get(id.index()))
        }
        (None, None) => true,
        _ => false,
    }
}

struct RestoreConfirmations(Vec<Confirmation>);

impl EditCommand for RestoreConfirmations {
    fn apply(&self, model: &mut Model) -> Box<dyn EditCommand> {
        let mut previous = Vec::new();
        for &(id, detailing, transfer) in &self.0 {
            let Some(wall) = model.wall_plates.get_mut(id.index()).filter(|p| p.id == id) else {
                continue;
            };
            previous.push((
                id,
                std::mem::replace(&mut wall.src_wall_detailing_confirmed, detailing),
                wall.src_steel_plate.as_mut().is_some_and(|steel| {
                    std::mem::replace(&mut steel.force_transfer_confirmed, transfer)
                }),
            ));
        }
        Box::new(Self(previous))
    }

    fn label(&self) -> &str {
        "SRC耐震壁の確認状態変更"
    }
}

#[cfg(test)]
#[path = "../../sepika-solver/tests/support/rc_wall_side_column_model.rs"]
mod rc_wall_side_column_model;
#[cfg(test)]
#[path = "../../sepika-solver/tests/support/src_wall_model.rs"]
mod src_wall_model;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_section_edit_requires_src_confirmation_again_and_undo_restores_it() {
        let mut model = src_wall_model::input_model(true);
        let shape = model.sections[0].shape.clone().unwrap();
        let sepika_core::section_shape::SectionShape::SrcColumnRect {
            b,
            d,
            rebar,
            steel_height,
            steel_width,
            steel_web_thick,
            ..
        } = shape
        else {
            unreachable!()
        };
        let mut undo = crate::UndoStack::new();
        assert!(
            undo.run(
                &mut model,
                Box::new(crate::EditSectionShape {
                    section: SectionId(0),
                    new_shape: sepika_core::section_shape::SectionShape::SrcColumnRect {
                        b,
                        d,
                        rebar,
                        steel_height,
                        steel_width,
                        steel_web_thick,
                        steel_flange_thick: 14.0,
                    },
                    frame_use: Some(sepika_core::model::FrameSectionUse::Column),
                })
            ),
            "{:?}",
            undo.last_error()
        );
        let flags = |m: &Model| {
            let wall = m.wall_plate(WallPlateId(0)).unwrap();
            (
                wall.src_wall_detailing_confirmed,
                wall.src_steel_plate.unwrap().force_transfer_confirmed,
            )
        };
        assert_eq!(flags(&model), (false, false));
        undo.undo(&mut model);
        assert_eq!(flags(&model), (true, true));
        undo.redo(&mut model);
        assert_eq!(flags(&model), (false, false));
    }

    #[test]
    fn preparing_rigid_faces_does_not_change_src_wall_specification() {
        let mut model = src_wall_model::input_model(true);
        let mut prepared = model.clone();
        prepared.elements[0].rigid_zone.face_i = Some(400.0);
        let mut undo = crate::UndoStack::new();
        assert!(undo.run(&mut model, Box::new(crate::ApplyPreparedModel { prepared })));
        let plate = &model.wall_plates[0];
        assert!(plate.src_wall_detailing_confirmed);
        assert!(plate.src_steel_plate.unwrap().force_transfer_confirmed);
    }
}
