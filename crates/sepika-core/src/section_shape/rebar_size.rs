use serde::{Deserialize, Serialize};
use std::fmt;

/// 対応する標準異形鉄筋の呼び名。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StandardRebar {
    D10,
    D13,
    D16,
    D19,
    D22,
    D25,
    D29,
    D32,
    D35,
    D38,
    D41,
}

impl StandardRebar {
    /// 入力で選択できる呼び名を小さい順に返す。
    pub const ALL: [Self; 11] = [
        Self::D10,
        Self::D13,
        Self::D16,
        Self::D19,
        Self::D22,
        Self::D25,
        Self::D29,
        Self::D32,
        Self::D35,
        Self::D38,
        Self::D41,
    ];

    /// 呼び名に含まれる数値。
    pub const fn designation_number(self) -> f64 {
        match self {
            Self::D10 => 10.0,
            Self::D13 => 13.0,
            Self::D16 => 16.0,
            Self::D19 => 19.0,
            Self::D22 => 22.0,
            Self::D25 => 25.0,
            Self::D29 => 29.0,
            Self::D32 => 32.0,
            Self::D35 => 35.0,
            Self::D38 => 38.0,
            Self::D41 => 41.0,
        }
    }

    /// 公称直径 \[mm\]。
    pub const fn nominal_diameter_mm(self) -> f64 {
        self.nominal_dimensions().0
    }

    /// 公称断面積 \[mm²\]。
    pub const fn area_mm2(self) -> f64 {
        self.nominal_dimensions().1
    }

    /// 公称周長 \[mm\]。
    pub const fn perimeter_mm(self) -> f64 {
        self.nominal_dimensions().2
    }

    const fn nominal_dimensions(self) -> (f64, f64, f64) {
        match self {
            Self::D10 => (9.53, 71.33, 29.9),
            Self::D13 => (12.7, 126.7, 39.9),
            Self::D16 => (15.9, 198.6, 50.0),
            Self::D19 => (19.1, 286.5, 60.0),
            Self::D22 => (22.2, 387.1, 69.8),
            Self::D25 => (25.4, 506.7, 79.8),
            Self::D29 => (28.6, 642.4, 89.9),
            Self::D32 => (31.8, 794.2, 99.9),
            Self::D35 => (34.9, 956.6, 109.7),
            Self::D38 => (38.1, 1140.0, 119.7),
            Self::D41 => (41.3, 1340.0, 129.8),
        }
    }
}

impl fmt::Display for StandardRebar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "D{}", self.designation_number())
    }
}

/// RC・SRC梁柱の鉄筋寸法。標準呼び名は文字列、数値径 \[mm\] は数値として入出力する。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RebarSize {
    Standard(StandardRebar),
    DiameterMm(f64),
}

impl RebarSize {
    /// 規準で呼び名の数値を指定する項および配筋配置に用いる値 \[mm\]。
    pub const fn designation_or_diameter_mm(self) -> f64 {
        match self {
            Self::Standard(size) => size.designation_number(),
            Self::DiameterMm(diameter_mm) => diameter_mm,
        }
    }

    /// 鉄筋1本の断面積 \[mm²\]。数値径は円断面とする。
    pub fn area_mm2(self) -> f64 {
        match self {
            Self::Standard(size) => size.area_mm2(),
            Self::DiameterMm(diameter_mm) => super::one_bar_area(diameter_mm),
        }
    }

    /// 付着計算に使う鉄筋1本の周長 \[mm\]。数値径は円断面とする。
    pub fn perimeter_mm(self) -> f64 {
        match self {
            Self::Standard(size) => size.perimeter_mm(),
            Self::DiameterMm(diameter_mm) => std::f64::consts::PI * diameter_mm,
        }
    }
}

impl fmt::Display for RebarSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard(size) => size.fmt(f),
            Self::DiameterMm(diameter_mm) => write!(f, "{diameter_mm} mm"),
        }
    }
}
