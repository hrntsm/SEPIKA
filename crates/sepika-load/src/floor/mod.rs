//! スラブ面荷重の大梁・小梁・柱への分配。
//!
//! 責務ごとにサブモジュールへ分割している:
//! - [`types`] — 基本型（[`LoadShape`]・[`Cmq`]・[`LoadTarget`]・[`BeamLoad`]）と辺荷重ヘルパ
//! - [`geometry`] — 幾何ヘルパ（座標取得・距離・矩形判定）
//! - [`fem`] — 固定端モーメント・せん断（CMQ）の閉形式公式
//! - [`rect`] — 矩形床の分配戦略（三角形・台形／一方向／負担面積／小梁二段階）
//! - [`cantilever`] — 片持ちスラブ・出隅スラブの分配戦略
//! - [`polygon`] — 多角形床の負担面積法（最近接辺グリッドサンプリング）
//! - [`rigid_zone`] — 剛域を考慮した大梁 CMQ（[`cmq_with_rigid_zone`]）
//!
//! 本モジュールにはこれらを束ねるディスパッチャ [`distribute_slab`] と、床領域単位の
//! 束ね役 [`distribute_region`] を置く。
//!
//! # 床領域と床板の分配
//!
//! 床領域（大梁の 1 スパン区画）は、床領域内が小梁でさらに細かい打設単位に分かれていれば
//! 複数の床板（[`Slab`]）を持つ。[`distribute_region`] は床領域内の各床板を**独立に**
//! [`distribute_slab_w`] へ渡し、各辺荷重を床板割当領域の境界に従って実支持部材へ解決する
//! （[`resolve_edges_to_span`]）。主架構の辺は `LoadTarget::Span`、二次部材の辺は
//! `LoadTarget::Secondary` となり、二次部材が受け持った荷重は逐次伝達（[`crate::cascade`]）が
//! 両端反力へ変換して主架構へ渡す。支持先を解決できない辺は `LoadTarget::Edge` のまま残り、
//! 呼び出し側（`sepika-job::auto_loads`）が捨てる。

mod beam_design;
mod cantilever;
mod fem;
mod geometry;
mod polygon;
mod rect;
mod rigid_zone;
mod types;

pub use beam_design::{
    beam_distribution_is_ready, beam_distribution_is_sufficient, beam_expected_slabs_covered,
    beam_mass_equiv_udl, beam_self_weight_udl, cantilever_extremes, covered_length_of_loads,
    flip_member_loads, load_shape_to_member_loads, orient_member_loads,
    secondary_beam_distribution_gaps, secondary_beam_distribution_loads,
    secondary_beam_distribution_split, secondary_beams_missing_distribution, simple_beam_extremes,
    span_node_key, BeamExtremes, SecondaryBeamDistributionGaps, JOIST_COVER_MIN_RATIO,
    JOIST_DEFLECTION_SAMPLE_DIVISIONS, JOIST_FORCE_SAMPLE_DIVISIONS,
};
/// 取り付く壁版（[`crate::wall_attached`]）が、取付き線に載る等分布荷重の CMQ を
/// 床側と同じ式で求めるための再公開（`fem` 自体は非公開モジュール）。
pub(crate) use fem::{fem_linear, fem_uniform};
pub use fem::{fixed_end_moments, simple_beam_moment_at, simple_reactions};
pub use geometry::{point_in_slab_boundary, slab_dimensions, slab_dimensions_of};
pub use polygon::{
    integrate_polygon, PolygonDistribution, PolygonDistributionError, PolygonIntegrationOptions,
};
pub use rigid_zone::{cmq_with_rigid_zone, RigidZoneCmqMode, RigidZoneCmqResult};
pub use types::{BeamLoad, Cmq, LoadShape, LoadTarget};

/// 分配荷重と、多角形経路の面積・格子・誤差診断。
#[derive(Clone, Debug)]
pub struct SlabDistribution {
    pub loads: Vec<BeamLoad>,
    pub polygon: Option<PolygonDistribution>,
}

