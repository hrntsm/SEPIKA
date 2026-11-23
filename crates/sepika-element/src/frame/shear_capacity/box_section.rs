use super::ShearLimit;
use sepika_core::material_grade::resolve_steel_f;
use sepika_core::model::Material;

#[derive(Clone, Copy, Debug)]
struct Plate {
    start_mm: [f64; 2],
    end_mm: [f64; 2],
    corner_radius_mm: f64,
    thickness_mm: f64,
    yield_stress_mpa: f64,
}

impl Plate {
    fn length_mm(&self) -> f64 {
        if self.corner_radius_mm == 0.0 {
            (self.end_mm[0] - self.start_mm[0]).hypot(self.end_mm[1] - self.start_mm[1])
        } else {
            self.corner_radius_mm * std::f64::consts::FRAC_PI_2
        }
    }

    fn point_mm(&self, t: f64) -> [f64; 2] {
        let r = self.corner_radius_mm;
        if r == 0.0 {
            return std::array::from_fn(|i| {
                self.start_mm[i] + t * (self.end_mm[i] - self.start_mm[i])
            });
        }
        let angle = t * std::f64::consts::FRAC_PI_2;
        let (sin, cos) = angle.sin_cos();
        [self.start_mm[0] + r * sin, self.end_mm[1] + r * cos]
    }

    fn first_moments_mm3(&self, t: f64) -> [f64; 2] {
        let r = self.corner_radius_mm;
        let length_mm = self.length_mm() * t;
        if r == 0.0 {
            let point = self.point_mm(t);
            return std::array::from_fn(|i| {
                self.thickness_mm * length_mm * self.start_mm[i].midpoint(point[i])
            });
        }
        let angle = t * std::f64::consts::FRAC_PI_2;
        let (sin, cos) = angle.sin_cos();
        [
            self.thickness_mm * (length_mm * self.start_mm[0] + r * r * (1.0 - cos)),
            self.thickness_mm * (length_mm * self.end_mm[1] + r * r * sin),
        ]
    }

    fn second_moments_mm4(&self) -> [f64; 2] {
        let r = self.corner_radius_mm;
        let length_mm = self.length_mm();
        if r == 0.0 {
            return std::array::from_fn(|i| {
                let a = self.start_mm[i];
                let b = self.end_mm[i];
                self.thickness_mm * length_mm * (a * a + a * b + b * b) / 3.0
            });
        }
        [self.start_mm[0], self.end_mm[1]].map(|center_mm| {
            self.thickness_mm
                * (length_mm * (center_mm * center_mm + r * r / 2.0) + 2.0 * center_mm * r * r)
        })
    }

    fn critical_position(&self, scaled_q: [f64; 2]) -> f64 {
        let gradient = |p: [f64; 2]| scaled_q[1] * p[1] - scaled_q[0] * p[0];
        let start = gradient(self.start_mm);
        let end = gradient(self.end_mm);
        if start <= 0.0 {
            return 0.0;
        }
        if end >= 0.0 {
            return 1.0;
        }
        if self.corner_radius_mm == 0.0 {
            return start / (start - end);
        }
        let [a, b] = scaled_q;
        let center_balance = gradient([self.start_mm[0], self.end_mm[1]]);
        let normalized = center_balance / (self.corner_radius_mm * a.hypot(b));
        let angle = b.atan2(a) + normalized.clamp(-1.0, 1.0).asin();
        (angle / std::f64::consts::FRAC_PI_2).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug)]
pub(super) struct BoxSection {
    plates: Vec<Plate>,
    first_moments_mm3: [f64; 2],
    second_moments_mm4: [f64; 2],
}

