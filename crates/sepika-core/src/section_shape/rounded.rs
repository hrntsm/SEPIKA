use super::SectionShape;
use std::f64::consts::PI;

/// 材料領域の幾何性能。A `[mm²]`、Iy・Iz `[mm⁴]`、Zp `[mm³]`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoundedSectionProperties {
    pub area: f64,
    pub iy: f64,
    pub iz: f64,
    pub plastic_modulus_strong: f64,
}

impl RoundedSectionProperties {
    fn validate(self) -> Result<Self, String> {
        if [self.area, self.iy, self.iz, self.plastic_modulus_strong]
            .into_iter()
            .all(|v| v.is_finite() && v > 0.0)
        {
            Ok(self)
        } else {
            Err("断面幾何性能が有限な正値として算定できません。入力寸法の桁と単位を確認してください".into())
        }
    }
    fn subtract(self, inner: Self) -> Self {
        Self {
            area: self.area - inner.area,
            iy: self.iy - inner.iy,
            iz: self.iz - inner.iz,
            plastic_modulus_strong: self.plastic_modulus_strong - inner.plastic_modulus_strong,
        }
    }
}

pub(super) fn rounded_rectangle(h: f64, b: f64, r: f64) -> RoundedSectionProperties {
    let a = (1.0 - PI / 4.0) * r.powi(2);
    let m = r.powi(3) / 6.0;
    let q = (1.0 / 3.0 - PI / 16.0) * r.powi(4);
    let cy = h / 2.0 - r;
    let cz = b / 2.0 - r;
    RoundedSectionProperties {
        area: h * b - 4.0 * a,
        iy: b * h.powi(3) / 12.0 - 4.0 * (cy * cy * a + 2.0 * cy * m + q),
        iz: h * b.powi(3) / 12.0 - 4.0 * (cz * cz * a + 2.0 * cz * m + q),
        plastic_modulus_strong: b * h.powi(2) / 4.0 - 4.0 * (cy * a + m),
    }
}

/// 図心原点の角丸矩形。寸法・半径の単位は mm。
#[derive(Clone, Copy, Debug)]
pub struct RoundedRectangle {
    height: f64,
    width: f64,
    radius: f64,
}

impl RoundedRectangle {
    /// 正の有限寸法と `0≤radius≤min(height,width)/2` が必要。不正値はエラー。
    pub fn new(height: f64, width: f64, radius: f64) -> Result<Self, String> {
        if ![height, width, radius].into_iter().all(f64::is_finite)
            || height <= 0.0
            || width <= 0.0
            || radius < 0.0
            || radius > height.min(width) / 2.0
        {
            return Err("角丸矩形の寸法・半径が不正です".into());
        }
        rounded_rectangle(height, width, radius).validate()?;
        Ok(Self {
            height,
            width,
            radius,
        })
    }

    /// 正側縁から座標 `z` までの領域の (A `[mm²]`, ∫z dA `[mm³]`, ∫z² dA `[mm⁴]`)。
    /// `z` は図心から正側への有限距離 `[mm]` が前提。断面外は空領域または全領域となる。
    pub fn above(self, z: f64) -> (f64, f64, f64) {
        let p = rounded_rectangle(self.height, self.width, self.radius);
        let x = z.abs().min(self.height / 2.0);
        let a = self.height / 2.0 - self.radius;
        let flat = x.min(a);
        let mut moments = [
            self.width * flat,
            self.width * flat.powi(2) / 2.0,
            self.width * flat.powi(3) / 3.0,
        ];
        if x > a && self.radius > 0.0 {
            let r = self.radius;
            let u = (x - a).min(r);
            let root = (r * r - u * u).max(0.0).sqrt();
            let angle = (u / r).clamp(0.0, 1.0).asin();
            let f0 = (u * root + r * r * angle) / 2.0;
            let f1 = (r.powi(3) - root.powi(3)) / 3.0;
            let f2 = (u * (2.0 * u * u - r * r) * root + r.powi(4) * angle) / 8.0;
            let base = self.width - 2.0 * r;
            moments[0] += base * (x - a) + 2.0 * f0;
            moments[1] += base * (x * x - a * a) / 2.0 + 2.0 * (a * f0 + f1);
            moments[2] +=
                base * (x.powi(3) - a.powi(3)) / 3.0 + 2.0 * (a * a * f0 + 2.0 * a * f1 + f2);
        }
        if z >= self.height / 2.0 {
            return (0.0, 0.0, 0.0);
        }
        if z <= -self.height / 2.0 {
            return (p.area, 0.0, p.iy);
        }
        let sign = if z >= 0.0 { 1.0 } else { -1.0 };
        (
            p.area / 2.0 - sign * moments[0],
            p.plastic_modulus_strong / 2.0 - moments[1],
            p.iy / 2.0 - sign * moments[2],
        )
    }

