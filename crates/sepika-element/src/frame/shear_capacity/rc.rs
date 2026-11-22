use super::ShearLimit;
use sepika_core::material_grade::{
    has_jis_rebar_strength_limit, is_supported_shear_rebar_grade, rebar_f_value,
    rebar_material_strength_issue,
};
use sepika_core::model::{ElementData, Material, MaterialCategory, Model};
use sepika_core::rc_capacity::{
    rc_qsu_arakawa_mean_coefficients, rc_qsu_arakawa_mean_limit, RcCapacityInput,
};
use sepika_core::rc_rebar_geom::{RectEdge, SteelSide};
use sepika_core::section_shape::SectionShape;
use sepika_core::units::ConcreteClass;

#[derive(Clone, Debug)]
pub(super) struct RcSection {
    inputs: [Option<[RcCapacityInput; 2]>; 2],
    is_column: bool,
    gross_area_mm2: f64,
    axial_roundoff_n: f64,
}

struct ArakawaDemand {
    concrete_n: f64,
    reinforcement_and_axial_n: f64,
    moment_over_depth_n: f64,
}

impl ArakawaDemand {
    fn new(input: &RcCapacityInput, moment_nmm: f64) -> Result<Self, String> {
        let [concrete_n, reinforcement_and_axial_n] = rc_qsu_arakawa_mean_coefficients(input);
        let moment_over_depth_n = moment_nmm.abs() / input.d_eff;
        if !concrete_n.is_finite()
            || concrete_n <= 0.0
            || !reinforcement_and_axial_n.is_finite()
            || reinforcement_and_axial_n < 0.0
            || !moment_over_depth_n.is_finite()
            || !(concrete_n / 1.12 + reinforcement_and_axial_n).is_finite()
        {
            return Err("荒川mean式の終局せん断耐力の入力を確認できません".into());
        }
        Ok(Self {
            concrete_n,
            reinforcement_and_axial_n,
            moment_over_depth_n,
        })
    }

    fn resistance(&self, shear_n: f64) -> (f64, f64) {
        let span_ratio = if self.moment_over_depth_n == 0.0 {
            0.0
        } else {
            self.moment_over_depth_n / shear_n.abs()
        };
        let denominator = span_ratio.clamp(1.0, 3.0) + 0.12;
        let strength_n = self.concrete_n / denominator + self.reinforcement_and_axial_n;
        let ratio_gradient = if span_ratio > 1.0 && span_ratio < 3.0 {
            // d(|Q|/Qu)/d|Q|の分子 Qu - |Q|・dQu/d|Q| は、中間枝でも正。
            (self.concrete_n * (0.12 / denominator) / denominator + self.reinforcement_and_axial_n)
                / strength_n
        } else {
            1.0
        };
        (strength_n, ratio_gradient)
    }
}

struct RcDemand {
    axes: [Option<[ArakawaDemand; 2]>; 2],
}

impl RcDemand {
    fn resistances(&self, shear_n: [f64; 2]) -> [(f64, f64); 2] {
        std::array::from_fn(|axis| {
            let Some(sides) = &self.axes[axis] else {
                return (f64::INFINITY, 0.0);
            };
            let [a, b] = sides.each_ref().map(|side| side.resistance(shear_n[axis]));
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        })
    }

    fn utilization(&self, shear_n: [f64; 2]) -> f64 {
        let [y, z] = self.resistances(shear_n);
        (shear_n[0] / y.0).hypot(shear_n[1] / z.0)
    }
}

pub(super) fn ellipse_limit(shear_n: [f64; 2], resistances: [(f64, f64); 2]) -> ShearLimit {
    let ratios: [f64; 2] = std::array::from_fn(|i| shear_n[i] / resistances[i].0);
    let utilization = ratios[0].hypot(ratios[1]);
    if utilization == 0.0 {
        return ShearLimit::zero();
    }
    ShearLimit {
        utilization,
        normal_per_n: std::array::from_fn(|i| {
            ratios[i] / utilization / resistances[i].0 * resistances[i].1
        }),
    }
}

impl RcSection {
    pub(super) fn uses_axial_force(&self) -> bool {
        self.is_column
    }

