use arrow::array::{BooleanArray, UInt32Array};
use arrow::record_batch::RecordBatch;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{
    member_force_schema, modal_schema, nodal_disp_schema, read_all, read_time_history_range,
    time_history_schema, CaseId, ParquetWriter, ResultBatch, ResultEntry, ResultKind,
    ResultManifest, ResultProvenance, ResultQuery, ResultStore, ResultWriter,
};

/// finish 済みの結果を sync で公開し、選択 entry のファイルを照会する。
/// sync の失敗時は既存の公開 entry とファイルを保持する。
pub struct FsResultStore {
    dir: PathBuf,
    manifest_path: PathBuf,
    manifest: ResultManifest,
    pending: Arc<Mutex<Vec<ResultEntry>>>,
}

impl FsResultStore {
    /// ディレクトリを作成(なければ)し、既存の manifest.json があれば読み込んで開く。
    pub fn open(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let dir = dir.canonicalize()?;
        let manifest_path = dir.join("manifest.json");
        let manifest = if manifest_path.exists() {
            let data = std::fs::read_to_string(&manifest_path)?;
            serde_json::from_str(&data).map_err(std::io::Error::other)?
        } else {
            ResultManifest { entries: vec![] }
        };
        Ok(Self {
            dir,
            manifest_path,
            manifest,
            pending: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// 同一 case/kind の保留成果をまとめて公開する。失敗時は公開済み成果を保持する。
    pub fn sync(&mut self) -> std::io::Result<()> {
        let drained: Vec<ResultEntry> = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| std::io::Error::other("pending mutex poisoned"))?;
            pending.drain(..).collect()
        };
        if drained.is_empty() {
            return Ok(());
        }
        let mut new_manifest = self.manifest.clone();
        for entry in &drained {
            if let Some(existing) = new_manifest
                .entries
                .iter_mut()
                .find(|e| e.case == entry.case && e.kind == entry.kind)
            {
                *existing = entry.clone();
            } else {
                new_manifest.entries.push(entry.clone());
            }
        }
        let data = serde_json::to_string_pretty(&new_manifest).map_err(std::io::Error::other)?;
        let publish = (|| {
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(&self.dir)?;
            file.write_all(data.as_bytes())?;
            file.as_file().sync_all()?;
            file.persist(&self.manifest_path).map_err(|e| e.error)?;
            Ok::<(), std::io::Error>(())
        })();
        if let Err(e) = publish {
            if let Ok(mut pending) = self.pending.lock() {
                for entry in drained {
                    pending.push(entry);
                }
            }
            return Err(e);
        }
        self.manifest = new_manifest;
        Ok(())
    }

    /// finish 済みライタが積んだ保留エントリを、manifest へ吸収せずに破棄する。
    pub fn discard_pending(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}

/// `col_idx` 列(UInt32)の値が `ids` に含まれる行だけを残す素朴なフィルタ。
fn filter_by_u32_column(
    batches: Vec<RecordBatch>,
    col_idx: usize,
    ids: &[u32],
) -> std::io::Result<Vec<RecordBatch>> {
    let id_set: std::collections::HashSet<u32> = ids.iter().copied().collect();
    let mut result = Vec::with_capacity(batches.len());
    for batch in batches {
        let col = batch
            .column(col_idx)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .ok_or_else(|| std::io::Error::other("フィルタ対象列は UInt32 であるべき"))?;
        let num_rows = batch.num_rows();
        let keep: Vec<bool> = (0..num_rows)
            .map(|i| id_set.contains(&col.value(i)))
            .collect();
        let mask = BooleanArray::from(keep);
        let filtered =
            arrow::compute::filter_record_batch(&batch, &mask).map_err(std::io::Error::other)?;
        if filtered.num_rows() > 0 {
            result.push(filtered);
        }
    }
    Ok(result)
}

struct FsResultWriter {
    inner: ParquetWriter,
    rows: u64,
    case: CaseId,
    kind: ResultKind,
    path: String,
    pending: Arc<Mutex<Vec<ResultEntry>>>,
    provenance: Option<ResultProvenance>,
    temporary_path: tempfile::TempPath,
}

impl ResultWriter for FsResultWriter {
    fn write_rows(&mut self, batch: &RecordBatch) -> std::io::Result<()> {
        self.rows += batch.num_rows() as u64;
        self.inner.write_rows(batch)
    }

    fn finish(self: Box<Self>) -> std::io::Result<()> {
        let FsResultWriter {
            inner,
            rows,
            case,
            kind,
            path,
            pending,
            provenance,
            temporary_path,
        } = *self;
        Box::new(inner).finish()?;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)?
            .sync_all()?;
        let sha256 = Some(file_sha256(&path)?);
        temporary_path.keep().map_err(|e| e.error)?;
        pending
            .lock()
            .map_err(|_| std::io::Error::other("pending mutex poisoned"))?
            .push(ResultEntry {
                case,
                kind,
                rows,
                path,
                provenance,
                sha256,
            });
        Ok(())
    }
}

impl ResultStore for FsResultStore {
    fn writer(
        &mut self,
        case: CaseId,
        kind: ResultKind,
        provenance: Option<ResultProvenance>,
    ) -> std::io::Result<Box<dyn ResultWriter>> {
        let schema = match kind {
            ResultKind::NodalDisp => nodal_disp_schema(),
            ResultKind::MemberForce => member_force_schema(),
            ResultKind::Modal => modal_schema(),
            ResultKind::TimeHistory => time_history_schema(),
            ResultKind::Story => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Story kind はスキーマ未定義のため未対応(MCP からは呼ばれない前提)",
                ));
            }
        };
        let temporary = tempfile::Builder::new()
            .prefix(&format!("case{case}-{kind:?}-"))
            .suffix(".parquet")
            .tempfile_in(&self.dir)?;
        let (file, temporary_path) = temporary.into_parts();
        let path_str = temporary_path.to_string_lossy().into_owned();
        let inner = ParquetWriter::from_file(file, schema).map_err(std::io::Error::other)?;
        Ok(Box::new(FsResultWriter {
            inner,
            rows: 0,
            case,
            kind,
            path: path_str,
            provenance,
            temporary_path,
            pending: Arc::clone(&self.pending),
        }))
    }

    fn query(&self, q: &ResultQuery) -> std::io::Result<ResultBatch> {
        let entry = self
            .manifest
            .entries
            .iter()
            .find(|e| e.case == q.case && e.kind == q.kind)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "結果エントリがありません")
            })?;
        if std::path::Path::new(&entry.path).canonicalize()?.parent() != Some(self.dir.as_path()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "公開ファイルは結果ディレクトリ内である必要があります",
            ));
        }
        let expected = entry.sha256.as_ref().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "公開ファイルのhashがありません",
            )
        })?;
        if file_sha256(&entry.path)? != *expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "公開ファイルのhashが一致しません",
            ));
        }
        let path = entry.path.as_str();

        match q.kind {
            ResultKind::TimeHistory => {
                let node_ids: Option<Vec<u32>> = q
                    .node_filter
                    .as_ref()
                    .map(|ids| ids.iter().map(|n| n.0).collect());
                let batches = read_time_history_range(path, q.step_range, node_ids.as_deref())
                    .map_err(std::io::Error::other)?;
                let batch = arrow::compute::concat_batches(&time_history_schema(), &batches)
                    .map_err(std::io::Error::other)?;
                Ok(ResultBatch { batch })
            }
            ResultKind::NodalDisp => {
                let mut batches = read_all(path).map_err(std::io::Error::other)?;
                if let Some(ids) = &q.node_filter {
                    let ids: Vec<u32> = ids.iter().map(|n| n.0).collect();
                    batches = filter_by_u32_column(batches, 0, &ids)?;
                }
                let batch = arrow::compute::concat_batches(&nodal_disp_schema(), &batches)
                    .map_err(std::io::Error::other)?;
                Ok(ResultBatch { batch })
            }
            ResultKind::MemberForce => {
                let mut batches = read_all(path).map_err(std::io::Error::other)?;
                if let Some(ids) = &q.member_filter {
                    let ids: Vec<u32> = ids.iter().map(|e| e.0).collect();
                    batches = filter_by_u32_column(batches, 0, &ids)?;
                }
                let batch = arrow::compute::concat_batches(&member_force_schema(), &batches)
                    .map_err(std::io::Error::other)?;
                Ok(ResultBatch { batch })
            }
            ResultKind::Modal => {
                let batches = read_all(path).map_err(std::io::Error::other)?;
                let batch = arrow::compute::concat_batches(&modal_schema(), &batches)
                    .map_err(std::io::Error::other)?;
                Ok(ResultBatch { batch })
            }
            ResultKind::Story => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Story kind の query は未対応(MCP からは呼ばれない前提)",
            )),
        }
    }

    fn manifest(&self) -> &ResultManifest {
        &self.manifest
    }
}

