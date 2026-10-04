use super::SectionShape;

#[derive(Clone, Copy, Debug)]
pub enum Primitive {
    Rectangle { y: [f64; 2], z: [f64; 2] },
    Circle { center: [f64; 2], radius: f64 },
}

impl Primitive {
    pub fn interval(self, x: f64, direction: [f64; 2]) -> Option<[f64; 2]> {
        let [a, b] = direction;
        match self {
            Self::Circle {
                center: [y, z],
                radius,
            } => {
                let dx = x - a * y - b * z;
                let square = radius * radius - dx * dx;
                if square <= 0.0 {
                    return None;
                }
                let center = -b * y + a * z;
                let half = square.sqrt();
                Some([center - half, center + half])
            }
            Self::Rectangle { y, z } => {
                let mut span = [f64::NEG_INFINITY, f64::INFINITY];
                for (bounds, offset, slope) in [(y, a * x, -b), (z, b * x, a)] {
                    if slope.abs() < 1e-14 {
                        if offset < bounds[0] || offset > bounds[1] {
                            return None;
                        }
                    } else {
                        let lo = (bounds[0] - offset) / slope;
                        let hi = (bounds[1] - offset) / slope;
                        span[0] = span[0].max(lo.min(hi));
                        span[1] = span[1].min(lo.max(hi));
                    }
                }
                (span[1] > span[0]).then_some(span)
            }
        }
    }

    pub fn breaks(self, direction: [f64; 2]) -> Vec<f64> {
        let [a, b] = direction;
        match self {
            Self::Rectangle { y, z } => y
                .into_iter()
                .flat_map(|y| z.map(|z| a * y + b * z))
                .collect(),
            Self::Circle {
                center: [y, z],
                radius,
            } => vec![a * y + b * z - radius, a * y + b * z + radius],
        }
    }
}

/// 部材局所y・z座標の材料領域。後の領域が優先し、Noneは空洞を表す。
pub struct SectionGeometry {
    pub parts: Vec<(Primitive, Option<usize>)>,
}

impl SectionGeometry {
    /// 図心基準の実断面を指定方向へ投影した外側範囲 [mm]。空洞は除外する。
    pub fn projected_range(&self, direction: [f64; 2]) -> Result<[f64; 2], String> {
        let norm = direction[0].hypot(direction[1]);
        if !norm.is_finite() || norm <= 1e-12 {
            return Err("断面の投影方向が不正です".into());
        }
        let mut range = [f64::INFINITY, f64::NEG_INFINITY];
        for (primitive, material) in &self.parts {
            if material.is_none() {
                continue;
            }
            let bounds = match *primitive {
                Primitive::Rectangle { .. } => primitive.breaks(direction),
                Primitive::Circle { center, radius } => {
                    let mid = center[0] * direction[0] + center[1] * direction[1];
                    vec![mid - radius * norm, mid + radius * norm]
                }
            };
            for value in bounds {
                range[0] = range[0].min(value);
                range[1] = range[1].max(value);
            }
        }
        if !range.iter().all(|v| v.is_finite()) || range[1] <= range[0] {
            return Err("断面の投影外側範囲を解決できません".into());
        }
        Ok(range)
    }

    /// 図心を通る方向ベクトルに沿った外側フェースまでの距離 [mm]。
    /// 空洞・内側材料境界は除外し、正方向の最外交点を返す。
    pub fn face_distance(&self, direction: [f64; 2]) -> Result<f64, String> {
        let norm = direction[0].hypot(direction[1]);
        if !norm.is_finite() || norm <= 1e-12 {
            return Err("支持部材と材軸が平行でフェース距離を解決できません".into());
        }
        let normal = [direction[1] / norm, -direction[0] / norm];
        let distance = self
            .parts
            .iter()
            .filter(|(_, material)| material.is_some())
            .filter_map(|(primitive, _)| primitive.interval(0.0, normal))
            .map(|interval| interval[1] / norm)
            .fold(f64::NEG_INFINITY, f64::max);
        if !distance.is_finite() || distance <= 0.0 {
            return Err("材軸から正方向の外側フェースを解決できません".into());
        }
        Ok(distance)
    }