    pub(super) fn from_model(
        model: &Model,
        elem: &ElementData,
        shape: &SectionShape,
        material: &Material,
    ) -> Result<Self, String> {
        let fc = material
            .fc
            .filter(|fc| (18.0..=60.0).contains(fc))
            .filter(|_| material.category == MaterialCategory::Concrete)
            .filter(|_| material.concrete_class == ConcreteClass::Normal)
            .ok_or("荒川mean式は普通コンクリートのFcが18〜60 N/mm²の範囲に適用します")?;
        let main_rebar = model
            .element_rebar_material(elem)
            .filter(|material| has_jis_rebar_strength_limit(material))
            .ok_or("荒川mean式に必要なJIS主筋を確認できません")?;
        if let Some(issue) = rebar_material_strength_issue(main_rebar) {
            return Err(issue);
        }
        let shear_rebar = model.element_shear_rebar_material(elem);
        let clear_span = model
            .analysis_rigid_zone(elem)
            .flexible_length_from(model.member_length(elem));
        let direction = |width_mm: f64,
                         depth_mm: f64,
                         sides: [SteelSide; 2],
                         reinforcement_area_mm2,
                         pitch_mm|
         -> Result<[RcCapacityInput; 2], String> {
            let (pw, sigma_wy) =
                shear_reinforcement(width_mm, reinforcement_area_mm2, pitch_mm, shear_rebar)?;
            if !clear_span.is_finite()
                || clear_span <= 0.0
                || [width_mm, depth_mm]
                    .into_iter()
                    .any(|v| !v.is_finite() || v <= 0.0)
                || sides.iter().any(|side| {
                    !side.area_mm2.is_finite()
                        || side.area_mm2 <= 0.0
                        || !side.effective_depth_mm.is_finite()
                        || side.effective_depth_mm <= 0.0
                        || side.effective_depth_mm > depth_mm
                })
            {
                return Err("荒川mean式に必要な断面・配筋・内法長を確認できません".into());
            }
            Ok(sides.map(|side| RcCapacityInput {
                b: width_mm,
                d: depth_mm,
                at: side.area_mm2,
                d_eff: side.effective_depth_mm,
                sigma_y: 0.0,
                fc,
                pw,
                sigma_wy,
                clear_span,
                sigma_0: 0.0,
            }))
        };

        let (inputs, b_mm, d_mm, is_column) = match shape {
            SectionShape::RcBeamRect { b, d, rebar } => {
                if model.beam_has_attached_slab(elem) {
                    return Err("スラブ付きRC梁の非線形せん断の適用条件を確認できません".into());
                }
                rebar.validate(*b, *d).map_err(|e| e.to_string())?;
                let strong = direction(
                    *b,
                    *d,
                    [true, false].map(|top| rebar.bending_steel(*d, top).tension),
                    rebar.aw_mm2(),
                    rebar.stirrup.pitch,
                )?;
                ([Some(strong), None], *b, *d, false)
            }
            SectionShape::RcColumnRect { b, d, rebar } => {
                rebar.validate(*b, *d).map_err(|e| e.to_string())?;
                let steel = |sides: [RectEdge; 2]| sides.map(|edge| rebar.edge_steel(edge, *b, *d));
                let strong = direction(
                    *b,
                    *d,
                    steel([RectEdge::Top, RectEdge::Bottom]),
                    rebar.aw_x_mm2(),
                    rebar.hoop.pitch,
                )?;
                let weak = direction(
                    *d,
                    *b,
                    // ファイバー配置では配筋の左側が局所 z 正側に写り、正の My で引張となる。
                    steel([RectEdge::Right, RectEdge::Left]),
                    rebar.aw_y_mm2(),
                    rebar.hoop.pitch,
                )?;
                ([Some(strong), Some(weak)], *b, *d, true)
            }
            _ => return Err("荒川mean式の終局せん断は矩形RC梁・柱に適用します".into()),
        };
        let gross_area_mm2 = b_mm * d_mm;
        let axial_roundoff_n = 64.0 * f64::EPSILON * gross_area_mm2 * fc;
        if !axial_roundoff_n.is_finite() {
            return Err("荒川mean式の断面積とコンクリート強度を確認できません".into());
        }
        Ok(Self {
            inputs,
            is_column,
            gross_area_mm2,
            axial_roundoff_n,
        })
    }

    fn axial_stress(&self, compression_n: f64) -> Result<f64, String> {
        if self.is_column && compression_n < -self.axial_roundoff_n {
            return Err("RC柱に引張軸力が作用して荒川mean式の適用範囲外です".into());
        }
        if !self.is_column && compression_n.abs() > self.axial_roundoff_n {
            return Err("RC梁に軸力が作用して荒川mean式の適用範囲外です".into());
        }
        Ok(if self.is_column {
            compression_n.max(0.0) / self.gross_area_mm2
        } else {
            0.0
        })
    }

