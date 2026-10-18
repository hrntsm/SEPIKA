//! ST-Bridge の配筋（`StbSecBarArrangement*`）属性の解析（best-effort）。

use super::xml::Attrs;
use sepika_core::model::stb_numeric_lexical;
use sepika_core::section_shape::{
    BeamStirrup, CircleColumnHoop, RcBeamRebar, RcCircleColumnRebar, RcRectColumnRebar,
    RectColumnHoop,
};

/// 配筋属性に現れる材質のグレード名（主筋・せん断補強筋）。
///
/// 材質は断面の材料として持つため（`Section::rebar_material` ほか）、形状には
/// 入れずここで分けて返す。グレード名から材料を起こすのは取り込みの境界の役目。
#[derive(Default, Clone, Debug)]
pub(super) struct RebarGrades {
    pub main: Option<String>,
    pub shear: Option<String>,
}

/// 幾何のみの RC 梁断面に用いる既定配筋（無筋相当）。
pub(super) fn default_beam_rebar() -> RcBeamRebar {
    RcBeamRebar {
        main_dia: 0.0,
        top: Vec::new(),
        bottom: Vec::new(),
        cover: 0.0,
        stirrup: BeamStirrup {
            dia: 0.0,
            pitch: 0.0,
            legs: 0,
        },
    }
}

/// 幾何のみの RC 矩形柱断面に用いる既定配筋（無筋相当）。
pub(super) fn default_rect_column_rebar() -> RcRectColumnRebar {
    RcRectColumnRebar {
        main_dia: 0.0,
        x: Vec::new(),
        y: Vec::new(),
        cover: 0.0,
        hoop: RectColumnHoop {
            dia: 0.0,
            pitch: 0.0,
            legs_x: 0,
            legs_y: 0,
        },
    }
}

/// 幾何のみの RC 円形柱断面に用いる既定配筋（無筋相当）。
pub(super) fn default_circle_column_rebar() -> RcCircleColumnRebar {
    RcCircleColumnRebar {
        main_dia: 0.0,
        count: 0,
        cover: 0.0,
        hoop: CircleColumnHoop {
            dia: 0.0,
            pitch: 0.0,
        },
    }
}

/// 鉄筋径の文字列を mm へ解釈する。数値ならそのまま、`D22`/`D10` のような呼び名は
/// 先頭の `D`/`d` を除いた数値を径とする（best-effort。厳密な JIS 公称径ではない）。
fn parse_bar_dia(v: &str) -> Option<f64> {
    let t = v.trim();
    t.strip_prefix(['D', 'd'])
        .unwrap_or(t)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|x| x.is_finite() && *x > 0.0)
}

/// 候補キーのいずれかから f64 を取る（欠落は 0）。
fn attr_f64(a: &Attrs, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(v) = a.get(k) {
            if let Ok(x) = stb_numeric_lexical(v).parse::<f64>() {
                return x;
            }
        }
    }
    0.0
}

/// 候補キーのいずれかから鉄筋径 [mm] を取る（呼び名 `D22` も解釈。欠落は 0）。
fn attr_dia(a: &Attrs, keys: &[&str]) -> f64 {
    for k in keys {
        if let Some(v) = a.get(k) {
            if let Some(x) = parse_bar_dia(v) {
                return x;
            }
        }
    }
    0.0
}

/// 候補キーのいずれかから u32 を取る（欠落は 0）。
fn attr_u32(a: &Attrs, keys: &[&str]) -> u32 {
    for k in keys {
        if let Some(v) = a.get(k) {
            if let Ok(x) = stb_numeric_lexical(v).parse::<u32>() {
                return x;
            }
        }
    }
    0
}

/// 正式な段別属性の本数を段番号順に集める。
fn stage_counts(a: &Attrs, prefix: &str, stages: usize) -> Vec<u32> {
    let counts: Vec<_> = ["1st", "2nd", "3rd"]
        .iter()
        .take(stages)
        .map(|suffix| {
            a.get(&format!("{prefix}{suffix}"))
                .and_then(|v| stb_numeric_lexical(v).parse::<u32>().ok())
        })
        .collect();
    let last = counts.iter().rposition(Option::is_some);
    counts
        .into_iter()
        .take(last.map_or(0, |i| i + 1))
        .map(|v| v.unwrap_or(0))
        .collect()
}