    pub fn of(shape: &SectionShape) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut rect = |y: [f64; 2], z: [f64; 2], material| {
            parts.push((Primitive::Rectangle { y, z }, material));
        };
        match *shape {
            SectionShape::RcBeamRect { b, d, .. }
            | SectionShape::RcColumnRect { b, d, .. }
            | SectionShape::SrcBeamRect { b, d, .. }
            | SectionShape::SrcColumnRect { b, d, .. } => {
                rect([-d / 2.0, d / 2.0], [-b / 2.0, b / 2.0], Some(0));
                if let SectionShape::SrcBeamRect {
                    steel_height: h,
                    steel_width: w,
                    steel_web_thick: tw,
                    steel_flange_thick: tf,
                    ..
                }
                | SectionShape::SrcColumnRect {
                    steel_height: h,
                    steel_width: w,
                    steel_web_thick: tw,
                    steel_flange_thick: tf,
                    ..
                } = *shape
                {
                    if h > d || w > b || h <= 2.0 * tf || w < tw {
                        return Err("SRC内蔵鉄骨の寸法が不正です".into());
                    }
                    rect([-h / 2.0, -h / 2.0 + tf], [-w / 2.0, w / 2.0], Some(1));
                    rect([h / 2.0 - tf, h / 2.0], [-w / 2.0, w / 2.0], Some(1));
                    rect(
                        [-h / 2.0 + tf, h / 2.0 - tf],
                        [-tw / 2.0, tw / 2.0],
                        Some(1),
                    );
                }
            }
            SectionShape::CftBox {
                height: h,
                width: w,
                thick: t,
            } => {
                if t <= 0.0 || 2.0 * t >= h.min(w) {
                    return Err("CFT鋼管厚が不正です".into());
                }
                rect([-h / 2.0, h / 2.0], [-w / 2.0, w / 2.0], Some(0));
                rect(
                    [-h / 2.0 + t, h / 2.0 - t],
                    [-w / 2.0 + t, w / 2.0 - t],
                    Some(1),
                );
            }
            SectionShape::SteelH {
                height: h,
                width: w,
                web_thick: tw,
                flange_thick: tf,
            } => {
                rect([-h / 2.0, -h / 2.0 + tf], [-w / 2.0, w / 2.0], Some(0));
                rect([h / 2.0 - tf, h / 2.0], [-w / 2.0, w / 2.0], Some(0));
                rect(
                    [-h / 2.0 + tf, h / 2.0 - tf],
                    [-tw / 2.0, tw / 2.0],
                    Some(0),
                );
            }
            SectionShape::SteelFlatBar { width: w, thick: h } => {
                rect([-h / 2.0, h / 2.0], [-w / 2.0, w / 2.0], Some(0))
            }
            SectionShape::SteelBuiltH {
                height: h,
                upper_width: uw,
                upper_thick: ut,
                lower_width: lw,
                lower_thick: lt,
                web_thick: tw,
            } => {
                rect([0.0, lt], [-lw / 2.0, lw / 2.0], Some(0));
                rect([h - ut, h], [-uw / 2.0, uw / 2.0], Some(0));
                rect([lt, h - ut], [-tw / 2.0, tw / 2.0], Some(0));
                center_rectangles(&mut parts);
            }
            SectionShape::SteelAngle {
                leg_a: a,
                leg_b: b,
                thick: t,
            } => {
                rect([0.0, a], [0.0, t], Some(0));
                rect([0.0, t], [t, b], Some(0));
                center_rectangles(&mut parts);
            }
            SectionShape::SteelTee {
                height: h,
                width: w,
                web_thick: tw,
                flange_thick: tf,
            } => {
                rect([0.0, h - tf], [-tw / 2.0, tw / 2.0], Some(0));
                rect([h - tf, h], [-w / 2.0, w / 2.0], Some(0));
                center_rectangles(&mut parts);
            }
            SectionShape::SteelChannel {
                height: h,
                width: w,
                web_thick: tw,
                flange_thick: tf,
            } => {
                rect([0.0, tf], [0.0, w], Some(0));
                rect([h - tf, h], [0.0, w], Some(0));
                rect([tf, h - tf], [0.0, tw], Some(0));
                center_rectangles(&mut parts);
            }
            SectionShape::SteelLipChannel {
                height: h,
                width: w,
                lip: l,
                thick: t,
            } => {
                if l > h / 2.0 {
                    return Err("リップが重なる断面です".into());
                }
                rect([0.0, h], [0.0, t], Some(0));
                rect([0.0, t], [t, w], Some(0));
                rect([h - t, h], [t, w], Some(0));
                rect([t, l], [w - t, w], Some(0));
                rect([h - l, h - t], [w - t, w], Some(0));
                center_rectangles(&mut parts);
            }
            SectionShape::RcColumnCircle { d, .. } | SectionShape::SteelRoundBar { dia: d } => {
                parts.push((
                    Primitive::Circle {
                        center: [0.0; 2],
                        radius: d / 2.0,
                    },
                    Some(0),
                ));
            }
            SectionShape::SteelPipe {
                outer_dia: d,
                thick: t,
            }
            | SectionShape::CftPipe {
                outer_dia: d,
                thick: t,
            } => {
                if t <= 0.0 {
                    return Err("鋼管厚が不正です".into());
                }
                parts.push((
                    Primitive::Circle {
                        center: [0.0; 2],
                        radius: d / 2.0,
                    },
                    Some(0),
                ));
                parts.push((
                    Primitive::Circle {
                        center: [0.0; 2],
                        radius: d / 2.0 - t,
                    },
                    if matches!(shape, SectionShape::CftPipe { .. }) {
                        Some(1)
                    } else {
                        None
                    },
                ));
            }
            SectionShape::SteelBox {
                height: h,
                width: w,
                thick: t,
                corner_r: r,
            } => {
                if t <= 0.0 || r < 0.0 || r > h.min(w) / 2.0 {
                    return Err("角形鋼管の寸法が不正です".into());
                }
                rounded_rectangle(&mut parts, h, w, r, Some(0));
                rounded_rectangle(&mut parts, h - 2.0 * t, w - 2.0 * t, (r - t).max(0.0), None);
            }
            SectionShape::RcWall { .. } | SectionShape::RcSlab { .. } => {
                return Err("側柱に壁・床の断面は指定できません".into())
            }
        }
        if parts.iter().any(|(p, _)| match p {
            Primitive::Rectangle { y, z } => {
                !y.iter().chain(z).all(|v| v.is_finite()) || y[1] <= y[0] || z[1] <= z[0]
            }
            Primitive::Circle { center, radius } => {
                !center.iter().all(|v| v.is_finite()) || !radius.is_finite() || *radius <= 0.0
            }
        }) {
            return Err("側柱の断面寸法が不正です".into());
        }
        Ok(Self { parts })
    }
}