use cantilever::{distribute_cantilever, distribute_to_node};
use geometry::boundary_coords;
use polygon::distribute_polygon;
use rect::distribute_rect;
use sepika_core::model::{
    FloorRegion, LoadTransfer, Model, RegionAnchor, Slab, SlabShape, SupportMemberId,
};

fn short_direction_dimensions(coords: &[[f64; 3]]) -> Option<(f64, f64)> {
    let dimensions = slab_dimensions_of(coords)?;
    let edge_x = [
        coords[1][0] - coords[0][0],
        coords[1][1] - coords[0][1],
        coords[1][2] - coords[0][2],
    ];
    let edge_y = [
        coords[3][0] - coords[0][0],
        coords[3][1] - coords[0][1],
        coords[3][2] - coords[0][2],
    ];
    let dot = sepika_core::geom::vec3::dot(edge_x, edge_y);
    if dot.abs() / (dimensions.0 * dimensions.1) > 1e-6 {
        return None;
    }
    Some(dimensions)
}

#[derive(Clone, Debug, PartialEq)]
pub enum FloorDistributionError {
    Polygon(PolygonDistributionError),
    InvalidBoundary(String),
    InvalidAttachedSlab(String),
    SelfWeight(String),
    ShortDirectionOnSquare { slab_id: sepika_core::ids::SlabId },
    ShortDirectionRequiresRectangle { slab_id: sepika_core::ids::SlabId },
}

impl std::fmt::Display for FloorDistributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Polygon(error) => error.fmt(f),
            Self::InvalidBoundary(message) => f.write_str(message),
            Self::InvalidAttachedSlab(message) => f.write_str(message),
            Self::SelfWeight(message) => f.write_str(message),
            Self::ShortDirectionOnSquare { slab_id } => write!(
                f,
                "床板 {} は X・Y スパンが同寸の正方形のため、短辺方向を決められません。X または Y を指定してください。",
                slab_id.0
            ),
            Self::ShortDirectionRequiresRectangle { slab_id } => write!(
                f,
                "床板 {} の短辺方向指定は矩形床にのみ対応しています。X または Y を指定してください。",
                slab_id.0
            ),
        }
    }
}

impl std::error::Error for FloorDistributionError {}

pub fn validate_one_way_directions(model: &Model) -> Result<(), FloorDistributionError> {
    for slab in &model.slabs {
        if !matches!(&slab.shape, SlabShape::Enclosed)
            || slab.method() != sepika_core::model::DistributionMethod::OneWay
            || slab.one_way() != Some(sepika_core::model::OneWayDir::Short)
        {
            continue;
        }
        let coords = boundary_coords(model, slab).unwrap_or_default();
        let Some((lx, ly)) = short_direction_dimensions(&coords) else {
            return Err(FloorDistributionError::ShortDirectionRequiresRectangle {
                slab_id: slab.id,
            });
        };
        if (lx - ly).abs() < 1e-6 {
            return Err(FloorDistributionError::ShortDirectionOnSquare { slab_id: slab.id });
        }
    }
    Ok(())
}

#[cfg(test)]
use fem::{fem_trapezoid, fem_triangle};

