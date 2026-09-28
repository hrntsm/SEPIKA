//! 線形静的解析（荷重ケース・組合せ・一括・地震静的）と結果の表示切替。
//!
//! `actions` からの構造分割。アルゴリズム変更は行わない。

use super::*;

impl App {
    /// 線形静的解析を実行し、結果を `self.core.scoped.results` に格納する。
    /// 指定した荷重ケースが存在しない場合はエラーメッセージをセット。
    ///
    /// 解析に先立って準備計算（`ensure_preparation`）を実行する。剛域を反映し、
    /// スラブ荷重・躯体自重を「DL」等の標準ケースへ、階が定義済みなら
    /// 地震荷重を「EX」「EY」ケースへ同期する（モデル・関連設定が前回同期時から
    /// 変わっていなければ荷重の再計算は丸ごとスキップする）。
    pub fn run_linear_static(&mut self, lc: LoadCaseId) {
        let ready = self.begin_analysis();
        self.invalidate_missing_tip_seismic(&[lc]);
        if !ready {
            return;
        }
        let res = squid_n_job::compute::compute_linear_static(self.core.model.clone(), lc)
            .map_err(|e| e.to_string());
        self.apply_static_case_result(StaticCaseKey::User(lc), res);
    }

    /// `StaticCaseKey` で区別される単一荷重ケースの静的解析結果の共通適用。
    /// bundle への格納・last_static 設定・staleness.mark_fresh・design_check の
    /// 実行はいずれも `run_linear_static`/`run_seismic` で同一のため、
    /// ここへ集約し同期版・バックグラウンドジョブ双方から使う。
    pub(super) fn apply_static_case_result(
        &mut self,
        key: StaticCaseKey,
        res: Result<squid_n_solver::statics::linear::StaticOnce, String>,
    ) {
        match res {
            Ok(res) => {
                let member_forces = res.member_forces.clone();
                let panel_moments = res.panel_moments.clone();
                let mut bundle = self.core.scoped.results.take().unwrap_or_default();
                bundle.statics.retain(|(id, _)| *id != key);
                bundle.statics.push((key, res));
                bundle.member_forces = member_forces;
                bundle.panel_moments = panel_moments;
                self.core.scoped.results = Some(bundle);
                self.core.scoped.last_static = Some(StaticKey::Case(key));
                self.ui.scoped.nav.focus_result = Some(StaticKey::Case(key));
                self.core.scoped.staleness.mark_fresh();
                self.run_design_check();
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 準備計算が自動生成する標準ケース（EX/EY）のうち、どれに当たるかを
    /// 荷重ケース名と種別から判別する。専用の結果キー
    /// （[`StaticCaseKey::Seismic`]）を持つケースであり、
    /// 剛心の精算・保有水平耐力の判定などがその結果を参照する。
    pub(crate) fn standard_lateral_case(&self, lc: LoadCaseId) -> Option<StaticCaseKey> {
        use squid_n_core::model::{LoadCaseKind, EX_CASE_NAME, EY_CASE_NAME};
        let case = self.core.model.load_cases.iter().find(|c| c.id == lc)?;
        match (case.name.as_str(), case.kind) {
            (EX_CASE_NAME, LoadCaseKind::Seismic) => Some(StaticCaseKey::Seismic(SeismicDir::X)),
            (EY_CASE_NAME, LoadCaseKind::Seismic) => Some(StaticCaseKey::Seismic(SeismicDir::Y)),
            _ => None,
        }
    }

    /// 荷重ケース 1 つの静的解析をバックグラウンドで実行する（解析パネルの
    /// 「荷重ケース」実行ボタンの入口）。
    ///
    /// 標準の水平力ケース（EX/EY）は、同期済み荷重ケースを解き、結果を方向別の
    /// `StaticCaseKey::Seismic` へ格納する（剛心の精算・保有水平耐力の
    /// 判定がこのキーを参照するため）。それ以外は線形静的解析として
    /// `StaticCaseKey::User` へ格納する。
    pub fn start_load_case_job(&mut self, lc: LoadCaseId) {
        match self.standard_lateral_case(lc) {
            Some(StaticCaseKey::Seismic(dir)) => self.start_seismic_job(dir),
            _ => self.start_linear_static_job(lc),
        }
    }

    /// 線形静的解析をバックグラウンドスレッドで実行する。
    /// UI スレッドをブロックしないよう重い解析を逃がす。
    /// 既にジョブが実行中の場合は何もしない（last_error に案内文を設定）。
    pub fn start_linear_static_job(&mut self, lc: LoadCaseId) {
        let ready = self.begin_analysis_job();
        if self.core.scoped.job.is_none() {
            self.invalidate_missing_tip_seismic(&[lc]);
        }
        if !ready {
            return;
        }
        let model = self.core.model.clone();
        self.spawn_analysis_job("線形静的解析", move || JobResult::StaticCase {
            key: StaticCaseKey::User(lc),
            res: Self::run_compute(|| {
                squid_n_job::compute::compute_linear_static(model, lc).map_err(|e| e.to_string())
            }),
        });
    }

    /// 静的解析の単体実行（解析パネル「▶ 単体実行」の入口）をバックグラウンドで
    /// 実行する。
    ///
    /// 荷重ケース単体・荷重組合せのどちらも同じ導線で実行する。求解の最小単位は
    /// 荷重ケースであり、荷重組合せは参照する荷重ケースを解いてからその線形和として
    /// 組み立てる（重ね合わせの原理。`Analysis::linear_combination`）。
    pub fn start_static_target_job(&mut self, target: StaticTarget) {
        match target {
            StaticTarget::Case(lc) => self.start_load_case_job(lc),
            StaticTarget::Combo(index) => self.start_combination_job(index),
        }
    }

    /// [`Self::start_static_target_job`] の同期版（解き終わるまで戻らない）。
    /// 振り分け先は同じで、標準の水平力ケース（EX/EY）は方向別の結果キーへ
    /// 格納する（`start_load_case_job` と同じ規約）。
    pub fn run_static_target(&mut self, target: StaticTarget) {
        match target {
            StaticTarget::Case(lc) => match self.standard_lateral_case(lc) {
                Some(StaticCaseKey::Seismic(dir)) => self.run_seismic(dir),
                _ => self.run_linear_static(lc),
            },
            StaticTarget::Combo(index) => self.run_combination(index),
        }
    }

    /// 荷重組合せ解析を実行し、結果を `bundle.combos` に格納する。
    /// 指定インデックスの荷重組合せが存在しない場合はエラーメッセージをセット。
    ///
    /// 求解は参照する荷重ケース単体で行い、組合せの結果はその線形和として
    /// 組み立てる（`Analysis::linear_combination`）。
    ///
    /// 解析に先立って準備計算（`ensure_preparation`）を実行し、スラブ荷重・躯体
    /// 自重を「DL」等の標準ケースへ、階が定義済みなら地震荷重を「EX」「EY」
    /// ケースへ同期する。
    /// 組合せが空の地震荷重ケースを参照している場合は解かずにエラーで案内する
    /// （地震項が黙って 0 になるのを防ぐ）。
    pub fn run_combination(&mut self, index: usize) {
        let ready = self.begin_analysis();
        if let Some(combo) = self.core.model.combinations.get(index) {
            let ids: Vec<_> = combo.terms.iter().map(|(id, _)| *id).collect();
            self.invalidate_missing_tip_seismic(&ids);
        }
        if !ready {
            return;
        }
        let Some(combo) = self.core.model.combinations.get(index).cloned() else {
            self.report_error(format!("荷重組合せ #{} が存在しません", index));
            return;
        };
        if let Some(name) = self.empty_lateral_case_in_combo(&combo) {
            self.report_error(format!(
                "荷重組合せ「{}」が参照する水平力の荷重ケース「{}」が空です（地震水平力がありません）。解析タブの「準備計算 実行」を行うか水平力を手入力してください。",
                combo.name, name
            ));
            return;
        }
        let name = combo.name.clone();
        let res = Self::compute_combination(self.core.model.clone(), combo);
        self.apply_combo_result(name, res);
    }

    /// 荷重組合せ解析の純粋計算部分。所有権を取り `&self` を使わないため、
    /// バックグラウンドジョブ（`start_combination_job`）からも呼び出せる。
    /// `Analysis::linear_combination` は参照する荷重ケースを単体で解いてから
    /// その結果を線形和する（荷重ベクトルを合成して解き直すことはしない）。
    fn compute_combination(
        model: squid_n_core::model::Model,
        combo: squid_n_core::model::LoadCombination,
    ) -> Result<squid_n_solver::statics::linear::StaticOnce, String> {
        let model = squid_n_load::wall_expand::expand_wall_elements_owned(model).0;
        match Analysis::prepare(&model) {
            Ok(analysis) => analysis
                .linear_combination(&combo)
                .map_err(|e| format!("荷重組合せ解析エラー: {:?}", e)),
            Err(e) => Err(format!("解析準備エラー: {:?}", e)),
        }
    }

    /// `compute_combination` の結果を適用する（bundle.combos への格納・
    /// last_static 設定・design_term 自動判定・design_check の実行）。
    /// `name` は組合せ名（`bundle.combos` 内の名前一致検索・再実行時の位置差替に
    /// 使う。`run_combination`/`start_combination_job` 双方から使う）。
    pub(super) fn apply_combo_result(
        &mut self,
        name: String,
        res: Result<squid_n_solver::statics::linear::StaticOnce, String>,
    ) {
        match res {
            Ok(res) => {
                let member_forces = res.member_forces.clone();
                let panel_moments = res.panel_moments.clone();
                let mut bundle = self.core.scoped.results.take().unwrap_or_default();
                let pos = match bundle.combos.iter().position(|(n, _)| *n == name) {
                    Some(pos) => {
                        bundle.combos[pos].1 = res;
                        pos
                    }
                    None => {
                        bundle.combos.push((name.clone(), res));
                        bundle.combos.len() - 1
                    }
                };
                bundle.member_forces = member_forces;
                bundle.panel_moments = panel_moments;
                self.core.scoped.results = Some(bundle);
                self.core.scoped.last_static = Some(StaticKey::Combo(pos));
                self.ui.scoped.nav.focus_result = Some(StaticKey::Combo(pos));
                self.core.scoped.staleness.mark_fresh();
                self.core.design_term = if squid_n_load::combo::is_short_term_combo(&name) {
                    LoadTerm::Short
                } else {
                    LoadTerm::Long
                };
                self.run_design_check();
            }
            Err(e) => self.report_error(e),
        }
    }

    /// 荷重組合せ解析をバックグラウンドスレッドで実行する。
    /// UI スレッドをブロックしないよう重い解析を逃がす。
    /// 既にジョブが実行中の場合は何もしない（last_error に案内文を設定）。
    pub fn start_combination_job(&mut self, index: usize) {
        let ready = self.begin_analysis_job();
        if self.core.scoped.job.is_none() {
            if let Some(combo) = self.core.model.combinations.get(index) {
                let ids: Vec<_> = combo.terms.iter().map(|(id, _)| *id).collect();
                self.invalidate_missing_tip_seismic(&ids);
            }
        }
        if !ready {
            return;
        }
        let Some(combo) = self.core.model.combinations.get(index).cloned() else {
            self.report_error(format!("荷重組合せ #{} が存在しません", index));
            return;
        };
        if let Some(name) = self.empty_lateral_case_in_combo(&combo) {
            self.report_error(format!(
                "荷重組合せ「{}」が参照する水平力の荷重ケース「{}」が空です（地震水平力がありません）。解析タブの「準備計算 実行」を行うか水平力を手入力してください。",
                combo.name, name
            ));
            return;
        }
        let model = self.core.model.clone();
        let name = combo.name.clone();
        self.spawn_analysis_job("荷重組合せ解析", move || JobResult::Combo {
            name,
            res: Self::run_compute(|| Self::compute_combination(model, combo)),
        });
    }

    /// 一括解析（全荷重ケース単体＋全荷重組合せ）を実行し、結果を `bundle` へ
    /// 格納する（解析パネル「▶▶ 一括解析」の入口）。
    ///
    /// 求解は荷重ケース単体のみで行い（`Analysis::prepare` を 1 回だけ行い、
    /// `analysis_cfg.threads` の並列設定に応じて荷重ケース単位に並列解析する）、
    /// 荷重組合せはその結果の線形和として組み立てる（重ね合わせの原理。
    /// `Analysis::linear_static_with_combinations`）。同じ荷重ケースを参照する組合せが
    /// 何件あっても、求解は荷重ケース数ぶんで済む。
    ///
    /// 個別の解析エラーは処理を止めず、件数と最初のエラー内容を `last_error` に
    /// まとめる（他の結果は失わない）。荷重ケースが 1 件もない場合、および 1 件も
    /// 解けなかった場合は既存の結果を変更せず、案内メッセージを `last_error` に
    /// 設定して return する。
    pub fn run_static_all(&mut self) {
        if !self.begin_analysis() {
            return;
        }
        if self.core.model.load_cases.is_empty() {
            self.report_error("荷重ケースがありません。荷重タブで作成してください。");
            return;
        }
        let (case_keys, combos, errors, excluded) = self.static_all_inputs();
        let computed = Self::compute_static_all(self.core.model.clone(), case_keys, combos);
        self.apply_static_all_result(computed, errors, excluded);
    }

    /// `run_static_all`/`start_static_all_job` 共通の事前準備。UI スレッド側の
    /// `self.core.model` を参照するため、バックグラウンドジョブでもここで行う。
    ///
    /// - 荷重ケース: 結果の格納キー（標準の水平力ケースは方向別の
    ///   `StaticCaseKey::Seismic`/`Wind`、それ以外は `User`）を対応付ける。
    ///   空の水平力ケース（未生成の EX/EY 等）は解析対象から外す（水平力が黙って
    ///   0 の結果を方向別キーへ格納すると、剛心の精算・保有水平耐力の判定が
    ///   それを正しい地震時応力として扱ってしまうため）。
    /// - 荷重組合せ: 空の水平力ケースを参照する組合せを除外する（地震・風の項が
    ///   黙って 0 になるのを防ぐ）。
    ///
    /// 戻り値は (荷重ケースと格納キーの対応, 解析対象の組合せ, エラー文一覧)。
    #[allow(clippy::type_complexity)]
    fn static_all_inputs(
        &self,
    ) -> (
        Vec<(LoadCaseId, StaticCaseKey)>,
        Vec<squid_n_core::model::LoadCombination>,
        Vec<String>,
        (Vec<StaticCaseKey>, Vec<String>),
    ) {
        let mut errors: Vec<String> = Vec::new();
        let mut excluded_ids = Vec::new();
        let case_keys = self
            .core.model
            .load_cases
            .iter()
            .filter(|lc| {
                if is_empty_lateral_case(lc) {
                    if self.tip_seismic_case(lc) {
                        excluded_ids.push(lc.id);
                    }
                    errors.push(format!(
                        "[{}] 水平力がありません。「準備計算 実行」を行うか水平力を手入力してください。",
                        lc.name
                    ));
                    return false;
                }
                true
            })
            .map(|lc| {
                let key = self
                    .standard_lateral_case(lc.id)
                    .unwrap_or(StaticCaseKey::User(lc.id));
                (lc.id, key)
            })
            .collect();
        let excluded_keys = excluded_ids
            .iter()
            .flat_map(|id| {
                [
                    self.standard_lateral_case(*id)
                        .unwrap_or(StaticCaseKey::User(*id)),
                    StaticCaseKey::User(*id),
                ]
            })
            .collect();
        let mut excluded_combos = Vec::new();
        let combos = self
            .core.model
            .combinations
            .iter()
            .filter(|combo| {
                if combo.terms.iter().any(|(id, _)| excluded_ids.contains(id)) {
                    excluded_combos.push(combo.name.clone());
                }
                match self.empty_lateral_case_in_combo(combo) {
                    Some(name) => {
                        errors.push(format!(
                            "[{}] 荷重ケース「{}」に水平力がありません。「準備計算 実行」を行うか水平力を手入力してください。",
                            combo.name, name
                        ));
                        false
                    }
                    None => true,
                }
            })
            .cloned()
            .collect();
        (case_keys, combos, errors, (excluded_keys, excluded_combos))
    }

    fn tip_seismic_case(&self, case: &squid_n_core::model::LoadCase) -> bool {
        self.standard_lateral_case(case.id).is_some()
            && self
                .core
                .model
                .slabs
                .iter()
                .any(|slab| slab.tip_loads.iter().any(|tip| tip.case == case.id))
    }

    fn invalidate_missing_tip_seismic(&mut self, ids: &[LoadCaseId]) {
        let affected: Vec<_> = self
            .core
            .model
            .load_cases
            .iter()
            .filter(|case| {
                ids.contains(&case.id) && self.tip_seismic_case(case) && is_empty_lateral_case(case)
            })
            .map(|case| case.id)
            .collect();
        if affected.is_empty() {
            return;
        }
        let keys = affected
            .iter()
            .flat_map(|id| {
                [
                    self.standard_lateral_case(*id).unwrap(),
                    StaticCaseKey::User(*id),
                ]
            })
            .collect();
        let combos = self
            .core
            .model
            .combinations
            .iter()
            .filter(|combo| combo.terms.iter().any(|(id, _)| affected.contains(id)))
            .map(|combo| combo.name.clone())
            .collect();
        self.remove_excluded_tip_results(&(keys, combos));
    }

    /// 一括解析の純粋計算部分。所有権を取り `&self` を使わないため、
    /// バックグラウンドジョブ（`start_static_all_job`）からも呼び出せる。
    ///
    /// `Analysis::prepare` を 1 回だけ行い、`case_keys` の荷重ケースを単体で解いて
    /// （荷重ケース単位の並列）、`combos` をその結果の線形和として組み立てる。
    /// `Analysis::prepare` 自体が失敗した場合は `Err` で全体を中断する
    /// （既存結果は `apply_static_all_result` 側で変更しない）。
    fn compute_static_all(
        model: squid_n_core::model::Model,
        case_keys: Vec<(LoadCaseId, StaticCaseKey)>,
        combos: Vec<squid_n_core::model::LoadCombination>,
    ) -> Result<StaticAllComputed, String> {
        let model = squid_n_load::wall_expand::expand_wall_elements_owned(model).0;
        let analysis = Analysis::prepare(&model).map_err(|e| format!("解析準備エラー: {:?}", e))?;
        let ids: Vec<LoadCaseId> = case_keys.iter().map(|(id, _)| *id).collect();
        let batch = analysis.linear_static_with_combinations(&ids, &combos);
        let case_name = |id: LoadCaseId| {
            model
                .load_cases
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("#{}", id.0))
        };
        let cases = case_keys
            .iter()
            .zip(batch.cases)
            .map(|((id, key), res)| {
                (
                    *key,
                    res.map_err(|e| format!("[{}] {:?}", case_name(*id), e)),
                )
            })
            .collect();
        let combos = combos
            .iter()
            .zip(batch.combos)
            .map(|(combo, res)| {
                (
                    combo.name.clone(),
                    res.map_err(|e| format!("[{}] {:?}", combo.name, e)),
                )
            })
            .collect();
        Ok(StaticAllComputed { cases, combos })
    }

    /// `compute_static_all` の結果を適用する。個別の解析エラーは処理を止めず、
    /// 件数と最初のエラー内容を `last_error` にまとめる（他の結果は失わない）。
    /// `pre_errors`（事前フィルタで除外された荷重ケース・組合せのエラー）と合わせて
    /// 1 件も解けなかった場合、および `Analysis::prepare` 自体が失敗した場合は
    /// 既存の結果を変更せず、案内メッセージを `last_error` に設定して return する。
    ///
    /// 表示対象（`last_static`）は最後に成功した荷重組合せ、組合せが 1 件もなければ
    /// 最後に成功した荷重ケースとする。
    pub(super) fn apply_static_all_result(
        &mut self,
        computed: Result<StaticAllComputed, String>,
        mut errors: Vec<String>,
        excluded: (Vec<StaticCaseKey>, Vec<String>),
    ) {
        self.remove_excluded_tip_results(&excluded);
        let items = match computed {
            Ok(items) => items,
            Err(e) => {
                self.report_error(e);
                return;
            }
        };

        let had_results = self.core.scoped.results.is_some();
        let mut bundle = self.core.scoped.results.take().unwrap_or_default();
        let mut last_case: Option<StaticCaseKey> = None;
        for (key, res) in items.cases {
            match res {
                Ok(res) => {
                    bundle.statics.retain(|(k, _)| *k != key);
                    bundle.statics.push((key, res));
                    last_case = Some(key);
                }
                Err(e) => errors.push(e),
            }
        }
        let mut last_combo: Option<(usize, String)> = None;
        for (name, res) in items.combos {
            match res {
                Ok(res) => {
                    let pos = match bundle.combos.iter().position(|(n, _)| *n == name) {
                        Some(pos) => {
                            bundle.combos[pos].1 = res;
                            pos
                        }
                        None => {
                            bundle.combos.push((name.clone(), res));
                            bundle.combos.len() - 1
                        }
                    };
                    last_combo = Some((pos, name));
                }
                Err(e) => errors.push(e),
            }
        }

        let display = match &last_combo {
            Some((pos, _)) => Some(StaticKey::Combo(*pos)),
            None => last_case.map(StaticKey::Case),
        };
        let Some(display) = display else {
            if had_results {
                self.core.scoped.results = Some(bundle);
            }
            self.report_error(format!(
                "一括解析エラー（{} 件すべて失敗）: {}",
                errors.len(),
                errors.first().cloned().unwrap_or_default()
            ));
            return;
        };
        let displayed = match display {
            StaticKey::Combo(pos) => bundle
                .combos
                .get(pos)
                .map(|(_, s)| (s.member_forces.clone(), s.panel_moments.clone())),
            StaticKey::Case(key) => bundle
                .statics
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, s)| (s.member_forces.clone(), s.panel_moments.clone())),
        };
        if let Some((member_forces, panel_moments)) = displayed {
            bundle.member_forces = member_forces;
            bundle.panel_moments = panel_moments;
        }
        self.core.scoped.results = Some(bundle);
        self.core.scoped.last_static = Some(display);
        self.ui.scoped.nav.focus_result = Some(display);
        self.core.scoped.staleness.mark_fresh();
        if let Some((_, name)) = &last_combo {
            self.core.design_term = if squid_n_load::combo::is_short_term_combo(name) {
                LoadTerm::Short
            } else {
                LoadTerm::Long
            };
        }
        self.run_design_check();

        if !errors.is_empty() {
            self.report_error(format!("{} 件でエラー: {}", errors.len(), errors[0]));
        }
    }

