//! RC 柱の短期設計せん断力用 ΣMy（崩壊メカニズム判定）。

use std::collections::HashSet;

use num_rational::BigRational;
use num_traits::Zero;
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::geom::vec3::{cross, local_frame_axes_and_fallback_reference, norm, sub};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, Material, MaterialCategory, Model,
    RegionAnchor, SlabShape,
};
use sepika_core::rc_capacity::{rc_column_mu_simple, rc_mu_simple, RcCapacityInput};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::rigid_arm::resolve_lengths;
use sepika_element::transform::LocalFrame;
use sepika_solver::common::constraint::Reducer;
use smallvec::SmallVec;

use super::beam::{
    beam_supports_slab, column_transmits_bending, restrains_direction, transmits_bending,
};
use super::section_props::{axis_props_from_shape, rebar_info_from_shape, AxisProps};
use crate::material_strength::{main_rebar_category_issue, rebar_sigma_y_of};
use crate::member_design_check::combine_incomplete_reasons;
use crate::ultimate::rc_props::RcDirection;
use crate::MemberKind;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnHead {
    Continuous,
    Terminal,
    Unconfirmed,
}

fn beam_yield_is_lower(beam_my: f64, column_my: f64) -> bool {
    beam_my > 1e-9 && column_my > beam_my + 1e-9
}

fn has_concrete_material(model: &Model, member: &ElementData) -> bool {
    model
        .element_material(member)
        .is_some_and(|material| material.category == MaterialCategory::Concrete)
}

fn mechanism_moment(
    [foot_my, head_my]: [f64; 2],
    [foot_beams, head_beams]: [Option<f64>; 2],
    head: ColumnHead,
    allow_new_head_yield: impl FnOnce() -> bool,
    allow_foot_yield: impl FnOnce() -> bool,
) -> f64 {
    let both_columns = foot_my + head_my;
    if head != ColumnHead::Continuous {
        // 上階に続かない場合は、一般階の上下反転で従来の採否を置き換えられない。
        // 柱頭で梁降伏しないことを確認できなければ、柱脚を置換する条件も確定しない。
        let Some(head_beams) = head_beams else {
            return both_columns;
        };
        let foot_beams = foot_beams.unwrap_or(0.0);
        return if beam_yield_is_lower(head_beams, head_my) {
            match head {
                ColumnHead::Terminal => foot_my + head_beams,
                _ => both_columns,
            }
        } else if beam_yield_is_lower(foot_beams, foot_my) {
            head_my + 0.5 * foot_beams
        } else {
            both_columns
        };
    }

    let mut moment = both_columns;
    let head_beams = head_beams.unwrap_or(0.0);
    let head_beam_my = 0.5 * head_beams;
    if beam_yield_is_lower(head_beam_my, head_my)
        && (beam_yield_is_lower(head_beams, head_my) || allow_new_head_yield())
    {
        moment = moment.min(foot_my + head_beam_my);
    }
    let foot_beam_my = 0.5 * foot_beams.unwrap_or(0.0);
    if beam_yield_is_lower(foot_beam_my, foot_my) && allow_foot_yield() {
        moment = moment.min(head_my + foot_beam_my);
    }
    moment
}

/// 設計軸力 N = NL + n·|NE|（圧縮正）。`n` は引張正の部材軸力。
pub fn design_axial_for_mechanism(n_long: f64, n_combo: f64, n_factor: f64) -> f64 {
    let n_l = (-n_long).max(0.0);
    let n_e = (n_combo - n_long).abs();
    n_l + n_factor.max(0.0) * n_e
}

fn horizontal_unit(v: [f64; 3]) -> Option<[f64; 2]> {
    let h = [v[0], v[1]];
    let n = (h[0] * h[0] + h[1] * h[1]).sqrt();
    if n < 1e-9 {
        None
    } else {
        Some([h[0] / n, h[1] / n])
    }
}

fn alignment_score(load_h: [f64; 2], axis_h: [f64; 2]) -> f64 {
    let cos = load_h[0] * axis_h[0] + load_h[1] * axis_h[1];
    cos * cos
}

/// 梁を強軸/弱軸のどちらか一方にだけ割り当てる。
///
/// 45° 付近で両方向にヒットしないよう、スコアが大きい方を採用する。
/// 同点（ちょうど 45°）は `prefer_on_tie` が true の方向（強軸）に寄せる。
fn aligns_exclusively(
    load_h: [f64; 2],
    peer_h: [f64; 2],
    axis_h: [f64; 2],
    prefer_on_tie: bool,
) -> bool {
    let s = alignment_score(load_h, axis_h);
    if s + 1e-12 < 0.5 {
        return false;
    }
    let sp = alignment_score(peer_h, axis_h);
    s > sp + 1e-12 || ((s - sp).abs() <= 1e-12 && prefer_on_tie)
}

/// 梁の略算降伏モーメント。上下の大きい方を用いる。
fn beam_my_simple(shape: &SectionShape, material: Option<&Material>) -> Option<f64> {
    let sigma_y = rebar_sigma_y_of(material);
    if sigma_y <= 0.0 {
        return None;
    }
    let mu_of = |props: AxisProps| -> Option<f64> {
        if props.at <= 0.0 || props.d <= 0.0 {
            return None;
        }
        let inp = RcCapacityInput {
            b: props.b,
            d: props.d_full,
            at: props.at,
            d_eff: props.d,
            sigma_y,
            fc: 0.0,
            pw: 0.0,
            sigma_wy: 0.0,
            clear_span: 0.0,
            sigma_0: 0.0,
        };
        let my = rc_mu_simple(&inp);
        (my.is_finite() && my > 0.0).then_some(my)
    };
    let top = axis_props_from_shape(shape, RcDirection::Strong, true)?;
    let bottom = axis_props_from_shape(shape, RcDirection::Strong, false)?;
    Some(mu_of(top)?.max(mu_of(bottom)?))
}

