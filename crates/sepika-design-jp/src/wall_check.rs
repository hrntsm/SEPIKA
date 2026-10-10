//! 壁版・荷重ケース・検定種別を保持する壁検定結果。
use crate::CheckOutcome;
use sepika_core::ids::{ElemId, NodeId, WallPlateId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallCheckKind {
    AllowableShear,
    ReferenceSkeleton,
}
impl WallCheckKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::AllowableShear => "許容せん断",
            Self::ReferenceSkeleton => "せん断参考骨格",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallSkipKind {
    MissingInput,
    InvalidInput,
    NotApplicable,
    NotImplemented,
    MissingResponse,
}

impl WallSkipKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingInput => "入力不足",
            Self::InvalidInput => "不正入力",
            Self::NotApplicable => "適用外",
            Self::NotImplemented => "未実装",
            Self::MissingResponse => "応答未取得",
        }
    }
}

/// `seismic_target=false` は自重・雑壁のみの対象外であり、未検定耐震壁の集計へ含めない。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WallCheck {
    pub plate: Option<WallPlateId>,
    pub elem: Option<ElemId>,
    pub node: Option<NodeId>,
    pub case: String,
    pub kind: WallCheckKind,
    pub seismic_target: bool,
    pub skip_kind: Option<WallSkipKind>,
    pub outcome: CheckOutcome,
}
impl WallCheck {
    pub fn label(&self) -> String {
        format!(
            "壁版 {} / 要素 {} / {} / {}",
            self.plate
                .map(|id| id.0.to_string())
                .unwrap_or_else(|| "-".into()),
            self.elem
                .map(|id| id.0.to_string())
                .unwrap_or_else(|| "-".into()),
            self.case,
            self.kind.label()
        )
    }
}

#[derive(Default, Debug, serde::Serialize)]
pub struct WallCheckSummary {
    pub n_walls: usize,
    pub n_ok_walls: usize,
    pub n_ng_walls: usize,
    pub n_skipped_walls: usize,
    pub n_checks: usize,
    pub n_ok: usize,
    pub n_ng: usize,
    pub n_skipped: usize,
    pub n_outside: usize,
    pub max_ratio: Option<f64>,
}
impl WallCheckSummary {
    pub fn for_kind(checks: &[WallCheck], kind: WallCheckKind) -> Self {
        Self::from_checks(
            &checks
                .iter()
                .filter(|w| w.kind == kind)
                .cloned()
                .collect::<Vec<_>>(),
        )
    }
    pub fn from_checks(checks: &[WallCheck]) -> Self {
        let mut summary = Self::default();
        let mut walls = std::collections::HashMap::<_, (bool, bool)>::new();
        for check in checks {
            if !check.seismic_target {
                summary.n_outside += 1;
                continue;
            }
            let wall = walls.entry((check.plate, check.elem)).or_default();
            summary.n_checks += 1;
            match &check.outcome {
                CheckOutcome::Checked(cr) if !cr.components.is_empty() => {
                    if cr.ok() {
                        summary.n_ok += 1;
                    } else {
                        summary.n_ng += 1;
                        wall.0 = true;
                    }
                    summary.max_ratio =
                        Some(summary.max_ratio.map_or(cr.ratio(), |r| r.max(cr.ratio())));
                }
                _ => {
                    summary.n_skipped += 1;
                    wall.1 = true;
                }
            }
        }
        summary.n_walls = walls.len();
        for (ng, skipped) in walls.values() {
            if *ng {
                summary.n_ng_walls += 1;
            } else if *skipped {
                summary.n_skipped_walls += 1;
            } else {
                summary.n_ok_walls += 1;
            }
        }
        summary
    }
}
