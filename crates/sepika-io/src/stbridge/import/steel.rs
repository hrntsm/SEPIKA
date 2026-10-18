//! 形鋼ライブラリ要素（`StbSecRoll-*` / `StbSecBuild-*` / `StbSecPipe`）からの断面形状復元。

use super::xml::{get_f64, Attrs};
use sepika_core::section_shape::SectionShape;

/// 形鋼ライブラリ要素と属性から [`SectionShape`] を復元する。
pub(super) fn steel_shape_from(tag: &str, a: &Attrs) -> Option<SectionShape> {
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
        "StbSecRoll-BOX" | "StbSecBuild-BOX" => {
            let thick = a_(if tag == "StbSecBuild-BOX" { "t1" } else { "t" })?;
            let corner_r = if tag == "StbSecBuild-BOX" {
                Some(0.0)
            } else {
                a_("r")
            };
            Some(SectionShape::SteelBox {
                height: a_("A")?,
                width: a_("B")?,
                thick,
                corner_r,
            })
        }
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
