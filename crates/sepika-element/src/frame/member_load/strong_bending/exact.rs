use super::{
    resolve, Comp, FrameStrongBendingPeak, FrameStrongBendingSpan, LoadedFrameSection,
    StrongBendingResponse,
};
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use sepika_core::model::MemberLoadKind;

pub(super) enum ExactSpanError {
    VaryingStrongLoad,
    Unconfirmed,
}

fn rational(value: f64) -> Result<BigRational, ExactSpanError> {
    BigRational::from_float(value).ok_or(ExactSpanError::Unconfirmed)
}

fn integer(value: i32) -> BigRational {
    BigRational::from_integer(value.into())
}

fn finite(value: &BigRational) -> Result<f64, ExactSpanError> {
    value
        .to_f64()
        .filter(|value| value.is_finite())
        .ok_or(ExactSpanError::Unconfirmed)
}

impl StrongBendingResponse<'_> {
    pub(super) fn exact_full_span(&self) -> Result<FrameStrongBendingSpan, ExactSpanError> {
        let length_mm = rational(self.length_mm)?;
        let mut lo = 0.0;
        let mut shear_n = rational(self.response.node_force_local[1])?;
        let mut moment_nmm = rational(-self.response.node_force_local[5])?;
        let i_moment_nmm = moment_nmm.clone();
        let mut candidates = vec![(BigRational::zero(), moment_nmm.clone())];
        let mut flat_ranges = Vec::new();
        let mut has_positive_shear = false;
        let mut has_negative_shear = false;
        let two = integer(2);
        let breaks = self
            .breaks
            .iter()
            .copied()
            .filter(|&x| 0.0 < x && x < self.length_mm);
        for hi in breaks.chain([self.length_mm]) {
            let lo_mm = rational(lo)?;
            let hi_mm = rational(hi)?;
            let width_mm = &hi_mm - &lo_mm;
            let mut intensity_n_per_mm = BigRational::zero();
            let mut intensity_gradient = BigRational::zero();
            for &load in &self.strong_loads {
                match load {
                    Comp::Point { a, p } if a == lo => shear_n += rational(p)?,
                    Comp::Dist { a, b, w1, w2 } if a <= lo && hi <= b => {
                        let w1 = rational(w1)?;
                        let w2 = rational(w2)?;
                        if w1 != w2 {
                            let a_mm = rational(a)?;
                            let gradient = (w2 - &w1) / (rational(b)? - &a_mm);
                            intensity_n_per_mm += &gradient * (&lo_mm - a_mm);
                            intensity_gradient += gradient;
                        }
                        intensity_n_per_mm += w1;
                    }
                    _ => {}
                }
            }
            if !intensity_gradient.is_zero() {
                return Err(ExactSpanError::VaryingStrongLoad);
            }
            if intensity_n_per_mm.is_zero() {
                if shear_n.is_zero() {
                    flat_ranges.push(([lo, hi], moment_nmm.abs()));
                    candidates.push(((&lo_mm + &hi_mm) / &two, moment_nmm.clone()));
                }
            } else {
                let extremum_mm = -&shear_n / &intensity_n_per_mm;
                if extremum_mm.is_positive() && extremum_mm < width_mm {
                    let extremum_nmm = &moment_nmm + &shear_n * &extremum_mm / &two;
                    candidates.push((lo_mm + extremum_mm, extremum_nmm));
                }
            }
            let shear_increment_n = intensity_n_per_mm * &width_mm;
            let end_shear_n = &shear_n + &shear_increment_n;
            has_positive_shear |= shear_n.is_positive() || end_shear_n.is_positive();
            has_negative_shear |= shear_n.is_negative() || end_shear_n.is_negative();
            moment_nmm += (&shear_n + &shear_increment_n / &two) * width_mm;
            candidates.push((hi_mm, moment_nmm.clone()));
            shear_n = end_shear_n;
            lo = hi;
        }

        let maximum_abs_nmm = candidates
            .iter()
            .map(|(_, mz)| mz.abs())
            .max()
            .unwrap_or_else(BigRational::zero);
        let mut unverified_maximum_plateau = false;
        if !maximum_abs_nmm.is_zero() {
            for ([lo, hi], mz) in flat_ranges {
                if mz == maximum_abs_nmm
                    && (self.response.has_axial_or_weak_distributed_load(lo, hi)
                        || !self.response.exact_cut(&rational(lo)?, true)?[2].is_zero())
                {
                    unverified_maximum_plateau = true;
                    break;
                }
            }
        }

        let demand_peaks = candidates
            .into_iter()
            .filter(|(_, mz)| mz.abs() == maximum_abs_nmm)
            .map(|(x, mz)| self.response.exact_peak(x, mz, &length_mm))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FrameStrongBendingSpan {
            end_moments_nmm: [finite(&i_moment_nmm)?, finite(&moment_nmm)?],
            monotonic: !(has_positive_shear && has_negative_shear),
            interior_maximum: demand_peaks
                .iter()
                .any(|peak| !peak.is_i_face && !peak.is_j_face),
            unverified_maximum_plateau,
            demand_peaks,
        })
    }
}

