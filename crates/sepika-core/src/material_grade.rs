//! 材料グレード名（`SS400`・`SD345`・`Fc21` 等）から規格値を解決する対応表。
//!
//! 鋼材の基準強度 F・鉄筋の基準強度・コンクリート設計基準強度 Fc の
//! 「名称 → 数値」対応は本モジュールに一本化する（H12 建告第2464号、
//! JIS 規格、大臣認定品の基準強度）。設計計算（sepika-design-jp）・
//! UI プリセット（sepika-app）は
//! いずれも本対応表を参照し、独立の表を持たない。
//!
//! - [`steel_f_value`] — 鋼材の基準強度 F（完全一致、板厚区分対応）
//! - [`steel_f_value_prefix`] — 同（対応する種別記号付き名称。`SN490B` → `SN490` 等）
//! - [`resolve_steel_f`] — 割当鋼材と製品実板厚からの基準強度解決
//! - [`rebar_f_value`] — 鉄筋の基準強度（`SD345` → 345 等）
//! - [`rebar_grade_f_value`] — 同（高強度鉄筋の製品名を含む。`KH785` → 785 等）
//! - [`rebar_yield_strength`] / [`shear_rebar_yield_strength`] — 配筋の材質からの σy・σwy 解決
//! - [`is_supported_shear_rebar_grade`] — せん断補強筋として対応するグレードかどうかの判定
//! - [`shear_rebar_material_issue`] — 未対応グレード・`fy` 未設定をまとめた材料不備の判定
//! - [`parse_concrete_fc`] — コンクリート `FcXX` 名称の解釈
//! - [`material_presets`] — UI に提示する標準材料プリセット一覧

use crate::model::{Material, MaterialCategory};
use crate::section_shape::{concrete_young_modulus_gamma, E_STEEL};
use crate::units::{
    concrete_unit_weight_kn_m3, to_internal::mass_density_from_unit_weight_kn_m3, ConcreteClass,
    ConcreteComposition, STEEL_MASS_DENSITY_TON_MM3,
};

/// 板厚 2 区分（`t<=40` / `40<t<=100`）の F 値を返す。
/// 100mm 超は規定がないため最終区分値をそのまま用いる（非保守的になり得る）。
fn bucket2(t: f64, le40: f64, gt40: f64) -> f64 {
    if t <= 40.0 {
        le40
    } else {
        gt40
    }
}

/// 鋼材の基準強度表で扱う分類名一覧。種別記号付きの対応名は別途解決する。
pub const STEEL_GRADES: &[&str] = &[
    "SS400", "SS490", "SM400", "SM490", "SM520", "SN400", "SN490", "STK400", "STK490", "STKN400",
    "STKN490", "STKR400", "STKR490", "SNR400", "SNR490", "SSC400", "SWH400", "BCR235", "BCR295",
    "BCP235", "BCP325", "TMCP325", "TMCP355", "TMCP385", "TMCP440", "SA440", "LY100", "LY225",
];

/// 鋼材の基準強度 F [N/mm²]。板厚は mm で指定する。
/// SM520 は有効厚さ 100 mm 以下の 3 区分を返し、範囲外は `None`。
/// その他の対応鋼種・認定品の板厚区分は鋼種ごとに適用する。
pub fn steel_f_value(grade: &str, thickness: f64) -> Option<f64> {
    match grade {
        "SS400" | "SM400" | "SN400" | "STK400" | "STKN400" | "STKR400" | "SNR400" | "SSC400"
        | "SWH400" => Some(bucket2(thickness, 235.0, 215.0)),
        "SS490" => Some(bucket2(thickness, 275.0, 255.0)),
        "SM490" | "SN490" | "STK490" | "STKN490" | "STKR490" | "SNR490" => {
            Some(bucket2(thickness, 325.0, 295.0))
        }
        "SM520" if thickness.is_finite() && thickness > 0.0 && thickness <= 100.0 => {
            Some(if thickness <= 40.0 {
                355.0
            } else if thickness <= 75.0 {
                335.0
            } else {
                325.0
            })
        }
        "SM520" => None,
        "BCR295" => Some(295.0),
        "BCR235" => Some(235.0),
        "BCP235" => Some(235.0),
        "BCP325" => Some(325.0),
        "TMCP325" => Some(325.0),
        "TMCP355" => Some(355.0),
        "TMCP385" => Some(385.0),
        "TMCP440" => Some(440.0),
        "SA440" => Some(440.0),
        "LY100" => Some(80.0),
        "LY225" => Some(205.0),
        _ => None,
    }
}

/// 対応する鋼材名から基準強度表の分類名を返す。前後空白は除き、大小文字は区別する。
pub fn steel_grade_base(name: &str) -> Option<&'static str> {
    let name = name.trim();
    STEEL_GRADES.iter().copied().find(|base| {
        name == *base
            || name.strip_prefix(base).is_some_and(|suffix| match *base {
                "SN400" | "SM400" => matches!(suffix, "A" | "B" | "C"),
                "SN490" => matches!(suffix, "B" | "C"),
                "SM490" => matches!(suffix, "A" | "B" | "C" | "YA" | "YB"),
                "SM520" => matches!(suffix, "B" | "C"),
                "STKN400" | "STKN490" => matches!(suffix, "B" | "W"),
                "SNR400" => matches!(suffix, "A" | "B"),
                "SNR490" => suffix == "B",
                _ => false,
            })
    })
}

/// 鋼材の基準強度 F [N/mm²]（板厚 `[mm]` 区分対応）。
/// 対応済みの分類名・種別記号付き名称のみ解決し、任意の前方一致は行わない。
/// 前後空白は除き、大小文字は区別する。未知の名称は `None`。
pub fn steel_f_value_prefix(name: &str, thickness: f64) -> Option<f64> {
    steel_grade_base(name).and_then(|grade| steel_f_value(grade, thickness))
}

/// 対応済み SM520 系列の板厚 `[mm]` が適用範囲外の場合に、未算定理由を返す。
/// 未知の名称と他鋼種は `None`。
pub fn steel_f_value_issue(name: &str, thickness: f64) -> Option<String> {
    (steel_grade_base(name) == Some("SM520")
        && (!thickness.is_finite() || thickness <= 0.0 || thickness > 100.0))
        .then(|| format!("鋼材「{}」の板厚 {thickness} mm は SM520 系列の基準強度 F の適用範囲（0 < t ≤ 100 mm）外です。明示 fy や既定強度では補完できません", name.trim()))
}

