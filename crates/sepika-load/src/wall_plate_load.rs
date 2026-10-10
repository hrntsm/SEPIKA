//! 要素にならない囲まれた壁版の自重を、明示された境界辺の負担率で分配する。
//!
//! 各辺の支持先は、床板と同じく壁版割当領域の境界
//! （[`sepika_core::model::SupportBoundary`]）を正本とする。境界の頂点にモデル節点が
//! 無くても（間柱端が梁中間にある場合など）支持部材と材軸区間をそのまま使える。

use std::collections::HashMap;

use sepika_core::geom::MEMBER_AXIS_TOL_MM;
use sepika_core::ids::{NodeId, SecondaryMemberId, WallPlateId};
use sepika_core::model::{
    MemberLoadKind, Model, NodalLoad, SupportMemberId, WallPlate, WallPlateShape,
};

use crate::cascade::SecondaryKey;

use crate::floor::{fem_uniform, BeamLoad, LoadShape, LoadTarget};

/// 1 枚の壁版の自重のうち、1 つの境界辺が受け持つぶん。
#[derive(Clone, Copy, Debug)]
pub struct WallEdgeShare {
    /// 辺が載る支持部材。
    pub support: SupportMemberId,
    /// 支持部材材軸上の無次元区間（割当領域の境界が持つ値）。
    pub span: [f64; 2],
    /// この辺が受け持つ重量 [N]（下向きを正）。
    pub total: f64,
}

impl WallEdgeShare {
    /// 受け手が間柱ならその安定 ID。主架構（柱・梁）なら `None`。
    pub fn post(&self) -> Option<SecondaryMemberId> {
        match self.support {
            SupportMemberId::Secondary(key) => Some(key),
            SupportMemberId::Primary(_) => None,
        }
    }
}

/// 間柱 1 本が壁版から受け持つ荷重。
#[derive(Clone, Debug)]
pub struct PostWallLoad {
    /// 材軸局所の部材荷重（下向きを正。原点は間柱の材軸始端）。
    pub member_loads: Vec<MemberLoadKind>,
}

/// 要素にならない壁版の自重の分配結果。
#[derive(Clone, Debug, Default)]
pub struct EnclosedWallLoads {
    /// 間柱が受け持つ荷重（間柱の安定 ID キー）。
    pub posts: HashMap<SecondaryKey, PostWallLoad>,
    /// 主架構（柱・大梁）が受け持つ辺荷重。床板の分配と同じ幾何解決
    /// （`sepika-job::auto_loads::slab_load_case_content`）へ合流させる。
    pub primary: Vec<BeamLoad>,
}

/// 辺が鉛直か（水平投影が許容差以下）。
fn is_vertical(a: [f64; 3], b: [f64; 3]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (dx * dx + dy * dy).sqrt() <= MEMBER_AXIS_TOL_MM && (b[2] - a[2]).abs() > MEMBER_AXIS_TOL_MM
}

/// 辺が水平か（鉛直方向の差が許容差以下）。
fn is_horizontal(a: [f64; 3], b: [f64; 3]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    (b[2] - a[2]).abs() <= MEMBER_AXIS_TOL_MM && (dx * dx + dy * dy).sqrt() > MEMBER_AXIS_TOL_MM
}

/// 境界の辺ごとに、耐震スリットで縁が切れているかを返す（`boundary` と同じ並び）。
/// 指定した柱際・梁際スリットをすべて境界辺へ対応付ける。
/// 指定した役割を解決できない辺がある場合は `None` を返す。
fn slit_edge_flags(
    model: &Model,
    plate: &WallPlate,
    boundary: &[NodeId],
    coords: &[[f64; 3]],
) -> Option<Vec<bool>> {
    let n = boundary.len();
    let mut out = vec![false; n];
    if n != 4 || !plate.slit.any() {
        return (!plate.slit.any()).then_some(out);
    }
    let mut column_resolved = [false; 2];
    let mut beam_resolved = [false; 2];
    let faces = plate.column_face_nodes(model);
    let mid_z = |i: usize| (coords[i][2] + coords[(i + 1) % n][2]) / 2.0;
    let horizontal: Vec<usize> = (0..n)
        .filter(|&i| is_horizontal(coords[i], coords[(i + 1) % n]))
        .collect();
    let lowest = horizontal
        .iter()
        .copied()
        .min_by(|&a, &b| mid_z(a).total_cmp(&mid_z(b)));
    for i in 0..n {
        let (a, b) = (coords[i], coords[(i + 1) % n]);
        if is_vertical(a, b) {
            let lower = if a[2] <= b[2] {
                boundary[i]
            } else {
                boundary[(i + 1) % n]
            };
            if let Some([f0, f1]) = faces {
                if f0 != f1 {
                    if lower == f0 {
                        column_resolved[0] = true;
                        out[i] = plate.slit.column_face[0];
                    } else if lower == f1 {
                        column_resolved[1] = true;
                        out[i] = plate.slit.column_face[1];
                    }
                }
            }
        } else if is_horizontal(a, b) {
            let is_bottom = lowest == Some(i);
            let role = usize::from(!is_bottom);
            beam_resolved[role] = true;
            out[i] = plate.slit.beam_face[role];
        }
    }
    let columns_resolved = plate
        .slit
        .column_face
        .iter()
        .zip(column_resolved)
        .all(|(&specified, resolved)| !specified || resolved);
    let beams_resolved = plate
        .slit
        .beam_face
        .iter()
        .zip(beam_resolved)
        .all(|(&specified, resolved)| !specified || resolved);
    (columns_resolved && beams_resolved).then_some(out)
}

