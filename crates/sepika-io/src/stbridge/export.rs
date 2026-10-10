//! ST-Bridge 直列化（Export）。
//!
//! ST-Bridge 2.0.2 の幾何モデルを出力する。対応範囲は入出力ドキュメントを参照。
//! - 断面は標準要素（`StbSecColumn_S`/`StbSecBeam_RC` 等）＋形鋼ライブラリ `StbSecSteel`。
//! - 部材は複数形コンテナ（`StbColumns`/`StbGirders`/`StbBeams`/`StbBraces`/`StbSlabs`/
//!   `StbWalls`）に入れ、向きは `rotate`、端部は `condition_*`、ブレースは `feature_brace`。
//! - 材料は ST-Bridge の慣習どおり断面のグレード名（鋼 `strength_main`、RC/SRC/CFT の
//!   コンクリート `strength_concrete`）で表す（`StbModel` は材料テーブルを持たない）。
//! - 節点・原階は保存された外部 ID/GUID を用い、その他は内部 ID +1 で出力する。
//!
//! ST-Bridge の幾何スコープ外（材料の E/ν・節点荷重・拘束・独自属性）は往復しない。
//! 完全一致の往復が必要な場合はネイティブの `.ovika` を使う。
//!
//! - [`export_stbridge`] — 内部モデルを標準 ST-Bridge 2.0.2 XML 文字列へ出力する。
//! - [`fmt`] — 整数値は小数点なし、それ以外は既定の f64 表記で整形する（`pub(super)`）。
//! - [`esc`] — XML 特殊文字をエスケープする（`pub(super)`）。

use super::section_std::standard_sections;
use super::{StbError, STB_VERSION};
use sepika_core::ids::{NodeId, SectionId, SlabId};
use sepika_core::model::{
    AxisGroup, AxisGroupKind, ElementKind, EndCondition, Model, StoryLevelKind, StrengthTarget,
    WallPlateShape,
};

/// ST-Bridge の id は `positiveInteger`（1 以上）。内部 0 始まり id に +1 して出力する。
fn sid(internal_id: u32) -> u32 {
    internal_id + 1
}

fn node_sid(model: &Model, node: NodeId) -> u32 {
    model
        .stb_node_ids
        .iter()
        .find(|n| n.node == node)
        .map(|n| n.id)
        .unwrap_or_else(|| sid(node.0))
}

/// 二次部材の両端に一致するモデル節点。対応する節点が無ければ `None`
/// （ST-Bridge は材端を節点 ID で表すため）。
fn secondary_end_nodes(
    model: &Model,
    sm: &sepika_core::model::SecondaryMember,
) -> Option<[NodeId; 2]> {
    let (a, b) = model.secondary_member_end_points(sm)?;
    let tol = sepika_core::geom::MEMBER_AXIS_TOL_MM;
    let find = |p: [f64; 3]| {
        model
            .nodes
            .iter()
            .find(|n| sepika_core::geom::vec3::dist(n.coord, p) <= tol)
            .map(|n| n.id)
    };
    Some([find(a)?, find(b)?])
}

/// 内部モデルを標準 ST-Bridge 2.0.2 XML 文字列へ出力する（警告は破棄する）。
pub fn export_stbridge(model: &Model) -> Result<String, StbError> {
    export_stbridge_with_report(model).map(|(xml, _)| xml)
}

