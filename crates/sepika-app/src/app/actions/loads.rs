//! 荷重ケース自動同期・CMQ 表示ソース。
//!
//! `actions` からの構造分割。アルゴリズム変更は行わない。

use super::*;

impl App {
    /// CMQ 図（ビューア）が表示対象とする荷重ケース。
    ///
    /// 応力図の `nav.focus_result`（解析結果ケース。解析未実行時は空）とは異なり、
    /// CMQ 図は解析実行前でも使える診断図であるため、荷重タブ・ナビゲータで
    /// 選択中の荷重ケース（`nav.focus_load_case`）をそのまま参照する。未選択時は
    /// 静解析の対象決定（`resolved_analysis_target`）と同じ規約で先頭ケースへ
    /// フォールバックする。
    #[cfg(feature = "gui")]
    pub(crate) fn cmq_display_load_case(&self) -> Option<&sepika_core::model::LoadCase> {
        self.ui
            .scoped
            .nav
            .focus_load_case
            .and_then(|id| self.core.model.load_cases.iter().find(|lc| lc.id == id))
            .or_else(|| self.core.model.load_cases.first())
    }

    /// 重力系の標準荷重ケース（DL・LL(架構用)・LL(地震用)）へ自動計算値を同期する。
    ///
    /// - 「DL」（kind=Dead・[`DL_CASE_NAME`]）: スラブの `loads`（仕上げ等の
    ///   固定荷重）の分配と、躯体自重（柱梁・壁・ダンパー・フレーム外雑壁。
    ///   `sepika_load::self_weight::self_weight_case_content`）の合算。
    ///   **二次部材（小梁・間柱）が受け持つ床荷重とその自重は、逐次伝達
    ///   （`sepika_load::cascade`）が両端反力へ変えて主架構まで運ぶ**ため、
    ///   `self_weight_case_content` は二次部材の自重を返さない（二重計上の防止）。
    /// - 「LL(架構用)」（kind=Live）: スラブ用途（`SlabUsage`）から令別表第1 の
    ///   **骨組用**積載（LL）を分配（長期骨組解析用。用途未設定のスラブは寄与 0）。
    /// - 「LL(地震用)」（kind=LiveSeismic）: スラブ用途から令別表第1 の地震用積載を
    ///   分配。`gravity_cases_for_seismic_weight` が LiveSeismic を優先採用するため、
    ///   地震用重量にはこの（骨組用より小さい）地震用値が算入される（令85条1項）。
    ///
    /// 各ケースについて現在の自動計算値を求め、既存ケースの内容と一致するなら
    /// 何もしない（undo 履歴・stale フラグを汚さない）。差分があれば
    /// `SyncSlabLoadsToCase`（全置換、undo 対応）を発行する。
    /// 対応するケースがなく内容も空の場合は空ケースを作らない。
    ///
    /// DL に自重を含めるため、階の自動生成（地震用重量）では密度からの自重直接
    /// 算入を無効にして二重計上を防ぐ。
    ///
    /// 解析実行系（`sync_auto_load_cases_action` 経由）・`generate_stories_action`
    /// の入口で毎回呼ぶことを想定した冪等な同期アクション。
    pub fn sync_gravity_load_cases_action(&mut self) {
        let result = match sepika_job::auto_loads::compute_gravity_auto_load_cases(&self.core.model)
        {
            Ok(result) => result,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        for case in result.cases {
            self.sync_one_auto_case(case.name, case.kind, case.nodal, case.member);
        }
        self.sync_tip_load_cases_action();
    }

    /// 地震荷重の標準ケース（EX・EY、kind=Seismic）へ Ai 分布の水平力を同期する。
    ///
    /// 階（`model.stories`）が定義されている場合に、地震静的解析と同じ載荷
    /// （`build_seismic_load_case_from_model`。方向・Ai算定法・Z・地盤種別・C0 は
    /// `analysis_cfg`）を EX/EY ケースへ書き込む。これにより荷重組合せ
    /// （G+P±K など）が EX/EY を参照して解析できる。
    ///
    /// 設計用固有周期 T は `design_seismic_period` で決定する（`Analysis::prepare`
    /// を要しないモデル単独版 `build_seismic_load_case_from_model` を使うため、
    /// 本関数自体は剛性行列組立や固有値解析を一切行わない）。X・Y 双方向で T を
    /// 共有するため `design_seismic_period` の呼び出しは 1 回のみ。
    ///
    /// 再生成できない方向の旧 Auto 荷重と関連する静的解析結果を除去する。
    /// SemiPrecise で固有値解析が未実行の場合は `last_notice` に案内する。
    /// 冪等な同期アクション（`sync_gravity_load_cases_action` と同じ規約）。
    pub fn sync_seismic_load_cases_action(&mut self) {
        self.sync_prepared_model(false);
    }

    /// 入力・生成出力・地震設定の変更を検出する。表示名は重量入力から除く。
    pub(crate) fn compute_auto_load_sync_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        sepika_job::weight_preparation::weight_input_key(
            &self.core.model,
            self.core.analysis_cfg.mass_method,
        )
        .hash(&mut hasher);
        sepika_job::weight_preparation::weight_output_key(&self.core.model).hash(&mut hasher);
        sepika_job::weight_preparation::weights_are_current(
            &self.core.model,
            self.core.analysis_cfg.mass_method,
        )
        .hash(&mut hasher);
        let generated_cases: Vec<_> = self
            .core
            .model
            .load_cases
            .iter()
            .map(|case| {
                (
                    case.id,
                    &case.name,
                    case.kind,
                    case.nodal
                        .iter()
                        .filter(|load| load.source != sepika_core::model::LoadSource::Manual)
                        .collect::<Vec<_>>(),
                    case.member
                        .iter()
                        .filter(|load| load.source != sepika_core::model::LoadSource::Manual)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        bincode::serialize(&generated_cases)
            .expect("生成荷重の直列化")
            .hash(&mut hasher);
        std::mem::discriminant(&self.core.analysis_cfg.ai_mode).hash(&mut hasher);
        self.core.analysis_cfg.z.to_bits().hash(&mut hasher);
        (self.core.analysis_cfg.soil as u8).hash(&mut hasher);
        self.core.analysis_cfg.c0.to_bits().hash(&mut hasher);
        if matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise) {
            self.design_seismic_period()
                .ok()
                .map(f64::to_bits)
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    /// 現在入力から重量と自動荷重を一括同期する。無変更時は Undo を追加しない。
    pub fn sync_auto_load_cases_action(&mut self) {
        self.sync_prepared_model(false);
    }

    pub(super) fn sync_prepared_model(&mut self, initialize_stories: bool) {
        #[cfg(feature = "gui")]
        self.clear_generated_member_selection();
        let current = self.compute_auto_load_sync_hash();
        if self.core.scoped.auto_load_sync_hash == Some(current)
            && (!initialize_stories || !self.core.model.stories.is_empty())
        {
            return;
        }
        let period = if matches!(self.core.analysis_cfg.ai_mode, AiMode::SemiPrecise) {
            match self.design_seismic_period() {
                Ok(period) => Some(period),
                Err(error) => {
                    self.report_notice(error);
                    None
                }
            }
        } else {
            None
        };
        let mut prepared = self.core.model.clone();
        let result = sepika_job::prepare::prepare_model(
            &mut prepared,
            &self.core.analysis_cfg,
            period,
            initialize_stories,
        );
        #[cfg(feature = "gui")]
        if result.as_ref().is_ok_and(|report| report.nodes_renumbered) {
            self.handle_prepared_node_renumbering(true);
        }
        let failed: Vec<_> = prepared
            .load_cases
            .iter()
            .filter(|case| {
                case.kind == sepika_core::model::LoadCaseKind::Seismic
                    && matches!(case.name.as_str(), EX_CASE_NAME | EY_CASE_NAME)
                    && sepika_job::compute::missing_seismic_horizontal_load(case)
            })
            .map(|case| case.id)
            .collect();
        if !self.core.model.eq_ignoring_dofmap(&prepared) {
            if !self.core.scoped.undo.run(
                &mut self.core.model,
                Box::new(sepika_edit::ApplyPreparedModel { prepared }),
            ) {
                sepika_job::prepare::clear_standard_seismic_auto(&mut self.core.model);
                self.invalidate_missing_tip_seismic(&failed);
                self.report_error(format!(
                    "準備結果を採用できません: {}",
                    self.core.scoped.undo.last_error().unwrap_or("入力の不整合")
                ));
                return;
            }
            self.core.scoped.staleness.mark_edited();
        }
        self.invalidate_missing_tip_seismic(&failed);
        match result {
            Ok(report) => {
                self.core.scoped.generated_panels = report.panels;
                for notice in report.notices {
                    if initialize_stories && notice.contains("地震用重量を再生成できません")
                    {
                        self.report_error(format!("階の生成エラー: {notice}"));
                    }
                    self.report_notice(notice);
                }
                self.core.scoped.auto_load_sync_hash = Some(self.compute_auto_load_sync_hash());
            }
            Err(error) => {
                self.report_notice(
                    "EX/EY を再生成できないため旧 Auto 水平力を両方向とも除去しました。",
                );
                self.report_error(error.to_string());
            }
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn handle_prepared_node_renumbering(&mut self, renumbered: bool) {
        if renumbered {
            self.clear_geometry_selection();
            self.ui.scoped.boundary_node = None;
        }
    }

    #[cfg(test)]
    fn apply_failed_seismic_cases(
        &mut self,
        result: &sepika_job::auto_loads::AutoLoadComputeResult,
    ) {
        for case in &result.cases {
            if case.kind == sepika_core::model::LoadCaseKind::Seismic
                && case.nodal.is_empty()
                && case.member.is_empty()
            {
                self.sync_one_auto_case(case.name, case.kind, Vec::new(), Vec::new());
                let ids: Vec<_> = self
                    .core
                    .model
                    .load_cases
                    .iter()
                    .filter(|lc| lc.name == case.name && lc.kind == case.kind)
                    .map(|lc| lc.id)
                    .collect();
                self.invalidate_missing_tip_seismic(&ids);
                self.report_notice(format!("{} の Ai 地震力を再生成できないため、旧 Auto 水平力と単体・依存組合せの旧結果を除去しました。準備計算の条件を修正してください（手入力水平力だけでは解析できません）。", case.name));
            }
        }
    }

    fn sync_tip_load_cases_action(&mut self) {
        let cases = match sepika_job::auto_loads::compute_tip_loads(&self.core.model) {
            Ok(cases) => cases,
            Err(error) => {
                self.report_error(error.to_string());
                return;
            }
        };
        self.apply_tip_load_cases(cases);
    }

    fn apply_tip_load_cases(
        &mut self,
        cases: Vec<(LoadCaseId, Vec<sepika_core::model::MemberLoad>)>,
    ) {
        for (id, member) in cases {
            if self.core.model.load_cases[id.index()].tip_loads_match(&member) {
                continue;
            }
            self.core.scoped.undo.run(
                &mut self.core.model,
                Box::new(sepika_edit::SyncTipLoadsToCase { id, member }),
            );
            self.core.scoped.staleness.mark_edited();
        }
    }

    /// 名前付き荷重ケースを指定の `kind`・内容へ冪等に同期する
    /// （`sync_gravity_load_cases_action`／`sync_seismic_load_cases_action`
    /// の各ケース同期の共通処理）。
    ///
    /// 同期の対象は自動生成分（`LoadSource::Auto`）だけで、利用者が同じケースへ
    /// 手入力した荷重は残す。要否判定も自動生成分どうしの比較で行う
    /// （手入力を足しただけで同期が走ると、undo 履歴が無意味に伸びる）。
    fn sync_one_auto_case(
        &mut self,
        name: &str,
        kind: sepika_core::model::LoadCaseKind,
        nodal: Vec<sepika_core::model::NodalLoad>,
        member: Vec<sepika_core::model::MemberLoad>,
    ) {
        let existing = self.core.model.load_cases.iter().find(|lc| lc.name == name);
        let needs_create = existing.is_none() && !(nodal.is_empty() && member.is_empty());
        let needs_update = existing
            .map(|lc| lc.kind != kind || !lc.auto_loads_match(&nodal, &member))
            .unwrap_or(false);
        if !needs_create && !needs_update {
            return;
        }

        self.core.scoped.undo.run(
            &mut self.core.model,
            Box::new(sepika_edit::SyncSlabLoadsToCase {
                name: name.to_string(),
                kind,
                nodal,
                member,
            }),
        );
        self.core.scoped.staleness.mark_edited();
    }

    /// 組合せが参照する水平力欠損ケースの名前を返す。
    /// 空の地震・風ケースを含む組合せをそのまま解くと水平力の項が黙って 0 になり、
    /// 長期と同じ結果を短期の検定に用いてしまうため、実行前のガードに使う
    /// （`run_combination`/`run_static_all`）。いずれも準備計算が
    /// EX/EY・WX/WY へ内容を生成するため、空のまま残っていることが異常の合図になる。
    pub(crate) fn empty_lateral_case_in_combo(
        &self,
        combo: &sepika_core::model::LoadCombination,
    ) -> Option<String> {
        combo.terms.iter().find_map(|(id, _)| {
            self.core
                .model
                .load_cases
                .iter()
                .find(|lc| lc.id == *id)
                .filter(|lc| is_empty_lateral_case(lc))
                .map(|lc| lc.name.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_direction_failure_rejects_both_and_keeps_unrelated_results() {
        use sepika_core::model::{LoadCombination, LoadSource, NodalLoad};
        use sepika_job::auto_loads::{AutoLoadCaseContent, AutoLoadComputeResult};
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        let ex = app
            .core
            .model
            .load_cases
            .iter()
            .find(|lc| lc.name == EX_CASE_NAME)
            .unwrap()
            .id;
        let ey = app
            .core
            .model
            .load_cases
            .iter()
            .find(|lc| lc.name == EY_CASE_NAME)
            .unwrap()
            .id;
        app.core.model.load_cases[ey.index()]
            .nodal
            .push(NodalLoad::manual(
                sepika_core::ids::NodeId(2),
                [0.0, 100.0, 0.0, 0.0, 0.0, 0.0],
            ));
        app.core.model.combinations = vec![
            LoadCombination {
                name: "EY依存".into(),
                terms: vec![(ey, 1.0)],
            },
            LoadCombination {
                name: "EX依存".into(),
                terms: vec![(ex, 1.0)],
            },
            LoadCombination {
                name: "無関係".into(),
                terms: vec![(LoadCaseId(0), 1.0)],
            },
        ];
        app.run_static_all();
        assert!(app.core.scoped.last_error.is_none());
        app.select_displayed_result(StaticKey::Combo(1));
        let before = app.current_static().unwrap().disp.clone();
        let ex_loads = app.core.model.load_cases[ex.index()].nodal.clone();
        let result = AutoLoadComputeResult {
            cases: vec![
                AutoLoadCaseContent {
                    name: EX_CASE_NAME,
                    kind: sepika_core::model::LoadCaseKind::Seismic,
                    nodal: ex_loads.clone(),
                    member: Vec::new(),
                },
                AutoLoadCaseContent {
                    name: EY_CASE_NAME,
                    kind: sepika_core::model::LoadCaseKind::Seismic,
                    nodal: Vec::new(),
                    member: Vec::new(),
                },
            ],
            notices: Vec::new(),
        };
        let mut result = result;
        sepika_job::auto_loads::clear_failed_tip_seismic_cases(&app.core.model, &mut result);
        app.apply_failed_seismic_cases(&result);
        let bundle = app.core.scoped.results.as_ref().unwrap();
        assert!(bundle.seismic(SeismicDir::X).is_none());
        assert!(bundle.seismic(SeismicDir::Y).is_none());
        assert_eq!(
            bundle
                .combos
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["無関係"]
        );
        let _ = before;
        assert!(app.current_static().is_none());
        assert!(app.core.model.load_cases[ex.index()].nodal.is_empty());
        assert_eq!(app.core.model.load_cases[ey.index()].nodal.len(), 1);
        assert_eq!(
            app.core.model.load_cases[ey.index()].nodal[0].source,
            LoadSource::Manual
        );
        assert!(app
            .core
            .scoped
            .last_notice
            .as_deref()
            .unwrap()
            .contains("EY"));
    }
}
