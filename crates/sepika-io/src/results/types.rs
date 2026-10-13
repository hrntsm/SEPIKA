use arrow::record_batch::RecordBatch;
use sepika_core::ids::{ElemId, NodeId};

pub type CaseId = u32;

pub const RESULT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ResultProvenance {
    pub run: String,
    pub job_kind: String,
    pub input_sha256: String,
    pub conditions: serde_json::Value,
    pub analysis_contract_version: Option<u32>,
    pub design_contract_version: Option<u32>,
    pub schema_version: Option<u32>,
}

/// 表示専用値を除いた開始モデルの SHA-256 を返す。
pub fn model_input_sha256(model: &sepika_core::model::Model) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = bincode::serialize(&model.calculation_model()).map_err(std::io::Error::other)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResultKind {
    NodalDisp,
    MemberForce,
    Story,
    Modal,
    TimeHistory,
}

pub struct ResultQuery {
    pub case: CaseId,
    pub kind: ResultKind,
    pub node_filter: Option<Vec<NodeId>>,
    pub member_filter: Option<Vec<ElemId>>,
    pub step_range: Option<(u64, u64)>,
}

pub struct ResultBatch {
    pub batch: RecordBatch,
}

/// 結果の書き込み口。IO 失敗は `Err` で返す。
pub trait ResultWriter {
    fn write_rows(&mut self, batch: &RecordBatch) -> std::io::Result<()>;
    fn finish(self: Box<Self>) -> std::io::Result<()>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResultManifest {
    pub entries: Vec<ResultEntry>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ResultEntry {
    pub case: CaseId,
    pub kind: ResultKind,
    pub rows: u64,
    pub path: String,
    #[serde(default)]
    pub provenance: Option<ResultProvenance>,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// IO 失敗・未対応の kind・該当エントリなしは `Err` で返す。
pub trait ResultStore: Send {
    fn writer(
        &mut self,
        case: CaseId,
        kind: ResultKind,
        provenance: Option<ResultProvenance>,
    ) -> std::io::Result<Box<dyn ResultWriter>>;
    fn query(&self, q: &ResultQuery) -> std::io::Result<ResultBatch>;
    fn manifest(&self) -> &ResultManifest;
}