    pub(super) fn validate_response(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
    ) -> Result<(), String> {
        self.axial_stress(axial_compression_n)?;
        if !self.is_column && shear_n[1].abs() > self.axial_roundoff_n {
            return Err("RC梁の弱軸せん断は荒川mean式の適用範囲外です".into());
        }
        Ok(())
    }

    pub(super) fn limit(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
    ) -> Result<ShearLimit, String> {
        let sigma_0 = self.axial_stress(axial_compression_n)?;
        Ok(ellipse_limit(
            shear_n,
            self.demand(sigma_0, moments_nmm, [false; 2])?
                .resistances(shear_n),
        ))
    }

    pub(super) fn bound_utilization(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
        unknown_sign: [bool; 2],
    ) -> Result<f64, String> {
        let sigma_0 = self.axial_stress(axial_compression_n)?;
        Ok(self
            .demand(sigma_0, moments_nmm, unknown_sign)?
            .utilization(shear_n))
    }

    fn tension_inputs(
        &self,
        axis: usize,
        moment_nmm: f64,
        unknown_sign: bool,
    ) -> Option<&[RcCapacityInput]> {
        let inputs = self.inputs[axis].as_ref()?;
        Some(if unknown_sign || moment_nmm == 0.0 {
            inputs
        } else if moment_nmm < 0.0 {
            &inputs[..1]
        } else {
            &inputs[1..]
        })
    }

    fn demand(
        &self,
        sigma_0: f64,
        moments_nmm: [f64; 2],
        unknown_sign: [bool; 2],
    ) -> Result<RcDemand, String> {
        let axis = |i: usize| -> Result<_, String> {
            let Some(inputs) = self.tension_inputs(i, moments_nmm[i], unknown_sign[i]) else {
                return Ok(None);
            };
            Ok(Some([
                ArakawaDemand::new(
                    &RcCapacityInput {
                        sigma_0,
                        ..inputs[0]
                    },
                    moments_nmm[i],
                )?,
                ArakawaDemand::new(
                    &RcCapacityInput {
                        sigma_0,
                        ..inputs[inputs.len() - 1]
                    },
                    moments_nmm[i],
                )?,
            ]))
        };
        Ok(RcDemand {
            axes: [axis(0)?, axis(1)?],
        })
    }