fn column_my_at_n(model: &Model, elem: &ElementData, n_axial: f64, strong: bool) -> Option<f64> {
    let sec = elem
        .section
        .and_then(|sid| model.sections.get(sid.index()))?;
    let mat = model.element_material(elem)?;
    let fc = mat.fc.filter(|&v| v > 0.0)?;
    let shape = sec.shape.as_ref()?;
    let (props, b, d_full, as_total) = match shape {
        SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. } => {
            let direction = if strong {
                RcDirection::Strong
            } else {
                RcDirection::Weak
            };
            let props = axis_props_from_shape(shape, direction, true)?;
            let info = rebar_info_from_shape(shape, true)?;
            (props, props.b, props.d_full, info.main_area)
        }
        _ => return None,
    };
    let sigma_y = rebar_sigma_y_of(model.element_rebar_material(elem));
    if sigma_y <= 0.0 {
        return None;
    }
    let inp = RcCapacityInput {
        b,
        d: d_full,
        at: props.at,
        d_eff: props.d,
        sigma_y,
        fc,
        pw: props.pw,
        sigma_wy: 0.0,
        clear_span: 0.0,
        sigma_0: 0.0,
    };
    Some(rc_column_mu_simple(&inp, as_total, n_axial))
}

fn rigid_arm_is_parallel(
    [i, j]: [[f64; 3]; 2],
    [offset_i, offset_j]: [[f64; 3]; 2],
    [axis_x, axis_y]: [f64; 2],
) -> bool {
    // 外積だけを厳密に求めても、座標の差や偏心の加算を丸めると斜交を見落とす。
    let exact = |value| BigRational::from_float(value).expect("有限な座標・偏心・方向成分");
    let exact_delta =
        |d: usize| exact(j[d]) + exact(offset_j[d]) - exact(i[d]) - exact(offset_i[d]);
    let delta_is_zero = |d: usize| {
        if offset_i[d] == offset_j[d] {
            i[d] == j[d]
        } else {
            exact_delta(d).is_zero()
        }
    };
    if !delta_is_zero(2) {
        return false;
    }
    if axis_x == 0.0 {
        return delta_is_zero(0);
    }
    if axis_y == 0.0 {
        return delta_is_zero(1);
    }
    exact_delta(0) * exact(axis_y) == exact_delta(1) * exact(axis_x)
}

fn girder_strong_axis_is_parallel(
    model: &Model,
    girder: &ElementData,
    [axis_x, axis_y]: [f64; 2],
) -> bool {
    let Ok(Some([i, j])) = model.primary_frame_axis_end_positions(girder) else {
        return false;
    };
    let reference = girder.local_axis.ref_vector;
    let Ok(frame) = LocalFrame::from_nodes(i, j, reference) else {
        return false;
    };
    let [local_zx, local_zy, local_zz] = frame.rot[2];
    let axis_len = axis_x.hypot(axis_y);
    // 参照方向の直交成分が小さい場合は、実際の基底が代替軸を使うことがある。
    if local_zz.abs() > 1e-12
        || (local_zx * (axis_y / axis_len) - local_zy * (axis_x / axis_len)).abs() > 1e-12
    {
        return false;
    }
    // 正規化や座標差の丸めで、同一構面内の平行を否定したり微小な斜交を見落としたりしない。
    let exact = |value| BigRational::from_float(value).expect("有限な材軸端と断面参照方向");
    let [dx, dy, dz] = std::array::from_fn(|k| exact(j[k]) - exact(i[k]));
    let [rx, ry, rz] = reference.map(exact);
    let normal_x = &dy * &rz - &dz * &ry;
    let normal_y = &dz * &rx - &dx * &rz;
    let normal_z = &dx * &ry - &dy * &rx;
    normal_z.is_zero()
        && (!normal_x.is_zero() || !normal_y.is_zero())
        && normal_x * exact(axis_y) == normal_y * exact(axis_x)
}

struct MemberAxis {
    unit: [f64; 3],
    exact: [BigRational; 3],
}

fn cross_exact(a: &[BigRational; 3], b: &[BigRational; 3]) -> [BigRational; 3] {
    [
        &a[1] * &b[2] - &a[2] * &b[1],
        &a[2] * &b[0] - &a[0] * &b[2],
        &a[0] * &b[1] - &a[1] * &b[0],
    ]
}

fn actual_axes(
    [i, j]: [[f64; 3]; 2],
    reference: [f64; 3],
) -> Result<[MemberAxis; 3], &'static str> {
    let (axes, fallback) = local_frame_axes_and_fallback_reference(i, j, reference)?;
    // 単位化済みの基底だけの比較では、非零の斜交を丸めて平行と判定することがある。
    let exact = |value| BigRational::from_float(value).expect("有限な材軸端と断面参照方向");
    let x = std::array::from_fn(|k| exact(j[k]) - exact(i[k]));
    let z = cross_exact(&x, &fallback.unwrap_or(reference).map(exact));
    let y = cross_exact(&z, &x);
    let [ex, ey, ez] = axes;
    Ok([
        MemberAxis { unit: ex, exact: x },
        MemberAxis { unit: ey, exact: y },
        MemberAxis { unit: ez, exact: z },
    ])
}

impl MemberAxis {
    fn is_parallel(&self, other: &Self) -> bool {
        norm(cross(self.unit, other.unit)) <= 1e-12
            && self.exact.iter().any(|v| !v.is_zero())
            && other.exact.iter().any(|v| !v.is_zero())
            && cross_exact(&self.exact, &other.exact)
                .iter()
                .all(Zero::is_zero)
    }
}

fn node_pair(i: NodeId, j: NodeId) -> [NodeId; 2] {
    [i.min(j), i.max(j)]
}

#[derive(Default)]
struct GirderMoments<'a> {
    sum_my: f64,
    girders: SmallVec<[&'a ElementData; 2]>,
    floor_girders: SmallVec<[ElemId; 2]>,
}