fn center_rectangles(parts: &mut [(Primitive, Option<usize>)]) {
    let mut area = 0.0;
    let mut moment = [0.0; 2];
    for (p, _) in parts.iter() {
        if let Primitive::Rectangle { y, z } = p {
            let a = (y[1] - y[0]) * (z[1] - z[0]);
            area += a;
            moment[0] += a * (y[0] + y[1]) / 2.0;
            moment[1] += a * (z[0] + z[1]) / 2.0;
        }
    }
    for (p, _) in parts {
        if let Primitive::Rectangle { y, z } = p {
            for v in y {
                *v -= moment[0] / area;
            }
            for v in z {
                *v -= moment[1] / area;
            }
        }
    }
}

fn rounded_rectangle(
    parts: &mut Vec<(Primitive, Option<usize>)>,
    h: f64,
    w: f64,
    r: f64,
    material: Option<usize>,
) {
    if r == 0.0 {
        parts.push((
            Primitive::Rectangle {
                y: [-h / 2.0, h / 2.0],
                z: [-w / 2.0, w / 2.0],
            },
            material,
        ));
        return;
    }
    if h > 2.0 * r {
        parts.push((
            Primitive::Rectangle {
                y: [-h / 2.0 + r, h / 2.0 - r],
                z: [-w / 2.0, w / 2.0],
            },
            material,
        ));
    }
    if w > 2.0 * r {
        parts.push((
            Primitive::Rectangle {
                y: [-h / 2.0, h / 2.0],
                z: [-w / 2.0 + r, w / 2.0 - r],
            },
            material,
        ));
    }
    for y in [-h / 2.0 + r, h / 2.0 - r] {
        for z in [-w / 2.0 + r, w / 2.0 - r] {
            parts.push((
                Primitive::Circle {
                    center: [y, z],
                    radius: r,
                },
                material,
            ));
        }
    }
}

#[cfg(test)]
mod face_tests {
    use super::*;

