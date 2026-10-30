//! 部位別の数量算定式（概算数量の拾い出し規則）。
//!
//! 入力は内部単位系（mm）、出力も mm 系（mm³ / mm² / mm / 個所）。
//! 単位換算（m³・m²・t）は呼び出し側（[`super::try_compute_quantity_takeoff`]）で行う。
//!
//! - [`Haunch`] — 梁端ハンチ寸法
//! - [`girder_concrete_volume`] — 大梁・基礎梁のコンクリート体積
//! - [`girder_formwork_area`] / [`foundation_girder_formwork_area`] — 型枠面積
//! - [`girder_main_bar_length`] — 大梁主筋 1 本の長さ（1断面・全断面）
//! - [`stirrup_set_length`] / [`shear_bar_count`] — スターラップ
//! - [`beam_joint_count`] / [`column_joint_count`] — 鉄筋継手個所数
//! - [`column_concrete_volume`] / [`column_formwork_area`] / [`hoop_set_length`] — 柱
//! - [`beam_concrete_volume`] / [`beam_formwork_area`] — 小梁
//! - [`wall_bar_length`] — 壁筋（横筋・縦筋）総長さ

/// フェイスでの全幅 Bi・全せい Di と、フェイスから内側へのハンチ長 Li `[mm]`。
/// 未入力端は呼出側で `None` とする。全寸法は基準断面への増分を含む。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Haunch {
    /// フェイスでの全幅 Bi `[mm]`。
    pub b: f64,
    /// フェイスでの全せい Di `[mm]`。
    pub d: f64,
    /// ハンチ長さ Li `[mm]`。
    pub len: f64,
}

/// 大梁・基礎梁のコンクリート体積 `[mm³]`。寸法・内法全長は `[mm]`。
/// ハンチの幅・せいはフェイスの全寸法から基準寸法へ線形に変化する。
/// 有限・非負の増分、正のハンチ長、内法内で重ならない両端区間を前提とする。
pub fn girder_concrete_volume(
    b: f64,
    d: f64,
    l: f64,
    haunch_i: Option<Haunch>,
    haunch_j: Option<Haunch>,
) -> f64 {
    let mut v = b * d * l;
    for h in [haunch_i, haunch_j].into_iter().flatten() {
        let delta_b = h.b - b;
        let delta_d = h.d - d;
        v += ((b * delta_d + d * delta_b) / 2.0 + delta_b * delta_d / 3.0) * h.len;
    }
    v
}

/// 大梁の型枠面積 `[mm²]`（両側スラブ厚が等しい場合の式）。
///
/// `(D×L + (Di−D)/2×Li + (Dj−D)/2×Lj)×2 + B×L + (Bi−B)/2×Li + (Bj−B)/2×Lj`
///
/// 側面のせい `D`（および `Di`, `Dj`）はスラブの有無によりスラブ厚さを
/// 控除した値を渡すこと（両側スラブ付は両面 `D−t`、片側スラブ付は
/// 片面 `D`・片面 `D−t`、スラブなしは両面 `D`。呼び出し側で
/// `d_side1`/`d_side2` として与える）。ハンチ部の型枠は傾斜を無視し
/// 底面・側面への投影面積で計算する。
pub fn girder_formwork_area(
    b: f64,
    d_side1: f64,
    d_side2: f64,
    l: f64,
    haunch_i: Option<Haunch>,
    haunch_j: Option<Haunch>,
) -> f64 {
    let mut area = (d_side1 + d_side2) * l;
    for h in [haunch_i, haunch_j].into_iter().flatten() {
        area += (h.d - d_side1.max(d_side2)).max(0.0) / 2.0 * h.len * 2.0;
    }
    area += b * l;
    for h in [haunch_i, haunch_j].into_iter().flatten() {
        area += (h.b - b).max(0.0) / 2.0 * h.len;
    }
    area
}

