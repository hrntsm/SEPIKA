//! ジョブ結果の永続化と結果取得の関数。

use super::*;

/// `summary`（JSON オブジェクト）に、結果ストアへ書き込んだ場所を示す
/// `"store": {"case": .., "kinds": [..]}` を追記する。
fn attach_store_info(
    summary: &mut serde_json::Value,
    case: u32,
    kinds: &[&str],
    store: &sepika_io::results::FsResultStore,
    source: &sepika_io::results::ResultProvenance,
) {
    if let serde_json::Value::Object(map) = summary {
        map.insert(
            "store".to_string(),
            serde_json::json!({ "case": case, "kinds": kinds, "persisted": true,
                "entries": store.manifest().entries.iter().filter(|entry| entry.provenance.as_ref().is_some_and(|saved| saved.run == source.run)).collect::<Vec<_>>() }),
        );
    }
}

/// 永続化対象の結果をストアへ保存し、サマリ JSON 文字列を返す。
/// 組合せ内力はストアへ保存せず、非永続のサマリに含める。
/// 書き込み失敗は `Err` で返す（呼び出し側はジョブを `Failed` へ遷移させる）。
/// `ServerState` のロックを保持したまま呼び出すこと。
pub fn persist_job_outcome(
    store: &mut sepika_io::results::FsResultStore,
    outcome: JobOutcome,
    source: &sepika_io::results::ResultProvenance,
) -> Result<String, String> {
    #[cfg(not(test))]
    let result = persist_job_outcome_inner(store, outcome, source);
    #[cfg(test)]
    let result = persist_job_outcome_inner(store, outcome, source, None);
    if result.is_err() {
        store.discard_pending();
    }
    result
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SavePoint {
    Start(u32, sepika_io::results::ResultKind),
    Write(u32, sepika_io::results::ResultKind),
    Finish(u32, sepika_io::results::ResultKind),
    Sync,
}

#[cfg(test)]
pub(crate) type SaveHook = std::sync::Arc<dyn Fn(SavePoint) -> std::io::Result<()> + Send + Sync>;

#[cfg(all(test, feature = "mcp"))]
pub(crate) fn persist_job_outcome_with_hook(
    store: &mut sepika_io::results::FsResultStore,
    outcome: JobOutcome,
    source: &sepika_io::results::ResultProvenance,
    hook: Option<&SaveHook>,
) -> Result<String, String> {
    let result = persist_job_outcome_inner(store, outcome, source, hook);
    if result.is_err() {
        store.discard_pending();
    }
    result
}
#[cfg(test)]
struct TestResultWriter {
    inner: Box<dyn sepika_io::results::ResultWriter>,
    case: u32,
    kind: sepika_io::results::ResultKind,
    hook: SaveHook,
}

#[cfg(test)]
impl sepika_io::results::ResultWriter for TestResultWriter {
    fn write_rows(&mut self, batch: &arrow::record_batch::RecordBatch) -> std::io::Result<()> {
        (self.hook)(SavePoint::Write(self.case, self.kind))?;
        self.inner.write_rows(batch)
    }

    fn finish(self: Box<Self>) -> std::io::Result<()> {
        (self.hook)(SavePoint::Finish(self.case, self.kind))?;
        self.inner.finish()
    }
}
fn persist_job_outcome_inner(
    store: &mut sepika_io::results::FsResultStore,
    outcome: JobOutcome,
    source: &sepika_io::results::ResultProvenance,
    #[cfg(test)] hook: Option<&SaveHook>,
) -> Result<String, String> {
    use sepika_io::results::{member_force_batch, modal_batch, nodal_disp_batch, ResultKind};

    /// バッチを 1 つ書き込んで finish する。
    fn write_one(
        store: &mut sepika_io::results::FsResultStore,
        case: u32,
        kind: ResultKind,
        batch: Result<arrow::record_batch::RecordBatch, arrow::error::ArrowError>,
        source: &sepika_io::results::ResultProvenance,
        #[cfg(test)] hook: Option<&SaveHook>,
    ) -> Result<(), String> {
        let batch = batch.map_err(|e| format!("結果バッチの生成に失敗: {e}"))?;
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(SavePoint::Start(case, kind))
                .map_err(|e| format!("結果ストアの書き込み開始に失敗: {e}"))?;
        }
        let w = store
            .writer(case, kind, Some(source.clone()))
            .map_err(|e| format!("結果ストアの書き込み開始に失敗: {e}"))?;
        #[cfg(not(test))]
        let mut w = w;
        #[cfg(test)]
        let mut w: Box<dyn sepika_io::results::ResultWriter> = match hook {
            Some(hook) => Box::new(TestResultWriter {
                inner: w,
                case,
                kind,
                hook: hook.clone(),
            }),
            None => w,
        };
        w.write_rows(&batch)
            .map_err(|e| format!("結果ストアへの書き込みに失敗: {e}"))?;

        w.finish()
            .map_err(|e| format!("結果ストアの書き込み完了に失敗: {e}"))?;
        Ok(())
    }

    let mut source = source.clone();
    match &outcome {
        JobOutcome::LinearStatic { case, .. } => {
            source.conditions["target"] = serde_json::json!({"case": case})
        }
        JobOutcome::DesignCheck { summary, .. } => {
            source.conditions["target"] = summary["load_target"].clone()
        }
        JobOutcome::Eigen { .. } => source.conditions["target"] = serde_json::json!({"case": 0}),
        _ => {}
    }
    match outcome {
        JobOutcome::LinearStatic {
            case,
            node_ids,
            disp,
            member_force_rows,
            mut summary,
        } => {
            let mut kinds: Vec<&str> = Vec::new();
            write_one(
                store,
                case,
                ResultKind::NodalDisp,
                nodal_disp_batch(&node_ids, &disp),
                &source,
                #[cfg(test)]
                hook,
            )?;
            kinds.push("NodalDisp");
            if !member_force_rows.is_empty() {
                write_one(
                    store,
                    case,
                    ResultKind::MemberForce,
                    member_force_batch(&member_force_rows),
                    &source,
                    #[cfg(test)]
                    hook,
                )?;
                kinds.push("MemberForce");
            }
            #[cfg(test)]
            if let Some(hook) = hook {
                hook(SavePoint::Sync)
                    .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            }
            store
                .sync()
                .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            attach_store_info(&mut summary, case, &kinds, store, &source);
            Ok(summary.to_string())
        }
        JobOutcome::DesignCheck {
            case,
            member_force_rows,
            mut summary,
        } => {
            let Some(case) = case else {
                let batch = member_force_batch(&member_force_rows)
                    .map_err(|e| format!("結果バッチの生成に失敗: {e}"))?;
                let (rows, truncated) = batch_to_json_rows(&batch, RESULT_GET_ROW_LIMIT);
                summary["member_forces"] = serde_json::json!({
                    "rows": rows, "truncated": truncated, "total_rows": member_force_rows.len(),
                    "units": {"n_q": "N", "m": "Nmm"},
                });
                summary["store"] = serde_json::json!({
                    "case": null, "kinds": [], "persisted": false,
                    "reason": "組合せ内力を単独荷重ケースの結果へ保存しません。result_getでは取得できません",
                    "retrieval": {
                        "tool": "analysis_status", "field": "status.Done.result_ref",
                        "encoding": "json_string", "force_field": "member_forces",
                        "load_target_field": "load_target",
                    },
                    "lifetime": "server_process",
                });
                return Ok(summary.to_string());
            };
            let mut kinds: Vec<&str> = Vec::new();
            if !member_force_rows.is_empty() {
                write_one(
                    store,
                    case,
                    ResultKind::MemberForce,
                    member_force_batch(&member_force_rows),
                    &source,
                    #[cfg(test)]
                    hook,
                )?;
                kinds.push("MemberForce");
            }
            #[cfg(test)]
            if let Some(hook) = hook {
                hook(SavePoint::Sync)
                    .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            }
            store
                .sync()
                .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            if !kinds.is_empty() {
                attach_store_info(&mut summary, case, &kinds, store, &source);
            }
            Ok(summary.to_string())
        }
        JobOutcome::Eigen {
            period,
            omega2,
            participation,
            effective_mass,
            mut summary,
        } => {
            let case = 0u32;
            write_one(
                store,
                case,
                ResultKind::Modal,
                modal_batch(&period, &omega2, &participation, &effective_mass),
                &source,
                #[cfg(test)]
                hook,
            )?;
            #[cfg(test)]
            if let Some(hook) = hook {
                hook(SavePoint::Sync)
                    .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            }
            store
                .sync()
                .map_err(|e| format!("結果マニフェストの永続化に失敗: {e}"))?;
            attach_store_info(&mut summary, case, &["Modal"], store, &source);
            Ok(summary.to_string())
        }
        JobOutcome::Pushover { summary }
        | JobOutcome::TimeHistory { summary }
        | JobOutcome::UltimateCheck { summary } => Ok(summary.to_string()),
    }
}

