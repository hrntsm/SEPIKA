//! プロジェクト／ST-Bridge の入出力とモデル読み込み。
//!
//! `actions` からの構造分割。アルゴリズム変更は行わない。

use super::*;

/// 保存確認を出す解析結果サイズの閾値 [byte]。直列化した解析結果（時刻歴の
/// 詳細記録を含む）がこれを超える場合、詳細記録を保存に含めるかを確認する。
/// 読込側の上限（`sepika_io::ovika`、4 GiB）より十分小さい値とする。
pub(crate) const SAVE_RECORDING_CONFIRM_BYTES: usize = 512 * 1024 * 1024;

/// 保存前の確認ダイアログが必要か（解析結果の直列化サイズが閾値超過、かつ
/// 時刻歴の詳細記録を含む場合のみ）。
pub(crate) fn needs_recording_confirm(results_bytes: usize, has_recording: bool) -> bool {
    has_recording && results_bytes > SAVE_RECORDING_CONFIRM_BYTES
}

impl App {
    /// モデルを差し替え、選択・入力ドラフト・表示参照・再生位置を破棄する。配置・視点は保持する。
    /// 不正な構造参照・主架構節点数・二次部材の断面用途は差し替え前に拒否し、通知して `false` を返す。
    pub fn load_model(&mut self, model: sepika_core::model::Model) -> bool {
        if let Err(error) = model.validate_reference_ids() {
            self.report_error(format!("読込モデルの検証エラー: {error}"));
            return false;
        }
        if let Err(error) = model.validate_frame_node_counts() {
            self.report_error(format!("読込モデルの検証エラー: {error}"));
            return false;
        }
        if let Err(error) = model.validate_lip_channels() {
            self.report_error(format!("読込モデルの検証エラー: {error}"));
            return false;
        }
        if let Err(error) = model.validate_secondary_section_usage() {
            self.report_error(format!("読込モデルの検証エラー: {error}"));
            return false;
        }
        self.studies = study::Studies::default();
        self.core.model = model;
        self.core.scoped = ModelScoped::default();
        self.ui.scoped = UiModelScoped::default();
        #[cfg(feature = "gui")]
        {
            self.ui.view.viewport.reset_model_state();
            self.ui.view.workspace.secondary.reset_model_state();
        }
        self.sync_node_edit();
        true
    }

    /// 作成モード（梁・壁・スラブ）とその選択バッファをすべて解除する。
    ///
    /// 作成パレットが見えていない間にモードが残ると、3D クリックで意図しない部材が
    /// 無言で生成されるため、パレットの非表示に合わせて呼ぶ。モデル差し替え時の解除は
    /// これらのフィールドが [`UiModelScoped`] にあることで自動的に行われるので、
    /// `load_model` からは呼ばない。
    #[cfg(feature = "gui")]
    pub(crate) fn reset_draw_modes(&mut self) {
        self.ui.scoped.beam_draw_mode = false;
        self.ui.scoped.beam_draw_first = None;
        self.ui.scoped.wall_draw_mode = false;
        self.ui.scoped.slab_draw_mode = false;
        self.ui.scoped.beam_place_mode = false;
        self.ui.scoped.post_place_mode = false;
        self.ui.scoped.work_scope = None;
        self.ui.scoped.member_place_first = None;
    }

