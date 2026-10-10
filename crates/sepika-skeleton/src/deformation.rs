//! M–φ（モーメント–曲率）から M–θ（モーメント–部材角）への参照変換。

use sepika_material::Concrete;

/// 参照変換の不正入力と、根拠未同定の未対応指定。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeformationError {
    InvalidInput(&'static str),
    Unsupported(&'static str),
}

impl std::fmt::Display for DeformationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(reason) => write!(f, "不正入力: {reason}"),
            Self::Unsupported(reason) => write!(f, "未対応: {reason}"),
        }
    }
}
impl std::error::Error for DeformationError {}

pub(crate) fn finite(value: f64, name: &'static str) -> Result<f64, DeformationError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(DeformationError::InvalidInput(name))
    }
}
pub(crate) fn positive(value: f64, name: &'static str) -> Result<f64, DeformationError> {
    finite(value, name)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(DeformationError::InvalidInput(name))
    }
}

pub(crate) fn inflection_length(span_mm: f64, ratio: f64) -> Result<f64, DeformationError> {
    positive(span_mm, "部材長は有限正の mm が必要")?;
    positive(ratio, "反曲点比は有限正が必要")?;
    if ratio > 1.0 {
        return Err(DeformationError::InvalidInput("反曲点比は1以下が必要"));
    }
    positive(span_mm * ratio, "反曲点距離は有限正の mm が必要")
}

/// 一定せん断・反曲点モーメント0の区間の部材角 （rad） を返す。
#[allow(clippy::too_many_arguments)]
pub(crate) fn mphi_to_mtheta(
    curvature_inv_mm: f64,
    moment_n_mm: f64,
    ky_yield: Option<f64>,
    span_mm: f64,
    inflection_ratio: f64,
    plastic_hinge_length_mm: f64,
    shear_add: ShearContribution,
    pullout_rotation_rad: f64,
) -> Result<(f64, f64), DeformationError> {
    finite(curvature_inv_mm, "曲率は有限値が必要")?;
    finite(moment_n_mm, "モーメントは有限値が必要")?;
    finite(pullout_rotation_rad, "抜出し角は有限値が必要")?;
    let l = inflection_length(span_mm, inflection_ratio)?;
    positive(plastic_hinge_length_mm, "塑性ヒンジ長は有限正が必要")?;
    if let Some(ky_y) = ky_yield {
        positive(ky_y, "降伏曲率は有限正が必要")?;
    }
    let theta_f = match ky_yield {
        Some(ky_y) if curvature_inv_mm > ky_y => {
            ky_y * l / 3.0 + (curvature_inv_mm - ky_y) * plastic_hinge_length_mm
        }
        _ => curvature_inv_mm * l / 3.0,
    };
    let theta = theta_f + shear_add.rotation(moment_n_mm, l)? + pullout_rotation_rad;
    Ok((finite(theta, "合成部材角が非有限")?, moment_n_mm))
}

/// 明示的な寄与なし、または等価せん断剛性 Ks=GAs （N）。
#[derive(Clone, Copy, Debug)]
pub enum ShearContribution {
    None,
    Stiffness { k_s: f64 },
}

impl ShearContribution {
    pub fn none() -> Self {
        Self::None
    }
    /// 矩形幅・せい （mm） と有限正の材料剛性から算定。不正入力は失敗する。
    pub fn rc_rect(width: f64, depth: f64, concrete: &Concrete) -> Result<Self, DeformationError> {
        positive(width, "矩形幅は有限正が必要")?;
        positive(depth, "矩形せいは有限正が必要")?;
        positive(concrete.fc, "コンクリート強度は有限正が必要")?;
        if !concrete.ec0.is_finite() || concrete.ec0 >= 0.0 {
            return Err(DeformationError::InvalidInput(
                "圧縮ピークひずみは有限負が必要",
            ));
        }
        let g = positive(concrete.e0_shear(), "せん断弾性係数は有限正が必要")?;
        let k_s = positive(g * (5.0 / 6.0 * width * depth), "せん断剛性は有限正が必要")?;
        Ok(Self::Stiffness { k_s })
    }
    /// M （Nmm）、材端反曲点距離 l （mm） に対する部材角 （rad）。M=Ql が前提。
    pub fn rotation(&self, m: f64, l: f64) -> Result<f64, DeformationError> {
        finite(m, "モーメントは有限値が必要")?;
        positive(l, "反曲点距離は有限正が必要")?;
        match self {
            Self::None => Ok(0.0),
            Self::Stiffness { k_s } => {
                positive(*k_s, "せん断剛性は有限正が必要")?;
                finite((m / l) / k_s, "せん断部材角が非有限")
            }
        }
    }
}

/// 同じ付着モデルの符号付き抜出し s （mm） と正の腕長 z （mm）、その定義。
#[derive(Clone, Debug)]
pub struct PulloutPoint {
    pub slip_mm: f64,
    pub lever_arm_mm: f64,
    pub source: String,
    pub lever_arm_definition: String,
    pub rotation_center: String,
}
impl PulloutPoint {
    /// 抜出し角 s/z （rad）。腕長・出典・回転中心が欠落すれば失敗する。
    pub fn rotation(&self) -> Result<f64, DeformationError> {
        finite(self.slip_mm, "抜出し量は有限値が必要")?;
        positive(self.lever_arm_mm, "抜出し腕長は有限正が必要")?;
        if self.source.trim().is_empty()
            || self.lever_arm_definition.trim().is_empty()
            || self.rotation_center.trim().is_empty()
        {
            return Err(DeformationError::InvalidInput(
                "抜出しモデルの出典・腕長定義・回転中心が必要",
            ));
        }
        finite(self.slip_mm / self.lever_arm_mm, "抜出し角が非有限")
    }
}

/// 正側のひび割れ・降伏・終局点に対応する抜出し入力。点間補間はしない。
#[derive(Clone, Debug)]
pub enum PulloutContribution {
    None,
    Explicit {
        points: Box<[PulloutPoint; 3]>,
    },
    /// σdb/(Esξ) の ξ の単位・校正対象が未同定のため、常に理由付きで拒否する。
    AutomaticBond,
}
impl PulloutContribution {
    /// ひび割れ・降伏・終局の順に、同モデルの明示抜出し入力を指定する。
    pub fn explicit(
        crack: PulloutPoint,
        yield_point: PulloutPoint,
        ultimate: PulloutPoint,
    ) -> Self {
        Self::Explicit {
            points: Box::new([crack, yield_point, ultimate]),
        }
    }

    pub fn none() -> Self {
        Self::None
    }
    pub(crate) fn rotations(&self) -> Result<[f64; 3], DeformationError> {
        match self {
            Self::None => Ok([0.0; 3]),
            Self::AutomaticBond => Err(DeformationError::Unsupported(
                "自動抜出し式のξの単位・校正対象と腕長が未同定。各評価点のs/zを明示する必要がある",
            )),
            Self::Explicit { points } => {
                let angles = [
                    points[0].rotation()?,
                    points[1].rotation()?,
                    points[2].rotation()?,
                ];
                if angles.iter().any(|angle| *angle < 0.0) {
                    return Err(DeformationError::InvalidInput(
                        "正側骨格の抜出し角は非負が必要",
                    ));
                }
                Ok(angles)
            }
        }
    }
}
