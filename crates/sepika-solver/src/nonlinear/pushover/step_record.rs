//! 増分解析の「1 ステップ確定時の記録」を 1 か所へ集める。
//!
//! 確定ステップの記録はどのフェーズでも同一とし、終了目標の判定は呼び出し側が行う。
use super::ductility::{compute_ductility_refs, update_ductility, DuctilityRef, DuctilityTracker};
use super::hinge::{compute_hinge_thresholds, track_hinges, HingeThreshold};
use super::member_response::record_member_step;
use super::response::{
    compute_base_shear, compute_story_drift, compute_story_shear, get_roof_disp,
    max_story_drift_angle,
};
use super::shear_yield::{compute_shear_yield_thresholds, track_shear_yield, ShearThreshold};
use super::types::{
    CapacityPoint, DuctilityMethod, HingeEvent, MemberStepState, PushoverStep, ShearYieldEvent,
};
use crate::common::tangent::{add_support_spring_f_int, compute_f_int};
use crate::statics::analysis::SeismicDir;
use sepika_core::dof::DofMap;
use sepika_core::model::Model;
use sepika_element::behavior::ElementBehavior;

/// 1 ステップを記録した結果のうち、呼び出し側が制御判断に使う値。
///
/// 終了目標の判定（[`super::types::PushoverTarget::reached`]）と、均等刻みの
/// 勾配更新（頂部変位の増分）に用いる。
pub(super) struct RecordedStep {
    /// 頂部変位 [mm]（加力方向成分）。
    pub(super) roof: f64,
    /// 全層の最大層間変形角 [rad]。
    pub(super) drift_angle: f64,
}

/// 確定ステップの記録先と、記録に要る追跡状態をまとめて持つ。
///
/// 蓄積したものは解析の最後に [`StepRecorder::finish`] で受け取る。
pub(super) struct StepRecorder<'a> {
    /// 加力方向（ベースシア・層せん断・層間変位の射影に使う）。
    dir: SeismicDir,
    /// 各層の階高 [mm]（層間変形角の分母）。
    heights: &'a [f64],
    /// 部材ごとの曲げヒンジ発生モーメント閾値。
    hinge_thresholds: Vec<HingeThreshold>,
    /// 部材ごとのせん断降伏耐力 Qy。
    shear_thresholds: Vec<ShearThreshold>,
    /// 部材ごとの塑性率基点。
    ductility_refs: Vec<DuctilityRef>,
    /// 塑性率の定義（基点曲率の取り方）。
    ductility_method: DuctilityMethod,
    /// 部材ごとの塑性率トラッカー（最大応答曲率を保持する）。
    ductility_trackers: Vec<DuctilityTracker>,
    /// 記録用の通し番号。長期載荷→荷重制御→変位制御で連番とし、
    /// **確定したステップにのみ**採番する。`capacity_curve`・`steps` の並びと
    /// ヒンジ・せん断降伏イベントの `step` を対応付ける単調キーになる。
    step_no: u32,
    capacity_curve: Vec<CapacityPoint>,
    steps: Vec<PushoverStep>,
    hinges: Vec<HingeEvent>,
    shear_yields: Vec<ShearYieldEvent>,
    member_history_steps: Vec<Vec<MemberStepState>>,
    wall_history: Vec<super::wall_response::WallStepResponse>,
    pub(super) run: super::wall_response::WallRunIdentity,
    pub(super) reference_external: Vec<f64>,
    pub(super) constant_external: Vec<f64>,
    confirmed_history: Vec<super::story_response::ConfirmedStepResponse>,
}

/// 解析終了時に [`StepRecorder`] から取り出す蓄積結果。
pub(super) struct RecordedSteps {
    pub(super) capacity_curve: Vec<CapacityPoint>,
    pub(super) steps: Vec<PushoverStep>,
    pub(super) hinges: Vec<HingeEvent>,
    pub(super) shear_yields: Vec<ShearYieldEvent>,
    pub(super) member_history_steps: Vec<Vec<MemberStepState>>,
    pub(super) wall_history: Vec<super::wall_response::WallStepResponse>,
    pub(super) run: super::wall_response::WallRunIdentity,
    pub(super) confirmed_history: Vec<super::story_response::ConfirmedStepResponse>,
}