fn sum_beam_my_at_node<'a>(
    model: &'a Model,
    topology: &'a ColumnMechanismTopology,
    node: NodeId,
    aligns: impl Fn([f64; 3]) -> bool,
    legacy_bending_axis: Option<[f64; 2]>,
    column_bending_axis: Option<&MemberAxis>,
    unconfirmed_axes: &mut SmallVec<[ElemId; 2]>,
) -> Result<Option<GirderMoments<'a>>, String> {
    let mut sum = Some(GirderMoments::default());
    let mut material_issue = None;
    for other in topology.frame.elements_at(model, node) {
        let (Some(n0), Some(n1)) = (
            other.nodes.first().and_then(|n| model.nodes.get(n.index())),
            other.nodes.get(1).and_then(|n| model.nodes.get(n.index())),
        ) else {
            continue;
        };
        let axis = sub(n1.coord, n0.coord);
        match MemberKind::try_of_element(other, model) {
            Ok(MemberKind::Girder) => {}
            Err(_) if aligns(axis) => {
                sum = None;
                continue;
            }
            _ => continue,
        }
        let end = usize::from(other.nodes[1] == node);
        if other.end_offsets_mm[end] != [0.0; 3] {
            sum = None;
            continue;
        }
        let has_far_offset = other.end_offsets_mm[1 - end] != [0.0; 3];
        let actual_ends = model.primary_frame_axis_end_positions(other).ok().flatten();
        let actual_axis = actual_ends.map_or(axis, |[i, j]| sub(j, i));
        let end_condition = other.end_cond[end];
        // 材軸方向の腕が柱の曲げ軸と平行な場合だけ、その軸まわりの付加曲げは生じない。
        if end_condition != EndCondition::Fixed {
            let rigid = model.analysis_rigid_zone(other);
            let (i, j) = resolve_lengths(
                rigid.rigid_length_i(),
                rigid.rigid_length_j(),
                norm(actual_axis),
            );
            if [i, j][end] > 0.0
                && !legacy_bending_axis.is_some_and(|axis| {
                    rigid_arm_is_parallel([n0.coord, n1.coord], other.end_offsets_mm, axis)
                })
            {
                sum = None;
                continue;
            }
        }
        if matches!(
            end_condition,
            EndCondition::Pinned | EndCondition::SemiRigid { k_theta: 0.0 }
        ) {
            continue;
        }
        let assigned = aligns(axis) || aligns(actual_axis);
        let matching_strong_axis = column_bending_axis.map(|column_axis| {
            let axes =
                actual_ends.and_then(|ends| actual_axes(ends, other.local_axis.ref_vector).ok());
            if let Some([girder_x, _, girder_z]) = axes {
                if column_axis.is_parallel(&girder_z) {
                    return true;
                }
                if column_axis.is_parallel(&girder_x) {
                    return false;
                }
            }
            unconfirmed_axes.push(other.id);
            false
        });
        match end_condition {
            _ if !assigned && matching_strong_axis != Some(true) => continue,
            EndCondition::Fixed if !has_far_offset => {}
            _ => {
                sum = None;
                continue;
            }
        }
        if matching_strong_axis == Some(false) {
            continue;
        }
        let Some(sec) = model.element_section(other) else {
            sum = None;
            continue;
        };
        let Some(shape @ SectionShape::RcBeamRect { .. }) = &sec.shape else {
            sum = None;
            continue;
        };
        let material = model.element_rebar_material(other);
        if let Some(reason) = main_rebar_category_issue(material) {
            material_issue.get_or_insert_with(|| {
                format!("大梁 ID {} の断面「{}」の{reason}", other.id.0, sec.name)
            });
            continue;
        }
        if topology.has_attached_floor(model, other) {
            if let Some(sum) = &mut sum {
                sum.floor_girders.push(other.id);
                sum.girders.push(other);
            }
            continue;
        }
        sum = sum
            .zip(beam_my_simple(shape, material))
            .and_then(|(mut sum, my)| {
                sum.sum_my += my;
                sum.girders.push(other);
                sum.sum_my.is_finite().then_some(sum)
            });
    }
    material_issue.map_or(Ok(sum), Err)
}

fn girder_ids_text(ids: &[ElemId]) -> String {
    ids.iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join("、")
}

/// 柱の方向別降伏モーメント和を求めるモデルの接続情報。
pub struct ColumnMechanismTopology {
    frame: NodeAdjacency,
    linked_nodes: HashSet<NodeId>,
    unconfirmed_reverse_joints: HashSet<NodeId>,
    shell_node_pairs: HashSet<[NodeId; 2]>,
    line_slab_supports: HashSet<ElemId>,
}

impl ColumnMechanismTopology {
    /// 検定対象モデルの主架構線材と、別節点との連結拘束を索引化する。
    pub fn new(model: &Model) -> Self {
        let mut unconfirmed_reverse_joints = HashSet::new();
        let mut shell_node_pairs = HashSet::new();
        for elem in &model.elements {
            match elem.kind {
                ElementKind::PanelZone => {
                    unconfirmed_reverse_joints.extend(elem.nodes.first().copied());
                }
                ElementKind::NodalSpring | ElementKind::Wall => {
                    unconfirmed_reverse_joints.extend(elem.nodes.iter().copied())
                }
                ElementKind::Shell => {
                    unconfirmed_reverse_joints.extend(elem.nodes.iter().copied());
                    for (index, &first) in elem.nodes.iter().enumerate() {
                        shell_node_pairs.extend(
                            elem.nodes[index + 1..]
                                .iter()
                                .map(|&second| node_pair(first, second)),
                        );
                    }
                }
                // 軸力要素も、偏心端では軸力により付加曲げを節点へ伝える。
                ElementKind::Brace { .. } => unconfirmed_reverse_joints.extend(
                    elem.nodes
                        .iter()
                        .zip(elem.end_offsets_mm)
                        .filter_map(|(&node, offset)| (offset != [0.0; 3]).then_some(node)),
                ),
                _ => {}
            }
        }
        // 時刻歴の簡易検定では、壁が解析要素へ展開される前のモデルを受け取ることがある。
        unconfirmed_reverse_joints.extend(
            model
                .wall_plates
                .iter()
                .filter(|plate| model.wall_plate_becomes_element(plate))
                .filter_map(|plate| plate.boundary_nodes(model))
                .flatten(),
        );
        // 固定節点への拘束は、縮約後に別節点への参照を残さず回転を止めることがある。
        for constraint in &model.constraints {
            match constraint {
                Constraint::RigidLink { master, slaves, .. } => {
                    unconfirmed_reverse_joints.insert(*master);
                    unconfirmed_reverse_joints.extend(slaves.iter().copied());
                }
                Constraint::Mpc { master, terms } => {
                    unconfirmed_reverse_joints.insert(*master);
                    unconfirmed_reverse_joints.extend(terms.iter().map(|&(node, _, _)| node));
                }
                Constraint::RigidDiaphragm { .. } => {}
            }
        }
        let line_slab_supports = model
            .slabs
            .iter()
            .filter(|slab| {
                matches!(
                    slab.shape,
                    SlabShape::Attached {
                        anchor: RegionAnchor::Line { .. },
                        ..
                    }
                ) && model.slab_plate_thickness(slab).is_some()
            })
            .filter_map(|slab| model.attached_slab_supports(slab).ok())
            .flatten()
            .map(|support| support.elem)
            .collect();
        Self {
            frame: NodeAdjacency::primary_frame(model),
            linked_nodes: Reducer::explicit_constraint_linked_nodes(model),
            unconfirmed_reverse_joints,
            shell_node_pairs,
            line_slab_supports,
        }
    }

