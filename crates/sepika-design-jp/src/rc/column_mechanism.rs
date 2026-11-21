//! RC 柱の短期設計せん断力用 ΣMy（崩壊メカニズム判定）。

use std::collections::HashSet;

use sepika_core::adjacency::NodeAdjacency;
use sepika_core::geom::vec3::{norm, sub};
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, ElementKind, EndCondition, Material, Model};
use sepika_core::rc_capacity::{rc_column_mu_simple, rc_mu_simple, RcCapacityInput};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::rigid_arm::resolve_lengths;
use sepika_element::transform::LocalFrame;
use sepika_solver::common::constraint::Reducer;

use super::section_props::{axis_props_from_shape, rebar_info_from_shape, AxisProps};
use crate::material_strength::{main_rebar_category_issue, rebar_sigma_y_of};
use crate::member_design_check::combine_incomplete_reasons;
use crate::ultimate::rc_props::RcDirection;
use crate::MemberKind;

/// 柱端のヒンジ種別。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnEndHinge {
    /// 梁ヒンジ。
    Beam,
    /// 柱ヒンジ。
    Column,
}

/// 1 端のヒンジを判定する。
/// `force_column_hinge`: その端を強制的に柱ヒンジにする場合 true。
pub fn resolve_column_end_hinge(
    column_my: f64,
    sum_beam_my: f64,
    force_column_hinge: bool,
) -> ColumnEndHinge {
    if force_column_hinge || sum_beam_my <= 1e-9 || column_my <= sum_beam_my + 1e-9 {
        ColumnEndHinge::Column
    } else {
        ColumnEndHinge::Beam
    }
}

