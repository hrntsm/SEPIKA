use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HighStrengthBolt {
    F10T,
    F8TGalvanized,
}

impl HighStrengthBolt {
    pub fn label(self) -> &'static str {
        match self {
            Self::F10T => "F10T",
            Self::F8TGalvanized => "F8T（溶融亜鉛めっき）",
        }
    }
}

/// 同じ断面符号を使用する小梁の各支持端に適用する接合条件。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondaryBeamBoltConnection {
    pub bolt: HighStrengthBolt,
    pub diameter_mm: u32,
    pub friction_surfaces: u32,
    /// 片端のボルト本数。0は未設定。
    pub count: u32,
    pub surface_treatment_confirmed: bool,
    pub concentric_shear_only_confirmed: bool,
}

impl Default for SecondaryBeamBoltConnection {
    fn default() -> Self {
        Self {
            bolt: HighStrengthBolt::F10T,
            diameter_mm: 20,
            friction_surfaces: 1,
            count: 0,
            surface_treatment_confirmed: false,
            concentric_shear_only_confirmed: false,
        }
    }
}