/// 床板の面荷重を境界（および二次部材経由の節点荷重）へ分配する。
/// 自重の参照が未解決、または鋼の物理単位重量が固定設計値を超える場合はエラー。
///
/// 分岐は床板の形で決まる:
///
/// 1. **取り付く床板**（[`SlabShape::Attached`]）→ [`distribute_attached`]。
///    - 取付き先が点（出隅）: 全荷重をその節点（柱）へ集中する。荷重伝達方向にも
///      片持ち梁の取付きにも依らない（出隅の片持ちスラブの床荷重分配）。
///    - 取付き先が線 ＋ [`LoadTransfer::Anchor`]: 取付き大梁へ `w × 出し幅` の等分布を
///      載せる（[`distribute_cantilever`]）。側辺・先端辺の支持部材へは分配しない。
///    - 取付き先が線 ＋ [`LoadTransfer::Columns`]: 取付き線の区間中点（無次元位置
///      `t_mid = (t_i+t_j)/2`）に集中したとみなし、単純梁の反力公式で両端の柱へ按分する
///      （全長 `[0, 1]` なら `t_mid = 0.5` で半分ずつ）。
/// 2. **大梁または小梁で囲まれた床板**（[`SlabShape::Enclosed`]）
///    - 境界が矩形（[`slab_dimensions`] が `Some` を返す）→ 矩形床の分配
///      （[`distribute_rect`]）。一方向の指定があればその方向（全体座標 X/Y）へ、
///      なければ境界辺 0・2 が負担する。
///    - それ以外（三角形・台形・五角形などの多角形）→ 多角形の負担面積法
///      （[`distribute_polygon`]）。ただし短辺方向指定は入力エラーとする。
///
/// いずれの経路も総和保存（Σ大梁荷重 (+Σ小梁反力・Σ柱集中荷重) = w×面積）を満たすよう
/// 設計している（床は全体座標 XY 平面内（Z一定）にあることを仮定する）。
/// L 形の取り付く床板は取付き線ごとの複数の床板で表す。
pub fn distribute_slab(
    model: &Model,
    slab: &Slab,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    let loads = distribute_slab_w(model, slab, model.slab_dead_intensity(slab))?;
    model
        .validate_slab_design_self_weight(slab, "床板の設計DL分配")
        .map_err(FloorDistributionError::SelfWeight)?;
    Ok(loads)
}

/// 指定した面荷重強度 `w`（N/mm²）のみを床板の境界へ分配する。
///
/// 分岐ロジックは [`distribute_slab`] と同一で、荷重源だけを引数 `w` に差し替える。
/// これにより DL（固定荷重）と LL（積載荷重）を別々の荷重ケースへ分配できる
/// （用途別（床用/小梁用/大梁・柱・基礎用/地震力用）の積載荷重の使い分けや、荷重組合せでの DL/LL 係数分けに用いる）。
/// ゼロ荷重でも境界・支持参照を検査する。
pub fn distribute_slab_w(
    model: &Model,
    slab: &Slab,
    w: f64,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    distribute_slab_w_checked(model, slab, w)
}

pub fn distribute_slab_w_checked(
    model: &Model,
    slab: &Slab,
    w: f64,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    Ok(
        distribute_slab_w_with_diagnostics(model, slab, w, PolygonIntegrationOptions::default())?
            .loads,
    )
}

/// 面荷重 [N/mm²] を分配し、多角形では辺別面積・格子・誤差上界も返す。
/// 要求誤差は多角形経路に適用する。境界・支持参照異常はゼロ荷重時もエラー。
pub fn distribute_slab_w_with_diagnostics(
    model: &Model,
    slab: &Slab,
    w: f64,
    options: PolygonIntegrationOptions,
) -> Result<SlabDistribution, FloorDistributionError> {
    let mut loads = Vec::new();
    if !w.is_finite() {
        return Err(FloorDistributionError::InvalidBoundary(
            "床板の面荷重が非有限".into(),
        ));
    }
    model
        .validate_attached_slab(slab)
        .map_err(|e| FloorDistributionError::InvalidAttachedSlab(e.to_string()))?;
    if matches!(slab.shape, SlabShape::Enclosed) {
        validate_enclosed_supports(model, slab)?;
    }
    let coords = boundary_coords(model, slab).ok_or_else(|| {
        FloorDistributionError::InvalidBoundary(format!(
            "床板 {} の境界参照を解決できません",
            slab.id.0
        ))
    })?;
    if coords.len() < 3 || coords.iter().flatten().any(|x| !x.is_finite()) {
        return Err(FloorDistributionError::InvalidBoundary(
            "床板の境界頂点不足または非有限座標".into(),
        ));
    }
    if matches!(slab.shape, SlabShape::Enclosed) {
        polygon::local_polygon(&coords).map_err(FloorDistributionError::Polygon)?;
    }
    if matches!(&slab.shape, SlabShape::Enclosed)
        && slab.method() == sepika_core::model::DistributionMethod::OneWay
        && slab.one_way() == Some(sepika_core::model::OneWayDir::Short)
    {
        let Some((lx, ly)) = short_direction_dimensions(&coords) else {
            return Err(FloorDistributionError::ShortDirectionRequiresRectangle {
                slab_id: slab.id,
            });
        };
        if (lx - ly).abs() < 1e-6 {
            return Err(FloorDistributionError::ShortDirectionOnSquare { slab_id: slab.id });
        }
    }
    if let SlabShape::Attached { anchor, .. } = &slab.shape {
        if w != 0.0 {
            distribute_attached(&coords, w, *anchor, &mut loads);
        }
        return Ok(SlabDistribution {
            loads,
            polygon: None,
        });
    }
    let polygon = match slab_dimensions_of(&coords) {
        Some((lx, ly)) => {
            if w != 0.0 {
                distribute_rect(slab, &coords, lx, ly, w, &mut loads)?;
            }
            None
        }
        None => Some(
            distribute_polygon(&coords, w, &mut loads, options)
                .map_err(FloorDistributionError::Polygon)?,
        ),
    };
    Ok(SlabDistribution { loads, polygon })
}