/// 耐震スリット指定を境界辺へ対応付けたフラグ（`boundary_len` と同じ並び）。
///
/// 境界が 4 辺の囲まれた壁版で、境界の頂点にモデル節点があり、柱際・梁際の辺の
/// 役割を決められるときだけ `Some` を返す。それ以外は `None`（指定を反映できない）。
fn resolved_slit_edge_flags(
    model: &Model,
    plate: &WallPlate,
    boundary_len: usize,
) -> Option<Vec<bool>> {
    if boundary_len != 4 {
        return None;
    }
    let nodes = plate.boundary_nodes(model)?;
    let coords = plate.boundary_coords(model)?;
    if nodes.len() != 4 || coords.len() != 4 {
        return None;
    }
    slit_edge_flags(model, plate, &nodes, &coords)
}

/// 指定した耐震スリットをすべて境界辺へ対応付けられるか。
/// 指定なしは `true`、指定した役割が一つでも未解決なら `false`。
pub fn slit_specification_is_reflected(model: &Model, plate: &WallPlate) -> bool {
    if !plate.slit.any() {
        return true;
    }
    let Some(region) = model.wall_plate_assignment_region(plate.id) else {
        return false;
    };
    resolved_slit_edge_flags(model, plate, region.boundary.len()).is_some()
}

/// 壁版 1 枚の自重を辺へ配る。
///
/// 各辺の支持部材と材軸区間は、壁版が割り当てられた壁版割当領域の境界をそのまま使う
/// （境界の頂点にモデル節点が無くても支持先を引ける）。負担率の並びは境界の辺順に
/// 対応する。指定スリットの対応不足または不正なDL選択では分配せず、
/// 公開DL生成入口が壁版ID付きのエラーを返す。
fn edge_shares_with(
    model: &Model,
    plate: &WallPlate,
    basis: crate::cascade::SelfWeightBasis,
) -> Vec<WallEdgeShare> {
    if !matches!(plate.shape, WallPlateShape::Enclosed) {
        return Vec::new();
    }

    let total = match basis {
        crate::cascade::SelfWeightBasis::Design => model.wall_plate_self_weight(plate, model),
        crate::cascade::SelfWeightBasis::MassEquiv => {
            model.wall_plate_physical_weight(plate, model)
        }
    };
    let Some(total) = total else {
        return Vec::new();
    };
    if total <= 0.0 {
        return Vec::new();
    }
    let Some(region) = model.wall_plate_assignment_region(plate.id) else {
        return Vec::new();
    };
    let boundary = &region.boundary;
    if boundary.len() < 3 {
        return Vec::new();
    }
    let slit_edge = resolved_slit_edge_flags(model, plate, boundary.len())
        .unwrap_or_else(|| vec![false; boundary.len()]);
    let mut shares = Vec::new();
    let Ok(ratios) = dl_ratios(model, plate) else {
        return Vec::new();
    };
    for (i, &ratio) in ratios.iter().enumerate() {
        if ratio == 0.0 {
            continue;
        }
        if slit_edge[i] {
            return Vec::new();
        }
        let edge = boundary[i];
        shares.push(WallEdgeShare {
            support: edge.support,
            span: edge.span,
            total: total * ratio,
        });
    }
    shares
}

