use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ResultInputKey {
    Static(StaticCaseKey),
    Combo(String),
    Pushover(SeismicDir),
    Modal,
}

#[derive(Clone, Debug)]
pub struct HoldingCapacitySource {
    pub direction: SeismicDir,
    /// 下層から順に、Qu の最大値を採用した増分ステップ。非正の耐力は None。
    pub qu_steps: Vec<Option<u32>>,
    /// 部材応答・層間変形を採用した最終増分ステップ。
    pub response_step: Option<u32>,
}

impl ResultsBundle {
    pub(super) fn record_input(&mut self, key: ResultInputKey, input: Vec<u8>) {
        self.input_records.retain(|(k, _)| *k != key);
        self.input_records.push((key, input));
    }
}

impl App {
    fn calculation_model(&self) -> sepika_core::model::Model {
        let mut model = self.core.model.clone();
        model.axes.clear();
        model.vibration_cases.clear();
        model.lumped_vibration_cases.clear();
        model
    }

    pub(super) fn result_input(&self, key: &ResultInputKey) -> Vec<u8> {
        let cfg = self.core.analysis_cfg;
        let mut relevant = AnalysisSettings {
            ai_mode: cfg.ai_mode,
            z: cfg.z,
            soil: cfg.soil,
            c0: cfg.c0,
            mass_method: cfg.mass_method,
            heavy_snow_zone: cfg.heavy_snow_zone,
            snow_delta1: cfg.snow_delta1,
            snow_delta3: cfg.snow_delta3,
            threads: cfg.threads,
            ..Default::default()
        };
        if let ResultInputKey::Pushover(dir) = key {
            relevant.push_dir = *dir;
            relevant.push_steps = cfg.push_steps;
            relevant.push_max_disp = cfg.push_max_disp;
            relevant.push_use_max_disp = cfg.push_use_max_disp;
            relevant.push_use_drift_angle = cfg.push_use_drift_angle;
            relevant.push_drift_denom = cfg.push_drift_denom;
            relevant.push_control = cfg.push_control;
            relevant.push_apply_long_term = cfg.push_apply_long_term;
            relevant.ductility_method = cfg.ductility_method;
        }
        let mut model = self.calculation_model();
        if matches!(key, ResultInputKey::Modal) {
            // 固有周期で再生成する EX/EY を含めると、Ai 更新だけで固有値自身が
            // 陳腐化する。固有値の剛性・質量行列はこれらの水平荷重に依存しない。
            model
                .load_cases
                .retain(|case| !is_standard_seismic_case(case));
        }
        bincode::serialize(&(model, relevant, self.result_generation_period(key)))
            .expect("計算入力の直列化")
    }

    fn result_generation_period(&self, key: &ResultInputKey) -> Option<f64> {
        if !matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise) {
            return None;
        }
        let depends_on_seismic = match key {
            ResultInputKey::Pushover(_) | ResultInputKey::Static(StaticCaseKey::Seismic(_)) => true,
            ResultInputKey::Static(StaticCaseKey::User(id)) => self
                .core
                .model
                .load_cases
                .iter()
                .any(|case| case.id == *id && is_standard_seismic_case(case)),
            ResultInputKey::Combo(name) => self
                .core
                .model
                .combinations
                .iter()
                .find(|combo| combo.name == *name)
                .is_some_and(|combo| {
                    combo.terms.iter().any(|(id, _)| {
                        self.core
                            .model
                            .load_cases
                            .iter()
                            .any(|case| case.id == *id && is_standard_seismic_case(case))
                    })
                }),
            ResultInputKey::Modal => false,
        };
        depends_on_seismic
            .then(|| self.design_seismic_period().ok())
            .flatten()
    }

    pub(super) fn job_input(&self) -> Vec<u8> {
        let period = matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise)
            .then(|| self.design_seismic_period().ok())
            .flatten();
        bincode::serialize(&(self.calculation_model(), self.core.analysis_cfg, period))
            .expect("ジョブ入力の直列化")
    }

    pub(super) fn require_result_input(&self, key: ResultInputKey) -> Result<(), String> {
        let label = match &key {
            ResultInputKey::Static(StaticCaseKey::Seismic(dir)) => format!("地震静的 {dir:?}"),
            ResultInputKey::Static(StaticCaseKey::User(id)) => format!("静的荷重ケース {id:?}"),
            ResultInputKey::Combo(name) => format!("静的組合せ「{name}」"),
            ResultInputKey::Pushover(dir) => format!("増分解析 {dir:?}"),
            ResultInputKey::Modal => "固有値解析".into(),
        };
        let input = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.input_records.iter().find(|(k, _)| *k == key))
            .map(|(_, input)| input);
        match input {
            Some(input) if *input == self.result_input(&key) => Ok(()),
            Some(_) => Err(format!("保有水平耐力の判定は無効です。{label}の生成入力が現在のモデル・計算設定と一致しません。{label}を再実行してください。")),
            None => Err(format!("保有水平耐力の判定は無効です。{label}の入力識別情報がありません。{label}を再実行してください。")),
        }
    }
}