impl BoxSection {
    pub(super) fn new(
        height_mm: f64,
        width_mm: f64,
        vertical_thick_mm: f64,
        horizontal_thick_mm: f64,
        outside_corner_radius_mm: Option<f64>,
        material: &Material,
    ) -> Result<Self, String> {
        let outside_radius_mm = outside_corner_radius_mm.ok_or("角形鋼管の外角Rが未確定です")?;
        let half_height = (height_mm - horizontal_thick_mm) / 2.0;
        let half_width = (width_mm - vertical_thick_mm) / 2.0;
        if [
            half_height,
            half_width,
            vertical_thick_mm,
            horizontal_thick_mm,
        ]
        .into_iter()
        .any(|value| !value.is_finite() || value <= 0.0)
            || height_mm <= 2.0 * horizontal_thick_mm
            || width_mm <= 2.0 * vertical_thick_mm
            || !outside_radius_mm.is_finite()
            || !(0.0..height_mm.min(width_mm) / 2.0).contains(&outside_radius_mm)
        {
            return Err("角形鋼管のせん断流を求める断面寸法が不正です".into());
        }
        let r = if outside_radius_mm == 0.0 {
            0.0
        } else if vertical_thick_mm == horizontal_thick_mm && outside_radius_mm >= vertical_thick_mm
        {
            outside_radius_mm - vertical_thick_mm / 2.0
        } else {
            return Err("角丸の板厚中心線には等厚で同心の内外角が必要です".into());
        };
        let fy_v = resolve_steel_f(Some(material), Some(vertical_thick_mm))?;
        let fy_h = resolve_steel_f(Some(material), Some(horizontal_thick_mm))?;
        let plate = |start_mm, end_mm, thickness_mm, fy_mpa: f64, corner_radius_mm| Plate {
            start_mm,
            end_mm,
            thickness_mm,
            yield_stress_mpa: fy_mpa / 3.0_f64.sqrt(),
            corner_radius_mm,
        };
        let mut plates = Vec::with_capacity(3);
        plates.push(plate(
            [0.0, half_width],
            [half_height - r, half_width],
            vertical_thick_mm,
            fy_v,
            0.0,
        ));
        if r > 0.0 {
            plates.push(plate(
                [half_height - r, half_width],
                [half_height, half_width - r],
                vertical_thick_mm,
                fy_v,
                r,
            ));
        }
        plates.push(plate(
            [half_height, half_width - r],
            [half_height, 0.0],
            horizontal_thick_mm,
            fy_h,
            0.0,
        ));
        let mut first_moments_mm3 = [0.0; 2];
        let mut second_moments_mm4 = [0.0; 2];
        for plate in &plates {
            let first = plate.first_moments_mm3(1.0);
            let second = plate.second_moments_mm4();
            for i in 0..2 {
                first_moments_mm3[i] += first[i];
                second_moments_mm4[i] += 4.0 * second[i];
            }
        }
        if first_moments_mm3
            .into_iter()
            .chain(second_moments_mm4)
            .any(|moment| !moment.is_finite() || moment <= 0.0)
        {
            return Err("角形鋼管の板厚中心線の断面諸量を算定できません".into());
        }
        Ok(Self {
            plates,
            first_moments_mm3,
            second_moments_mm4,
        })
    }

    pub(super) fn limit(&self, shear_n: [f64; 2]) -> ShearLimit {
        // 直交二対称断面では、四象限のうち各方向のせん断流が同符号の象限に最大絶対値がある。
        let abs_n = shear_n.map(f64::abs);
        let scaled_q = std::array::from_fn(|i| abs_n[i] / self.second_moments_mm4[i]);
        let mut integrated = [0.0; 2];
        let mut result = ShearLimit::zero();
        for plate in &self.plates {
            let t = plate.critical_position(scaled_q);
            let first = plate.first_moments_mm3(t);
            let resistance_n_per_mm = plate.thickness_mm * plate.yield_stress_mpa;
            let coeff = [
                (self.first_moments_mm3[0] - integrated[0] - first[0]).max(0.0),
                integrated[1] + first[1],
            ];
            let per_n: [f64; 2] = std::array::from_fn(|i| {
                coeff[i] / self.second_moments_mm4[i] / resistance_n_per_mm
            });
            let utilization = per_n[0] * abs_n[0] + per_n[1] * abs_n[1];
            if utilization > result.utilization {
                result = ShearLimit {
                    utilization,
                    normal_per_n: std::array::from_fn(|i| per_n[i].copysign(shear_n[i])),
                };
            }
            let full = plate.first_moments_mm3(1.0);
            for i in 0..2 {
                integrated[i] += full[i];
            }
        }
        result
    }
}
