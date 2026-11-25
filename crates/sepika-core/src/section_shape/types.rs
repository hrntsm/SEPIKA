//! 断面形状の型定義。

pub(super) fn validate_src_steel_fit(
    b_mm: f64,
    d_mm: f64,
    h_mm: f64,
    w_mm: f64,
    tw_mm: f64,
    tf_mm: f64,
) -> Result<(), String> {
    if h_mm > d_mm {
        return Err(format!(
            "内蔵 H 形鋼のせい {h_mm} mm が SRC 断面せい {d_mm} mm を超えています"
        ));
    }
    if w_mm > b_mm {
        return Err(format!(
            "内蔵 H 形鋼の幅 {w_mm} mm が SRC 断面幅 {b_mm} mm を超えています"
        ));
    }
    if tw_mm > w_mm {
        return Err(format!(
            "内蔵 H 形鋼のウェブ厚 {tw_mm} mm がフランジ幅 {w_mm} mm を超えています"
        ));
    }
    if h_mm <= 2.0 * tf_mm {
        let web_height_mm = h_mm - 2.0 * tf_mm;
        return Err(format!(
            "内蔵 H 形鋼のウェブ内法せい {web_height_mm} mm は正の値が必要です（鋼材せい {h_mm} mm、フランジ厚 {tf_mm} mm）"
        ));
    }
    Ok(())
}

/// SRC内蔵H形断面の製造区分。組立はウェブ・上下フランジに同一材種を使う。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SrcHManufacture {
    #[default]
    Unknown,
    Rolled,
    BuiltUp,
}

impl SrcHManufacture {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "不明",
            Self::Rolled => "圧延H形鋼",
            Self::BuiltUp => "鋼板組立H形断面",
        }
    }
}