/// 内部モデルを標準 ST-Bridge 2.0.2 XML 文字列へ出力し、[`ExportReport`] も返す。
///
/// 標準スキーマの表現限界による近似・切り捨て（主筋の 4 段目以降、円形 RC 梁の
/// `StbSecRaw` フォールバックなど）は警告として報告する。
pub fn export_stbridge_with_report(model: &Model) -> Result<(String, ExportReport), StbError> {
    let mut output_model = model.clone();
    output_model
        .assign_stb_node_ids()
        .map_err(StbError::Unmappable)?;
    let model = &output_model;
    if model.stb_node_ids.iter().any(|n| n.id == 0) {
        return Err(StbError::Unmappable("STB節点IDは正整数が必要です".into()));
    }
    for input in &model.stb_strengths.members {
        if let StrengthTarget::Slab(id) = input.target {
            let boundary = model
                .slabs
                .iter()
                .find(|s| s.id == id)
                .and_then(|s| s.boundary_nodes(model));
            let represented = boundary.is_some_and(|nodes| nodes.contains(&input.node));
            let resolved = model.resolve_stb_concrete(input);
            if !represented
                && !resolved.is_ok_and(|r| {
                    matches!(
                        r.source,
                        sepika_core::model::StrengthSource::Member
                            | sepika_core::model::StrengthSource::Section
                    )
                })
            {
                return Err(StbError::Unmappable(format!(
                    "床 {} の元第1節点 {} に依存するFc省略を分割後の境界で表現できません",
                    id.0, input.node.0
                )));
            }
        }
    }
    for section in &model.stb_strengths.sections {
        for bar in &section.reinforcement {
            if model
                .resolve_stb_rebar(bar)
                .is_ok_and(|r| r.native_override)
            {
                let resolved = model.resolve_stb_rebar(bar).map_err(StbError::Unmappable)?;
                if sepika_core::standard_material::rebar_grade_strength(&resolved.grade)
                    != Some(resolved.value)
                {
                    return Err(StbError::Unmappable(format!(
                        "断面 {} のnative鉄筋物性を標準grade {} で表現できません",
                        section.section.0, resolved.grade
                    )));
                }
            }
        }
    }
    for section in &model.stb_strengths.sections {
        for steel in &section.steel {
            let resolved = model
                .resolve_stb_steel(steel)
                .map_err(StbError::Unmappable)?;
            if resolved.native_override
                && sepika_core::standard_material::standard_material_properties(&resolved.grade)
                    .filter(|p| {
                        p.fc.is_none()
                            && sepika_core::standard_material::rebar_grade_strength(&resolved.grade)
                                .is_none()
                    })
                    .and_then(|p| p.fy)
                    != Some(resolved.value)
            {
                return Err(StbError::Unmappable(format!(
                    "断面 {} の明示鋼材fyを標準grade {} で表現できません",
                    section.section.0, resolved.grade
                )));
            }
        }
    }
    let std = standard_sections(model)?;
    let mut warnings = std.warnings;
    warnings.extend(model.source_story_diagnostics());
    warnings.extend(model.source_story_assignment_diagnostics());
    let (sections_body, steel_lib, col_map, beam_map, brace_map) = (
        std.sections_xml,
        std.steel_lib,
        std.col_map,
        std.beam_map,
        std.brace_map,
    );

    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str(&format!(
        "<ST_BRIDGE xmlns=\"https://www.building-smart.or.jp/dl\" \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" version=\"{STB_VERSION}\">\n"
    ));

    if let Some(common) = &model.stb_strengths.common {
        s.push_str(&format!(
            "  <StbCommon project_name=\"{}\" app_name=\"{}\" app_version=\"{}\"{}",
            esc(&common.project_name),
            esc(&common.app_name),
            esc(&common.app_version),
            common
                .strength_concrete
                .as_ref()
                .map(|g| format!(" strength_concrete=\"{}\"", esc(g)))
                .unwrap_or_default()
        ));
        if common.reinforcement.is_empty() {
            s.push_str("/>\n");
        } else {
            s.push_str(">\n    <StbReinforcementStrengthList>\n");
            for r in &common.reinforcement {
                s.push_str(&format!(
                    "      <StbReinforcementStrength D=\"{}\" strength=\"{}\"/>\n",
                    esc(&r.diameter),
                    esc(&r.strength)
                ));
            }
            s.push_str("    </StbReinforcementStrengthList>\n  </StbCommon>\n");
        }
    } else {
        s.push_str(
            "  <StbCommon project_name=\"SEPIKA\" app_name=\"SEPIKA\" app_version=\"0.0.1\"/>\n",
        );
    }

    s.push_str("  <StbModel>\n");

    s.push_str("    <StbNodes>\n");
    for n in model
        .nodes
        .iter()
        .filter(|n| !model.generated_masters.contains(&n.id))
    {
        s.push_str(&format!(
            "      <StbNode id=\"{}\" X=\"{}\" Y=\"{}\" Z=\"{}\" kind=\"ON_GRID\"{} />\n",
            node_sid(model, n.id),
            fmt(n.coord[0]),
            fmt(n.coord[1]),
            fmt(n.coord[2]),
            model
                .stb_node_ids
                .iter()
                .find(|i| i.node == n.id)
                .and_then(|i| i.guid.as_ref())
                .map(|g| format!(" guid=\"{}\"", esc(g)))
                .unwrap_or_default(),
        ));
    }
    s.push_str("    </StbNodes>\n");

    s.push_str(&axes_body(model));

    let export_native_stories =
        !model.source_stories_initialized && model.source_stories.is_empty();
    let has_stories = if export_native_stories {
        !model.stories.is_empty()
    } else {
        !model.source_stories.is_empty()
    };
    if has_stories {
        s.push_str("    <StbStories>\n");
        if export_native_stories {
            for st in &model.stories {
                let mut members: Vec<u32> = model
                    .nodes
                    .iter()
                    .filter(|n| n.story == Some(st.id) && !model.generated_masters.contains(&n.id))
                    .map(|n| n.id.0)
                    .collect();
                for nid in &st.node_ids {
                    if !model.generated_masters.contains(nid) && !members.contains(&nid.0) {
                        members.push(nid.0);
                    }
                }
                members.sort_unstable();
                s.push_str(&format!(
                    "      <StbStory id=\"{}\" name=\"{}\" height=\"{}\" kind=\"{}\">\n",
                    sid(st.id.0),
                    esc(&st.name),
                    fmt(st.elevation),
                    story_kind(st.level_kind),
                ));
                if !members.is_empty() {
                    s.push_str("        <StbNodeIdList>\n");
                    for nid in members {
                        s.push_str(&format!(
                            "          <StbNodeId id=\"{}\"/>\n",
                            node_sid(model, NodeId(nid))
                        ));
                    }
                    s.push_str("        </StbNodeIdList>\n");
                }
                s.push_str("      </StbStory>\n");
            }
        } else {
            for story in &model.source_stories {
                if story.id == 0 {
                    return Err(StbError::Unmappable("STB原階IDは正整数が必要です".into()));
                }
                let mut attributes = String::new();
                if let Some(guid) = &story.guid {
                    attributes.push_str(&format!(" guid=\"{}\"", esc(guid)));
                }
                if let Some(id) = story.id_dependence {
                    attributes.push_str(&format!(" id_dependence=\"{}\"", id));
                }
                if let Some(fc) = &story.strength_concrete {
                    attributes.push_str(&format!(" strength_concrete=\"{}\"", esc(fc)));
                }
                s.push_str(&format!(
                    "      <StbStory id=\"{}\" name=\"{}\" height=\"{}\" kind=\"{}\"{}>\n",
                    story.id,
                    esc(&story.name),
                    fmt(story.height),
                    story.kind.as_str(),
                    attributes
                ));
                if !story.node_ids.is_empty() {
                    s.push_str("        <StbNodeIdList>\n");
                    for reference in &story.node_ids {
                        s.push_str(&format!("          <StbNodeId id=\"{}\"/>\n", reference.id));
                    }
                    s.push_str("        </StbNodeIdList>\n");
                }
                s.push_str("      </StbStory>\n");
            }
        }
        s.push_str("    </StbStories>\n");
    }

    s.push_str("    <StbMembers>\n");
    s.push_str(&members_body(model, &col_map, &beam_map, &brace_map)?);
    s.push_str("    </StbMembers>\n");

    let slab_sec_base = slab_section_id_base(model, &col_map, &beam_map);
    let wall_sec_base = slab_sec_base + model.floor_regions.len() as u32;

    s.push_str("    <StbSections>\n");
    s.push_str(&sections_body);
    s.push_str(&slab_sections(model, slab_sec_base));
    s.push_str(&wall_sections(model, wall_sec_base));
    s.push_str(&steel_lib);
    s.push_str("    </StbSections>\n");

    s.push_str("    <StbJoints/>\n");
    s.push_str("  </StbModel>\n");
    s.push_str("</ST_BRIDGE>\n");
    Ok((s, ExportReport { warnings }))
}

