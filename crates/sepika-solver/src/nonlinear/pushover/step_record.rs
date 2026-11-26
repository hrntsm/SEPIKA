//! 増分解析の「1 ステップ確定時の記録」を 1 か所へ集める。
//!
//! 確定ステップの記録はどのフェーズでも同一とし、終了目標の判定は呼び出し側が行う。
use super::ductility::{compute_ductility_refs, update_ductility, DuctilityRef, DuctilityTracker};
use super::hinge::{compute_hinge_thresholds, track_hinges, HingeThreshold};
use super::member_response::record_member_step;
use super::response::{compute_story_drift, get_roof_disp, max_story_drift_angle};
use super::shear_yield::{record_member_shear, track_shear_yield, ShearThreshold};
use super::types::{
    CapacityPoint, DuctilityMethod, HingeEvent, MemberShearDiagnostic, MemberStepState,
    PushoverStep, ShearYieldEvent,
};
use crate::statics::analysis::SeismicDir;
use sepika_core::dof::DofMap;
use sepika_core::model::Model;
use sepika_core::src_wall_capacity::{is_src_wall_candidate, resolve_src_wall, SrcWallCapacity};
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse};
use sepika_element::wall::src_face::{src_wall_shear_strength, SrcWallFaces};

use super::story_response::{SrcFrameFaceReview, SrcWallShearReview};
use sepika_core::section_shape::SectionShape;

type SrcWallRecording = (usize, Result<(SrcWallFaces, SrcWallCapacity), String>);

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
    fixed_dofs: std::sync::Arc<[[bool; 6]]>,
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
    shear_diagnostics: Vec<MemberShearDiagnostic>,
    member_history_steps: Vec<Vec<MemberStepState>>,
    wall_history: Vec<super::wall_response::WallStepResponse>,
    pub(super) run: super::wall_response::WallRunIdentity,
    pub(super) reference_external: Vec<f64>,
    pub(super) constant_external: Vec<f64>,
    pub(super) physical_loads: super::loads::PhysicalPushoverLoads,
    confirmed_history: Vec<super::story_response::ConfirmedStepResponse>,
    src_wall_recording: Vec<SrcWallRecording>,
}

/// 解析終了時に [`StepRecorder`] から取り出す蓄積結果。
pub(super) struct RecordedSteps {
    pub(super) capacity_curve: Vec<CapacityPoint>,
    pub(super) steps: Vec<PushoverStep>,
    pub(super) hinges: Vec<HingeEvent>,
    pub(super) shear_yields: Vec<ShearYieldEvent>,
    pub(super) shear_diagnostics: Vec<MemberShearDiagnostic>,
    pub(super) member_history_steps: Vec<Vec<MemberStepState>>,
    pub(super) wall_history: Vec<super::wall_response::WallStepResponse>,
    pub(super) run: super::wall_response::WallRunIdentity,
    pub(super) confirmed_history: Vec<super::story_response::ConfirmedStepResponse>,
}

impl<'a> StepRecorder<'a> {
    pub(super) fn new(
        model: &Model,
        dofmap: &DofMap,
        dir: SeismicDir,
        heights: &'a [f64],
        ductility_method: DuctilityMethod,
        shear_thresholds: Vec<ShearThreshold>,
        physical_loads: super::loads::PhysicalPushoverLoads,
    ) -> Self {
        let mut run = super::wall_response::WallRunIdentity::new();
        let src_wall_recording = model
            .elements
            .iter()
            .enumerate()
            .filter(|(_, wall)| is_src_wall_candidate(model, wall))
            .map(|(index, wall)| {
                run.src_wall_elements.push(wall.id);
                let resolved = (|| {
                    let faces = SrcWallFaces::new(wall, model)?;
                    let rule =
                        resolve_src_wall(model, wall)?.ok_or("SRC耐震壁の断面がありません")?;
                    Ok((faces, rule))
                })();
                (index, resolved)
            })
            .collect();
        Self {
            dir,
            heights,
            hinge_thresholds: compute_hinge_thresholds(model),
            shear_thresholds,
            ductility_refs: compute_ductility_refs(model),
            ductility_method,
            ductility_trackers: vec![DuctilityTracker::default(); model.elements.len()],
            step_no: 0,
            capacity_curve: Vec::new(),
            steps: Vec::new(),
            hinges: Vec::new(),
            shear_yields: Vec::new(),
            shear_diagnostics: Vec::new(),
            member_history_steps: Vec::new(),
            wall_history: Vec::new(),
            run,
            reference_external: vec![],
            constant_external: vec![],
            physical_loads,
            fixed_dofs: super::physical_response::fixed_dofs(model, dofmap),
            confirmed_history: vec![],
            src_wall_recording,
        }
    }

