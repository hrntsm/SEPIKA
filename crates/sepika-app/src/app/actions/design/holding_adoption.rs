use crate::app::App;

impl App {
    pub(crate) fn holding_adoption_unavailable_reasons(&self) -> Vec<String> {
        [
            (
                "Ai/Qun外力分布",
                "保存加力条件と層外力は照合できますが、Qun分布への変更に必要な全体崩壊形・非崩壊部分の成立証拠はありません",
            ),
            (
                "全体・部分・局部崩壊形",
                "必要な崩壊形の網羅と各耐力の最小値の確認がありません。機構分類・通常収束・目標到達・任意終了・数値的特異だけでは成立しません",
            ),
            (
                "非崩壊階の別途Ds応力",
                "非崩壊階の識別と別途評価応力の証拠がありません。目的ごとの単一run/stepでは階別の別途評価を保持できません",
            ),
            (
                "壁せん断破壊時のβu",
                "対象壁と破壊状態の対応が未確認です。確定点の負担力・βuやせん断降伏履歴だけでは、必要な壁破壊時の応力状態を証明できません",
            ),
            (
                "Ds/Quの脆性部材除外整合",
                "除外対象・除外後のDs/Qu双方の応力と成立条件を対応付ける証拠がありません。部材群耐力入力は除外の証拠ではありません",
            ),
            (
                "局部崩壊時の鉛直支持",
                "局部崩壊の対象と崩壊後の鉛直荷重支持力の確認がありません。水平切断面の釣合いでは代替できません",
            ),
            (
                "未崩壊の確認済耐力Qu",
                "未崩壊の確認済耐力として利用する適用条件と全層の成立証拠がありません。Qu≥Qunだけでは採用できません",
            ),
            (
                "連層壁脚拘束のDs/Qu対応",
                "回転系破壊の連層壁への該当性と、Ds時の浮上り・沈み込み拘束及びQu時の拘束状態との対応が未確認です",
            ),
            (
                "直交梁押さえ効果",
                "対象への該当性と作用軸力・負担せん断力・部材種別及びDs採用条件の証拠がありません。立体モデルであることだけでは効果の有無・適切性を確認できません",
            ),
            (
                "ピロティ設計方針・単独柱変動軸力",
                "形式への該当性、層崩壊回避又は許容の設計方針、単独柱の変動軸力・外力分布・変形集中等の必要な検討が未確認です",
            ),
            (
                "ピロティの性能確認",
                "対象への該当性と設計方針に対応する性能確認の証拠がありません。単独柱未崩壊の機構やDsの割増しでも性能確認を省略できません",
            ),
        ]
        .into_iter()
        .map(|(condition, evidence)| format!("{condition}は採用保留。{evidence}"))
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_solver::nonlinear::pushover::{MechanismType, PushoverTermination};
    use sepika_solver::statics::analysis::SeismicDir;

    fn ready() -> App {
        let mut app = App::default();
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            crate::sample::portal_frame(),
        ));
        app.core.analysis_cfg.threads = 1;
        app.core.analysis_cfg.push_steps = 3;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        crate::app::tests::select_holding_points(&mut app);
        app.compute_unconfirmed_holding_capacity()
            .expect("既存数値候補は評価可能");
        app
    }

    #[test]
    fn public_entry_and_csv_report_all_missing_adoption_conditions_for_both_rank_modes() {
        let mut app = ready();
        for auto in [false, true] {
            app.core.design_rank_auto = auto;
            app.compute_unconfirmed_holding_capacity()
                .expect("既存数値候補は評価可能");
            assert!(app.core.scoped.holding_capacity_source.is_some());
            let reason = app.compute_holding_capacity().err().expect("採用条件不足");
            let csv = crate::summary::build_report_csv(&app);
            #[cfg(feature = "gui")]
            let gui = {
                let ctx = egui::Context::default();
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(4000.0, 10000.0),
                        )),
                        ..Default::default()
                    },
                    |ui| crate::design_view::design_table(ui, &mut app),
                );
                output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            for condition in [
                "Ai/Qun外力分布",
                "全体・部分・局部崩壊形",
                "非崩壊階の別途Ds応力",
                "壁せん断破壊時のβu",
                "Ds/Quの脆性部材除外整合",
                "局部崩壊時の鉛直支持",
                "未崩壊の確認済耐力Qu",
                "連層壁脚拘束のDs/Qu対応",
                "直交梁押さえ効果",
                "ピロティ設計方針・単独柱変動軸力",
                "ピロティの性能確認",
            ] {
                let expected = format!("{condition}は採用保留。");
                assert!(reason.contains(&expected), "{reason}");
                assert!(csv.contains(&expected), "{csv}");
                #[cfg(feature = "gui")]
                assert!(gui.contains(&expected), "{gui}");
            }
            assert!(app.core.scoped.holding_capacity_source.is_none());
            assert!(app.core.scoped.ds_beta_u_by_story.is_empty());
            assert!(app.core.scoped.ds_wall_details.is_empty());
        }
    }

    #[test]
    fn public_entry_keeps_run_mixing_rejection_before_adoption_check() {
        let mut app = ready();
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap()
            .ds_evaluation
            .as_mut()
            .unwrap()
            .run_id
            .push_str("other");
        let reason = app.compute_holding_capacity().err().expect("採用条件不足");
        assert!(reason.contains("confirmed_run_mismatch"), "{reason}");
        assert!(app.core.scoped.holding_capacity_source.is_none());
    }

    #[test]
    fn all_story_capacity_comparisons_passing_do_not_prove_noncollapse_adoption() {
        let mut app = ready();
        app.core.design_rank_auto = false;
        let run = app
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .pushover_x
            .as_mut()
            .unwrap();
        for record in run.confirmed_history.as_mut().unwrap() {
            for cut in &mut record.cuts {
                for force in &mut cut.forces {
                    force.force_n *= 1_000_000.0;
                }
                cut.external_n *= 1_000_000.0;
                cut.reference_n *= 1_000_000.0;
                cut.support_n *= 1_000_000.0;
                cut.tolerance_n *= 1_000_000.0;
            }
        }
        let candidate = app
            .compute_unconfirmed_holding_capacity()
            .expect("釣合い済み数値候補")
            .0;
        assert!(!candidate.stories.is_empty());
        assert!(candidate.stories.iter().all(|s| s.ok && s.qu >= s.qun));
        let reason = app.compute_holding_capacity().err().expect("採用条件不足");
        assert!(
            reason.contains("未崩壊の確認済耐力Quは採用保留"),
            "{reason}"
        );
        assert!(app.core.scoped.holding_capacity_source.is_none());
    }

    #[test]
    fn termination_classification_and_selection_words_do_not_prove_adoption() {
        let mut app = ready();
        for termination in [
            PushoverTermination::Unknown,
            PushoverTermination::TargetReached,
            PushoverTermination::ScheduleCompleted,
            PushoverTermination::LambdaCap { lambda: 1.0 },
            PushoverTermination::NonConvergence {
                phase: "変位制御".into(),
                load_factor: 1.0,
            },
            PushoverTermination::TangentSingular {
                phase: "変位制御".into(),
                load_factor: 1.0,
            },
        ] {
            for mechanism in [MechanismType::Overall, MechanismType::Partial] {
                let po = app
                    .core
                    .scoped
                    .results
                    .as_mut()
                    .unwrap()
                    .pushover_x
                    .as_mut()
                    .unwrap();
                po.termination = termination.clone();
                let point = po.ds_evaluation.as_ref().unwrap();
                po.hinges = if matches!(mechanism, MechanismType::Overall) {
                    app.core
                        .model
                        .elements
                        .iter()
                        .flat_map(|e| {
                            [0.0, 1.0].map(|pos| sepika_solver::nonlinear::pushover::HingeEvent {
                                step: point.step,
                                elem: e.id,
                                pos,
                                level: sepika_solver::nonlinear::pushover::HingeLevel::Yield,
                                ductility: 1.0,
                            })
                        })
                        .collect()
                } else {
                    vec![]
                };
                assert_eq!(
                    std::mem::discriminant(&po.mechanism_at(point, &app.core.model).unwrap()),
                    std::mem::discriminant(&mechanism),
                );
                for point in [&mut po.ds_evaluation, &mut po.capacity_evaluation]
                    .into_iter()
                    .flatten()
                {
                    point.selection_reason =
                        "全条件確認済み、全体崩壊、Qu>=Qun、壁脚拘束、性能確認済み".into();
                }
                let reason = app.compute_holding_capacity().err().expect("採用条件不足");
                assert!(
                    reason.contains("全体・部分・局部崩壊形は採用保留"),
                    "{reason}"
                );
                assert!(
                    reason.contains("未崩壊の確認済耐力Quは採用保留"),
                    "{reason}"
                );
            }
        }
    }
}
