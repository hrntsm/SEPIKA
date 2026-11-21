//! 解析ジョブのエラー型。

use thiserror::Error;

/// 解析ジョブのエラー。
#[derive(Debug, Error)]
pub enum JobError {
    /// 等価骨格未同定。モデルを生成せず、元解析条件付き診断を返す。
    #[error("{0}")]
    LumpedIdentification(Box<sepika_solver::dynamic::lumped_mass::LumpedMassIdentificationFailure>),
    /// 解析準備（自由度写像・拘束の縮約・モデル整合性）で失敗した。
    #[error("解析準備エラー: {0}")]
    Prepare(String),

    /// 連立方程式の求解で失敗した（特異・非正定値など）。
    #[error("解析エラー: {0}")]
    Solve(String),

    /// 非線形解析が収束しなかった。
    #[error("収束しませんでした: {0}")]
    Convergence(String),

    /// 入力（モデル・解析条件）が解析の前提を満たさない。
    #[error("入力不備: {0}")]
    InvalidInput(String),
}

/// 解析ジョブの結果型。
pub type JobResult<T> = Result<T, JobError>;