/// Parametric section shape definition.
///
/// Each variant carries the minimal parameters needed to define the geometry.
/// Call `to_section()` to compute the derived section properties (A, Iy, Iz, J, ...)
/// and produce a `sepika_core::Section`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SectionShape {
    /// Steel H‑shape (H形鋼).
    SteelH {
        height: f64,
        width: f64,
        web_thick: f64,
        flange_thick: f64,
        /// フィレット半径 `[mm]`。None は未知、Some(0) は明示した直角。
        root_r: Option<f64>,
    },
    /// Steel rectangular hollow section / box (角形鋼管).
    SteelBox {
        height: f64,
        width: f64,
        /// 左右の縦板厚 `[mm]`。
        thick: f64,
        /// 上下の横板厚 `[mm]`。異厚は明示した鋭角形状のみ。
        horizontal_thick: f64,
        /// 角R `[mm]`。None は未知、Some(0) は明示した直角。
        corner_r: Option<f64>,
    },
    /// Steel L‑angle (山形鋼).
    SteelAngle { leg_a: f64, leg_b: f64, thick: f64 },
    /// Steel C‑channel (溝形鋼).
    SteelChannel {
        height: f64,
        width: f64,
        web_thick: f64,
        flange_thick: f64,
    },
    /// Steel T‑shape (T形鋼).
    SteelTee {
        height: f64,
        width: f64,
        web_thick: f64,
        flange_thick: f64,
    },
    /// Steel round pipe (鋼管).
    SteelPipe { outer_dia: f64, thick: f64 },
    /// Steel flat bar / plate (平鋼・鋼板). 中実矩形。
    ///
    /// `width`: 幅 B `[mm]`（Z 方向）、`thick`: 板厚 t `[mm]`（Y 方向＝せい）。
    /// 部材のせい/幅の向きは局所座標（`ref_vector`）で与える。
    SteelFlatBar { width: f64, thick: f64 },
    /// Steel solid round bar (中実丸鋼).
    ///
    /// `dia`: 直径 D `[mm]`。
    SteelRoundBar { dia: f64 },
    /// Steel welded built-up H with unequal flanges (非対称組立 H 形鋼).
    ///
    /// 上下フランジの幅・厚が異なる溶接組立断面。`height`: せい H（外〜外）、
    /// `upper_width`/`upper_thick`: 上フランジ、`lower_width`/`lower_thick`: 下フランジ、
    /// `web_thick`: ウェブ厚。
    SteelBuiltH {
        height: f64,
        upper_width: f64,
        upper_thick: f64,
        lower_width: f64,
        lower_thick: f64,
        web_thick: f64,
    },
    /// Steel cold-formed lipped channel (リップ溝形鋼).
    ///
    /// `height`: せい H `[mm]`（Y 方向）、`width`: フランジ幅 B `[mm]`（Z 方向。ウェブ外面〜
    /// フランジ先端）、`lip`: リップ長 C `[mm]`（Y 方向）、`thick`: 板厚 t `[mm]`（全要素一様）。
    SteelLipChannel {
        height: f64,
        width: f64,
        lip: f64,
        thick: f64,
    },
    /// RC 梁（実配筋）。`b`: 幅 `[mm]`, `d`: せい `[mm]`。
    RcBeamRect { b: f64, d: f64, rebar: RcBeamRebar },
    /// RC 矩形柱（実配筋）。
    RcColumnRect {
        b: f64,
        d: f64,
        rebar: RcRectColumnRebar,
    },
    /// RC 円形柱（実配筋）。
    RcColumnCircle { d: f64, rebar: RcCircleColumnRebar },
    /// SRC 梁（RC 梁 + 内蔵 H 形鉄骨）。
    SrcBeamRect {
        b: f64,
        d: f64,
        rebar: RcBeamRebar,
        steel_height: f64,
        steel_width: f64,
        steel_web_thick: f64,
        steel_flange_thick: f64,
        #[serde(default)]
        steel_manufacture: SrcHManufacture,
    },
    /// SRC 矩形柱（RC 矩形柱 + 内蔵 H 形鉄骨）。
    SrcColumnRect {
        b: f64,
        d: f64,
        rebar: RcRectColumnRebar,
        steel_height: f64,
        steel_width: f64,
        steel_web_thick: f64,
        steel_flange_thick: f64,
        #[serde(default)]
        steel_manufacture: SrcHManufacture,
    },
    /// CFT 角形（角形鋼管 + 充填コンクリート）。検定では `Material.fc` の充填コンクリート強度を用いる。
    CftBox {
        height: f64,
        width: f64,
        /// 左右の縦板厚 `[mm]`。
        thick: f64,
        /// 上下の横板厚 `[mm]`。異厚は明示した鋭角形状のみ。
        horizontal_thick: f64,
        /// 角R `[mm]`。None は未知、Some(0) は明示した直角。
        corner_r: Option<f64>,
    },
    /// CFT 円形（円形鋼管 + 充填コンクリート）。
    CftPipe { outer_dia: f64, thick: f64 },
    /// RC 耐震壁（壁エレメント用）。
    ///
    /// `thickness`: 壁板厚 `[mm]`、`ps`: 壁板の直交する各方向のせん断補強筋比の
    /// うち小さい方（小数。例 0.0025）。`pwh_ratio` は実壁厚に対する横筋比。
    /// `pwv_ratio`: 縦筋面積/(実壁厚×壁長) の小数。方向比の未知は `None`、最小比から推定しない。
    RcWall {
        thickness: f64,
        ps: f64,
        pwh_ratio: Option<f64>,
        pwv_ratio: Option<f64>,
    },
    /// RC スラブ（床）。
    ///
    /// `thickness`: 板厚 `[mm]`。
    RcSlab { thickness: f64 },
}

impl SectionShape {
    /// RC・SRC梁柱の主筋とせん断補強筋の順に返す。対象外の形状は `None`。
    pub fn rebar_sizes(&self) -> Option<(super::RebarSize, super::RebarSize)> {
        match self {
            Self::RcBeamRect { rebar, .. } | Self::SrcBeamRect { rebar, .. } => {
                Some((rebar.main_dia, rebar.stirrup.dia))
            }
            Self::RcColumnRect { rebar, .. } | Self::SrcColumnRect { rebar, .. } => {
                Some((rebar.main_dia, rebar.hoop.dia))
            }
            Self::RcColumnCircle { rebar, .. } => Some((rebar.main_dia, rebar.hoop.dia)),
            _ => None,
        }
    }

