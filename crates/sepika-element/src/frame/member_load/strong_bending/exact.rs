use super::{
    resolve, Comp, FrameStaticBendingSection, FrameStaticStrongBending, FrameStrongBendingPeak,
    FrameStrongBendingSpan, LoadedFrameSection, StrongBendingResponse,
};
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use sepika_core::model::MemberLoadKind;
use std::cell::OnceCell;

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

type ExactVector = [BigRational; 3];

fn exact_vector([x, y, z]: [f64; 3]) -> Result<ExactVector, ExactSpanError> {
    Ok([rational(x)?, rational(y)?, rational(z)?])
}

fn exact_dot(a: &ExactVector, b: &ExactVector) -> BigRational {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn input_axis_directions(
    [i, j]: [[f64; 3]; 2],
    reference: [f64; 3],
) -> Result<[ExactVector; 3], ExactSpanError> {
    let a = exact_vector(i)?;
    let b = exact_vector(j)?;
    let x: ExactVector = std::array::from_fn(|k| &b[k] - &a[k]);
    let r = exact_vector(reference)?;
    let xx = exact_dot(&x, &x);
    let xr = exact_dot(&x, &r);
    let y = std::array::from_fn(|k| &r[k] * &xx - &x[k] * &xr);
    let z = std::array::from_fn(|k| {
        let l = (k + 1) % 3;
        let m = (k + 2) % 3;
        &x[l] * &r[m] - &x[m] * &r[l]
    });
    Ok([x, y, z])
}

impl LoadedFrameSection {
    pub(super) fn verify_static_load_projections(
        &self,
        ends: [[f64; 3]; 2],
        reference: [f64; 3],
    ) -> Result<(), String> {
        let axis_directions = OnceCell::new();
        self.verify_load_projections_with(|dir, index| {
            let axes = axis_directions.get_or_init(|| input_axis_directions(ends, reference));
            axes.as_ref().is_ok_and(|axes| {
                !axes[index].iter().all(Zero::is_zero)
                    && exact_vector(dir).is_ok_and(|dir| exact_dot(&dir, &axes[index]).is_zero())
            })
        })
    }
}
struct ExactBending {
    length_mm: BigRational,
    end_moments_nmm: [BigRational; 2],
    candidates: Vec<(BigRational, BigRational)>,
    flat_ranges: Vec<([f64; 2], BigRational)>,
    monotonic: bool,
}

impl ExactBending {
    fn absolute_maximum(&self) -> BigRational {
        self.candidates
            .iter()
            .map(|(_, mz)| mz.abs())
            .max()
            .unwrap_or_else(BigRational::zero)
    }

    fn end_moments(&self) -> Result<[f64; 2], ExactSpanError> {
        Ok([
            finite(&self.end_moments_nmm[0])?,
            finite(&self.end_moments_nmm[1])?,
        ])
    }
}

#[derive(Default)]
struct ExactLoadProfile {
    point_n: BigRational,
    intensity_n_per_mm: BigRational,
    intensity_gradient: BigRational,
}

impl ExactLoadProfile {
    fn new(loads: &[Comp], lo: f64, hi: f64) -> Result<Self, ExactSpanError> {
        let mut profile = Self::default();
        for &load in loads {
            match load {
                Comp::Point { a, p } if a == lo => profile.point_n += rational(p)?,
                Comp::Dist { a, b, w1, w2 } if a <= lo && hi <= b => {
                    let w1 = rational(w1)?;
                    let w2 = rational(w2)?;
                    if w1 != w2 {
                        let a_mm = rational(a)?;
                        let gradient = (w2 - &w1) / (rational(b)? - &a_mm);
                        profile.intensity_n_per_mm += &gradient * (rational(lo)? - a_mm);
                        profile.intensity_gradient += gradient;
                    }
                    profile.intensity_n_per_mm += w1;
                }
                _ => {}
            }
        }
        Ok(profile)
    }

    fn has_distributed_load(&self) -> bool {
        !self.intensity_n_per_mm.is_zero() || !self.intensity_gradient.is_zero()
    }
}

impl StrongBendingResponse<'_> {
    fn load_intervals(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        let inner = self
            .breaks
            .iter()
            .copied()
            .filter(|&x| 0.0 < x && x < self.length_mm);
        std::iter::once(0.0)
            .chain(inner.clone())
            .zip(inner.chain([self.length_mm]))
    }

    fn exact_bending(&self) -> Result<ExactBending, ExactSpanError> {
        let length_mm = rational(self.length_mm)?;
        let mut shear_n = rational(self.response.node_force_local[1])?;
        let mut moment_nmm = rational(-self.response.node_force_local[5])?;
        let i_moment_nmm = moment_nmm.clone();
        let mut candidates = vec![(BigRational::zero(), moment_nmm.clone())];
        let mut flat_ranges = Vec::new();
        let mut has_positive_shear = false;
        let mut has_negative_shear = false;
        let two = integer(2);
        for (lo, hi) in self.load_intervals() {
            let lo_mm = rational(lo)?;
            let hi_mm = rational(hi)?;
            let width_mm = &hi_mm - &lo_mm;
            let ExactLoadProfile {
                point_n,
                intensity_n_per_mm,
                intensity_gradient,
            } = ExactLoadProfile::new(&self.strong_loads, lo, hi)?;
            shear_n += point_n;
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
        }

        Ok(ExactBending {
            length_mm,
            end_moments_nmm: [i_moment_nmm, moment_nmm],
            candidates,
            flat_ranges,
            monotonic: !(has_positive_shear && has_negative_shear),
        })
    }

    pub(super) fn exact_full_span(&self) -> Result<FrameStrongBendingSpan, ExactSpanError> {
        let bending = self.exact_bending()?;
        let maximum_abs_nmm = bending.absolute_maximum();
        let end_moments_nmm = bending.end_moments()?;
        let mut unverified_maximum_plateau = false;
        if !maximum_abs_nmm.is_zero() {
            for ([lo, hi], mz) in bending.flat_ranges {
                if mz == maximum_abs_nmm
                    && (self.response.has_axial_or_weak_distributed_load(lo, hi)
                        || !self.response.exact_cut(&rational(lo)?, true)?[2].is_zero())
                {
                    unverified_maximum_plateau = true;
                    break;
                }
            }
        }

        let demand_peaks = bending
            .candidates
            .into_iter()
            .filter(|(_, mz)| mz.abs() == maximum_abs_nmm)
            .map(|(x, mz)| self.response.exact_peak(x, mz, &bending.length_mm))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FrameStrongBendingSpan {
            end_moments_nmm,
            monotonic: bending.monotonic,
            interior_maximum: demand_peaks
                .iter()
                .any(|peak| !peak.is_i_face && !peak.is_j_face),
            unverified_maximum_plateau,
            unverified_distribution: None,
            demand_peaks,
        })
    }

    pub(super) fn exact_static_sections(&self) -> Result<FrameStaticStrongBending, ExactSpanError> {
        let bending = self.exact_bending()?;
        let maximum = bending.absolute_maximum();
        let end_moments_nmm = bending.end_moments()?;
        let [i, j] = &bending.end_moments_nmm;
        let monotonic = bending.monotonic
            && self.stations_mm.iter().skip(1).all(|(_, force)| {
                let qy_n = force[1];
                qy_n == 0.0 || qy_n > 0.0 && j > i || qy_n < 0.0 && j < i
            });
        let mut interior_maximum = false;
        let mut sections: Vec<FrameStaticBendingSection> =
            Vec::with_capacity(bending.candidates.len());
        let mut compression_at_zero_crossing_n = None::<f64>;
        for (x, mz) in bending.candidates {
            interior_maximum |= !x.is_zero() && x != bending.length_mm && mz.abs() == maximum;
            let x_mm = finite(&x)?;
            let saved_left = if rational(x_mm)? == x {
                self.stations_mm
                    .binary_search_by(|(position, _)| position.total_cmp(&x_mm))
                    .ok()
                    .map(|index| self.stations_mm[index].1)
            } else {
                None
            };
            let section = self.response.exact_peak(x, mz, &bending.length_mm)?;
            if let Some(previous) = sections.last().map(|section| &section.recovered) {
                let after_point = previous.right.as_ref().ok().and_then(Option::as_ref);
                let [n, _, _, _, my, _] = *after_point.unwrap_or(&previous.left);
                if n < 0.0
                    && my == 0.0
                    && previous.left[5] != 0.0
                    && section.left[5] != 0.0
                    && previous.left[5].is_sign_positive() != section.left[5].is_sign_positive()
                {
                    compression_at_zero_crossing_n =
                        Some(compression_at_zero_crossing_n.map_or(n, |prior| prior.min(n)));
                }
            }
            sections.push(FrameStaticBendingSection {
                recovered: section,
                saved_left,
            });
        }
        let issue = self.static_combination_issue(&sections)?;
        Ok(FrameStaticStrongBending {
            end_moments_nmm,
            monotonic,
            interior_maximum,
            has_strong_bending: !maximum.is_zero(),
            compression_at_zero_crossing_n,
            sections,
            combination_issue: issue,
        })
    }

    fn static_combination_issue(
        &self,
        sections: &[FrameStaticBendingSection],
    ) -> Result<Option<&'static str>, ExactSpanError> {
        let response = self.response;
        let start = response.exact_cut(&BigRational::zero(), true)?;
        let mut weak_shear = !start[2].is_zero();
        let mut axial_varying = false;
        let mut axial_loads = Vec::new();
        let mut weak_loads = Vec::new();
        for load in &response.map.loads {
            let [axial, _, weak] = resolve(load, response.frame());
            axial_loads.extend(axial);
            weak_loads.extend(weak);
        }
        for (lo, hi) in self.load_intervals() {
            axial_varying |= ExactLoadProfile::new(&axial_loads, lo, hi)?.has_distributed_load();
            let weak = ExactLoadProfile::new(&weak_loads, lo, hi)?;
            weak_shear |= weak.has_distributed_load() || lo > 0.0 && !weak.point_n.is_zero();
        }
        Ok(if axial_varying {
            Some("軸方向の分布荷重があるため、全区間の軸力・曲げ等の組合せの最大を確定できません")
        } else if weak_shear {
            Some("弱軸せん断があり弱軸曲げが一定とは限らないため、全区間の組合せの最大を確定できません")
        } else if !start[4].is_zero()
            && sections
                .iter()
                .map(|section| &section.recovered)
                .any(|section| {
                    let right = section.right.as_ref().ok().and_then(Option::as_ref);
                    (!section.is_i_face || right.is_none()) && section.left[0] > 0.0
                        || right.is_some_and(|forces| forces[0] > 0.0)
                })
        {
            Some("引張軸力と二軸曲げが同時に作用するため、全区間の組合せの最大を確定できません")
        } else {
            None
        })
    }
}

impl LoadedFrameSection {
    pub(super) fn exact_static_cut_mm(&self, x_mm: f64) -> Result<[f64; 6], String> {
        let recover = || {
            let x = rational(x_mm)?;
            let mut mz =
                rational(-self.node_force_local[5])? + rational(self.node_force_local[1])? * &x;
            for comp in self
                .map
                .loads
                .iter()
                .filter_map(|load| resolve(load, self.frame())[1])
            {
                mz += exact_load_about_cut(comp, &x, false)?.1;
            }
            exact_forces_to_f64(self.exact_cut(&x, false)?, &mz)
        };
        recover().map_err(|_| "照合位置の断面力を有限な値で回復できません".into())
    }

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