/// 両端ヒンジから ΣMy を組み立てる。両端とも梁降伏なら下端を柱降伏へ落とす。
/// 柱頭の梁寄与に係数を掛ける。係数が `None` の柱頭は柱降伏とする。
#[allow(clippy::too_many_arguments)]
pub fn sum_my_from_end_hinges(
    hinge_i: ColumnEndHinge,
    hinge_j: ColumnEndHinge,
    column_my_i: f64,
    column_my_j: f64,
    sum_beam_my_i: f64,
    sum_beam_my_j: f64,
    i_is_bottom: bool,
    head_beam_factor: Option<f64>,
) -> f64 {
    let (mut hi, mut hj) = (hinge_i, hinge_j);
    if hi == ColumnEndHinge::Beam && hj == ColumnEndHinge::Beam {
        if i_is_bottom {
            hi = ColumnEndHinge::Column;
        } else {
            hj = ColumnEndHinge::Column;
        }
    }
    let contrib = |h, col_my, beam_sum, beam_factor: Option<f64>| match (h, beam_factor) {
        (ColumnEndHinge::Beam, Some(factor)) => factor * beam_sum,
        _ => col_my,
    };
    let (factor_i, factor_j) = if i_is_bottom {
        (Some(0.5), head_beam_factor)
    } else {
        (head_beam_factor, Some(0.5))
    };
    contrib(hi, column_my_i, sum_beam_my_i, factor_i)
        + contrib(hj, column_my_j, sum_beam_my_j, factor_j)
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

fn sum_beam_my_at_node(
    model: &Model,
    adjacency: &NodeAdjacency,
    node: NodeId,
    load_h: [f64; 2],
    peer_h: [f64; 2],
    prefer_on_tie: bool,
) -> Result<Option<f64>, String> {
    let mut sum = Some(0.0);
    for other in adjacency.elements_at(model, node) {
        let (Some(n0), Some(n1)) = (
            other.nodes.first().and_then(|n| model.nodes.get(n.index())),
            other.nodes.get(1).and_then(|n| model.nodes.get(n.index())),
        ) else {
            continue;
        };
        let axis = sub(n1.coord, n0.coord);
        let aligns = |axis| {
            horizontal_unit(axis)
                .is_some_and(|axis_h| aligns_exclusively(load_h, peer_h, axis_h, prefer_on_tie))
        };
        let has_offset = other.end_offsets_mm != [[0.0; 3]; 2];
        if !aligns(axis)
            && !(has_offset
                && model
                    .primary_frame_axis_end_positions(other)
                    .ok()
                    .flatten()
                    .is_some_and(|[i, j]| aligns(sub(j, i))))
        {
            continue;
        }
        match MemberKind::try_of_element(other, model) {
            Ok(MemberKind::Girder) => {}
            Ok(_) => continue,
            Err(_) => {
                sum = None;
                continue;
            }
        }
        if has_offset {
            sum = None;
            continue;
        }
        let end = usize::from(n1.id == node);
        match other.end_cond[end] {
            EndCondition::Fixed => {}
            EndCondition::Pinned | EndCondition::SemiRigid { k_theta: 0.0 } => {
                let rigid = model.analysis_rigid_zone(other);
                let (i, j) =
                    resolve_lengths(rigid.rigid_length_i(), rigid.rigid_length_j(), norm(axis));
                // ピンでも可撓端のせん断力は剛域を介して節点に曲げを伝える。
                if [i, j][end] > 0.0 {
                    sum = None;
                }
                continue;
            }
            EndCondition::SemiRigid { .. } => {
                sum = None;
                continue;
            }
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
            return Err(format!(
                "大梁 ID {} の断面「{}」の{reason}",
                other.id.0, sec.name
            ));
        }
        sum = sum
            .zip(beam_my_simple(shape, material))
            .map(|(sum, my)| sum + my)
            .filter(|sum| sum.is_finite());
    }
    Ok(sum)
}

/// 柱の方向別降伏モーメント和を求めるモデルの接続情報。
pub struct ColumnMechanismTopology {
    frame: NodeAdjacency,
    linked_nodes: HashSet<NodeId>,
}

impl ColumnMechanismTopology {
    /// 検定対象モデルの主架構線材と、別節点との連結拘束を索引化する。
    pub fn new(model: &Model) -> Self {
        Self {
            frame: NodeAdjacency::primary_frame(model),
            linked_nodes: Reducer::explicit_constraint_linked_nodes(model),
        }
    }

    fn head_beam_factor(&self, model: &Model, head: NodeId, head_z_mm: f64) -> Option<f64> {
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
            Some(0.5)
        } else if self.linked_nodes.contains(&head) {
            None
        } else {
            Some(1.0)
        }
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
    let frame = LocalFrame::from_nodes(p_i, p_j, column.local_axis.ref_vector)
        .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", column.id.0));
    let ey = frame.rot[1];
    let ez = frame.rot[2];
    let load_strong = horizontal_unit(ey)?;
    let load_weak = horizontal_unit(ez)?;

    let i_is_bottom = p_i[2] <= p_j[2];
    let design_n = design_axial(i_is_bottom);
    let (head, head_z_mm) = if i_is_bottom {
        (nj, p_j[2])
    } else {
        (ni, p_i[2])
    };
    let head_beam_factor = topology.head_beam_factor(model, head, head_z_mm);

    let sum_for = |strong: bool, load_h: [f64; 2], peer_h: [f64; 2]| {
        let beam_sum =
            |node| sum_beam_my_at_node(model, &topology.frame, node, load_h, peer_h, strong);
        let beam_ends = beam_sum(ni).and_then(|bi| beam_sum(nj).map(|bj| (bi, bj)));
        let (n, beams) = match (&design_n, beam_ends) {
            (Ok(n), Ok(beams)) => (*n, beams),
            (n, beams) => {
                let axis = if strong { "Qy" } else { "Qz" };
                let reason =
                    combine_incomplete_reasons(beams.err(), n.as_ref().err().map(String::as_str))
                        .expect("検定に必要な梁または柱端軸力を取得できない");
                return Err(format!(
                    "RC柱 ID {} の {axis} 方向のせん断検定: {reason}",
                    column.id.0
                ));
            }
        };
        let (Some(col_i), Some(col_j)) = (
            column_my_at_n(model, column, n[0], strong),
            column_my_at_n(model, column, n[1], strong),
        ) else {
            return Ok(None);
        };
        match beams {
            (Some(bi), Some(bj)) if column.end_offsets_mm == [[0.0; 3]; 2] => {
                let hi = resolve_column_end_hinge(col_i, bi, false);
                let hj = resolve_column_end_hinge(col_j, bj, false);
                Ok(Some(sum_my_from_end_hinges(
                    hi,
                    hj,
                    col_i,
                    col_j,
                    bi,
                    bj,
                    i_is_bottom,
                    head_beam_factor,
                )))
            }
            _ => Ok(Some(col_i + col_j)),
        }
    };

    Some(ColumnMechanism {
        qy: sum_for(true, load_strong, load_weak),
        qz: sum_for(false, load_weak, load_strong),
        missing_gravity_axial,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hinge_column_when_no_beams_or_weaker_beams() {
        assert_eq!(
            resolve_column_end_hinge(100.0, 0.0, false),
            ColumnEndHinge::Column
        );
        assert_eq!(
            resolve_column_end_hinge(100.0, 100.0, false),
            ColumnEndHinge::Column
        );
        assert_eq!(
            resolve_column_end_hinge(100.0, 80.0, false),
            ColumnEndHinge::Beam
        );
        assert_eq!(
            resolve_column_end_hinge(100.0, 200.0, true),
            ColumnEndHinge::Column
        );
    }

    #[test]
    fn both_beam_hinges_force_bottom_to_column() {
        // 両端梁 → 下端(i) を柱へ。ΣMy = col_i + 0.5*beam_j
        let s = sum_my_from_end_hinges(
            ColumnEndHinge::Beam,
            ColumnEndHinge::Beam,
            100.0,
            120.0,
            300.0,
            400.0,
            true,
            Some(0.5),
        );
        assert!((s - (100.0 + 0.5 * 400.0)).abs() < 1e-9, "s={s}");
    }

    #[test]
    fn mixed_hinges_add_contributions() {
        // i 柱 + j 梁 → col_i + 0.5*beam_j
        let s = sum_my_from_end_hinges(
            ColumnEndHinge::Column,
            ColumnEndHinge::Beam,
            100.0,
            120.0,
            300.0,
            400.0,
            true,
            Some(0.5),
        );
        assert!((s - (100.0 + 200.0)).abs() < 1e-9);
    }

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
