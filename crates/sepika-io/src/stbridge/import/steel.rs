//! 形鋼ライブラリ要素（`StbSecRoll-*` / `StbSecBuild-*` / `StbSecPipe`）からの断面形状復元。

use super::xml::{get_f64, Attrs};
use sepika_core::section_shape::SectionShape;

/// 形鋼ライブラリ要素と属性から [`SectionShape`] を復元する。
pub(super) fn steel_shape_from(tag: &str, a: &Attrs) -> Option<Result<SectionShape, String>> {
    if tag == "StbSecBuild-BOX" {
        return Some(built_box_shape(a));
    }
    other_steel_shape(tag, a).map(Ok)
}

fn built_box_shape(a: &Attrs) -> Result<SectionShape, String> {
    let dimension = |key: &str| {
        let raw = a
            .get(key)
            .ok_or_else(|| format!("{key} が欠落しています"))?;
        raw.parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| format!("{key}={raw:?} は数値（有限の正値）でなければなりません"))
    };
    let result = (|| {
        let height_mm = dimension("A")?;
        let width_mm = dimension("B")?;
        let side_thick_mm = dimension("t1")?;
        let top_bottom_thick_mm = dimension("t2")?;
        for (inner, value, outer_key, thick_key) in [
            ("A−2*t2", height_mm - 2.0 * top_bottom_thick_mm, "A", "t2"),
            ("B−2*t1", width_mm - 2.0 * side_thick_mm, "B", "t1"),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!(
                    "{outer_key}={:?}, {thick_key}={:?}: 内法{inner}={value} は有限の正値でなければなりません",
                    a.get(outer_key).unwrap(), a.get(thick_key).unwrap()
                ));
            }
        }
        Ok(SectionShape::SteelBox {
            height: height_mm,
            width: width_mm,
            thick: side_thick_mm,
            horizontal_thick: top_bottom_thick_mm,
            corner_r: Some(0.0),
        })
    })();
    result.map_err(|reason| format!("StbSecBuild-BOX: {reason}"))
}

fn other_steel_shape(tag: &str, a: &Attrs) -> Option<SectionShape> {
    let a_ = |key: &str| get_f64(a, key).ok();
    match tag {
        "StbSecRoll-H" | "StbSecBuild-H" => {
            let height = a_("A")?;
            let web_thick = a_("t1")?;
            let upper_width = a_("B")?;
            let upper_thick = a_("t2")?;
            if tag == "StbSecBuild-H" {
                return Some(SectionShape::SteelBuiltH {
                    height,
                    upper_width,
                    upper_thick,
                    lower_width: upper_width,
                    lower_thick: upper_thick,
                    web_thick,
                });
            }
            Some(SectionShape::SteelH {
                height,
                width: upper_width,
                web_thick,
                flange_thick: upper_thick,
                root_r: a_("r"),
            })
        }
        "StbSecRoll-BOX" => Some(SectionShape::SteelBox {
            height: a_("A")?,
            width: a_("B")?,
            thick: a_("t")?,
            horizontal_thick: a_("t")?,
            corner_r: a_("r"),
        }),
        "StbSecPipe" => Some(SectionShape::SteelPipe {
            outer_dia: a_("D")?,
            thick: a_("t")?,
        }),
        "StbSecRoll-L" => Some(SectionShape::SteelAngle {
            leg_a: a_("A")?,
            leg_b: a_("B")?,
            thick: a_("t1")?,
        }),
        "StbSecRoll-C" => Some(SectionShape::SteelChannel {
            height: a_("A")?,
            width: a_("B")?,
            web_thick: a_("t1")?,
            flange_thick: a_("t2")?,
        }),
        "StbSecRoll-T" => Some(SectionShape::SteelTee {
            height: a_("A")?,
            width: a_("B")?,
            web_thick: a_("t1")?,
            flange_thick: a_("t2")?,
        }),
        _ => None,
    }
}