fn is_standard_seismic_case(case: &sepika_core::model::LoadCase) -> bool {
    use sepika_core::model::{LoadCaseKind, EX_CASE_NAME, EY_CASE_NAME};
    case.kind == LoadCaseKind::Seismic && matches!(case.name.as_str(), EX_CASE_NAME | EY_CASE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::{dof::Dof6Mask, ids::StoryId};

    fn ready() -> App {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        assert!(app.compute_holding_capacity().is_ok());
        app
    }

    #[test]
    fn 層最大耐力と最終応答の採用ステップを区別して記録する() {
        let mut app = ready();
        let po = app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap();
        po.capacity_curve.truncate(3);
        for (point, (step, force)) in
            po.capacity_curve
                .iter_mut()
                .zip([(1, 100.0), (2, 200.0), (3, 150.0)])
        {
            point.step = step;
            point.story_shear[0] = force;
        }
        let result = app.compute_holding_capacity().unwrap().0;
        assert_eq!(result.stories[0].qu, 200.0);
        let source = app.core.scoped.holding_capacity_source.as_ref().unwrap();
        assert_eq!(source.direction, SeismicDir::X);
        assert_eq!(source.qu_steps, vec![Some(2)]);
        assert_eq!(source.response_step, Some(3));
        app.core.model.materials[0].fy = None;
        assert!(app.compute_holding_capacity().is_err());
        assert!(app.core.scoped.holding_capacity_source.is_none());
    }

    #[test]
    fn 材料編集後に静的解析だけ再実行しても旧増分結果は使えない() {
        let mut app = ready();
        app.core.model.materials[0].fy = None;
        app.core.scoped.staleness.mark_edited();
        app.run_seismic(SeismicDir::X);
        assert!(!app.core.scoped.staleness.results_stale);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(
            reason.contains("増分解析 X") && reason.contains("一致しません"),
            "{reason}"
        );
        assert!(app.core.scoped.ds_beta_u_by_story.is_empty());
        assert!(app.core.scoped.ds_rank_fallback_stories.is_empty());
        app.core.model.materials[0].fy = Some(235.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 各計算入力の編集後は保有耐力判定が無効になる() {
        let base = ready();
        let edits: &[fn(&mut App)] = &[
            |a| a.core.model.materials[0].fc = Some(99.0),
            |a| a.core.model.materials[0].fy = None,
            |a| a.core.model.sections[0].area *= 1.1,
            |a| a.core.model.sections[0].shape = None,
            |a| {
                a.core.model.wall_attrs.push(sepika_core::model::WallAttr {
                    elem: ElemId(0),
                    opening_area: 1.0,
                    opening_weight: 0.0,
                    slit: Default::default(),
                    openings: Vec::new(),
                    finish_intensity: 0.0,
                })
            },
            |a| a.core.model.load_cases[0].member.clear(),
            |a| {
                a.core
                    .model
                    .combinations
                    .push(sepika_core::model::LoadCombination {
                        name: "追加".into(),
                        terms: vec![(LoadCaseId(0), 1.0)],
                    })
            },
            |a| a.core.model.stories[0].seismic_weight = Some(1.0),
            |a| a.core.analysis_cfg.z = 0.8,
            |a| a.core.analysis_cfg.push_steps += 1,
            |a| {
                a.core.model.stress_cfg.rigid_zone_consider_walls =
                    !a.core.model.stress_cfg.rigid_zone_consider_walls
            },
        ];
        for (index, edit) in edits.iter().enumerate() {
            let mut app = App::default();
            app.core.model = base.core.model.clone();
            app.core.analysis_cfg = base.core.analysis_cfg;
            app.core.scoped.results = base.core.scoped.results.clone();
            edit(&mut app);
            assert!(app.compute_holding_capacity().is_err(), "編集 {index}");
        }
    }

    #[test]
    fn 表示ステップと表示選択と表示専用通り芯の変更は利用を維持する() {
        let mut app = ready();
        let initial = app.compute_holding_capacity().unwrap().0.stories[0].qun;
        #[cfg(feature = "gui")]
        {
            app.ui.scoped.hinge_step = Some(1);
            app.ui.view.camera.yaw += 0.1;
        }
        app.ui.scoped.nav.focus_result = Some(StaticKey::Case(StaticCaseKey::User(LoadCaseId(0))));
        app.core.model.axes.push(sepika_core::model::AxisGroup {
            name: "表示".into(),
            kind: sepika_core::model::AxisGroupKind::Other,
            axes: Vec::new(),
        });
        app.core.scoped.staleness.mark_non_calc_edited();
        app.core.analysis_cfg.th_record_every = 17;
        app.core.analysis_cfg.seismic_dir = SeismicDir::Y;
        assert_eq!(
            app.compute_holding_capacity().unwrap().0.stories[0].qun,
            initial
        );
    }

    #[test]
    fn 方向不足と他方向の旧結果は代用しない() {
        let mut app = ready();
        app.set_pushover_view_dir(SeismicDir::Y);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(reason.contains("増分解析 Y"), "{reason}");
        app.set_pushover_view_dir(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.core.analysis_cfg.push_dir = SeismicDir::Y;
        app.run_pushover();
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.core.analysis_cfg.push_dir = SeismicDir::X;
        app.run_pushover();
        assert!(app.compute_holding_capacity().is_ok());
        app.set_pushover_view_dir(SeismicDir::Y);
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(
            reason.contains("増分解析 Y") && reason.contains("一致しません"),
            "{reason}"
        );
    }

    #[test]
    fn 略算_qudは固有値に依存せず偏心精算の静的結果は入力一致を要求する() {
        let mut app = ready();
        app.run_seismic(SeismicDir::Y);
        app.run_eigen(1);
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("地震静的 Y"));
        app.run_seismic(SeismicDir::Y);
        assert!(app.compute_holding_capacity().is_ok());
        app.run_eigen(1);
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 精算周期は固有値を再解析しても標準地震結果として採用しない() {
        let mut app = ready();
        app.run_eigen(1);
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_err());
        let old_period = app.design_seismic_period().unwrap();
        app.core.model.materials[0].young *= 0.5;
        app.run_eigen(1);
        assert!(app.design_seismic_period().unwrap() > old_period);
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_err());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .seismic(SeismicDir::X)
            .is_none());
        assert!(app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .seismic(SeismicDir::Y)
            .is_none());
    }

    #[test]
    fn 略算周期での固有値更新は地震結果の入力識別を変えない() {
        let mut app = ready();
        app.run_eigen(1);
        let push_input = app.result_input(&ResultInputKey::Pushover(SeismicDir::X));
        let static_input = app.result_input(&ResultInputKey::Static(StaticCaseKey::Seismic(
            SeismicDir::X,
        )));
        app.run_eigen(3);
        assert_eq!(
            app.result_input(&ResultInputKey::Pushover(SeismicDir::X)),
            push_input
        );
        assert_eq!(
            app.result_input(&ResultInputKey::Static(StaticCaseKey::Seismic(
                SeismicDir::X
            ))),
            static_input
        );
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 時刻歴だけ再実行しても旧増分結果を最新にしない() {
        let mut app = ready();
        app.core.model.materials[0].fy = Some(345.0);
        app.run_time_history(sepika_solver::dynamic::timehistory::GroundMotion {
            dt: 0.01,
            accel_x: vec![0.0; 3],
            accel_y: None,
            accel_theta: None,
        });
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        assert!(!app.core.scoped.staleness.results_stale);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("増分解析 X"));
    }

    #[test]
    fn 長期軸力の精算に使う組合せも再解析が必要になる() {
        let mut app = ready();
        let p = LoadCaseId(app.core.model.load_cases.len() as u32);
        app.core
            .model
            .load_cases
            .push(sepika_core::model::LoadCase {
                id: p,
                name: "P=0".into(),
                kind: sepika_core::model::LoadCaseKind::Live,
                nodal: vec![],
                member: vec![],
            });
        app.core
            .model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "長期".into(),
                terms: vec![(LoadCaseId(0), 1.0), (p, 1.0)],
            });
        app.run_static_all();
        app.run_pushover();
        assert!(app.compute_holding_capacity().is_ok());
        app.core.model.materials[0].fy = Some(345.0);
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("静的組合せ「長期」"));
        app.run_static_all();
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 剛域反映で変化した入力も判定前に検査する() {
        let mut app = ready();
        app.core.model.elements[2].rigid_zone.face_i = None;
        let input = app.result_input(&ResultInputKey::Pushover(SeismicDir::X));
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .record_input(ResultInputKey::Pushover(SeismicDir::X), input);
        assert!(app
            .compute_holding_capacity()
            .err()
            .expect("拒否される")
            .contains("一致しません"));
        assert!(app.core.model.elements[2].rigid_zone.face_i.is_some());
    }

    #[test]
    fn 入力識別欠落の復元結果は判定に使えない() {
        let mut app = ready();
        let mut bundle = app.core.scoped.results.take().unwrap();
        bundle.input_records.clear();
        let bytes = rmp_serde::to_vec(&bundle).unwrap();
        app.core.scoped.results = Some(rmp_serde::from_slice(&bytes).unwrap());
        let reason = app.compute_holding_capacity().err().expect("拒否される");
        assert!(reason.contains("入力識別情報がありません"));
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        let bytes = rmp_serde::to_vec(app.core.scoped.results.as_ref().unwrap()).unwrap();
        app.core.scoped.results = Some(rmp_serde::from_slice(&bytes).unwrap());
        assert!(app.compute_holding_capacity().is_ok());
    }

    #[test]
    fn 計算中編集の旧ジョブ結果は破棄される() {
        let mut app = ready();
        let old_qu = app.pushover_for(SeismicDir::X).unwrap().qu;
        app.start_pushover_job();
        assert!(app.core.scoped.job.is_some());
        app.core.model.materials[0].fy = Some(345.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app.poll_job() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(app
            .core
            .scoped
            .last_error
            .as_ref()
            .unwrap()
            .contains("旧入力の解析結果を破棄"));
        assert_eq!(app.pushover_for(SeismicDir::X).unwrap().qu, old_qu);
        assert!(app.compute_holding_capacity().is_err());
    }
    #[test]
    fn 略算_qudは固有値の参考周期があっても独立略算値を採用する() {
        let mut app = ready();
        app.core.model.stories[1].seismic_weight = Some(100_000.0);
        app.run_seismic(SeismicDir::X);
        app.run_pushover();
        let baseline = app.compute_holding_capacity().unwrap().0.stories[0].qud;
        assert!((baseline - 100_000.0).abs() < 1e-8);
        app.run_eigen(1);
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .modal
            .as_mut()
            .unwrap()
            .period[0] = 20.0;
        let with_modal = app.compute_holding_capacity().unwrap().0.stories[0].qud;
        assert_eq!(baseline, with_modal);
    }
    #[test]
    fn 必要耐力用地震力は未検証精算塔屋と指定_ci副剛床を拒否する() {
        let mut app = ready();
        app.core.analysis_cfg.ai_mode = AiMode::SemiPrecise;
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("未対応"));
        app.core.analysis_cfg.ai_mode = AiMode::Approx;
        app.core.model.stories[1].level_kind =
            sepika_core::model::StoryLevelKind::Penthouse { k: 1.0 };
        assert!(app
            .compute_holding_capacity()
            .err()
            .unwrap()
            .contains("未検証"));
        app.core.model.stories[1].level_kind = Default::default();
        app.core
            .model
            .constraints
            .push(sepika_core::model::Constraint::RigidDiaphragm {
                story: StoryId(1),
                master: NodeId(2),
                slaves: vec![],
                weight: Some(100.0),
                ci_override: Some(0.3),
            });
        let error = app.compute_holding_capacity().err().unwrap();
        assert!(
            error.contains("指定 Ci") && error.contains("未対応"),
            "{error}"
        );
    }

    #[test]
    fn 地下_qudは上部_c0一のせん断力と地下震度重量を加算する() {
        let mut app = ready();
        let basement_node = NodeId(app.core.model.nodes.len() as u32);
        let spring_id = ElemId(app.core.model.elements.len() as u32);
        let mut basement = app.core.model.stories[1].clone();
        basement.id = StoryId(1);
        basement.name = "B1".into();
        basement.elevation = 1000.0;
        basement.node_ids = vec![basement_node];
        basement.seismic_weight = Some(100_000.0);
        basement.level_kind = sepika_core::model::StoryLevelKind::Basement { depth_mm: 0.0 };
        app.core.model.stories[1].id = StoryId(2);
        app.core.model.stories[1].seismic_weight = Some(100_000.0);
        for node in &mut app.core.model.nodes {
            if node.story == Some(StoryId(1)) {
                node.story = Some(StoryId(2));
            }
        }
        for constraint in &mut app.core.model.constraints {
            if let sepika_core::model::Constraint::RigidDiaphragm { story, .. } = constraint {
                if *story == StoryId(1) {
                    *story = StoryId(2);
                }
            }
        }
        app.core.model.stories.insert(1, basement);
        let mut node = app.core.model.nodes[0].clone();
        node.id = basement_node;
        node.story = Some(StoryId(1));
        node.restraint = Dof6Mask(0b111110);
        app.core.model.nodes.push(node);
        let mut spring = app.core.model.elements[0].clone();
        spring.id = spring_id;
        spring.kind = sepika_core::model::ElementKind::NodalSpring;
        spring.nodes = [NodeId(0), basement_node].into_iter().collect();
        spring.section = None;
        spring.spring = Some([1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        app.core.model.elements.push(spring);
        app.run_seismic(SeismicDir::X);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let result = app.compute_holding_capacity().unwrap().0;
        assert!((result.stories[0].qud - 110_000.0).abs() < 1e-8);
        assert!((result.stories[1].qud - 100_000.0).abs() < 1e-8);
    }
}
