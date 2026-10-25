//! 無開口RC自立壁のn倍法による方向別付加剛性。

use sepika_core::ids::StoryId;
use sepika_core::model::{
    MaterialCategory, Model, RegionAnchor, WallPlate, WallPlateShape, DIAPHRAGM_LEVEL_TOL_MM,
};

use super::core::ColumnStiffness;

/// 正の有限なn・壁面積・柱面積と非負有限の柱剛性から付加剛性 [N/mm] を返す。
/// aw・sum_acは [mm²]、sum_kcは [N/mm]、nは無次元。入力不正又は非有限な結果はErr。
pub fn misc_wall_stiffness(n: f64, aw: f64, sum_kc: f64, sum_ac: f64) -> Result<f64, String> {
    if [n, aw, sum_ac].iter().any(|v| !v.is_finite() || *v <= 0.0)
        || !sum_kc.is_finite()
        || sum_kc < 0.0
    {
        return Err("n倍法: n・壁面積・柱面積は正の有限値、柱剛性は非負有限値が必要です".into());
    }
    let stiffness = n * aw * sum_kc / sum_ac;
    if !stiffness.is_finite() {
        return Err("n倍法: 付加剛性の積又は除算結果が非有限です".into());
    }
    Ok(stiffness)
}

/// 層内同一断面の柱の総面積 [mm²]。断面・材料・面積の欠落や不正はErr。
pub fn sum_column_area(model: &Model, story: StoryId) -> Result<f64, String> {
    let mut sum = 0.0;
    for col in crate::secondary::story_columns::story_columns(model, story) {
        let failure =
            |reason: &str| format!("n倍法: 階{}の柱{}: {reason}", story.0, col.top_elem.0);
        let elem = model
            .element(col.top_elem)
            .ok_or_else(|| failure("部材を参照できません"))?;
        let sec = model
            .element_section(elem)
            .ok_or_else(|| failure("断面を参照できません"))?;
        for id in &col.segments {
            let part = model
                .element(*id)
                .ok_or_else(|| failure("分割部材を参照できません"))?;
            if part.section != elem.section {
                return Err(failure("層内で断面が変わる柱には適用できません"));
            }
            let mat = model
                .element_material(part)
                .ok_or_else(|| failure("材料を参照できません"))?;
            if !mat.young.is_finite() || mat.young <= 0.0 {
                return Err(failure("ヤング係数が正の有限値ではありません"));
            }
        }
        if !sec.area.is_finite() || sec.area <= 0.0 {
            return Err(failure("柱断面積が正の有限値ではありません"));
        }
        sum += sec.area;
    }
    if !sum.is_finite() || sum <= 0.0 {
        return Err(format!(
            "n倍法: 階{}の柱総面積が正の有限値ではありません",
            story.0
        ));
    }
    Ok(sum)
}

struct SelfStandingStiffness {
    pos: [f64; 2],
    dir: [f64; 2],
    aw: f64,
    z_mid: f64,
}