    /// 全領域の A・両軸 I・せい方向の Zp。単位は [`RoundedSectionProperties`] と同じ。
    pub fn properties(self) -> RoundedSectionProperties {
        rounded_rectangle(self.height, self.width, self.radius)
    }
}

impl SectionShape {
    /// H のフィレット、角形鋼管・角形 CFT の角Rを反映した鋼材性能。
    /// 対象外は `Ok(None)`。必要な寸法が未知・不正ならエラー。
    pub fn rounded_steel_properties(&self) -> Result<Option<RoundedSectionProperties>, String> {
        self.validate_surface_radius()?;
        match *self {
            Self::SteelH {
                height: h,
                width: b,
                web_thick: tw,
                flange_thick: tf,
                root_r,
            } => {
                let r = root_r.ok_or("断面性能の算定に必要なフィレット半径が未知です")?;
                let hw = h - 2.0 * tf;
                let a = (1.0 - PI / 4.0) * r.powi(2);
                let m = r.powi(3) / 6.0;
                let q = (1.0 / 3.0 - PI / 16.0) * r.powi(4);
                let cy = hw / 2.0 - r;
                let cz = tw / 2.0 + r;
                Ok(Some(
                    RoundedSectionProperties {
                        area: 2.0 * b * tf + hw * tw + 4.0 * a,
                        iy: (b * h.powi(3) - (b - tw) * hw.powi(3)) / 12.0
                            + 4.0 * (cy * cy * a + 2.0 * cy * m + q),
                        iz: (2.0 * tf * b.powi(3) + hw * tw.powi(3)) / 12.0
                            + 4.0 * (cz * cz * a - 2.0 * cz * m + q),
                        plastic_modulus_strong: b * tf * (h - tf)
                            + tw * hw.powi(2) / 4.0
                            + 4.0 * (cy * a + m),
                    }
                    .validate()?,
                ))
            }
            Self::SteelBox {
                height: h,
                width: b,
                thick: t,
                horizontal_thick: t2,
                corner_r,
            }
            | Self::CftBox {
                height: h,
                width: b,
                thick: t,
                horizontal_thick: t2,
                corner_r,
            } => {
                let ro = corner_r.ok_or("断面性能の算定に必要な角Rが未知です")?;
                Ok(Some(
                    rounded_rectangle(h, b, ro)
                        .subtract(rounded_rectangle(
                            h - 2.0 * t2,
                            b - 2.0 * t,
                            (ro - t).max(0.0),
                        ))
                        .validate()?,
                ))
            }
            _ => Ok(None),
        }
    }