/// 基礎梁の型枠面積 `[mm²]`。
///
/// `D×L + (Di−D)/2×Li + (Dj−D)/2×Lj + B×L + (Bi−B)×Li + (Bj−B)×Lj`
///
/// 側面は 1 面分＋底面で算定する（大梁と異なり
/// 側面の ×2 は付かない。地中梁は掘削面・耐圧版側が型枠不要となる
/// 取り扱い）。`d_side` はスラブ（耐圧版含む）厚さ控除後のせい。
pub fn foundation_girder_formwork_area(
    b: f64,
    d_side: f64,
    l: f64,
    haunch_i: Option<Haunch>,
    haunch_j: Option<Haunch>,
) -> f64 {
    let mut area = d_side * l + b * l;
    for h in [haunch_i, haunch_j].into_iter().flatten() {
        area += (h.d - d_side).max(0.0) / 2.0 * h.len;
        area += (h.b - b).max(0.0) * h.len;
    }
    area
}

/// 梁端部の主筋定着条件。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BeamBarEnd {
    /// 外端（外柱へ定着）: 延長 = 定着長さ L2。
    Exterior {
        /// 定着長さ L2 `[mm]`。
        l2: f64,
    },
    /// 内端（内柱を通し）: 延長 = 柱せいの半分 Dc/2。
    Interior {
        /// 柱せいの半分 Dc/2 `[mm]`。
        half_dc: f64,
    },
}

impl BeamBarEnd {
    /// 内法端からの延長長さ [mm]。
    fn extension(self) -> f64 {
        match self {
            BeamBarEnd::Exterior { l2 } => l2,
            BeamBarEnd::Interior { half_dc } => half_dc,
        }
    }
}

/// 大梁主筋 1 本の長さ `[mm]`（1 断面＝全断面配筋の場合）。
///
/// 主筋長さの算定タイプ:
/// - タイプ7（外端-内端）: `L = Lo + L2 + Dc/2`
/// - タイプ7a（両外端）:  `L = Lo + L2×2`
/// - タイプ8（両内端）:   `L = Lo + Dc/2×2`
///
/// 現状の部材モデルは全長 1 断面（端部・中央の断面分割なし）のため、
/// 2断面・3断面の端部筋・中央筋（タイプ1〜6、カットオフ +15d）は
/// 使用しない。
pub fn girder_main_bar_length(lo: f64, end_i: BeamBarEnd, end_j: BeamBarEnd) -> f64 {
    lo + end_i.extension() + end_j.extension()
}

/// スターラップ一組の長さ `[mm]`: `2×B + n×D`（n: 一組のスターラップ本数）。
pub fn stirrup_set_length(b: f64, d: f64, legs: u32) -> f64 {
    2.0 * b + legs as f64 * d
}

/// せん断補強筋（スターラップ・フープ）の本数（組数）: `L/ピッチ`。
/// ピッチが 0 以下の場合は 0。
pub fn shear_bar_count(length: f64, pitch: f64) -> f64 {
    if pitch <= 0.0 {
        return 0.0;
    }
    (length / pitch).max(0.0)
}

/// 梁（大梁・基礎梁）の主筋 1 本あたり鉄筋継手個所数。
///
/// 梁毎に 0.5 個所の継手があるものとみなし、梁の長さ L が 5.0m 以上の
/// 場合は 5.0m 毎に 0.5 個所を加える。
pub fn beam_joint_count(l: f64) -> f64 {
    0.5 + 0.5 * (l / 5_000.0).floor()
}

/// 柱の柱頭・柱脚通し主筋 1 本あたり鉄筋継手個所数。
///
/// 階ごとに 1 個所の継手があるものとみなし、階高が 7.0m 以上の場合は
/// 7.0m 毎にさらに 1 個所を加える。
pub fn column_joint_count(h: f64) -> f64 {
    1.0 + (h / 7_000.0).floor()
}

/// 柱のコンクリート体積 `[mm³]`: `Dx×Dy×H`（H: 床上〜床上の柱長さ）。
pub fn column_concrete_volume(dx: f64, dy: f64, h: f64) -> f64 {
    dx * dy * h
}

/// 柱の型枠面積 `[mm²]`: `2×(Dx+Dy)×H`。
pub fn column_formwork_area(dx: f64, dy: f64, h: f64) -> f64 {
    2.0 * (dx + dy) * h
}

/// 柱フープ一組の長さ `[mm]`: `nx×Dx + ny×Dy`
/// （nx/ny: X/Y 方向一組のフープ本数）。
pub fn hoop_set_length(dx: f64, dy: f64, nx: u32, ny: u32) -> f64 {
    nx as f64 * dx + ny as f64 * dy
}