    /// プロジェクトを指定パスへ保存する。成功時は project_path と未保存フラグを更新。
    ///
    /// 準備計算・解析結果は最新の場合のみ同梱する。解析が stale でも、
    /// 現在の生成入力と一致する応力源不要の直接検定は単独で保存できる。
    /// 生成入力または検定契約が不一致の検定は保存対象から除外する。
    /// 解析結果を同梱しないときは、振動荷重ケースもモデルから外して書く
    /// （結果のない空ケースがナビに残らないようにする。メモリ上のケースは残す）。
    /// 準備が古く自動部材荷重の位置・区間が不正な場合は、保存候補から全自動荷重と成果を除く。
    /// 未準備の質量設定は保存候補へ反映し、旧設定の準備・解析成果を同梱しない。
    /// 手入力と編集中モデル・履歴は保持し、保存のための準備計算は実行しない。
    ///
    /// 解析結果の直列化サイズが `SAVE_RECORDING_CONFIRM_BYTES` を超え、かつ
    /// 時刻歴の詳細記録（`ThRecording`）を含む場合は、保存せずに確認保留
    /// （`pending_save_recording`）をセットして戻る。確認ダイアログの選択に
    /// 応じて [`Self::save_project_to_opts`] が `Some(true)`（含めて保存）／
    /// `Some(false)`（除外して保存）で再入する。
    pub fn save_project_to(&mut self, path: std::path::PathBuf) {
        self.save_project_to_opts(path, None);
    }