/// ファイルの全 bytes を対象に SHA-256 を返す。
pub fn file_sha256(path: impl AsRef<std::path::Path>) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_publish_failure_preserves_previous_entry_and_file_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = FsResultStore::open(directory.path()).unwrap();
        for value in [3.0, 9.0] {
            let mut writer = store.writer(1, ResultKind::NodalDisp, None).unwrap();
            writer
                .write_rows(&super::super::nodal_disp_batch(&[1], &[[value; 6]]).unwrap())
                .unwrap();
            writer.finish().unwrap();
            if value == 3.0 {
                store.sync().unwrap();
            }
        }
        let old_entry = store.manifest().entries[0].clone();
        let old_bytes = std::fs::read(&old_entry.path).unwrap();
        let old_manifest = std::fs::read(&store.manifest_path).unwrap();
        let original_manifest_path = store.manifest_path.clone();
        store.manifest_path = directory.path().join("publish-blocker");
        std::fs::create_dir(&store.manifest_path).unwrap();
        assert!(store.sync().is_err());
        store.discard_pending();
        assert_eq!(store.manifest().entries[0].path, old_entry.path);
        assert_eq!(std::fs::read(original_manifest_path).unwrap(), old_manifest);
        assert_eq!(std::fs::read(&old_entry.path).unwrap(), old_bytes);
        let reopened = FsResultStore::open(directory.path()).unwrap();
        assert_eq!(reopened.manifest().entries[0].path, old_entry.path);
        assert_eq!(
            reopened
                .query(&ResultQuery {
                    case: 1,
                    kind: ResultKind::NodalDisp,
                    node_filter: None,
                    member_filter: None,
                    step_range: None
                })
                .unwrap()
                .batch
                .num_rows(),
            1
        );
    }
}
