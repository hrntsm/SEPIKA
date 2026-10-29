//! 開いているGUIの許可済み検討だけへ要求を中継するstdioサーバ。
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Implementation, ServerInfo},
    tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt,
};
use sepika_ai_connection::{
    pipe,
    protocol::{read_frame, write_frame, Request, Response},
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use tokio::{io::BufReader, net::windows::named_pipe::NamedPipeClient, sync::Mutex};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GuiRequest {
    pub envelope: Value,
}

#[derive(Clone)]
pub struct GuiBridge {
    pipe: String,
    client: Arc<Mutex<Option<BufReader<NamedPipeClient>>>>,
}
impl GuiBridge {
    pub fn new(pipe: String) -> Result<Self, String> {
        if !pipe.starts_with(r"\\.\pipe\sepika-gui-")
            || pipe[r"\\.\pipe\sepika-gui-".len()..].contains(['\\', '/'])
        {
            return Err("GUIが表示したローカルSEPIKAパイプを指定してください".into());
        }
        Ok(Self {
            pipe,
            client: Arc::new(Mutex::new(None)),
        })
    }
}
#[tool_router]
impl GuiBridge {
    #[tool(
        description = "開いているSEPIKA GUIの許可した検討へ要求を中継する。envelopeはversion=1,id,operation,session,project,study,request,proposal,input,arguments。最初にconnect（id,version,operationのみ）、GUI承認後に再connectでsessionと対象版を取得しbegin_request。操作: study,proposals,fixed_targets,decisions,conditions,members,nodes,materials,load_cases,combinations,sections,create_comparison(name),assign_section(targets,section),calculate,job(id),receipt(id),changes,quantity,results(run_index,offset,limit),evidence(reference,table,offset,limit),submit(explanation,unchecked,references)。一覧はoffsetと1〜100のlimit。更新は期待入力版の完全一致が必要。応答喪失時は再承認後receiptで要求IDを照会し、失敗とも成功とも推定しない。確認のみ依頼で作成/編集/計算は禁止。本案更新/採用/警告承諾/ファイル操作は提供しない。"
    )]
    pub async fn gui_request(
        &self,
        Parameters(args): Parameters<GuiRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let request: Request = serde_json::from_value(args.envelope)
            .map_err(|e| ErrorData::invalid_params(e.to_string(), None))?;
        let mut client = self.client.lock().await;
        if client.is_none() {
            *client = Some(BufReader::new(pipe::connect(&self.pipe).map_err(|e| {
                ErrorData::internal_error(format!("GUIに接続できません: {e}"), None)
            })?));
        }
        let stream = client.as_mut().unwrap();
        let result = async {
            write_frame(stream.get_mut(), &request).await?;
            read_frame::<Response>(stream).await?.ok_or_else(|| {
                std::io::Error::other("GUIとの接続が終了しました。受理結果は不明です")
            })
        }
        .await;
        match result {
            Ok(response) => Ok(CallToolResult::success(vec![Content::json(response)?])),
            Err(error) => {
                *client = None;
                Err(ErrorData::internal_error(
                    format!("{error}。再接続は再承認が必要です。自動再送しません"),
                    None,
                ))
            }
        }
    }
}
#[tool_handler]
impl ServerHandler for GuiBridge {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.server_info = Implementation::new("sepika-gui-bridge", env!("CARGO_PKG_VERSION"));
        info.capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build();
        info.instructions=Some("開いているGUIが状態の唯一の所有者です。独立起動MCPと異なりOVIKAファイルを読み込みません。GUIで許可された検討の操作だけを中継します。".into());
        info
    }
}
pub async fn run(pipe: String) -> Result<(), Box<dyn std::error::Error>> {
    let service = GuiBridge::new(pipe)?
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