    fn column_head(&self, model: &Model, head: NodeId, head_z_mm: f64) -> ColumnHead {
        let has_upper_column = self.frame.elements_at(model, head).any(|elem| {
            matches!(
                MemberKind::try_of_element(elem, model),
                Ok(MemberKind::Column)
            ) && elem.nodes.iter().take(2).any(|&node| {
                model
                    .node(node)
                    .is_some_and(|node| node.coord[2] > head_z_mm)
            })
        });
        if has_upper_column {
            ColumnHead::Continuous
        } else if self.linked_nodes.contains(&head) {
            ColumnHead::Unconfirmed
        } else {
            ColumnHead::Terminal
        }
    }

    fn has_attached_floor(&self, model: &Model, girder: &ElementData) -> bool {
        self.line_slab_supports.contains(&girder.id)
            || beam_supports_slab(model, girder)
            || self
                .shell_node_pairs
                .contains(&node_pair(girder.nodes[0], girder.nodes[1]))
    }

    fn allows_new_head_yield(
        model: &Model,
        girders: &[&ElementData],
        bending_axis: Option<[f64; 2]>,
    ) -> bool {
        let Some(bending_axis) = bending_axis else {
            return false;
        };
        girders.iter().all(|girder| {
            has_concrete_material(model, girder)
                && girder_strong_axis_is_parallel(model, girder, bending_axis)
        })
    }

    fn has_direct_rc_continuation(
        &self,
        model: &Model,
        column: &ElementData,
        joint: NodeId,
        above: bool,
    ) -> bool {
        let Some(node) = model.node(joint) else {
            return false;
        };
        if node.restraint.0 & Dof6Mask::FIXED.0 != 0
            || node.support_spring.is_some()
            || self.unconfirmed_reverse_joints.contains(&joint)
        {
            return false;
        }
        let mut found = false;
        for elem in self.frame.elements_at(model, joint) {
            if elem.id == column.id {
                continue;
            }
            match MemberKind::try_of_element(elem, model) {
                Ok(MemberKind::Girder) => continue,
                Ok(MemberKind::Column) => {}
                _ => return false,
            }
            let far = elem.nodes[usize::from(elem.nodes[0] == joint)];
            if found
                || !has_concrete_material(model, elem)
                || !model.node(far).is_some_and(|far| {
                    far.coord[..2] == node.coord[..2]
                        && if above {
                            far.coord[2] > node.coord[2]
                        } else {
                            far.coord[2] < node.coord[2]
                        }
                })
                || elem.end_offsets_mm != [[0.0; 3]; 2]
                || elem.end_cond != [EndCondition::Fixed; 2]
                || !matches!(
                    model
                        .element_section(elem)
                        .and_then(|sec| sec.shape.as_ref()),
                    Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. })
                )
            {
                return false;
            }
            found = true;
        }
        found
    }

    fn has_opposing_foot_girders(
        &self,
        model: &Model,
        foot: NodeId,
        girders: &[&ElementData],
        bending_axis: Option<[f64; 2]>,
    ) -> bool {
        let (Some(node), [first, second], Some([bx, by])) =
            (model.node(foot), girders, bending_axis)
        else {
            return false;
        };
        // 加力方向の割当から外れる梁も、斜交曲げやねじりを柱脚へ伝え得る。
        if self.frame.elements_at(model, foot).any(|elem| {
            elem.id != first.id
                && elem.id != second.id
                && matches!(
                    MemberKind::try_of_element(elem, model),
                    Ok(MemberKind::Girder)
                )
        }) {
            return false;
        }
        let shear_axis = [by, -bx];
        let component = usize::from(shear_axis[0] == 0.0);
        let rotation = [bx, by, 0.0].map(|value| value / bx.hypot(by));
        let mut opposing = [false; 2];
        for girder in [first, second] {
            let end = usize::from(girder.nodes[1] == foot);
            let Some(far) = model.node(girder.nodes[1 - end]) else {
                return false;
            };
            let side = usize::from(far.coord[component] > node.coord[component]);
            let has_far_support = far.restraint.is_fixed(Dof::Uz)
                || (transmits_bending(girder.end_cond[1 - end])
                    && (restrains_direction(far, rotation, [Dof::Rx, Dof::Ry, Dof::Rz])
                        || self.frame.elements_at(model, far.id).any(|column| {
                            matches!(
                                MemberKind::try_of_element(column, model),
                                Ok(MemberKind::Column)
                            ) && column_transmits_bending(model, column, far, rotation)
                        })));
            if opposing[side]
                || !has_far_support
                || !has_concrete_material(model, girder)
                || !girder_strong_axis_is_parallel(model, girder, [bx, by])
                || !rigid_arm_is_parallel([node.coord, far.coord], [[0.0; 3]; 2], shear_axis)
            {
                return false;
            }
            opposing[side] = true;
        }
        opposing == [true; 2]
    }
}