/// 割当鋼材と製品板厚 `[mm]` から F `[N/mm²]` を求める。既知材種は表、未知名は正有限の fy を用いる。
/// 材料不備・既知材種の必要板厚欠落や表の適用範囲外は理由付きエラー。
pub fn resolve_steel_f(mat: Option<&Material>, thickness_mm: Option<f64>) -> Result<f64, String> {
    let mat = mat.ok_or("鋼材が未割当、または参照先が不明です")?;
    if mat.category != MaterialCategory::Steel {
        return Err(format!("材料「{}」は鋼材ではありません", mat.name));
    }
    let Some(grade) = steel_grade_base(&mat.name) else {
        return mat
            .fy
            .filter(|fy| fy.is_finite() && *fy > 0.0)
            .ok_or_else(|| {
                format!(
                    "直接入力鋼材「{}」の基準強度 fy は正の有限値で入力してください",
                    mat.name
                )
            });
    };
    let thickness_mm = thickness_mm.filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| {
        format!("鋼材「{}」の基準強度 F に必要な実板厚が未入力または不正です。正の有限値 [mm] を入力してください", mat.name)
    })?;
    if let Some(reason) = steel_f_value_issue(&mat.name, thickness_mm) {
        return Err(reason);
    }
    if thickness_mm > 100.0
        && matches!(
            grade,
            "SS400"
                | "SM400"
                | "SN400"
                | "STK400"
                | "STKN400"
                | "STKR400"
                | "SNR400"
                | "SSC400"
                | "SWH400"
                | "SS490"
                | "SM490"
                | "SN490"
                | "STK490"
                | "STKN490"
                | "STKR490"
                | "SNR490"
        )
    {
        return Err(format!("鋼材「{}」の実板厚 {thickness_mm} mm は基準強度 F の適用範囲（0 < t ≤ 100 mm）外です。明示 fy や既定強度では補完できません", mat.name));
    }
    steel_f_value(grade, thickness_mm).ok_or_else(|| {
        format!(
            "鋼材「{}」の実板厚 {thickness_mm} mm に対する基準強度 F が未定義です",
            mat.name
        )
    })
}

/// SRC 内蔵 H 形鋼に適用できる材種の最小板厚 \[mm\]。分類・材種が不適合なら理由を返す。
pub fn src_h_steel_minimum_thickness_mm(steel: &Material) -> Result<f64, String> {
    if steel.category != MaterialCategory::Steel {
        return Err(format!("材料「{}」は鋼材ではありません", steel.name));
    }
    let name = steel.name.trim();
    let minimum_mm = match name {
        "SN400A" | "SN400B" | "SN490B" => 6.0,
        "SN400C" | "SN490C" => 16.0,
        "SS400" | "SM400A" | "SM400B" | "SM400C" | "SM490A" | "SM490B" | "SM490C" | "SM490YA"
        | "SM490YB" | "SM520B" | "SM520C" => 0.0,
        "SS490" => {
            return Err("内蔵鉄骨「SS490」は溶接を使用する部分に適用できません。断面入力から溶接を使用する部分か確認できません".into());
        }
        "SM400" | "SM490" | "SM520" | "SN400" | "SN490" => {
            return Err(format!("内蔵鉄骨「{name}」の JIS の種類記号が不完全です"));
        }
        "STK400" | "STK490" | "STKR400" | "STKR490" | "STKN400" | "STKN400W" | "STKN400B"
        | "STKN490" | "STKN490W" | "STKN490B" | "BCR235" | "BCR295" | "BCP235" | "BCP325" => {
            return Err(format!(
                "内蔵鉄骨「{name}」は鋼管用として登録されている材料名で、H 形鋼としての適用根拠が未確定です"
            ));
        }
        _ => {
            return Err(format!(
                "内蔵鉄骨「{name}」の H 形鋼としての適用根拠が未確定です。材料名と直接入力した降伏応力だけでは個別認定の番号・対象形状・板厚・基準強度を確認できません"
            ));
        }
    };
    Ok(minimum_mm)
}

/// ウェブとフランジの実板厚 \[mm\] のいずれかが材種の下限を下回れば理由を返す。
pub fn validate_src_h_steel_thickness(
    steel_name: &str,
    tw_mm: f64,
    tf_mm: f64,
    minimum_mm: f64,
) -> Result<(), String> {
    let name = steel_name.trim();
    // 断面形状だけでは圧延形鋼と鋼板組立を区別できない。
    if tw_mm < minimum_mm || tf_mm < minimum_mm {
        return Err(format!(
            "内蔵鉄骨「{name}」の実板厚（ウェブ {tw_mm} mm、フランジ {tf_mm} mm）は JIS G 3136 の適用範囲外です。両方に {minimum_mm} mm 以上が必要です"
        ));
    }
    Ok(())
}

/// CFT 鋼管材料の F [N/mm²] を材料名と板厚 `[mm]` から解決し、未知名は明示 fy を用いる。
/// 非鋼材・非正または非有限の明示 fy・SM520 系列の適用範囲外は `None`。
pub fn cft_steel_f_value(mat: &crate::model::Material, thickness: f64) -> Option<f64> {
    if mat.category != crate::model::MaterialCategory::Steel
        || steel_f_value_issue(&mat.name, thickness).is_some()
    {
        return None;
    }
    if mat.fy.is_some_and(|fy| !fy.is_finite() || fy <= 0.0) {
        return None;
    }
    steel_f_value_prefix(&mat.name, thickness).or(mat.fy)
}

/// 保有水平耐力計算（プッシュオーバー）用の材料強度割増係数。
///
/// 材料強度の基準強度は表の数値の 1.1 倍以下（JIS 規格品・大臣認定品）、
/// ただし 590N 級（SA440・TMCP440。HBL®440/G440 等の認定条件）は 1.05 倍以下と
/// できる規定（H12 建告第2464号の運用・各認定条件）に基づく。
/// 対応済み名称のみ解決し、前後空白は除き、大小文字は区別する。
/// 名称から鋼材グレードを解決できない材料（直接入力材料）は割増しない（1.0）。
///
/// 本係数は保有水平耐力計算（プッシュオーバー）の部材耐力算定にのみ用い、
/// 許容応力度計算（一次設計）には適用しない。
pub fn steel_material_strength_factor(name: &str) -> f64 {
    match steel_grade_base(name) {
        Some("SA440" | "TMCP440") => 1.05,
        Some(_) => 1.1,
        None => 1.0,
    }
}