impl LoadedFrameSection {
    fn exact_cut(&self, x: &BigRational, right: bool) -> Result<[BigRational; 5], ExactSpanError> {
        let g = &self.node_force_local;
        let mut n = rational(-g[0])?;
        let mut qy = rational(g[1])?;
        let mut qz = rational(g[2])?;
        let mx = rational(-g[3])?;
        let mut my = rational(-g[4])? - &qz * x;
        for load in &self.map.loads {
            for (axis, comp) in resolve(load, self.map.frame()).into_iter().enumerate() {
                let Some(comp) = comp else { continue };
                let (force, moment) = exact_load_about_cut(comp, x, right)?;
                match axis {
                    0 => n -= force,
                    1 => qy += force,
                    2 => {
                        qz += force;
                        my -= moment;
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok([n, qy, qz, mx, my])
    }

    fn exact_peak(
        &self,
        x: BigRational,
        mz: BigRational,
        length_mm: &BigRational,
    ) -> Result<FrameStrongBendingPeak, ExactSpanError> {
        let is_i_face = x.is_zero();
        let is_j_face = &x == length_mm;
        let x_mm = finite(&x)?;
        let left = self.exact_cut(&x, false)?;
        // 表示距離が荷重境界に丸まっても、実際の切断位置と左右を変えてはならない。
        let at_point_load = !is_j_face
            && self.map.loads.iter().any(|load| {
                matches!(load.kind, MemberLoadKind::Point { a, .. }
                    if a == x_mm && rational(a).is_ok_and(|a| a == x))
            });
        let right = at_point_load
            .then(|| self.exact_cut(&x, true))
            .transpose()?
            .filter(|right| right != &left)
            .map(|right| exact_forces_to_f64(right, &mz))
            .transpose()?;
        Ok(FrameStrongBendingPeak {
            x_mm,
            xi: finite(&(x / length_mm))?,
            left: exact_forces_to_f64(left, &mz)?,
            right: Ok(right),
            is_i_face,
            is_j_face,
        })
    }
}

fn exact_forces_to_f64(
    force: [BigRational; 5],
    mz: &BigRational,
) -> Result<[f64; 6], ExactSpanError> {
    let mut result = [0.0; 6];
    for (out, value) in result.iter_mut().zip(force.iter().chain([mz])) {
        *out = finite(value)?;
        if *out == 0.0 && !value.is_zero() {
            return Err(ExactSpanError::Unconfirmed);
        }
    }
    Ok(result)
}

fn exact_load_about_cut(
    comp: Comp,
    x: &BigRational,
    right: bool,
) -> Result<(BigRational, BigRational), ExactSpanError> {
    match comp {
        Comp::Point { a, p } => {
            let a = rational(a)?;
            if &a < x || (right && &a == x) {
                let p = rational(p)?;
                return Ok((p.clone(), p * (x - a)));
            }
        }
        Comp::Dist { a, b, w1, w2 } => {
            let a = rational(a)?;
            let b = rational(b)?;
            let hi = x.min(&b);
            if &a < hi {
                let width = hi - &a;
                let w1 = rational(w1)?;
                let wh = &w1 + (rational(w2)? - &w1) * &width / (&b - &a);
                let force = &width * (&w1 + &wh) / integer(2);
                let moment =
                    (x - hi) * &force + &width * &width * (w1 / integer(3) + wh / integer(6));
                return Ok((force, moment));
            }
        }
    }
    Ok((BigRational::zero(), BigRational::zero()))
}