/// 書き出し時に標準スキーマの表現限界で近似・切り捨てが生じた内容の報告。
#[derive(Debug, Default, Clone)]
pub struct ExportReport {
    /// 人間可読の警告メッセージ（段数超過による切り捨て、円形 RC 梁のフォールバックなど）。
    pub warnings: Vec<String>,
}

impl ExportReport {
    /// 警告が 1 件もないか。
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }
}

/// スラブ断面 id の採番開始値。既存断面 id（柱・梁。柱/梁の役割分割で
/// 増える分は col_map/beam_map の値域に現れる）と衝突しない範囲から採る。
/// `StbSections`（断面定義側）と `StbMembers`（スラブの断面参照側）が同じ
/// 採番を共有するための単一実装。
fn slab_section_id_base(
    model: &Model,
    col_map: &std::collections::HashMap<u32, u32>,
    beam_map: &std::collections::HashMap<u32, u32>,
) -> u32 {
    col_map
        .values()
        .chain(beam_map.values())
        .copied()
        .max()
        .map(|m| m + 1)
        .unwrap_or(0)
        .max(model.sections.len() as u32)
}

/// `StbMembers` 本体（柱・大梁・ブレース・スラブ・壁を複数形コンテナに束ねる）。
fn members_body(
    model: &Model,
    col_map: &std::collections::HashMap<u32, u32>,
    beam_map: &std::collections::HashMap<u32, u32>,
    brace_map: &std::collections::HashMap<u32, u32>,
) -> Result<String, StbError> {
    let mut columns = String::new();
    let mut girders = String::new();
    let mut unexported_secondary_ids: Vec<u32> = Vec::new();
    let mut braces = String::new();

    for e in &model.elements {
        match e.kind {
            ElementKind::Beam if e.nodes.len() == 2 => {
                let n0 = &model.nodes[e.nodes[0].index()];
                let n1 = &model.nodes[e.nodes[1].index()];
                let is_col = model
                    .element_section(e)
                    .and_then(|section| section.frame_use)
                    == Some(sepika_core::model::FrameSectionUse::Column);
                let role_map = if is_col { col_map } else { beam_map };
                let sec = e
                    .section
                    .map(|s| role_map.get(&s.0).copied().unwrap_or(s.0))
                    .map(|v| v as i64)
                    .unwrap_or(-1);
                let rot = rotate_of(e, n0.coord, n1.coord);
                let ks = kind_structure(model, e);
                if is_col {
                    let (bot, top) = if model
                        .stb_strengths
                        .members
                        .iter()
                        .any(|m| m.target == StrengthTarget::Element(e.id))
                        || n0.coord[2] <= n1.coord[2]
                    {
                        (e.nodes[0], e.nodes[1])
                    } else {
                        (e.nodes[1], e.nodes[0])
                    };
                    let (cb, ct) = if model
                        .stb_strengths
                        .members
                        .iter()
                        .any(|m| m.target == StrengthTarget::Element(e.id))
                        || n0.coord[2] <= n1.coord[2]
                    {
                        (e.end_cond[0], e.end_cond[1])
                    } else {
                        (e.end_cond[1], e.end_cond[0])
                    };
                    columns.push_str(&format!(
                        "        <StbColumn id=\"{}\" name=\"C{}\" id_node_bottom=\"{}\" id_node_top=\"{}\" \
                         rotate=\"{}\" id_section=\"{}\" kind_structure=\"{}\" condition_bottom=\"{}\" condition_top=\"{}\"{}/>\n",
                        sid(e.id.0), sid(e.id.0), node_sid(model, bot), node_sid(model, top),
                        fmt(rot), sec_ref(sec), ks, cond(cb), cond(ct), super::strength_export::member_attr(model,StrengthTarget::Element(e.id)),
                    ));
                } else {
                    girders.push_str(&format!(
                        "        <StbGirder id=\"{}\" name=\"G{}\" id_node_start=\"{}\" id_node_end=\"{}\" \
                         rotate=\"{}\" id_section=\"{}\" kind_structure=\"{}\" isFoundation=\"false\" \
                         condition_start=\"{}\" condition_end=\"{}\"{}/>\n",
                        sid(e.id.0), sid(e.id.0), node_sid(model, e.nodes[0]), node_sid(model, e.nodes[1]),
                        fmt(rot), sec_ref(sec), ks, cond(e.end_cond[0]), cond(e.end_cond[1]), super::strength_export::member_attr(model,StrengthTarget::Element(e.id)),
                    ));
                }
            }
            ElementKind::Brace { tension_only } if e.nodes.len() == 2 => {
                let sec = e
                    .section
                    .map(|s| brace_map.get(&s.0).copied().unwrap_or(s.0) as i64)
                    .unwrap_or(-1);
                let feature = if tension_only {
                    "TENSION"
                } else {
                    "TENSIONANDCOMPRESSION"
                };
                braces.push_str(&format!(
                    "        <StbBrace id=\"{}\" name=\"BR{}\" id_node_start=\"{}\" id_node_end=\"{}\" \
                     rotate=\"0\" id_section=\"{}\" kind_structure=\"S\" feature_brace=\"{}\" \
                     condition_start=\"PIN\" condition_end=\"PIN\"/>\n",
                    sid(e.id.0), sid(e.id.0), node_sid(model, e.nodes[0]), node_sid(model, e.nodes[1]),
                    sec_ref(sec), feature,
                ));
            }
            _ => {}
        }
    }

    let secondary_member_base = model.elements.len() as u32;
    let mut sec_beams = String::new();
    let mut posts = String::new();
    let all_secondaries: Vec<_> = model.beams().chain(model.posts()).collect();
    for (i, sm) in all_secondaries.iter().enumerate() {
        let mid = secondary_member_base + i as u32;
        let sec = sm
            .section
            .map(|s| {
                let role_map = match sm.kind {
                    sepika_core::model::SecondaryMemberKind::Beam => beam_map,
                    sepika_core::model::SecondaryMemberKind::Post => col_map,
                };
                role_map.get(&s.0).copied().unwrap_or(s.0) as i64
            })
            .unwrap_or(-1);
        let ks = model
            .secondary_material(sm)
            .map(|m| {
                if m.fc.is_some() {
                    "RC".to_string()
                } else {
                    "S".to_string()
                }
            })
            .unwrap_or_else(|| "S".to_string());
        let Some(nodes) = secondary_end_nodes(model, sm) else {
            unexported_secondary_ids.push(sm.id.0);
            continue;
        };
        match sm.kind {
            sepika_core::model::SecondaryMemberKind::Beam => {
                sec_beams.push_str(&format!(
                    "        <StbBeam id=\"{}\" name=\"B{}\" id_node_start=\"{}\" id_node_end=\"{}\" \
                     rotate=\"0\" id_section=\"{}\" kind_structure=\"{}\" isFoundation=\"false\"{}/>\n",
                    sid(mid), sid(mid), node_sid(model, nodes[0]), node_sid(model, nodes[1]), sec_ref(sec), ks, super::strength_export::member_attr(model,StrengthTarget::Secondary(sm.id)),
                ));
            }
            sepika_core::model::SecondaryMemberKind::Post => {
                let n0 = &model.nodes[nodes[0].index()];
                let n1 = &model.nodes[nodes[1].index()];
                let (bot, top) = if model
                    .stb_strengths
                    .members
                    .iter()
                    .any(|m| m.target == StrengthTarget::Secondary(sm.id))
                    || n0.coord[2] <= n1.coord[2]
                {
                    (nodes[0], nodes[1])
                } else {
                    (nodes[1], nodes[0])
                };
                posts.push_str(&format!(
                    "        <StbPost id=\"{}\" name=\"P{}\" id_node_bottom=\"{}\" id_node_top=\"{}\" \
                     rotate=\"0\" id_section=\"{}\" kind_structure=\"{}\"{}/>\n",
                    sid(mid), sid(mid), node_sid(model, bot), node_sid(model, top), sec_ref(sec), ks, super::strength_export::member_attr(model,StrengthTarget::Secondary(sm.id)),
                ));
            }
        }
    }

    let slab_member_base = model.elements.len() as u32 + all_secondaries.len() as u32;
    let slab_sec_base = slab_section_id_base(model, col_map, beam_map);
    let slab_sec_ids = slab_section_ids(model, slab_sec_base);
    let mut slabs = String::new();
    for slab in &model.slabs {
        let Some(mut boundary) = slab.boundary_nodes(model) else {
            continue;
        };
        if let Some(input) = model
            .stb_strengths
            .members
            .iter()
            .find(|m| m.target == StrengthTarget::Slab(slab.id))
        {
            if input.node_order.len() == boundary.len()
                && input.node_order.iter().all(|n| boundary.contains(n))
            {
                boundary = input.node_order.clone();
            }
            if let Some(first) = boundary.iter().position(|node| *node == input.node) {
                boundary.rotate_left(first);
            }
        }
        let mid = slab_member_base + slab.id.0;
        let sec = slab_sec_ids.get(&slab.id).copied().unwrap_or(slab_sec_base);
        let order = boundary
            .iter()
            .map(|n| node_sid(model, *n).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let kind_slab = "NORMAL";
        slabs.push_str(&format!(
            "        <StbSlab id=\"{}\" name=\"S{}\" id_section=\"{}\" kind_structure=\"RC\" kind_slab=\"{}\" isFoundation=\"false\"{}>\n",
            sid(mid),
            sid(slab.id.0),
            sid(sec),
            kind_slab,
            super::strength_export::member_attr(model,StrengthTarget::Slab(slab.id)),
        ));
        slabs.push_str(&format!(
            "          <StbNodeIdOrder>{order}</StbNodeIdOrder>\n"
        ));
        slabs.push_str("        </StbSlab>\n");
    }

    let wall_member_base = slab_member_base + model.slabs.len() as u32;
    let wall_sec_base = slab_sec_base + model.floor_regions.len() as u32;
    let stb_walls = stb_walls_for_export(model);
    let mut walls = String::new();
    for (wall_idx, wall) in stb_walls.iter().enumerate() {
        let order = wall
            .nodes
            .iter()
            .map(|n| node_sid(model, *n).to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let mid = wall_member_base + wall_idx as u32;
        let sec = wall_sec_base + wall_idx as u32;
        walls.push_str(&format!(
            "        <StbWall id=\"{}\" name=\"W{}\" id_section=\"{}\" kind_structure=\"RC\"{}>\n",
            sid(mid),
            sid(mid),
            sid(sec),
            super::strength_export::member_attr(
                model,
                StrengthTarget::Wall(sepika_core::ids::WallPlateId(wall.id))
            ),
        ));
        walls.push_str(&format!(
            "          <StbNodeIdOrder>{order}</StbNodeIdOrder>\n"
        ));
        walls.push_str("        </StbWall>\n");
    }

    let mut body = String::new();
    if !columns.is_empty() {
        body.push_str("      <StbColumns>\n");
        body.push_str(&columns);
        body.push_str("      </StbColumns>\n");
    }
    if !posts.is_empty() {
        body.push_str("      <StbPosts>\n");
        body.push_str(&posts);
        body.push_str("      </StbPosts>\n");
    }
    if !girders.is_empty() {
        body.push_str("      <StbGirders>\n");
        body.push_str(&girders);
        body.push_str("      </StbGirders>\n");
    }
    if !sec_beams.is_empty() {
        body.push_str("      <StbBeams>\n");
        body.push_str(&sec_beams);
        body.push_str("      </StbBeams>\n");
    }
    if !braces.is_empty() {
        body.push_str("      <StbBraces>\n");
        body.push_str(&braces);
        body.push_str("      </StbBraces>\n");
    }
    if !slabs.is_empty() {
        body.push_str("      <StbSlabs>\n");
        body.push_str(&slabs);
        body.push_str("      </StbSlabs>\n");
    }
    if !walls.is_empty() {
        body.push_str("      <StbWalls>\n");
        body.push_str(&walls);
        body.push_str("      </StbWalls>\n");
    }
    if !unexported_secondary_ids.is_empty() {
        return Err(StbError::SecondaryWithoutNode(format!(
            "{} 本（SM{}）。ST-Bridge は材端を節点 ID で表すため、材軸中間へアンカーした\
             二次部材は書き出せません。節点を持つ位置へアンカーし直してください。",
            unexported_secondary_ids.len(),
            unexported_secondary_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", SM")
        )));
    }
    Ok(body)
}

/// 断面参照属性値。負（未参照）は -1、そうでなければ +1 した positiveInteger。
fn sec_ref(sec_internal: i64) -> String {
    if sec_internal < 0 {
        "-1".to_string()
    } else {
        format!("{}", sec_internal as u32 + 1)
    }
}

/// 端部接合条件（FIX/PIN）。
fn cond(c: EndCondition) -> &'static str {
    match c {
        EndCondition::Pinned => "PIN",
        _ => "FIX",
    }
}