/// 小梁のコンクリート体積 `[mm³]`: `B×D×L`。
pub fn beam_concrete_volume(b: f64, d: f64, l: f64) -> f64 {
    b * d * l
}

/// 小梁の型枠面積 `[mm²]`: `(B+2×D)×L`。
pub fn beam_formwork_area(b: f64, d: f64, l: f64) -> f64 {
    (b + 2.0 * d) * l
}

/// 壁筋（横筋または縦筋）の総長さ `[mm]`。
///
/// - 横筋: `(L+2S)×W`（W: 横筋本数 = 壁高さ/ピッチ × 配筋列数）
/// - 縦筋: `(H+2S)×h`（h: 縦筋本数 = 壁長さ/ピッチ × 配筋列数）
///
/// `span` に L（横筋）または H（縦筋）、`count` に本数 W／h、
/// `s` に定着長さ S（35d）を与える。
pub fn wall_bar_length(span: f64, s: f64, count: f64) -> f64 {
    (span + 2.0 * s) * count
}
/// 円形柱の体積 `[mm³]`。直径・長さ `[mm]` は有限かつ正を前提とする。
pub fn circle_column_concrete_volume(diameter_mm: f64, length_mm: f64) -> f64 {
    std::f64::consts::PI * diameter_mm * diameter_mm / 4.0 * length_mm
}

/// 円形柱の側面型枠面積 `[mm²]`。直径・長さ `[mm]` は有限かつ正を前提とする。
pub fn circle_column_formwork_area(diameter_mm: f64, length_mm: f64) -> f64 {
    std::f64::consts::PI * diameter_mm * length_mm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_girder_formwork_slab_deduction() {
        let area_mm2 = girder_formwork_area(400.0, 800.0, 650.0, 6_000.0, None, None);
        assert_eq!(area_mm2, 11_100_000.0);
    }

    #[test]
    fn test_girder_main_bar_length_types() {
        let lo = 6_000.0;
        // タイプ7: 外端-内端 = Lo + L2 + Dc/2
        let t7 = girder_main_bar_length(
            lo,
            BeamBarEnd::Exterior { l2: 875.0 },
            BeamBarEnd::Interior { half_dc: 450.0 },
        );
        assert!((t7 - (lo + 875.0 + 450.0)).abs() < 1e-9);
        // タイプ7a: 両外端 = Lo + L2×2
        let t7a = girder_main_bar_length(
            lo,
            BeamBarEnd::Exterior { l2: 875.0 },
            BeamBarEnd::Exterior { l2: 875.0 },
        );
        assert!((t7a - (lo + 2.0 * 875.0)).abs() < 1e-9);
        // タイプ8: 両内端 = Lo + Dc/2×2
        let t8 = girder_main_bar_length(
            lo,
            BeamBarEnd::Interior { half_dc: 450.0 },
            BeamBarEnd::Interior { half_dc: 450.0 },
        );
        assert!((t8 - (lo + 2.0 * 450.0)).abs() < 1e-9);
    }

    #[test]
    fn test_joint_counts() {
        // 梁: L<5m → 0.5、5m ≤ L <10m → 1.0、10m → 1.5
        assert!((beam_joint_count(4_000.0) - 0.5).abs() < 1e-12);
        assert!((beam_joint_count(5_000.0) - 1.0).abs() < 1e-12);
        assert!((beam_joint_count(9_999.0) - 1.0).abs() < 1e-12);
        assert!((beam_joint_count(10_000.0) - 1.5).abs() < 1e-12);
        // 柱: H<7m → 1、7m ≤ H <14m → 2
        assert!((column_joint_count(3_500.0) - 1.0).abs() < 1e-12);
        assert!((column_joint_count(7_000.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn test_stirrup_and_hoop() {
        // スターラップ一組 = 2×B + n×D
        assert!((stirrup_set_length(400.0, 800.0, 2) - (800.0 + 1_600.0)).abs() < 1e-12);
        // フープ一組 = nx×Dx + ny×Dy
        assert!((hoop_set_length(700.0, 700.0, 2, 2) - 2_800.0).abs() < 1e-12);
        // 本数 = L/ピッチ
        assert!((shear_bar_count(3_000.0, 100.0) - 30.0).abs() < 1e-12);
        assert_eq!(shear_bar_count(3_000.0, 0.0), 0.0);
    }
}
