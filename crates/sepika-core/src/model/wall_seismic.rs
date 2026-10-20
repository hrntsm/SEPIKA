//! 壁エレメントの成立判定。

use super::{ElementData, ElementKind, MaterialCategory, Model};
use crate::ids::NodeId;
use crate::section_shape::SectionShape;

/// RC壁専用形状またはコンクリート主材の壁かを返す。
pub fn is_rc_wall(data: &ElementData, model: &Model) -> bool {
    let sec_is_rc_wall = data
        .section
        .and_then(|sid| model.sections.get(sid.index()))
        .is_some_and(|s| matches!(s.shape, Some(SectionShape::RcWall { .. })));
    let mat_is_rc = model
        .element_material(data)
        .is_some_and(|m| m.category == MaterialCategory::Concrete);
    sec_is_rc_wall || mat_is_rc
}

/// 上下大梁・スリット・RC壁厚・開口の成立条件を判定する。
/// 断面が未設定の暫定壁は、厚さと開口の判定を省略する。
pub fn wall_is_seismic(data: &ElementData, model: &Model) -> bool {
    let Some(g) = super::wall_element_geometry(data, model) else {
        return false;
    };
    if !wall_is_framed_with(&g, model) {
        return false;
    }
    let attr = model.wall_attrs.iter().find(|w| w.elem == data.id);
    if attr.is_some_and(|a| a.slit.any()) {
        return false;
    }
    if !is_rc_wall(data, model) {
        return true;
    }
    let Some(t) = wall_thickness(data, model) else {
        return true;
    };
    if t < 120.0 {
        return false;
    }
    let opening_area = attr
        .map(|a| a.total_opening_area_for(model.multi_opening_mode))
        .unwrap_or(0.0);
    if opening_area <= 0.0 {
        return true;
    }
    if g.lw <= 0.0 || g.h <= 0.0 {
        return true;
    }
    let r0 = (opening_area / (g.lw * g.h)).max(0.0).sqrt();
    r0 <= 0.4
}

/// 上下辺の両節点IDに一致する水平線材があるかを返す。側柱の有無は問わない。
pub fn wall_is_framed(data: &ElementData, model: &Model) -> bool {
    let Some(g) = super::wall_element_geometry(data, model) else {
        return false;
    };
    wall_is_framed_with(&g, model)
}

fn wall_is_framed_with(g: &super::WallElementGeometry, model: &Model) -> bool {
    has_girder(model, g.bottom[0], g.bottom[1]) && has_girder(model, g.top[0], g.top[1])
}

/// 成立する壁と周辺線材の材料区分が不整合なら、是正内容を返す。
pub fn wall_frame_category_issue(data: &ElementData, model: &Model) -> Option<String> {
    if !matches!(data.kind, ElementKind::Wall) || !wall_is_seismic(data, model) {
        return None;
    }
    let g = super::wall_element_geometry(data, model)?;
    let (wall_label, want) = if is_rc_wall(data, model) {
        ("RC 造", MaterialCategory::Concrete)
    } else {
        ("S 造", MaterialCategory::Steel)
    };
    let edges = [
        (g.bottom[0], g.bottom[1]),
        (g.top[0], g.top[1]),
        (g.bottom[0], g.top[0]),
        (g.bottom[1], g.top[1]),
    ];
    for e in &model.elements {
        if !e.kind.is_primary_frame_support() || e.nodes.len() < 2 {
            continue;
        }
        let (n0, n1) = (e.nodes[0], e.nodes[1]);
        if !edges
            .iter()
            .any(|(a, b)| (*a == n0 && *b == n1) || (*a == n1 && *b == n0))
        {
            continue;
        }
        let Some(mat) = model.element_material(e) else {
            continue;
        };
        let cft_column = want == MaterialCategory::Concrete
            && edges[2..]
                .iter()
                .any(|(a, b)| (*a == n0 && *b == n1) || (*a == n1 && *b == n0))
            && model
                .element_section(e)
                .and_then(|s| s.shape.as_ref())
                .is_some_and(|s| {
                    matches!(
                        s,
                        SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }
                    )
                });
        if mat.category != want && !cft_column {
            return Some(format!(
                "耐震壁 ID {} は{}ですが、周辺架構の部材 ID {} の材料「{}」の区分は{}です。\
                 耐震壁と周辺架構の構造種別を揃えてください。\
                 壁エレメントは壁と周辺架構を一体の耐震要素としてモデル化するため、\
                 混合構造は扱えません。",
                data.id.0,
                wall_label,
                e.id.0,
                mat.name,
                mat.category.label()
            ));
        }
    }
    None
}

fn has_girder(model: &Model, a: NodeId, b: NodeId) -> bool {
    model.elements.iter().any(|e| {
        if !e.kind.is_primary_frame_support() || e.nodes.len() < 2 {
            return false;
        }
        let (n0, n1) = (e.nodes[0], e.nodes[1]);
        if !((n0 == a && n1 == b) || (n0 == b && n1 == a)) {
            return false;
        }
        let (Some(p0), Some(p1)) = (model.nodes.get(n0.index()), model.nodes.get(n1.index()))
        else {
            return false;
        };
        let (dx, dy, dz) = (
            p1.coord[0] - p0.coord[0],
            p1.coord[1] - p0.coord[1],
            p1.coord[2] - p0.coord[2],
        );
        let lp = (dx * dx + dy * dy).sqrt();
        lp > 1e-9 && dz.abs() <= 0.05 * lp
    })
}

/// 壁板厚 [mm]（RcWall 形状 → Section.thickness → Section.width の順）。
pub(crate) fn wall_thickness(data: &ElementData, model: &Model) -> Option<f64> {
    let sec = data
        .section
        .and_then(|sid| model.sections.get(sid.index()))?;
    let t = match &sec.shape {
        Some(SectionShape::RcWall { thickness, .. }) => *thickness,
        _ => sec.thickness.unwrap_or(sec.width),
    };
    (t > 0.0).then_some(t)
}