/// RC 柱の方向別降伏モーメント和 [N·mm]。 `Ok(None)` は柱 My 未算定、
/// `Err` は入力不足・不適合により、その方向の QD1 が未算定。
#[derive(Clone, Debug)]
pub struct ColumnMechanism {
    /// Qy（強軸）方向。
    pub qy: Result<Option<f64>, String>,
    /// Qz（弱軸）方向。
    pub qz: Result<Option<f64>, String>,
    pub(crate) missing_gravity_axial: bool,
    pub(crate) girder_yield_holds: [Option<String>; 2],
    pub(crate) excluded_girder_axes: [bool; 2],
}

/// 柱のメカニズム ΣMy [N·mm]。方向別に算定値または材料区分の不適合を返す。
/// `n_*` は引張正。
/// `model` は `Model::validate_local_frames` 済みとし、そのモデルの `topology` を渡す。
#[allow(clippy::too_many_arguments)]
pub fn compute_column_mechanism_sum_my(
    model: &Model,
    topology: &ColumnMechanismTopology,
    column: &ElementData,
    n_long_i: f64,
    n_long_j: f64,
    n_combo_i: f64,
    n_combo_j: f64,
    n_axial_factor: f64,
) -> Option<ColumnMechanism> {
    column_mechanism_sum_my(model, topology, column, false, |_| {
        Ok([
            design_axial_for_mechanism(n_long_i, n_combo_i, n_axial_factor),
            design_axial_for_mechanism(n_long_j, n_combo_j, n_axial_factor),
        ])
    })
}

/// 時刻歴の柱端軸力 [N] から方向別の ΣMy [N·mm] を求める。
/// 軸力は i端・j端の順で引張正。欠損・非有限があれば接続大梁の理由と併記する。
pub fn time_history_column_mechanism_sum_my(
    model: &Model,
    topology: &ColumnMechanismTopology,
    column: &ElementData,
    long_end_n: [Option<f64>; 2],
    response_end_n: [Option<f64>; 2],
) -> Option<ColumnMechanism> {
    let missing_gravity_axial = long_end_n.iter().any(|n| n.is_none_or(|n| !n.is_finite()));
    column_mechanism_sum_my(
        model,
        topology,
        column,
        missing_gravity_axial,
        |i_is_bottom| {
            let mut reason = None;
            let mut design_n = [0.0; 2];
            for index in 0..2 {
                let end = if (index == 0) == i_is_bottom {
                    "柱脚"
                } else {
                    "柱頭"
                };
                let n_long = long_end_n[index].filter(|n| n.is_finite());
                let n_response = response_end_n[index].filter(|n| n.is_finite());
                for (value, source) in [(n_long, "重力参照"), (n_response, "時刻歴")] {
                    if value.is_none() {
                        reason = combine_incomplete_reasons(
                            reason,
                            Some(&format!("{end}の{source}軸力Nが不足または非有限です")),
                        );
                    }
                }
                if let (Some(n_long), Some(n_response)) = (n_long, n_response) {
                    let n = design_axial_for_mechanism(n_long, n_response, 1.0);
                    if n.is_finite() {
                        design_n[index] = n;
                    } else {
                        reason = combine_incomplete_reasons(
                            reason,
                            Some(&format!("{end}の設計軸力Nが非有限です")),
                        );
                    }
                }
            }
            reason.map_or(Ok(design_n), Err)
        },
    )
}