fn self_standing_stiffness(
    model: &Model,
    plate: &WallPlate,
) -> Result<Option<SelfStandingStiffness>, String> {
    let WallPlateShape::Attached {
        anchor: RegionAnchor::FloorRegion { nodes },
        ..
    } = &plate.shape
    else {
        return Ok(None);
    };
    let failure = |reason: &str| format!("n倍法: 壁{}: {reason}", plate.id.0);
    if !plate.openings.is_empty() || plate.slit.any() {
        return Ok(None);
    }
    if !plate.opening_area.is_finite() || plate.opening_area < 0.0 {
        return Err(failure("開口面積が不正です"));
    }
    if plate.opening_area > 0.0 {
        return Ok(None);
    }
    let coord = |id: sepika_core::ids::NodeId| {
        model
            .nodes
            .get(id.index())
            .filter(|node| node.id == id && node.coord.iter().all(|v| v.is_finite()))
            .map(|node| node.coord)
            .ok_or_else(|| failure("支持節点の参照又は座標が不正です"))
    };
    let a = coord(nodes[0])?;
    let b = coord(nodes[1])?;
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if !len.is_finite() || len <= 0.0 {
        return Err(failure("平面長さが正の有限値ではありません"));
    }
    let z_base = a[2] / 2.0 + b[2] / 2.0;
    if matches!(
        plate.shape,
        WallPlateShape::Attached {
            extent: Some(_),
            ..
        }
    ) && model.story_level_diagnostics().is_empty()
        && model
            .stories
            .last()
            .and_then(|s| s.sl_mm())
            .is_some_and(|sl| (z_base - sl).abs() <= DIAPHRAGM_LEVEL_TOL_MM)
    {
        return Ok(None);
    }
    let story_height = model
        .try_story_height_above(z_base)
        .map_err(|reason| failure(&reason))?;
    let extent = model
        .try_wall_plate_extent(plate)
        .map_err(|reason| failure(&reason))?;
    if extent.iter().any(|v| !v.is_finite()) {
        return Err(failure("壁高さが非有限です"));
    }
    if extent[0].min(extent[1]) < story_height - DIAPHRAGM_LEVEL_TOL_MM {
        return Ok(None);
    }
    let mat = model
        .wall_plate_material(plate)
        .ok_or_else(|| failure("主材料を参照できません"))?;
    if mat.category != MaterialCategory::Concrete {
        return Ok(None);
    }
    let t = model
        .wall_plate_thickness(plate)
        .ok_or_else(|| failure("構造厚を参照できません"))?;
    let aw = len * t;
    let z_mid = z_base + story_height / 2.0;
    if !aw.is_finite() || aw <= 0.0 || !z_mid.is_finite() {
        return Err(failure("壁断面積又は帰属高さが不正です"));
    }
    Ok(Some(SelfStandingStiffness {
        pos: [a[0] / 2.0 + b[0] / 2.0, a[1] / 2.0 + b[1] / 2.0],
        dir: [dx / len, dy / len],
        aw,
        z_mid,
    }))
}

/// 対象層の無開口RC自立壁を付加する。n未指定なら追加しない。
/// n不正、対象壁・柱の必須入力不足、層内異断面、剛性集計不整合はErr。
pub fn append_misc_wall_stiffnesses(
    model: &Model,
    story: StoryId,
    cols: &mut Vec<ColumnStiffness>,
) -> Result<(), String> {
    let Some(n) = model.stress_cfg.misc_wall_n else {
        return Ok(());
    };
    if !n.is_finite() || n <= 0.0 {
        return Err("n倍法: 係数nは正の有限値にしてください".into());
    }
    if model.wall_plates.is_empty() {
        return Ok(());
    }
    let idx = story.index();
    let elev = model
        .stories
        .get(idx)
        .and_then(|s| s.sl_mm())
        .ok_or_else(|| format!("n倍法: 階{}のSLを参照できません", story.0))?;
    let below = if idx == 0 {
        f64::NEG_INFINITY
    } else {
        model.stories[idx - 1]
            .sl_mm()
            .ok_or_else(|| format!("n倍法: 階{}の直下階SLを参照できません", story.0))?
    };
    let mut walls = Vec::new();
    for plate in &model.wall_plates {
        if let Some(w) = self_standing_stiffness(model, plate)? {
            if w.z_mid > below + 1e-9 && w.z_mid <= elev + 1e-9 {
                walls.push((plate.id, w));
            }
        }
    }
    if walls.is_empty() {
        return Ok(());
    }
    let sum_ac = sum_column_area(model, story)?;
    let columns = crate::secondary::story_columns::story_columns(model, story);
    if columns.len() != cols.len()
        || cols
            .iter()
            .any(|c| !c.dx.is_finite() || c.dx < 0.0 || !c.dy.is_finite() || c.dy < 0.0)
    {
        return Err(format!(
            "n倍法: 階{}の柱剛性集合が不足又は不正です",
            story.0
        ));
    }
    let sum_kx = cols.iter().map(|c| c.dx).sum();
    let sum_ky = cols.iter().map(|c| c.dy).sum();
    let mut additions = Vec::new();
    for (id, w) in walls {
        let failure = |reason| format!("階{}の壁{}: {reason}", story.0, id.0);
        let [cx, cy] = w.dir;
        additions.push(ColumnStiffness {
            pos: w.pos,
            dx: misc_wall_stiffness(n, w.aw, sum_kx, sum_ac).map_err(failure)? * cx * cx,
            dy: misc_wall_stiffness(n, w.aw, sum_ky, sum_ac).map_err(failure)? * cy * cy,
        });
    }
    cols.extend(additions);
    Ok(())
}