    fn remove_excluded_tip_results(&mut self, excluded: &(Vec<StaticCaseKey>, Vec<String>)) {
        if excluded.0.is_empty() && excluded.1.is_empty() {
            return;
        }
        let Some(bundle) = self.core.scoped.results.as_mut() else {
            return;
        };
        let old_names: Vec<_> = bundle.combos.iter().map(|(name, _)| name.clone()).collect();
        let remap = |key: Option<StaticKey>| match key {
            Some(StaticKey::Case(key)) if excluded.0.contains(&key) => None,
            Some(StaticKey::Combo(index)) => old_names.get(index).and_then(|name| {
                (!excluded.1.contains(name)).then(|| {
                    let removed_before = old_names[..index]
                        .iter()
                        .filter(|name| excluded.1.contains(name))
                        .count();
                    StaticKey::Combo(index - removed_before)
                })
            }),
            other => other,
        };
        let invalid_display = self.core.scoped.last_static.is_some()
            && remap(self.core.scoped.last_static).is_none()
            || self.ui.scoped.nav.focus_result.is_some()
                && remap(self.ui.scoped.nav.focus_result).is_none();
        self.core.scoped.last_static = remap(self.core.scoped.last_static);
        self.ui.scoped.nav.focus_result = remap(self.ui.scoped.nav.focus_result);
        bundle.statics.retain(|(key, _)| !excluded.0.contains(key));
        bundle.combos.retain(|(name, _)| !excluded.1.contains(name));
        if invalid_display {
            bundle.member_forces.clear();
            bundle.panel_moments.clear();
            bundle.member_checks.clear();
            bundle.joint_checks.clear();
            bundle.beam_checks.clear();
            bundle.slab_checks.clear();
        }
    }