    /// 確定したステップを 1 つ記録する。
    ///
    /// `load_factor` は参照外力ベクトル q に対する倍率で、呼び出し側が決めて渡す。
    /// 性能曲線は同じ確定状態の物理材端力と支持反力から回復する。
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
        let source = super::physical_response::current_case(&self.physical_loads, 1.0, load_factor);
        let physical = super::physical_response::PhysicalStep::recover(
            model,
            dofmap,
            self.fixed_dofs.clone(),
            behaviors,
            &responses,
            total_disp,
            source,
            self.physical_loads.reference.clone(),
        )?;
        let capacity = physical.capacity_forces(model, dofmap, self.dir)?;
        let roof = get_roof_disp(total_disp, model, dofmap, self.dir);
        let base_shear = capacity.base;
        let story_drift = compute_story_drift(model, dofmap, total_disp, self.dir);
        let drift_angle = max_story_drift_angle(&story_drift, self.heights);

        let mut cuts = super::story_response::record_cuts_with_responses(
            model, dofmap, behaviors, self.dir, &responses, &physical,
        );
        let external: Vec<_> = (0..dofmap.n_active())
            .map(|a| {
                self.reference_external.get(a).copied().unwrap_or(0.) * load_factor
                    + self.constant_external.get(a).copied().unwrap_or(0.)
            })
            .collect();
        let force_scale = crate::common::newton::l2_norm(&external).max(1.0);
        let d = usize::from(self.dir == SeismicDir::Y);
        for cut in &mut cuts {
            cut.reference_n = super::physical_response::load_above(
                model,
                &physical.reference,
                cut.elevation_mm,
                d,
            )?;
            cut.support_n = -physical.support_above(model, cut.elevation_mm, d);
            cut.external_n =
                super::physical_response::load_above(model, &physical.source, cut.elevation_mm, d)?
                    - cut.support_n;
            cut.tolerance_n = crate::common::newton::STATIC_NEWTON.tol
                * force_scale
                * (model.nodes.len() as f64).sqrt();
        }