/// 通り芯（`StbAxes`）。
///
/// 書き出せるのは平行芯（[`AxisGroupKind::Parallel`]）のグループのみ。円弧芯・
/// 放射芯・作図芯に相当する [`AxisGroupKind::Other`] のグループは幾何を保持して
/// いないため出力せず、往復しない（取り込みでは所属節点だけを保つ）。
///
/// `StbParallelAxis` の `id` は ST-Bridge の `positiveInteger`。内部の通り芯は id を
/// 持たないため、グループをまたいで 1 から通し番号を振る。
fn axes_body(model: &Model) -> String {
    let mut s = String::new();
    let groups: Vec<&AxisGroup> = model
        .axes
        .iter()
        .filter(|g| matches!(g.kind, AxisGroupKind::Parallel { .. }))
        .collect();
    if groups.is_empty() {
        return s;
    }
    s.push_str("    <StbAxes>\n");
    let mut next_id = 1u32;
    for g in groups {
        let AxisGroupKind::Parallel { origin, angle_deg } = g.kind else {
            continue;
        };
        s.push_str(&format!(
            "      <StbParallelAxes group_name=\"{}\" X=\"{}\" Y=\"{}\" angle=\"{}\">\n",
            esc(&g.name),
            fmt(origin[0]),
            fmt(origin[1]),
            fmt(angle_deg),
        ));
        for ax in &g.axes {
            s.push_str(&format!(
                "        <StbParallelAxis id=\"{}\" name=\"{}\" distance=\"{}\">\n",
                next_id,
                esc(&ax.name),
                fmt(ax.distance.unwrap_or(0.0)),
            ));
            next_id += 1;
            if !ax.nodes.is_empty() {
                s.push_str("          <StbNodeIdList>\n");
                for n in &ax.nodes {
                    s.push_str(&format!(
                        "            <StbNodeId id=\"{}\"/>\n",
                        node_sid(model, *n)
                    ));
                }
                s.push_str("          </StbNodeIdList>\n");
            }
            s.push_str("        </StbParallelAxis>\n");
        }
        s.push_str("      </StbParallelAxes>\n");
    }
    s.push_str("    </StbAxes>\n");
    s
}

