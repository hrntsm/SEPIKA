//! MCP サーバ実装（rmcp によるツールルータ）。

use super::*;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, Implementation, ServerInfo};
use rmcp::transport::stdio;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler, ServiceExt};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct SepikaServer {
    state: Arc<Mutex<ServerState>>,
    jobs: Arc<std::sync::Mutex<JobRegistry>>,
    #[cfg(test)]
    before_save: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    before_compute: Option<Arc<dyn Fn() + Send + Sync>>,
    #[cfg(test)]
    save_hook: Option<super::persist::SaveHook>,
    #[cfg(test)]
    after_save: Option<Arc<dyn Fn() + Send + Sync>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl SepikaServer {
    pub fn new(state: ServerState) -> Self {
        let jobs = state.jobs.clone();
        Self {
            state: Arc::new(Mutex::new(state)),
            jobs,
            #[cfg(test)]
            before_save: None,
            #[cfg(test)]
            before_compute: None,
            #[cfg(test)]
            save_hook: None,
            #[cfg(test)]
            after_save: None,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl SepikaServer {
    #[tool(description = "節点・部材・断面・壁版・床板・床領域を検索する")]
    pub async fn model_query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let st = self.state.lock().await;
        let items = super::query_model(&st.model, &args.kind, args.filter.as_deref());
        let result = QueryResult { items };
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    #[tool(
        description = "モデルを編集する（EditCommand 経由。Undo 可能。command キーで種別を指定）"
    )]
    pub async fn model_edit(
        &self,
        Parameters(args): Parameters<EditArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut st = self.state.lock().await;
        let result = super::apply_edit(&mut st, &args.body)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    #[tool(description = "数量積算（コンクリート体積・型枠面積・鉄筋/鉄骨重量の概算）を集計する")]
    pub async fn quantity_takeoff(
        &self,
        Parameters(args): Parameters<QuantityTakeoffArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let st = self.state.lock().await;
        let result = super::quantity_takeoff_json(&st.model, args.group_by.as_deref());
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    #[tool(description = "解析を非同期で実行する")]
    pub async fn analysis_run(
        &self,
        Parameters(args): Parameters<AnalysisRunArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let params = args
            .to_job_params()
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let kind = args.kind;

        let (id, model, mut source) = {
            let st = self.state.lock().await;
            let model = st.model.clone();
            let source = params
                .result_provenance(&model, kind, "")
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let id = {
                let mut jobs = st.jobs.lock().unwrap();
                let id = jobs.register(kind);
                jobs.update(&id, JobStatus::Running { progress: None });
                id
            };
            (id, model, source)
        };
        source.run.push_str(&id);
        let state = self.state.clone();
        let jobs = self.jobs.clone();
        let job_id = id.clone();
        #[cfg(test)]
        let before_save = self.before_save.clone();
        #[cfg(test)]
        let before_compute = self.before_compute.clone();
        #[cfg(test)]
        let save_hook = self.save_hook.clone();
        #[cfg(test)]
        let after_save = self.after_save.clone();
        tokio::spawn(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                #[cfg(test)]
                if let Some(hook) = before_compute {
                    hook();
                }
                super::compute_job(&model, kind, &params)
            })
            .await;
            let failure = match outcome {
                Ok(Ok(job_outcome)) => {
                    let save_id = job_id.clone();
                    let saved = tokio::task::spawn_blocking(move || {
                        let mut st = state.blocking_lock();
                        let saved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            #[cfg(test)]
                            if let Some(hook) = before_save {
                                hook();
                            }
                            #[cfg(not(test))]
                            {
                                super::persist_job_outcome(&mut st.results, job_outcome, &source)
                            }
                            #[cfg(test)]
                            {
                                super::persist::persist_job_outcome_with_hook(
                                    &mut st.results,
                                    job_outcome,
                                    &source,
                                    save_hook.as_ref(),
                                )
                            }
                        }));
                        let status = match saved {
                            Ok(Ok(summary)) => JobStatus::Done {
                                result_ref: summary,
                            },
                            Ok(Err(e)) => JobStatus::Failed {
                                error: format!("解析は完了しましたが結果の保存に失敗しました: {e}"),
                                kind: "internal".to_string(),
                            },
                            Err(_) => {
                                st.results.discard_pending();
                                JobStatus::Failed {
                                    error: "結果保存タスクが異常終了しました".to_string(),
                                    kind: "internal".to_string(),
                                }
                            }
                        };
                        #[cfg(test)]
                        if let Some(hook) = after_save {
                            hook();
                        }
                        st.jobs.lock().unwrap().update(&save_id, status);
                    })
                    .await;
                    saved.err().map(|e| JobStatus::Failed {
                        error: format!("結果保存タスクが異常終了しました: {e}"),
                        kind: "internal".to_string(),
                    })
                }
                Ok(Err(e)) => Some(JobStatus::Failed {
                    error: e.to_string(),
                    kind: e.kind().to_string(),
                }),
                Err(join_err) => Some(JobStatus::Failed {
                    error: format!("解析タスクが異常終了しました: {join_err}"),
                    kind: "internal".to_string(),
                }),
            };
            if let Some(status) = failure {
                jobs.lock().unwrap().update(&job_id, status);
            }
        });
        Ok(CallToolResult::success(vec![Content::json(
            serde_json::json!({ "job_id": id }),
        )?]))
    }

    /// 結果ストアから結果を取得する。
    #[tool(description = "解析結果ストアから結果を取得する")]
    pub async fn result_get(
        &self,
        Parameters(args): Parameters<ResultGetArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let step_range = match &args.step_range {
            None => None,
            Some(v) if v.len() == 2 => Some((v[0], v[1])),
            Some(_) => {
                return Err(ErrorData::invalid_params(
                    "step_range は [start, end) の2要素で指定してください",
                    None,
                ));
            }
        };
        let st = self.state.lock().await;
        let result = super::result_get_json(
            &st.results,
            &st.model,
            args.case,
            &args.kind,
            args.node_ids.clone(),
            args.member_ids.clone(),
            step_range,
        )
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        Ok(CallToolResult::success(vec![Content::json(result)?]))
    }

    /// ジョブの現在状態を返す。
    #[tool(description = "ジョブの状態を取得する")]
    pub async fn analysis_status(
        &self,
        Parameters(args): Parameters<AnalysisStatusArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let job = self.jobs.lock().unwrap().get(&args.job_id).cloned();
        match job {
            Some(j) => Ok(CallToolResult::success(vec![Content::json(&j)?])),
            None => Err(ErrorData::invalid_params("job not found", None)),
        }
    }
}

#[tool_handler]
impl ServerHandler for SepikaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default().with_server_info(Implementation::new("sepika-mcp", "0.1.0"))
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QueryArgs {
    pub kind: String,
    pub filter: Option<String>,
}

