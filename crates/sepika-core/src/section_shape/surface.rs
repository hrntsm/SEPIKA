use super::SectionShape;
use std::f64::consts::PI;

fn positive(values: &[f64]) -> Result<(), String> {
    if values.iter().all(|v| v.is_finite() && *v > 0.0) {
        Ok(())
    } else {
        Err("被覆外周の断面寸法は有限な正値が必要です".into())
    }
}

fn radius(value: Option<f64>, limit: f64) -> Result<(), String> {
    if let Some(r) = value {
        if !r.is_finite() || r < 0.0 || r > limit {
            return Err(
                "被覆外周の半径が不正です（有限な非負値で断面内に収まることが必要）".into(),
            );
        }
    }
    Ok(())
}

impl SectionShape {
    /// 被覆用半径とその適用寸法を検証する。未知半径は保持を許容する。
    pub fn validate_surface_radius(&self) -> Result<(), String> {
        match *self {
            Self::SteelH {
                height,
                width,
                web_thick,
                flange_thick,
                root_r,
            } => {
                positive(&[height, width, web_thick, flange_thick])?;
                if height <= 2.0 * flange_thick || width < web_thick {
                    return Err("H形断面の被覆外周寸法が不正です".into());
                }
                radius(
                    root_r,
                    ((height - 2.0 * flange_thick) / 2.0).min((width - web_thick) / 2.0),
                )
            }
            Self::SteelBox {
                height,
                width,
                thick,
                corner_r,
            }
            | Self::CftBox {
                height,
                width,
                thick,
                corner_r,
            } => {
                positive(&[height, width, thick])?;
                if height.min(width) <= 2.0 * thick {
                    return Err("角形鋼管の内法寸法が不正です（高さ・幅は板厚の 2 倍より大きい必要があります）".into());
                }
                radius(corner_r, height.min(width) / 2.0)
            }
            _ => Ok(()),
        }
    }

    /// 吹きつけ被覆の実外周 [mm]。半径未知・不正寸法・未対応形状はエラー。
    pub fn coating_surface_perimeter(&self) -> Result<f64, String> {
        self.validate_surface_radius()?;
        let unknown = || "被覆外周に必要な半径が未知です".to_string();
        let p = match *self {
            Self::SteelH {
                height,
                width,
                web_thick,
                root_r,
                ..
            } => {
                4.0 * width + 2.0 * height
                    - 2.0 * web_thick
                    - (8.0 - 2.0 * PI) * root_r.ok_or_else(unknown)?
            }
            Self::SteelBox {
                height,
                width,
                corner_r,
                ..
            }
            | Self::CftBox {
                height,
                width,
                corner_r,
                ..
            } => 2.0 * (height + width) - (8.0 - 2.0 * PI) * corner_r.ok_or_else(unknown)?,
            Self::SteelBuiltH {
                height,
                upper_width,
                upper_thick,
                lower_width,
                lower_thick,
                web_thick,
            } => {
                positive(&[
                    height,
                    upper_width,
                    upper_thick,
                    lower_width,
                    lower_thick,
                    web_thick,
                ])?;
                if height <= upper_thick + lower_thick || upper_width.min(lower_width) < web_thick {
                    return Err("組立Hの板組輪郭寸法が不正です".into());
                }
                2.0 * (upper_width + lower_width + height - web_thick)
            }
            Self::SteelPipe { outer_dia, thick } | Self::CftPipe { outer_dia, thick } => {
                positive(&[outer_dia, thick])?;
                if outer_dia <= 2.0 * thick {
                    return Err("鋼管の被覆外周寸法が不正です".into());
                }
                PI * outer_dia
            }
            Self::SteelFlatBar { width, thick } => {
                positive(&[width, thick])?;
                2.0 * (width + thick)
            }
            Self::SteelRoundBar { dia } => {
                positive(&[dia])?;
                PI * dia
            }
            _ => return Err("被覆実外周の未対応形状です".into()),
        };
        if !p.is_finite() || p <= 0.0 {
            return Err("被覆外周が有限な正値ではありません".into());
        }
        Ok(p)
    }

