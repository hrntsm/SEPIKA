//! 終局検定用の部材需要組み立て。

use std::collections::HashMap;

use sepika_core::ids::ElemId;
use sepika_core::model::ElementKind;
use sepika_core::section_shape::SectionShape;
use sepika_design_jp::ultimate::{MemberDemand, StaticBeamBending};
use sepika_element::frame::beam::MemberForces;
use sepika_solver::nonlinear::pushover::PushoverMemberResponse;

/// 静的内力の各部材について、せん断力 `|Q|` の最大値を QL マップにする。
pub fn q_long_map_from_member_forces(
    member_forces: &[(ElemId, MemberForces)],
) -> HashMap<ElemId, f64> {
    member_forces
        .iter()
        .map(|(id, mf)| (*id, max_abs_force(mf, 1)))
        .collect()
}

/// 静的内力から終局検定用の部材需要を組み立てる。
///
/// 軸力は引張正の始端値を圧縮正へ変換し、曲げは部材内の最大絶対値を採用する。
/// この入口の結果は静的RC梁では強軸曲げの符号を未確認として扱う。
/// `q_long_by_elem` がない場合は強軸 QL を未設定とする。弱軸 QL は設定しない。
pub fn member_demand_from_static_forces(
    member_forces: &[(ElemId, MemberForces)],
    q_long_by_elem: Option<&HashMap<ElemId, f64>>,
) -> Vec<(ElemId, MemberDemand)> {
    member_forces
        .iter()
        .filter_map(|(id, mf)| {
            Some((
                *id,
                MemberDemand {
                    q_long: q_long_by_elem.and_then(|m| m.get(id).copied()),
                    static_beam_bending: StaticBeamBending::Unverified,
                    ..static_member_demand(mf)?
                },
            ))
        })
        .collect()
}

fn static_member_demand(mf: &MemberForces) -> Option<MemberDemand> {
    Some(MemberDemand {
        n_axial: -mf.at.first()?.1[0],
        mz: max_abs_force(mf, 5),
        my: max_abs_force(mf, 4),
        ..Default::default()
    })
}

fn max_abs_force(mf: &MemberForces, component: usize) -> f64 {
    mf.at
        .iter()
        .map(|(_, force)| force[component].abs())
        .fold(0.0, f64::max)
}

/// 増分解析（プッシュオーバー）応答から終局検定用の部材需要を組み立てる。
///
/// 応答が空の場合は `None`。`q_long_by_elem` は `None` なら未設定。
pub fn member_demand_from_pushover(
    member_response: &[PushoverMemberResponse],
    q_long_by_elem: Option<&HashMap<ElemId, f64>>,
) -> Option<Vec<(ElemId, MemberDemand)>> {
    if member_response.is_empty()
        || member_response.iter().any(|r| {
            r.wall_shear_signed.is_some()
                || [
                    r.axial,
                    r.m_strong,
                    r.m_weak,
                    r.shear_strong,
                    r.shear_weak,
                    r.rp,
                ]
                .iter()
                .any(|v| !v.is_finite())
        })
    {
        return None;
    }
    Some(
        member_response
            .iter()
            .map(|r| {
                let mut d = MemberDemand::from_pushover(
                    r.axial,
                    r.m_strong,
                    r.m_weak,
                    r.shear_strong,
                    r.shear_weak,
                    r.rp,
                );
                d.q_long = q_long_by_elem.and_then(|m| m.get(&r.elem).copied());
                (r.elem, d)
            })
            .collect(),
    )
}