    /// 一括解析をバックグラウンドスレッドで実行する。
    /// UI スレッドをブロックしないよう重い解析を逃がす。
    /// 既にジョブが実行中の場合は何もしない（last_error に案内文を設定）。
    pub fn start_static_all_job(&mut self) {
        if !self.begin_analysis_job() {
            return;
        }
        if self.core.model.load_cases.is_empty() {
            self.report_error("荷重ケースがありません。荷重タブで作成してください。");
            return;
        }
        let (case_keys, combos, pre_errors, excluded) = self.static_all_inputs();
        let model = self.core.model.clone();
        self.spawn_analysis_job("一括解析", move || JobResult::StaticAll {
            computed: Self::run_compute(|| Self::compute_static_all(model, case_keys, combos)),
            pre_errors,
            excluded,
        });
    }

    /// 表示対象の静的解析結果を解決する。優先順: ナビゲータ選択 → 最後に実行した結果。
    pub fn current_static(&self) -> Option<&squid_n_solver::statics::linear::StaticOnce> {
        let bundle = self.core.scoped.results.as_ref()?;
        let resolve = |key: StaticKey| -> Option<&squid_n_solver::statics::linear::StaticOnce> {
            match key {
                StaticKey::Case(case_key) => bundle
                    .statics
                    .iter()
                    .find(|(k, _)| *k == case_key)
                    .map(|(_, s)| s),
                StaticKey::Combo(idx) => bundle.combos.get(idx).map(|(_, s)| s),
            }
        };
        self.ui
            .scoped
            .nav
            .focus_result
            .and_then(resolve)
            .or_else(|| self.core.scoped.last_static.and_then(resolve))
    }