/// 保有水平耐力計算（プッシュオーバー）で、材料の降伏強度 fy を
/// **鋼材**として用いる文脈（鋼材断面の集中ばね・純鋼材ファイバー・
/// 曲げヒンジ・せん断降伏閾値）の材料強度割増係数。
///
/// 直接入力の割増係数（[`crate::model::Material::strength_factor`]）があれば
/// それを優先し、なければ材料名から自動判定する
/// （[`steel_material_strength_factor`]: 鋼材グレード=1.1、590N 級=1.05、
/// 名称から解決できない材料=1.0）。
pub fn material_strength_factor_steel(mat: &crate::model::Material) -> f64 {
    mat.strength_factor
        .unwrap_or_else(|| steel_material_strength_factor(&mat.name))
}

const ORDINARY_JIS_REBAR_MAX_STRENGTH_FACTOR: f64 = 1.1;

/// 保有水平耐力計算の主筋・壁縦筋に用いる無次元の材料強度割増係数。
/// SD490 は 1.0。他の材種は直接指定値を優先し、空欄は 1.1。
/// JIS 主筋の入力は [`rebar_material_strength_issue`] で検証する。せん断補強筋には適用しない。
pub fn material_strength_factor_rebar(mat: &crate::model::Material) -> f64 {
    if main_rebar_grade(mat) == Some("SD490") {
        1.0
    } else {
        mat.strength_factor.unwrap_or(1.1)
    }
}

fn main_rebar_grade(mat: &crate::model::Material) -> Option<&str> {
    (mat.category == crate::model::MaterialCategory::Rebar).then(|| mat.name.trim())
}

fn ordinary_jis_rebar_fy(grade: &str) -> Option<f64> {
    match grade {
        "SR235" | "SRR235" | "SDR235" => Some(235.0),
        "SR295" | "SD295" | "SD295A" | "SD295B" => Some(295.0),
        "SD345" => Some(345.0),
        "SD390" => Some(390.0),
        _ => None,
    }
}

/// 保有水平耐力計算で基準強度と割増を検証する JIS 主筋・壁縦筋の材種かを返す。
/// 鉄筋材料名の前後の空白は除く。
pub fn has_jis_rebar_strength_limit(mat: &crate::model::Material) -> bool {
    main_rebar_grade(mat)
        .is_some_and(|grade| grade == "SD490" || ordinary_jis_rebar_fy(grade).is_some())
}

/// 梁・柱の主筋で、適用条件の確認に基づく材料強度の直接入力が必要かを返す。
/// 既知の高強度鉄筋名と、対応済みJIS材種以外の490 N/mm²を超えるFyを対象とする。
pub fn requires_direct_high_strength_main_rebar(mat: &crate::model::Material) -> bool {
    let grade = mat.name.trim();
    matches!(
        grade,
        "USD590" | "USD590A" | "USD590B" | "USD685" | "USD685A" | "USD685B"
    ) || (!has_jis_rebar_strength_limit(mat) && mat.fy.is_some_and(|fy| fy > 490.0))
}

/// 保有水平耐力計算で梁・柱の主筋に使う鉄筋区分の材料について、強度の入力不備を返す。
/// 高強度鉄筋の直接入力には、正の有限Fy [N/mm²] と明示の割増1.0が必要。
pub fn main_rebar_material_strength_issue(mat: &crate::model::Material) -> Option<String> {
    if !requires_direct_high_strength_main_rebar(mat) {
        return rebar_material_strength_issue(mat);
    }
    let reason = if mat.fy.is_none_or(|fy| !fy.is_finite() || fy <= 0.0) {
        "Fy が未指定または不正です"
    } else if mat.strength_factor != Some(1.0) {
        "一般主筋の割増は適用できません"
    } else {
        return None;
    };
    Some(format!(
        "材料 ID {}「{}」の保有水平耐力計算用の入力を確認してください。{reason}。設計者が認定・指定書等の適用条件を確認し、材料強度を正の有限値の Fy [N/mm²] として入力し、割増を 1.0 と明示してください",
        mat.id.0, mat.name
    ))
}

/// 保有水平耐力計算で JIS 主筋・壁縦筋に使う材料の入力不備を返す。
/// Fy [N/mm²] は材種の基準強度、割増は空欄または材種ごとの許容範囲が必要。
pub fn rebar_material_strength_issue(mat: &crate::model::Material) -> Option<String> {
    let grade = main_rebar_grade(mat)?;
    if grade == "SD490" {
        return (mat.fy != Some(490.0) || mat.strength_factor.is_some_and(|factor| factor != 1.0))
            .then(|| {
                format!(
                    "材料 ID {}「{}」（材種 SD490）は保有水平耐力計算で Fy = 490 N/mm²、割増は空欄または 1.0 が必要です",
                    mat.id.0, mat.name
                )
            });
    }
    let fy = ordinary_jis_rebar_fy(grade)?;
    let allowed_factors = 1.0..=ORDINARY_JIS_REBAR_MAX_STRENGTH_FACTOR;
    let invalid_factor = mat
        .strength_factor
        .is_some_and(|factor| !allowed_factors.contains(&factor));
    (mat.fy != Some(fy) || invalid_factor).then(|| {
        format!(
            "材料 ID {}「{}」（材種 {grade}）は保有水平耐力計算で Fy = {fy} N/mm²、割増は空欄または 1.0～{ORDINARY_JIS_REBAR_MAX_STRENGTH_FACTOR} が必要です",
            mat.id.0, mat.name
        )
    })
}