    /// 標準異形鉄筋の呼び名を丸鋼材種と組み合わせた場合は理由を返す。
    pub fn validate_rebar_size_materials(
        &self,
        main: Option<&crate::model::Material>,
        shear: Option<&crate::model::Material>,
    ) -> Result<(), String> {
        if let Some((main_size, shear_size)) = self.rebar_sizes() {
            for (role, size, material) in [
                ("主筋", main_size, main),
                ("せん断補強筋", shear_size, shear),
            ] {
                if let super::RebarSize::Standard(size) = size {
                    if let Some(material) =
                        material.filter(|m| matches!(m.name.trim(), "SR235" | "SR295"))
                    {
                        return Err(format!(
                            "{role}の標準異形鉄筋 {size} に丸鋼材種 {} が指定されています。",
                            material.name
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// 組立H形鋼の上下フランジで幅または厚さが異なる場合に true を返す。
    pub fn is_asymmetric_built_h(&self) -> bool {
        matches!(
            self,
            Self::SteelBuiltH {
                upper_width,
                upper_thick,
                lower_width,
                lower_thick,
                ..
            } if upper_width != lower_width || upper_thick != lower_thick
        )
    }

    /// 梁の実配筋を持つ形状はその参照を返す。
    pub fn beam_rebar(&self) -> Option<&RcBeamRebar> {
        match self {
            SectionShape::RcBeamRect { rebar, .. } | SectionShape::SrcBeamRect { rebar, .. } => {
                Some(rebar)
            }
            _ => None,
        }
    }

    /// 矩形柱の実配筋を持つ形状はその参照を返す。
    pub fn rect_column_rebar(&self) -> Option<&RcRectColumnRebar> {
        match self {
            SectionShape::RcColumnRect { rebar, .. }
            | SectionShape::SrcColumnRect { rebar, .. } => Some(rebar),
            _ => None,
        }
    }

    /// 円形柱の実配筋を持つ形状はその参照を返す。
    pub fn circle_column_rebar(&self) -> Option<&RcCircleColumnRebar> {
        match self {
            SectionShape::RcColumnCircle { rebar, .. } => Some(rebar),
            _ => None,
        }
    }

    /// 実配筋型（`RcBeamRect`・`RcColumnRect`・`RcColumnCircle`・`SrcBeamRect`・
    /// `SrcColumnRect`）の幾何検証。実鉄筋座標を生成できない場合は
    /// [`crate::error::RebarGeometryError`] を返す。実配筋を持たない形状は `Ok(())`。
    pub fn validate_rebar(&self) -> Result<(), crate::error::RebarGeometryError> {
        match self {
            SectionShape::RcBeamRect { b, d, rebar } => rebar.validate(*b, *d),
            SectionShape::RcColumnRect { b, d, rebar } => rebar.validate(*b, *d),
            SectionShape::RcColumnCircle { d, rebar } => rebar.validate(*d),
            SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. } => {
                self.validate_src_geometry()
            }
            _ => Ok(()),
        }
    }

    /// 矩形SRCの寸法・内蔵Hと実主筋の干渉を検証する。対象外は成功する。
    pub fn validate_src_geometry(&self) -> Result<(), crate::error::RebarGeometryError> {
        use crate::error::RebarGeometryError;
        let (Self::SrcBeamRect {
            b,
            d,
            steel_height: h,
            steel_width: w,
            steel_web_thick: tw,
            steel_flange_thick: tf,
            ..
        }
        | Self::SrcColumnRect {
            b,
            d,
            steel_height: h,
            steel_width: w,
            steel_web_thick: tw,
            steel_flange_thick: tf,
            ..
        }) = *self
        else {
            return Ok(());
        };
        for (field, value) in [
            ("SRC b", b),
            ("SRC d", d),
            ("SRC Hせい", h),
            ("SRC H幅", w),
            ("SRC web厚", tw),
            ("SRC flange厚", tf),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(RebarGeometryError::InvalidDimension { field });
            }
        }
        validate_src_steel_fit(b, d, h, w, tw, tf).map_err(RebarGeometryError::SectionGeometry)?;
        let web_height_mm = h - 2.0 * tf;
        let as_mm2 = 2.0 * w * tf + tw * web_height_mm;
        for value in [
            b * d - as_mm2,
            as_mm2,
            b * d.powi(3) / 12.0,
            d * b.powi(3) / 12.0,
            w * tf.powi(3) / 12.0,
            tw * web_height_mm.powi(3) / 12.0,
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(RebarGeometryError::SectionGeometry(
                    "SRCの材料面積・断面二次モーメントは正の有限値が必要です".into(),
                ));
            }
        }
        let (points, dia) = match self {
            Self::SrcBeamRect { rebar, .. } => (rebar.bar_positions(b, d)?, rebar.main_dia),
            Self::SrcColumnRect { rebar, .. } => (rebar.bar_positions(b, d)?, rebar.main_dia),
            _ => unreachable!(),
        };
        let diameter_mm = dia.designation_or_diameter_mm();
        let radius = diameter_mm / 2.0;
        for (i, p) in points.iter().enumerate() {
            if p.x.abs() + radius > b / 2.0 || p.y.abs() + radius > d / 2.0 {
                return Err(RebarGeometryError::SectionGeometry(format!(
                    "SRC主筋{}が断面外です",
                    i + 1
                )));
            }
            for (cy, cz, width, height) in [
                (0.0, (h - tf) / 2.0, w, tf),
                (0.0, -(h - tf) / 2.0, w, tf),
                (0.0, 0.0, tw, web_height_mm),
            ] {
                let dy = ((p.x - cy).abs() - width / 2.0).max(0.0);
                let dz = ((p.y - cz).abs() - height / 2.0).max(0.0);
                if dy.hypot(dz) < radius {
                    return Err(RebarGeometryError::SectionGeometry(format!(
                        "SRC主筋{}（径 {diameter_mm} mm、中心位置 ({}, {}) mm）が内蔵 H 形鋼 {h}×{w}×{tw}×{tf} mm と干渉します",
                        i + 1,
                        p.x,
                        p.y
                    )));
                }
            }
        }
        Ok(())
    }

    /// 主材料に鋼材を用いる鋼断面形状か。SRC・CFT は含まない。
    pub fn is_steel(&self) -> bool {
        match self {
            Self::SteelH { .. }
            | Self::SteelBox { .. }
            | Self::SteelAngle { .. }
            | Self::SteelChannel { .. }
            | Self::SteelTee { .. }
            | Self::SteelPipe { .. }
            | Self::SteelFlatBar { .. }
            | Self::SteelRoundBar { .. }
            | Self::SteelBuiltH { .. }
            | Self::SteelLipChannel { .. } => true,
            Self::RcBeamRect { .. }
            | Self::RcColumnRect { .. }
            | Self::RcColumnCircle { .. }
            | Self::SrcBeamRect { .. }
            | Self::SrcColumnRect { .. }
            | Self::CftBox { .. }
            | Self::CftPipe { .. }
            | Self::RcWall { .. }
            | Self::RcSlab { .. } => false,
        }
    }

    /// コンクリート系（RC / SRC / CFT）の断面形状か。
    pub fn is_concrete_like(&self) -> bool {
        matches!(
            self,
            SectionShape::RcBeamRect { .. }
                | SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::SrcBeamRect { .. }
                | SectionShape::SrcColumnRect { .. }
                | SectionShape::CftBox { .. }
                | SectionShape::CftPipe { .. }
                | SectionShape::RcWall { .. }
                | SectionShape::RcSlab { .. }
        )
    }
}

/// 円断面の径 \[mm\] から断面積 `[mm²]`。
pub fn one_bar_area(dia: f64) -> f64 {
    let r = dia / 2.0;
    std::f64::consts::PI * r * r
}

/// RC 梁の実配筋。
///
/// `main_dia`: 主筋の標準呼び名または正の有限な数値径 `[mm]`（全段共通）。`top`/`bottom`: 上端筋・下端筋の
/// かぶり側から内側へ数えた段別本数。`cover`: かぶり `[mm]`。`stirrup`: あばら筋。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RcBeamRebar {
    pub main_dia: super::RebarSize,
    pub top: Vec<u32>,
    pub bottom: Vec<u32>,
    pub cover: f64,
    pub stirrup: BeamStirrup,
}

impl RcBeamRebar {
    /// 上端筋・下端筋とも未入力（段が空）か。
    pub fn is_unset(&self) -> bool {
        self.top.is_empty() && self.bottom.is_empty()
    }
}

/// RC 矩形柱の実配筋。
///
/// `main_dia`: 主筋の標準呼び名または正の有限な数値径 `[mm]`（全段共通）。`x`/`y`: X 方向・Y 方向の段別本数。
/// 各段は断面中心対称な 2 本の配筋線を 1 段とし、本数は片側 1 線あたり。
/// `cover`: かぶり `[mm]`。`hoop`: 帯筋。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RcRectColumnRebar {
    pub main_dia: super::RebarSize,
    pub x: Vec<u32>,
    pub y: Vec<u32>,
    pub cover: f64,
    pub hoop: RectColumnHoop,
}

impl RcRectColumnRebar {
    /// X 方向・Y 方向とも未入力（段が空）か。
    pub fn is_unset(&self) -> bool {
        self.x.is_empty() && self.y.is_empty()
    }
}

/// RC 円形柱の実配筋。
///
/// `main_dia`: 主筋の標準呼び名または正の有限な数値径 `[mm]`。`count`: 主筋総本数（円周上へ等間隔配置）。
/// `cover`: かぶり `[mm]`。`hoop`: 帯筋。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RcCircleColumnRebar {
    pub main_dia: super::RebarSize,
    pub count: u32,
    pub cover: f64,
    pub hoop: CircleColumnHoop,
}

impl RcCircleColumnRebar {
    /// 主筋総本数が未入力（0）か。
    pub fn is_unset(&self) -> bool {
        self.count == 0
    }
}

/// RC 梁のあばら筋。
///
/// `dia`: 標準呼び名または数値径 `[mm]`。`pitch`: 間隔 `[mm]`。`legs`: 脚数。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BeamStirrup {
    pub dia: super::RebarSize,
    pub pitch: f64,
    pub legs: u32,
}