    #[test]
    fn 中空断面は内面でなく外側フェースを使い丸角も実形状で求める() {
        for shape in [
            SectionShape::SteelPipe {
                outer_dia: 400.0,
                thick: 20.0,
            },
            SectionShape::CftPipe {
                outer_dia: 400.0,
                thick: 20.0,
            },
            SectionShape::SteelRoundBar { dia: 400.0 },
        ] {
            let geometry = SectionGeometry::of(&shape).unwrap();
            assert!((geometry.face_distance([0.6, 0.8]).unwrap() - 200.0).abs() < 1e-9);
            assert_eq!(
                geometry.projected_range([0.6, 0.8]).unwrap(),
                [-200.0, 200.0]
            );
        }
        let shape = SectionShape::SteelBox {
            height: 400.0,
            width: 400.0,
            thick: 20.0,
            corner_r: 40.0,
        };
        let geometry = SectionGeometry::of(&shape).unwrap();
        let d = std::f64::consts::FRAC_1_SQRT_2;
        let expected = 160.0 * 2.0_f64.sqrt() + 40.0;
        assert!((geometry.face_distance([d, d]).unwrap() - expected).abs() < 1e-9);
        assert_eq!(geometry.face_distance([0.0, 1.0]).unwrap(), 200.0);
        let range = geometry.projected_range([d, d]).unwrap();
        assert!((range[1] - expected).abs() < 1e-9 && (range[0] + expected).abs() < 1e-9);
    }

    #[test]
    fn 非対称tと組立hは図心から取付き側別の距離を返す() {
        let shapes = [
            (
                SectionShape::SteelTee {
                    height: 300.0,
                    width: 200.0,
                    web_thick: 10.0,
                    flange_thick: 20.0,
                },
                (2800.0 * 140.0 + 4000.0 * 290.0) / 6800.0,
            ),
            (
                SectionShape::SteelBuiltH {
                    height: 300.0,
                    upper_width: 200.0,
                    upper_thick: 20.0,
                    lower_width: 100.0,
                    lower_thick: 10.0,
                    web_thick: 10.0,
                },
                (1000.0 * 5.0 + 2700.0 * 145.0 + 4000.0 * 290.0) / 7700.0,
            ),
        ];
        for (shape, centroid) in shapes {
            let geometry = SectionGeometry::of(&shape).unwrap();
            assert!(
                (geometry.face_distance([1.0, 0.0]).unwrap() - (300.0 - centroid)).abs() < 1e-9
            );
            assert!((geometry.face_distance([-1.0, 0.0]).unwrap() - centroid).abs() < 1e-9);
        }
    }

    #[test]
    fn 柱外形の交点と小梁用投影境界を区別する() {
        let h = SectionGeometry::of(&SectionShape::SteelH {
            height: 300.0,
            width: 200.0,
            web_thick: 10.0,
            flange_thick: 20.0,
        })
        .unwrap();
        assert_eq!(h.face_distance([0.0, 1.0]).unwrap(), 5.0);
        assert_eq!(h.face_distance([1.0, 0.0]).unwrap(), 150.0);
        assert_eq!(h.projected_range([0.0, 1.0]).unwrap(), [-100.0, 100.0]);
        let channel = SectionGeometry::of(&SectionShape::SteelChannel {
            height: 300.0,
            width: 100.0,
            web_thick: 10.0,
            flange_thick: 20.0,
        })
        .unwrap();
        let centroid = (4000.0 * 50.0 + 2600.0 * 5.0) / 6600.0;
        assert!((channel.face_distance([0.0, -1.0]).unwrap() - centroid).abs() < 1e-9);
        assert!(channel.face_distance([0.0, 1.0]).is_err());
        let range = channel.projected_range([0.0, 1.0]).unwrap();
        assert!((range[0] + centroid).abs() < 1e-9 && (range[1] - (100.0 - centroid)).abs() < 1e-9);
        let angle = SectionGeometry::of(&SectionShape::SteelAngle {
            leg_a: 100.0,
            leg_b: 80.0,
            thick: 10.0,
        })
        .unwrap();
        let cy = (1000.0 * 50.0 + 700.0 * 5.0) / 1700.0;
        assert!((angle.face_distance([-1.0, 0.0]).unwrap() - cy).abs() < 1e-9);
        let range = angle.projected_range([1.0, 0.0]).unwrap();
        assert!((range[0] + cy).abs() < 1e-9 && (range[1] - (100.0 - cy)).abs() < 1e-9);
        let lip = SectionGeometry::of(&SectionShape::SteelLipChannel {
            height: 300.0,
            width: 100.0,
            lip: 30.0,
            thick: 10.0,
        })
        .unwrap();
        assert!(lip.face_distance([0.0, -1.0]).unwrap() > 0.0);
        assert!(lip.face_distance([0.0, 1.0]).is_err());
        let range = lip.projected_range([0.0, 1.0]).unwrap();
        assert!((range[1] - range[0] - 100.0).abs() < 1e-9);
    }
}
