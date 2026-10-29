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
    pub input: super::references::InputReference,
    pub explanation: String,
    pub unchecked: Vec<String>,
    pub references: Vec<super::references::ResultReference>,
    pub quantity_references: Vec<super::references::QuantityEvidenceReference>,
    pub calculation_state: String,
    pub verification: Verification,
    pub submitted: SystemTime,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Verification {
    pub scope_available: bool,
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
    pub fixed: Vec<super::references::TargetReference>,
    pub started: SystemTime,
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

#[derive(Clone, Serialize, Deserialize)]
pub struct AiJob {
    pub id: String,
    pub request: String,
    pub input: super::references::InputReference,
    pub state: String,
    pub run_index: Option<usize>,
    pub previous_runs: usize,
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
}