    /// 結果表示の対象を切り替える（ナビゲータ・結果タブの選択ドロップダウン共通）。
    ///
    /// 変位図・層指標だけでなく、応力図（N/Q/M）・断面検定が参照する
    /// [`ResultsBundle::member_forces`] も選択結果へ差し替える。荷重組合せを選んだ
    /// 場合は荷重継続性区分（長期/短期）を組合せ名から `is_short_term_combo` で
    /// 再判定し、断面検定を再実行する。これにより、選んだ荷重（組合せ）の長期/短期に
    /// 応じた断面算定結果が表示される。単一荷重ケースを選んだ場合は現在の区分を維持する
    /// （`apply_static_case_result` と同じ扱い）。該当キーの解析結果がない場合は何もしない。
    pub fn select_displayed_result(&mut self, key: StaticKey) {
        let resolved = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|bundle| match key {
                StaticKey::Case(case_key) => bundle
                    .statics
                    .iter()
                    .find(|(k, _)| *k == case_key)
                    .map(|(_, s)| (s.member_forces.clone(), s.panel_moments.clone(), None)),
                StaticKey::Combo(idx) => bundle.combos.get(idx).map(|(name, s)| {
                    (
                        s.member_forces.clone(),
                        s.panel_moments.clone(),
                        Some(name.clone()),
                    )
                }),
            });
        let Some((member_forces, panel_moments, combo_name)) = resolved else {
            return;
        };
        self.ui.scoped.nav.focus_result = Some(key);
        self.core.scoped.last_static = Some(key);
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.member_forces = member_forces;
            bundle.panel_moments = panel_moments;
        }
        if let Some(name) = combo_name {
            self.core.design_term = if squid_n_load::combo::is_short_term_combo(&name) {
                LoadTerm::Short
            } else {
                LoadTerm::Long
            };
        }
        self.run_design_check();
    }

    /// 同期済みの EX/EY ケースを線形静的解析し、結果を格納する。
    /// 結果は `StaticCaseKey::Seismic(dir)` に格納するため、X/Y 双方の地震静的結果
    /// および任意のユーザー荷重ケースの結果と衝突せず共存できる。
    /// 準備計算 `ensure_preparation` が Ai 荷重を同期する。
    pub fn run_seismic(&mut self, dir: SeismicDir) {
        let ready = self.begin_analysis();
        if let Some(id) = self.seismic_case_id(dir) {
            self.invalidate_missing_tip_seismic(&[id]);
        }
        if !ready {
            return;
        }
        let Some(lc) = self.seismic_case_id(dir) else {
            self.report_error("地震荷重ケースがありません");
            return;
        };
        if let Err(msg) = self.check_seismic_period_for_synced_case(lc) {
            self.report_error(msg);
            return;
        }
        let res = squid_n_job::compute::compute_linear_static(self.core.model.clone(), lc)
            .map_err(|e| e.to_string());
        self.apply_static_case_result(StaticCaseKey::Seismic(dir), res);
    }

    /// 地震静的解析をバックグラウンドスレッドで実行する。
    /// UI スレッドをブロックしないよう重い解析を逃がす。
    /// 既にジョブが実行中の場合は何もしない（last_error に案内文を設定）。
    pub fn start_seismic_job(&mut self, dir: SeismicDir) {
        let ready = self.begin_analysis_job();
        if self.core.scoped.job.is_none() {
            if let Some(id) = self.seismic_case_id(dir) {
                self.invalidate_missing_tip_seismic(&[id]);
            }
        }
        if !ready {
            return;
        }
        let Some(lc) = self.seismic_case_id(dir) else {
            self.report_error("地震荷重ケースがありません");
            return;
        };
        if let Err(msg) = self.check_seismic_period_for_synced_case(lc) {
            self.report_error(msg);
            return;
        }
        let model = self.core.model.clone();
        self.spawn_analysis_job("地震静的解析", move || JobResult::StaticCase {
            key: StaticCaseKey::Seismic(dir),
            res: Self::run_compute(|| {
                squid_n_job::compute::compute_linear_static(model, lc).map_err(|e| e.to_string())
            }),
        });
    }

    fn seismic_case_id(&self, dir: SeismicDir) -> Option<LoadCaseId> {
        let name = match dir {
            SeismicDir::X => squid_n_core::model::EX_CASE_NAME,
            SeismicDir::Y => squid_n_core::model::EY_CASE_NAME,
        };
        self.core
            .model
            .load_cases
            .iter()
            .find(|case| {
                case.name == name && case.kind == squid_n_core::model::LoadCaseKind::Seismic
            })
            .map(|case| case.id)
    }

    fn check_seismic_period_for_synced_case(&self, id: LoadCaseId) -> Result<(), String> {
        if self.core.analysis_cfg.ai_mode == AiMode::SemiPrecise
            && self.core.model.load_cases[id.index()]
                .nodal
                .iter()
                .any(|load| load.source == squid_n_core::model::LoadSource::Auto)
        {
            self.design_seismic_period()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_tip_results_clear_selected_cache_even_without_new_success() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        for name in ["除外対象", "保持対象"] {
            app.core
                .model
                .combinations
                .push(squid_n_core::model::LoadCombination {
                    name: name.into(),
                    terms: vec![(LoadCaseId(0), 1.0)],
                });
        }
        app.run_static_all();
        app.select_displayed_result(StaticKey::Combo(0));
        assert!(!app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .member_forces
            .is_empty());
        app.apply_static_all_result(
            Ok(StaticAllComputed::default()),
            vec!["水平力がありません".into()],
            (Vec::new(), vec!["除外対象".into()]),
        );
        let bundle = app.core.scoped.results.as_ref().unwrap();
        assert_eq!(bundle.combos.len(), 1);
        assert_eq!(bundle.combos[0].0, "保持対象");
        assert!(bundle.member_forces.is_empty());
        assert!(bundle.member_checks.is_empty());
        assert!(app.core.scoped.last_static.is_none());
        assert!(app.ui.scoped.nav.focus_result.is_none());
    }
}
