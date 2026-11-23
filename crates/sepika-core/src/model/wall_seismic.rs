//! 壁エレメントの成立判定。

use super::{ElementData, ElementKind, MaterialCategory, Model, Section, WallPlate};
use crate::ids::NodeId;
use crate::section_shape::SectionShape;

impl Model {
    /// 壁断面の主材料が鋼材区分かを返す。内蔵鋼材の割当は含めない。
    pub fn wall_section_uses_steel(&self, section: &Section) -> bool {
        section
            .material
            .and_then(|id| self.materials.get(id.index()))
            .is_some_and(|material| material.category == MaterialCategory::Steel)
    }

    pub(crate) fn wall_plate_uses_steel(&self, plate: &WallPlate) -> bool {
        self.wall_plate_section(plate)
            .is_some_and(|section| self.wall_section_uses_steel(section))
    }

    /// 鋼材を主材料とする壁版の ID と是正理由を返す。
    pub fn wall_plate_steel_input_issue(&self, plate: &WallPlate) -> Option<String> {
        if self.wall_plate_uses_steel(plate) {
            return Some(format!(
                "鋼板壁版 ID {} は解析の対象外です。鋼板の重量は荷重として入力してください。",
                plate.id.0
            ));
        }
        let steel = plate.src_steel_plate?;
        let invalid = !self.wall_plate_becomes_element(plate)
            || plate.slit.any()
            || plate.opening_area != 0.0
            || !plate.openings.is_empty()
            || !self.wall_plate_section(plate).and_then(|s| s.shape.as_ref()).is_some_and(|shape| {
                matches!(shape, SectionShape::RcWall { thickness, .. }
                    if steel.thickness_mm.is_finite() && steel.thickness_mm > 0.0 && steel.thickness_mm < *thickness)
            })
            || self.materials.get(steel.material.index()).is_none_or(|m| {
                m.id != steel.material || m.category != MaterialCategory::Steel
            });
        invalid.then(|| {
            format!(
                "壁版 ID {} の内蔵鋼板は無開口・無スリットのRC耐震壁版、鋼材、および壁厚未満の正の鋼板厚が必要です。",
                plate.id.0
            )
        })
    }

    /// 鋼材を主材料とする壁版・壁要素があれば、対象の ID を含む是正理由を返す。
    pub fn validate_wall_primary_materials(&self) -> Result<(), String> {
        let issue = self
            .wall_plates
            .iter()
            .find_map(|plate| self.wall_plate_steel_input_issue(plate))
            .or_else(|| {
                self.elements
                    .iter()
                    .find_map(|data| wall_steel_input_issue(data, self))
            });
        match issue {
            Some(issue) => Err(issue),
            None => Ok(()),
        }
    }
}

fn wall_uses_steel(data: &ElementData, model: &Model) -> bool {
    data.kind == ElementKind::Wall
        && model
            .element_material(data)
            .is_some_and(|material| material.category == MaterialCategory::Steel)
}

/// 鋼材を主材料とする壁要素の ID と是正理由を返す。
pub fn wall_steel_input_issue(data: &ElementData, model: &Model) -> Option<String> {
    wall_uses_steel(data, model).then(|| {
        format!(
            "鋼板壁 ID {} は解析の対象外です。鋼板の重量は荷重として入力してください。",
            data.id.0
        )
    })
}

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

/// 上下大梁・スリット・RC壁厚・開口の成立条件を判定する。鋼材を主材料とする壁は成立しない。
/// 断面が未設定の暫定壁は、厚さと開口の判定を省略する。
pub fn wall_is_seismic(data: &ElementData, model: &Model) -> bool {
    if wall_uses_steel(data, model) {
        return false;
    }
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
    if data.kind != ElementKind::Wall || !is_rc_wall(data, model) || !wall_is_seismic(data, model) {
        return None;
    }
    let g = super::wall_element_geometry(data, model)?;
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
        let cft_column = edges[2..]
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
        if mat.category != MaterialCategory::Concrete && !cft_column {
            return Some(format!(
                "耐震壁 ID {} はRC 造ですが、周辺架構の部材 ID {} の材料「{}」の区分は{}です。\
                 耐震壁と周辺架構の構造種別を揃えてください。\
                 壁エレメントは壁と周辺架構を一体の耐震要素としてモデル化するため、\
                 混合構造は扱えません。",
                data.id.0,
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
