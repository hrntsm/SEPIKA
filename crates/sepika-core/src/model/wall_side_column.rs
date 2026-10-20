//! 要素表現に依存しない壁側柱の物理分類。

use super::{
    wall_element_geometry, wall_is_seismic, ElementData, ElementKind, FrameSectionUse,
    MaterialCategory, Model,
};
use crate::geom::vec3::{cross, dot, unit};
use crate::ids::{ElemId, NodeId};
use crate::structure_kind::{shape_composite_kind, StructureKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallSideColumnRole {
    RcSrcReplacement,
    SteelPlateBoundary,
    InactiveWall,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallSideColumnReason {
    NotFrameMember,
    NotTwoNodes,
    MissingNode,
    NotVertical,
    NoMatchingWall,
    WallNotSeismic,
    InvalidWallGeometry,
    MissingMaterial,
    InvalidPrimaryMaterial,
    MissingSteelMaterial,
    InvalidSteelMaterial,
    ShapeUseMismatch,
    ShapeMaterialMismatch,
    ColumnUseRequired,
    GirderUseRequired,
    FrameMaterialMismatch,
    DuplicateColumns,
    NonParallelWalls,
    InvalidSteelYield,
}

/// 関連する全対象 ID を昇順で保持する。解析種別ごとの適用可否は利用側で判定する。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WallSideColumnIssue {
    pub reason: WallSideColumnReason,
    pub walls: Vec<ElemId>,
    pub members: Vec<ElemId>,
}

/// `side` は壁幾何の下辺 a 側が 0、b 側が 1。法線は下辺方向×鉛直方向。
#[derive(Clone, Debug, PartialEq)]
pub struct WallSideColumnBinding {
    pub wall: ElemId,
    pub column: ElemId,
    pub side: usize,
    pub edge: [NodeId; 2],
    pub normal: [f64; 3],
    pub wall_structure: Option<StructureKind>,
    pub column_structure: Option<StructureKind>,
    pub frame_use: Option<FrameSectionUse>,
    pub role: WallSideColumnRole,
    pub issues: Vec<WallSideColumnIssue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WallSideColumnClassification {
    pub column: ElemId,
    pub bindings: Vec<WallSideColumnBinding>,
    pub issues: Vec<WallSideColumnIssue>,
}

/// 成立しない壁や候補のない壁も、理由を失わず保持する。
#[derive(Clone, Debug, PartialEq)]
pub struct WallSideColumnWall {
    pub wall: ElemId,
    pub role: WallSideColumnRole,
    pub structure: Option<StructureKind>,
    pub issues: Vec<WallSideColumnIssue>,
}

#[derive(Clone, Debug, Default)]
pub struct WallSideColumnIndex {
    bindings: Vec<WallSideColumnBinding>,
    walls: Vec<WallSideColumnWall>,
}

fn issue(
    reason: WallSideColumnReason,
    walls: Vec<ElemId>,
    members: Vec<ElemId>,
) -> WallSideColumnIssue {
    WallSideColumnIssue {
        reason,
        walls,
        members,
    }
}

fn same_pair(nodes: &[NodeId], edge: [NodeId; 2]) -> bool {
    nodes.len() == 2
        && ((nodes[0] == edge[0] && nodes[1] == edge[1])
            || (nodes[0] == edge[1] && nodes[1] == edge[0]))
}

fn explicit_structure(
    data: &ElementData,
    model: &Model,
) -> Result<StructureKind, WallSideColumnReason> {
    let shape = model.element_section(data).and_then(|s| s.shape.as_ref());
    if let Some(composite) = shape.and_then(shape_composite_kind) {
        return Ok(composite);
    }
    let material = model
        .element_material(data)
        .ok_or(WallSideColumnReason::MissingMaterial)?;
    let kind = match material.category {
        MaterialCategory::Concrete => StructureKind::Rc,
        MaterialCategory::Steel => StructureKind::S,
        MaterialCategory::Rebar => return Err(WallSideColumnReason::InvalidPrimaryMaterial),
    };
    if shape.is_some_and(|s| s.is_concrete_like()) && kind != StructureKind::Rc {
        return Err(WallSideColumnReason::ShapeMaterialMismatch);
    }
    Ok(kind)
}

fn member_input_issues(
    data: &ElementData,
    model: &Model,
    wall: ElemId,
) -> Vec<WallSideColumnIssue> {
    use crate::section_shape::SectionShape;
    let Some(section) = model.element_section(data) else {
        return Vec::new();
    };
    let shape = section.shape.as_ref();
    let mut issues = Vec::new();
    let expected_use = match shape {
        Some(SectionShape::SrcBeamRect { .. }) => Some(FrameSectionUse::Girder),
        Some(
            SectionShape::SrcColumnRect { .. }
            | SectionShape::CftBox { .. }
            | SectionShape::CftPipe { .. },
        ) => Some(FrameSectionUse::Column),
        _ => section.rc_frame_use(),
    };
    if expected_use.is_some_and(|usage| section.frame_use != Some(usage)) {
        issues.push(issue(
            WallSideColumnReason::ShapeUseMismatch,
            vec![wall],
            vec![data.id],
        ));
    }
    if !shape.is_some_and(|s| shape_composite_kind(s).is_some()) {
        return issues;
    }
    match model.element_material(data) {
        None => issues.push(issue(
            WallSideColumnReason::MissingMaterial,
            vec![wall],
            vec![data.id],
        )),
        Some(m) if m.category != MaterialCategory::Concrete => issues.push(issue(
            WallSideColumnReason::ShapeMaterialMismatch,
            vec![wall],
            vec![data.id],
        )),
        _ => {}
    }
    match model.element_steel_material(data) {
        None => issues.push(issue(
            WallSideColumnReason::MissingSteelMaterial,
            vec![wall],
            vec![data.id],
        )),
        Some(m) if m.category != MaterialCategory::Steel => issues.push(issue(
            WallSideColumnReason::InvalidSteelMaterial,
            vec![wall],
            vec![data.id],
        )),
        _ => {}
    }
    issues
}

impl WallSideColumnIndex {
    /// 全壁・全一致候補を壁 ID、側縁、柱 ID の順に収集する。
    pub fn build(model: &Model) -> Self {
        let mut result = Self::default();
        let mut walls: Vec<_> = model
            .elements
            .iter()
            .filter(|e| e.kind == ElementKind::Wall)
            .collect();
        walls.sort_by_key(|e| e.id);
        let mut members: Vec<_> = model
            .elements
            .iter()
            .filter(|e| {
                e.kind.is_primary_frame_support() || matches!(e.kind, ElementKind::Brace { .. })
            })
            .collect();
        members.sort_by_key(|e| e.id);
        for wall in walls {
            let structure = explicit_structure(wall, model);
            let mut record = WallSideColumnWall {
                wall: wall.id,
                role: match structure {
                    Ok(StructureKind::Rc | StructureKind::Src) => {
                        WallSideColumnRole::RcSrcReplacement
                    }
                    Ok(StructureKind::S) => WallSideColumnRole::SteelPlateBoundary,
                    _ => WallSideColumnRole::Unresolved,
                },
                structure: structure.as_ref().ok().copied(),
                issues: Vec::new(),
            };
            if let Err(reason) = structure {
                record.issues.push(issue(reason, vec![wall.id], vec![]));
            }
            record
                .issues
                .extend(member_input_issues(wall, model, wall.id));
            if !wall_is_seismic(wall, model) {
                if record.role != WallSideColumnRole::Unresolved {
                    record.role = WallSideColumnRole::InactiveWall;
                }
                record.issues.push(issue(
                    WallSideColumnReason::WallNotSeismic,
                    vec![wall.id],
                    vec![],
                ));
            }
            if record.structure == Some(StructureKind::S)
                && model
                    .element_material(wall)
                    .is_some_and(|m| m.fy.is_none_or(|fy| !fy.is_finite() || fy <= 0.0))
            {
                record.issues.push(issue(
                    WallSideColumnReason::InvalidSteelYield,
                    vec![wall.id],
                    vec![],
                ));
            }
            let geometry = (wall.nodes.len() == 4)
                .then(|| wall_element_geometry(wall, model))
                .flatten();
            let Some(g) = geometry else {
                if record.role != WallSideColumnRole::Unresolved {
                    record.role = WallSideColumnRole::InactiveWall;
                }
                record.issues.push(issue(
                    WallSideColumnReason::InvalidWallGeometry,
                    vec![wall.id],
                    vec![],
                ));
                result.walls.push(record);
                continue;
            };
            let Some(normal) = unit(cross(g.ex_bottom, [0.0, 0.0, 1.0])) else {
                if record.role != WallSideColumnRole::Unresolved {
                    record.role = WallSideColumnRole::InactiveWall;
                }
                record.issues.push(issue(
                    WallSideColumnReason::InvalidWallGeometry,
                    vec![wall.id],
                    vec![],
                ));
                result.walls.push(record);
                continue;
            };
            for edge in [g.bottom, g.top] {
                for member in members.iter().filter(|e| same_pair(&e.nodes, edge)) {
                    if model.element_section(member).and_then(|s| s.frame_use)
                        != Some(FrameSectionUse::Girder)
                    {
                        record.issues.push(issue(
                            WallSideColumnReason::GirderUseRequired,
                            vec![wall.id],
                            vec![member.id],
                        ));
                    }
                    record
                        .issues
                        .extend(member_input_issues(member, model, wall.id));
                    match explicit_structure(member, model) {
                        Err(reason) => {
                            record
                                .issues
                                .push(issue(reason, vec![wall.id], vec![member.id]))
                        }
                        Ok(kind) if !frame_compatible(record.structure, kind, false) => {
                            record.issues.push(issue(
                                WallSideColumnReason::FrameMaterialMismatch,
                                vec![wall.id],
                                vec![member.id],
                            ))
                        }
                        _ => {}
                    }
                }
            }
            for side in 0..2 {
                let edge = [g.bottom[side], g.top[side]];
                let candidates: Vec<_> = members
                    .iter()
                    .filter(|e| same_pair(&e.nodes, edge))
                    .collect();
                for candidate in &candidates {
                    let column_structure = explicit_structure(candidate, model);
                    let frame_use = model.element_section(candidate).and_then(|s| s.frame_use);
                    let mut binding = WallSideColumnBinding {
                        wall: wall.id,
                        column: candidate.id,
                        side,
                        edge,
                        normal,
                        wall_structure: record.structure,
                        column_structure: column_structure.as_ref().ok().copied(),
                        frame_use,
                        role: record.role,
                        issues: record.issues.clone(),
                    };
                    binding
                        .issues
                        .extend(member_input_issues(candidate, model, wall.id));
                    if !candidate.kind.is_primary_frame_support() {
                        binding.issues.push(issue(
                            WallSideColumnReason::NotFrameMember,
                            vec![wall.id],
                            vec![candidate.id],
                        ));
                    }
                    if frame_use != Some(FrameSectionUse::Column) {
                        binding.issues.push(issue(
                            WallSideColumnReason::ColumnUseRequired,
                            vec![wall.id],
                            vec![candidate.id],
                        ));
                    }
                    let vertical = model
                        .nodes
                        .get(edge[0].index())
                        .zip(model.nodes.get(edge[1].index()))
                        .is_some_and(|(a, b)| crate::geom::is_vertical_axis(a.coord, b.coord));
                    if !vertical {
                        binding.issues.push(issue(
                            WallSideColumnReason::NotVertical,
                            vec![wall.id],
                            vec![candidate.id],
                        ));
                    }
                    match column_structure {
                        Err(reason) => {
                            binding
                                .issues
                                .push(issue(reason, vec![wall.id], vec![candidate.id]))
                        }
                        Ok(kind) if !frame_compatible(record.structure, kind, true) => {
                            binding.issues.push(issue(
                                WallSideColumnReason::FrameMaterialMismatch,
                                vec![wall.id],
                                vec![candidate.id],
                            ))
                        }
                        _ => {}
                    }
                    if candidates.len() > 1 {
                        binding.issues.push(issue(
                            WallSideColumnReason::DuplicateColumns,
                            vec![wall.id],
                            candidates.iter().map(|e| e.id).collect(),
                        ));
                    }
                    result.bindings.push(binding);
                }
            }
            result.walls.push(record);
        }
        let columns: std::collections::BTreeSet<_> =
            result.bindings.iter().map(|b| b.column).collect();
        for column in columns {
            let bindings: Vec<_> = result
                .bindings
                .iter()
                .filter(|b| {
                    b.column == column
                        && b.role == WallSideColumnRole::RcSrcReplacement
                        && b.issues.is_empty()
                })
                .collect();
            let nonparallel = bindings.iter().enumerate().any(|(i, a)| {
                bindings[i + 1..]
                    .iter()
                    .any(|b| dot(a.normal, b.normal).abs() < 1.0 - 1e-10)
            });
            if nonparallel {
                let walls = bindings.iter().map(|b| b.wall).collect();
                let valid_walls: Vec<_> = bindings.iter().map(|b| b.wall).collect();
                let conflict = issue(WallSideColumnReason::NonParallelWalls, walls, vec![column]);
                for binding in result
                    .bindings
                    .iter_mut()
                    .filter(|b| b.column == column && valid_walls.contains(&b.wall))
                {
                    binding.issues.push(conflict.clone());
                }
            }
        }
        result
    }

    pub fn walls(&self) -> &[WallSideColumnWall] {
        &self.walls
    }

    pub fn bindings(&self) -> &[WallSideColumnBinding] {
        &self.bindings
    }

    pub fn wall_bindings(&self, wall: ElemId) -> impl Iterator<Item = &WallSideColumnBinding> {
        self.bindings.iter().filter(move |b| b.wall == wall)
    }

    /// 有効な RC/SRC 置換壁の接続を返す。不正候補や非平行共有は関連 ID 付きで拒否する。
    pub fn replacement_bindings(
        &self,
        column: ElemId,
    ) -> Result<Vec<&WallSideColumnBinding>, Vec<WallSideColumnIssue>> {
        let bindings: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.column == column)
            .collect();
        let mut issues = Vec::new();
        for binding in &bindings {
            for entry in &binding.issues {
                let blocks = binding.role != WallSideColumnRole::InactiveWall
                    || matches!(
                        entry.reason,
                        WallSideColumnReason::MissingMaterial
                            | WallSideColumnReason::InvalidPrimaryMaterial
                            | WallSideColumnReason::ShapeMaterialMismatch
                            | WallSideColumnReason::MissingSteelMaterial
                            | WallSideColumnReason::InvalidSteelMaterial
                            | WallSideColumnReason::DuplicateColumns
                    );
                if blocks && !issues.contains(entry) {
                    issues.push(entry.clone());
                }
            }
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        Ok(bindings
            .into_iter()
            .filter(|b| b.role == WallSideColumnRole::RcSrcReplacement)
            .collect())
    }

    /// 壁断面へ算入する接続を返す。鋼板境界と非成立壁の側柱は算入しない。
    pub fn wall_replacement_bindings(
        &self,
        wall: ElemId,
    ) -> Result<Vec<&WallSideColumnBinding>, Vec<WallSideColumnIssue>> {
        let Some(record) = self.walls.iter().find(|w| w.wall == wall) else {
            return Ok(Vec::new());
        };
        if record.role == WallSideColumnRole::InactiveWall {
            return Ok(Vec::new());
        }
        if !record.issues.is_empty() {
            return Err(record.issues.clone());
        }
        let bindings: Vec<_> = self.wall_bindings(wall).collect();
        let mut issues = Vec::new();
        for binding in &bindings {
            if let Err(entries) = self.replacement_bindings(binding.column) {
                for entry in entries {
                    if !issues.contains(&entry) {
                        issues.push(entry);
                    }
                }
            }
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        if record.role == WallSideColumnRole::RcSrcReplacement {
            Ok(bindings)
        } else {
            Ok(Vec::new())
        }
    }

    /// 無関係な部材にも理由を返し、未確定・不正候補を非側柱へ読み替えない。
    pub fn classify(&self, column: &ElementData, model: &Model) -> WallSideColumnClassification {
        let bindings: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.column == column.id)
            .cloned()
            .collect();
        let mut issues = Vec::new();
        for binding in &bindings {
            for entry in &binding.issues {
                if !issues.contains(entry) {
                    issues.push(entry.clone());
                }
            }
        }
        if bindings.is_empty() {
            let reason = if !column.kind.is_primary_frame_support() {
                WallSideColumnReason::NotFrameMember
            } else if column.nodes.len() != 2 {
                WallSideColumnReason::NotTwoNodes
            } else {
                match model
                    .nodes
                    .get(column.nodes[0].index())
                    .zip(model.nodes.get(column.nodes[1].index()))
                {
                    None => WallSideColumnReason::MissingNode,
                    Some((a, b)) if !crate::geom::is_vertical_axis(a.coord, b.coord) => {
                        WallSideColumnReason::NotVertical
                    }
                    _ => WallSideColumnReason::NoMatchingWall,
                }
            };
            issues.push(issue(reason, vec![], vec![column.id]));
        }
        WallSideColumnClassification {
            column: column.id,
            bindings,
            issues,
        }
    }
}

fn frame_compatible(wall: Option<StructureKind>, member: StructureKind, column: bool) -> bool {
    match wall {
        Some(StructureKind::Rc | StructureKind::Src) => {
            matches!(member, StructureKind::Rc | StructureKind::Src)
                || (column && member == StructureKind::Cft)
        }
        Some(StructureKind::S) => member == StructureKind::S,
        _ => true,
    }
}

#[cfg(test)]
mod tests;