fn validate_enclosed_supports(model: &Model, slab: &Slab) -> Result<(), FloorDistributionError> {
    let invalid = |reason: String| {
        FloorDistributionError::InvalidBoundary(format!("床板 {}: {reason}", slab.id.0))
    };
    let region = model
        .slab_assignment_region(slab.id)
        .ok_or_else(|| invalid("支持境界なし（全周支持以外は未対応）".into()))?;
    if region.boundary.len() < 3 {
        return Err(invalid("支持境界不足（自由辺・支持欠落は未対応）".into()));
    }
    let mut intervals = Vec::new();
    for edge in &region.boundary {
        if edge
            .span
            .iter()
            .any(|t| !t.is_finite() || *t < 0.0 || *t > 1.0)
            || edge.span[0] == edge.span[1]
        {
            return Err(invalid("支持辺の有向区間が不正".into()));
        }
        let (a, b) = model
            .support_member_axis(edge.support)
            .ok_or_else(|| invalid(format!("支持部材 {:?} の参照不明", edge.support)))?;
        if a.iter().chain(b.iter()).any(|x| !x.is_finite()) {
            return Err(invalid("支持部材の非有限座標".into()));
        }
        if let SupportMemberId::Primary(id) = edge.support {
            if model
                .element(id)
                .is_none_or(|element| element.nodes.len() != 2)
            {
                return Err(invalid("支持大梁は2節点線分のみ対応".into()));
            }
        }
        let at = |t: f64| {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        intervals.push((at(edge.span[0]), at(edge.span[1])));
    }
    let origin = intervals[0].0;
    let points: Vec<_> = intervals
        .iter()
        .flat_map(|&(a, b)| [a, b])
        .map(|p| [p[0] - origin[0], p[1] - origin[1]])
        .collect();
    let (lo, hi) = sepika_core::geom::polygon::bounding_box(&points);
    let tolerance = 64.0 * f64::EPSILON * (hi[0] - lo[0]).hypot(hi[1] - lo[1]).max(1.0);
    for i in 0..intervals.len() {
        let end = intervals[i].1;
        let start = intervals[(i + 1) % intervals.len()].0;
        if (0..3).any(|k| (end[k] - start[k]).abs() > tolerance) {
            return Err(invalid(
                "支持辺が閉じていない（開口・自由辺・支持欠落は未対応）".into(),
            ));
        }
    }
    Ok(())
}

/// 取り付く床板（片持ちスラブ・バルコニー・出隅）の分配。
///
/// - 取付き先が点（出隅）: 全荷重をその節点（柱）へ集中する。
/// - 取付き先が線 ＋ [`LoadTransfer::Anchor`]: 取付き大梁へ `w × 出し幅` の等分布を
///   載せる。側辺・先端辺の支持部材へは分配しない（[`distribute_cantilever`]）。
/// - 取付き先が線 ＋ [`LoadTransfer::Columns`]: 取付き線の区間中点（無次元位置
///   `t_mid = (t_i+t_j)/2`）に集中したとみなし、単純梁の集中荷重反力公式
///   （`R0 = W(1-t_mid)`、`R1 = W・t_mid`）で両端の柱へ按分する。全長
///   （`span = [0, 1]`）なら `t_mid = 0.5` で半分ずつになる。**この按分は
///   `extent[0] == extent[1]`（張り出し量が区間の両端で等しい）のときに限り
///   厳密である。** 張り出し量が異なる場合、真の面積重心は区間中点から張り出しの
///   大きい側へずれるが、その差は見ていない。
fn distribute_attached(
    coords: &[[f64; 3]],
    w: f64,
    anchor: RegionAnchor,
    loads: &mut Vec<BeamLoad>,
) {
    match anchor {
        RegionAnchor::Point(node) => distribute_to_node(node, coords, w, 1.0, loads),
        RegionAnchor::Line {
            nodes,
            span,
            transfer,
        } => match transfer {
            LoadTransfer::Anchor => distribute_cantilever(coords, w, loads),
            LoadTransfer::Columns => {
                // 部分区間（span != [0, 1]）では、総荷重の作用点は取付き線上の区間中点
                // （無次元位置 t_mid）にある。単純梁の集中荷重の反力公式
                // （R0 = P(1-t), R1 = P・t）で両端の柱へ按分する。全長（t_mid=0.5）では
                // 従来どおり半分ずつになる。
                let t_mid = 0.5 * (span[0] + span[1]);
                distribute_to_node(nodes[0], coords, w, 1.0 - t_mid, loads);
                distribute_to_node(nodes[1], coords, w, t_mid, loads);
            }
        },
        // 床板の取付き先には使わない（`RegionAnchor::FloorRegion` のドキュメント参照。
        // 壁側〔自立壁〕専用のアンカーであり、`Slab::shape` 経由では到達しない）。
        RegionAnchor::FloorRegion { .. } => {}
    }
}

/// 局所辺インデックス（`Edge(k)`）を、実支持部材への作用（`Span`/`Secondary`）へ解決する。
///
/// 取り付く床板の辺 0 は取付き先の無次元区間を `Span::t` へ引き継ぎ、囲まれた床板は
/// 床板割当領域の境界（[`sepika_core::model::SupportBoundary`]）を正本として辺の支持部材と
/// 材軸区間を引く。解決できない辺は `Edge` のまま返し、`elem` は `Primary` の `Span` では
/// 実要素 ID、それ以外は呼び出し側が解決する番兵 `ElemId(u32::MAX)` とする。
fn resolve_edges_to_span(
    model: &Model,
    slab: &Slab,
    loads: Vec<BeamLoad>,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    let attached_anchor_t = match &slab.shape {
        SlabShape::Attached {
            anchor: RegionAnchor::Line { span, .. },
            ..
        } => Some(*span),
        _ => None,
    };
    let region = match slab.shape {
        SlabShape::Enclosed => model.slab_assignment_region(slab.id),
        SlabShape::Attached { .. } => None,
    };
    if slab.supports_tip_loads() {
        let supports = model
            .attached_slab_supports(slab)
            .map_err(|e| FloorDistributionError::InvalidAttachedSlab(e.to_string()))?;
        let boundary = slab.boundary_coords(model).expect("検証済み取付き線");
        let attach_length = sepika_core::geom::vec3::dist(boundary[0], boundary[1]);
        return Ok(loads
            .into_iter()
            .flat_map(|load| {
                if load.target != LoadTarget::Edge(0) {
                    return vec![load];
                }
                let LoadShape::Uniform { w } = load.shape else {
                    return vec![load];
                };
                supports
                    .iter()
                    .filter_map(|support| {
                        let elem = model.element(support.elem)?;
                        let length =
                            model.member_length(elem) * (support.span[1] - support.span[0]).abs();
                        let intensity = w * attach_length * support.fraction / length;
                        Some(BeamLoad {
                            elem: support.elem,
                            target: LoadTarget::Span {
                                nodes: [elem.nodes[0], elem.nodes[1]],
                                t: support.span,
                            },
                            shape: LoadShape::Uniform { w: intensity },
                            cmq: fem::fem_uniform(intensity, length),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect());
    }
    Ok(loads
        .into_iter()
        .map(|mut bl| {
            let LoadTarget::Edge(k) = bl.target else {
                return bl;
            };
            if let Some(anchor_t) = attached_anchor_t {
                let Some([n0, n1]) = slab.edge_nodes(model, k) else {
                    bl.elem = sepika_core::ids::ElemId(u32::MAX);
                    return bl;
                };
                let t = if k == 0 { anchor_t } else { [0.0, 1.0] };
                bl.target = LoadTarget::Span { nodes: [n0, n1], t };
                bl.elem = sepika_core::ids::ElemId(u32::MAX);
                return bl;
            }
            let Some(boundary) = region.and_then(|region| region.boundary.get(k)) else {
                bl.elem = sepika_core::ids::ElemId(u32::MAX);
                return bl;
            };
            match boundary.support {
                SupportMemberId::Secondary(member) => {
                    bl.target = LoadTarget::Secondary {
                        member,
                        t: boundary.span,
                    };
                    bl.elem = sepika_core::ids::ElemId(u32::MAX);
                }
                SupportMemberId::Primary(elem) => {
                    let Some(element) = model.element(elem) else {
                        bl.elem = sepika_core::ids::ElemId(u32::MAX);
                        return bl;
                    };
                    if element.nodes.len() != 2 {
                        bl.elem = sepika_core::ids::ElemId(u32::MAX);
                        return bl;
                    }
                    bl.target = LoadTarget::Span {
                        nodes: [element.nodes[0], element.nodes[1]],
                        t: boundary.span,
                    };
                    bl.elem = elem;
                }
            }
            bl
        })
        .collect())
}

/// [`distribute_slab_w`] の戻り値を [`resolve_edges_to_span`] で解決した版。
///
/// どの床領域からも参照されない床板（片持ち・バルコニー・出隅、または帰属先が
/// 見つからない浮き床板）を、床領域とは独立に分配する用途に使う
/// （`sepika-job::auto_loads` 参照）。戻り値の `LoadTarget` は `Node`/`Span`/
/// `Secondary` と、支持先を解決できなかった辺の `Edge`。
pub fn distribute_slab_resolved(
    model: &Model,
    slab: &Slab,
    w: f64,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    distribute_slab_resolved_checked(model, slab, w)
}

pub fn distribute_slab_resolved_checked(
    model: &Model,
    slab: &Slab,
    w: f64,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    resolve_edges_to_span(model, slab, distribute_slab_w_checked(model, slab, w)?)
}

/// 床領域（大梁の 1 スパン区画）の面荷重を、床領域内の床板へ束ねて分配する。
///
/// 床領域内が小梁でさらに細かい打設単位に分かれていれば、各床板を独立に
/// [`distribute_slab_w`] へ渡す（[`Self`] のモジュールドキュメント参照）。
/// `w_of` は床板ごとの面荷重強度 [N/mm²] を返す関数（DL/LL を分けるため）。
/// 戻り値の `LoadTarget` は `Node`/`Span`/`Secondary` と、支持先を解決できなかった辺の
/// `Edge`（[`resolve_edges_to_span`]）。
pub fn distribute_region(
    model: &Model,
    region: &FloorRegion,
    w_of: impl Fn(&Slab) -> f64,
) -> Result<Vec<BeamLoad>, FloorDistributionError> {
    let mut loads = Vec::new();
    for &sid in &region.slab_ids {
        let slab = model.slab(sid).ok_or_else(|| {
            FloorDistributionError::InvalidBoundary(format!("床領域の床板 {} は参照不明", sid.0))
        })?;
        let slab_loads = distribute_slab_w(model, slab, w_of(slab))?;
        loads.extend(resolve_edges_to_span(model, slab, slab_loads)?);
    }
    Ok(loads)
}

#[cfg(test)]
mod distribution_verification;
#[cfg(test)]
mod tests;