/// RC 矩形柱の帯筋。
///
/// `dia`: 標準呼び名または数値径 `[mm]`。`pitch`: 間隔 `[mm]`。`legs_x`/`legs_y`: X 方向・Y 方向の脚数。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RectColumnHoop {
    pub dia: super::RebarSize,
    pub pitch: f64,
    pub legs_x: u32,
    pub legs_y: u32,
}

/// RC 円形柱の帯筋。
///
/// `dia`: 標準呼び名または数値径 `[mm]`。`pitch`: 間隔 `[mm]`。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CircleColumnHoop {
    pub dia: super::RebarSize,
    pub pitch: f64,
}

/// 主筋 1 本の断面内座標。
///
/// `x`: 幅 b 方向 `[mm]`、`y`: せい d 方向 `[mm]`。断面中心を原点とする。
/// 段別配筋から都度生成する派生値であり、モデルへは保存しない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RebarPoint {
    pub x: f64,
    pub y: f64,
}

impl SectionShape {
    /// 角形鋼管・角形CFTの（左右縦板厚、上下横板厚）`[mm]`。他形状は `None`。
    pub fn box_thicknesses(&self) -> Option<(f64, f64)> {
        match *self {
            Self::SteelBox {
                thick,
                horizontal_thick,
                ..
            }
            | Self::CftBox {
                thick,
                horizontal_thick,
                ..
            } => Some((thick, horizontal_thick)),
            _ => None,
        }
    }

    pub fn has_unequal_box_thickness(&self) -> bool {
        self.box_thicknesses().is_some_and(|(t1, t2)| t1 != t2)
    }
}