impl<'a> StepRecorder<'a> {
    pub(super) fn new(
        model: &Model,
        dir: SeismicDir,
        heights: &'a [f64],
        ductility_method: DuctilityMethod,
    ) -> Result<Self, String> {
        Ok(Self {
            dir,
            heights,
            hinge_thresholds: compute_hinge_thresholds(model),
            shear_thresholds: compute_shear_yield_thresholds(model)?,
            ductility_refs: compute_ductility_refs(model),
            ductility_method,
            ductility_trackers: vec![DuctilityTracker::default(); model.elements.len()],
            step_no: 0,
            capacity_curve: Vec::new(),
            steps: Vec::new(),
            hinges: Vec::new(),
            shear_yields: Vec::new(),
            member_history_steps: Vec::new(),
            wall_history: Vec::new(),
            run: super::wall_response::WallRunIdentity::new(),
            reference_external: vec![],
            constant_external: vec![],
            confirmed_history: vec![],
        })
    }

    /// 確定したステップを 1 つ記録する。
    ///
    /// `load_factor` は参照外力ベクトル q に対する倍率で、呼び出し側が決めて渡す。
    /// ベースシアは内力の釣合いから算定する。
    pub(super) fn record(
        &mut self,
        load_factor: f64,
        model: &Model,
        dofmap: &DofMap,
        behaviors: &[Box<dyn ElementBehavior>],
        total_disp: &[f64],
    ) -> Result<RecordedStep, String> {
        let responses = super::geom::member_recording_responses(model, behaviors)?;
        let member_step = record_member_step(model, dofmap, behaviors, total_disp, &responses)
            .map_err(|error| format!("増分解析・部材履歴 step={}: {error}", self.step_no))?;
        let owner_cut_loads = model
            .layers()
            .iter()
            .map(|layer| {
                let z = (model.stories[layer.bottom.index()].elevation
                    + model.stories[layer.top.index()].elevation)
                    * 0.5;
                model
                    .elements
                    .iter()
                    .zip(&responses)
                    .try_fold(0.0, |sum, (elem, response)| {
                        let Some(response) = response else {
                            return Ok(sum);
                        };
                        let force = response.section.applied_force_above_elevation(
                            model.nodes[elem.nodes[0].index()].coord[2],
                            model.nodes[elem.nodes[1].index()].coord[2],
                            z,
                        )?;
                        Ok::<_, String>(sum + force[usize::from(self.dir == SeismicDir::Y)])
                    })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let roof = get_roof_disp(total_disp, model, dofmap, self.dir);
        let mut f_int_now = compute_f_int(model, dofmap, behaviors);
        for ((elem, behavior), response) in model.elements.iter().zip(behaviors).zip(&responses) {
            if let Some(response) = response {
                let gdofs = behavior.global_dofs(dofmap);
                for (end, applied) in response
                    .section
                    .applied_nodal_forces_global()
                    .iter()
                    .enumerate()
                {
                    for (d, force) in applied.iter().enumerate() {
                        let a = gdofs[end * 6 + d];
                        if a != usize::MAX {
                            f_int_now[a] += force;
                        }
                    }
                }
                debug_assert_eq!(elem.nodes.len(), 2);
            }
        }
        add_support_spring_f_int(model, dofmap, total_disp, &mut f_int_now);
        let base_shear = compute_base_shear(model, dofmap, &f_int_now, self.dir);
        let story_drift = compute_story_drift(model, dofmap, total_disp, self.dir);
        let drift_angle = max_story_drift_angle(&story_drift, self.heights);

        self.capacity_curve.push(CapacityPoint {
            step: self.step_no,
            roof_disp: roof,
            base_shear,
            story_shear: compute_story_shear(model, dofmap, &f_int_now, self.dir),
            story_drift: story_drift.clone(),
        });
        self.steps.push(PushoverStep {
            load_factor,
            top_disp: roof,
            base_shear,
            story_drifts: story_drift,
        });
        let mu = update_ductility(
            behaviors,
            &mut self.ductility_trackers,
            &self.ductility_refs,
            self.ductility_method,
        );
        track_hinges(
            model,
            behaviors,
            &responses,
            &self.hinge_thresholds,
            &mu,
            self.step_no,
            &mut self.hinges,
        );
        track_shear_yield(
            model,
            behaviors,
            &responses,
            &self.shear_thresholds,
            self.step_no,
            &mut self.shear_yields,
        );
        self.member_history_steps.push(member_step);
        self.wall_history
            .extend(super::wall_response::record_wall_step(
                model,
                dofmap,
                behaviors,
                total_disp,
                self.dir,
                self.step_no,
            ));
        let mut cuts = super::story_response::record_cuts(model, dofmap, behaviors, self.dir);
        let external: Vec<_> = self
            .reference_external
            .iter()
            .zip(&self.constant_external)
            .map(|(q, f)| q * load_factor + f)
            .collect();
        let mut support = vec![0.0; dofmap.n_active()];
        add_support_spring_f_int(model, dofmap, total_disp, &mut support);
        let force_scale = crate::common::newton::l2_norm(&external).max(1.0);
        let d = usize::from(self.dir == SeismicDir::Y);
        for (cut, owner_load) in cuts.iter_mut().zip(owner_cut_loads) {
            let sum_above = |f: &[f64]| {
                model
                    .nodes
                    .iter()
                    .filter(|n| n.coord[2] > cut.elevation_mm)
                    .filter_map(|n| dofmap.active(n.id.index() * 6 + d))
                    .filter_map(|a| f.get(a as usize))
                    .sum::<f64>()
            };
            cut.reference_n = sum_above(&self.reference_external);
            cut.support_n = sum_above(&support);
            cut.external_n = sum_above(&external) + owner_load - cut.support_n;
            cut.tolerance_n = crate::common::newton::STATIC_NEWTON.tol
                * force_scale
                * (model.nodes.len() as f64).sqrt();
        }
        self.confirmed_history
            .push(super::story_response::ConfirmedStepResponse {
                run_id: self.run.run_id.clone(),
                input_generation: None,
                direction: self.dir,
                step: self.step_no,
                members: super::member_response::compute_member_response_with_responses(
                    model, dofmap, behaviors, total_disp, self.dir, &responses,
                ),
                cuts,
            });
        self.step_no += 1;

        Ok(RecordedStep { roof, drift_angle })
    }

    /// 1 ステップも確定していないか。
    pub(super) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 性能曲線上の最大ベースシア [N]。目的別の採用層耐力とは区別する。
    /// 単調載荷では崩壊機構形成後に頭打ちとなるため、ピーク値を採る。
    pub(super) fn qu(&self) -> f64 {
        self.capacity_curve
            .iter()
            .map(|c| c.base_shear)
            .fold(0.0_f64, f64::max)
    }

    /// これまでに記録したヒンジ発生イベント（崩壊機構の判定に使う）。
    pub(super) fn hinges(&self) -> &[HingeEvent] {
        &self.hinges
    }

    /// これまでに記録したせん断降伏イベント。
    pub(super) fn shear_yields(&self) -> &[ShearYieldEvent] {
        &self.shear_yields
    }

    pub(super) fn finish(self) -> RecordedSteps {
        RecordedSteps {
            capacity_curve: self.capacity_curve,
            steps: self.steps,
            hinges: self.hinges,
            shear_yields: self.shear_yields,
            member_history_steps: self.member_history_steps,
            wall_history: self.wall_history,
            run: self.run,
            confirmed_history: self.confirmed_history,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::ElemId;
    use sepika_core::model::{MemberLoad, MemberLoadKind};

    #[test]
    fn unrecordable_member_step_preserves_preceding_confirmed_history() {
        let model = super::super::tests::single_column_model(235.0, 80_000.0);
        let map = DofMap::build(&model);
        let heights = super::super::response::story_heights(&model);
        let mut recorder =
            StepRecorder::new(&model, SeismicDir::X, &heights, DuctilityMethod::default()).unwrap();
        let behaviors: Vec<_> = model
            .elements
            .iter()
            .map(|elem| sepika_element::factory::build_behavior(elem, &model))
            .collect();
        let mut disp = vec![0.0; map.n_active()];
        recorder
            .record(0.0, &model, &map, &behaviors, &disp)
            .unwrap();
        disp[map.active(6).unwrap() as usize] = 1e45;
        let error = recorder
            .record(1.0, &model, &map, &behaviors, &disp)
            .err()
            .unwrap();
        for expected in [
            "step=1",
            "ElemId(0)",
            "rz_i=",
            "f32 変換で有限範囲を超えました",
        ] {
            assert!(error.contains(expected), "{error}");
        }
        assert_eq!(recorder.step_no, 1);
        let records = recorder.finish();
        assert_eq!(records.steps.len(), 1);
        assert_eq!(records.capacity_curve.len(), 1);
        assert_eq!(records.member_history_steps.len(), 1);
        assert_eq!(records.confirmed_history.len(), 1);
        assert!(records.hinges.is_empty());
        assert!(records.shear_yields.is_empty());
        assert!(records.wall_history.is_empty());
        assert_eq!(records.member_history_steps[0][0].rz_i, 0.0);
    }

    #[test]
    fn loaded_wall_owner_and_ordinary_girder_keep_legacy_capacity_and_same_step_cut_partition() {
        let mut model = super::super::tests::prepared_wall_story_model_with(4000.0, 100_000.0);
        for elem in &mut model.elements {
            elem.rigid_zone = Default::default();
        }
        let map = DofMap::build(&model);
        let heights = super::super::response::story_heights(&model);
        let mut recorder =
            StepRecorder::new(&model, SeismicDir::X, &heights, DuctilityMethod::default()).unwrap();
        for (step, (sign, point)) in [(1.0, false), (-1.0, false), (1.0, true), (-1.0, true)]
            .into_iter()
            .enumerate()
        {
            model.load_cases[0].nodal.clear();
            model.load_cases[0].member = [(ElemId(3), 3000.0), (ElemId(2), 4000.0)]
                .map(|(elem, length)| {
                    MemberLoad::manual(
                        elem,
                        [1.0, 0.0, 0.0],
                        MemberLoadKind::Distributed {
                            a: 0.0,
                            b: length,
                            w1: sign * 2.0,
                            w2: sign * 2.0,
                        },
                    )
                })
                .into();
            if point {
                model.load_cases[0].member[0].kind = MemberLoadKind::Point {
                    a: 2250.0,
                    p: sign * 6000.0,
                };
            }
            let upper_reaction = if point { 4500.0 } else { 3000.0 };
            let wall_shear = 8000.0 + upper_reaction;
            let above_load = if point { 6000.0 } else { 3000.0 };
            let result =
                crate::statics::linear::linear_static_once(&model, model.load_cases[0].id).unwrap();
            let mut displacement = vec![0.0; map.n_active()];
            for node in &model.nodes {
                for d in 0..6 {
                    if let Some(a) = map.active(node.id.index() * 6 + d) {
                        displacement[a as usize] = result.disp[node.id.index()][d];
                    }
                }
            }
            let mut behaviors: Vec<_> = model
                .elements
                .iter()
                .map(|elem| sepika_element::factory::build_behavior(elem, &model))
                .collect();
            let (legacy_rhs, loads) = super::super::loads::prepare_gravity(
                &model,
                &map,
                &behaviors,
                &model.load_cases[0],
            )
            .unwrap();
            let horizontal_rhs: f64 = model
                .nodes
                .iter()
                .filter_map(|node| map.active(node.id.index() * 6))
                .map(|a| legacy_rhs[a as usize])
                .sum();
            assert!((horizontal_rhs - sign * 8000.0).abs() < 1e-7);
            crate::common::elem_loop::apply_du_trial_with_member_loads(
                &model,
                &map,
                &mut behaviors,
                &displacement,
                &loads,
            )
            .unwrap();
            for behavior in &mut behaviors {
                behavior.commit_state();
            }
            let responses =
                super::super::geom::member_recording_responses(&model, &behaviors).unwrap();
            assert!(responses[2].is_none());
            let column = responses[3].as_ref().unwrap();
            for end in [0, 1] {
                assert!(
                    (column.physical_frame_end_force_global[end * 6]
                        + sign
                            * if end == 0 {
                                6000.0 - upper_reaction
                            } else {
                                upper_reaction
                            })
                    .abs()
                        < 1e-5
                );
            }
            let applied: f64 = column
                .section
                .applied_nodal_forces_global()
                .iter()
                .map(|end| end[0])
                .sum();
            assert!((applied - sign * 6000.0).abs() < 1e-7);
            recorder.reference_external = vec![0.0; map.n_active()];
            recorder.constant_external = legacy_rhs;
            recorder
                .record(1.0, &model, &map, &behaviors, &displacement)
                .unwrap();
            // 集中荷重の上端等価力は6000*(3*0.75²-2*0.75³)=5062.5 N。物理上端反力4500 Nとは異なる。
            let expected_capacity = if point { 13062.5 } else { 11000.0 };
            let capacity = &recorder.capacity_curve[step];
            println!("mixed point={point} sign={sign} base={} story={} wall={wall_shear} upper_reaction={upper_reaction} owner_residual={:?} owner_applied={:?}", capacity.base_shear, capacity.story_shear[0], column.nodal_residual.data, column.section.applied_nodal_forces_global());
            assert!((capacity.base_shear - sign * expected_capacity).abs() < 1e-5);
            assert!((capacity.story_shear[0] - sign * expected_capacity).abs() < 1e-5);
            let cut = &recorder.confirmed_history[step].cuts[0];
            assert_eq!(
                cut.forces
                    .iter()
                    .filter(|force| force.elem == ElemId(3))
                    .count(),
                1
            );
            assert!(
                (cut.forces
                    .iter()
                    .find(|force| force.elem == ElemId(3))
                    .unwrap()
                    .force_n
                    - sign * (above_load - upper_reaction))
                    .abs()
                    < 1e-5
            );
            let evaluated = cut.evaluate().unwrap();
            assert!((evaluated.wall_n - sign * (8000.0 + above_load)).abs() < 1e-5);
            assert!(evaluated.frame_n.abs() < 1e-5);
            let wall = recorder
                .wall_history
                .iter()
                .find(|wall| wall.step == step as u32)
                .unwrap()
                .response
                .as_ref()
                .unwrap();
            assert!((wall.qdir_n - sign * wall_shear).abs() < 1e-5);
        }
        assert!(recorder.wall_history[0].response.as_ref().unwrap().qdir_n > 0.0);
    }
    #[test]
    fn elastic_wall_girders_skip_yield_events_while_ordinary_girders_keep_them() {
        for with_wall in [true, false] {
            let mut model = super::super::tests::prepared_wall_story_model_with(4000.0, 100_000.0);
            if !with_wall {
                model
                    .elements
                    .retain(|elem| elem.kind != sepika_core::model::ElementKind::Wall);
            }
            let index = model
                .elements
                .iter()
                .position(|elem| elem.id == ElemId(2))
                .unwrap();
            let mut behaviors: Vec<_> = model
                .elements
                .iter()
                .map(|elem| sepika_element::factory::build_behavior(elem, &model))
                .collect();
            let mut du = sepika_element::behavior::LocalVec {
                data: smallvec::smallvec![0.0;12],
            };
            du.data[8] = 10000.0;
            behaviors[index]
                .update_state(&du, true, &sepika_element::behavior::Ctx { model: &model })
                .unwrap();
            let responses =
                super::super::geom::member_recording_responses(&model, &behaviors).unwrap();
            let thresholds = super::super::hinge::compute_hinge_thresholds(&model);
            let shear = super::super::shear_yield::compute_shear_yield_thresholds(&model).unwrap();
            assert_eq!(thresholds[index].my == 0.0, with_wall);
            assert_eq!(shear[index].y.qy(0.0).is_infinite(), with_wall);
            let mut hinges = vec![];
            let mut shear_events = vec![];
            super::super::hinge::track_hinges(
                &model,
                &behaviors,
                &responses,
                &thresholds,
                &vec![20.0; behaviors.len()],
                0,
                &mut hinges,
            );
            super::super::shear_yield::track_shear_yield(
                &model,
                &behaviors,
                &responses,
                &shear,
                0,
                &mut shear_events,
            );
            assert_eq!(
                hinges.iter().any(|event| event.elem == ElemId(2)),
                !with_wall
            );
            assert_eq!(
                shear_events.iter().any(|event| event.elem == ElemId(2)),
                !with_wall
            );
        }
    }
}