/// 配筋属性から材質グレード名を取る。
fn parse_grades(a: &Attrs, shear_key: &str) -> RebarGrades {
    RebarGrades {
        main: a.get("strength_main").cloned(),
        shear: a.get(shear_key).cloned(),
    }
}

/// 梁の上端筋・下端筋をかぶり側から内側への段別本数として読む。
pub(super) fn parse_beam_rebar(a: &Attrs) -> (RcBeamRebar, RebarGrades, Vec<String>) {
    let warnings = Vec::new();
    let top = stage_counts(a, "N_main_top_", 3);
    let bottom = stage_counts(a, "N_main_bottom_", 3);
    let mut rebar = RcBeamRebar {
        main_dia: attr_dia(a, &["D_main"]),
        top,
        bottom,
        cover: attr_f64(a, &["depth_cover_top", "depth_cover_bottom"]),
        stirrup: BeamStirrup {
            dia: attr_dia(a, &["D_stirrup"]),
            pitch: attr_f64(a, &["pitch_stirrup"]),
            legs: attr_u32(a, &["N_stirrup"]),
        },
    };
    if let Some(center) = ["center_top", "center_bottom", "center_side"]
        .iter()
        .find_map(|k| {
            a.get(k)
                .and_then(|v| stb_numeric_lexical(v).parse::<f64>().ok())
        })
    {
        rebar.cover = center - rebar.stirrup.dia - rebar.main_dia / 2.0;
    }
    (rebar, parse_grades(a, "strength_stirrup"), warnings)
}

/// 矩形柱の段別主筋本数とX/Y方向の帯筋脚数を読む。
pub(super) fn parse_rect_column_rebar(a: &Attrs) -> (RcRectColumnRebar, RebarGrades, Vec<String>) {
    let warnings = Vec::new();
    // 標準Xは縦辺の列数を表し、内部xは横辺の列数を表すため入れ替える。
    let x = stage_counts(a, "N_main_Y_", 2);
    let y = stage_counts(a, "N_main_X_", 2);
    let legs_x = attr_u32(a, &["N_band_direction_X"]);
    let legs_y = attr_u32(a, &["N_band_direction_Y"]);
    let mut rebar = RcRectColumnRebar {
        main_dia: attr_dia(a, &["D_main"]),
        x,
        y,
        cover: attr_f64(
            a,
            &[
                "depth_cover_start_X",
                "depth_cover_end_X",
                "depth_cover_start_Y",
                "depth_cover_end_Y",
            ],
        ),
        hoop: RectColumnHoop {
            dia: attr_dia(a, &["D_band"]),
            pitch: attr_f64(a, &["pitch_band"]),
            legs_x,
            legs_y,
        },
    };
    if let Some(center) = [
        "center_start_X",
        "center_end_X",
        "center_start_Y",
        "center_end_Y",
    ]
    .iter()
    .find_map(|k| {
        a.get(k)
            .and_then(|v| stb_numeric_lexical(v).parse::<f64>().ok())
    }) {
        rebar.cover = center - rebar.hoop.dia - rebar.main_dia / 2.0;
    }
    (rebar, parse_grades(a, "strength_band"), warnings)
}

/// 円形柱の主筋全本数と帯筋径・間隔を読む。
pub(super) fn parse_circle_column_rebar(
    a: &Attrs,
) -> (RcCircleColumnRebar, RebarGrades, Vec<String>) {
    let warnings = Vec::new();
    let mut rebar = RcCircleColumnRebar {
        main_dia: attr_dia(a, &["D_main"]),
        count: attr_u32(a, &["N_main"]),
        cover: attr_f64(
            a,
            &[
                "depth_cover_start_X",
                "depth_cover_end_X",
                "depth_cover_start_Y",
                "depth_cover_end_Y",
            ],
        ),
        hoop: CircleColumnHoop {
            dia: attr_dia(a, &["D_band"]),
            pitch: attr_f64(a, &["pitch_band"]),
        },
    };
    if let Some(center) = [
        "center_start_X",
        "center_end_X",
        "center_start_Y",
        "center_end_Y",
    ]
    .iter()
    .find_map(|k| {
        a.get(k)
            .and_then(|v| stb_numeric_lexical(v).parse::<f64>().ok())
    }) {
        rebar.cover = center - rebar.hoop.dia - rebar.main_dia / 2.0;
    }
    (rebar, parse_grades(a, "strength_band"), warnings)
}