/// 鉄筋の基準強度 [N/mm²]（H12 建告第2464号）。
///
/// 異形鉄筋 `SD` ・丸鋼 `SR` は続く数値が基準強度を表す
/// （`SD295A`・`SD345`・`SR235` 等）。未知の名称は `None`。
pub fn rebar_f_value(name: &str) -> Option<f64> {
    let rest = name
        .strip_prefix("SD")
        .or_else(|| name.strip_prefix("SR"))?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// 鉄筋のグレード名から降伏点（基準強度）[N/mm²] を解決する。
///
/// 規格名・既知の製品名から基準強度を解決する。
///
/// [`rebar_f_value`]（異形鉄筋 `SD`・丸鋼 `SR` の接頭辞規則）で解けない名称は、
/// 大臣認定品の高強度鉄筋として**名称末尾の数値**を強度とみなす
/// （`USD685` → 685、`KH785`（スーパーフープ）→ 785、`SBPD1275` → 1275 等。
/// いずれも製品名の数値が降伏点 [N/mm²] を表す命名規則）。
/// 数値を含まない名称は `None`。
pub fn rebar_grade_f_value(name: &str) -> Option<f64> {
    let name = name.trim();
    if let Some(v) = rebar_f_value(name) {
        return Some(v);
    }
    let digits: String = name
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// RC 主筋の降伏点 σy [N/mm²] を解決する。
///
/// 断面の主筋材料（[`crate::model::Section::rebar_material`]）の `fy` を用いる。
/// 未割当・`fy` 未設定は `None` を返し、**既定値で埋めない**（未入力のまま既定
/// 345 N/mm² を用いると、SD295 の部材で耐力を過大評価する＝危険側になるため。
/// 非線形解析は `None` を入力不備として停止する）。
///
/// グレード名から強度を推定するのは**取り込みの境界だけ**の役目とし
/// （[`rebar_grade_f_value`]）、内部では材料テーブルの値だけを見る。名称の
/// 打ち間違いがそのまま強度として通ることを構造的に防ぐためである。
pub fn rebar_yield_strength(rebar_mat: Option<&crate::model::Material>) -> Option<f64> {
    rebar_mat.and_then(|m| m.fy).filter(|v| *v > 0.0)
}

/// せん断補強筋の降伏点 σwy [N/mm²] を解決する。
///
/// 断面のせん断補強筋材料（[`crate::model::Section::shear_rebar_material`]）の
/// `fy` を用いる。材料未割当（`None`）は `None` を返し、呼び出し側が
/// [`SHEAR_REBAR_DEFAULT_FY`]（材料未割当時に用いる SD295 相当の代表値）を既定とする。
/// 材料を割り当てているのに `fy` が未設定の場合は [`shear_rebar_material_issue`] が
/// 不備として扱う（無言で既定値を用いない）。
pub fn shear_rebar_yield_strength(mat: Option<&crate::model::Material>) -> Option<f64> {
    mat.and_then(|m| m.fy).filter(|v| *v > 0.0)
}

/// せん断補強筋の材料が未割当（`None`）のときに用いる降伏点 σwy [N/mm²]。
/// SD295 相当の代表値。材料を割り当てている場合には用いない
/// （`fy` 未設定は [`shear_rebar_material_issue`] が不備とする）。
pub const SHEAR_REBAR_DEFAULT_FY: f64 = 295.0;

/// せん断補強筋として対応するグレードかどうかを判定する。
///
/// 対応グレードは `SR235`・`SR295`・`SD295`・`SD295A`・`SD295B`・`SD345`・`SD390`・
/// `SD490`（名称の明示一致）。名称は trim のみを行い、許容応力度表の行選択にのみ
/// 用いる（材料の降伏強度は `Material.fy` から解決する）。
pub fn is_supported_shear_rebar_grade(name: &str) -> bool {
    let g = name.trim();
    matches!(
        g,
        "SR235" | "SR295" | "SD295" | "SD295A" | "SD295B" | "SD345" | "SD390" | "SD490"
    )
}

/// 未対応のせん断補強筋グレードに対するエラー文言を組み立てる。
/// 材料名と対応グレードの一覧を示す。
pub fn unsupported_shear_rebar_message(name: &str) -> String {
    format!(
        "せん断補強筋グレード『{}』は未対応です。対応グレードは SR235・SR295・SD295・SD345・SD390・SD490 です。",
        name.trim()
    )
}

/// せん断補強筋材料の不備（未対応グレード・`fy` 未設定）をまとめて判定する。
///
/// 材料未割当（`None`）は不備なし（`None`）。未対応グレードは
/// [`unsupported_shear_rebar_message`]、対応グレードで `fy` が未設定または 0 以下なら
/// 材料名を含む是正文言を返す。
pub fn shear_rebar_material_issue(mat: Option<&crate::model::Material>) -> Option<String> {
    let mat = mat?;
    if !is_supported_shear_rebar_grade(&mat.name) {
        return Some(unsupported_shear_rebar_message(&mat.name));
    }
    if !mat.fy.is_some_and(|fy| fy > 0.0) {
        return Some(format!(
            "せん断補強筋材料「{}」に降伏強度 fy が設定されていません。材料タブで fy を設定してください。",
            mat.name.trim()
        ));
    }
    None
}

/// 対応材種のせん断補強筋に入力した Fy [N/mm²] が正の有限値でなければ、是正文言を返す。
pub fn shear_rebar_fy_issue(mat: &crate::model::Material) -> Option<String> {
    (is_supported_shear_rebar_grade(&mat.name)
        && mat.fy.is_some_and(|fy| !fy.is_finite() || fy <= 0.0))
    .then(|| {
        format!(
            "せん断補強筋材料「{}」の降伏強度 Fy が不正です。材料タブで Fy に正の有限値 [N/mm²] を入力してください。",
            mat.name.trim()
        )
    })
}

/// UI で選択できる主筋のグレード（表示順。JIS G 3112 の異形棒鋼と高強度異形棒鋼）。
pub const MAIN_REBAR_GRADES: &[&str] = &["SD295A", "SD295B", "SD345", "SD390", "SD490", "USD685"];

/// コンクリートのグレード名 `FcXX` から設計基準強度 Fc [N/mm²] を取り出す。
/// 大文字小文字を問わず `Fc` で始まり、続く数値を Fc とする（`Fc21`→21）。
pub fn parse_concrete_fc(name: &str) -> Option<f64> {
    let rest = name
        .strip_prefix("Fc")
        .or_else(|| name.strip_prefix("FC"))
        .or_else(|| name.strip_prefix("fc"))?;
    let digits: String = rest
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// UI に提示する標準材料プリセット（内部単位系 N-mm-s。密度は ton/mm³）。
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialPreset {
    pub name: &'static str,
    pub category: MaterialCategory,
    /// ヤング係数 E [N/mm²]
    pub young: f64,
    /// ポアソン比 ν
    pub poisson: f64,
    /// 質量密度 [ton/mm³]
    pub density: f64,
    /// コンクリート設計基準強度 Fc [N/mm²]（鋼材・鉄筋は None）
    pub fc: Option<f64>,
    /// 基準強度 F（板厚 40mm 以下）／鉄筋降伏点 [N/mm²]（コンクリートは None）
    pub fy: Option<f64>,
}

/// UI プリセットとして提示する鋼材グレード（表示順）。
const PRESET_STEEL: &[&str] = &[
    "SS400", "SN400", "SM400", "SM490", "SN490", "BCR295", "BCP235", "TMCP325", "TMCP355",
    "TMCP385", "TMCP440", "SA440", "LY100", "LY225",
];

/// UI プリセットとして提示する鉄筋グレード（表示順）。
const PRESET_REBAR: &[&str] = &["SD295", "SD345", "SD390"];

/// UI プリセットとして提示するコンクリート強度（表示順）。
const PRESET_CONCRETE_FC: &[f64] = &[
    18.0, 21.0, 24.0, 27.0, 30.0, 33.0, 36.0, 40.0, 42.0, 45.0, 50.0, 55.0, 60.0,
];

/// プリセットのコンクリート名（`Fc18`〜`Fc60`）。`PRESET_CONCRETE_FC` と同順。
const PRESET_CONCRETE_NAMES: &[&str] = &[
    "Fc18", "Fc21", "Fc24", "Fc27", "Fc30", "Fc33", "Fc36", "Fc40", "Fc42", "Fc45", "Fc50", "Fc55",
    "Fc60",
];

/// 標準材料プリセット一覧を生成する。
///
/// - 鋼材・鉄筋: E=205000、ν=0.3、質量密度 7.85 t/m³（物理質量。設計用単位体積重量
///   78.5 kN/m³ とは別で、固定荷重側は材料区分から解決する）。
///   `fy` は基準強度 F（板厚 40mm 以下）。設計計算では名称から
///   [`steel_f_value_prefix`] で板厚区分込みの F を再解決する。
/// - コンクリート: ν=0.2。E は Ec=3.35·10⁴·(γC/24)²·(Fc/60)^(1/3)
///   （γC = γRC - 1.0）。密度は単位体積重量表の γRC（鉄筋込み）から導出する。
pub fn material_presets() -> Vec<MaterialPreset> {
    let steel_density = STEEL_MASS_DENSITY_TON_MM3;
    let mut out = Vec::new();
    for &name in PRESET_STEEL {
        out.push(MaterialPreset {
            name,
            category: MaterialCategory::Steel,
            young: E_STEEL,
            poisson: 0.3,
            density: steel_density,
            fc: None,
            fy: steel_f_value(name, 0.0),
        });
    }
    for &name in PRESET_REBAR {
        out.push(MaterialPreset {
            name,
            category: MaterialCategory::Rebar,
            young: E_STEEL,
            poisson: 0.3,
            density: steel_density,
            fc: None,
            fy: rebar_f_value(name),
        });
    }
    for (&fc, &name) in PRESET_CONCRETE_FC.iter().zip(PRESET_CONCRETE_NAMES) {
        let gamma_rc =
            concrete_unit_weight_kn_m3(fc, ConcreteClass::Normal, ConcreteComposition::Rc);
        let gamma_c = gamma_rc - 1.0;
        out.push(MaterialPreset {
            name,
            category: MaterialCategory::Concrete,
            young: concrete_young_modulus_gamma(fc, gamma_c),
            poisson: 0.2,
            density: mass_density_from_unit_weight_kn_m3(gamma_rc),
            fc: Some(fc),
            fy: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// H12 建告第2464号の基準強度表と一致することを確認する（板厚 40mm 以下）。
    #[test]
    fn test_steel_f_value_le40() {
        for (g, f) in [
            ("SS400", 235.0),
            ("SN400", 235.0),
            ("SM400", 235.0),
            ("SM490", 325.0),
            ("SN490", 325.0),
            ("SM520", 355.0),
            ("SS490", 275.0),
            ("BCR295", 295.0),
            ("BCP235", 235.0),
            ("BCP325", 325.0),
            ("TMCP325", 325.0),
            ("TMCP355", 355.0),
            ("TMCP385", 385.0),
            ("TMCP440", 440.0),
            ("SA440", 440.0),
            ("LY100", 80.0),
            ("LY225", 205.0),
        ] {
            assert_eq!(steel_f_value(g, 40.0), Some(f), "{g}");
        }
    }

    /// 板厚 40mm 超の低減（JIS 規格品）と、TMCP・SA440・LY・BCR/BCP が
    /// 板厚によらず一定であることを確認する。SM520 は 75mm 超で 325 へ低減する。
    #[test]
    fn test_steel_f_value_gt40() {
        assert_eq!(steel_f_value("SS400", 41.0), Some(215.0));
        assert_eq!(steel_f_value("SM490", 41.0), Some(295.0));
        assert_eq!(steel_f_value("SN490", 41.0), Some(295.0));
        assert_eq!(steel_f_value("SM520", 41.0), Some(335.0));
        assert_eq!(steel_f_value("SM520", 75.0), Some(335.0));
        assert_eq!(steel_f_value("SM520", 76.0), Some(325.0));
        assert_eq!(steel_f_value("SM520", 100.0), Some(325.0));
        for g in [
            "TMCP325", "TMCP355", "TMCP385", "TMCP440", "SA440", "BCR295", "LY225",
        ] {
            assert_eq!(steel_f_value(g, 41.0), steel_f_value(g, 40.0), "{g}");
        }
    }

    #[test]
    fn supported_steel_names_restore_stock_properties_and_f_values() {
        use crate::standard_material::standard_material_properties;
        for (name, le40, gt40) in [
            ("SS400", 235., 215.),
            ("SS490", 275., 255.),
            ("SM400", 235., 215.),
            ("SM400A", 235., 215.),
            ("SM400B", 235., 215.),
            ("SM400C", 235., 215.),
            ("SM490", 325., 295.),
            ("SM490A", 325., 295.),
            ("SM490B", 325., 295.),
            ("SM490C", 325., 295.),
            ("SM490YA", 325., 295.),
            ("SM490YB", 325., 295.),
            ("SM520", 355., 335.),
            ("SM520B", 355., 335.),
            ("SM520C", 355., 335.),
            ("SN400", 235., 215.),
            ("SN400A", 235., 215.),
            ("SN400B", 235., 215.),
            ("SN400C", 235., 215.),
            ("SN490", 325., 295.),
            ("SN490B", 325., 295.),
            ("SN490C", 325., 295.),
            ("STK400", 235., 215.),
            ("STK490", 325., 295.),
            ("STKN400", 235., 215.),
            ("STKN400B", 235., 215.),
            ("STKN400W", 235., 215.),
            ("STKN490", 325., 295.),
            ("STKN490B", 325., 295.),
            ("STKN490W", 325., 295.),
            ("STKR400", 235., 215.),
            ("STKR490", 325., 295.),
            ("SNR400", 235., 215.),
            ("SNR400A", 235., 215.),
            ("SNR400B", 235., 215.),
            ("SNR490", 325., 295.),
            ("SNR490B", 325., 295.),
            ("SSC400", 235., 215.),
            ("SWH400", 235., 215.),
            ("BCR235", 235., 235.),
            ("BCR295", 295., 295.),
            ("BCP235", 235., 235.),
            ("BCP325", 325., 325.),
            ("TMCP325", 325., 325.),
            ("TMCP355", 355., 355.),
            ("TMCP385", 385., 385.),
            ("TMCP440", 440., 440.),
            ("SA440", 440., 440.),
            ("LY100", 80., 80.),
            ("LY225", 205., 205.),
        ] {
            for input in [name.to_owned(), format!(" \t{name}\n ")] {
                let properties = standard_material_properties(&input).unwrap();
                assert_eq!(properties.young, 205000., "{input:?}");
                assert_eq!(properties.poisson, 0.3, "{input:?}");
                assert_eq!(properties.density, 7.85e-9, "{input:?}");
                assert_eq!(properties.fc, None, "{input:?}");
                assert_eq!(properties.fy, Some(le40), "{input:?}");
                assert_eq!(steel_f_value_prefix(&input, 40.), Some(le40), "{input:?}");
                let expected_factor = if matches!(name, "SA440" | "TMCP440") {
                    1.05
                } else {
                    1.1
                };
                assert_eq!(
                    steel_material_strength_factor(&input),
                    expected_factor,
                    "{input:?}"
                );
                assert_eq!(
                    steel_f_value_prefix(&input, 40.001),
                    Some(gt40),
                    "{input:?}"
                );
            }
        }
    }

    #[test]
    fn unsupported_steel_names_do_not_restore_stock_properties_or_f_values() {
        for name in [
            "SN400-custom",
            "sn400b",
            "SM400YA",
            "SM400YB",
            "SM400XYZ",
            "SN400XYZ",
            "SM490XYZ",
            "SM490Y",
            "SN490A",
            "SM520A",
            "STKN400A",
            "STKN490C",
            "SNR400C",
            "SNR490A",
            "SS400A",
            "STK400B",
            "STKR490B",
            "BCR295X",
            "TMCP325X",
            "LY100X",
            "SM400AA",
            "SM400A-extra",
            "SM490YAextra",
            "SM400 A",
            "SM 400A",
            "sm400a",
            "Sm400A",
            "SM400a",
            "sn490b",
            "",
            " \t\n",
            "未知",
        ] {
            assert!(
                crate::standard_material::standard_material_properties(name).is_none(),
                "{name:?}"
            );
            assert_eq!(steel_material_strength_factor(name), 1.0, "{name:?}");
            for thickness in [40., 40.001] {
                assert_eq!(steel_f_value_prefix(name, thickness), None, "{name:?}");
            }
        }
    }

    /// 文脈別係数: 直接入力の割増係数が最優先、なければ鋼材=名称判定・主筋=1.1。
    #[test]
    fn test_material_strength_factor_by_context() {
        let mk = |name: &str, factor: Option<f64>| crate::model::Material {
            id: crate::ids::MaterialId(0),
            name: name.to_string(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: None,
            concrete_class: Default::default(),
            strength_factor: factor,
        };
        for name in [
            "SN400B",
            " SN400B ",
            "SA440",
            " SA440 ",
            "TMCP440",
            " TMCP440 ",
            "SN400-custom",
            "SM400YA",
            "sn400b",
            "未知鋼材",
        ] {
            let mut mat = mk(name, Some(1.07));
            mat.fy = Some(200.0);
            assert_eq!(material_strength_factor_steel(&mat), 1.07, "{name:?}");
        }
        assert_eq!(material_strength_factor_steel(&mk("SS400", None)), 1.1);
        assert_eq!(material_strength_factor_steel(&mk("SA440", None)), 1.05);
        assert_eq!(material_strength_factor_steel(&mk("カスタム", None)), 1.0);
        assert_eq!(material_strength_factor_rebar(&mk("Fc24", None)), 1.1);
        assert_eq!(material_strength_factor_rebar(&mk("SD345", None)), 1.1);
        assert_eq!(
            material_strength_factor_steel(&mk("カスタム", Some(1.2))),
            1.2
        );
        assert_eq!(material_strength_factor_rebar(&mk("Fc24", Some(1.0))), 1.0);
        let mut conflict = mk("SD490", Some(1.2));
        conflict.category = MaterialCategory::Concrete;
        assert_eq!(material_strength_factor_rebar(&conflict), 1.2);
        assert!(rebar_material_strength_issue(&conflict).is_none());
    }

    #[test]
    fn test_rebar_f_value() {
        assert_eq!(rebar_f_value("SD295A"), Some(295.0));
        assert_eq!(rebar_f_value("SD345"), Some(345.0));
        assert_eq!(rebar_f_value("SD390"), Some(390.0));
        assert_eq!(rebar_f_value("SR235"), Some(235.0));
        assert_eq!(rebar_f_value("SS400"), None);
    }

    #[test]
    fn jis_main_rebar_strength_limits_use_the_grade_reference_strength() {
        for (grade, fy) in [
            ("SR235", 235.0),
            ("SRR235", 235.0),
            ("SDR235", 235.0),
            ("SR295", 295.0),
            ("SD295", 295.0),
            ("SD295A", 295.0),
            ("SD295B", 295.0),
            ("SD345", 345.0),
            ("SD390", 390.0),
        ] {
            let mut mat = rebar_mat(grade, Some(fy));
            mat.strength_factor = Some(1.1);
            assert!(rebar_material_strength_issue(&mat).is_none(), "{grade}");
            mat.strength_factor = Some(1.2);
            assert!(rebar_material_strength_issue(&mat).is_some(), "{grade}");
            mat.strength_factor = Some(1.0);
            mat.fy = Some(fy + 1.0);
            assert!(rebar_material_strength_issue(&mat).is_some(), "{grade}");
        }

        for name in ["sd345", "SD345-別材料", "USD685"] {
            let mut mat = rebar_mat(name, Some(400.0));
            mat.strength_factor = Some(1.2);
            assert!(rebar_material_strength_issue(&mat).is_none(), "{name}");
        }
    }

    /// 高強度鉄筋の製品名は末尾の数値を降伏点とみなす。
    #[test]
    fn test_rebar_grade_f_value_high_strength() {
        assert_eq!(rebar_grade_f_value("SD345"), Some(345.0));
        assert_eq!(rebar_grade_f_value("USD685"), Some(685.0));
        assert_eq!(rebar_grade_f_value("KH785"), Some(785.0));
        assert_eq!(rebar_grade_f_value("UB785"), Some(785.0));
        assert_eq!(rebar_grade_f_value("SBPD1275"), Some(1275.0));
        assert_eq!(rebar_grade_f_value("不明"), None);
    }

    /// 鉄筋材料の見本（`fy` を指定して作る）。
    fn rebar_mat(name: &str, fy: Option<f64>) -> crate::model::Material {
        crate::model::Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: crate::ids::MaterialId(0),
            name: name.into(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy,
        }
    }

    /// 主筋 σy は断面の主筋材料の `fy` だけから決まる。材料が未割当のとき、
    /// および `fy` が未設定・非正値のときは None（既定値で埋めない）。
    #[test]
    fn test_rebar_yield_strength_resolution_order() {
        let mut mat = rebar_mat("SD295A", Some(295.0));
        assert_eq!(rebar_yield_strength(Some(&mat)), Some(295.0));
        mat.fy = None;
        assert_eq!(rebar_yield_strength(Some(&mat)), None);
        mat.fy = Some(0.0);
        assert_eq!(rebar_yield_strength(Some(&mat)), None);
        assert_eq!(rebar_yield_strength(None), None);
    }

    /// せん断補強筋 σwy も材料の `fy` から解決し、未設定は None（既定 295 は呼び出し側）。
    #[test]
    fn test_shear_rebar_yield_strength() {
        let mat = rebar_mat("SD345", Some(345.0));
        assert_eq!(shear_rebar_yield_strength(Some(&mat)), Some(345.0));
        assert_eq!(shear_rebar_yield_strength(None), None);
        assert_eq!(SHEAR_REBAR_DEFAULT_FY, 295.0);
    }

    /// せん断補強筋として対応するグレードの判定。対応は SR235・SR295・SD295 系・
    /// SD345・SD390・SD490 の明示一致のみで、高強度製品名・`SD295` の未知名は未対応とする。
    #[test]
    fn test_is_supported_shear_rebar_grade() {
        for name in [
            "SR235",
            "SR295",
            "SD295",
            "SD295A",
            "SD295B",
            "SD345",
            "SD390",
            "SD490",
            "  SD345  ",
        ] {
            assert!(is_supported_shear_rebar_grade(name), "{name} は対応");
        }
        for name in [
            "KH785",
            "UB785",
            "SBPD1275",
            "USD685",
            "",
            "S",
            "SS400",
            "SD295X",
            "SD295Z",
            "SD295-FOO",
        ] {
            assert!(!is_supported_shear_rebar_grade(name), "{name} は未対応");
        }
    }

    /// 未対応グレードのエラー文言に材料名と対応グレードの一覧が含まれる。
    #[test]
    fn test_unsupported_shear_rebar_message() {
        let msg = unsupported_shear_rebar_message("KH785");
        assert!(msg.contains("KH785"), "{msg}");
        assert!(
            msg.contains("SR235・SR295・SD295・SD345・SD390・SD490"),
            "{msg}"
        );
    }

    /// せん断補強筋材料の不備判定: 未割当は不備なし、未対応グレードと
    /// 対応グレードの `fy` 未設定（0 以下を含む）は不備とする。
    #[test]
    fn test_shear_rebar_material_issue() {
        assert_eq!(shear_rebar_material_issue(None), None);
        assert_eq!(
            shear_rebar_material_issue(Some(&rebar_mat("SR235", Some(235.0)))),
            None
        );
        let msg = shear_rebar_material_issue(Some(&rebar_mat("KH785", Some(785.0)))).unwrap();
        assert!(msg.contains("KH785"), "{msg}");
        for fy in [None, Some(0.0)] {
            let msg = shear_rebar_material_issue(Some(&rebar_mat("SR235", fy))).unwrap();
            assert!(msg.contains("SR235") && msg.contains("fy"), "{msg}");
        }
        for name in ["SD295X", "SD295Z", "SD295-FOO"] {
            let msg = shear_rebar_material_issue(Some(&rebar_mat(name, Some(295.0)))).unwrap();
            assert!(msg.contains(name), "{msg}");
            assert!(msg.contains("未対応"), "{msg}");
        }
    }

    #[test]
    fn test_parse_concrete_fc() {
        assert_eq!(parse_concrete_fc("Fc21"), Some(21.0));
        assert_eq!(parse_concrete_fc("FC36"), Some(36.0));
        assert_eq!(parse_concrete_fc("fc60"), Some(60.0));
        assert_eq!(parse_concrete_fc("SD345"), None);
    }

    /// プリセット一覧: 件数・代表値・コンクリートの γ 帯依存を確認する。
    #[test]
    fn test_material_presets() {
        let presets = material_presets();
        assert_eq!(presets.len(), 14 + 3 + 13);
        let find = |name: &str| {
            presets
                .iter()
                .find(|p| p.name == name)
                .unwrap_or_else(|| panic!("preset {name} not found"))
        };

        let ss400 = find("SS400");
        assert_eq!(ss400.category, MaterialCategory::Steel);
        assert_eq!(ss400.young, 205000.0);
        assert_eq!(ss400.fy, Some(235.0));

        let ly100 = find("LY100");
        assert_eq!(ly100.fy, Some(80.0));

        let sd345 = find("SD345");
        assert_eq!(sd345.category, MaterialCategory::Rebar);
        assert_eq!(sd345.fy, Some(345.0));

        // コンクリート: Fc≤36 は γ=23、36<Fc≤48 は γ=23.5、48<Fc は γ=24 で Ec を算定。
        let fc24 = find("Fc24");
        assert_eq!(fc24.category, MaterialCategory::Concrete);
        assert_eq!(fc24.fc, Some(24.0));
        let ec24 = 3.35e4 * (23.0f64 / 24.0).powi(2) * (24.0f64 / 60.0).powf(1.0 / 3.0);
        assert!((fc24.young - ec24).abs() < 1e-9);
        let fc42 = find("Fc42");
        let ec42 = 3.35e4 * (23.5f64 / 24.0).powi(2) * (42.0f64 / 60.0).powf(1.0 / 3.0);
        assert!((fc42.young - ec42).abs() < 1e-9);
        let fc60 = find("Fc60");
        let ec60 = 3.35e4 * (60.0f64 / 60.0).powf(1.0 / 3.0);
        assert!((fc60.young - ec60).abs() < 1e-9);

        // 密度: 鋼・鉄筋は物理質量密度 7.85 t/m³（= 7.85e-9 t/mm³）、
        // コンクリートは γRC（Fc≤36 の全プリセットで 24.0 kN/m³）。
        assert_eq!(ss400.density, 7.85e-9);
        assert_eq!(sd345.density, 7.85e-9);
        let rho_rc = mass_density_from_unit_weight_kn_m3(24.0);
        for name in ["Fc18", "Fc21", "Fc24", "Fc27", "Fc30", "Fc33", "Fc36"] {
            let p = find(name);
            assert_eq!(p.category, MaterialCategory::Concrete);
            assert!((p.density - rho_rc).abs() < 1e-18, "{name}");
        }
    }
}

#[cfg(test)]
mod steel_f_resolution_tests {
    use super::*;

    fn steel(name: &str, fy: Option<f64>) -> Material {
        Material {
            id: crate::ids::MaterialId(0),
            name: name.into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy,
            concrete_class: Default::default(),
            strength_factor: None,
        }
    }

    #[test]
    fn known_steel_uses_the_product_thickness_table_independently_of_explicit_fy() {
        for fy in [None, Some(235.0), Some(999.0)] {
            let material = steel("SS400", fy);
            for (thickness, expected) in [(40.0, 235.0), (40.001, 215.0), (50.0, 215.0)] {
                assert_eq!(
                    resolve_steel_f(Some(&material), Some(thickness)),
                    Ok(expected)
                );
            }
        }
    }

    #[test]
    fn direct_steel_uses_positive_finite_fy_without_a_thickness_bucket() {
        let material = steel("直接入力", Some(287.0));
        assert_eq!(resolve_steel_f(Some(&material), None), Ok(287.0));
        assert_eq!(resolve_steel_f(Some(&material), Some(150.0)), Ok(287.0));
        for fy in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert!(resolve_steel_f(Some(&steel("直接入力", fy)), None).is_err());
        }
    }

    #[test]
    fn missing_material_or_known_grade_thickness_cannot_be_replaced_by_fy_or_defaults() {
        assert!(resolve_steel_f(None, Some(20.0)).is_err());
        let mut material = steel("SS400", Some(999.0));
        for thickness in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(100.001),
        ] {
            assert!(resolve_steel_f(Some(&material), thickness).is_err());
        }
        material.name = "SM520B".into();
        assert!(resolve_steel_f(Some(&material), Some(100.001)).is_err());
        material.category = MaterialCategory::Concrete;
        assert!(resolve_steel_f(Some(&material), Some(20.0)).is_err());
    }
}

#[cfg(test)]
mod sm520_contract_tests {
    use super::*;
    #[test]
    fn sm520_notice_boundaries_and_invalid_thickness_are_fixed() {
        for name in ["SM520", "SM520B", "SM520C", " SM520B "] {
            for (t, expected) in [
                (40.0, Some(355.0)),
                (40.001, Some(335.0)),
                (75.0, Some(335.0)),
                (75.001, Some(325.0)),
                (100.0, Some(325.0)),
                (100.001, None),
            ] {
                assert_eq!(steel_f_value_prefix(name, t), expected, "{name} t={t}");
            }
            for t in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                assert_eq!(steel_f_value_prefix(name, t), None);
                assert!(steel_f_value_issue(name, t).unwrap().contains("適用範囲"));
            }
        }
    }
    #[test]
    fn cft_explicit_fy_cannot_restore_out_of_range_sm520_but_unknown_names_keep_direct_input() {
        for name in ["SM520", "SM520B", "SM520C", " SM520C "] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let steel = crate::model::Material {
                    id: crate::ids::MaterialId(0),
                    name: name.into(),
                    category: MaterialCategory::Steel,
                    young: 205000.0,
                    poisson: 0.3,
                    density: 7.85e-9,
                    shear: None,
                    fc: None,
                    fy,
                    concrete_class: Default::default(),
                    strength_factor: None,
                };
                assert_eq!(cft_steel_f_value(&steel, 100.001), None);
                let mut unknown = steel.clone();
                unknown.name = "未知材種".into();
                assert_eq!(cft_steel_f_value(&unknown, 100.001), fy);
                unknown.name = "sm520b".into();
                assert_eq!(cft_steel_f_value(&unknown, 100.001), fy);
            }
        }
        assert_eq!(steel_f_value_prefix("SS400", 100.001), Some(215.0));
        assert_eq!(steel_f_value_prefix("SM490", 100.001), Some(295.0));
    }
}
