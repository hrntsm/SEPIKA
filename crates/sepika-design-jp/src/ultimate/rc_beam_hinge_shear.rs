use std::cell::OnceCell;
use std::collections::HashMap;
use std::fmt::Write;

use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::Dof;
use sepika_core::face_distance::girder_column_faces;
use sepika_core::geom::vec3::local_frame_axes;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementData, MaterialCategory, Model};
use sepika_solver::statics::analysis::SeismicDir;

use crate::rc::{
    beam_supports_slab, column_transmits_bending, frame_end_positions, restrains_direction,
    transmits_bending,
};
use crate::MemberKind;

use super::options::{MemberDemand, StaticBeamHingeMechanism};

/// 同じ水平載荷軸の正・負地震荷重ごとのRC梁両端曲げ降伏によるせん断需要の入力。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamHingeShearDemand {
    /// 全体座標系の載荷軸。梁の局所曲げ軸・符号とは異なる。
    pub seismic_axis: SeismicDir,
    /// 大梁の実材軸i端を原点とする実支持フェース位置 [mm]。i/j順で正の内法を取り、
    /// `sepika_core::face_distance::girder_column_faces` の位置とそれぞれ厳密に一致する値を指定する。
    pub face_positions_mm: [f64; 2],
    /// 選択済み長期架構用組合せで、実フェース間を単純支持とした端別断面せん断力 [N]。局所Qyの符号、i/j順。
    pub q0_n: [f64; 2],
    /// 内法内の各断面の符号付き常時せん断力が、端別の Q0 の最小と最大の間にあると確認した場合だけ true。
    pub end_envelope_confirmed: bool,
    /// 正地震荷重の確認結果。未確認は None。
    pub positive: Option<RcUltimateBeamHingeShearDirectionDemand>,
    /// 負地震荷重の確認結果。未確認は None。
    pub negative: Option<RcUltimateBeamHingeShearDirectionDemand>,
}

/// 一つの地震荷重方向で確認したRC梁両端曲げ降伏の条件。
#[derive(Clone, Copy, Debug)]
pub struct RcUltimateBeamHingeShearDirectionDemand {
    /// 実支持フェースで強軸曲げ降伏が先行し、フェース間の地震時強軸せん断が両端曲げだけで釣り合うと確認した機構。
    pub mechanism: StaticBeamHingeMechanism,
    /// 各端の引張側の実配筋、定着、材料・係数を確認した、実際の耐力を下回らない正の強軸曲げ耐力 [N·mm]。i/j順。
    pub yield_moment_bounds_nmm: [f64; 2],
    /// 指定方向と曲げ耐力の組で、両端の支持・接合による強軸曲げ伝達を設計者が確認した場合だけ true。
    pub support_transfer_confirmed: bool,
}

/// 正・負地震荷重それぞれの両端降伏による要求せん断力。各方向に数値または未算定理由を返す。
#[derive(Clone, Debug)]
pub struct RcUltimateBeamHingeShearChecks {
    pub seismic_axis: SeismicDir,
    pub positive: Result<RcUltimateBeamHingeShear, String>,
    pub negative: Result<RcUltimateBeamHingeShear, String>,
}

/// RC梁の両端曲げ降伏から確認できた設計用せん断力の一枝。耐力との適否判定ではない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RcUltimateBeamHingeShear {
    /// 大梁の実材軸i端を原点とする実支持フェース位置 [mm]。i/j順。
    pub face_positions_mm: [f64; 2],
    /// 実支持フェース間の内法 [mm]。
    pub clear_span_mm: f64,
    /// 両端の曲げ耐力上限を採用した符号付き強軸曲げ [N·mm]。正は下端引張、i/j順。
    pub yield_mz_at_upper_bound_nmm: [f64; 2],
    /// 確認済みの端別常時せん断力 Q0 [N]。局所Qyの符号、i/j順。
    pub q0_n: [f64; 2],
    /// 両端の曲げ耐力上限に対応する符号付き強軸せん断力 Qy [N]。
    pub qy_at_upper_bound_n: f64,
    /// 両端の曲げ耐力上限を用いた QD2 [N]。局所Qyの符号、i/j順。
    pub qd2_at_upper_bound_n: [f64; 2],
    /// 両端降伏によるせん断成分が 0 から上限までの間にあるときの、方向別絶対需要の上限 [N]。
    /// 検討にはこの値を用い、丸めた詳細表示は用いない。
    pub max_abs_qd2_bound_n: f64,
}