/// `model.edit` の引数。`command` キーで種別を指定する。
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct EditArgs {
    pub body: serde_json::Value,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct QueryResult {
    pub items: Vec<serde_json::Value>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct QuantityTakeoffArgs {
    /// 集計単位（既定 `"category"`）。
    pub group_by: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnalysisRunArgs {
    pub kind: JobKind,
    /// 対象荷重ケース ID（未指定なら先頭ケース）。
    pub load_case: Option<u32>,
    /// DesignCheck用の保存組合せindex。load_caseと同時指定不可。
    pub load_combination: Option<usize>,
    /// モード数（既定 3）。
    pub n_modes: Option<usize>,
    /// 加力・入力方向 "X"/"Y"（既定 "X"）。
    pub dir: Option<String>,
    /// 最大ステップ数（既定 50）。
    pub steps: Option<usize>,
    /// 目標変位 `[mm]`。
    pub max_disp: Option<f64>,
    /// 目標最大層間変形角の分母 n（既定 150）。
    pub max_drift_denom: Option<f64>,
    /// Ds 判定に明示採用する確定 step（0 始まり、未指定は未判定）。
    pub ds_step: Option<u32>,
    /// 保有耐力比較に明示採用する確定 step（0 始まり、未指定は未判定）。
    pub capacity_step: Option<u32>,
    /// サンプル波の時間刻み `[s]`（既定 0.01）。
    pub dt: Option<f64>,
    /// サンプル波の継続時間 `[s]`（既定 2.0）。
    pub duration: Option<f64>,
    /// サンプル波の周期 `[s]`（既定 0.5）。
    pub period: Option<f64>,
    /// サンプル波の振幅 [mm/s²]（既定 1000）。
    pub amp: Option<f64>,
    /// 地域係数 Z（既定 1.0）。
    pub z: Option<f64>,
    /// 地盤種別 `"I"`/`"II"`/`"III"`（既定 `"II"`）。
    pub soil: Option<String>,
    /// 標準せん断力係数 C0（既定 0.2）。
    pub c0: Option<f64>,
    /// Ai 算定法 `"Approx"`/`"SemiPrecise"`（既定 `"Approx"`）。
    pub ai_mode: Option<String>,
    /// 参考の固有周期 T `[s]`。数値指定は告示精算の適用証拠にならず、SemiPrecise の標準地震力は未対応。
    pub design_period: Option<f64>,
}

impl AnalysisRunArgs {
    /// 任意パラメータを `super::JobParams`（既定値込み）へ変換する。
    /// 不正な文字列の場合のみエラーを返す。
    fn to_job_params(&self) -> Result<super::JobParams, String> {
        if self.load_combination.is_some()
            && (self.load_case.is_some() || self.kind != JobKind::DesignCheck)
        {
            return Err(
                "load_combinationはDesignCheck専用でload_caseとの同時指定はできません".into(),
            );
        }
        let dir = match self.dir.as_deref() {
            None => super::JobDir::X,
            Some("X") => super::JobDir::X,
            Some("Y") => super::JobDir::Y,
            Some(other) => {
                return Err(format!("不明な方向: {other}（\"X\" または \"Y\"）"));
            }
        };
        let soil = match self.soil.as_deref() {
            None => None,
            Some("I") => Some(sepika_load::ai::SoilClass::I),
            Some("II") => Some(sepika_load::ai::SoilClass::II),
            Some("III") => Some(sepika_load::ai::SoilClass::III),
            Some(other) => {
                return Err(format!(
                    "不明な地盤種別: {other}（\"I\"、\"II\"、または \"III\"）"
                ));
            }
        };
        let ai_mode = match self.ai_mode.as_deref() {
            None => None,
            Some("Approx") => Some(sepika_solver::statics::analysis::AiMode::Approx),
            Some("SemiPrecise") => Some(sepika_solver::statics::analysis::AiMode::SemiPrecise),
            Some(other) => {
                return Err(format!(
                    "不明な Ai 算定法: {other}（\"Approx\" または \"SemiPrecise\"）"
                ));
            }
        };
        let d = super::JobParams::default();
        Ok(super::JobParams {
            load_case: self.load_case,
            load_combination: self.load_combination,
            n_modes: self.n_modes.unwrap_or(d.n_modes),
            dir,
            steps: self.steps.unwrap_or(d.steps),
            max_disp: self.max_disp,
            max_drift_denom: self.max_drift_denom,
            ds_step: self.ds_step,
            capacity_step: self.capacity_step,
            dt: self.dt.unwrap_or(d.dt),
            duration: self.duration.unwrap_or(d.duration),
            period: self.period.unwrap_or(d.period),
            amp: self.amp.unwrap_or(d.amp),
            z: self.z.unwrap_or(d.z),
            soil: soil.unwrap_or(d.soil),
            c0: self.c0.unwrap_or(d.c0),
            ai_mode: ai_mode.unwrap_or(d.ai_mode),
            design_period: self.design_period,
        })
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AnalysisStatusArgs {
    pub job_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ResultGetArgs {
    pub case: u32,
    /// "NodalDisp" | "MemberForce" | "Modal" | "TimeHistory"
    pub kind: String,
    pub node_ids: Option<Vec<u32>>,
    pub member_ids: Option<Vec<u32>>,
    pub step_range: Option<Vec<u64>>,
}

pub async fn run_stdio_server(state: ServerState) -> Result<(), Box<dyn std::error::Error>> {
    let service = SepikaServer::new(state).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::rc_wall534_fixture as crack_fixture;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LoadCase, LocalAxis, Material,
        MaterialCategory, NodalLoad, Node, Section, Story,
    };
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// テスト用の結果ストアディレクトリを用意する（テストごとに固有の名前を渡すこと）。
    /// 前回実行の残骸を消してから使う（実ストア=ファイルシステムを使うため）。
    /// プロセス ID 入りのサブディレクトリを介し、同一マシンで並行する別プロセスの
    /// テスト実行と衝突しないようにする。
    fn test_store_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sepika-test-{}/sepika_mcp_test_{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 実ストア（`FsResultStore`）を使う `ServerState` を組み立てる。
    fn make_state(model: Model, dir: &Path) -> ServerState {
        ServerState::with_fs_store(model, dir).expect("FsResultStore::open が失敗しないこと")
    }

    /// 片持ち梁（node0 固定・node1 自由）+ 荷重ケース1つの、解析が完走できる最小モデル。
    /// LinearStatic/Eigen/TimeHistory/DesignCheck の各ジョブテストで共有する:
    /// - 先端に質量を与えている(Eigen/TimeHistory が固有値解析できるように)。
    /// - 材料名を鋼材(SN400)にし、断面係数(iz)を小さくしている
    ///   (DesignCheck で NG が出ることを確認できるように、わざと過大応力にしている)。
    /// - 荷重は全体座標系 Z 方向のせん断力とし、曲げモーメントが生じるようにしている。
    ///   `ref_vector=[0,0,1]`(=全体Z)の梁は局所 y 軸が全体 Z に一致するため
    ///   （`LocalFrame::from_nodes` 参照）、全体 Z 方向の力が局所 Qy/Mz
    ///   （`compute_design_check_job` が強軸として見る成分）に載る。
    ///   全体 X 方向の軸力だけでは M・Q が実質ゼロで検定に影響しないため使わない。
    fn cantilever_with_load_case() -> Model {
        Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: [0.0, 0.0, 0.0],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: [1000.0, 0.0, 0.0],
                    restraint: Dof6Mask::FREE,
                    mass: Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
                    story: None,
                    support_spring: None,
                },
            ],
            elements: vec![ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [0.0, 0.0, 1.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            }],
            sections: vec![Section {
                id: SectionId(0),
                name: "beam".into(),
                area: 100.0,
                iy: 833.33,
                iz: 10.0,
                j: 100.0,
                depth: 10.0,
                width: 10.0,
                as_y: 83.33,
                as_z: 83.33,
                floor: None,
                frame_use: Some(sepika_core::model::FrameSectionUse::Girder),
                panel_thickness: None,
                thickness: Some(10.0),
                shape: None,
                material: Some(MaterialId(0)),
                rebar_material: None,
                shear_rebar_material: None,
                steel_material: None,
                property_basis: Default::default(),
            }],
            materials: vec![Material {
                concrete_class: Default::default(),
                strength_factor: None,
                id: MaterialId(0),
                name: "SN400".into(),
                category: MaterialCategory::Steel,
                young: 20000.0,
                poisson: 0.3,
                density: 0.0,
                shear: None,
                fc: None,
                fy: Some(235.0),
            }],
            load_cases: vec![LoadCase {
                id: LoadCaseId(1),
                name: "case1".into(),
                nodal: vec![NodalLoad::manual(
                    NodeId(1),
                    [0.0, 0.0, 1000.0, 0.0, 0.0, 0.0],
                )],
                member: Vec::new(),
                kind: sepika_core::model::LoadCaseKind::Dead,
            }],
            ..Default::default()
        }
    }

    /// 上と同じモデルから荷重ケースだけを抜いたもの（LinearStatic ジョブが
    /// `JobError::LoadCaseNotFound` で失敗する経路を確認するため）。
    fn cantilever_without_load_case() -> Model {
        Model {
            load_cases: vec![],
            ..cantilever_with_load_case()
        }
    }

    /// 1層・鉛直柱モデル（Pushover ジョブ用）。
    /// sepika-app の `sample::portal_frame`（Beam 要素・SN400B）と同じ材料構成をベースに
    /// 単純な片持ち柱 + Story(地震重量) を組み立てる
    /// （`Analysis::prepare` の線形剛性検証を通す必要があるため、
    /// ねじり剛性を持たない Fiber 要素ではなく Beam 要素を使う。
    /// sepika-solver 側の `pushover::tests::single_column_model` は Fiber 要素かつ
    /// `Analysis::prepare` を経由しないテストのため、そのままでは
    /// `App::compute_pushover`/本ジョブの「まず prepare で検証する」流儀に合わない）。
    fn pushover_model() -> Model {
        Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: [0.0, 0.0, 0.0],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: [0.0, 0.0, 3000.0],
                    restraint: Dof6Mask::FREE,
                    mass: None,
                    story: Some(StoryId(1)),
                    support_spring: None,
                },
            ],
            elements: vec![ElementData {
                id: ElemId(0),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [1.0, 0.0, 0.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                vertical_eccentricity: Some(0.0),
                plastic_zone: None,
                spring: None,
            }],
            sections: vec![Section {
                id: SectionId(0),
                name: "col".into(),
                area: 10000.0,
                iy: 8.333e6,
                iz: 8.333e6,
                j: 1.0e6,
                depth: 100.0,
                width: 100.0,
                // 有効せん断断面積 As = A·5/6（矩形。0 は解析前チェックで入力エラー）。
                as_y: 10000.0 * 5.0 / 6.0,
                as_z: 10000.0 * 5.0 / 6.0,
                floor: None,
                frame_use: Some(sepika_core::model::FrameSectionUse::Column),
                panel_thickness: None,
                thickness: None,
                shape: None,
                material: Some(MaterialId(0)),
                rebar_material: None,
                shear_rebar_material: None,
                steel_material: None,
                property_basis: Default::default(),
            }],
            materials: vec![Material {
                concrete_class: Default::default(),
                strength_factor: None,
                id: MaterialId(0),
                name: "steel".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 0.0,
                // sepika-solver 側の `single_column_model`（Fiber 要素、ねじり無視）は
                // shear=Some(0.0) だが、ここは Beam 要素（弾性ねじり剛性 GJ を持つ）なので
                // shear=None にして young/poisson から G を導出させる
                // （G=0 のままだと頂部のねじり自由度の剛性が 0 になり特異行列になる）。
                shear: None,
                fc: None,
                fy: Some(235.0),
            }],
            stories: vec![
                // 階は床であり、先頭は基部の床（`Model::layers` の不変条件）。
                Story {
                    wall_weights: Vec::new(),
                    id: StoryId(0),
                    name: "1F".into(),
                    elevation: Some(0.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(0)],
                    seismic_weight: None,
                    weight_override: None,
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
                Story {
                    wall_weights: Vec::new(),
                    id: StoryId(1),
                    name: "2F".into(),
                    elevation: Some(3000.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(1)],
                    seismic_weight: Some(80_000.0),
                    weight_override: Some(80_000.0),
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
            ],
            ..Default::default()
        }
    }

    fn pushover_model_with_saved_gravity() -> Model {
        use sepika_core::model::{LoadCaseKind, LoadCombination, DL_CASE_NAME};

        let mut model = pushover_model();
        model.load_cases = vec![
            LoadCase {
                id: LoadCaseId(0),
                name: DL_CASE_NAME.into(),
                kind: LoadCaseKind::Dead,
                nodal: Vec::new(),
                member: Vec::new(),
            },
            LoadCase {
                id: LoadCaseId(1),
                name: "P=0".into(),
                kind: LoadCaseKind::Live,
                nodal: Vec::new(),
                member: Vec::new(),
            },
        ];
        model.combinations.push(LoadCombination {
            name: "G+P".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)],
        });
        model.ultimate_gravity_combination = Some(model.combinations.len() - 1);
        model
    }

    /// `AnalysisRunArgs` を最小限のフィールド指定で組み立てる
    /// （他は全て既定値=None）。
    fn run_args(kind: JobKind) -> AnalysisRunArgs {
        AnalysisRunArgs {
            kind,
            load_case: None,
            load_combination: None,
            n_modes: None,
            dir: None,
            steps: None,
            max_disp: None,
            max_drift_denom: None,
            ds_step: None,
            capacity_step: None,
            dt: None,
            duration: None,
            period: None,
            amp: None,
            z: None,
            soil: None,
            c0: None,
            ai_mode: None,
            design_period: None,
        }
    }

    /// `CallToolResult`（`analysis_run` の戻り値）から `job_id` を取り出す。
    fn extract_job_id(result: &CallToolResult) -> String {
        let text = result.content[0]
            .raw
            .as_text()
            .expect("analysis_run の応答は text content のはず")
            .text
            .clone();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["job_id"].as_str().unwrap().to_string()
    }

    /// ジョブが `Done`/`Failed` のいずれかの終端状態に達するまでポーリングする。
    async fn wait_for_terminal(server: &SepikaServer, job_id: &str) -> JobStatus {
        for _ in 0..400 {
            {
                let jobs = server.jobs.lock().unwrap();
                if let Some(job) = jobs.get(job_id) {
                    match &job.status {
                        JobStatus::Done { .. } | JobStatus::Failed { .. } => {
                            return job.status.clone();
                        }
                        _ => {}
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("job did not reach a terminal state in time");
    }

    #[tokio::test]
    async fn public_mcp_crack_reference_matches_independent_points_and_opening_skip() {
        use sepika_design_jp::{
            wall_check::{WallCheck, WallCheckKind},
            CheckOutcome,
        };
        let dir = test_store_dir("534-crack-reference");
        let model = crack_fixture::input_model(crate::job::rc_wall617_fixture::input_model());
        let server = SepikaServer::new(make_state(model.clone(), &dir));
        let result = server
            .analysis_run(Parameters(run_args(JobKind::DesignCheck)))
            .await
            .unwrap();
        let summary = done_summary(&wait_for_terminal(&server, &extract_job_id(&result)).await);
        let checks: Vec<WallCheck> =
            serde_json::from_value(summary["wall_checks"].clone()).unwrap();
        let reference = checks
            .iter()
            .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
            .unwrap();
        assert!(
            matches!(reference.outcome, CheckOutcome::Checked(_)),
            "{:?}",
            reference
        );
        crack_fixture::assert_reference(reference.reference_skeleton.as_ref().unwrap());
        {
            let mut state = server.state.lock().await;
            state.model.wall_plates[0]
                .openings
                .push(sepika_core::model::WallOpening {
                    width: 200.0,
                    height: 200.0,
                    offset: None,
                });
        }
        let result = server
            .analysis_run(Parameters(run_args(JobKind::DesignCheck)))
            .await
            .unwrap();
        let summary = done_summary(&wait_for_terminal(&server, &extract_job_id(&result)).await);
        let checks: Vec<WallCheck> =
            serde_json::from_value(summary["wall_checks"].clone()).unwrap();
        let reference = checks
            .iter()
            .find(|w| w.kind == WallCheckKind::ReferenceSkeleton)
            .unwrap();
        assert!(
            matches!(&reference.outcome, CheckOutcome::Skipped { reason } if reason.contains("Qc用開口低減率が未確定")),
            "{:?}",
            reference
        );
        assert!(reference.reference_skeleton.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saving_status_responds_before_save_release() {
        let dir = test_store_dir("saving_status");
        let mut server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered_tx = std::sync::Mutex::new(Some(entered_tx));
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = gate.clone();
        server.before_save = Some(Arc::new(move || {
            entered_tx.lock().unwrap().take().unwrap().send(()).unwrap();
            let (lock, wake) = &*worker_gate;
            let guard = lock.lock().unwrap();
            drop(wake.wait_while(guard, |released| !*released).unwrap());
        }));
        let run = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let id = extract_job_id(&run);
        tokio::time::timeout(Duration::from_secs(5), entered_rx)
            .await
            .unwrap()
            .unwrap();
        let status = tokio::time::timeout(
            Duration::from_millis(250),
            server.analysis_status(Parameters(AnalysisStatusArgs { job_id: id.clone() })),
        )
        .await;
        let (lock, wake) = &*gate;
        *lock.lock().unwrap() = true;
        wake.notify_all();
        let status = status.expect("保存終了前に公開statusが応答する").unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&status.content[0].raw.as_text().unwrap().text).unwrap();
        assert!(value["status"]["Running"]["progress"].is_null(), "{value}");
        assert!(matches!(
            wait_for_terminal(&server, &id).await,
            JobStatus::Done { .. }
        ));
    }

    fn done_summary(status: &JobStatus) -> serde_json::Value {
        match status {
            JobStatus::Done { result_ref } => {
                serde_json::from_str(result_ref).expect("summary は JSON のはず")
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn frame_node_counts_mcp_tools_reject_without_mutating_model_or_results() {
        use sepika_core::ids::NodeId;
        use sepika_core::model::ElementKind;
        for (kind_index, kind) in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
            ElementKind::Brace {
                tension_only: false,
            },
        ]
        .into_iter()
        .enumerate()
        {
            for nodes in [
                vec![],
                vec![NodeId(0)],
                vec![NodeId(0), NodeId(2), NodeId(1)],
                vec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
                vec![NodeId(0), NodeId(0)],
            ] {
                let mut model = cantilever_with_load_case();
                model.elements[0].kind = kind;
                model.elements[0].nodes = nodes.into();
                let reason = model.validate_frame_node_counts().unwrap_err().to_string();
                let before = format!("{model:?}");
                let dir = test_store_dir(&format!(
                    "frame562-{kind_index}-{}",
                    model.elements[0].nodes.len()
                ));
                let server = SepikaServer::new(make_state(model, &dir));
                let result = server
                    .quantity_takeoff(Parameters(QuantityTakeoffArgs { group_by: None }))
                    .await
                    .unwrap();
                let value: serde_json::Value =
                    serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
                assert_eq!(value["status"], "unavailable");
                assert!(value["totals"].is_null());
                assert_eq!(value["reason"], reason);
                let result = server
                    .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
                    .await
                    .unwrap();
                match wait_for_terminal(&server, &extract_job_id(&result)).await {
                    JobStatus::Failed { error, kind } => {
                        assert!(error.contains(&reason), "{error}");
                        assert_eq!(kind, "invalid_input");
                    }
                    other => panic!("{other:?}"),
                }
                let state = server.state.lock().await;
                assert_eq!(format!("{:?}", state.model), before);
                assert!(state.results.manifest().entries.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn lip_channel_geometry_actual_mcp_tools_report_reason_without_results() {
        use sepika_core::section_shape::SectionShape;
        let mut model = cantilever_with_load_case();
        model.sections[0].shape = Some(SectionShape::SteelLipChannel {
            height: 150.0,
            width: 75.0,
            lip: 100.0,
            thick: 2.3,
        });
        model.sections[0].name = "LC544".into();
        let reason = model.validate_lip_channels().unwrap_err().to_string();
        let before = format!("{model:?}");
        let dir = test_store_dir("lip544");
        let server = SepikaServer::new(make_state(model, &dir));
        let result = server
            .model_query(Parameters(QueryArgs {
                kind: "sections".into(),
                filter: None,
            }))
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
        assert!(value["items"][0]["unavailable_reason"]
            .as_str()
            .unwrap()
            .contains("H=150"));
        let result = server
            .quantity_takeoff(Parameters(QuantityTakeoffArgs { group_by: None }))
            .await
            .unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
        assert_eq!(value["status"], "unavailable");
        assert!(value["totals"].is_null());
        assert_eq!(value["reason"], reason);
        let result = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        match wait_for_terminal(&server, &extract_job_id(&result)).await {
            JobStatus::Failed { error, kind } => {
                assert!(error.contains(&reason), "{error}");
                assert_eq!(kind, "invalid_input");
            }
            other => panic!("{other:?}"),
        }
        let state = server.state.lock().await;
        assert_eq!(format!("{:?}", state.model), before);
        assert!(state.results.manifest().entries.is_empty());
    }
    #[tokio::test]
    async fn test_analysis_run_fails_without_load_case() {
        let dir = test_store_dir("linear_static_no_case");
        let server = SepikaServer::new(make_state(cantilever_without_load_case(), &dir));
        let result = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        match status {
            JobStatus::Failed { error, kind } => {
                // 文言は利用者向けの日本語、種別コードは機械可読な安定値。
                assert!(error.contains("荷重ケース"), "unexpected error: {error}");
                assert_eq!(kind, "load_case_not_found");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// LinearStatic ジョブ → Done → manifest に NodalDisp/MemberForce が載る →
    /// result_get(NodalDisp, node_ids 指定) が該当行を返す。
    #[tokio::test]
    async fn test_linear_static_job_persists_and_result_get_filters_nodes() {
        let dir = test_store_dir("linear_static_result_get");
        let server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let result = server
            .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
            .await
            .unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        let summary = done_summary(&status);
        assert_eq!(summary["store"]["case"], 1);
        let kinds: Vec<String> = summary["store"]["kinds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(kinds.contains(&"NodalDisp".to_string()));
        assert!(kinds.contains(&"MemberForce".to_string()));

        {
            let st = server.state.lock().await;
            let manifest = st.results.manifest();
            assert!(manifest
                .entries
                .iter()
                .any(|e| e.case == 1 && e.kind == sepika_io::results::ResultKind::NodalDisp));
            assert!(manifest
                .entries
                .iter()
                .any(|e| e.case == 1 && e.kind == sepika_io::results::ResultKind::MemberForce));
        }

        let got = server
            .result_get(Parameters(ResultGetArgs {
                case: 1,
                kind: "NodalDisp".to_string(),
                node_ids: Some(vec![1]),
                member_ids: None,
                step_range: None,
            }))
            .await
            .unwrap();
        let text = got.content[0].raw.as_text().unwrap().text.clone();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = value["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "node_ids=[1] で絞り込んだ行数");
        assert_eq!(rows[0]["node_id"], 1);
        assert_eq!(value["truncated"], false);
    }

    #[tokio::test]
    async fn load_state_combination_jobs_preserve_single_case_store_and_return_their_own_forces() {
        async fn get(server: &SepikaServer, case: u32, kind: &str) -> serde_json::Value {
            let result = server
                .result_get(Parameters(ResultGetArgs {
                    case,
                    kind: kind.into(),
                    node_ids: None,
                    member_ids: None,
                    step_range: None,
                }))
                .await
                .unwrap();
            serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap()
        }
        async fn run(server: &SepikaServer, args: AnalysisRunArgs) -> serde_json::Value {
            let result = server.analysis_run(Parameters(args)).await.unwrap();
            let job_id = extract_job_id(&result);
            assert!(matches!(
                wait_for_terminal(server, &job_id).await,
                JobStatus::Done { .. }
            ));
            let status = server
                .analysis_status(Parameters(AnalysisStatusArgs { job_id }))
                .await
                .unwrap();
            let job: serde_json::Value =
                serde_json::from_str(&status.content[0].raw.as_text().unwrap().text).unwrap();
            // 公開 API の result_ref はオブジェクトではなく JSON 文字列。
            serde_json::from_str(job["status"]["Done"]["result_ref"].as_str().unwrap()).unwrap()
        }
        let dir = test_store_dir("load_state_combination_store");
        let mut model = cantilever_with_load_case();
        model.load_cases = [
            (sepika_core::model::LoadCaseKind::Dead, 100_000.0),
            (sepika_core::model::LoadCaseKind::Live, 20_000.0),
            (sepika_core::model::LoadCaseKind::Snow, 40_000.0),
            (sepika_core::model::LoadCaseKind::Seismic, 30_000.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (kind, force))| LoadCase {
            id: LoadCaseId(i as u32),
            name: format!("任意荷重{i}"),
            kind,
            nodal: vec![NodalLoad::manual(
                NodeId(1),
                [0.0, 0.0, -force, 0.0, 0.0, 0.0],
            )],
            member: vec![],
        })
        .collect();
        model.combinations = [1.0, 0.7]
            .into_iter()
            .enumerate()
            .map(|(i, snow)| sepika_core::model::LoadCombination {
                name: format!("保存組合せ{i}"),
                terms: vec![
                    (LoadCaseId(0), 1.0),
                    (LoadCaseId(1), 1.0),
                    (LoadCaseId(2), snow),
                ],
            })
            .collect();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        let model_path = dir.with_extension("ovika");
        sepika_io::ovika::save_ovika(&model_path, &model, Default::default()).unwrap();
        let server = SepikaServer::new(make_state(
            sepika_io::ovika::load_ovika(&model_path).unwrap().model,
            &dir,
        ));
        let mut originals = Vec::new();
        for case in [0, 3] {
            let mut args = run_args(JobKind::LinearStatic);
            args.load_case = Some(case);
            let summary = run(&server, args).await;
            assert_eq!(summary["store"]["case"], case);
            assert_eq!(summary["store"]["persisted"], true);
            originals.push((
                case,
                get(&server, case, "NodalDisp").await,
                get(&server, case, "MemberForce").await,
            ));
        }
        assert_eq!(
            server.state.lock().await.results.manifest().entries.len(),
            4
        );
        for (index, q, term) in [(0, 160_000.0, "short"), (1, 148_000.0, "long")] {
            let mut args = run_args(JobKind::DesignCheck);
            args.load_case = None;
            args.load_combination = Some(index);
            let summary = run(&server, args).await;
            for (case, disp, force) in &originals {
                assert_eq!(&get(&server, *case, "NodalDisp").await, disp);
                assert_eq!(&get(&server, *case, "MemberForce").await, force);
            }
            assert!(summary["case"].is_null());
            assert_eq!(summary["store"]["persisted"], false);
            assert!(summary["store"]["case"].is_null());
            assert_eq!(summary["load_target"]["source"], "saved_combination");
            assert_eq!(summary["load_target"]["combination_index"], index);
            assert_eq!(summary["term"], term);
            assert_eq!(
                summary["member_forces"]["rows"][0]["qy"]
                    .as_f64()
                    .unwrap()
                    .abs(),
                q
            );
            assert_eq!(summary["member_forces"]["truncated"], false);
            assert_eq!(summary["member_forces"]["total_rows"], 3);
            assert_eq!(
                summary["member_forces"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|row| row["pos"].as_f64().unwrap())
                    .collect::<Vec<_>>(),
                vec![0.0, 0.5, 1.0]
            );
            assert_eq!(summary["store"]["retrieval"]["encoding"], "json_string");
            assert_eq!(summary["store"]["lifetime"], "server_process");
        }
        server.state.lock().await.model.load_cases[2].kind =
            sepika_core::model::LoadCaseKind::Other;
        let mut args = run_args(JobKind::DesignCheck);
        args.load_case = Some(3);
        let summary = run(&server, args).await;
        assert!(summary["case"].is_null());
        assert_eq!(summary["store"]["persisted"], false);
        assert_eq!(
            summary["load_target"]["source"],
            "automatic_gravity_combination"
        );
        assert_eq!(
            summary["member_forces"]["rows"][0]["qy"]
                .as_f64()
                .unwrap()
                .abs(),
            150_000.0
        );
        let reopened = SepikaServer::new(make_state(server.state.lock().await.model.clone(), &dir));
        assert_eq!(
            reopened.state.lock().await.results.manifest().entries.len(),
            4
        );
        for (case, mut disp, mut force) in originals {
            assert_eq!(disp["status"], "current");
            assert_eq!(force["status"], "current");
            disp["status"] = "stale".into();
            force["status"] = "stale".into();
            assert_eq!(get(&server, case, "NodalDisp").await, disp);
            assert_eq!(get(&server, case, "MemberForce").await, force);
            assert_eq!(get(&reopened, case, "NodalDisp").await, disp);
            assert_eq!(get(&reopened, case, "MemberForce").await, force);
        }
    }

    /// Eigen ジョブ → Done（周期がサマリに含まれる）→
    /// result_get(Modal) が n_modes 行返す。
    #[tokio::test]
    async fn test_eigen_job_persists_and_result_get_modal() {
        let dir = test_store_dir("eigen_result_get");
        let server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let mut args = run_args(JobKind::Eigen);
        args.n_modes = Some(1);
        let result = server.analysis_run(Parameters(args)).await.unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        let summary = done_summary(&status);
        assert_eq!(summary["n_modes"], 1);
        assert!(summary["period"].as_array().unwrap()[0].as_f64().unwrap() > 0.0);
        assert_eq!(summary["store"]["case"], 0);

        let got = server
            .result_get(Parameters(ResultGetArgs {
                case: 0,
                kind: "Modal".to_string(),
                node_ids: None,
                member_ids: None,
                step_range: None,
            }))
            .await
            .unwrap();
        let text = got.content[0].raw.as_text().unwrap().text.clone();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let rows = value["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1, "n_modes=1 なので1行返るはず");
    }

    /// Pushover ジョブ（stories 付きモデル）→ Done でサマリに qu[kN] が含まれる。
    #[tokio::test]
    async fn test_pushover_job_completes_with_max_base_shear_in_summary() {
        let dir = test_store_dir("pushover_basic");
        let server = SepikaServer::new(make_state(pushover_model_with_saved_gravity(), &dir));
        let mut args = run_args(JobKind::Pushover);
        // 既定(steps=50, 目標層間変形角1/150)だと機構形成後に特異行列となり得るため
        // (sepika-solver 側の同種テストと同じ配慮)、目標変位を明示して小さめの値にする。
        args.steps = Some(10);
        args.max_disp = Some(30.0);
        let result = server.analysis_run(Parameters(args)).await.unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        let summary = done_summary(&status);
        assert_eq!(
            summary["wall_run"]["gravity_terms"],
            serde_json::json!([[0, 1.0], [1, 1.0]])
        );
        assert!(summary["max_base_shear_kN"].as_f64().unwrap() > 0.0);
        assert!(
            summary.get("store").is_none(),
            "Pushover はストアへ書かない"
        );
    }

    /// DesignCheck ジョブ → Done でサマリに NG 数が含まれる。
    #[tokio::test]
    async fn test_design_check_job_reports_ng_count() {
        let dir = test_store_dir("design_check_basic");
        let server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let result = server
            .analysis_run(Parameters(run_args(JobKind::DesignCheck)))
            .await
            .unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        let summary = done_summary(&status);
        assert_eq!(summary["case"], 1);
        assert!(summary["n_checks"].as_u64().unwrap() > 0);
        assert!(
            summary["n_ng"].as_u64().unwrap() > 0,
            "断面係数を小さくしてあるので過大応力で NG になるはず: {summary}"
        );
    }

    /// `result_get`: 存在しない case を指定すると invalid_params エラーになる。
    #[tokio::test]
    async fn test_result_get_missing_case_is_invalid_params() {
        let dir = test_store_dir("result_get_missing");
        let server = SepikaServer::new(make_state(cantilever_with_load_case(), &dir));
        let err = server
            .result_get(Parameters(ResultGetArgs {
                case: 999,
                kind: "NodalDisp".to_string(),
                node_ids: None,
                member_ids: None,
                step_range: None,
            }))
            .await
            .expect_err("manifest にない case は Err のはず");
        assert!(
            err.message.contains("結果がありません"),
            "エラーメッセージに『結果がありません』が含まれるはず: {err:?}"
        );
    }

    #[test]
    fn test_to_job_params_parses_seismic_settings() {
        let mut args = run_args(JobKind::LinearStatic);
        args.z = Some(1.2);
        args.soil = Some("III".to_string());
        args.c0 = Some(0.25);
        args.ai_mode = Some("SemiPrecise".to_string());
        args.design_period = None;
        let params = args
            .to_job_params()
            .expect("SemiPrecise without design_period");
        assert!((params.z - 1.2).abs() < 1e-12);
        assert_eq!(params.soil, sepika_load::ai::SoilClass::III);
        assert!((params.c0 - 0.25).abs() < 1e-12);
        assert_eq!(
            params.ai_mode,
            sepika_solver::statics::analysis::AiMode::SemiPrecise
        );
        assert_eq!(params.design_period, None);
    }

    #[tokio::test]
    async fn test_analysis_run_semi_precise_without_design_period_completes() {
        // ジョブは完了し、サマリ JSON に notices が載ることを確認する。
        let dir = test_store_dir("semi_precise_no_t");
        let server = SepikaServer::new(make_state(pushover_model_with_saved_gravity(), &dir));
        let mut args = run_args(JobKind::Pushover);
        args.steps = Some(10);
        args.max_disp = Some(30.0);
        args.ai_mode = Some("SemiPrecise".to_string());
        args.design_period = None;
        let result = server.analysis_run(Parameters(args)).await.unwrap();
        let job_id = extract_job_id(&result);
        let status = wait_for_terminal(&server, &job_id).await;
        let JobStatus::Done { result_ref } = status else {
            panic!("SemiPrecise without design_period should still complete: {status:?}");
        };
        let summary: serde_json::Value = serde_json::from_str(&result_ref).expect("summary JSON");
        assert_eq!(
            summary["wall_run"]["gravity_terms"],
            serde_json::json!([[0, 1.0], [1, 1.0]])
        );
        let notices = summary["notices"]
            .as_array()
            .expect("notices がサマリに載ること");
        assert!(
            notices
                .iter()
                .any(|n| n.as_str().is_some_and(|s| s.contains("EX/EY"))),
            "notices に EX/EY 未更新の旨が含まれること: {notices:?}"
        );
    }
    #[tokio::test]
    async fn seismic_mcp_rejects_invalid_c0_and_unsupported_refined_period() {
        for name in ["EX", "EY"] {
            for (tag, c0, mode) in [
                ("invalid_c0", 0.1, "Approx"),
                ("unsupported_rt", 0.2, "SemiPrecise"),
                ("missing_period", 0.2, "SemiPrecise"),
                ("inconsistent_gl", 0.2, "Approx"),
            ] {
                let dir = test_store_dir(&format!("{tag}_{name}"));
                let mut model = pushover_model();
                if tag == "inconsistent_gl" {
                    model.stories[0].elevation = Some(-9000.0);
                    model.nodes[0].coord[2] = -9000.0;
                    let template = model.stories[1].clone();
                    let node_template = model.nodes[1].clone();
                    let beam_template = model.elements[0].clone();
                    model.stories.truncate(1);
                    model.nodes.truncate(1);
                    model.elements.clear();
                    for (i, elevation) in
                        [-6000.0, -3000.0, 6000.0, 12000.0].into_iter().enumerate()
                    {
                        let id = (i + 1) as u32;
                        let mut story = template.clone();
                        story.id = StoryId(id);
                        story.name = format!("floor{i}");
                        story.elevation = Some(elevation);
                        story.node_ids = vec![NodeId(id)];
                        story.weight_override = Some(100_000.0);
                        if i < 2 {
                            story.level_kind = sepika_core::model::StoryLevelKind::Basement {
                                depth_mm: if i == 0 { 9000.0 } else { 3000.0 },
                            };
                        }
                        model.stories.push(story);
                        let mut node = node_template.clone();
                        node.id = NodeId(id);
                        node.coord[2] = elevation;
                        node.story = Some(StoryId(id));
                        model.nodes.push(node);
                        let mut beam = beam_template.clone();
                        beam.id = ElemId(i as u32);
                        beam.nodes = smallvec::smallvec![NodeId(id - 1), NodeId(id)];
                        model.elements.push(beam);
                    }
                }
                model.load_cases.push(sepika_core::model::LoadCase {
                    id: sepika_core::ids::LoadCaseId(0),
                    name: name.into(),
                    kind: sepika_core::model::LoadCaseKind::Seismic,
                    nodal: vec![sepika_core::model::NodalLoad::auto(
                        NodeId(1),
                        [16_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                    )],
                    member: Vec::new(),
                });
                model.load_cases.push(sepika_core::model::LoadCase {
                    id: sepika_core::ids::LoadCaseId(1),
                    name: "DL".into(),
                    kind: sepika_core::model::LoadCaseKind::Dead,
                    nodal: vec![sepika_core::model::NodalLoad::manual(
                        NodeId(1),
                        [0.0, 0.0, -1000.0, 0.0, 0.0, 0.0],
                    )],
                    member: Vec::new(),
                });
                let server = SepikaServer::new(make_state(model, &dir));
                let mut args = run_args(JobKind::LinearStatic);
                args.load_case = Some(0);
                args.c0 = Some(c0);
                args.ai_mode = Some(mode.into());
                args.design_period = (tag != "missing_period").then_some(0.5);
                let result = server.analysis_run(Parameters(args)).await.unwrap();
                let status = wait_for_terminal(&server, &extract_job_id(&result)).await;
                let JobStatus::Failed { error, kind } = status else {
                    panic!("{tag}/{name}: {status:?}");
                };
                assert_eq!(kind, "invalid_input");
                let expected = match tag {
                    "invalid_c0" => "C0 は有限値かつ 0.2 以上",
                    "unsupported_rt" => "告示1793号第2ただし書",
                    "missing_period" => "固有値解析が未実行です",
                    _ => "GL が一致しません",
                };
                assert!(error.contains(expected), "{tag}/{name}: {error}");
                if tag != "missing_period" {
                    assert!(error.contains(&format!("{name} の Ai 地震力")), "{error}");
                }
                let other = if name == "EX" { "EY" } else { "EX" };
                assert!(!error.contains(&format!("{other} の Ai 地震力")), "{error}");
                let mut dl_args = run_args(JobKind::LinearStatic);
                dl_args.load_case = Some(1);
                dl_args.c0 = Some(c0);
                dl_args.ai_mode = Some(mode.into());
                dl_args.design_period = (tag != "missing_period").then_some(0.5);
                let result = server.analysis_run(Parameters(dl_args)).await.unwrap();
                let status = wait_for_terminal(&server, &extract_job_id(&result)).await;
                assert!(
                    matches!(status, JobStatus::Done { .. }),
                    "{tag}/{name}/DL: {status:?}"
                );
            }
        }
    }
    #[tokio::test]
    async fn mixed_structure_basement_mcp_standard_load_matches_above_ground_period() {
        let dir = test_store_dir("mixed_above_gl");
        let mut model = pushover_model();
        let rc = seismic_freshness_fixture::two_storeys_with_weightless_steel_girders();
        let mut concrete = rc.materials[0].clone();
        concrete.id = sepika_core::ids::MaterialId(1);
        model.materials.push(concrete);
        model.materials.push(rc.materials[2].clone());
        let mut column = rc.sections[0].clone();
        column.id = sepika_core::ids::SectionId(1);
        column.material = Some(sepika_core::ids::MaterialId(1));
        model.sections.push(column);
        let story_template = model.stories[1].clone();
        let node_template = model.nodes[1].clone();
        let beam_template = model.elements[0].clone();
        model.stories.truncate(1);
        model.nodes.truncate(1);
        model.elements.clear();
        model.stories[0].elevation = Some(-9000.0);
        model.nodes[0].coord[2] = -9000.0;
        for (i, elevation) in [-6000.0, -3000.0, 15000.0, 30000.0].into_iter().enumerate() {
            let id = (i + 1) as u32;
            let mut story = story_template.clone();
            story.id = StoryId(id);
            story.name = format!("floor{id}");
            story.elevation = Some(elevation);
            story.node_ids = vec![NodeId(id)];
            story.seismic_weight = Some(100_000.0);
            story.weight_override = Some(100_000.0);
            if i < 2 {
                story.level_kind = sepika_core::model::StoryLevelKind::Basement {
                    depth_mm: -elevation,
                };
            }
            if i == 2 {
                story.structure = sepika_core::model::StoryStructure::S;
            }
            model.stories.push(story);
            let mut node = node_template.clone();
            node.id = NodeId(id);
            node.coord[2] = elevation;
            node.story = Some(StoryId(id));
            model.nodes.push(node);
            let mut beam = beam_template.clone();
            beam.id = ElemId(i as u32);
            beam.nodes = smallvec::smallvec![NodeId(id - 1), NodeId(id)];
            beam.section = Some(sepika_core::ids::SectionId(if i == 2 { 0 } else { 1 }));
            model.elements.push(beam);
        }
        model.load_cases.push(sepika_core::model::LoadCase {
            id: sepika_core::ids::LoadCaseId(0),
            name: "EX".into(),
            kind: sepika_core::model::LoadCaseKind::Seismic,
            nodal: vec![],
            member: vec![],
        });
        let server = SepikaServer::new(make_state(model, &dir));
        let mut args = run_args(JobKind::LinearStatic);
        args.load_case = Some(0);
        args.ai_mode = Some("Approx".into());
        let run = server.analysis_run(Parameters(args)).await.unwrap();
        let status = wait_for_terminal(&server, &extract_job_id(&run)).await;
        let _summary = done_summary(&status);
        let result = server
            .result_get(Parameters(ResultGetArgs {
                case: 0,
                kind: "MemberForce".into(),
                node_ids: None,
                member_ids: Some(vec![0]),
                step_range: None,
            }))
            .await
            .unwrap();
        let text = &result.content[0].raw.as_text().unwrap().text;
        let value: serde_json::Value = serde_json::from_str(text).unwrap();
        let rows = value["rows"].as_array().unwrap();
        assert!(!rows.is_empty());
        for row in rows {
            let qy = row["qy"].as_f64().unwrap();
            let qz = row["qz"].as_f64().unwrap();
            assert!((qy.hypot(qz) - 57_250.0).abs() < 1e-4, "{row}");
        }
    }
    mod seismic_freshness_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-job/tests/fixtures/seismic_freshness.rs"
        ));
    }

    #[tokio::test]
    async fn public_mcp_ex_uses_edited_density_finish_and_seismic_live_without_prepare_button() {
        use sepika_core::model::{AreaLoad, SlabUsage};
        let dir = test_store_dir("seismic_freshness_439");
        let mut model = seismic_freshness_fixture::two_storeys_with_weightless_steel_girders();
        sepika_job::prepare::prepare_model(&mut model, &Default::default(), None, true).unwrap();
        let ex = model
            .load_cases
            .iter()
            .find(|case| case.name == "EX")
            .unwrap()
            .id
            .0;
        let server = SepikaServer::new(make_state(model, &dir));
        for (density, finish, seismic_live) in [
            (2.4e-9, 0.0, 0.0),
            (2.6e-9, 0.0, 0.0),
            (2.6e-9, 0.001, 0.0),
            (2.6e-9, 0.001, 0.0005),
        ] {
            {
                let mut state = server.state.lock().await;
                state.model.materials[0].density = density;
                for slab in &mut state.model.slabs {
                    slab.plate.loads = vec![AreaLoad {
                        kind: "DL".into(),
                        value: finish,
                    }];
                    slab.plate.usage = Some(SlabUsage::Custom {
                        floor: 0.005,
                        beam: 0.004,
                        frame: 0.003,
                        seismic: seismic_live,
                    });
                }
            }
            let mut args = run_args(JobKind::LinearStatic);
            args.load_case = Some(ex);
            let result = server.analysis_run(Parameters(args)).await.unwrap();
            let status = wait_for_terminal(&server, &extract_job_id(&result)).await;
            assert!(matches!(status, JobStatus::Done { .. }), "{status:?}");
            let result = server
                .result_get(Parameters(ResultGetArgs {
                    case: ex,
                    kind: "MemberForce".into(),
                    node_ids: None,
                    member_ids: Some(vec![0, 1]),
                    step_range: None,
                }))
                .await
                .unwrap();
            let value: serde_json::Value =
                serde_json::from_str(&result.content[0].raw.as_text().unwrap().text).unwrap();
            let rows = value["rows"].as_array().unwrap();
            let actual: f64 = rows
                .iter()
                .filter(|row| row["pos"].as_f64() == Some(0.0))
                .map(|row| row["qy"].as_f64().unwrap().abs())
                .sum();
            let expected = 0.2
                * seismic_freshness_fixture::expected_weights(density, finish, seismic_live)
                    .iter()
                    .sum::<f64>();
            assert!((actual - expected).abs() < 1e-4, "{value} / {expected}");
        }
    }
    mod high_density_steel_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-job/tests/fixtures/high_density_steel.rs"
        ));
    }

    mod steel_slab_density_fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-job/tests/fixtures/steel_slab.rs"
        ));
    }

    #[tokio::test]
    async fn public_mcp_high_density_steel_rejects_existing_design_load_case_and_recovers() {
        for (mut model, material, suffix) in [
            (high_density_steel_fixture::steel_frame(), 0, "steel_501"),
            (
                steel_slab_density_fixture::one_cubic_metre(true),
                1,
                "slab_581",
            ),
        ] {
            let dir = test_store_dir(suffix);
            sepika_job::prepare::prepare_model(&mut model, &Default::default(), None, true)
                .unwrap();
            let ex = model
                .load_cases
                .iter()
                .find(|c| c.name == "EX")
                .unwrap()
                .id
                .0;
            let server = SepikaServer::new(make_state(model, &dir));
            server.state.lock().await.model.materials[material].density = 85e-6 / 9806.65;
            let mut args = run_args(JobKind::LinearStatic);
            args.load_case = Some(ex);
            let result = server.analysis_run(Parameters(args)).await.unwrap();
            let status = wait_for_terminal(&server, &extract_job_id(&result)).await;
            let text = format!("{status:?}");
            assert!(
                text.contains(&format!("材料 {material}")) && text.contains("過小評価"),
                "{text}"
            );
            assert!(!matches!(status, JobStatus::Done { .. }));
            server.state.lock().await.model.materials[material].density = 7.85e-9;
            let mut args = run_args(JobKind::LinearStatic);
            args.load_case = Some(ex);
            let result = server.analysis_run(Parameters(args)).await.unwrap();
            let status = wait_for_terminal(&server, &extract_job_id(&result)).await;
            assert!(matches!(status, JobStatus::Done { .. }), "{status:?}");
        }
    }
    include!("server_lifecycle_tests.rs");
    #[tokio::test]
    async fn sm520_design_job_rejects_out_of_range_and_keeps_independent_forces() {
        for name in ["SM520", "SM520B", "SM520C"] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let mut model = cantilever_with_load_case();
                model.materials[0].name = name.into();
                model.materials[0].fy = fy;
                model.sections[0].thickness = Some(100.001);
                let dir = test_store_dir(&format!("sm520-{name}-{fy:?}"));
                let server = SepikaServer::new(make_state(model, &dir));
                let static_job = server
                    .analysis_run(Parameters(run_args(JobKind::LinearStatic)))
                    .await
                    .unwrap();
                let status = wait_for_terminal(&server, &extract_job_id(&static_job)).await;
                let _ = done_summary(&status);
                for _ in 0..2 {
                    let job = server
                        .analysis_run(Parameters(run_args(JobKind::DesignCheck)))
                        .await
                        .unwrap();
                    let status = wait_for_terminal(&server, &extract_job_id(&job)).await;
                    let JobStatus::Failed { error, kind } = status else {
                        panic!("範囲外鋼材が正常完了しました: {status:?}")
                    };
                    assert_eq!(kind, "invalid_input");
                    assert!(
                        error.contains("部材 0") && error.contains("適用範囲"),
                        "{error}"
                    );
                    let forces = server
                        .result_get(Parameters(ResultGetArgs {
                            case: 1,
                            kind: "MemberForce".into(),
                            node_ids: None,
                            member_ids: None,
                            step_range: None,
                        }))
                        .await
                        .unwrap();
                    let rows: serde_json::Value =
                        serde_json::from_str(&forces.content[0].raw.as_text().unwrap().text)
                            .unwrap();
                    assert!(!rows["rows"].as_array().unwrap().is_empty());
                }
                let stored_model = server.state.lock().await.model.clone();
                let reopened = SepikaServer::new(make_state(stored_model, &dir));
                let job = reopened
                    .analysis_run(Parameters(run_args(JobKind::DesignCheck)))
                    .await
                    .unwrap();
                let status = wait_for_terminal(&reopened, &extract_job_id(&job)).await;
                let JobStatus::Failed { error, kind } = status else {
                    panic!("再読込後に範囲外鋼材が正常完了しました: {status:?}")
                };
                assert_eq!(kind, "invalid_input");
                assert!(
                    error.contains("部材 0") && error.contains("適用範囲"),
                    "{error}"
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "server_external_ids.rs"]
mod external_id_tests;

#[cfg(test)]
#[path = "nonnodal_quantity_acceptance_tests.rs"]
mod nonnodal_quantity_acceptance_tests;