    /// 角形 CFT の内角Rを反映した充填コンクリートの幾何性能。
    /// 対象外は `Ok(None)`。角Rが未知・不正ならエラー。
    pub fn rounded_core_properties(&self) -> Result<Option<RoundedSectionProperties>, String> {
        let Self::CftBox {
            height: h,
            width: b,
            thick: t,
            horizontal_thick: t2,
            corner_r,
        } = *self
        else {
            return Ok(None);
        };
        self.validate_surface_radius()?;
        let ro = corner_r.ok_or("コア領域の算定に必要な角Rが未知です")?;
        Ok(Some(
            rounded_rectangle(h - 2.0 * t2, b - 2.0 * t, (ro - t).max(0.0)).validate()?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn integrate(h: f64, width_at: impl Fn(f64) -> f64) -> (f64, f64, f64, f64) {
        let n = 262_144;
        let dt = PI / n as f64;
        let mut p = (0.0, 0.0, 0.0, 0.0);
        for i in 0..n {
            let theta = -PI / 2.0 + (i as f64 + 0.5) * dt;
            let z = h / 2.0 * theta.sin();
            let dz = h / 2.0 * theta.cos() * dt;
            let b = width_at(z);
            p.0 += b * dz;
            p.1 += b * z * z * dz;
            p.2 += b.powi(3) / 12.0 * dz;
            p.3 += b * z.abs() * dz;
        }
        p
    }

    fn width(h: f64, b: f64, r: f64, z: f64) -> f64 {
        if z.abs() >= h / 2.0 {
            0.0
        } else if z.abs() <= h / 2.0 - r {
            b
        } else {
            b - 2.0 * r + 2.0 * (r * r - (z.abs() - h / 2.0 + r).powi(2)).sqrt()
        }
    }

    fn compare(p: RoundedSectionProperties, reference: (f64, f64, f64, f64)) {
        for (name, actual, expected) in [
            ("A", p.area, reference.0),
            ("Iy", p.iy, reference.1),
            ("Iz", p.iz, reference.2),
            ("Zp", p.plastic_modulus_strong, reference.3),
        ] {
            let error = (actual / expected - 1.0).abs();
            eprintln!(
                "{name}: actual={actual:.12e}, reference={expected:.12e}, relative={error:.12e}"
            );
            assert!(error < 2.0e-8, "{name}: {error}");
        }
    }

    #[test]
    fn material_properties_converge_as_radius_steps_shrink() {
        for (h, b) in [(400.0_f64, 400.0_f64), (600.0, 400.0), (400.0, 600.0)] {
            let props = |r| {
                let shape = SectionShape::CftBox {
                    height: h,
                    width: b,
                    thick: 16.0,
                    horizontal_thick: 16.0,
                    corner_r: Some(r),
                };
                let steel = shape.rounded_steel_properties().unwrap().unwrap();
                let core = shape.rounded_core_properties().unwrap().unwrap();
                [
                    steel.area,
                    steel.iy,
                    steel.iz,
                    steel.iy * 2.0 / h,
                    steel.iz * 2.0 / b,
                    steel.plastic_modulus_strong,
                    core.area,
                    core.iy,
                    core.iz,
                    core.plastic_modulus_strong,
                ]
            };
            for boundary in [0.0, 16.0] {
                let at = props(boundary);
                let mut previous = [f64::INFINITY; 10];
                for eps in [0.1_f64, 0.01, 0.001] {
                    let left = props((boundary - eps).max(0.0));
                    let right = props(boundary + eps);
                    for j in 0..at.len() {
                        let error = (left[j] - at[j]).abs().max((right[j] - at[j]).abs());
                        assert!(error == 0.0 || error < previous[j] * 0.2);
                        previous[j] = error;
                    }
                }
                for j in 0..at.len() {
                    assert!(previous[j] / at[j] < 1e-5);
                }
            }
        }
    }

    #[test]
    fn clipped_moments_match_independent_width_quadrature() {
        for (h, b) in [(400.0_f64, 400.0_f64), (600.0, 400.0), (400.0, 600.0)] {
            for r in [0.0, 8.0, 16.0, 40.0, b.min(h) / 2.0] {
                let domain = RoundedRectangle::new(h, b, r).unwrap();
                for z in [
                    -h / 2.0,
                    -h / 2.0 + r,
                    -1.0,
                    0.0,
                    1.0,
                    h / 2.0 - r,
                    h / 2.0 - 0.01,
                    h / 2.0,
                ] {
                    let actual = domain.above(z);
                    let mut previous = None;
                    for n in [65_536, 131_072] {
                        let start = (z / (h / 2.0)).clamp(-1.0, 1.0).asin();
                        let dt = (PI / 2.0 - start) / n as f64;
                        let mut expected = [0.0; 3];
                        for k in 0..n {
                            let theta = start + (k as f64 + 0.5) * dt;
                            let y = h / 2.0 * theta.sin();
                            let da = width(h, b, r, y) * h / 2.0 * theta.cos() * dt;
                            for (j, m) in expected.iter_mut().enumerate() {
                                *m += da * y.powi(j as i32);
                            }
                        }
                        for (j, (a, e)) in [actual.0, actual.1, actual.2]
                            .into_iter()
                            .zip(expected)
                            .enumerate()
                        {
                            let tolerance = 1e-9 * b * h.powi(j as i32 + 1);
                            assert!(
                                (a - e).abs() < tolerance,
                                "h={h},b={b},r={r},z={z},k={j}: {a}/{e}"
                            );
                            if let Some(prev) = previous {
                                let prev: [f64; 3] = prev;
                                assert!((e - prev[j]).abs() < tolerance);
                            }
                        }
                        previous = Some(expected);
                    }
                }
            }
        }
    }

    #[test]
    fn box_and_core_independent_integration() {
        for (h, b) in [(400.0_f64, 400.0_f64), (500.0, 300.0)] {
            for r in [0.0, 5.0, 10.0, 30.0, b / 2.0] {
                let t = 10.0;
                let ri = (r - t).max(0.0);
                let shape = SectionShape::CftBox {
                    height: h,
                    width: b,
                    thick: t,
                    horizontal_thick: t,
                    corner_r: Some(r),
                };
                let steel = shape.rounded_steel_properties().unwrap().unwrap();
                let core = shape.rounded_core_properties().unwrap().unwrap();
                let outer_ref = integrate(h, |z| width(h, b, r, z));
                let core_ref = integrate(h - 2.0 * t, |z| width(h - 2.0 * t, b - 2.0 * t, ri, z));
                compare(core, core_ref);
                compare(
                    steel,
                    (
                        outer_ref.0 - core_ref.0,
                        outer_ref.1 - core_ref.1,
                        outer_ref.2 - core_ref.2,
                        outer_ref.3 - core_ref.3,
                    ),
                );
                let outer = rounded_rectangle(h, b, r);
                for (sum, total) in [
                    (steel.area + core.area, outer.area),
                    (steel.iy + core.iy, outer.iy),
                    (steel.iz + core.iz, outer.iz),
                ] {
                    assert!((sum / total - 1.0).abs() < 1.0e-14);
                }
            }
        }
    }

    #[test]
    fn h_independent_integration() {
        let (h, b, tw, tf) = (400.0, 200.0, 9.0, 12.0);
        for r in [0.0, 13.0, (b - tw) / 2.0] {
            let shape = SectionShape::SteelH {
                height: h,
                width: b,
                web_thick: tw,
                flange_thick: tf,
                root_r: Some(r),
            };
            let hw = h - 2.0 * tf;
            let middle = integrate(hw, |z| {
                let d = hw / 2.0 - z.abs();
                if d >= r {
                    tw
                } else {
                    tw + 2.0 * (r - (r * r - (r - d).powi(2)).sqrt())
                }
            });
            compare(
                shape.rounded_steel_properties().unwrap().unwrap(),
                (
                    middle.0 + 2.0 * b * tf,
                    middle.1 + 2.0 * (b * tf.powi(3) / 12.0 + b * tf * ((h - tf) / 2.0).powi(2)),
                    middle.2 + 2.0 * tf * b.powi(3) / 12.0,
                    middle.3 + b * tf * (h - tf),
                ),
            );
        }
    }

    #[test]
    fn unknown_and_invalid_dimensions_are_errors() {
        for r in [
            None,
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(151.0),
        ] {
            let shape = SectionShape::CftBox {
                height: 500.0,
                width: 300.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: r,
            };
            assert!(shape.rounded_steel_properties().is_err());
            assert!(shape.rounded_core_properties().is_err());
        }
        for r in [
            None,
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(96.0),
        ] {
            let shape = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 9.0,
                flange_thick: 12.0,
                root_r: r,
            };
            assert!(shape.rounded_steel_properties().is_err());
        }
    }

    #[test]
    fn unrepresentable_geometry_returns_error_instead_of_nonfinite_properties() {
        let shape = SectionShape::SteelBox {
            height: 1.0e200,
            width: 1.0e200,
            thick: 1.0e199,
            horizontal_thick: 1.0e199,
            corner_r: Some(0.0),
        };
        assert!(shape.try_calc_area().is_err());
        assert!(shape.try_calc_iy().is_err());
        assert!(shape.try_calc_iz().is_err());
    }

    #[test]
    fn unknown_input_keeps_shape_and_pending_properties_until_radius_is_resolved() {
        use crate::ids::SectionId;
        use crate::model::PropertyBasis;
        for shape in [
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 9.0,
                flange_thick: 12.0,
                root_r: None,
            },
            SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: None,
            },
            SectionShape::CftBox {
                height: 400.0,
                width: 300.0,
                thick: 10.0,
                horizontal_thick: 10.0,
                corner_r: None,
            },
        ] {
            assert!(shape.try_to_section(SectionId(0), "未入力".into()).is_err());
            let pending = shape.input_section(SectionId(0), "未入力".into()).unwrap();
            assert_eq!(pending.shape.as_ref(), Some(&shape));
            assert_eq!(pending.property_basis.area, PropertyBasis::PendingShape);
            assert!(pending.resolved_area().is_err());
            assert!(pending.ensure_properties_resolved().is_err());
            let decoded: crate::model::Section =
                serde_json::from_str(&serde_json::to_string(&pending).unwrap()).unwrap();
            assert_eq!(decoded, pending);
            let resolved = decoded.with_surface_radius(Some(0.0)).unwrap();
            assert_eq!(resolved.property_basis.area, PropertyBasis::Shape);
            assert_eq!(
                resolved.resolved_area().unwrap(),
                resolved.shape.as_ref().unwrap().calc_area()
            );
            assert!(resolved.ensure_properties_resolved().is_ok());
        }
    }
}
