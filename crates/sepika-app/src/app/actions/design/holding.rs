use super::super::*;

#[derive(Debug)]
struct RcWallDsGeometry {
    plate: sepika_core::ids::WallPlateId,
    clear_length_mm: f64,
    clear_height_mm: f64,
    columns: [(ElemId, f64, f64); 2],
}

fn rc_wall_ds_geometry(
    wall: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    plate_id: Option<sepika_core::ids::WallPlateId>,
) -> Result<RcWallDsGeometry, String> {
    use sepika_core::model::{MaterialCategory, SupportMemberId};
    use sepika_core::section_shape::SectionShape;
    let plate_id = plate_id.ok_or("壁版の対応が不明です")?;
    let plate = model.wall_plate(plate_id).ok_or("壁版が未解決です")?;
    if !plate.openings.is_empty() || plate.opening_area != 0.0 {
        return Err(format!(
            "壁版 ID {}: 開口付きのDs判定は未対応です",
            plate_id.0
        ));
    }
    let parent = model
        .wall_regions
        .iter()
        .find(|r| r.wall_plate_ids.contains(&plate_id))
        .ok_or("壁領域が未解決です")?;
    if parent.wall_plate_ids.as_slice() != [plate_id] || !model.wall_plate_covers_region(plate) {
        return Err(format!(
            "壁版 ID {}: 単独壁と壁単体Qの負担範囲を確認できません",
            plate_id.0
        ));
    }
    let assignment = model
        .wall_plate_assignment_region(plate_id)
        .ok_or("壁版割当領域が未解決です")?;
    let g = sepika_core::model::wall_element_geometry(wall, model).ok_or("壁幾何が不正です")?;
    let points =
        [g.bottom[0], g.bottom[1], g.top[1], g.top[0]].map(|id| model.nodes[id.index()].coord);
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("壁の座標が非有限です".into());
    }
    if points[0][2] != points[1][2]
        || points[2][2] != points[3][2]
        || !(0..2)
            .all(|axis| points[0][axis] == points[3][axis] && points[1][axis] == points[2][axis])
    {
        return Err("矩形・同一鉛直面以外の壁は未対応です".into());
    }
    let nodes = [g.bottom[0], g.bottom[1], g.top[1], g.top[0]];
    let mut supports = Vec::new();
    for i in 0..4 {
        let candidates: Vec<_> = assignment
            .boundary
            .iter()
            .filter_map(|edge| {
                let SupportMemberId::Primary(id) = edge.support else {
                    return None;
                };
                let e = model.element(id)?;
                (e.nodes.len() == 2
                    && e.nodes.contains(&nodes[i])
                    && e.nodes.contains(&nodes[(i + 1) % 4]))
                .then_some(e)
            })
            .collect();
        if candidates.len() != 1 {
            return Err("周辺の主架構支持材を一意に解決できません".into());
        }
        supports.push(candidates[0]);
    }
    let mut columns = [(ElemId(0), 0.0, 0.0); 2];
    for (side, edge_index) in [3, 1].into_iter().enumerate() {
        let column = supports[edge_index];
        if sepika_design_jp::MemberKind::of_element(column, model)
            != sepika_design_jp::MemberKind::Column
        {
            return Err(format!("側柱 ID {}: 柱用途を確認できません", column.id.0));
        }
        for other in model
            .wall_assignment_regions
            .regions
            .iter()
            .filter(|r| r.id != assignment.id)
        {
            let Some(other_plate) = other.assignment.plate() else {
                continue;
            };
            if parent.wall_plate_ids.contains(&other_plate) {
                continue;
            }
            if other
                .boundary
                .iter()
                .any(|e| e.support == SupportMemberId::Primary(column.id))
            {
                return Err(format!(
                    "壁版 ID {}・側柱 ID {}: 壁版 ID {}と共有する側柱は未対応です",
                    plate_id.0, column.id.0, other_plate.0
                ));
            }
        }
        let section = model
            .element_section(column)
            .ok_or("側柱の断面が未設定です")?;
        let material = model
            .element_material(column)
            .ok_or("側柱の材料が未設定です")?;
        if material.category != MaterialCategory::Concrete
            || material.fc.filter(|v| v.is_finite() && *v > 0.0).is_none()
        {
            return Err(format!("側柱 ID {}: RC材料またはFcが不正です", column.id.0));
        }
        let (area, diameter) = match section.shape.as_ref() {
            Some(SectionShape::RcColumnRect { b, d, .. })
                if [*b, *d].iter().all(|v| v.is_finite() && *v > 0.0) =>
            {
                (b * d, b.min(*d))
            }
            Some(SectionShape::RcColumnCircle { d, .. }) if d.is_finite() && *d > 0.0 => {
                (std::f64::consts::PI * d * d / 4.0, *d)
            }
            _ => {
                return Err(format!(
                    "側柱 ID {}: RC柱の実断面寸法が不明または未対応です",
                    column.id.0
                ))
            }
        };
        columns[side] = (column.id, area, diameter);
    }
    let mut faces = [0.0; 4];
    for i in 0..4 {
        let section = model
            .element_section(supports[i])
            .ok_or("周辺材断面が未設定です")?;
        if ![section.width, section.depth]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return Err(format!(
                "周辺材 ID {}: 実断面寸法が不正です",
                supports[i].id.0
            ));
        }
        let material = model
            .element_material(supports[i])
            .ok_or("周辺材材料が未設定です")?;
        if material.category != MaterialCategory::Concrete
            || material.fc.filter(|v| v.is_finite() && *v > 0.0).is_none()
        {
            return Err(format!(
                "周辺材 ID {}: RC材料またはFcが不正です",
                supports[i].id.0
            ));
        }
        let direction = match i {
            0 => [0.0, 0.0, 1.0],
            1 => g.ex_bottom.map(|v| -v),
            2 => [0.0, 0.0, -1.0],
            _ => g.ex_bottom,
        };
        faces[i] = sepika_core::face_distance::wall_support_face(
            model,
            SupportMemberId::Primary(supports[i].id),
            direction,
        )?;
        if !faces[i].is_finite() || faces[i] <= 0.0 {
            return Err(format!(
                "周辺材 ID {}: 実フェース距離が不正です",
                supports[i].id.0
            ));
        }
    }
    let clear_length_mm = g.lw - faces[1] - faces[3];
    let clear_height_mm = points[3][2] - points[0][2] - faces[0] - faces[2];
    if ![clear_length_mm, clear_height_mm]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        return Err("壁板の内法寸法が非正または非有限です".into());
    }
    Ok(RcWallDsGeometry {
        plate: plate_id,
        clear_length_mm,
        clear_height_mm,
        columns,
    })
}

impl App {
    /// 目的別評価点の識別・釣合いを検査し、採用条件の不足を理由付きで返す。
    /// 自動・手動ランクとも採用条件が未確認なら合否を保留し、前回の採用結果を失効する。
