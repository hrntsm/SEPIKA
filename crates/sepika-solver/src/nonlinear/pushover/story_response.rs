//! 同じ確定状態の層切断面力と目的別評価点。
use super::{PushoverMemberResponse, PushoverResult, PushoverSign};
use crate::statics::analysis::SeismicDir;
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementKind, Model};
use sepika_element::behavior::{Ctx, ElementBehavior};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EvaluationPurpose {
    Ds,
    HoldingCapacity,
}

/// 採用点は目的・入力・run・方向・確定step・利用者の選定理由を保持する。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EvaluationPoint {
    pub purpose: EvaluationPurpose,
    pub run_id: String,
    pub input_generation: Vec<u8>,
    pub direction: SeismicDir,
    pub step: u32,
    pub selection_reason: String,
    /// 部材群判定用の耐力 `[N]`。同時点の負担力とは別の設計入力。
    pub member_capacities_n: Vec<(ElemId, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ForceGroup {
    Wall,
    Brace,
    Frame,
}

/// 物理要素を一度だけ計上した切断交点の符号付き合力 `[N]`。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CutForce {
    pub elem: ElemId,
    pub group: ForceGroup,
    pub force_n: f64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoryCut {
    pub layer: usize,
    pub elevation_mm: f64,
    pub forces: Vec<CutForce>,
    pub external_n: f64,
    pub reference_n: f64,
    /// 切断面上側の固定支持・厳密ゼロ変位拘束・地盤支持ばねの抵抗 `[N]`。
    pub support_n: f64,
    pub tolerance_n: f64,
    pub unavailable: Option<String>,
}

/// 確定時の非線形内力。最終変位による過去応力の再計算には使わない。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConfirmedStepResponse {
    pub run_id: String,
    pub input_generation: Option<Vec<u8>>,
    pub direction: SeismicDir,
    pub sign: PushoverSign,
    pub step: u32,
    pub members: Vec<PushoverMemberResponse>,
    pub cuts: Vec<StoryCut>,
    pub src_walls: Vec<SrcWallShearReview>,
    pub src_frame_faces: Vec<SrcFrameFaceReview>,
}

/// SRC柱・大梁の物理フェースの確定断面力。元の局所軸、i端・j端順。
/// 成分は [N, Qy, Qz, Mx, My, Mz]、引張正、単位 [N, Nmm]。
/// 分布荷重を含む。非零集中荷重が一致する面は応力が不連続のため Err。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SrcFrameFaceReview {
    pub elem: ElemId,
    pub faces: [Result<[f64; 6], String>; 2],
}

/// SRC耐震壁の同一確定stepで回収した上下の内法断面力と38条のせん断照査 [N]。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SrcWallShearReview {
    pub elem: ElemId,
    pub ends: Option<[sepika_core::src_wall_capacity::SrcWallEndForces; 2]>,
    pub demand_n: Option<f64>,
    pub limit_n: Option<f64>,
    pub unavailable: Option<String>,
}