fn column_mechanism_sum_my(
    model: &Model,
    topology: &ColumnMechanismTopology,
    column: &ElementData,
    missing_gravity_axial: bool,
    design_axial: impl FnOnce(bool) -> Result<[f64; 2], String>,
) -> Option<ColumnMechanism> {
    if column.kind != ElementKind::Beam || column.nodes.len() < 2 {
        return None;
    }
    if MemberKind::of_element(column, model) != MemberKind::Column {
        return None;
    }
    let (Some(ni), Some(nj)) = (column.nodes.first().copied(), column.nodes.get(1).copied()) else {
        return None;
    };
    let (Some(p_i), Some(p_j)) = (
        model.nodes.get(ni.index()).map(|n| n.coord),
        model.nodes.get(nj.index()).map(|n| n.coord),
    ) else {
        return None;
    };
    let [_, column_y, column_z] = actual_axes([p_i, p_j], column.local_axis.ref_vector)
        .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", column.id.0));
    let ey = column_y.unit;
    let ez = column_z.unit;
    let load_strong = horizontal_unit(ey);
    let load_weak = horizontal_unit(ez);

    let [ref_x, ref_y, _] = column.local_axis.ref_vector;
    let reference_h = [ref_x, ref_y, 0.0];
    let reference_length = norm(reference_h);
    // 単位化済みの柱軸だけでは、入力の幾何が平行であることを確認できない。
    let bending_y = if p_i[..2] != p_j[..2] {
        None
    } else if reference_h == [0.0; 3] && ey == [1.0, 0.0, 0.0] {
        Some([1.0, 0.0])
    } else if reference_length > 0.0 && ey == reference_h.map(|value| value / reference_length) {
        Some([ref_x, ref_y])
    } else {
        None
    };
    let bending_z = bending_y.map(|[x, y]| [-y, x]);

    let i_is_bottom = p_i[2] <= p_j[2];
    let design_n = design_axial(i_is_bottom);
    let (foot, head, head_z_mm) = if i_is_bottom {
        (ni, nj, p_j[2])
    } else {
        (nj, ni, p_i[2])
    };
    let confirmed_clear_height = match [column.rigid_zone.face_i, column.rigid_zone.face_j] {
        [Some(i), Some(j)] if i >= 0.0 && j >= 0.0 => {
            let height_mm = norm(sub(p_j, p_i)) - (i + j);
            height_mm.is_finite() && height_mm > 0.0
        }
        _ => false,
    };
    let column_head = topology.column_head(model, head, head_z_mm);
    let continuous_rc_joint_pair = column_head == ColumnHead::Continuous
        && p_i[..2] == p_j[..2]
        && has_concrete_material(model, column)
        && column.end_cond == [EndCondition::Fixed; 2]
        && column.end_offsets_mm == [[0.0; 3]; 2]
        && topology.has_direct_rc_continuation(model, column, head, true)
        && topology.has_direct_rc_continuation(model, column, foot, false);

    let sum_for = |strong: bool,
                   load_h: Option<[f64; 2]>,
                   peer_h: Option<[f64; 2]>,
                   bending_axis: Option<[f64; 2]>,
                   actual_bending_axis: &MemberAxis| {
        let aligns = |axis| {
            horizontal_unit(axis).is_some_and(|axis_h| match (load_h, peer_h) {
                (Some(load), Some(peer)) => aligns_exclusively(load, peer, axis_h, strong),
                (Some(load), None) => alignment_score(load, axis_h) + 1e-12 >= 0.5,
                _ => false,
            })
        };
        let check_axes = column.end_offsets_mm == [[0.0; 3]; 2];
        let beam_sum = |node, unconfirmed_axes| {
            sum_beam_my_at_node(
                model,
                topology,
                node,
                aligns,
                bending_axis,
                check_axes.then_some(actual_bending_axis),
                unconfirmed_axes,
            )
        };
        let mut foot_axes = SmallVec::new();
        let mut head_axes = SmallVec::new();
        let (i_axes, j_axes) = if i_is_bottom {
            (&mut foot_axes, &mut head_axes)
        } else {
            (&mut head_axes, &mut foot_axes)
        };
        let beams_i = beam_sum(ni, i_axes);
        let beams_j = beam_sum(nj, j_axes);
        let beam_ends =
            beams_i.and_then(|bi| beams_j.map(|bj| if i_is_bottom { (bi, bj) } else { (bj, bi) }));
        let axis = if strong { "Qy" } else { "Qz" };
        let with_context = |reason| {
            format!(
                "RC柱 ID {} の {axis} 方向のせん断検定: {reason}",
                column.id.0
            )
        };
        let excludes_foot_axes = !foot_axes.is_empty() && column_head != ColumnHead::Continuous;
        let excludes_head_axes = !head_axes.is_empty();
        let excluded_girder_axis = excludes_head_axes || excludes_foot_axes;
        let mut hold = None;
        for (end, ids, excludes) in [
            ("柱頭", &head_axes, excludes_head_axes),
            ("柱脚", &foot_axes, excludes_foot_axes),
        ] {
            if excludes {
                let ids = girder_ids_text(ids);
                hold = combine_incomplete_reasons(
                    hold,
                    Some(&format!("{axis}方向の{end}: 大梁 ID {ids} と柱の検討曲げ軸の対応を確認できないため、梁降伏の候補を除外")),
                );
            }
        }
        let (foot_beams, head_beams) = match beam_ends {
            Ok(beams) => beams,
            Err(reason) => {
                let reason = combine_incomplete_reasons(
                    Some(reason),
                    design_n.as_ref().err().map(String::as_str),
                )
                .expect("接続大梁の未算定理由");
                return (Err(with_context(reason)), hold, excluded_girder_axis);
            }
        };
        let allow_foot_yield = || {
            continuous_rc_joint_pair
                && foot_beams.as_ref().is_some_and(|beams| {
                    topology.has_opposing_foot_girders(model, foot, &beams.girders, bending_axis)
                })
        };
        if check_axes && foot_beams.is_some() && head_beams.is_some() {
            for (end, beams) in [
                ("柱頭", head_beams.as_ref()),
                (
                    "柱脚",
                    foot_beams.as_ref().filter(|beams| {
                        column_head != ColumnHead::Continuous
                            || !beams.floor_girders.is_empty() && allow_foot_yield()
                    }),
                ),
            ] {
                if let Some(beams) = beams.filter(|beams| !beams.floor_girders.is_empty()) {
                    let ids = girder_ids_text(&beams.floor_girders);
                    hold = combine_incomplete_reasons(
                        hold,
                        Some(&format!("{axis}方向の{end}: 床板が取り付く大梁 ID {ids} の床板筋の寄与を含む梁端降伏曲げを確認できないため、梁降伏の候補を除外")),
                    );
                }
            }
        }
        let direction = (|| -> Result<Option<f64>, String> {
            let n = design_n.as_ref().map_err(Clone::clone)?;
            if excluded_girder_axis && !confirmed_clear_height {
                return Err("柱の内法高さを正の有限な値として確認できないため、設計用せん断力を算定できません".into());
            }
            let (Some(col_i), Some(col_j)) = (
                column_my_at_n(model, column, n[0], strong),
                column_my_at_n(model, column, n[1], strong),
            ) else {
                return if hold.is_some() {
                    Err("両柱端の降伏曲げを確認できないため、設計用せん断力を算定できません".into())
                } else {
                    Ok(None)
                };
            };
            let (foot_my, head_my) = if i_is_bottom {
                (col_i, col_j)
            } else {
                (col_j, col_i)
            };
            let moment = match (&foot_beams, &head_beams) {
                (Some(foot_beams), Some(head_beams)) if check_axes => {
                    let confirmed_sum = |beams: &GirderMoments<'_>, excluded: bool| {
                        (beams.floor_girders.is_empty() && !excluded).then_some(beams.sum_my)
                    };
                    mechanism_moment(
                        [foot_my, head_my],
                        [
                            confirmed_sum(foot_beams, excludes_foot_axes),
                            confirmed_sum(head_beams, excludes_head_axes),
                        ],
                        column_head,
                        || {
                            ColumnMechanismTopology::allows_new_head_yield(
                                model,
                                &head_beams.girders,
                                bending_axis,
                            )
                        },
                        allow_foot_yield,
                    )
                }
                _ => col_i + col_j,
            };
            if hold.is_some() && !(moment.is_finite() && moment > 0.0) {
                return Err(
                    "降伏曲げの和が正の有限な値でないため、設計用せん断力を算定できません".into(),
                );
            }
            Ok(Some(moment))
        })();
        match direction {
            Err(reason) => (
                Err(with_context(
                    combine_incomplete_reasons(Some(reason), hold.as_deref()).expect("未算定理由"),
                )),
                hold,
                excluded_girder_axis,
            ),
            Ok(moment) => (Ok(moment), hold, excluded_girder_axis),
        }
    };
    let (qy, hold_y, excluded_y_axis) = sum_for(true, load_strong, load_weak, bending_z, &column_z);
    let (qz, hold_z, excluded_z_axis) =
        sum_for(false, load_weak, load_strong, bending_y, &column_y);
    Some(ColumnMechanism {
        qy,
        qz,
        missing_gravity_axial,
        girder_yield_holds: [hold_y, hold_z],
        excluded_girder_axes: [excluded_y_axis, excluded_z_axis],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_axial_adds_abs_earthquake() {
        // NL=1000 (n_long=-1000), NE 増分 200 → N=1200
        let n = design_axial_for_mechanism(-1000.0, -1200.0, 1.0);
        assert!((n - 1200.0).abs() < 1e-9);
        let n2 = design_axial_for_mechanism(-1000.0, -1200.0, 2.0);
        assert!((n2 - 1400.0).abs() < 1e-9);
    }

    /// 加力方向の梁 My が取れない（SRC 等）とその方向のメカニズムは破棄される。
    #[test]
    fn missing_beam_my_discards_mechanism_direction() {
        use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
        use sepika_core::model::{
            ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis,
            Material, MaterialCategory, Node, RigidZone,
        };
        use sepika_core::section_shape::{
            BeamStirrup, RcBeamRebar, RcRectColumnRebar, RectColumnHoop,
        };
        use sepika_core::Dof6Mask;
        use smallvec::smallvec;

        let col_rebar = RcRectColumnRebar {
            main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
            x: vec![2],
            y: vec![2],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        };
        let col_shape = SectionShape::RcColumnRect {
            b: 500.0,
            d: 500.0,
            rebar: col_rebar,
        };
        let beam_shape = SectionShape::SrcBeamRect {
            b: 300.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                top: vec![4],
                bottom: vec![4],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
            steel_height: 400.0,
            steel_width: 200.0,
            steel_web_thick: 9.0,
            steel_flange_thick: 16.0,
            steel_manufacture: Default::default(),
        };
        let materials = vec![
            Material {
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 21_000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205_000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(345.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ];
        let mut col_sec = col_shape.to_section(SectionId(0), "C".into());
        col_sec.frame_use = Some(FrameSectionUse::Column);
        col_sec.material = Some(MaterialId(0));
        col_sec.rebar_material = Some(MaterialId(1));
        let mut beam_sec = beam_shape.to_section(SectionId(1), "B-SRC".into());
        beam_sec.frame_use = Some(FrameSectionUse::Girder);
        beam_sec.material = Some(MaterialId(0));
        beam_sec.rebar_material = Some(MaterialId(1));

        let line = |id: u32, n0: u32, n1: u32, sid: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec![NodeId(n0), NodeId(n1)],
            section: Some(SectionId(sid)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        };
        let node = |id: u32, x: f64, y: f64, z: f64| Node {
            id: NodeId(id),
            coord: [x, y, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let mut model = Model {
            nodes: vec![
                node(0, 0.0, 0.0, 0.0),
                node(1, 0.0, 0.0, 4000.0),
                node(2, 6000.0, 0.0, 0.0),
                node(3, 6000.0, 0.0, 4000.0),
            ],
            elements: vec![line(0, 0, 1, 0), line(1, 0, 2, 1), line(2, 1, 3, 1)],
            sections: vec![col_sec, beam_sec],
            materials,
            ..Default::default()
        };
        model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];

        let adj = ColumnMechanismTopology::new(&model);
        let sum = compute_column_mechanism_sum_my(
            &model,
            &adj,
            &model.elements[0],
            -800_000.0,
            -800_000.0,
            -1_000_000.0,
            -1_000_000.0,
            1.0,
        )
        .expect("柱として算定できる");
        let qy = sum
            .qy
            .unwrap()
            .expect("SRC 梁付き方向は端軸力の Mu_i+Mu_j で確定");
        let qz = sum.qz.unwrap().expect("梁無し方向は柱ヒンジで確定");
        assert!((qy - qz).abs() / qy < 1e-9, "qy={qy}, qz={qz}");
    }

    #[test]
    fn diagonal_beam_assigns_to_one_direction_only() {
        let axis = [1.0_f64 / 2.0_f64.sqrt(), 1.0 / 2.0_f64.sqrt()];
        let strong = [1.0, 0.0];
        let weak = [0.0, 1.0];
        assert!(aligns_exclusively(strong, weak, axis, true));
        assert!(!aligns_exclusively(weak, strong, axis, false));
    }

    /// 梁のない鉛直柱 1 本だけのモデル。ΣMy は両端の柱 Mu 和になる。
    fn column_only_model(shape: SectionShape) -> Model {
        use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
        use sepika_core::model::{
            ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis,
            Material, MaterialCategory, Node, RigidZone,
        };
        use sepika_core::Dof6Mask;
        use smallvec::smallvec;

        let materials = vec![
            Material {
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 21_000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205_000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(345.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ];
        let mut sec = shape.to_section(SectionId(0), "C".into());
        sec.frame_use = Some(FrameSectionUse::Column);
        sec.material = Some(MaterialId(0));
        sec.rebar_material = Some(MaterialId(1));
        let node = |id: u32, z: f64| Node {
            id: NodeId(id),
            coord: [0.0, 0.0, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        Model {
            nodes: vec![node(0, 0.0), node(1, 4000.0)],
            elements: vec![ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: smallvec![NodeId(0), NodeId(1)],
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [1.0, 0.0, 0.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: RigidZone::default(),
                end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
                plastic_zone: None,
                spring: None,
            }],
            sections: vec![sec],
            materials,
            ..Default::default()
        }
    }

    fn sum_my_of(model: &Model) -> (Option<f64>, Option<f64>) {
        let adj = ColumnMechanismTopology::new(model);
        let sum_my = compute_column_mechanism_sum_my(
            model,
            &adj,
            &model.elements[0],
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        )
        .expect("柱として算定できる");
        (sum_my.qy.unwrap(), sum_my.qz.unwrap())
    }

    /// 柱 1 本の上端（節点 j）に X 方向の梁 1 本を取り付けたモデル。
    /// 柱の強軸たわみ方向 ey は X を向き、梁は強軸方向へ割り当てられる。
    fn column_with_beam(beam_shape: SectionShape) -> Model {
        use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
        use sepika_core::model::{
            ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis,
            Material, MaterialCategory, Node, RigidZone,
        };
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
        use sepika_core::Dof6Mask;
        use smallvec::smallvec;

        let materials = vec![
            Material {
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 21_000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            Material {
                id: MaterialId(1),
                name: "SD345".into(),
                category: MaterialCategory::Rebar,
                young: 205_000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(345.0),
                concrete_class: Default::default(),
                strength_factor: None,
            },
        ];
        let col_shape = SectionShape::RcColumnRect {
            b: 600.0,
            d: 700.0,
            rebar: RcRectColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                x: vec![4],
                y: vec![4],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        };
        let mut col_sec = col_shape.to_section(SectionId(0), "C".into());
        col_sec.frame_use = Some(FrameSectionUse::Column);
        col_sec.material = Some(MaterialId(0));
        col_sec.rebar_material = Some(MaterialId(1));
        let mut beam_sec = beam_shape.to_section(SectionId(1), "B".into());
        beam_sec.frame_use = Some(FrameSectionUse::Girder);
        beam_sec.material = Some(MaterialId(0));
        beam_sec.rebar_material = Some(MaterialId(1));

        let node = |id: u32, x: f64, z: f64| Node {
            id: NodeId(id),
            coord: [x, 0.0, z],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let line = |id: u32, n0: u32, n1: u32, sid: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec![NodeId(n0), NodeId(n1)],
            section: Some(SectionId(sid)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        };
        let mut model = Model {
            nodes: vec![
                node(0, 0.0, 0.0),
                node(1, 0.0, 4000.0),
                node(2, 6000.0, 4000.0),
            ],
            elements: vec![line(0, 0, 1, 0), line(1, 1, 2, 1)],
            sections: vec![col_sec, beam_sec],
            materials,
            ..Default::default()
        };
        model.elements[0].local_axis.ref_vector = [1.0, 0.0, 0.0];
        model
    }

    /// 上下非対称の `RcBeamRect` は ΣMy に大きい引張側（上端引張）の My を用いる。
    #[test]
    fn new_beam_rect_sum_my_uses_larger_tension_side() {
        use sepika_core::section_shape::{one_bar_area, BeamStirrup, RcBeamRebar};

        let beam = |top: Vec<u32>, bottom: Vec<u32>| SectionShape::RcBeamRect {
            b: 400.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                top,
                bottom,
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };

        let sum = sum_my_of(&column_with_beam(beam(vec![4], vec![2])));
        let s = sum.0.unwrap();

        let a1 = one_bar_area(22.0);
        let sigma_y = 345.0;
        let col_my = 0.8 * 4.0 * a1 * sigma_y * 700.0;
        let beam_bottom = 0.9 * 2.0 * a1 * sigma_y * 539.0;
        let beam_top = 0.9 * 4.0 * a1 * sigma_y * 539.0;
        let expected = col_my + beam_top;
        let if_bottom_tension = col_my + beam_bottom;
        assert!((s - expected).abs() < 1e-6, "s={s}, expected={expected}");
        assert!(
            (s - if_bottom_tension).abs() > 1.0,
            "下端引張を採用してはならない: s={s}"
        );
    }

    /// 新型 `RcColumnRect` の ΣMy が段数違いの等価断面と一致する。
    ///
    /// 最外段 4 本（`x:[4]`）は段構成（1段・2段）によらず引張側 `at` が等しく、
    /// 軸力 0 では `Mu = 0.8·at·σy·D` のため両者は一致する。
    #[test]
    fn new_column_rect_sum_my_matches_legacy_equivalent() {
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};

        let new_shape = SectionShape::RcColumnRect {
            b: 600.0,
            d: 700.0,
            rebar: RcRectColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                x: vec![4],
                y: vec![4],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        };
        let equivalent_shape = SectionShape::RcColumnRect {
            b: 600.0,
            d: 700.0,
            rebar: RcRectColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                x: vec![4, 2],
                y: vec![4],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        };

        let new_sum = sum_my_of(&column_only_model(new_shape));
        let equivalent_sum = sum_my_of(&column_only_model(equivalent_shape));

        let (ns, nw) = (new_sum.0.unwrap(), new_sum.1.unwrap());
        assert!(ns > 0.0 && nw > 0.0, "new_sum={new_sum:?}");
        assert!(
            (ns - equivalent_sum.0.unwrap()).abs() < 1e-6
                && (nw - equivalent_sum.1.unwrap()).abs() < 1e-6,
            "new={new_sum:?}, equivalent={equivalent_sum:?}"
        );
    }

    /// 新型 `RcColumnCircle`（等価正方形）の ΣMy は両方向で同値の非ゼロを返す。
    #[test]
    fn new_column_circle_sum_my_is_nonzero_and_isotropic() {
        use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar};

        let shape = SectionShape::RcColumnCircle {
            d: 600.0,
            rebar: RcCircleColumnRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                count: 8,
                cover: 40.0,
                hoop: CircleColumnHoop {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                },
            },
        };
        let sum = sum_my_of(&column_only_model(shape));
        let (s, w) = (sum.0.unwrap(), sum.1.unwrap());
        assert!(s > 0.0 && w > 0.0, "sum={sum:?}");
        assert!((s - w).abs() < 1e-9, "円形柱は両方向同値: sum={sum:?}");
    }
}