impl RcUltimateBeamHingeShearChecks {
    pub(super) fn append_detail(&self, detail: &mut String) {
        for (label, direction) in [("正", &self.positive), ("負", &self.negative)] {
            let _ = write!(
                detail,
                "。{:?}軸{label}の両端降伏枝（従来の比較とは別）: ",
                self.seismic_axis
            );
            match direction {
                Ok(shear) => {
                    let [q0_i, q0_j] = shear.q0_n;
                    let qy = shear.qy_at_upper_bound_n;
                    let [qd2_i, qd2_j] = shear.qd2_at_upper_bound_n;
                    let upper = shear.max_abs_qd2_bound_n;
                    let _ = write!(detail, "参考表示（丸めあり） Q0(i/j)={q0_i:.3}/{q0_j:.3} N, 曲げ上限を用いたQy={qy:.3} N, QD2(i/j)={qd2_i:.3}/{qd2_j:.3} N, 絶対包絡上限={upper:.3} N");
                }
                Err(reason) => detail.push_str(reason),
            }
        }
    }
}

pub(super) const BOND_HINGE_CONFLICT: &str =
    "同じ地震荷重方向で梁の曲げ非降伏と両端曲げ降伏が指定され、矛盾します";

pub(super) fn has_haunch(model: &Model, id: ElemId) -> bool {
    model.member_detail(id).is_some_and(|detail| {
        [detail.haunch_i, detail.haunch_j]
            .into_iter()
            .flatten()
            .any(|haunch| haunch.depth_increase != 0.0 || haunch.width_increase != 0.0)
    })
}

fn verified_faces(
    model: &Model,
    elem: &ElementData,
    kind: MemberKind,
    input: RcUltimateBeamHingeShearDemand,
    adjacency: &OnceCell<NodeAdjacency>,
    connection_counts: &HashMap<NodeId, usize>,
) -> Result<[f64; 2], String> {
    if kind != MemberKind::Girder || !elem.kind.is_primary_frame_support() || elem.nodes.len() != 2
    {
        return Err("両端降伏のせん断需要は矩形RC大梁の主架構線材に限ります".into());
    }
    if !model
        .element_material(elem)
        .is_some_and(|mat| mat.category == MaterialCategory::Concrete)
    {
        return Err("矩形RC大梁のコンクリート材料を確認できません".into());
    }
    for (name, condition) in ["i端", "j端"].into_iter().zip(elem.end_cond) {
        if !transmits_bending(condition) {
            return Err(format!("{name}が曲げ解放または回転剛性不正です"));
        }
    }
    if beam_supports_slab(model, elem)
        || has_haunch(model, elem.id)
        || model
            .girder_groups
            .iter()
            .any(|group| group.len() >= 2 && group.contains(&elem.id))
    {
        return Err("スラブ付き、ハンチ付き、または複数要素で構成するRC梁は適用外です".into());
    }
    let [input_i_mm, input_j_mm] = input.face_positions_mm;
    for (name, position_mm) in ["i端", "j端"].into_iter().zip(input.face_positions_mm) {
        if !position_mm.is_finite() {
            return Err(format!("{name}の確認済みフェース位置が非有限です"));
        }
    }
    let input_span_mm = input_j_mm - input_i_mm;
    if !input_span_mm.is_finite() || input_span_mm <= 0.0 {
        return Err("入力の実支持フェース位置に有限で正の内法が成立しません".into());
    }
    let adjacency = adjacency.get_or_init(|| NodeAdjacency::primary_frame(model));
    let [i, j] = girder_column_faces(model, elem, adjacency)
        .ok_or("両端降伏のせん断需要は大梁用途のRC梁に限ります")?;
    let faces = [i?, j?];
    let [i, j] = frame_end_positions(model, elem).ok_or("梁の実材軸を確認できません")?;
    let [_, _, strong_axis] = local_frame_axes(i, j, elem.local_axis.ref_vector)?;
    for (side, (face, input_position_mm)) in faces.iter().zip(input.face_positions_mm).enumerate() {
        let name = ["i端", "j端"][side];
        if face.columns.iter().any(|&id| has_haunch(model, id)) {
            return Err(format!(
                "{name}はハンチ付きの接続柱で、実支持フェースを確認できません"
            ));
        }
        let near = model
            .node(elem.nodes[side])
            .ok_or_else(|| format!("{name}の梁接合節点を確認できません"))?;
        let mut receiving_columns = face
            .columns
            .iter()
            .filter_map(|&id| {
                let column = model.element(id)?;
                let [i, j] = *column.nodes.as_slice() else {
                    return None;
                };
                let (near_side, far) = if i == near.id {
                    (0, j)
                } else if j == near.id {
                    (1, i)
                } else {
                    return None;
                };
                transmits_bending(column.end_cond[near_side]).then_some((column, far))
            })
            .peekable();
        if receiving_columns.peek().is_none() {
            return Err(format!("{name}の接続柱に曲げを伝達できる端部がありません"));
        }
        let other_path = restrains_direction(near, strong_axis, [Dof::Rx, Dof::Ry, Dof::Rz])
            || connection_counts
                .get(&near.id)
                .is_some_and(|&count| count > 1 + face.columns.len());
        if !other_path
            && !receiving_columns.any(|(column, far)| {
                connection_counts.get(&far).is_some_and(|&count| count > 1)
                    || column_transmits_bending(model, column, near, strong_axis)
            })
        {
            return Err(format!(
                "{name}の接続柱が行き止まりか、強軸曲げを受ける直接支持を確認できません"
            ));
        }
        if face.position_mm != input_position_mm {
            return Err(format!(
                "{name}の確認済みフェース位置 {input_position_mm} mm がモデルの実柱面 {} mm と一致しません",
                face.position_mm
            ));
        }
    }
    if !input.end_envelope_confirmed {
        return Err(
            "内法区間の符号付き常時せん断力が両端の Q0 の範囲内にあると確認できません".into(),
        );
    }
    if input.q0_n.iter().any(|value| !value.is_finite()) {
        return Err("常時荷重による端別せん断力 Q0 が非有限です".into());
    }
    Ok(input.face_positions_mm)
}