/// DL支持比。指定辺・梁三方式・スリット例外を同時適用しない。
pub fn dl_ratios(model: &Model, plate: &WallPlate) -> Result<Vec<f64>, String> {
    use sepika_core::model::WallDlSupport;
    let err = |s: &str| format!("壁版 {}: {s}", plate.id.0);
    if plate.is_attached() {
        return Err(err("取付き壁のDLは取付き先と伝達規則で決めます"));
    }
    let region = model
        .wall_plate_assignment_region(plate.id)
        .ok_or_else(|| err("壁版割当領域が未設定です"))?;
    if plate.dl_support.is_some() && !plate.self_weight_shares.is_empty() {
        return Err(err("DL梁方式と任意辺負担率の同時指定はできません"));
    }
    let flags = if plate.slit.any() {
        resolved_slit_edge_flags(model, plate, region.boundary.len())
            .ok_or_else(|| err("支持辺のスリット対応が未解決です"))?
    } else {
        vec![false; region.boundary.len()]
    };
    let checked = |ratios: Vec<f64>| {
        if ratios.iter().zip(&flags).any(|(r, s)| *r > 0.0 && *s) {
            return Err(err("選択したDL支持辺がスリットで切れています"));
        }
        Ok(ratios)
    };
    let coords = plate
        .boundary_coords(model)
        .ok_or_else(|| err("支持境界が未設定です"))?;
    let mut horizontal: Vec<_> = (0..coords.len())
        .filter(|&i| is_horizontal(coords[i], coords[(i + 1) % coords.len()]))
        .collect();
    let z = |i: usize| (coords[i][2] + coords[(i + 1) % coords.len()][2]) / 2.0;
    horizontal.sort_by(|&a, &b| z(a).total_cmp(&z(b)));
    let exception = plate.slit.column_face == [true, true] && plate.slit.beam_face == [true, false];
    if !exception && plate.dl_support.is_none() {
        if !plate.has_valid_self_weight_shares(model) {
            return Err(err("DL支持方式または有効な任意辺負担率が未指定です"));
        }
        return checked(plate.self_weight_shares.clone());
    }
    if horizontal.len() != 2 {
        return Err(err("三方式に必要な上下の水平支持梁を解決できません"));
    }
    let lower = horizontal[0];
    let upper = horizontal[1];
    let mut ratios = vec![0.0; region.boundary.len()];
    let mode = if exception {
        WallDlSupport::UpperBeam
    } else {
        plate.dl_support.unwrap()
    };
    let lower_secondary = matches!(region.boundary[lower].support,SupportMemberId::Secondary(id) if model.secondary_member(id).is_some_and(|m|m.kind==sepika_core::model::SecondaryMemberKind::Beam));
    if lower_secondary && !exception {
        ratios[lower] = 1.0;
    } else {
        match mode {
            WallDlSupport::LowerBeam => ratios[lower] = 1.0,
            WallDlSupport::UpperBeam => ratios[upper] = 1.0,
            WallDlSupport::HeightMidpoint => {
                let w = model.wall_weight(plate)?;
                let cut = (w.z_range_mm[0] + w.z_range_mm[1]) / 2.0;
                let a = w.band(w.z_range_mm[0] - 1.0, cut)?.design_n;
                if w.totals.design_n == 0.0 {
                    return checked(ratios);
                }
                ratios[lower] = a / w.totals.design_n;
                ratios[upper] = 1.0 - ratios[lower];
            }
        }
    }
    checked(ratios)
}

/// 要素にならない全壁版の自重を分配する（設計重量基準）。
pub fn distribute_enclosed_wall_plates(model: &Model) -> Result<EnclosedWallLoads, String> {
    distribute_enclosed_wall_plates_with_basis(model, crate::cascade::SelfWeightBasis::Design)
}

/// [`distribute_enclosed_wall_plates`] の自重基準（[`crate::cascade::SelfWeightBasis`]）を
/// 選べる版。支持経路・負担率は基準によらず同じで、躯体の単位体積重量だけが変わる。
pub fn distribute_enclosed_wall_plates_with_basis(
    model: &Model,
    basis: crate::cascade::SelfWeightBasis,
) -> Result<EnclosedWallLoads, String> {
    if basis == crate::cascade::SelfWeightBasis::Design {
        for plate in &model.wall_plates {
            model.validate_wall_design_self_weight(plate)?;
        }
    }
    let mut out = EnclosedWallLoads::default();
    for plate in &model.wall_plates {
        for share in edge_shares_with(model, plate, basis) {
            match share.post() {
                Some(key) => push_post_share(model, &mut out, key, &share),
                None => push_primary_share(model, &mut out.primary, &share),
            }
        }
    }
    Ok(out)
}