impl SrcWallShearReview {
    /// 正のせん断力が38条耐力に数値許容差内で到達した場合は true。
    /// 許容差を超える超過、または照査結果が不完全な場合は理由を返す。
    pub fn reached_limit(&self) -> Result<bool, String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        let Some(ends) = &self.ends else {
            return Err("上下端の応力がありません".into());
        };
        if ends.iter().any(|end| {
            [
                end.axial_n,
                end.columns_compression_n,
                end.moment_nmm,
                end.shear_n,
            ]
            .iter()
            .any(|v| !v.is_finite())
        }) {
            return Err("上下端の応力に非有限値があります".into());
        }
        let demand_n = ends[0].shear_n.abs().max(ends[1].shear_n.abs());
        if Some(demand_n) != self.demand_n {
            return Err("上下端の応力と設計せん断力が一致しません".into());
        }
        if demand_n == 0.0 {
            return Ok(false);
        }
        let Some(limit_n) = self.limit_n.filter(|v| v.is_finite() && *v > 0.0) else {
            return Err("38条の終局せん断耐力がありません".into());
        };
        let tolerance_n = crate::common::newton::STATIC_NEWTON.tol * limit_n;
        let difference_n = demand_n - limit_n;
        if difference_n > tolerance_n {
            return Err(format!(
                "38条の終局せん断耐力を超過しました |Q|={demand_n} N Qu={limit_n} N"
            ));
        }
        Ok(difference_n.abs() <= tolerance_n)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StoryForceEvaluation {
    pub layer: usize,
    pub qu_n: f64,
    pub wall_n: f64,
    pub brace_n: f64,
    pub frame_n: f64,
    pub external_n: f64,
    pub reference_n: f64,
    pub support_n: f64,
    pub residual_n: f64,
    pub tolerance_n: f64,
    pub beta_u: f64,
}

impl StoryCut {
    pub fn evaluate(&self) -> Result<StoryForceEvaluation, String> {
        let fail = |s: &str| format!("層 {}: {s}", self.layer + 1);
        if let Some(reason) = &self.unavailable {
            return Err(fail(reason));
        }
        if [
            self.external_n,
            self.reference_n,
            self.support_n,
            self.tolerance_n,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.tolerance_n <= 0.0
        {
            return Err(fail("外力・許容差が不正です"));
        }
        let mut seen = std::collections::HashSet::new();
        let (mut wall, mut brace, mut frame) = (0.0, 0.0, 0.0);
        for force in &self.forces {
            if !seen.insert(force.elem) {
                return Err(fail("物理要素が重複計上されています"));
            }
            if !force.force_n.is_finite() {
                return Err(fail("有限な確定内力がありません"));
            }
            match force.group {
                ForceGroup::Wall => wall += force.force_n,
                ForceGroup::Brace => brace += force.force_n,
                ForceGroup::Frame => frame += force.force_n,
            }
        }
        let qu = wall + brace + frame;
        let residual = qu - self.external_n;
        let beta = (qu.abs() > self.tolerance_n).then(|| (wall + brace) / qu);
        let diagnostic = |reason: &str| {
            let beta_text = beta.map_or_else(|| "未定義".into(), |v| v.to_string());
            fail(&format!(
                "{reason}; Qu={qu} N Wall={wall} N Brace={brace} N Frame={frame} N 上層外力={} N 基準外力={} N 上側支持抵抗={} N 残差={residual} N 許容差={} N βu={beta_text} [-]",
                self.external_n, self.reference_n, self.support_n, self.tolerance_n
            ))
        };
        if residual.abs() > self.tolerance_n {
            return Err(diagnostic(
                "層切断面力と上層外力の釣合い残差が許容差を超えています",
            ));
        }
        if qu.abs() <= self.tolerance_n {
            return Err(diagnostic("βuの分母がゼロまたは許容差以下です"));
        }
        let beta = (wall + brace) / qu;
        if !(0.0..=1.0).contains(&beta) {
            return Err(diagnostic("βuが0〜1の範囲外です"));
        }
        let sign = qu.signum();
        if [wall, brace, frame].iter().any(|v| v * sign < 0.0) {
            return Err(diagnostic("負の負担寄与があります"));
        }
        Ok(StoryForceEvaluation {
            layer: self.layer,
            qu_n: qu,
            wall_n: wall,
            brace_n: brace,
            frame_n: frame,
            external_n: self.external_n,
            reference_n: self.reference_n,
            support_n: self.support_n,
            residual_n: residual,
            tolerance_n: self.tolerance_n,
            beta_u: beta,
        })
    }
}

impl PushoverResult {
    /// RC線材・SRC耐震壁のせん断耐力または適用条件で、指定stepを設計に採用できない理由を返す。
    pub fn shear_design_unavailable_at(&self, step: u32) -> Option<String> {
        if let Some(reason) = self.src_wall_design_unavailable_at(step) {
            return Some(reason);
        }
        let reached = self
            .shear_yields
            .iter()
            .filter_map(|event| {
                let detail = event.detail.filter(|detail| detail.is_rc)?;
                let reached_n = detail.shear_n[0].hypot(detail.shear_n[1]);
                let exact = detail.exactly_at_limit
                    && detail.limit_n.is_finite()
                    && detail.limit_n > 0.0
                    && reached_n.is_finite()
                    && (reached_n - detail.limit_n).abs()
                        <= crate::common::newton::STATIC_NEWTON.tol * detail.limit_n;
                (event.step < step || event.step == step && !exact).then_some((event, detail))
            })
            .min_by_key(|(event, _)| event.step);
        let unconfirmed = self
            .shear_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_rc && diagnostic.step <= step)
            .min_by_key(|diagnostic| (diagnostic.step, diagnostic.at_i.is_none()));
        if let Some(diagnostic) =
            unconfirmed.filter(|d| reached.is_none_or(|(e, _)| d.step <= e.step))
        {
            return Some(format!(
                "RC線材せん断適用範囲外: 部材ID {} {} step {}（{}）。このstep以降は設計に採用できません",
                diagnostic.elem.0,
                diagnostic.location(),
                diagnostic.step,
                diagnostic.reason,
            ));
        }
        reached.map(|(event, detail)| {
            format!(
                "RC線材の終局せん断耐力Quに到達: 部材ID {} {}端 step {}。指定step {}は参考結果です",
                event.elem.0,
                if detail.at_i { "i" } else { "j" },
                event.step,
                step,
            )
        })
    }

    fn src_wall_design_unavailable_at(&self, step: u32) -> Option<String> {
        let run = self.wall_run.as_ref()?;
        if run.src_wall_elements.is_empty() {
            return None;
        }
        let history = self.confirmed_history.as_deref().unwrap_or_default();
        let direction = history.get(step as usize).map(|record| record.direction);
        for i in 0..=step {
            let mut reached = None;
            let record = history.get(i as usize).filter(|record| {
                record.step == i
                    && record.run_id == run.run_id
                    && record.sign == run.sign
                    && Some(record.direction) == direction
                    && record.input_generation == run.input_generation
            });
            for &elem in &run.src_wall_elements {
                let review = record.and_then(|record| {
                    let mut reviews = record.src_walls.iter().filter(|review| review.elem == elem);
                    reviews.next().filter(|_| reviews.next().is_none())
                });
                match review.map_or_else(
                    || Err("同じ確定stepの38条せん断照査がありません".into()),
                    SrcWallShearReview::reached_limit,
                ) {
                    Err(reason) => {
                        return Some(format!(
                            "SRC耐震壁 ID {} step {i}（{reason}）。このstep以降は参考応答です",
                            elem.0,
                        ))
                    }
                    Ok(true) if i < step => {
                        reached.get_or_insert(elem);
                    }
                    Ok(_) => {}
                }
            }
            if let Some(elem) = reached {
                return Some(format!(
                    "SRC耐震壁 ID {} step {i}（38条の終局せん断耐力に到達しました）。このstepより後は参考応答です",
                    elem.0,
                ));
            }
        }
        None
    }