fn scale_to_n(value: &BigRational) -> Result<f64, String> {
    value
        .to_f64()
        .filter(|&n| n.is_finite() && (n != 0.0 || value.is_zero()))
        .ok_or_else(|| "両端降伏のせん断力が数値の表現範囲外です".into())
}

fn calculate(
    face_positions_mm: [f64; 2],
    q0_n: [f64; 2],
    direction: RcUltimateBeamHingeShearDirectionDemand,
) -> Result<RcUltimateBeamHingeShear, String> {
    if !direction.support_transfer_confirmed {
        return Err(
            "この方向と曲げ耐力の組で、支持・接合による両端の強軸曲げ伝達を確認できません".into(),
        );
    }
    let [i, j] = direction.yield_moment_bounds_nmm;
    for (name, value) in ["i端", "j端"].into_iter().zip([i, j]) {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!(
                "{name}の確認済み曲げ耐力は有限の正値でなければなりません"
            ));
        }
    }
    let [mi, mj] = match direction.mechanism {
        StaticBeamHingeMechanism::ITopJBottom => [-i, j],
        StaticBeamHingeMechanism::IBottomJTop => [i, -j],
    };
    let [xi, xj] = face_positions_mm;
    let clear_span_mm = xj - xi;
    // 中間丸めでは桁あふれのほか、Q0との相殺で微小な非零需要を失う。
    let exact = |value| BigRational::from_float(value).unwrap();
    let qy = (exact(mj) - exact(mi)) / (exact(xj) - exact(xi));
    let qy_n = scale_to_n(&qy)?;
    let q0 = q0_n.map(exact);
    let qd2 = q0.each_ref().map(|q0| q0 + &qy);
    let [qi, qj] = qd2.each_ref().map(scale_to_n);
    let max_abs = q0.iter().chain(&qd2).map(Signed::abs).max().unwrap();
    let mut max_abs_qd2_bound_n = scale_to_n(&max_abs)?;
    if exact(max_abs_qd2_bound_n) < max_abs {
        max_abs_qd2_bound_n = max_abs_qd2_bound_n.next_up();
        if !max_abs_qd2_bound_n.is_finite() {
            return Err("両端降伏のせん断力の包絡上限が数値の表現範囲外です".into());
        }
    }
    Ok(RcUltimateBeamHingeShear {
        face_positions_mm,
        clear_span_mm,
        yield_mz_at_upper_bound_nmm: [mi, mj],
        q0_n,
        qy_at_upper_bound_n: qy_n,
        qd2_at_upper_bound_n: [qi?, qj?],
        max_abs_qd2_bound_n,
    })
}

pub(super) fn check(
    model: &Model,
    elem: &ElementData,
    kind: MemberKind,
    demand: MemberDemand,
    adjacency: &OnceCell<NodeAdjacency>,
    connection_counts: &HashMap<NodeId, usize>,
) -> Option<RcUltimateBeamHingeShearChecks> {
    let input = demand.beam_hinge_shear?;
    let bond = demand
        .beam_bond
        .filter(|bond| bond.seismic_axis == input.seismic_axis);
    let checked_faces = verified_faces(model, elem, kind, input, adjacency, connection_counts);
    let direction = |value: Option<_>, conflict: bool| {
        let value = value.ok_or("この方向の両端曲げ降伏と端別曲げ耐力を確認できません")?;
        if conflict {
            return Err(BOND_HINGE_CONFLICT.into());
        }
        calculate(
            *checked_faces.as_ref().map_err(Clone::clone)?,
            input.q0_n,
            value,
        )
    };
    Some(RcUltimateBeamHingeShearChecks {
        seismic_axis: input.seismic_axis,
        positive: direction(
            input.positive,
            bond.is_some_and(|bond| bond.positive.is_some()),
        ),
        negative: direction(
            input.negative,
            bond.is_some_and(|bond| bond.negative.is_some()),
        ),
    })
}