/// 間柱が受け持つぶんを、間柱の材軸局所の等分布荷重として積む。
///
/// 材軸区間は割当領域の境界が持つ無次元区間をそのまま材軸長へ写す。
fn push_post_share(
    model: &Model,
    out: &mut EnclosedWallLoads,
    key: SecondaryKey,
    share: &WallEdgeShare,
) {
    let Some(sm) = model.secondary_member(key) else {
        return;
    };
    let Some((_, _, len)) = model.secondary_member_axis(sm) else {
        return;
    };
    if len <= 1e-9 {
        return;
    }
    let s0 = share.span[0] * len;
    let s1 = share.span[1] * len;
    let (lo, hi) = (s0.min(s1), s0.max(s1));
    if hi - lo <= 1e-9 {
        return;
    }
    let w = share.total / (hi - lo);
    let entry = out.posts.entry(key).or_insert_with(|| PostWallLoad {
        member_loads: Vec::new(),
    });
    entry.member_loads.push(MemberLoadKind::Distributed {
        a: lo,
        b: hi,
        w1: w,
        w2: w,
    });
}

/// 主架構が受け持つぶんを、支持部材の材軸区間への等分布 `LoadTarget::Span` として積む。
///
/// 実部材への割り付けは床板の辺荷重と同じ解決
/// （`sepika-job::auto_loads::slab_load_case_content`）へ委ねる。
fn push_primary_share(model: &Model, loads: &mut Vec<BeamLoad>, share: &WallEdgeShare) {
    let SupportMemberId::Primary(elem) = share.support else {
        return;
    };
    let Some(element) = model.element(elem) else {
        return;
    };
    if element.nodes.len() != 2 {
        return;
    }
    let Some((a, b)) = model.support_member_axis(share.support) else {
        return;
    };
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let member_len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let loaded_len = (share.span[1] - share.span[0]).abs() * member_len;
    if loaded_len <= 1e-9 || share.total.abs() <= 1e-9 {
        return;
    }
    let w = share.total / loaded_len;
    loads.push(BeamLoad {
        elem,
        target: LoadTarget::Span {
            nodes: [element.nodes[0], element.nodes[1]],
            t: share.span,
        },
        shape: LoadShape::Uniform { w },
        cmq: fem_uniform(w, loaded_len),
    });
}

/// 壁版と二次部材のDL支持反力相当量 [N] を節点へ加算する。
/// 壁の地震用階重量では使用せず、共通実領域の水平帯積分を使う。
pub fn accumulate_wall_and_secondary_dl_weight(
    model: &Model,
    node_weight: &mut [f64],
) -> Result<(), String> {
    accumulate_wall_and_secondary_with_basis(
        model,
        node_weight,
        crate::cascade::SelfWeightBasis::Design,
        false,
    )
}

/// DL支持反力の物理質量相当量を節点へ集計する（階帯集計ではない）。
/// 支持経路・端部負担率は設計重量版と同じで、各部材の物理質量相当総量を使う。
/// 設計重量・階帯・質量行列負担との同一配分を意味しない。
pub fn accumulate_wall_and_secondary_dl_mass_equiv(
    model: &Model,
    node_mass: &mut [f64],
) -> Result<(), String> {
    accumulate_wall_and_secondary_with_basis(
        model,
        node_mass,
        crate::cascade::SelfWeightBasis::MassEquiv,
        false,
    )
}

/// [`accumulate_wall_and_secondary_dl_weight`] の、二次部材端の反力を重力ケースと
/// 同じく主架構へ解決してから集計する版（自重同期済み DL の置換量 [N] の算定用）。
///
/// 重力ケース（`compute_gravity_auto_load_cases`）は要素が接続しない節点の荷重を
/// `resolve_nodal_to_primary` で大梁の中間集中荷重へ変換する。置換量を重力ケースと
/// 同じ帰属で求めることで、質量置換後の節点質量が負になるのを防ぐ。
pub(crate) fn accumulate_wall_and_secondary_dl_weight_resolved(
    model: &Model,
    node_weight: &mut [f64],
) -> Result<(), String> {
    accumulate_wall_and_secondary_with_basis(
        model,
        node_weight,
        crate::cascade::SelfWeightBasis::Design,
        true,
    )
}

/// [`accumulate_wall_and_secondary_dl_mass_equiv`] の解決版（物理質量相当）。
pub(crate) fn accumulate_wall_and_secondary_dl_mass_equiv_resolved(
    model: &Model,
    node_mass: &mut [f64],
) -> Result<(), String> {
    accumulate_wall_and_secondary_with_basis(
        model,
        node_mass,
        crate::cascade::SelfWeightBasis::MassEquiv,
        true,
    )
}

