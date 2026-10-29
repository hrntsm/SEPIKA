use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub id: String,
    pub operation: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub study: Option<String>,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub proposal: Option<String>,
    #[serde(default)]
    pub input: Option<Value>,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rejection {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: String,
    pub result: Option<Value>,
    pub error: Option<Rejection>,
}
impl Response {
    pub fn accepted(id: String, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }
    pub fn rejected(id: String, code: &str, message: &str) -> Self {
        Self {
            id,
            result: None,
            error: Some(Rejection {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

/// 改行を含む最大1MiBのUTF-8 JSON行だけを受け付け、途中のEOFを拒否します。
pub async fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> io::Result<Option<T>> {
    let mut bytes = Vec::new();
    let n = reader
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if n > MAX_FRAME_BYTES || bytes.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "JSON行は改行込み1MiB以下で指定してください",
        ));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "応答が1MiBを超えます。ページ範囲を狭めてください",
        ));
    }
    writer.write_all(&bytes).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn malformed_oversized_non_utf8_and_incomplete_frames_are_rejected() {
        for bytes in [
            b"{\n".to_vec(),
            vec![b'x'; MAX_FRAME_BYTES + 1],
            vec![0xff, b'\n'],
            b"{}".to_vec(),
        ] {
            assert!(read_frame::<Value>(&mut BufReader::new(bytes.as_slice()))
                .await
                .is_err());
        }
        assert!(read_frame::<Value>(&mut BufReader::new(b"".as_slice()))
            .await
            .unwrap()
            .is_none());
        assert!(read_frame::<Request>(&mut BufReader::new(
            b"{\"version\":1,\"id\":\"a\",\"operation\":\"connect\",\"unexpected\":true}\n"
                .as_slice()
        ))
        .await
        .is_err());
        assert_eq!(
            read_frame::<Value>(&mut BufReader::new("{\"目的\":\"確認\"}\n".as_bytes()))
                .await
                .unwrap()
                .unwrap()["目的"],
            "確認"
        );
    }
}
