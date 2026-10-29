use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::SystemTime;

#[cfg(any(feature = "gui", test))]
mod operations;

#[derive(Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub content: Value,
    pub result: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Review,
    Comparison,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Submission {
    pub receipt_id: String,
    pub run_index: Option<usize>,
    pub job_id: Option<String>,
    pub input: super::references::InputReference,
    pub explanation: String,
    pub unchecked: Vec<String>,
    pub references: Vec<super::references::ResultReference>,
    pub quantity_references: Vec<super::references::QuantityEvidenceReference>,
    pub calculation_state: String,
    pub verification: Verification,
    pub submitted: SystemTime,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub scope_available: bool,
    pub checked: Option<usize>,
    pub unchecked: Option<usize>,
    pub confirmed_ng: Option<usize>,
    pub observed_counts: Option<ObservedCounts>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedCounts {
    pub scope: String,
    pub count_unit: String,
    pub checked: usize,
    pub unchecked: usize,
    pub confirmed_ng: usize,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub mode: Mode,
    pub baseline: super::references::InputReference,
    pub started_input: super::references::InputReference,
    pub origin: super::StudyInput,
    pub from_main: bool,
    pub source_targets: Vec<super::references::TargetReference>,
    pub historical: bool,
    pub fixed: Vec<super::references::TargetReference>,
    pub purpose: String,
    pub note: String,
    pub proposal_name: String,
    pub conditions: String,
    pub frame: String,
    pub results: Vec<super::references::ResultReference>,
    pub display: super::references::DisplayConditions,
    pub quantity: Option<super::references::QuantityEvidenceReference>,
    pub prepared: SystemTime,
    pub started: Option<SystemTime>,
    pub interrupted: Option<(SystemTime, String)>,
    pub last_operation: SystemTime,
    pub last_operation_name: String,
    pub submissions: Vec<Submission>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StudyAi {
    pub id: String,
    pub receipts: Vec<Receipt>,
    pub tasks: Vec<Task>,
    pub jobs: Vec<AiJob>,
}
impl Default for StudyAi {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            receipts: Vec::new(),
            tasks: Vec::new(),
            jobs: Vec::new(),
        }
    }
}

impl StudyAi {
    /// 同じ要求ID・入力版の保持計算と所有する実行から同期し、状態変更の有無を返す。
    pub(super) fn sync_jobs(
        &mut self,
        runs: &[super::StudyRun],
        running: Option<&super::RunningStudy>,
    ) -> bool {
        let mut changed = false;
        for job in &mut self.jobs {
            let pending = matches!(job.state.as_str(), "running" | "awaiting_gui_confirmation");
            if !pending && job.state != "interrupted_no_result" {
                continue;
            }
            let found = runs.iter().position(|run| {
                run.ai_job.as_ref() == Some(&job.id) && run.input.input_reference() == job.input
            });
            let active = running.filter(|running| running.ai_job.as_ref() == Some(&job.id));
            let failure = active.and_then(|running| running.ai_error.as_ref());
            let state = if failure.is_some() {
                "failed"
            } else if let Some(index) = found {
                runs[index].calculation_state()
            } else if pending {
                match active {
                    Some(running)
                        if running.worker.core.scoped.pending_unset_analysis.is_some() =>
                    {
                        "awaiting_gui_confirmation"
                    }
                    Some(_) => "running",
                    None => "interrupted_no_result",
                }
            } else {
                "interrupted_no_result"
            };
            let error = failure
                .cloned()
                .or_else(|| found.and_then(|index| runs[index].error.clone()));
            if job.state != state || job.run_index != found || job.error != error {
                job.state = state.into();
                job.run_index = found;
                job.error = error;
                if let Some(task) = self.tasks.iter_mut().find(|task| task.id == job.request) {
                    task.last_operation = SystemTime::now();
                    task.last_operation_name = match state {
                        "awaiting_gui_confirmation" => "計算の続行確認を待機",
                        "calculated" => "計算結果を取込",
                        "failed" => "計算の失敗を確認",
                        "interrupted_no_result" => "計算の中断・結果なしを確認",
                        _ => "計算の実行を確認",
                    }
                    .into();
                }
                changed = true;
            }
        }
        changed
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AiJob {
    pub id: String,
    pub request: String,
    pub input: super::references::InputReference,
    pub state: String,
    pub error: Option<String>,
    pub run_index: Option<usize>,
}

pub struct Permission {
    pub connection: String,
    pub session: String,
    pub project: String,
    pub study: String,
    pub mode: Mode,
    pub history: bool,
}

#[derive(Default)]
pub struct AiConnection {
    #[cfg(windows)]
    pub listener: Option<sepika_ai_connection::listener::Listener>,
    pub pending: Option<String>,
    pub permission: Option<Permission>,
    pub selected_study: usize,
    pub comparison: bool,
    pub history: bool,
    pub purpose: String,
    pub review: bool,
    pub draft: Option<RequestDraft>,
    pub request_evidence: Option<super::references::EvidenceReference>,
}

pub struct RequestDraft {
    pub input: super::references::InputReference,
    pub fixed: Vec<super::references::TargetReference>,
    pub source_targets: Vec<super::references::TargetReference>,
    pub from_main: bool,
    pub results: Vec<super::references::ResultReference>,
    pub display: super::references::DisplayConditions,
    pub quantity: Option<super::references::QuantityEvidenceReference>,
    pub mode: Mode,
    pub purpose: String,
    pub proposal_name: String,
    pub conditions: String,
    pub frame: String,
}