fn accumulate_wall_and_secondary_with_basis(
    model: &Model,
    node_weight: &mut [f64],
    basis: crate::cascade::SelfWeightBasis,
    resolve_to_primary: bool,
) -> Result<(), String> {
    if !wall_plates_without_load_path(model).is_empty() {
        return Err("壁版の自重支持辺が未指定・不正、または支持先へ荷重を伝えられません".into());
    }
    let transfer =
        crate::cascade::solve_with_basis(model, |_| 0.0, true, basis).map_err(|e| e.to_string())?;
    if !transfer.invalid_end_shares.is_empty()
        || !transfer.unresolved.is_empty()
        || !transfer.cyclic.is_empty()
    {
        return Err("二次部材の端部負担率または支持先が不正で、自重を伝えられません".into());
    }
    for plate in &model.wall_plates {
        for share in edge_shares_with(model, plate, basis)
            .into_iter()
            .filter(|s| s.post().is_none())
        {
            let SupportMemberId::Primary(elem) = share.support else {
                continue;
            };
            let Some(element) = model.element(elem) else {
                continue;
            };
            if element.nodes.len() != 2 {
                continue;
            }
            let len = model.member_length(element);
            let s0 = share.span[0] * len;
            let s1 = share.span[1] * len;
            let (lo, hi) = (s0.min(s1), s0.max(s1));
            if hi - lo <= 1e-9 {
                continue;
            }
            let w = share.total / (hi - lo);
            let load = MemberLoadKind::Distributed {
                a: lo,
                b: hi,
                w1: w,
                w2: w,
            };
            let (ri, rj) = crate::floor::simple_reactions(&load, len);
            node_weight[element.nodes[0].index()] += ri;
            node_weight[element.nodes[1].index()] += rj;
        }
    }
    let (nodal, member) = transfer.primary_loads(model);
    if resolve_to_primary {
        // 重力ケースと同じ帰属にするため、要素が接続しない節点の反力を
        // 主架構の梁中間集中荷重へ変換してから節点重量へ加算する。
        let loads: Vec<NodalLoad> = nodal
            .into_iter()
            .map(|(node, w)| NodalLoad::auto(node, [0.0, 0.0, -w, 0.0, 0.0, 0.0]))
            .collect();
        let (nodal, resolved_member) =
            crate::secondary::resolve_nodal_to_primary(model, loads, crate::secondary::SPAN_TOL_MM);
        for nl in &nodal {
            node_weight[nl.node.index()] += -nl.values[2];
        }
        for load in resolved_member {
            add_point_load_reactions(model, node_weight, load.elem, &load.kind);
        }
    } else {
        for (node, weight) in nodal {
            node_weight[node.index()] += weight;
        }
    }
    for load in member {
        let Some(elem) = model.element(load.elem) else {
            continue;
        };
        let LoadShape::Point { p, x } = load.shape else {
            continue;
        };
        let (ri, rj) = crate::floor::simple_reactions(
            &MemberLoadKind::Point { a: x, p },
            model.member_length(elem),
        );
        node_weight[elem.nodes[0].index()] += ri;
        node_weight[elem.nodes[1].index()] += rj;
    }
    Ok(())
}

/// 主架構梁へ載る部材荷重を単純梁の静定反力として両端節点へ加算する。
fn add_point_load_reactions(
    model: &Model,
    node_weight: &mut [f64],
    elem: sepika_core::ids::ElemId,
    kind: &MemberLoadKind,
) {
    let Some(element) = model.element(elem) else {
        return;
    };
    if element.nodes.len() != 2 {
        return;
    }
    let (ri, rj) = crate::floor::simple_reactions(kind, model.member_length(element));
    node_weight[element.nodes[0].index()] += ri;
    node_weight[element.nodes[1].index()] += rj;
}

/// 自重を持つ非要素の囲まれた壁版のうち、支持先へ伝達できないものを返す。負担率の
/// 不備・割当領域の境界欠落・正の負担率の辺がスリットで切れている場合のみを判定し、
/// 支持部材の実在・種別・材軸解決・材端節点が 2 つであることは `Model::validate` が
/// 保証する前提とする。
/// 上下の梁際をともに切った納まりの可否はここでは判定せず、解析前チェックが入力方針として扱う。
pub fn wall_plates_without_load_path(model: &Model) -> Vec<WallPlateId> {
    model
        .wall_plates
        .iter()
        .filter(|plate| {
            if !matches!(plate.shape, WallPlateShape::Enclosed) {
                return false;
            }
            model
                .wall_plate_self_weight(plate, model)
                .is_some_and(|w| w > 0.0)
                && edge_shares_with(model, plate, crate::cascade::SelfWeightBasis::Design)
                    .is_empty()
        })
        .map(|plate| plate.id)
        .collect()
}

#[cfg(test)]
mod tests;