    /// 成形版被覆の包絡周長 [mm]。矩形・円形の包絡を用い、未対応形状はエラー。
    pub fn coating_envelope_perimeter(&self) -> Result<f64, String> {
        self.validate_surface_radius()?;
        let p = match *self {
            Self::SteelH { height, width, .. }
            | Self::SteelBox { height, width, .. }
            | Self::CftBox { height, width, .. } => 2.0 * (height + width),
            Self::SteelBuiltH {
                height,
                upper_width,
                lower_width,
                ..
            } => {
                self.coating_surface_perimeter()?;
                2.0 * (height + upper_width.max(lower_width))
            }
            Self::SteelPipe { .. } | Self::CftPipe { .. } => self.coating_surface_perimeter()?,
            Self::SteelFlatBar { .. } | Self::SteelRoundBar { .. } => {
                self.coating_surface_perimeter()?
            }
            _ => return Err("被覆包絡周長の未対応形状です".into()),
        };
        if !p.is_finite() || p <= 0.0 {
            return Err("被覆包絡周長が有限な正値ではありません".into());
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn h(r: Option<f64>) -> SectionShape {
        SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 8.0,
            flange_thick: 13.0,
            root_r: r,
        }
    }
    fn boxes(r: Option<f64>) -> [SectionShape; 2] {
        [
            SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                corner_r: r,
            },
            SectionShape::CftBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                corner_r: r,
            },
        ]
    }
    #[test]
    fn 実外周と包絡を区別する() {
        for r in [0.0, 13.0, 96.0] {
            assert!(
                (h(Some(r)).coating_surface_perimeter().unwrap() - (1584.0 - (8.0 - 2.0 * PI) * r))
                    .abs()
                    < 1e-10
            );
            assert_eq!(h(Some(r)).coating_envelope_perimeter().unwrap(), 1200.0);
        }
        for r in [0.0, 30.0, 150.0] {
            for shape in boxes(Some(r)) {
                assert!(
                    (shape.coating_surface_perimeter().unwrap() - (1400.0 - (8.0 - 2.0 * PI) * r))
                        .abs()
                        < 1e-10
                );
                assert_eq!(shape.coating_envelope_perimeter().unwrap(), 1400.0);
            }
        }
    }
    #[test]
    fn 未知と不正半径は直角へ読み替えない() {
        for shape in [h(None), boxes(None)[0].clone(), boxes(None)[1].clone()] {
            assert!(shape.validate_surface_radius().is_ok());
            assert!(shape
                .coating_surface_perimeter()
                .unwrap_err()
                .contains("未知"));
            assert!(shape.coating_envelope_perimeter().is_ok());
        }
        for r in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 151.0] {
            for shape in [
                h(Some(r)),
                boxes(Some(r))[0].clone(),
                boxes(Some(r))[1].clone(),
            ] {
                assert!(shape.validate_surface_radius().is_err());
                assert!(shape.coating_surface_perimeter().is_err());
                assert!(shape.coating_envelope_perimeter().is_err());
            }
        }
        assert!(h(Some(96.1)).validate_surface_radius().is_err());
    }
    #[test]
    fn フィレット半径と角rは幾何性能を更新しjは維持する() {
        for (a, b) in [
            (h(Some(0.0)), h(Some(13.0))),
            (boxes(Some(0.0))[0].clone(), boxes(Some(30.0))[0].clone()),
            (boxes(Some(0.0))[1].clone(), boxes(Some(30.0))[1].clone()),
        ] {
            assert_ne!(a.calc_area(), b.calc_area());
            assert_ne!(a.calc_iy(), b.calc_iy());
            assert_ne!(a.calc_iz(), b.calc_iz());
            assert_eq!(a.calc_j(), b.calc_j());
            if let (Some(a), Some(b)) = (a.cft_core_props(), b.cft_core_props()) {
                assert_ne!(a.area, b.area);
                assert_ne!(a.iy, b.iy);
                assert_ne!(a.iz, b.iz);
                assert_eq!(a.j, b.j);
            }
        }
    }
    #[test]
    fn 板組輪郭と円周を使い未対応形状はエラー() {
        let shape = SectionShape::SteelBuiltH {
            height: 400.0,
            upper_width: 200.0,
            upper_thick: 12.0,
            lower_width: 300.0,
            lower_thick: 16.0,
            web_thick: 8.0,
        };
        assert_eq!(shape.coating_surface_perimeter().unwrap(), 1784.0);
        assert_eq!(shape.coating_envelope_perimeter().unwrap(), 1400.0);
        for shape in [
            SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 12.0,
            },
        ] {
            assert_eq!(shape.coating_surface_perimeter().unwrap(), PI * 400.0);
            assert_eq!(shape.coating_envelope_perimeter().unwrap(), PI * 400.0);
        }
        assert!(SectionShape::SteelAngle {
            leg_a: 100.0,
            leg_b: 100.0,
            thick: 10.0
        }
        .coating_surface_perimeter()
        .is_err());
    }
}