    /// 明示した採用点を作る。終了理由から法的な評価点を自動推定しない。
    pub fn evaluation_point(
        &self,
        purpose: EvaluationPurpose,
        direction: SeismicDir,
        step: u32,
        reason: String,
    ) -> Result<EvaluationPoint, String> {
        let run = self
            .wall_run
            .as_ref()
            .ok_or("解析runの入力識別情報がありません")?;
        let input = run
            .input_generation
            .as_ref()
            .ok_or("解析runの入力識別情報がありません")?;
        let point = EvaluationPoint {
            purpose,
            run_id: run.run_id.clone(),
            input_generation: input.clone(),
            direction,
            step,
            selection_reason: reason,
            member_capacities_n: vec![],
        };
        self.confirmed_response(&point, purpose)?;
        Ok(point)
    }
    pub fn confirmed_response(
        &self,
        point: &EvaluationPoint,
        purpose: EvaluationPurpose,
    ) -> Result<&ConfirmedStepResponse, String> {
        let run = self
            .wall_run
            .as_ref()
            .ok_or("confirmed_run_missing: member=unresolved 解析runの入力識別情報がありません")?;
        let point_mismatch = if point.purpose != purpose {
            Some("confirmed_purpose_mismatch")
        } else if point.selection_reason.trim().is_empty() {
            Some("confirmed_selection_reason_missing")
        } else if point.run_id != run.run_id {
            Some("confirmed_run_mismatch")
        } else if Some(&point.input_generation) != run.input_generation.as_ref() {
            Some("confirmed_generation_mismatch")
        } else {
            None
        };
        if let Some(cause) = point_mismatch {
            return Err(format!(
                "{cause}: member=unresolved 評価目的・run・入力世代・選定理由が不整合です"
            ));
        }
        let mut records = self
            .confirmed_history
            .as_ref()
            .ok_or("confirmed_history_missing: member=unresolved 線材・層切断面の確定履歴がありません。再解析してください")?
            .iter()
            .filter(|r| r.step == point.step);
        let record = records
            .next()
            .ok_or("confirmed_step_missing: member=unresolved 採用stepの確定履歴がありません")?;
        let record_mismatch = if records.next().is_some() {
            Some("confirmed_step_duplicate")
        } else if record.run_id != point.run_id {
            Some("confirmed_run_mismatch")
        } else if record.input_generation.as_ref() != Some(&point.input_generation) {
            Some("confirmed_generation_mismatch")
        } else if record.direction != point.direction || record.sign != run.sign {
            Some("confirmed_direction_mismatch")
        } else if self
            .capacity_curve
            .iter()
            .filter(|p| p.step == point.step)
            .count()
            != 1
            || point.step as usize >= self.steps.len()
        {
            Some("confirmed_step_mismatch")
        } else {
            None
        };
        if let Some(cause) = record_mismatch {
            return Err(format!(
                "{cause}: member=unresolved 確定履歴のrun・入力世代・方向・stepが不整合です"
            ));
        }
        if let Some(reason) = self.shear_design_unavailable_at(point.step) {
            return Err(reason);
        }
        let mut seen = std::collections::HashSet::new();
        if let Some(member) = record.members.iter().find(|r| {
            [
                r.m_strong,
                r.m_weak,
                r.shear_strong,
                r.shear_weak,
                r.axial,
                r.rp,
                r.horizontal_force,
            ]
            .iter()
            .any(|v| !v.is_finite())
        }) {
            return Err(format!(
                "confirmed_member_nonfinite: member={} 確定部材応答が非有限です",
                member.elem.0
            ));
        }
        if let Some(member) = record.members.iter().find(|r| !seen.insert(r.elem)) {
            return Err(format!(
                "confirmed_member_duplicate: member={} 確定部材応答が重複しています",
                member.elem.0
            ));
        }
        Ok(record)
    }
    pub fn mechanism_at(
        &self,
        point: &EvaluationPoint,
        model: &Model,
    ) -> Result<super::MechanismType, String> {
        self.confirmed_response(point, EvaluationPurpose::Ds)?;
        let hinges: Vec<_> = self
            .hinges
            .iter()
            .filter(|h| h.step <= point.step)
            .cloned()
            .collect();
        Ok(super::mechanism::determine_mechanism(
            &hinges,
            model,
            point.direction,
        ))
    }
    pub fn evaluate_stories(
        &self,
        point: &EvaluationPoint,
        purpose: EvaluationPurpose,
    ) -> Result<Vec<StoryForceEvaluation>, String> {
        let record = self.confirmed_response(point, purpose)?;
        if record.cuts.is_empty() {
            return Err("層切断面の確定履歴がありません".into());
        }
        record
            .cuts
            .iter()
            .enumerate()
            .map(|(i, cut)| {
                if cut.layer != i {
                    return Err("層切断面の順序・層識別が不整合です".into());
                }
                cut.evaluate().map_err(|reason| {
                    format!(
                        "purpose={:?} run={} direction={:?} step={} reason={}: {reason}",
                        point.purpose,
                        point.run_id,
                        point.direction,
                        point.step,
                        point.selection_reason
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
pub(super) fn record_cuts(
    model: &Model,
    dofmap: &sepika_core::dof::DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    dir: SeismicDir,
) -> Vec<StoryCut> {
    let responses = super::geom::member_recording_responses(model, behaviors).unwrap();
    let empty = sepika_core::model::LoadCase {
        origin: Default::default(),
        id: sepika_core::ids::LoadCaseId(0),
        name: String::new(),
        nodal: vec![],
        member: vec![],
        kind: Default::default(),
    };
    let physical = super::physical_response::PhysicalStep::recover(
        model,
        dofmap,
        super::physical_response::fixed_dofs(model, dofmap),
        behaviors,
        &responses,
        &vec![0.; dofmap.n_active()],
        empty.clone(),
        empty,
    )
    .unwrap();
    record_cuts_with_responses(model, dofmap, behaviors, dir, &responses, &physical)
}

pub(super) fn record_cuts_with_responses(
    model: &Model,
    dofmap: &sepika_core::dof::DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    dir: SeismicDir,
    responses: &[Option<sepika_element::behavior::LoadedFrameResponse>],
    physical: &super::physical_response::PhysicalStep,
) -> Vec<StoryCut> {
    let d = usize::from(dir == SeismicDir::Y);
    let ctx = Ctx { model };
    let index = sepika_core::model::WallSideColumnIndex::build(model);
    model
        .layers()
        .iter()
        .map(|layer| {
            let z = (layer.bottom_sl_mm + layer.top_sl_mm) * 0.5;
            let mut cut = StoryCut {
                layer: layer.index,
                elevation_mm: z,
                forces: vec![],
                external_n: f64::NAN,
                reference_n: f64::NAN,
                support_n: 0.0,
                tolerance_n: f64::NAN,
                unavailable: None,
            };
            for (i, elem) in model.elements.iter().enumerate() {
                // パネルの柱節点参照は軸力追従用であり、独立した層水平力を伝達しない。
                if matches!(elem.kind, ElementKind::PanelZone) {
                    continue;
                }
                let nodes: Option<Vec<_>> = elem
                    .nodes
                    .iter()
                    .map(|n| model.nodes.get(n.index()))
                    .collect();
                let Some(nodes) = nodes else {
                    cut.unavailable = Some("切断要素の節点参照が欠落しています".into());
                    continue;
                };
                let actual_ends = match model.primary_frame_axis_ends(elem) {
                    Ok(ends) => ends,
                    Err(reason) => {
                        cut.unavailable = Some(reason);
                        continue;
                    }
                };
                let crosses = if let Some(ends) = actual_ends {
                    let heights = [nodes[0].coord[2], ends[0][2], ends[1][2], nodes[1].coord[2]];
                    heights.iter().any(|height| *height > z)
                        && heights.iter().any(|height| *height <= z)
                } else {
                    nodes.iter().any(|n| n.coord[2] > z) && nodes.iter().any(|n| n.coord[2] < z)
                };
                if !crosses {
                    continue;
                }
                let Some(b) = behaviors.get(i) else {
                    cut.unavailable = Some("切断要素の確定内力が欠落しています".into());
                    continue;
                };
                let f = b.internal_force(&ctx);
                let global_dofs = b.global_dofs(dofmap);
                if f.data.len() != global_dofs.len() || f.data.iter().any(|v| !v.is_finite()) {
                    cut.unavailable = Some(format!(
                        "切断要素 {:?}の確定内力が不正です（{}成分、{}節点）",
                        elem.id,
                        f.data.len(),
                        nodes.len()
                    ));
                    continue;
                }
                let side_column = match index.replacement_bindings(elem.id) {
                    Ok(bindings) => bindings.iter().any(|binding| {
                        model
                            .element(binding.wall)
                            .and_then(|wall| sepika_core::model::wall_element_geometry(wall, model))
                            .is_some_and(|g| g.ex_bottom[d].abs() > 1e-6)
                    }),
                    Err(issues) => {
                        cut.unavailable = Some(
                            issues
                                .iter()
                                .map(|issue| {
                                    sepika_element::factory::wall_side_column_issue_message(
                                        issue,
                                        Some(sepika_core::model::AnalysisKind::Incremental),
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("; "),
                        );
                        continue;
                    }
                };
                let group = if (matches!(elem.kind, ElementKind::Wall)
                    && sepika_core::model::wall_element_geometry(elem, model)
                        .is_some_and(|g| g.ex_bottom[d].abs() > 1e-6))
                    || side_column
                {
                    ForceGroup::Wall
                } else if matches!(elem.kind, ElementKind::Brace { .. }) {
                    ForceGroup::Brace
                } else {
                    ForceGroup::Frame
                };
                let response = responses[i].as_ref();
                let force_n = if let Some(response) = response.filter(|_| actual_ends.is_none()) {
                    let pi = nodes[0].coord;
                    let pj = nodes[1].coord;
                    let xi = (z - pi[2]) / (pj[2] - pi[2]);
                    let section = match response.section.at(xi) {
                        Ok(section) => section,
                        Err(reason) => {
                            cut.unavailable = Some(reason);
                            continue;
                        }
                    };
                    let frame = sepika_element::transform::LocalFrame::from_nodes(
                        pi,
                        pj,
                        elem.local_axis.ref_vector,
                    )
                    .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", elem.id.0));
                    (pj[2] - pi[2]).signum()
                        * (section[0] * frame.rot[0][d]
                            - section[1] * frame.rot[1][d]
                            - section[2] * frame.rot[2][d])
                } else {
                    // 偏心接続腕だけを横切る断面もあるため、単一の材軸交点の断面力では合力を回復できない。
                    let end_force = physical.element_nodal[i]
                        .iter()
                        .filter(|(node, _)| model.nodes[*node].coord[2] > z)
                        .map(|(_, force)| force[d])
                        .sum::<f64>();
                    let source = sepika_core::model::LoadCase {
                        origin: Default::default(),
                        id: physical.source.id,
                        name: String::new(),
                        nodal: vec![],
                        member: physical
                            .source
                            .member
                            .iter()
                            .filter(|load| load.elem == elem.id)
                            .cloned()
                            .collect(),
                        kind: Default::default(),
                    };
                    match super::physical_response::load_above(model, &source, z, d) {
                        Ok(applied) => end_force + applied,
                        Err(reason) => {
                            cut.unavailable = Some(reason);
                            continue;
                        }
                    }
                };
                cut.forces.push(CutForce {
                    elem: elem.id,
                    group,
                    force_n,
                });
            }
            cut
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{CapacityPoint, PushoverStep};
    use super::*;
    fn cut(layer: usize, qu: f64, wall: f64) -> StoryCut {
        StoryCut {
            layer,
            elevation_mm: 1500.0 + 3000.0 * layer as f64,
            forces: vec![
                CutForce {
                    elem: ElemId(0),
                    group: ForceGroup::Wall,
                    force_n: wall,
                },
                CutForce {
                    elem: ElemId(1),
                    group: ForceGroup::Frame,
                    force_n: qu - wall,
                },
            ],
            external_n: qu,
            reference_n: qu,
            support_n: 0.0,
            tolerance_n: 1e-6,
            unavailable: None,
        }
    }
    fn result(run_id: &str, dir: SeismicDir, cuts: Vec<Vec<StoryCut>>) -> PushoverResult {
        let run = super::super::wall_response::WallRunIdentity {
            gravity_terms: Vec::new(),
            gravity_applied: false,
            sign: PushoverSign::Positive,
            src_wall_elements: vec![],
            run_id: run_id.into(),
            input_generation: Some(vec![42]),
            input_unavailable: None,
        };
        let mut po = PushoverResult {
            target: None,
            wall_capacities: Vec::new(),
            rc_wall_shear_method: sepika_core::model::RcWallShearMethod::default(),
            steps: vec![],
            wall_history: None,
            wall_run: Some(run),
            confirmed_history: Some(vec![]),
            ds_evaluation: None,
            capacity_evaluation: None,
            capacity_curve: vec![],
            hinges: vec![],
            shear_yields: vec![],
            shear_diagnostics: vec![],
            mechanism: super::super::MechanismType::Overall,
            qu: 0.0,
            member_response: vec![],
            control: Default::default(),
            member_history: vec![],
            fiber_states: vec![],
            termination: Default::default(),
        };
        for (step, cuts) in cuts.into_iter().enumerate() {
            let shear: Vec<_> = cuts.iter().map(|c| c.external_n).collect();
            po.capacity_curve.push(CapacityPoint {
                step: step as u32,
                roof_disp: step as f64,
                base_shear: shear[0],
                story_shear: shear,
                story_drift: vec![1.0; cuts.len()],
            });
            po.steps.push(PushoverStep {
                floor_disp: Vec::new(),
                load_factor: 1.0,
                top_disp: step as f64,
                base_shear: 0.0,
                story_drifts: vec![1.0; cuts.len()],
            });
            po.confirmed_history
                .as_mut()
                .unwrap()
                .push(ConfirmedStepResponse {
                    run_id: run_id.into(),
                    input_generation: Some(vec![42]),
                    direction: dir,
                    sign: PushoverSign::Positive,
                    step: step as u32,
                    members: vec![],
                    cuts,
                    src_walls: vec![],
                    src_frame_faces: vec![],
                });
        }
        po
    }
    fn point(
        po: &PushoverResult,
        purpose: EvaluationPurpose,
        dir: SeismicDir,
        step: u32,
    ) -> EvaluationPoint {
        po.evaluation_point(purpose, dir, step, "独立指定".into())
            .unwrap()
    }

    #[test]
    fn src_wall_shear_at_limit_is_adoptable_only_at_the_first_arrival_step() {
        use sepika_core::src_wall_capacity::SrcWallEndForces;
        let wall = ElemId(14);
        let limit_n = 1000.0;
        let tolerance_n = crate::common::newton::STATIC_NEWTON.tol * limit_n;
        for (difference_n, at_allowed, next_allowed, reason) in [
            (-2.0 * tolerance_n, true, true, ""),
            (-0.5 * tolerance_n, true, false, "到達"),
            (0.0, true, false, "到達"),
            (0.5 * tolerance_n, true, false, "到達"),
            (2.0 * tolerance_n, false, false, "超過"),
        ] {
            let mut po = result("SRC", SeismicDir::X, vec![vec![cut(0, 150.0, 90.0)]; 3]);
            po.wall_run.as_mut().unwrap().src_wall_elements = vec![wall];
            for (record, demand_n) in po.confirmed_history.as_mut().unwrap().iter_mut().zip([
                0.0,
                limit_n + difference_n,
                100.0,
            ]) {
                record.src_walls = vec![SrcWallShearReview {
                    elem: wall,
                    ends: Some(
                        [SrcWallEndForces {
                            shear_n: -demand_n,
                            ..Default::default()
                        }; 2],
                    ),
                    demand_n: Some(demand_n),
                    limit_n: (demand_n > 0.0).then_some(limit_n),
                    unavailable: None,
                }];
            }
            for purpose in [EvaluationPurpose::Ds, EvaluationPurpose::HoldingCapacity] {
                assert!(po
                    .evaluation_point(purpose, SeismicDir::X, 0, "照査".into())
                    .is_ok());
                for (step, allowed) in [(1, at_allowed), (2, next_allowed)] {
                    let selected = po.evaluation_point(purpose, SeismicDir::X, step, "照査".into());
                    if allowed {
                        assert!(selected.is_ok(), "{selected:?}");
                    } else {
                        let message = selected.unwrap_err();
                        assert!(
                            message.contains("ID 14 step 1") && message.contains(reason),
                            "{message}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn rc_shear_reference_steps_cannot_be_adopted_for_design() {
        use super::super::{MemberShearYield, ShearYieldEvent};

        let mut po = result("A", SeismicDir::X, vec![vec![cut(0, 150.0, 90.0)]; 3]);
        let purposes = [EvaluationPurpose::HoldingCapacity, EvaluationPurpose::Ds];
        let points =
            purposes.map(|purpose| [0, 1, 2].map(|step| point(&po, purpose, SeismicDir::X, step)));
        for (is_rc, exactly_at_limit, shear_n, expected) in [
            (true, true, 120.0, [true, true, false]),
            (true, false, 120.1, [true, false, false]),
            (true, true, 120.1, [true, false, false]),
            (false, false, 120.1, [true, true, true]),
        ] {
            po.shear_yields = vec![ShearYieldEvent {
                step: 1,
                elem: ElemId(7),
                detail: Some(MemberShearYield {
                    at_i: false,
                    is_rc,
                    shear_n: [shear_n, 0.0],
                    limit_n: 120.0,
                    governing_axis: Some(0),
                    exactly_at_limit,
                }),
            }];
            for (purpose, points) in purposes.iter().zip(&points) {
                for (point, accepted) in points.iter().zip(expected) {
                    let evaluation = po.evaluate_stories(point, *purpose);
                    assert_eq!(evaluation.is_ok(), accepted);
                    if let Err(reason) = evaluation {
                        assert!(reason.contains("部材ID 7 j端 step 1"));
                    }
                }
            }
            assert_eq!(po.capacity_curve.len(), 3);
        }
    }

    #[test]
    fn rc_shear_applicability_diagnostic_preserves_preceding_design_step() {
        use super::super::MemberShearDiagnostic;

        let mut po = result("A", SeismicDir::X, vec![vec![cut(0, 150.0, 90.0)]; 3]);
        po.shear_diagnostics.push(MemberShearDiagnostic {
            step: 1,
            elem: ElemId(4),
            at_i: Some(true),
            is_rc: true,
            reason: "柱の軸力が引張域です".into(),
        });
        let purpose = EvaluationPurpose::HoldingCapacity;
        assert_eq!(point(&po, purpose, SeismicDir::X, 0).step, 0);
        for step in [1, 2] {
            let error = po
                .evaluation_point(purpose, SeismicDir::X, step, "利用者指定".into())
                .unwrap_err();
            assert!(error.contains("せん断適用範囲外"));
            assert!(error.contains("部材ID 4 i端 step 1"));
            assert!(error.contains("引張域"));
        }
    }

    #[test]
    fn 二層外力100と50壁90と30は同時点で両層06() {
        let po = result(
            "A",
            SeismicDir::X,
            vec![vec![
                cut(0, 150_000.0, 90_000.0),
                cut(1, 50_000.0, 30_000.0),
            ]],
        );
        let values = po
            .evaluate_stories(
                &point(&po, EvaluationPurpose::Ds, SeismicDir::X, 0),
                EvaluationPurpose::Ds,
            )
            .unwrap();
        assert_eq!(
            values.iter().map(|v| v.qu_n).collect::<Vec<_>>(),
            [150_000.0, 50_000.0]
        );
        assert_eq!(
            values.iter().map(|v| v.beta_u).collect::<Vec<_>>(),
            [0.6, 0.6]
        );
    }
    #[test]
    fn 層ピークが異なっても明示stepの150対90と120対84を混合しない() {
        let po = result(
            "A",
            SeismicDir::X,
            vec![
                vec![cut(0, 150.0, 90.0), cut(1, 50.0, 30.0)],
                vec![cut(0, 120.0, 84.0), cut(1, 80.0, 56.0)],
            ],
        );
        let a = po
            .evaluate_stories(
                &point(&po, EvaluationPurpose::Ds, SeismicDir::X, 0),
                EvaluationPurpose::Ds,
            )
            .unwrap();
        let b = po
            .evaluate_stories(
                &point(&po, EvaluationPurpose::HoldingCapacity, SeismicDir::X, 1),
                EvaluationPurpose::HoldingCapacity,
            )
            .unwrap();
        assert_eq!([a[0].beta_u, b[0].beta_u], [0.6, 0.7]);
        assert_eq!([a[1].qu_n, b[1].qu_n], [50.0, 80.0]);
        assert_ne!(b[0].beta_u, 0.56);
    }
    #[test]
    fn 同stepでも異目的run世代方向の混合は拒否する() {
        let po = result("A", SeismicDir::X, vec![vec![cut(0, 150.0, 90.0)]]);
        let other = result("B", SeismicDir::X, vec![vec![cut(0, 120.0, 84.0)]]);
        let p = point(&po, EvaluationPurpose::Ds, SeismicDir::X, 0);
        assert_eq!(
            other
                .evaluate_stories(
                    &point(&other, EvaluationPurpose::Ds, SeismicDir::X, 0),
                    EvaluationPurpose::Ds
                )
                .unwrap()[0]
                .beta_u,
            0.7
        );
        assert!(other.evaluate_stories(&p, EvaluationPurpose::Ds).is_err());
        for kind in 0..6 {
            let mut invalid = p.clone();
            match kind {
                0 => invalid.purpose = EvaluationPurpose::HoldingCapacity,
                1 => invalid.run_id = "B".into(),
                2 => invalid.input_generation = vec![43],
                3 => invalid.direction = SeismicDir::Y,
                4 => invalid.step = 1,
                _ => invalid.selection_reason.clear(),
            }
            let error = po
                .evaluate_stories(&invalid, EvaluationPurpose::Ds)
                .unwrap_err();
            let cause = [
                "confirmed_purpose_mismatch:",
                "confirmed_run_mismatch:",
                "confirmed_generation_mismatch:",
                "confirmed_direction_mismatch:",
                "confirmed_step_missing:",
                "confirmed_selection_reason_missing:",
            ][kind];
            assert!(error.starts_with(cause), "case {kind}: {error}");
            assert!(error.contains("member=unresolved"), "{error}");
        }
        for kind in 0..5 {
            let mut invalid = po.clone();
            let history = invalid.confirmed_history.as_mut().unwrap();
            match kind {
                0 => history[0].run_id = "B".into(),
                1 => history[0].input_generation = Some(vec![43]),
                2 => history[0].direction = SeismicDir::Y,
                3 => history.push(history[0].clone()),
                _ => history.clear(),
            }
            let error = invalid
                .evaluate_stories(&p, EvaluationPurpose::Ds)
                .unwrap_err();
            let cause = [
                "confirmed_run_mismatch:",
                "confirmed_generation_mismatch:",
                "confirmed_direction_mismatch:",
                "confirmed_step_duplicate:",
                "confirmed_step_missing:",
            ][kind];
            assert!(error.starts_with(cause), "case {kind}: {error}");
        }
        let mut invalid = po.clone();
        invalid.confirmed_history = None;
        assert!(invalid
            .evaluate_stories(&p, EvaluationPurpose::Ds)
            .unwrap_err()
            .contains("確定履歴がありません"));
    }
    #[test]
    fn 非有限と重複の確定応答は該当する部材を識別する() {
        let mut po = result("A", SeismicDir::X, vec![vec![cut(0, 150.0, 90.0)]]);
        let p = point(&po, EvaluationPurpose::Ds, SeismicDir::X, 0);
        let member = |id| PushoverMemberResponse {
            elem: ElemId(id),
            axial: 0.0,
            m_strong: 0.0,
            m_weak: 0.0,
            shear_strong: 0.0,
            shear_weak: 0.0,
            rp: 0.0,
            horizontal_force: 0.0,
            wall_shear_signed: None,
            rc_ultimate_shear: None,
            rc_beam_ultimate_shear: None,
        };
        let mut invalid = member(17);
        invalid.axial = f64::NAN;
        po.confirmed_history.as_mut().unwrap()[0].members = vec![member(3), invalid];
        let error = po
            .confirmed_response(&p, EvaluationPurpose::Ds)
            .unwrap_err();
        assert!(
            error.starts_with("confirmed_member_nonfinite: member=17 "),
            "{error}"
        );
        assert!(error.contains("確定部材応答が非有限です"));
        po.confirmed_history.as_mut().unwrap()[0].members = vec![member(3), member(17), member(17)];
        let error = po
            .confirmed_response(&p, EvaluationPurpose::Ds)
            .unwrap_err();
        assert!(
            error.starts_with("confirmed_member_duplicate: member=17 "),
            "{error}"
        );
    }
    #[test]
    fn 方向反転xyは符号を保持し比率を変えない() {
        for dir in [SeismicDir::X, SeismicDir::Y] {
            for sign in [1.0, -1.0] {
                let po = result("A", dir, vec![vec![cut(0, sign * 150.0, sign * 90.0)]]);
                let v = po
                    .evaluate_stories(
                        &point(&po, EvaluationPurpose::Ds, dir, 0),
                        EvaluationPurpose::Ds,
                    )
                    .unwrap();
                assert_eq!(v[0].qu_n, sign * 150.0);
                assert_eq!(v[0].wall_n, sign * 90.0);
                assert_eq!(v[0].beta_u, 0.6);
            }
        }
    }
    #[test]
    fn ゼロ負寄与範囲外重複不釣合い欠損は補正せず拒否する() {
        for (qu, wall, reason) in [
            (0.0, 0.0, "分母"),
            (150.0, -10.0, "βu"),
            (150.0, 160.0, "βu"),
        ] {
            assert!(cut(0, qu, wall).evaluate().unwrap_err().contains(reason));
        }
        let mut c = cut(0, 150.0, 90.0);
        c.forces.push(c.forces[0].clone());
        assert!(c.evaluate().unwrap_err().contains("重複"));
        let mut c = cut(0, 150.0, 90.0);
        c.external_n = 120.0;
        assert!(c.evaluate().unwrap_err().contains("釣合い"));
        let mut c = cut(0, 150.0, 90.0);
        c.forces[0].force_n = f64::NAN;
        assert!(c.evaluate().is_err());
        let mut c = cut(0, 150.0, 90.0);
        c.unavailable = Some("欠損".into());
        assert!(c.evaluate().unwrap_err().contains("欠損"));
    }
    struct Prescribed {
        nodes: Vec<sepika_core::ids::NodeId>,
        forces: Vec<f64>,
    }
    impl ElementBehavior for Prescribed {
        fn n_dof(&self) -> usize {
            self.forces.len()
        }
        fn global_dofs(&self, map: &sepika_core::dof::DofMap) -> smallvec::SmallVec<[usize; 24]> {
            sepika_element::behavior::node_global_dofs(&self.nodes, map)
        }
        fn tangent_stiffness(&self, _: &Ctx) -> sepika_element::behavior::LocalMat {
            sepika_element::behavior::LocalMat::zeros(self.forces.len())
        }
        fn internal_force(&self, _: &Ctx) -> sepika_element::behavior::LocalVec {
            sepika_element::behavior::LocalVec {
                data: self.forces.iter().copied().collect(),
            }
        }
        fn mass_matrix(
            &self,
            _: sepika_element::behavior::MassOption,
        ) -> sepika_element::behavior::LocalMat {
            sepika_element::behavior::LocalMat::zeros(self.forces.len())
        }
    }
    #[test]
    fn 確定記録は独立外力100と50を上層累積し支持ばねを外部作用として含める() {
        for brace in [false, true] {
            for offsets in [
                [[0.0; 3]; 2],
                [[100.0, 0.0, 2000.0], [-100.0, 0.0, -2000.0]],
            ] {
                for support_n in [0.0, 20.0] {
                    let mut m = super::super::tests::two_story_model();
                    if offsets != [[0.0; 3]; 2] {
                        m.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Column);
                    }
                    if brace {
                        m.sections[0].frame_use = Some(sepika_core::model::FrameSectionUse::Brace);
                    }
                    for element in &mut m.elements {
                        if brace {
                            element.kind = ElementKind::Brace {
                                tension_only: false,
                            };
                        }
                        element.end_offsets_mm = offsets;
                    }
                    m.nodes[1].support_spring = Some([10.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
                    let map = sepika_core::dof::DofMap::build(&m);
                    let mut disp = vec![0.0; map.n_active()];
                    disp[map.active(6).unwrap() as usize] = support_n / 10.0;
                    let behaviors: Vec<Box<dyn ElementBehavior>> = m
                        .elements
                        .iter()
                        .zip([150_000.0 - support_n, 50_000.0])
                        .map(|(e, q)| {
                            let mut forces = vec![0.0; 12];
                            forces[0] = -q;
                            forces[6] = q;
                            Box::new(Prescribed {
                                nodes: e.nodes.to_vec(),
                                forces,
                            }) as Box<dyn ElementBehavior>
                        })
                        .collect();
                    let heights = [3000.0, 3000.0];
                    let mut recorder = super::super::step_record::StepRecorder::new(
                        &m,
                        &map,
                        SeismicDir::X,
                        &heights,
                        Default::default(),
                        crate::nonlinear::pushover::shear_yield::build_shear_yield_thresholds(&m),
                        super::super::loads::PhysicalPushoverLoads::empty(),
                    );
                    recorder.reference_external = vec![0.0; map.n_active()];
                    recorder.reference_external[map.active(6).unwrap() as usize] = 100_000.0;
                    recorder.reference_external[map.active(12).unwrap() as usize] = 50_000.0;
                    recorder.constant_external = vec![0.0; map.n_active()];
                    recorder.physical_loads.reference.nodal =
                        [(m.nodes[1].id, 100_000.), (m.nodes[2].id, 50_000.)]
                            .into_iter()
                            .map(|(node, force)| {
                                sepika_core::model::NodalLoad::manual(
                                    node,
                                    [force, 0., 0., 0., 0., 0.],
                                )
                            })
                            .collect();
                    recorder.record(1.0, &m, &map, &behaviors, &disp).unwrap();
                    let recorded = recorder.finish();
                    let cuts = &recorded.confirmed_history[0].cuts;
                    assert_eq!(cuts[0].reference_n, 150_000.0);
                    assert_eq!(cuts[1].reference_n, 50_000.0);
                    assert_eq!(cuts[0].support_n, support_n);
                    assert_eq!(cuts[0].evaluate().unwrap().qu_n, 150_000.0 - support_n);
                    assert_eq!(cuts[1].evaluate().unwrap().qu_n, 50_000.0);
                    assert_eq!(cuts[0].evaluate().unwrap().residual_n, 0.0);
                }
            }
        }
    }
    #[test]
    fn 開口を持つ隣接壁の共有側柱を壁系へ一度だけ計上する() {
        use sepika_core::ids::NodeId;
        let mut m = super::super::tests::wall_story_model_with(4000.0, 100_000.0);
        m.sections[1].frame_use = Some(sepika_core::model::FrameSectionUse::Column);
        m.elements.retain(|e| [0, 3, 4].contains(&e.id.0));
        for n in &mut m.nodes {
            n.restraint = sepika_core::dof::Dof6Mask::FREE;
        }
        for (source, id, x) in [(1, 4, 8000.0), (2, 5, 8000.0), (0, 6, 0.0), (3, 7, 0.0)] {
            let mut n = m.nodes[source].clone();
            n.id = NodeId(id);
            n.coord[0] = x;
            m.nodes.push(n);
        }
        let mut wall = m.elements[0].clone();
        wall.id = ElemId(5);
        wall.nodes = smallvec::smallvec![NodeId(1), NodeId(4), NodeId(5), NodeId(2)];
        m.elements.push(wall);
        let mut column = m.elements[1].clone();
        column.id = ElemId(6);
        column.nodes = smallvec::smallvec![NodeId(4), NodeId(5)];
        m.elements.push(column);
        let mut column = m.elements[1].clone();
        column.id = ElemId(7);
        column.nodes = smallvec::smallvec![NodeId(6), NodeId(7)];
        m.elements.push(column);
        let mut girder_section = m.sections[1].clone();
        girder_section.id = sepika_core::ids::SectionId(2);
        girder_section.shape = None;
        girder_section.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        m.sections.push(girder_section);
        for (id, pair) in [(8, [0, 1]), (9, [3, 2]), (10, [1, 4]), (11, [2, 5])] {
            let mut girder = m.elements[1].clone();
            girder.id = ElemId(id);
            girder.section = Some(sepika_core::ids::SectionId(2));
            girder.nodes = pair.map(NodeId).into_iter().collect();
            m.elements.push(girder);
        }
        // 開口低減後の実内力を指定する。低減係数を切断力へ再適用しない。
        m.wall_attrs.push(sepika_core::model::WallAttr {
            elem: ElemId(5),
            opening_area: 0.0,
            opening_weight: 0.0,
            slit: Default::default(),
            openings: vec![sepika_core::model::WallOpening {
                width: 1000.0,
                height: 1000.0,
                offset: Some([1500.0, 1000.0]),
            }],
            finish_intensity: 0.0,
        });
        let totals = [40.0, 5.0, 10.0, 30.0, 5.0, 60.0, 0.0, 0.0, 0.0, 0.0];
        for kind in [ElementKind::Beam, ElementKind::Fiber] {
            let mut m = m.clone();
            for frame in m
                .elements
                .iter_mut()
                .filter(|e| e.kind.is_primary_frame_support())
            {
                frame.kind = kind;
            }
            for dir in [SeismicDir::X, SeismicDir::Y] {
                let mut m = m.clone();
                if dir == SeismicDir::Y {
                    for n in &mut m.nodes {
                        n.coord.swap(0, 1);
                    }
                }
                for sign in [1.0, -1.0] {
                    let d = usize::from(dir == SeismicDir::Y);
                    let behaviors: Vec<Box<dyn ElementBehavior>> = m
                        .elements
                        .iter()
                        .zip(totals)
                        .map(|(e, q)| {
                            let mut forces = vec![0.0; e.nodes.len() * 6];
                            let upper = e
                                .nodes
                                .iter()
                                .filter(|n| m.nodes[n.index()].coord[2] > 1500.0)
                                .count();
                            for (k, n) in e.nodes.iter().enumerate() {
                                forces[k * 6 + d] = sign * q / upper.max(1) as f64
                                    * if m.nodes[n.index()].coord[2] > 1500.0 {
                                        1.0
                                    } else {
                                        -1.0
                                    };
                            }
                            Box::new(Prescribed {
                                nodes: e.nodes.to_vec(),
                                forces,
                            }) as Box<dyn ElementBehavior>
                        })
                        .collect();
                    let mut c =
                        record_cuts(&m, &sepika_core::dof::DofMap::build(&m), &behaviors, dir)
                            .remove(0);
                    c.external_n = sign * 150.0;
                    c.reference_n = sign * 150.0;
                    c.tolerance_n = 1e-6;
                    let v = c.evaluate().unwrap();
                    assert_eq!(v.wall_n, sign * 90.0);
                    assert_eq!(v.frame_n, sign * 60.0);
                    assert_eq!(v.beta_u, 0.6);
                    assert_eq!(c.forces.iter().filter(|f| f.elem == ElemId(4)).count(), 1);
                    let mut invalid = m.clone();
                    invalid.sections[1].frame_use = None;
                    let cut = record_cuts(
                        &invalid,
                        &sepika_core::dof::DofMap::build(&invalid),
                        &behaviors,
                        dir,
                    )
                    .remove(0);
                    let reason = cut.unavailable.unwrap();
                    assert!(reason.contains("側柱には柱用断面が必要"), "{reason}");
                    assert!(reason.contains("増分解析"), "{reason}");
                }
            }
        }
    }
}