/// 層種別を ST-Bridge の `kind`（GENERAL/PENTHOUSE/BASEMENT）へ写す。
fn story_kind(k: StoryLevelKind) -> &'static str {
    match k {
        StoryLevelKind::Penthouse { .. } => "PENTHOUSE",
        StoryLevelKind::Basement { .. } => "BASEMENT",
        StoryLevelKind::Normal => "GENERAL",
    }
}

/// 部材の構造種別（`kind_structure`）。
///
/// 判定は [`sepika_core::structure_kind::member_structure_kind`] に委ね、
/// ラベル（RC / S / SRC / CFT）をそのまま ST-Bridge の属性値として書き出す。
fn kind_structure(
    model: &sepika_core::model::Model,
    e: &sepika_core::model::ElementData,
) -> &'static str {
    sepika_core::structure_kind::member_structure_kind(model, e).label()
}

/// 部材の ref_vector と軸から `rotate` 角 [deg] を復元する（import の逆変換）。
/// `rotate=0` の基準（水平材は鉛直上、鉛直材はグローバル X）に対する軸まわりの回転角。
fn rotate_of(e: &sepika_core::model::ElementData, p_i: [f64; 3], p_j: [f64; 3]) -> f64 {
    let axis = {
        let d = [p_j[0] - p_i[0], p_j[1] - p_i[1], p_j[2] - p_i[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l < 1e-9 {
            return 0.0;
        }
        [d[0] / l, d[1] / l, d[2] / l]
    };
    let base = if axis[2].abs() > 0.99 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let bdot = base[0] * axis[0] + base[1] * axis[1] + base[2] * axis[2];
    let ref0 = normalize([
        base[0] - bdot * axis[0],
        base[1] - bdot * axis[1],
        base[2] - bdot * axis[2],
    ]);
    let r = e.local_axis.ref_vector;
    let rdot = r[0] * axis[0] + r[1] * axis[1] + r[2] * axis[2];
    let refv = normalize([
        r[0] - rdot * axis[0],
        r[1] - rdot * axis[1],
        r[2] - rdot * axis[2],
    ]);
    let cross = [
        ref0[1] * refv[2] - ref0[2] * refv[1],
        ref0[2] * refv[0] - ref0[0] * refv[2],
        ref0[0] * refv[1] - ref0[1] * refv[0],
    ];
    let sin = cross[0] * axis[0] + cross[1] * axis[1] + cross[2] * axis[2];
    let cos = ref0[0] * refv[0] + ref0[1] * refv[1] + ref0[2] * refv[2];
    if sin.abs() < 1e-9 && cos.abs() < 1e-9 {
        return 0.0;
    }
    sin.atan2(cos).to_degrees()
}

/// 単位ベクトル。縮退したベクトル（長さ 0）は方向を決められないため、
/// ST-Bridge の既定の参照方向として鉛直上向きを返す。
fn normalize(v: [f64; 3]) -> [f64; 3] {
    sepika_core::geom::vec3::unit(v).unwrap_or([0.0, 0.0, 1.0])
}

fn exports_stb_slab(model: &Model, slab: &sepika_core::model::Slab) -> bool {
    slab.boundary_nodes(model).is_some()
}

/// 各スラブが参照する `StbSecSlab_RC` の id を決める。
///
/// **同じ内部断面を指すスラブは 1 つの ST-Bridge 断面を共有する**。スラブごとに
/// 断面を出すと、断面を共有する床が N 枚あるモデルで同名の断面が N 個並び、
/// 再取り込みのたびに符号が `S15`・`S15#2`… と増殖する。
/// 断面が未割当のスラブは、そのスラブ専用の id を後ろへ割り当てる。
///
/// 割り当てる id は `base` から連番で、総数はスラブ枚数を超えない
/// （呼び出し側が `base + slabs.len()` を壁断面の開始値として予約している）。
/// `StbSections`（断面定義側）と `StbMembers`（スラブの参照側）が同じ採番を
/// 共有するための単一実装。
fn slab_section_ids(model: &Model, base: u32) -> std::collections::HashMap<SlabId, u32> {
    let mut shared: std::collections::HashMap<SectionId, u32> = std::collections::HashMap::new();
    let mut out: std::collections::HashMap<SlabId, u32> = std::collections::HashMap::new();
    let mut next = base;
    for slab in &model.slabs {
        if !exports_stb_slab(model, slab) {
            continue;
        }
        let id = match slab.section() {
            Some(sec) => *shared.entry(sec).or_insert_with(|| {
                let v = next;
                next += 1;
                v
            }),
            None => {
                let v = next;
                next += 1;
                v
            }
        };
        out.insert(slab.id, id);
    }
    out
}

/// スラブ断面（`StbSecSlab_RC`）ブロックを生成する。
///
/// 符号・階・板厚・コンクリート材料はいずれも**スラブへ割り当てた断面**から取り、
/// 同じ断面を指すスラブは 1 つのブロックを共有する（[`slab_section_ids`]）。
/// 断面が未割当のスラブは符号を `S{スラブID}`、板厚を建物一律の
/// `model.slab_thickness` として出力する（解析前チェックが止める状態だが、
/// 書き出し自体は不完全なモデルでも通す）。
fn slab_sections(model: &Model, base: u32) -> String {
    let ids = slab_section_ids(model, base);
    let mut body = String::new();
    let mut written: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for slab in &model.slabs {
        if !exports_stb_slab(model, slab) {
            continue;
        }
        let Some(&s) = ids.get(&slab.id) else {
            continue;
        };
        if !written.insert(s) {
            continue;
        }
        let sec = model.slab_section(slab);
        let t = model
            .slab_plate_thickness(slab)
            .unwrap_or(model.slab_thickness);
        let name = sec
            .map(|sc| sc.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("S{}", sid(slab.id.0)));
        body.push_str(&format!(
            "      <StbSecSlab_RC id=\"{}\" name=\"{}\"{}{}>\n",
            sid(s),
            esc(&name),
            sec.map(slab_floor_attr).unwrap_or_default(),
            sec.map(|sc| concrete_attr(model, sc)).unwrap_or_default(),
        ));
        body.push_str("        <StbSecFigureSlab_RC>\n");
        body.push_str(&format!(
            "          <StbSecSlab_RC_Straight depth=\"{}\"/>\n",
            fmt(t),
        ));
        body.push_str("        </StbSecFigureSlab_RC>\n");
        body.push_str("      </StbSecSlab_RC>\n");
    }
    body
}

/// 壁断面（`StbSecWall_RC`）ブロックを生成する。書き出す壁ごとに 1 つの断面を出力し、
/// 厚さとコンクリート材料は壁版（またはシェル）の断面から取る（厚さの未設定は 0）。
fn wall_sections(model: &Model, base: u32) -> String {
    let mut body = String::new();
    for (idx, wall) in stb_walls_for_export(model).iter().enumerate() {
        let s = sid(base + idx as u32);
        let t = wall
            .section
            .and_then(|sc| model.sections.get(sc.index()))
            .and_then(|sc| sc.thickness)
            .unwrap_or(0.0);
        let sec = wall.section.and_then(|sc| model.sections.get(sc.index()));
        body.push_str(&format!(
            "      <StbSecWall_RC id=\"{}\" name=\"{}\"{}>\n",
            s,
            esc(&format!("W{}", sid(wall.id))),
            sec.map(|sc| concrete_attr(model, sc)).unwrap_or_default(),
        ));
        body.push_str("        <StbSecFigureWall_RC>\n");
        body.push_str(&format!(
            "          <StbSecWall_RC_Straight thickness=\"{}\"/>\n",
            fmt(t),
        ));
        body.push_str("        </StbSecFigureWall_RC>\n");
        body.push_str("      </StbSecWall_RC>\n");
    }
    body
}

/// ST-Bridge へ出す壁 1 件（囲まれた壁版、またはシェル要素）。
struct StbWallOut {
    id: u32,
    nodes: Vec<NodeId>,
    section: Option<SectionId>,
}

/// 囲まれた壁版（3 節点以上）とシェル要素を、書き出し順で列挙する。
///
/// 解析用の壁要素は生成物のためここには出さない。4 節点でない壁版も
/// 入力の正として往復させる（解析要素にはしない。診断が警告する）。
fn stb_walls_for_export(model: &Model) -> Vec<StbWallOut> {
    let mut out = Vec::new();
    for plate in &model.wall_plates {
        if !matches!(plate.shape, WallPlateShape::Enclosed) {
            continue;
        }
        let Some(mut boundary) = plate.boundary_nodes(model) else {
            continue;
        };
        if boundary.len() < 3 {
            continue;
        }
        if let Some(input) = model
            .stb_strengths
            .members
            .iter()
            .find(|m| m.target == StrengthTarget::Wall(plate.id))
        {
            if input.node_order.len() == boundary.len()
                && input.node_order.iter().all(|n| boundary.contains(n))
            {
                boundary = input.node_order.clone();
            }
        }
        out.push(StbWallOut {
            id: plate.id.0,
            nodes: boundary,
            section: plate.section,
        });
    }
    for e in &model.elements {
        if e.kind != ElementKind::Shell || e.nodes.len() < 3 {
            continue;
        }
        out.push(StbWallOut {
            id: e.id.0,
            nodes: e.nodes.iter().copied().collect(),
            section: e.section,
        });
    }
    out
}

pub(super) fn fmt(x: f64) -> String {
    if x == x.trunc() && x.is_finite() {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

pub(super) fn esc(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|&c| c == '\t' || c == '\n' || c == '\r' || (c as u32) >= 0x20)
        .collect();
    cleaned
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\t', "&#9;")
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
}

/// 断面の `floor` を ST-Bridge の `floor` 属性へ（未設定は属性ごと省く）。
fn slab_floor_attr(sec: &sepika_core::model::Section) -> String {
    match &sec.floor {
        Some(f) => format!(" floor=\"{}\"", esc(f)),
        None => String::new(),
    }
}

/// 断面の主材料の名前を `strength_concrete` 属性へ（未割当は属性ごと省く）。
fn concrete_attr(model: &Model, sec: &sepika_core::model::Section) -> String {
    if let Some(input) = model
        .stb_strengths
        .sections
        .iter()
        .find(|s| s.section == sec.id)
    {
        return input
            .concrete
            .as_ref()
            .map(|g| format!(" strength_concrete=\"{}\"", esc(g)))
            .unwrap_or_default();
    }
    match sec
        .material
        .and_then(|mid| model.materials.get(mid.index()))
        .map(|m| m.name.as_str())
        .filter(|n| !n.is_empty())
    {
        Some(name) => format!(" strength_concrete=\"{}\"", esc(name)),
        None => String::new(),
    }
}