/// 結果 1 回あたりの `result_get` 応答に含める行数の上限。
const RESULT_GET_ROW_LIMIT: usize = 10_000;

/// 結果種別名（"NodalDisp" 等）を `ResultKind` へ変換する。
fn parse_result_kind(s: &str) -> Result<sepika_io::results::ResultKind, String> {
    use sepika_io::results::ResultKind;
    match s {
        "NodalDisp" => Ok(ResultKind::NodalDisp),
        "MemberForce" => Ok(ResultKind::MemberForce),
        "Modal" => Ok(ResultKind::Modal),
        "TimeHistory" => Ok(ResultKind::TimeHistory),
        other => Err(format!(
            "不明な結果種別: {other}（NodalDisp/MemberForce/Modal/TimeHistory のいずれか）"
        )),
    }
}

/// `RecordBatch` を JSON 行配列へ変換する。`row_limit` 超過分は切り詰める。
fn batch_to_json_rows(
    batch: &arrow::record_batch::RecordBatch,
    row_limit: usize,
) -> (Vec<serde_json::Value>, bool) {
    use arrow::array::{Float64Array, UInt32Array, UInt64Array};
    use arrow::datatypes::DataType;

    let schema = batch.schema();
    let total = batch.num_rows();
    let n = total.min(row_limit);
    let mut rows = Vec::with_capacity(n);
    for r in 0..n {
        let mut obj = serde_json::Map::new();
        for (c, field) in schema.fields().iter().enumerate() {
            let col = batch.column(c);
            let value = match field.data_type() {
                DataType::UInt32 => serde_json::json!(col
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .expect("UInt32 列のはず")
                    .value(r)),
                DataType::UInt64 => serde_json::json!(col
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .expect("UInt64 列のはず")
                    .value(r)),
                DataType::Float64 => serde_json::json!(col
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .expect("Float64 列のはず")
                    .value(r)),
                other => {
                    let _ = other;
                    serde_json::Value::Null
                }
            };
            obj.insert(field.name().clone(), value);
        }
        rows.push(serde_json::Value::Object(obj));
    }
    (rows, total > row_limit)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn complete_conditions(source: &sepika_io::results::ResultProvenance, case: u32) -> bool {
    let conditions = &source.conditions;
    let preparation = &conditions["preparation"];
    preparation["z"].as_f64().is_some()
        && preparation["c0"].as_f64().is_some()
        && matches!(preparation["soil"].as_str(), Some("I" | "II" | "III"))
        && matches!(
            preparation["ai_mode"].as_str(),
            Some("Approx" | "SemiPrecise")
        )
        && conditions
            .get("design_period_s")
            .is_some_and(|value| value.is_null() || value.as_f64().is_some())
        && match source.job_kind.as_str() {
            "LinearStatic" => conditions["target"]["case"].as_u64() == Some(u64::from(case)),
            "Eigen" => {
                case == 0
                    && conditions["target"]["case"] == 0
                    && conditions["n_modes"].as_u64().is_some()
            }
            "DesignCheck" => {
                conditions["target"]["source"] == "single_case"
                    && conditions["target"]["requested_case"].as_u64() == Some(u64::from(case))
                    && conditions["target"]["terms"] == serde_json::json!([[case, 1.0]])
            }
            _ => false,
        }
}

/// `result_get` ツールの中核ロジック。manifest に該当エントリがなければエラーを返す。
pub fn result_get_json(
    store: &dyn sepika_io::results::ResultStore,
    model: &Model,
    case: sepika_io::results::CaseId,
    kind_str: &str,
    node_ids: Option<Vec<u32>>,
    member_ids: Option<Vec<u32>>,
    step_range: Option<(u64, u64)>,
) -> Result<serde_json::Value, String> {
    let kind = parse_result_kind(kind_str)?;
    let entry = store.manifest().entries.iter().find(|e| e.case == case && e.kind == kind)
        .ok_or_else(|| format!("結果がありません（case={case}, kind={kind_str}）。analysis_run で解析を実行してください。"))?;
    let invalid = |reason: &str| {
        serde_json::json!({
            "case": case, "kind": kind_str, "status": "unavailable", "reason": reason,
            "entry": entry, "action": "analysis_run", "message": "解析を再実行してください。",
        })
    };
    let Some(source) = &entry.provenance else {
        return Ok(invalid("unknown_provenance"));
    };
    if source.run.is_empty()
        || source.job_kind.is_empty()
        || !valid_sha256(&source.input_sha256)
        || source.analysis_contract_version.is_none()
        || source.schema_version.is_none()
        || entry.sha256.as_ref().is_none_or(|hash| !valid_sha256(hash))
        || (source.job_kind == "DesignCheck" && source.design_contract_version.is_none())
    {
        return Ok(invalid("unknown_provenance"));
    }
    let supported = matches!(
        (source.job_kind.as_str(), kind),
        (
            "LinearStatic",
            sepika_io::results::ResultKind::NodalDisp | sepika_io::results::ResultKind::MemberForce
        ) | ("DesignCheck", sepika_io::results::ResultKind::MemberForce)
            | ("Eigen", sepika_io::results::ResultKind::Modal)
    );
    if !supported {
        return Ok(invalid("unsupported_contract"));
    }
    if !complete_conditions(source, case) {
        return Ok(invalid("unknown_provenance"));
    }
    let mut versions = vec![
        (
            source.analysis_contract_version.unwrap(),
            sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION,
        ),
        (
            source.schema_version.unwrap(),
            sepika_io::results::RESULT_SCHEMA_VERSION,
        ),
    ];
    if source.job_kind == "DesignCheck" {
        versions.push((
            source.design_contract_version.unwrap(),
            sepika_core::result_contract::DESIGN_CONTRACT_VERSION,
        ));
    }
    if versions.iter().any(|(saved, current)| saved > current) {
        return Ok(invalid("unsupported_contract"));
    }
    if versions.iter().any(|(saved, current)| saved < current) {
        return Ok(invalid("old_contract"));
    }
    match sepika_io::results::file_sha256(&entry.path) {
        Ok(hash) if Some(&hash) == entry.sha256.as_ref() => {}
        _ => return Ok(invalid("inconsistent_file")),
    }
    let current = sepika_io::results::model_input_sha256(model).map_err(|e| e.to_string())?
        == source.input_sha256;

    let node_filter = node_ids.map(|ids| ids.into_iter().map(sepika_core::ids::NodeId).collect());
    let member_filter =
        member_ids.map(|ids| ids.into_iter().map(sepika_core::ids::ElemId).collect());
    let query = sepika_io::results::ResultQuery {
        case,
        kind,
        node_filter,
        member_filter,
        step_range,
    };
    let result = match store.query(&query) {
        Ok(result) => result,
        Err(_) => return Ok(invalid("inconsistent_file")),
    };
    let (rows, truncated) = batch_to_json_rows(&result.batch, RESULT_GET_ROW_LIMIT);
    Ok(serde_json::json!({
        "case": case,
        "kind": kind_str,
        "rows": rows,
        "truncated": truncated,
        "status": if current { "current" } else { "stale" },
        "current_scope": "model_input_only",
        "entry": entry,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(model: &Model, kind: JobKind) -> sepika_io::results::ResultProvenance {
        JobParams::default()
            .result_provenance(model, kind, "job-0")
            .unwrap()
    }

    fn static_outcome(value: f64) -> JobOutcome {
        JobOutcome::LinearStatic {
            case: 1,
            node_ids: vec![7],
            disp: vec![[value; 6]],
            member_force_rows: vec![(9, 0.0, [value; 6])],
            summary: serde_json::json!({"case": 1}),
        }
    }

    fn test_dir(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("sepika589-{name}-{}", uuid::Uuid::new_v4()))
    }

    fn get(
        store: &sepika_io::results::FsResultStore,
        model: &Model,
        kind: &str,
    ) -> serde_json::Value {
        result_get_json(store, model, 1, kind, None, None, None).unwrap()
    }

    #[test]
    fn analysis22_missing_future24_and_current23_control_analysis_and_design15_rows() {
        assert_eq!(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION, 23);
        assert_eq!(sepika_core::result_contract::DESIGN_CONTRACT_VERSION, 15);
        let model = Model::default();
        for kind in [JobKind::LinearStatic, JobKind::DesignCheck] {
            let dir = test_dir("member-load-direction-contract");
            let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
            let outcome = if kind == JobKind::DesignCheck {
                JobOutcome::DesignCheck {
                    case: Some(1),
                    member_force_rows: vec![(9, 0.0, [12.5; 6])],
                    summary: serde_json::json!({"load_target": {"source": "single_case", "requested_case": 1, "terms": [[1, 1.0]]}}),
                }
            } else {
                static_outcome(12.5)
            };
            persist_job_outcome(&mut store, outcome, &source(&model, kind)).unwrap();
            let original = serde_json::to_value(store.manifest()).unwrap();
            let files: Vec<_> = store
                .manifest()
                .entries
                .iter()
                .map(|entry| (entry.path.clone(), std::fs::read(&entry.path).unwrap()))
                .collect();
            drop(store);
            for (version, reason) in [
                (Some(22), "old_contract"),
                (None, "unknown_provenance"),
                (Some(24), "unsupported_contract"),
                (Some(23), "current"),
            ] {
                let mut manifest = original.clone();
                for entry in manifest["entries"].as_array_mut().unwrap() {
                    if kind == JobKind::DesignCheck {
                        assert_eq!(entry["provenance"]["design_contract_version"], 15);
                    }
                    let provenance = entry["provenance"].as_object_mut().unwrap();
                    match version {
                        Some(version) => {
                            provenance.insert("analysis_contract_version".into(), version.into());
                        }
                        None => {
                            provenance.remove("analysis_contract_version");
                        }
                    }
                }
                let serialized = serde_json::to_vec(&manifest).unwrap();
                std::fs::write(dir.join("manifest.json"), &serialized).unwrap();
                let mut reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
                let result = get(&reopened, &model, "MemberForce");
                if reason == "current" {
                    assert_eq!(result["status"], "current");
                    assert_eq!(result["rows"][0]["n"], 12.5);
                } else {
                    assert_eq!(result["status"], "unavailable");
                    assert_eq!(result["reason"], reason);
                    assert!(result.get("rows").is_none());
                }
                reopened.sync().unwrap();
                assert_eq!(
                    std::fs::read(dir.join("manifest.json")).unwrap(),
                    serialized
                );
                for (path, bytes) in &files {
                    assert_eq!(&std::fs::read(path).unwrap(), bytes);
                }
                if let Some(evidence_dir) = std::env::var_os("SEPIKA687_EVIDENCE_DIR") {
                    let evidence_dir = std::path::PathBuf::from(evidence_dir);
                    std::fs::create_dir_all(&evidence_dir).unwrap();
                    let label = format!("{kind:?}-analysis{version:?}");
                    std::fs::write(
                        evidence_dir.join(format!("{label}-manifest.json")),
                        &serialized,
                    )
                    .unwrap();
                    std::fs::write(
                        evidence_dir.join(format!("{label}-response.json")),
                        serde_json::to_vec_pretty(&result).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }

    #[test]
    fn manifest_contract_versions_and_missing_metadata_reopen_without_restamping() {
        let model = Model::default();
        let dir = test_dir("versions");
        let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
        persist_job_outcome(
            &mut store,
            static_outcome(12.5),
            &source(&model, JobKind::LinearStatic),
        )
        .unwrap();
        let original = serde_json::to_value(store.manifest()).unwrap();
        let path = original["entries"][0]["path"].as_str().unwrap();
        let bytes = std::fs::read(path).unwrap();
        for (version, reason) in [
            (None, "unknown_provenance"),
            (Some(0), "old_contract"),
            (
                Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION),
                "current",
            ),
            (
                Some(sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION + 1),
                "unsupported_contract",
            ),
        ] {
            let mut manifest = original.clone();
            match version {
                Some(version) => {
                    manifest["entries"][0]["provenance"]["analysis_contract_version"] =
                        version.into()
                }
                None => {
                    manifest["entries"][0]
                        .as_object_mut()
                        .unwrap()
                        .remove("provenance");
                }
            }
            let serialized = serde_json::to_vec(&manifest).unwrap();
            std::fs::write(dir.join("manifest.json"), &serialized).unwrap();
            let mut reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
            let result = get(&reopened, &model, "NodalDisp");
            if reason == "current" {
                assert_eq!(result["status"], reason);
                assert_eq!(result["rows"][0]["ux"], 12.5);
            } else {
                assert_eq!(result["reason"], reason);
                assert!(result.get("rows").is_none());
            }
            reopened.sync().unwrap();
            assert_eq!(
                std::fs::read(dir.join("manifest.json")).unwrap(),
                serialized
            );
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }

    #[test]
    fn selected_entry_path_hash_conditions_and_model_input_control_retrieval() {
        let model = Model::default();
        let dir = test_dir("selected");
        let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
        let summary: serde_json::Value = serde_json::from_str(
            &persist_job_outcome(
                &mut store,
                static_outcome(2.0),
                &source(&model, JobKind::LinearStatic),
            )
            .unwrap(),
        )
        .unwrap();
        let first = get(&store, &model, "NodalDisp");
        assert_eq!(first["entry"], summary["store"]["entries"][0]);
        assert_eq!(
            first["entry"]["provenance"]["conditions"]["target"]["case"],
            1
        );
        let mut edited = model.clone();
        edited.slab_thickness = 321.0;
        let stale =
            result_get_json(&store, &edited, 1, "NodalDisp", Some(vec![7]), None, None).unwrap();
        assert_eq!(stale["status"], "stale");
        assert_eq!(stale["rows"][0]["node_id"], 7);
        let mut display = model.clone();
        display.axes.push(sepika_core::model::AxisGroup {
            name: "表示専用".into(),
            kind: sepika_core::model::AxisGroupKind::Other,
            axes: vec![],
        });
        assert_eq!(get(&store, &display, "NodalDisp")["status"], "current");
        persist_job_outcome(
            &mut store,
            static_outcome(8.0),
            &source(&edited, JobKind::LinearStatic),
        )
        .unwrap();
        let second = get(&store, &edited, "NodalDisp");
        assert_ne!(first["entry"]["path"], second["entry"]["path"]);
        assert_ne!(
            first["entry"]["provenance"]["run"],
            second["entry"]["provenance"]["run"]
        );
        std::fs::copy(
            second["entry"]["path"].as_str().unwrap(),
            dir.join("case1-NodalDisp.parquet"),
        )
        .unwrap();
        let mut manifest = serde_json::to_value(store.manifest()).unwrap();
        manifest["entries"][0]["path"] = first["entry"]["path"].clone();
        manifest["entries"][0]["sha256"] = first["entry"]["sha256"].clone();
        manifest["entries"][0]["provenance"] = first["entry"]["provenance"].clone();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
        let selected = get(&reopened, &edited, "NodalDisp");
        assert_eq!(selected["rows"][0]["ux"], 2.0);
        assert_eq!(selected["status"], "stale");
        assert_eq!(
            selected["entry"]["provenance"],
            first["entry"]["provenance"]
        );
        manifest["entries"][0]["sha256"] = second["entry"]["sha256"].clone();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
        let rejected = get(&reopened, &edited, "NodalDisp");
        assert_eq!(rejected["reason"], "inconsistent_file");
        assert!(rejected.get("rows").is_none());
        for malformed in [
            serde_json::json!({}),
            serde_json::json!("unknown"),
            serde_json::json!({"preparation": {}}),
        ] {
            manifest["entries"][0]["provenance"]["conditions"] = malformed;
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
            assert_eq!(
                get(&reopened, &edited, "NodalDisp")["reason"],
                "unknown_provenance"
            );
        }
        for (field, value, reason) in [
            ("run", serde_json::json!(""), "unknown_provenance"),
            (
                "input_sha256",
                serde_json::json!("X".repeat(64)),
                "unknown_provenance",
            ),
            (
                "job_kind",
                serde_json::json!("FutureJob"),
                "unsupported_contract",
            ),
        ] {
            let mut fixture = serde_json::to_value(store.manifest()).unwrap();
            fixture["entries"][0]["provenance"][field] = value;
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&fixture).unwrap(),
            )
            .unwrap();
            let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
            assert_eq!(get(&reopened, &edited, "NodalDisp")["reason"], reason);
        }
    }

    #[test]
    fn another_kind_and_model_in_same_directory_never_restamp_or_merge_old_runs() {
        let model = Model::default();
        let dir = test_dir("separate");
        let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
        persist_job_outcome(
            &mut store,
            static_outcome(2.0),
            &source(&model, JobKind::LinearStatic),
        )
        .unwrap();
        let old = get(&store, &model, "NodalDisp")["entry"].clone();
        drop(store);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).unwrap()).unwrap();
        manifest["entries"][0]["provenance"]["analysis_contract_version"] = 0.into();
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let mut edited = model.clone();
        edited.slab_thickness = 123.0;
        let mut reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
        let design = JobOutcome::DesignCheck {
            case: Some(1),
            member_force_rows: vec![(9, 0.0, [7.0; 6])],
            summary: serde_json::json!({"load_target": {"source": "single_case", "requested_case": 1, "terms": [[1, 1.0]]}}),
        };
        persist_job_outcome(
            &mut reopened,
            design,
            &source(&edited, JobKind::DesignCheck),
        )
        .unwrap();
        assert_eq!(
            get(&reopened, &edited, "NodalDisp")["reason"],
            "old_contract"
        );
        let force = get(&reopened, &edited, "MemberForce");
        assert_eq!(force["status"], "current");
        assert_eq!(force["rows"][0]["n"], 7.0);
        assert_ne!(
            force["entry"]["provenance"]["run"],
            old["provenance"]["run"]
        );
        assert_eq!(get(&reopened, &model, "MemberForce")["status"], "stale");
        let original: serde_json::Value = serde_json::to_value(reopened.manifest()).unwrap();
        for (field, version, reason) in [
            (
                "design_contract_version",
                serde_json::Value::Null,
                "unknown_provenance",
            ),
            ("design_contract_version", 0.into(), "old_contract"),
            (
                "design_contract_version",
                (sepika_core::result_contract::DESIGN_CONTRACT_VERSION + 1).into(),
                "unsupported_contract",
            ),
            ("schema_version", 0.into(), "old_contract"),
            ("schema_version", 2.into(), "unsupported_contract"),
        ] {
            let mut fixture = original.clone();
            fixture["entries"][1]["provenance"][field] = version;
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&fixture).unwrap(),
            )
            .unwrap();
            let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
            let result = get(&reopened, &edited, "MemberForce");
            assert_eq!(result["reason"], reason);
            assert!(result.get("rows").is_none());
        }
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn failed_write_finish_or_sync_keeps_previous_published_metadata_and_bytes() {
        use sepika_io::results::ResultKind;
        for point in [
            SavePoint::Start(1, ResultKind::NodalDisp),
            SavePoint::Write(1, ResultKind::NodalDisp),
            SavePoint::Finish(1, ResultKind::NodalDisp),
            SavePoint::Finish(1, ResultKind::MemberForce),
            SavePoint::Sync,
        ] {
            let model = Model::default();
            let dir = test_dir("failed");
            let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
            persist_job_outcome(
                &mut store,
                static_outcome(4.0),
                &source(&model, JobKind::LinearStatic),
            )
            .unwrap();
            let before = get(&store, &model, "NodalDisp");
            let bytes = std::fs::read(before["entry"]["path"].as_str().unwrap()).unwrap();
            let manifest = std::fs::read(dir.join("manifest.json")).unwrap();
            let hook: SaveHook = std::sync::Arc::new(move |visited| {
                if visited == point {
                    Err(std::io::Error::other("保存失敗注入"))
                } else {
                    Ok(())
                }
            });
            assert!(persist_job_outcome_with_hook(
                &mut store,
                static_outcome(9.0),
                &source(&model, JobKind::LinearStatic),
                Some(&hook)
            )
            .is_err());
            store.sync().unwrap();
            assert_eq!(get(&store, &model, "NodalDisp"), before);
            assert_eq!(
                std::fs::read(before["entry"]["path"].as_str().unwrap()).unwrap(),
                bytes
            );
            assert_eq!(std::fs::read(dir.join("manifest.json")).unwrap(), manifest);
            let reopened = sepika_io::results::FsResultStore::open(&dir).unwrap();
            assert_eq!(get(&reopened, &model, "NodalDisp"), before);
        }
    }

    #[test]
    fn load_state_combination_inline_forces_report_truncation_without_store_entries() {
        let dir =
            std::env::temp_dir().join(format!("sepika487-inline-limit-{}", std::process::id()));
        let mut store = sepika_io::results::FsResultStore::open(&dir).unwrap();
        let outcome = JobOutcome::DesignCheck {
            case: None,
            member_force_rows: (0..10_001)
                .map(|id| (id, 0.0, [100.0, 20.0, 40.0, 30.0, 50.0, 60.0]))
                .collect(),
            summary: serde_json::json!({"case": null, "load_target": {"source": "saved_combination", "combination_index": 0}}),
        };
        let summary: serde_json::Value = serde_json::from_str(
            &persist_job_outcome(
                &mut store,
                outcome,
                &JobParams::default()
                    .result_provenance(&Model::default(), JobKind::DesignCheck, "inline-test")
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            summary["member_forces"]["rows"].as_array().unwrap().len(),
            10_000
        );
        assert_eq!(summary["member_forces"]["total_rows"], 10_001);
        assert_eq!(summary["member_forces"]["truncated"], true);
        assert_eq!(summary["store"]["persisted"], false);
        assert!(store.manifest().entries.is_empty());
        assert!(sepika_io::results::FsResultStore::open(dir)
            .unwrap()
            .manifest()
            .entries
            .is_empty());
    }
}
