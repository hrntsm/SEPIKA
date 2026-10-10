//! 同じ確定状態の層切断面力と目的別評価点。
use super::{PushoverMemberResponse, PushoverResult};
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
    /// 部材群判定用の耐力 [N]。同時点の負担力とは別の設計入力。
    pub member_capacities_n: Vec<(ElemId, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ForceGroup {
    Wall,
    Brace,
    Frame,
}

/// 物理要素を一度だけ計上した符号付き上側材端力 [N]。
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
    pub step: u32,
    pub members: Vec<PushoverMemberResponse>,
    pub cuts: Vec<StoryCut>,
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
        if residual.abs() > self.tolerance_n {
            return Err(fail(
                "層切断面力と上層外力の釣合い残差が許容差を超えています",
            ));
        }
        if qu.abs() <= self.tolerance_n {
            return Err(fail("βuの分母がゼロまたは許容差以下です"));
        }
        let beta = (wall + brace) / qu;
        if !(0.0..=1.0).contains(&beta) {
            return Err(fail("βuが0〜1の範囲外です"));
        }
        let sign = qu.signum();
        if [wall, brace, frame].iter().any(|v| v * sign < 0.0) {
            return Err(fail("負の負担寄与があります"));
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
            .ok_or("解析runの入力識別情報がありません")?;
        if point.purpose != purpose
            || point.selection_reason.trim().is_empty()
            || point.run_id != run.run_id
            || Some(&point.input_generation) != run.input_generation.as_ref()
        {
            return Err("評価目的・run・入力世代・選定理由が不整合です".into());
        }
        let mut records = self
            .confirmed_history
            .as_ref()
            .ok_or("線材・層切断面の確定履歴がありません。再解析してください")?
            .iter()
            .filter(|r| r.step == point.step);
        let record = records.next().ok_or("採用stepの確定履歴がありません")?;
        if records.next().is_some()
            || record.run_id != point.run_id
            || record.input_generation.as_ref() != Some(&point.input_generation)
            || record.direction != point.direction
            || self
                .capacity_curve
                .iter()
                .filter(|p| p.step == point.step)
                .count()
                != 1
            || point.step as usize >= self.steps.len()
        {
            return Err("確定履歴のrun・入力世代・方向・stepが不整合です".into());
        }
        let mut seen = std::collections::HashSet::new();
        if record.members.iter().any(|r| {
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
            return Err("確定部材応答が非有限です".into());
        }
        if record.members.iter().any(|r| !seen.insert(r.elem)) {
            return Err("確定部材応答が重複しています".into());
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
                cut.evaluate()
            })
            .collect()
    }
}