    pub(super) fn projection_axes(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
    ) -> Result<[f64; 2], String> {
        let sigma_0 = self.axial_stress(axial_compression_n)?;
        let demand = self.demand(sigma_0, moments_nmm, [false; 2])?;
        let shear_n: [f64; 2] = std::array::from_fn(|i| {
            if self.inputs[i].is_some() {
                shear_n[i]
            } else {
                0.0
            }
        });
        let limits_n = |q| demand.resistances(q).map(|(capacity_n, _)| capacity_n);
        let anchor = match shear_n.map(|q| q != 0.0) {
            [false, false] => [0.0; 2],
            [true, false] | [false, true] => {
                let axis = usize::from(shear_n[1] != 0.0);
                let inputs = self
                    .tension_inputs(axis, moments_nmm[axis], false)
                    .ok_or("せん断方向の荒川mean式の入力がありません")?;
                let mut limit_n = f64::INFINITY;
                for input in inputs {
                    let resistance_n = rc_qsu_arakawa_mean_limit(
                        &RcCapacityInput { sigma_0, ..*input },
                        moments_nmm[axis],
                    )
                    .ok_or("荒川mean式の終局せん断耐力を正の有限値として算定できません")?;
                    limit_n = limit_n.min(resistance_n);
                }
                let mut q = [0.0; 2];
                q[axis] = limit_n.copysign(shear_n[axis]);
                q
            }
            [true, true] => {
                let length_n = shear_n[0].hypot(shear_n[1]);
                let direction = shear_n.map(|q| q / length_n);
                let mut lo: f64 = 0.0;
                let mut hi = limits_n([f64::INFINITY; 2]).into_iter().fold(0.0, f64::max);
                if !length_n.is_finite() || !hi.is_finite() || hi <= 0.0 {
                    return Err("二軸せん断の耐力の範囲を確定できません".into());
                }
                for _ in 0..52 {
                    let mid = lo.midpoint(hi);
                    if demand.utilization(direction.map(|value| value * mid)) > 1.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                direction.map(|value| value * lo.midpoint(hi))
            }
        };
        Ok(limits_n(anchor))
    }
}

fn shear_reinforcement(
    width_mm: f64,
    area_mm2: f64,
    pitch_mm: f64,
    material: Option<&Material>,
) -> Result<(f64, f64), String> {
    if area_mm2 == 0.0 {
        return Ok((0.0, 0.0));
    }
    let pw = area_mm2 / (width_mm * pitch_mm);
    if !pitch_mm.is_finite() || pitch_mm <= 0.0 || !pw.is_finite() || pw <= 0.0 {
        return Err("荒川mean式に必要なせん断補強筋比と間隔を確認できません".into());
    }
    let material = material
        .filter(|m| {
            m.category == MaterialCategory::Rebar && is_supported_shear_rebar_grade(&m.name)
        })
        .ok_or("荒川mean式に必要なせん断補強筋の材種を確認できません")?;
    let grade = material.name.trim();
    let nominal_fy = rebar_f_value(grade).ok_or("せん断補強筋の基準強度を確認できません")?;
    let max_fy = if grade.starts_with("SR") {
        295.0
    } else {
        490.0
    };
    let fy = material
        .fy
        .filter(|f| f.is_finite() && (*f > 0.0 && *f <= nominal_fy.min(max_fy)))
        .ok_or("せん断補強筋の降伏強度が未確定、または荒川mean式の上限外です")?;
    Ok((pw, fy))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column() -> RcSection {
        let input = RcCapacityInput {
            b: 400.0,
            d: 600.0,
            at: 1935.0,
            d_eff: 530.0,
            sigma_y: 345.0,
            fc: 24.0,
            pw: 0.002,
            sigma_wy: 295.0,
            clear_span: 3000.0,
            sigma_0: 0.0,
        };
        RcSection {
            inputs: [Some([input; 2]); 2],
            is_column: true,
            gross_area_mm2: 240_000.0,
            axial_roundoff_n: 1e-7,
        }
    }

    #[test]
    fn compression_changes_the_shear_boundary_and_tension_is_inapplicable() {
        let section = column();
        let shear_n = [458_569.036_373_128, 0.0];
        let moments_nmm = [486_083_178.555_515_7, 0.0];
        let with_compression = section.limit(shear_n, 1_200_000.0, moments_nmm).unwrap();
        assert!((with_compression.utilization - 1.0).abs() < 1e-12);
        assert!(
            section
                .limit(shear_n, 0.0, moments_nmm)
                .unwrap()
                .utilization
                > 1.0
        );
        assert!(section.limit(shear_n, -1.0, moments_nmm).is_err());
    }

    #[test]
    fn rectangular_column_has_elliptic_shear_boundary() {
        let section = column();
        let limit_n = 584_307.046_010_279_6;
        let response = section
            .limit([0.6 * limit_n, -0.8 * limit_n], 0.0, [0.0; 2])
            .unwrap();
        assert!((response.utilization - 1.0).abs() < 1e-12);
        assert!((response.normal_per_n[0] / response.normal_per_n[1] + 0.75).abs() < 1e-12);
    }

    #[test]
    fn biaxial_limit_uses_each_actual_shear_in_its_span_ratio() {
        let section = column();
        let moments_nmm = [387_768_178.555_515_7; 2];
        let trial_shear_n = [258_673.121_306_567_07, -258_673.121_306_567_07];
        let actual = section.limit(trial_shear_n, 0.0, moments_nmm).unwrap();
        assert!((actual.utilization - 1.231_451_863_846_450_4).abs() < 1e-12);
        assert!((actual.normal_per_n[0] - 1.027_837_763_720_446e-6).abs() < 1e-16);
        assert!((actual.normal_per_n[1] + 1.027_837_763_720_446e-6).abs() < 1e-16);

        let reached_shear_n = [203_213.609_893_404_47, -203_213.609_893_404_47];
        let reached = section.limit(reached_shear_n, 0.0, moments_nmm).unwrap();
        assert!((reached.utilization - 1.0).abs() < 1e-12);
        let axes = section
            .projection_axes(trial_shear_n, 0.0, moments_nmm)
            .unwrap();
        let projected = ellipse_limit(reached_shear_n, axes.map(|qu| (qu, 1.0)));
        assert!((projected.utilization - 1.0).abs() < 1e-12);
        assert!((axes[0] - 287_387.443_170_048).abs() < 1e-7);
    }
}