        self.capacity_curve.push(CapacityPoint {
            step: self.step_no,
            roof_disp: roof,
            base_shear,
            story_shear: capacity.story,
            story_drift: story_drift.clone(),
        });
        self.steps.push(PushoverStep {
            load_factor,
            top_disp: roof,
            base_shear,
            story_drifts: story_drift,
            floor_disp: model
                .stories
                .iter()
                .map(|story| {
                    super::response::story_reference_node(model, story).map_or([0.0; 3], |node| {
                        std::array::from_fn(|i| {
                            let global = node.index() * 6 + [0, 1, 5][i];
                            dofmap
                                .active(global)
                                .and_then(|a| total_disp.get(a as usize).copied())
                                .unwrap_or(0.0)
                        })
                    })
                })
                .collect(),
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
        record_member_shear(
            model,
            behaviors,
            self.step_no,
            &mut self.shear_yields,
            &mut self.shear_diagnostics,
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
        let src_walls = self
            .src_wall_recording
            .iter()
            .map(|recording| {
                src_wall_review(model, behaviors, &responses, &physical.source, recording)
            })
            .collect();
        self.confirmed_history
            .push(super::story_response::ConfirmedStepResponse {
                run_id: self.run.run_id.clone(),
                input_generation: None,
                direction: self.dir,
                sign: self.run.sign,
                step: self.step_no,
                members: super::member_response::compute_member_response(
                    model,
                    dofmap,
                    behaviors,
                    total_disp,
                    self.dir,
                    &responses,
                    (&physical.source, force_scale),
                ),
                cuts,
                src_walls,
                src_frame_faces: src_frame_face_reviews(model, &responses),
            });
        self.step_no += 1;

        Ok(RecordedStep { roof, drift_angle })
    }

    /// 1 ステップも確定していないか。
    pub(super) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 性能曲線上の最大ベースシア [N]。目的別の採用層耐力とは区別する。
    pub(super) fn qu(&self) -> f64 {
        self.capacity_curve
            .iter()
            .map(|c| c.base_shear * self.run.sign.factor())
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
            shear_diagnostics: self.shear_diagnostics,
            member_history_steps: self.member_history_steps,
            wall_history: self.wall_history,
            run: self.run,
            confirmed_history: self.confirmed_history,
        }
    }
}

fn src_frame_face_reviews(
    model: &Model,
    responses: &[Option<LoadedFrameResponse>],
) -> Vec<SrcFrameFaceReview> {
    use sepika_core::model::FrameSectionUse;

    let mut adjacency = None;
    model
        .elements
        .iter()
        .zip(responses)
        .filter_map(|(elem, response)| {
            let section = model.element_section(elem)?;
            (elem.kind.is_primary_frame_support()
                && matches!(
                    (section.frame_use, section.shape.as_ref()),
                    (
                        Some(FrameSectionUse::Column),
                        Some(SectionShape::SrcColumnRect { .. })
                    ) | (
                        Some(FrameSectionUse::Girder),
                        Some(SectionShape::SrcBeamRect { .. })
                    )
                ))
            .then_some((
                elem,
                response,
                section.frame_use == Some(FrameSectionUse::Girder),
            ))
        })
        .map(|(elem, response, girder)| {
            let at_face = |position_mm, length_mm| {
                response
                    .as_ref()
                    .ok_or("SRC線材の同じ確定状態の荷重込み断面力がありません")?
                    .section
                    .at_continuous_section_mm(position_mm, length_mm)
            };
            if girder {
                let adjacency = adjacency.get_or_insert_with(|| {
                    sepika_core::adjacency::NodeAdjacency::primary_frame(model)
                });
                let faces = sepika_core::face_distance::girder_column_faces(model, elem, adjacency)
                    .unwrap_or_else(|| {
                        std::array::from_fn(|_| Err("SRC梁の実柱面位置がありません".into()))
                    })
                    .map(|face| face.and_then(|face| at_face(face.position_mm, face.length_mm)));
                return SrcFrameFaceReview {
                    elem: elem.id,
                    faces,
                };
            }
            let length_mm = model.member_length(elem);
            let distances = [elem.rigid_zone.face_i, elem.rigid_zone.face_j];
            let faces = std::array::from_fn(|end| {
                let distance_mm = distances[end].ok_or("SRC柱の実フェース位置がありません")?;
                if !length_mm.is_finite()
                    || length_mm <= 0.0
                    || !distance_mm.is_finite()
                    || !(0.0..length_mm).contains(&distance_mm)
                    || distances[1 - end]
                        .is_some_and(|opposite| distance_mm + opposite >= length_mm)
                {
                    return Err("SRC柱の実フェース位置が不正です".into());
                }
                let position_mm = if end == 0 {
                    distance_mm
                } else {
                    length_mm - distance_mm
                };
                at_face(position_mm, length_mm)
            });
            SrcFrameFaceReview {
                elem: elem.id,
                faces,
            }
        })
        .collect()
}

fn src_wall_review(
    model: &Model,
    behaviors: &[Box<dyn ElementBehavior>],
    responses: &[Option<LoadedFrameResponse>],
    case: &sepika_core::model::LoadCase,
    (index, resolved): &SrcWallRecording,
) -> SrcWallShearReview {
    let elem = model.elements[*index].id;
    let mut review = SrcWallShearReview {
        elem,
        ends: None,
        demand_n: None,
        limit_n: None,
        unavailable: None,
    };
    let result = (|| {
        let (faces, rule) = resolved.as_ref().map_err(Clone::clone)?;
        faces.ensure_loads(std::slice::from_ref(case))?;
        let ctx = Ctx { model };
        let wall = behaviors[*index]
            .state_member_forces(&ctx)
            .ok_or("壁板の同じ状態の断面力がありません")?;
        let mut members = vec![(elem, wall)];
        for (column, positions) in faces.column_cut_positions() {
            let column_index = model
                .elements
                .iter()
                .position(|e| e.id == column)
                .ok_or("側柱がありません")?;
            let response = responses[column_index]
                .as_ref()
                .ok_or_else(|| format!("側柱 ID {} の同じ状態の断面力がありません", column.0,))?;
            members.push((column, response.section.recover(&positions)?));
        }
        let batch: Vec<_> = members
            .iter()
            .map(|(id, forces)| (*id, forces.at.as_slice()))
            .collect();
        let mut ends = faces.recover(&batch)?;
        if sepika_core::model::wall_element_geometry(&model.elements[*index], model)
            .is_some_and(|geometry| wall_shear_is_roundoff(&ends, &geometry, case))
        {
            for end in &mut ends {
                end.shear_n = 0.0;
            }
        }
        review.demand_n = Some(ends[0].shear_n.abs().max(ends[1].shear_n.abs()));
        review.ends = Some(ends);
        review.limit_n = src_wall_shear_strength(rule, ends)?;
        Ok::<(), String>(())
    })();
    review.unavailable = result.err();
    review
}

fn wall_shear_is_roundoff(
    ends: &[sepika_core::src_wall_capacity::SrcWallEndForces; 2],
    geometry: &sepika_core::model::WallElementGeometry,
    case: &sepika_core::model::LoadCase,
) -> bool {
    let scale_n = ends.iter().fold(1.0_f64, |scale_n, end| {
        scale_n
            .max(end.axial_n.abs())
            .max(end.columns_compression_n.abs())
            .max(end.moment_nmm.abs() / geometry.h)
    });
    let parallel = |v: &[f64]| v[0] * geometry.ex_bottom[0] + v[1] * geometry.ex_bottom[1];
    // 面内荷重が相殺しても、壁の各断面に同じ断面力が作用するとは限らない。
    ends.iter()
        .all(|end| end.shear_n.abs() <= 64.0 * f64::EPSILON * scale_n)
        && case
            .nodal
            .iter()
            .all(|load| parallel(&load.values) == 0.0 && load.values[3..] == [0.0; 3])
        && case.member.iter().all(|load| parallel(&load.dir) == 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::ElemId;
    use sepika_core::model::{MemberLoad, MemberLoadKind};

    #[test]
    fn cancelling_in_plane_loads_do_not_certify_zero_wall_shear() {
        use sepika_core::ids::{LoadCaseId, NodeId};
        use sepika_core::model::{LoadCase, NodalLoad};
        use sepika_core::src_wall_capacity::SrcWallEndForces;

        let model = super::super::tests::prepared_wall_story_model_with(4000.0, 100_000.0);
        let wall = model
            .elements
            .iter()
            .find(|e| e.kind == sepika_core::model::ElementKind::Wall)
            .unwrap();
        let geometry = sepika_core::model::wall_element_geometry(wall, &model).unwrap();
        let ends = [SrcWallEndForces {
            axial_n: 800_000.0,
            shear_n: 1e-10,
            ..Default::default()
        }; 2];
        let mut case = LoadCase {
            origin: Default::default(),
            id: LoadCaseId(0),
            name: "重力".into(),
            kind: Default::default(),
            nodal: vec![NodalLoad::manual(
                NodeId(0),
                [0.0, 0.0, -800_000.0, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        };
        assert!(wall_shear_is_roundoff(&ends, &geometry, &case));
        for (node, sign) in [geometry.bottom[0], geometry.top[0]]
            .into_iter()
            .zip([1.0, -1.0])
        {
            case.nodal.push(NodalLoad::manual(
                node,
                [
                    sign * geometry.ex_bottom[0],
                    sign * geometry.ex_bottom[1],
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ],
            ));
        }
        assert!(!wall_shear_is_roundoff(&ends, &geometry, &case));
    }

    #[test]
    fn unrecordable_member_step_preserves_preceding_confirmed_history() {
        let model = super::super::tests::single_column_model(235.0, 80_000.0);
        let map = DofMap::build(&model);
        let heights = super::super::response::story_heights(&model);
        let mut recorder = StepRecorder::new(
            &model,
            &map,
            SeismicDir::X,
            &heights,
            DuctilityMethod::default(),
            crate::nonlinear::pushover::shear_yield::build_shear_yield_thresholds(&model),
            super::super::loads::PhysicalPushoverLoads::empty(),
        );
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
    fn loaded_wall_owner_and_ordinary_girder_keep_total_resistance_and_same_step_cut_partition() {
        let mut model = super::super::tests::prepared_wall_story_model_with(4000.0, 100_000.0);
        for elem in &mut model.elements {
            elem.rigid_zone = Default::default();
        }
        let map = DofMap::build(&model);
        let heights = super::super::response::story_heights(&model);
        let mut recorder = StepRecorder::new(
            &model,
            &map,
            SeismicDir::X,
            &heights,
            DuctilityMethod::default(),
            crate::nonlinear::pushover::shear_yield::build_shear_yield_thresholds(&model),
            super::super::loads::PhysicalPushoverLoads::empty(),
        );
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
            let (legacy_rhs, loads) = crate::common::linear_load::prepare_nonlinear_loads(
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
            assert!(horizontal_rhs.abs() < 1e-7);
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
            assert!(responses[2].is_some());
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
            recorder.physical_loads = super::super::loads::PhysicalPushoverLoads::new(
                &model,
                &[(model.load_cases[0].id, 1.)],
                super::super::loads::PhysicalPushoverLoads::empty().reference,
            )
            .unwrap();
            recorder
                .record(1.0, &model, &map, &behaviors, &displacement)
                .unwrap();
            let expected_capacity = 14000.0;
            assert!(
                (result.story_shears.as_ref().unwrap()[0][0] - sign * expected_capacity).abs()
                    < 1e-5
            );
            let capacity = &recorder.capacity_curve[step];
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
            let shear = super::super::shear_yield::build_shear_yield_thresholds(&model);
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