pub(super) fn record_cuts(
    model: &Model,
    dofmap: &sepika_core::dof::DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    dir: SeismicDir,
) -> Vec<StoryCut> {
    let d = usize::from(dir == SeismicDir::Y);
    let ctx = Ctx { model };
    let walls: Vec<_> = model
        .elements
        .iter()
        .filter(|e| matches!(e.kind, ElementKind::Wall))
        .filter_map(|e| sepika_core::model::wall_element_geometry(e, model))
        .filter(|g| g.ex_bottom[d].abs() > 1e-6)
        .collect();
    model
        .layers()
        .iter()
        .map(|layer| {
            let z = (model.stories[layer.bottom.index()].elevation
                + model.stories[layer.top.index()].elevation)
                * 0.5;
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
                if !nodes.iter().any(|n| n.coord[2] > z) || !nodes.iter().any(|n| n.coord[2] < z) {
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
                let side_column = nodes.len() == 2
                    && (nodes[0].coord[0] - nodes[1].coord[0]).abs() < 1e-6
                    && (nodes[0].coord[1] - nodes[1].coord[1]).abs() < 1e-6
                    && walls.iter().any(|w| {
                        let wall_nodes = [w.bottom[0], w.bottom[1], w.top[0], w.top[1]]
                            .map(|n| &model.nodes[n.index()]);
                        [0, 1].into_iter().any(|k| {
                            (nodes[0].coord[0] - wall_nodes[k].coord[0]).abs() < 1e-6
                                && (nodes[0].coord[1] - wall_nodes[k].coord[1]).abs() < 1e-6
                                && z > wall_nodes[k].coord[2]
                                && z < wall_nodes[k + 2].coord[2]
                        })
                    });
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
                let force_n = nodes
                    .iter()
                    .filter(|n| n.coord[2] > z)
                    .map(|n| {
                        global_dofs
                            .iter()
                            .position(|g| {
                                Some(*g) == dofmap.active(n.id.index() * 6 + d).map(|a| a as usize)
                            })
                            .and_then(|k| f.data.get(k))
                            .copied()
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|v| v.iter().sum());
                let Some(force_n) = force_n else {
                    cut.unavailable = Some("切断要素の内力自由度順が節点と一致しません".into());
                    continue;
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
            run_id: run_id.into(),
            input_generation: Some(vec![42]),
            input_unavailable: None,
        };
        let mut po = PushoverResult {
            steps: vec![],
            wall_history: None,
            wall_run: Some(run),
            confirmed_history: Some(vec![]),
            ds_evaluation: None,
            capacity_evaluation: None,
            capacity_curve: vec![],
            hinges: vec![],
            shear_yields: vec![],
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
                    step: step as u32,
                    members: vec![],
                    cuts,
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
            assert!(
                po.evaluate_stories(&invalid, EvaluationPurpose::Ds)
                    .is_err(),
                "case {kind}"
            );
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
            assert!(invalid.evaluate_stories(&p, EvaluationPurpose::Ds).is_err());
        }
        let mut invalid = po.clone();
        invalid.confirmed_history = None;
        assert!(invalid
            .evaluate_stories(&p, EvaluationPurpose::Ds)
            .unwrap_err()
            .contains("確定履歴がありません"));
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
        for support_n in [0.0, 20.0] {
            let mut m = super::super::tests::two_story_model();
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
                SeismicDir::X,
                &heights,
                Default::default(),
            );
            recorder.reference_external = vec![0.0; map.n_active()];
            recorder.reference_external[map.active(6).unwrap() as usize] = 100_000.0;
            recorder.reference_external[map.active(12).unwrap() as usize] = 50_000.0;
            recorder.constant_external = vec![0.0; map.n_active()];
            recorder.record(1.0, &m, &map, &behaviors, &disp);
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
    #[test]
    fn 開口を持つ隣接壁の共有側柱を壁系へ一度だけ計上する() {
        use sepika_core::ids::NodeId;
        let mut m = super::super::tests::wall_story_model_with(4000.0, 100_000.0);
        m.elements.retain(|e| [0, 3, 4].contains(&e.id.0));
        for n in &mut m.nodes {
            n.restraint = sepika_core::dof::Dof6Mask::FREE;
        }
        for (source, id, x) in [
            (1, 4, 8000.0),
            (2, 5, 8000.0),
            (0, 6, 12000.0),
            (3, 7, 12000.0),
        ] {
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
        let totals = [40.0, 5.0, 10.0, 30.0, 5.0, 60.0];
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
                            forces[k * 6 + d] = sign * q / upper as f64
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
                let mut c = record_cuts(&m, &sepika_core::dof::DofMap::build(&m), &behaviors, dir)
                    .remove(0);
                c.external_n = sign * 150.0;
                c.reference_n = sign * 150.0;
                c.tolerance_n = 1e-6;
                let v = c.evaluate().unwrap();
                assert_eq!(v.wall_n, sign * 90.0);
                assert_eq!(v.frame_n, sign * 60.0);
                assert_eq!(v.beta_u, 0.6);
                assert_eq!(c.forces.iter().filter(|f| f.elem == ElemId(4)).count(), 1);
            }
        }
    }
}