    /// [`Self::save_project_to`] の本体。`include_recording` が `None` のとき、
    /// サイズ超過なら確認保留をセットして保存を中断する。`Some(false)` なら
    /// 時刻歴の詳細記録を除外して保存する（メモリ上の記録は保持したまま）。
    pub fn save_project_to_opts(
        &mut self,
        path: std::path::PathBuf,
        include_recording: Option<bool>,
    ) {
        let active = self.studies.active;
        if active != study::Location::Main {
            if let Err(reason) = self.switch_study(study::Location::Main) {
                self.report_error(reason);
                return;
            }
            self.save_project_to_opts(path, include_recording);
            let error = self.core.scoped.last_error.clone();
            let _ = self.switch_study(active);
            if let Some(error) = error {
                self.report_error(error);
            }
            return;
        }
        self.core.scoped.last_error = None;
        if let Err(error) = self.core.model.validate_reference_ids() {
            self.report_error(format!("保存エラー: {error}"));
            return;
        }
        let discard_auto_loads = self.core.scoped.staleness.preparation_stale
            && self.core.model.load_cases.iter().any(|case| {
                case.member.iter().any(|load| {
                    load.source.is_auto()
                        && self.core.model.element(load.elem).is_some_and(|element| {
                            load.validate_extent(self.core.model.member_length(element))
                                .is_err()
                        })
                })
            });
        let pending_mass_method = self.core.model.mass_method != self.core.analysis_cfg.mass_method;
        let prep = self
            .core
            .scoped
            .preparation
            .as_ref()
            .filter(|_| !self.core.scoped.staleness.preparation_stale && !pending_mass_method)
            .map(rmp_serde::to_vec_named);
        let exclude_recording = include_recording == Some(false);
        let taken_recordings = if exclude_recording {
            self.core
                .scoped
                .results
                .as_mut()
                .map(|bundle| bundle.take_th_recordings())
        } else {
            None
        };
        let persist_results = !discard_auto_loads
            && !pending_mass_method
            && self.core.scoped.results.is_some()
            && (!self.core.scoped.staleness.results_stale
                || (self.require_design_input().is_ok()
                    && self
                        .core
                        .scoped
                        .results
                        .as_ref()
                        .is_some_and(|b| b.design_source.is_none())));
        let results = self
            .core
            .scoped
            .results
            .as_ref()
            .filter(|_| persist_results)
            .map(|bundle| SavedResults {
                bundle: {
                    let mut saved = if self.core.scoped.staleness.results_stale {
                        ResultsBundle {
                            reference_sources: bundle
                                .reference_sources
                                .iter()
                                .filter(|(key, _)| *key == ResultInputKey::Design)
                                .cloned()
                                .collect(),
                            reference_runs: bundle
                                .reference_runs
                                .iter()
                                .filter(|(key, _)| *key == ResultInputKey::Design)
                                .cloned()
                                .collect(),
                            member_checks: bundle.member_checks.clone(),
                            joint_checks: bundle.joint_checks.clone(),
                            wall_checks: bundle.wall_checks.clone(),
                            beam_checks: bundle.beam_checks.clone(),
                            slab_checks: bundle.slab_checks.clone(),
                            design_contract_version: bundle.design_contract_version,
                            design_source: bundle.design_source.clone(),
                            input_records: bundle
                                .input_records
                                .iter()
                                .filter(|(key, _)| *key == ResultInputKey::Design)
                                .cloned()
                                .collect(),
                            ..Default::default()
                        }
                    } else {
                        bundle.clone()
                    };
                    if self.require_design_input().is_err() {
                        saved.clear_design_results();
                    }
                    self.filter_vibration_results(&mut saved);
                    saved
                },
                last_static: self.core.scoped.last_static,
                pushover_view_dir: Some(self.core.scoped.pushover_view_dir),
                view_vibration_case: self.core.scoped.view_vibration_case,
                view_lumped_vibration_case: self.core.scoped.view_lumped_vibration_case,
                last_run: self.core.scoped.staleness.last_run,
            })
            .as_ref()
            .map(rmp_serde::to_vec_named);
        if let Some(taken) = taken_recordings {
            if let Some(bundle) = self.core.scoped.results.as_mut() {
                bundle.restore_th_recordings(taken);
            }
        }
        let analysis_settings = Some(rmp_serde::to_vec_named(&SavedAnalysisSettings {
            references: Some(self.saved_references()),
            cfg: self.core.analysis_cfg,
            wave_name: self.core.scoped.wave_library_selection.clone(),
            wave_sha256: self.core.scoped.wave_library_selected_sha256.clone(),
            lumped_wave_name: self.core.scoped.lumped_wave_library_selection.clone(),
            lumped_wave_sha256: self.core.scoped.lumped_wave_library_selected_sha256.clone(),
        }));
        let prep_bytes = self.encoded_or_notice(prep, "準備計算の結果");
        let results_bytes = self.encoded_or_notice(results, "解析結果");
        let analysis_settings_bytes = self.encoded_or_notice(analysis_settings, "解析タブの設定値");

        if include_recording.is_none() {
            let has_recording = self
                .core
                .scoped
                .results
                .as_ref()
                .filter(|_| persist_results)
                .is_some_and(|b| b.has_th_recording());
            let size = results_bytes.as_ref().map(|b| b.len()).unwrap_or(0);
            if needs_recording_confirm(size, has_recording) {
                self.core.scoped.pending_save_recording = Some((path, size as u64 / (1024 * 1024)));
                return;
            }
        }

        let study_bytes = match rmp_serde::to_vec_named(&self.saved_studies()) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                self.report_error(format!("検討の保存エラー: {error}"));
                return;
            }
        };
        let extras = sepika_io::ovika::OvikaExtras {
            studies: study_bytes.as_deref(),
            preparation: prep_bytes.as_deref(),
            results: results_bytes.as_deref(),
            analysis_settings: analysis_settings_bytes.as_deref(),
        };
        let taken_vibration_cases = if persist_results {
            None
        } else {
            Some((
                std::mem::take(&mut self.core.model.vibration_cases),
                std::mem::take(&mut self.core.model.lumped_vibration_cases),
            ))
        };
        let save_model = (discard_auto_loads || pending_mass_method).then(|| {
            let mut model = self.core.model.clone();
            model.mass_method = self.core.analysis_cfg.mass_method;
            if discard_auto_loads {
                for case in &mut model.load_cases {
                    case.nodal.retain(|load| !load.source.is_auto());
                    case.member.retain(|load| !load.source.is_auto());
                }
            }
            model
        });
        let save_result = sepika_io::ovika::save_ovika(
            &path,
            save_model.as_ref().unwrap_or(&self.core.model),
            extras,
        );
        if let Some((spatial, lumped)) = taken_vibration_cases {
            self.core.model.vibration_cases = spatial;
            self.core.model.lumped_vibration_cases = lumped;
        }
        match save_result {
            Ok(()) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                self.report_notice(format!("保存しました: {}", name));
                self.core.scoped.project_path = Some(path);
                self.core.scoped.staleness.unsaved_changes = false;
                self.studies.dirty = false;
                for study in &mut self.studies.items {
                    for proposal in &mut study.proposals {
                        if let Some(slot) = &mut proposal.state {
                            slot.core.scoped.staleness.unsaved_changes = false;
                        }
                    }
                }
            }
            Err(e) => self.report_error(format!("保存エラー: {}", e)),
        }
    }

    /// 時刻歴の詳細記録を除いた解析結果のみで保存する（確認ダイアログの
    /// 「除外して保存」）。
    pub fn save_project_without_recording(&mut self, path: std::path::PathBuf) {
        self.save_project_to_opts(path, Some(false));
    }

    /// 保存サイズの確認待ちを解消する。`None` は保存せず取り消す。
    #[cfg(feature = "gui")]
    pub(crate) fn resolve_pending_save_recording(&mut self, include: Option<bool>) {
        if let Some((path, _)) = self.core.scoped.pending_save_recording.take() {
            if let Some(include) = include {
                self.save_project_to_opts(path, Some(include));
            }
        }
    }

    /// 保存用の派生データの直列化結果を受け取り、失敗していれば注意を報告する。
    ///
    /// 準備計算・解析結果はいずれもモデルから再計算できる派生データなので、
    /// 直列化に失敗しても保存自体は続行する（モデルは保存し、注意を報告する）。
    fn encoded_or_notice(
        &mut self,
        encoded: Option<Result<Vec<u8>, rmp_serde::encode::Error>>,
        label: &str,
    ) -> Option<Vec<u8>> {
        match encoded {
            Some(Ok(bytes)) => Some(bytes),
            Some(Err(e)) => {
                self.report_notice(format!(
                    "{}を保存できませんでした（モデルは保存します）: {}",
                    label, e
                ));
                None
            }
            None => None,
        }
    }

    /// プロジェクトを指定パスから読み込む。成功時はモデルを差し替える。
    ///
    /// 準備計算の結果・解析結果・解析タブの設定値が同梱されていれば復元し、
    /// 実行済み扱いにする。ただし現在の標準パネル適用条件を満たさない旧成功成果は、
    /// 共通理由の未算定記録を確認して利用不可にする。準備計算・解析結果は、同梱がない・復号に失敗した場合は
    /// 未実行のままとし、解析実行時または「準備計算 実行」で再計算する。解析タブの
    /// 設定値は、同梱がない・復号に失敗した場合は現状（既定値）のまま変更しない
    /// （同梱に含まれない場合があるため）。ただし質量モデルの方式
    /// （`mass_method`）だけは、解析タブの設定値の同梱有無によらず、読み込んだ
    /// モデル側の値へ必ず同期する（単一情報源の原則。詳細は本体側のコメント参照）。
    pub fn open_project_from(&mut self, path: std::path::PathBuf) {
        self.core.scoped.last_error = None;
        match sepika_io::ovika::load_ovika(&path) {
            Ok(contents) => {
                if let Err(e) = contents.model.validate() {
                    self.report_error(format!("読込モデルの検証エラー: {:?}", e));
                    return;
                }
                let saved_studies = contents.studies;
                if !self.load_model(contents.model) {
                    return;
                }
                if let Some(saved) = self.decode_on_load::<SavedAnalysisSettings>(
                    contents.analysis_settings,
                    "解析タブの設定値",
                ) {
                    self.core.analysis_cfg = saved.cfg;
                    self.core.analysis_cfg.mass_method = self.core.model.mass_method;
                    self.restore_references(saved.references);
                    self.restore_wave_library_selection(saved.wave_name, saved.wave_sha256);
                    self.restore_lumped_wave_library_selection(
                        saved.lumped_wave_name,
                        saved.lumped_wave_sha256,
                    );
                }
                self.core.analysis_cfg.mass_method = self.core.model.mass_method;
                if let Some(prep) =
                    self.decode_on_load::<PreparationResult>(contents.preparation, "準備計算の結果")
                {
                    self.core.scoped.preparation = Some(prep);
                    self.core.scoped.staleness.preparation_stale = false;
                }
                if let Some(saved) =
                    self.decode_on_load::<SavedResults>(contents.results, "解析結果")
                {
                    let bundle = saved.bundle;
                    self.core.scoped.pushover_view_dir =
                        saved.pushover_view_dir.unwrap_or_else(|| {
                            match (bundle.pushover_x.is_some(), bundle.pushover_y.is_some()) {
                                (true, false) => SeismicDir::X,
                                (false, true) => SeismicDir::Y,
                                _ => self.core.analysis_cfg.push_dir,
                            }
                        });
                    self.core.scoped.results = Some(bundle);
                    self.core.scoped.last_static = saved.last_static;
                    self.core.scoped.staleness.last_run = saved.last_run;
                    self.validate_restored_results();
                    let mut bundle = self.core.scoped.results.as_ref().unwrap().clone();
                    self.core.scoped.view_vibration_case = saved.view_vibration_case;
                    self.core.scoped.view_lumped_vibration_case = saved.view_lumped_vibration_case;
                    let rejected = self.filter_vibration_results(&mut bundle);
                    if rejected {
                        self.report_notice("保存時刻歴の入力識別が欠落または不一致のため、該当結果を復元しませんでした。時刻歴解析を再実行してください。");
                    }
                    let only_historical_vibration = (!bundle.time_histories.is_empty()
                        || !bundle.lumped_results.is_empty())
                        && !bundle.input_records.iter().any(|(key, _)| {
                            *key != ResultInputKey::Design
                                && self.require_result_input(key.clone()).is_ok()
                        });
                    if !bundle.has_analysis_results() || only_historical_vibration {
                        self.core.scoped.staleness.results_stale = true;
                        self.core.scoped.staleness.last_run = None;
                    }
                    self.core.scoped.results = Some(bundle);
                    self.hydrate_saved_vibration_views(
                        saved.view_vibration_case,
                        saved.view_lumped_vibration_case,
                    );
                }
                if let Some(saved) =
                    self.decode_on_load::<study::SavedStudies>(saved_studies, "検討")
                {
                    if let Err(reason) = self.restore_studies(saved) {
                        self.report_error(format!("検討の読込エラー: {reason}"));
                    }
                }
                self.prune_orphan_vibration_cases();
                self.core.scoped.project_path = Some(path);
            }
            Err(e) => self.report_error(format!("読込エラー: {}", e)),
        }
    }

    /// 読み込んだ派生データを復号する。失敗しても読込自体は続行し、
    /// 未復元（再計算が必要）である旨を注意として報告する。
    fn decode_on_load<T: serde::de::DeserializeOwned>(
        &mut self,
        bytes: Option<Vec<u8>>,
        label: &str,
    ) -> Option<T> {
        let bytes = bytes?;
        match rmp_serde::from_slice::<T>(&bytes) {
            Ok(value) => Some(value),
            Err(e) => {
                self.report_notice(format!(
                    "保存された{}を読み込めませんでした（再実行が必要です）: {}",
                    label, e
                ));
                None
            }
        }
    }

    /// ST-Bridge（XML, サブセット）ファイルを読み込む。
    /// SEPIKA プロジェクト（.ovika）とは別物なので、`load_model` が
    /// `project_path` をクリアしたまま（未保存扱いで）進む。
    /// ファイルが荷重情報（`StbLoadCase`）を持たない場合は、標準荷重ケース
    /// （DL・LL(架構用)・LL(地震用)・EX・EY）と標準荷重組合せ（長期 DL+LL、
    /// 短期地震 DL+LL±EX・DL+LL±EY）を自動作成する（新規モデルと同じ出発点。
    /// DL の自重・スラブ荷重は解析実行前の同期アクションが自動計算する）。
    pub fn import_stbridge_from(&mut self, path: std::path::PathBuf) {
        self.core.scoped.last_error = None;
        let xml = match sepika_io::stbridge::read_stbridge_file(&path) {
            Ok(s) => s,
            Err(e) => {
                self.report_error(format!("ST-Bridge読込エラー: {}", e));
                return;
            }
        };
        match sepika_io::stbridge::import_stbridge_with_report(&xml) {
            Ok((mut model, report)) => {
                if let Err(e) = model.validate() {
                    self.report_error(format!("ST-Bridge読込モデルの検証エラー: {:?}", e));
                    return;
                }
                if model.load_cases.is_empty() {
                    model.load_cases = sepika_core::model::default_load_cases();
                    if model.combinations.is_empty() {
                        model.combinations = sepika_core::model::default_combinations();
                    }
                }
                if !self.load_model(model) {
                    return;
                }
                self.log_attribute_dispositions(&report);
                let mut lines: Vec<String> = report
                    .warnings
                    .iter()
                    .chain(report.notes.iter())
                    .cloned()
                    .collect();
                let dropped = report.dropped_attributes().count();
                if dropped > 0 {
                    lines.push(format!(
                        "取り込まなかった属性が {dropped} 種類あります（扱いの全量はログを参照）"
                    ));
                }
                if !lines.is_empty() {
                    self.report_error(format!(
                        "⚠️ ST-Bridge 取り込み時の注意:\n- {}",
                        lines.join("\n- ")
                    ));
                }
            }
            Err(e) => self.report_error(format!("ST-Bridge読込エラー: {}", e)),
        }
    }

    /// ファイルに現れた属性の扱いをすべてログへ出す。
    ///
    /// 取り込まなかった属性を先に、取り込んだ属性を後に並べる。無視リストを持たない
    /// 設計なので `guid` のように解析へ用いない属性も「未取り込み」として現れるが、
    /// 「どの属性がどう扱われたか」を利用者が漏れなく追えることを優先する。
    fn log_attribute_dispositions(&mut self, report: &sepika_io::stbridge::ImportReport) {
        if report.attributes.is_empty() {
            return;
        }
        let dropped: Vec<&sepika_io::stbridge::AttrDisposition> =
            report.dropped_attributes().collect();
        if dropped.is_empty() {
            self.core.log.push(
                crate::app::LogLevel::Info,
                "ST-Bridge 取り込み: ファイルの属性はすべて取り込みました",
            );
        } else {
            self.core.log.push(
                crate::app::LogLevel::Notice,
                format!(
                    "ST-Bridge 取り込み: 取り込まなかった属性 {} 種類\n  {}",
                    dropped.len(),
                    dropped
                        .iter()
                        .map(|a| a.log_line())
                        .collect::<Vec<_>>()
                        .join("\n  ")
                ),
            );
        }
        let kept: Vec<String> = report
            .attributes
            .iter()
            .filter(|a| !a.is_dropped())
            .map(|a| a.log_line())
            .collect();
        if !kept.is_empty() {
            self.core.log.push(
                crate::app::LogLevel::Info,
                format!(
                    "ST-Bridge 取り込み: 取り込んだ属性 {} 種類\n  {}",
                    kept.len(),
                    kept.join("\n  ")
                ),
            );
        }
    }

    /// モデルを標準 ST-Bridge 2.0.2（XML, 幾何サブセット）として指定パスへ書き出す。
    pub fn export_stbridge_to(&mut self, path: std::path::PathBuf) {
        self.core.scoped.last_error = None;
        match sepika_io::stbridge::export_stbridge_with_report(&self.core.model) {
            Ok((xml, report)) => {
                if let Err(e) = std::fs::write(&path, xml) {
                    self.report_error(format!("ST-Bridge書出エラー: {}", e));
                    return;
                }
                for warning in report.warnings {
                    self.report_notice(format!("ST-Bridge書出: {warning}"));
                }
                let dropped = self
                    .core
                    .model
                    .axes
                    .iter()
                    .filter(|g| g.kind == sepika_core::model::AxisGroupKind::Other)
                    .count();
                if dropped > 0 {
                    self.report_notice(format!(
                        "通り芯のうち平行芯でないグループ {dropped} 件は ST-Bridge へ書き出せないため除きました"
                    ));
                }
            }
            Err(e) => self.report_error(format!("ST-Bridge書出エラー: {}", e)),
        }
    }
}