/// 保存長期termsの同一部材・同一位置6成分から、両軸の QL を含む静的ヒンジ需要を作る。
/// RC矩形梁の強軸曲げは最大絶対値をとる位置の符号と、両符号の有無を保持する。
/// 必要な応力が不完全な場合や、引張専用ブレースを反復解析する場合は理由付きで拒否する。
pub fn complete_ultimate_static_demand<F>(
    model: &sepika_core::model::Model,
    force_of: F,
) -> Result<Vec<(ElemId, MemberDemand)>, String>
where
    F: FnMut(sepika_core::ids::LoadCaseId) -> Result<Vec<(ElemId, MemberForces)>, String>,
{
    let selected_terms = model
        .ultimate_gravity_combination
        .and_then(|index| model.combinations.get(index))
        .map(|combination| {
            combination
                .terms
                .iter()
                .map(|(case, factor)| format!("case={} coefficient={factor}", case.0))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "unresolved".into());
    complete_static_demand(model, force_of).map_err(|reason: String| {
        let member = if reason.contains("member=") { "" } else { " member=unresolved" };
        format!("{reason}; mode=static_hinge purpose=frame_long selected_terms=[{selected_terms}]{member}")
    })
}

fn complete_static_demand<F>(
    model: &sepika_core::model::Model,
    mut force_of: F,
) -> Result<Vec<(ElemId, MemberDemand)>, String>
where
    F: FnMut(sepika_core::ids::LoadCaseId) -> Result<Vec<(ElemId, MemberForces)>, String>,
{
    let terms = sepika_core::load_combo::ultimate_gravity_terms(model)?;
    if model.stress_cfg.tension_only_iteration
        && model
            .elements
            .iter()
            .any(|elem| matches!(elem.kind, ElementKind::Brace { tension_only: true }))
    {
        return Err("gravity_superposition_unsupported: 引張専用ブレースの反復解析では、各ケースと長期架構用組合せ荷重に対するブレースの有効状態が同一と確認できないため、重力応力を合成できません".into());
    }
    let required: Vec<_> = model
        .elements
        .iter()
        .filter_map(
            |elem| match model.element_section(elem).and_then(|s| s.shape.as_ref())? {
                SectionShape::RcBeamRect { .. } => Some((elem.id, true)),
                SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::CftBox { .. }
                | SectionShape::CftPipe { .. } => Some((elem.id, false)),
                _ => None,
            },
        )
        .collect();
    let mut combined: Option<Vec<(ElemId, MemberForces)>> = None;
    for (case, factor) in terms {
        let mut forces = force_of(case).map_err(|reason| {
            format!(
                "gravity_case_missing: case={} factor={} purpose=frame_long: {reason}",
                case.0, factor
            )
        })?;
        forces.retain(|(id, _)| required.iter().any(|(required_id, _)| required_id == id));
        let mut seen = std::collections::HashSet::new();
        for (id, mf) in &forces {
            if !seen.insert(*id) {
                return Err(format!(
                    "gravity_member_duplicate: case={} member={}",
                    case.0, id.0
                ));
            }
            if mf.at.is_empty()
                || mf.at.iter().any(|(xi, f)| {
                    !xi.is_finite() || !(0.0..=1.0).contains(xi) || f.iter().any(|v| !v.is_finite())
                })
                || mf.at.windows(2).any(|rows| rows[0].0 >= rows[1].0)
                || mf.at.first().is_none_or(|r| r.0 != 0.0)
                || mf.at.last().is_none_or(|r| r.0 != 1.0)
            {
                return Err(format!(
                    "gravity_positions_invalid: case={} member={}",
                    case.0, id.0
                ));
            }
        }
        for (id, _) in &required {
            if !seen.contains(id) {
                return Err(format!(
                    "gravity_member_missing: case={} member={} purpose=frame_long",
                    case.0, id.0
                ));
            }
        }
        if let Some(sum) = &mut combined {
            if sum.len() != forces.len() {
                return Err(format!("gravity_grid_mismatch: case={}", case.0));
            }
            for (id, mf) in sum {
                let other = forces
                    .iter()
                    .find(|(candidate, _)| candidate == id)
                    .ok_or_else(|| {
                        format!("gravity_member_missing: case={} member={}", case.0, id.0)
                    })?;
                if mf.at.len() != other.1.at.len()
                    || mf.at.iter().zip(&other.1.at).any(|(a, b)| a.0 != b.0)
                {
                    return Err(format!(
                        "gravity_grid_mismatch: case={} member={}",
                        case.0, id.0
                    ));
                }
                for ((_, a), (_, b)) in mf.at.iter_mut().zip(&other.1.at) {
                    for (value, component) in a.iter_mut().zip(b) {
                        *value += factor * component;
                    }
                }
            }
        } else {
            let mut initial = forces;
            for (_, mf) in &mut initial {
                for (_, f) in &mut mf.at {
                    for v in f {
                        *v *= factor;
                    }
                }
            }
            combined = Some(initial);
        }
    }
    let forces = combined.ok_or("gravity_terms_empty: 長期架構用termsがありません")?;
    if let Some((member, _)) = forces
        .iter()
        .find(|(_, mf)| mf.at.iter().any(|(_, f)| f.iter().any(|v| !v.is_finite())))
    {
        return Err(format!(
            "gravity_sum_nonfinite: member={} 重力応力の合成値が非有限です",
            member.0
        ));
    }
    Ok(forces
        .iter()
        .filter_map(|(id, mf)| {
            let mut demand = static_member_demand(mf)?;
            demand.q_long = Some(max_abs_force(mf, 1));
            demand.q_long_weak = Some(max_abs_force(mf, 2));
            if required.contains(&(*id, true)) {
                let (min, max) = mf.at.iter().fold((0.0_f64, 0.0_f64), |(min, max), (_, f)| {
                    (min.min(f[5]), max.max(f[5]))
                });
                demand.mz = if -min > max { min } else { max };
                if min < 0.0 && max > 0.0 {
                    demand.static_beam_bending = StaticBeamBending::Mixed;
                }
            }
            Some((*id, demand))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_element::frame::beam::MemberForces;

    fn mf(rows: &[(f64, f64, f64, f64)]) -> MemberForces {
        // (pos, N, Qy, Mz)
        MemberForces {
            at: rows
                .iter()
                .map(|&(p, n, q, m)| (p, [n, q, 0.0, 0.0, 0.0, m]))
                .collect(),
        }
    }

    #[test]
    fn static_none_leaves_ql_unset() {
        let forces = vec![(
            ElemId(0),
            mf(&[
                (0.0, 1000.0, 50_000.0, 1.0e6),
                (1.0, 1000.0, 10_000.0, 2.0e6),
            ]),
        )];
        let demand = member_demand_from_static_forces(&forces, None);
        assert_eq!(demand.len(), 1);
        let (_, d) = &demand[0];
        assert!((d.n_axial + 1000.0).abs() < 1e-12);
        assert!((d.mz - 2.0e6).abs() < 1e-6);
        assert!(d.q_long.is_none());
    }

    #[test]
    fn static_explicit_ql() {
        let forces = vec![(
            ElemId(0),
            mf(&[(0.0, 0.0, 50_000.0, 0.0), (1.0, 0.0, 10_000.0, 0.0)]),
        )];
        let ql = q_long_map_from_member_forces(&forces);
        let demand = member_demand_from_static_forces(&forces, Some(&ql));
        let (_, d) = &demand[0];
        assert_eq!(d.q_long, Some(50_000.0));
    }

    pub(super) fn axial_column(
        force_z: f64,
        reverse_nodes: bool,
        ref_vector: [f64; 3],
    ) -> sepika_core::model::Model {
        use sepika_core::dof::Dof6Mask;
        use sepika_core::ids::{LoadCaseId, MaterialId, NodeId, SectionId};
        use sepika_core::model::*;
        let mut section = Section::zero(SectionId(0), "軸力対照".into());
        section.frame_use = Some(FrameSectionUse::Column);
        section.material = Some(MaterialId(0));
        section.area = 1000.0;
        section.iy = 1e7;
        section.iz = 1e7;
        section.j = 1e6;
        section.as_y = 800.0;
        section.as_z = 800.0;
        section.depth = 100.0;
        section.width = 100.0;
        let mut nodes = vec![NodeId(0), NodeId(1)];
        if reverse_nodes {
            nodes.reverse();
        }
        Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: [0.0; 3],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: [0.0, 0.0, 3000.0],
                    restraint: Dof6Mask::FREE,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
            ],
            elements: vec![ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: nodes.into(),
                section: Some(SectionId(0)),
                local_axis: LocalAxis { ref_vector },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
                plastic_zone: None,
                spring: None,
            }],
            sections: vec![section],
            materials: vec![Material {
                id: MaterialId(0),
                name: "鋼".into(),
                category: MaterialCategory::Steel,
                young: 200000.0,
                poisson: 0.3,
                density: 0.0,
                shear: None,
                fc: None,
                fy: Some(235.0),
                strength_factor: None,
                concrete_class: Default::default(),
            }],
            load_cases: vec![LoadCase {
                origin: Default::default(),
                id: LoadCaseId(0),
                name: "鉛直軸力".into(),
                kind: LoadCaseKind::Dead,
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, force_z, 0.0, 0.0, 0.0],
                )],
                member: vec![],
            }],
            ..Default::default()
        }
    }

    #[test]
    fn solved_column_axial_demand_keeps_physical_sign_when_node_order_or_local_axes_change() {
        use sepika_core::ids::LoadCaseId;
        for load_z in [-150000.0, 150000.0, 0.0] {
            for reverse in [false, true] {
                for ref_vector in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                    let result = crate::compute::compute_linear_static(
                        axial_column(load_z, reverse, ref_vector),
                        LoadCaseId(0),
                    )
                    .unwrap();
                    for (_, forces) in &result.member_forces {
                        for (_, values) in &forces.at {
                            assert!((values[0] - load_z).abs() < 1e-6);
                        }
                    }
                    let demand = member_demand_from_static_forces(&result.member_forces, None);
                    assert_eq!(demand.len(), 1);
                    assert!((demand[0].1.n_axial + load_z).abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn pushover_empty_returns_none() {
        assert!(member_demand_from_pushover(&[], None).is_none());
    }

    #[test]
    fn pushover_maps_response_and_optional_ql() {
        let resp = [PushoverMemberResponse {
            elem: ElemId(3),
            axial: 2000.0,
            m_strong: 3.0e6,
            m_weak: 1.0e6,
            shear_strong: 40_000.0,
            shear_weak: 5_000.0,
            rp: 0.01,
            horizontal_force: 0.0,
            wall_shear_signed: None,
        }];
        let mut ql = HashMap::new();
        ql.insert(ElemId(3), 8_000.0);
        let demand = member_demand_from_pushover(&resp, Some(&ql)).unwrap();
        assert_eq!(demand.len(), 1);
        let (id, d) = &demand[0];
        assert_eq!(*id, ElemId(3));
        assert!((d.n_axial - 2000.0).abs() < 1e-12);
        assert_eq!(d.q_long, Some(8_000.0));
        for axial in [-2000.0, 0.0, 2000.0] {
            let signed = [PushoverMemberResponse { axial, ..resp[0] }];
            assert_eq!(
                member_demand_from_pushover(&signed, None).unwrap()[0]
                    .1
                    .n_axial,
                axial
            );
        }
    }
}

#[cfg(test)]
mod wall_availability_tests {
    use super::*;
    #[test]
    fn line_demand_rejects_legacy_wall_zero_and_nonfinite_but_preserves_measured_zero() {
        let mut response = PushoverMemberResponse {
            elem: ElemId(0),
            m_strong: 0.0,
            m_weak: 0.0,
            shear_strong: 0.0,
            shear_weak: 0.0,
            axial: 0.0,
            rp: 0.0,
            horizontal_force: 0.0,
            wall_shear_signed: None,
        };
        let demand = member_demand_from_pushover(&[response], None).unwrap();
        assert_eq!(demand.len(), 1);
        assert_eq!(demand[0].1.n_axial, 0.0);
        response.wall_shear_signed = Some(0.0);
        assert!(member_demand_from_pushover(&[response], None).is_none());
        response.wall_shear_signed = None;
        response.m_strong = f64::NAN;
        assert!(member_demand_from_pushover(&[response], None).is_none());
    }
}

#[cfg(test)]
#[path = "ultimate565_tests.rs"]
mod gravity_contract_tests;
