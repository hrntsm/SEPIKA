use super::{
    resolve, Comp, FrameStaticBendingSection, FrameStaticStrongBending, FrameStrongBendingPeak,
    FrameStrongBendingSpan, LoadedFrameSection, StrongBendingResponse,
};
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use sepika_core::model::MemberLoadKind;
use std::cell::OnceCell;
use std::cmp::Ordering;

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
    candidates: Vec<ExactBendingCandidate>,
    flat_ranges: Vec<([f64; 2], BigRational)>,
    shear_extrema_n: [BigRational; 2],
}

struct ExactBendingCandidate {
    x: BigRational,
    mz: BigRational,
}

impl ExactBending {
    fn monotonic(&self) -> bool {
        let [min, max] = &self.shear_extrema_n;
        !min.is_negative() || !max.is_positive()
    }

    fn absolute_maximum(&self) -> BigRational {
        self.candidates
            .iter()
            .map(|cut| cut.mz.abs())
            .max()
            .unwrap_or_else(BigRational::zero)
    }

    fn interior_maximum(&self) -> bool {
        let maximum = self.absolute_maximum();
        !maximum.is_zero()
            && self
                .candidates
                .iter()
                .any(|cut| !cut.x.is_zero() && cut.x != self.length_mm && cut.mz.abs() == maximum)
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
                    profile.add_distribution(a, b, w1, w2, &rational(lo)?)?;
                }
                _ => {}
            }
        }
        Ok(profile)
    }

    fn add_distribution(
        &mut self,
        a: f64,
        b: f64,
        w1: f64,
        w2: f64,
        x: &BigRational,
    ) -> Result<(), ExactSpanError> {
        let w1 = rational(w1)?;
        let w2 = rational(w2)?;
        if w1 != w2 {
            let a_mm = rational(a)?;
            let gradient = (w2 - &w1) / (rational(b)? - &a_mm);
            self.intensity_n_per_mm += &gradient * (x - a_mm);
            self.intensity_gradient += gradient;
        }
        self.intensity_n_per_mm += w1;
        Ok(())
    }

    fn has_distributed_load(&self) -> bool {
        !self.intensity_n_per_mm.is_zero() || !self.intensity_gradient.is_zero()
    }
}

fn has_distributed_load_at_cut(
    loads: &[Comp],
    x: &BigRational,
    right: bool,
) -> Result<bool, ExactSpanError> {
    let mut profile = ExactLoadProfile::default();
    for &load in loads {
        if let Comp::Dist { a, b, w1, w2 } = load {
            let a_mm = rational(a)?;
            let b_mm = rational(b)?;
            if if right {
                &a_mm <= x && x < &b_mm
            } else {
                &a_mm < x && x <= &b_mm
            } {
                profile.add_distribution(a, b, w1, w2, x)?;
            }
        }
    }
    Ok(profile.has_distributed_load())
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

    fn strong_load_profile(&self, lo: f64, hi: f64) -> Result<ExactLoadProfile, ExactSpanError> {
        let profile = ExactLoadProfile::new(&self.strong_loads, lo, hi)?;
        if !profile.intensity_gradient.is_zero() {
            return Err(ExactSpanError::VaryingStrongLoad);
        }
        Ok(profile)
    }

    fn exact_bending(&self) -> Result<ExactBending, ExactSpanError> {
        self.exact_bending_from(
            rational(self.response.node_force_local[1])?,
            rational(-self.response.node_force_local[5])?,
        )
    }

    fn exact_bending_from(
        &self,
        mut shear_n: BigRational,
        mut moment_nmm: BigRational,
    ) -> Result<ExactBending, ExactSpanError> {
        let length_mm = rational(self.length_mm)?;
        let i_moment_nmm = moment_nmm.clone();
        let mut candidates = vec![ExactBendingCandidate {
            x: BigRational::zero(),
            mz: moment_nmm.clone(),
        }];
        let mut flat_ranges = Vec::new();
        let mut shear_extrema_n = None;
        let two = integer(2);
        for (lo, hi) in self.load_intervals() {
            let lo_mm = rational(lo)?;
            let hi_mm = rational(hi)?;
            let width_mm = &hi_mm - &lo_mm;
            let ExactLoadProfile {
                point_n,
                intensity_n_per_mm,
                ..
            } = self.strong_load_profile(lo, hi)?;
            shear_n += point_n;
            if intensity_n_per_mm.is_zero() {
                if shear_n.is_zero() {
                    flat_ranges.push(([lo, hi], moment_nmm.abs()));
                    candidates.push(ExactBendingCandidate {
                        x: (&lo_mm + &hi_mm) / &two,
                        mz: moment_nmm.clone(),
                    });
                }
            } else {
                let extremum_mm = -&shear_n / &intensity_n_per_mm;
                if extremum_mm.is_positive() && extremum_mm < width_mm {
                    let extremum_nmm = &moment_nmm + &shear_n * &extremum_mm / &two;
                    candidates.push(ExactBendingCandidate {
                        x: lo_mm + extremum_mm,
                        mz: extremum_nmm,
                    });
                }
            }
            let shear_increment_n = intensity_n_per_mm * &width_mm;
            let end_shear_n = &shear_n + &shear_increment_n;
            let [min_shear_n, max_shear_n] =
                shear_extrema_n.get_or_insert_with(|| [shear_n.clone(), shear_n.clone()]);
            for qy in [&shear_n, &end_shear_n] {
                if qy < min_shear_n {
                    *min_shear_n = qy.clone();
                }
                if qy > max_shear_n {
                    *max_shear_n = qy.clone();
                }
            }
            moment_nmm += (&shear_n + &shear_increment_n / &two) * width_mm;
            candidates.push(ExactBendingCandidate {
                x: hi_mm,
                mz: moment_nmm.clone(),
            });
            shear_n = end_shear_n;
        }

        Ok(ExactBending {
            length_mm,
            end_moments_nmm: [i_moment_nmm, moment_nmm],
            candidates,
            flat_ranges,
            shear_extrema_n: shear_extrema_n.ok_or(ExactSpanError::Unconfirmed)?,
        })
    }

    pub(super) fn exact_full_span(&self) -> Result<FrameStrongBendingSpan, ExactSpanError> {
        let bending = self.exact_bending()?;
        let maximum_abs_nmm = bending.absolute_maximum();
        let end_moments_nmm = bending.end_moments()?;
        let monotonic = bending.monotonic();
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
            .filter(|cut| cut.mz.abs() == maximum_abs_nmm)
            .map(|cut| {
                self.response
                    .exact_peak(cut.x, Some(&cut.mz), &bending.length_mm, false)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FrameStrongBendingSpan {
            end_moments_nmm,
            monotonic,
            interior_maximum: demand_peaks
                .iter()
                .any(|peak| !peak.is_i_face && !peak.is_j_face),
            unverified_maximum_plateau,
            unverified_distribution: None,
            demand_peaks,
        })
    }

    fn saved_cuts_confirm_monotonicity(
        &self,
        bending: &ExactBending,
    ) -> Result<bool, ExactSpanError> {
        if !bending.monotonic() {
            return Ok(false);
        }
        let [i, j] = &bending.end_moments_nmm;
        let direction = j.cmp(i);
        if self.stations_mm.windows(2).any(|pair| {
            pair[1].1[5]
                .partial_cmp(&pair[0].1[5])
                .is_none_or(|order| order != Ordering::Equal && order != direction)
        }) {
            return Ok(false);
        }
        let [min, max] = &bending.shear_extrema_n;
        let mut min_offset_n = BigRational::zero();
        let mut max_offset_n = BigRational::zero();
        for &(x_mm, force) in self.stations_mm.iter().skip(1) {
            let qy_n = self.response.exact_static_shear(&rational(x_mm)?)?;
            let offset_n = rational(force[1])? - qy_n;
            if offset_n < min_offset_n {
                min_offset_n = offset_n;
            } else if offset_n > max_offset_n {
                max_offset_n = offset_n;
            }
        }
        // 保存測点だけを見ると、同じ荷重を積算した未保存の切断面で符号が反転し得る。
        Ok(match direction {
            Ordering::Greater => !(min + min_offset_n).is_negative(),
            Ordering::Less => !(max + max_offset_n).is_positive(),
            Ordering::Equal => (min + min_offset_n).is_zero() && (max + max_offset_n).is_zero(),
        })
    }

    fn saved_cuts_agree_on_classification(
        &self,
        bending: &ExactBending,
        interior: bool,
    ) -> Result<bool, ExactSpanError> {
        let [i, j] = &bending.end_moments_nmm;
        let direction = j.cmp(i);
        if bending.shear_extrema_n.iter().all(Zero::is_zero)
            && self
                .stations_mm
                .iter()
                .all(|(_, f)| rational(f[5]).is_ok_and(|mz| &mz == i))
        {
            // 純曲げの端部にだけ微小なせん断差があっても、同値の保存曲げが否定する勾配は根拠にしない。
            return Ok(true);
        }
        let origin_qy = rational(self.response.node_force_local[1])?;
        for &(x_mm, force) in self.stations_mm.iter().skip(1) {
            let x = rational(x_mm)?;
            let qy = self.response.exact_static_shear(&x)?;
            let mz = self.response.exact_static_moment(&x)?;
            let dq = rational(force[1])? - qy;
            let dm = rational(force[5])? - mz;
            if dq.is_zero() && dm.is_zero() {
                continue;
            }
            let dm = dm - &x * &dq;
            let alternative = self.exact_bending_from(&origin_qy + dq, i + dm)?;
            if alternative.interior_maximum() != interior
                || !interior
                    && (!alternative.monotonic()
                        || alternative.end_moments_nmm[1].cmp(&alternative.end_moments_nmm[0])
                            != direction)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn exact_static_sections(
        &self,
        partial: bool,
    ) -> Result<FrameStaticStrongBending, ExactSpanError> {
        let length = rational(self.length_mm)?;
        let moment_recoverable = self.response.node_force_local[5].is_finite();
        let (end_moments_nmm, monotonic, interior_maximum, has_strong_bending, mut candidates) =
            if partial && !moment_recoverable {
                let mut boundaries = vec![(BigRational::zero(), None)];
                for (lo, hi) in self.load_intervals() {
                    self.strong_load_profile(lo, hi)?;
                    boundaries.push((rational(hi)?, None));
                }
                ([f64::NAN; 2], false, false, false, boundaries)
            } else {
                let bending = self.exact_bending()?;
                let end_moments_nmm = bending.end_moments()?;
                let interior = bending.interior_maximum();
                let strong_cuts_confirmed = self
                    .stations_mm
                    .iter()
                    .all(|(_, force)| force[1].is_finite() && force[5].is_finite());
                let monotonic =
                    strong_cuts_confirmed && self.saved_cuts_confirm_monotonicity(&bending)?;
                let agrees = strong_cuts_confirmed
                    && (interior || monotonic)
                    && self.saved_cuts_agree_on_classification(&bending, interior)?;
                (
                    end_moments_nmm,
                    monotonic && agrees,
                    interior && agrees,
                    !bending.absolute_maximum().is_zero(),
                    bending
                        .candidates
                        .into_iter()
                        .map(|candidate| (candidate.x, Some(candidate.mz)))
                        .collect(),
                )
            };
        let weak_loads: Vec<_> = self
            .response
            .map
            .loads
            .iter()
            .filter_map(|load| resolve(load, self.response.frame())[2])
            .collect();
        // 測点の有無で適用条件の照合を省かない。既存の曲げ極値とは厳密な位置が同じ場合だけ併合する。
        let mut saved_cuts = Vec::new();
        for &(x_mm, _) in self.stations_mm {
            let x = rational(x_mm)?;
            if candidates
                .binary_search_by(|candidate| candidate.0.cmp(&x))
                .is_err()
            {
                let mz = moment_recoverable
                    .then(|| self.response.exact_static_moment(&x))
                    .transpose()?;
                saved_cuts.push((x, mz));
            }
        }
        candidates.extend(saved_cuts);
        candidates.sort_by(|a, b| a.0.cmp(&b.0));
        let mut sections: Vec<FrameStaticBendingSection> = Vec::with_capacity(candidates.len());
        let mut compression_at_zero_crossing_n = None::<f64>;
        for (x, mz) in candidates {
            let x_mm = finite(&x)?;
            let saved_left = if rational(x_mm)? == x {
                self.stations_mm
                    .binary_search_by(|(position, _)| position.total_cmp(&x_mm))
                    .ok()
                    .map(|index| self.stations_mm[index].1)
            } else {
                None
            };
            let zero_limit_side = [
                !x.is_zero()
                    && has_distributed_load_at_cut(&self.strong_loads, &x, false)?
                    && !has_distributed_load_at_cut(&weak_loads, &x, false)?,
                x != length
                    && has_distributed_load_at_cut(&self.strong_loads, &x, true)?
                    && !has_distributed_load_at_cut(&weak_loads, &x, true)?,
            ];
            let section = self.response.exact_peak(x, mz.as_ref(), &length, partial)?;
            let saved_right = saved_left
                .filter(|_| section.right.as_ref().is_ok_and(Option::is_some))
                .map(|left| self.response.saved_right_cut(x_mm, left, partial))
                .transpose();
            let zero_shear = |recovered: [f64; 6], saved: Option<[f64; 6]>| {
                recovered[1] == 0.0 || saved.is_some_and(|force| force[1] == 0.0)
            };
            let [left, right] = zero_limit_side;
            let zero_shear_limits = if let Ok(Some(right_force)) = &section.right {
                [
                    left && zero_shear(section.left, saved_left),
                    right && zero_shear(*right_force, saved_right.as_ref().ok().copied().flatten()),
                ]
            } else {
                [
                    (left || right) && zero_shear(section.left, saved_left),
                    false,
                ]
            };
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
                saved_right,
                zero_shear_limits,
            });
        }
        let issue = if partial {
            None
        } else {
            self.static_combination_issue(&sections, &weak_loads)?
        };
        Ok(FrameStaticStrongBending {
            end_moments_nmm,
            monotonic,
            interior_maximum,
            has_strong_bending,
            compression_at_zero_crossing_n,
            sections,
            combination_issue: issue,
        })
    }

    fn static_combination_issue(
        &self,
        sections: &[FrameStaticBendingSection],
        weak_loads: &[Comp],
    ) -> Result<Option<&'static str>, ExactSpanError> {
        let response = self.response;
        let start = response.exact_cut(&BigRational::zero(), true)?;
        let mut weak_shear = !start[2].is_zero();
        let mut axial_varying = false;
        let mut axial_loads = Vec::new();
        for load in &response.map.loads {
            let [axial, _, _] = resolve(load, response.frame());
            axial_loads.extend(axial);
        }
        for (lo, hi) in self.load_intervals() {
            axial_varying |= ExactLoadProfile::new(&axial_loads, lo, hi)?.has_distributed_load();
            let weak = ExactLoadProfile::new(weak_loads, lo, hi)?;
            weak_shear |= weak.has_distributed_load() || lo > 0.0 && !weak.point_n.is_zero();
        }
        let issue = if axial_varying {
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
        };
        Ok(issue)
    }
}

impl LoadedFrameSection {
    fn exact_static_shear(&self, x: &BigRational) -> Result<BigRational, ExactSpanError> {
        let [_, Some(qy), ..] = self.partial_cut(x, false)? else {
            return Err(ExactSpanError::Unconfirmed);
        };
        Ok(qy)
    }

    fn saved_right_cut(
        &self,
        x_mm: f64,
        saved_left: [f64; 6],
        partial: bool,
    ) -> Result<[f64; 6], String> {
        let recover = || {
            let [n, qy, qz, mx, my, mz] = saved_left;
            let mut right = [n, qy, qz, mx, my].map(BigRational::from_float);
            for load in &self.map.loads {
                if matches!(load.kind, MemberLoadKind::Point { a, .. } if a == x_mm) {
                    for (axis, comp) in resolve(load, self.frame()).into_iter().enumerate() {
                        if let Some(Comp::Point { p, .. }) = comp {
                            if let Some(component) = &mut right[axis] {
                                *component += rational(if axis == 0 { -p } else { p })?;
                            }
                        }
                    }
                }
            }
            partial_forces_to_f64(right, BigRational::from_float(mz).as_ref(), partial)
        };
        recover()
            .map_err(|_| "保存断面力から集中荷重の右側の断面力を有限な値で確認できません".into())
    }

    pub(super) fn exact_static_cut_mm(&self, x_mm: f64) -> Result<[f64; 6], String> {
        let recover = || {
            let x = rational(x_mm)?;
            exact_forces_to_f64(self.exact_cut(&x, false)?, &self.exact_static_moment(&x)?)
        };
        recover().map_err(|_| "照合位置の断面力を有限な値で回復できません".into())
    }

    fn exact_static_moment(&self, x: &BigRational) -> Result<BigRational, ExactSpanError> {
        let mut mz = rational(-self.node_force_local[5])? + rational(self.node_force_local[1])? * x;
        for comp in self
            .map
            .loads
            .iter()
            .filter_map(|load| resolve(load, self.frame())[1])
        {
            mz += exact_load_about_cut(comp, x, false)?.1;
        }
        Ok(mz)
    }

    fn partial_cut(
        &self,
        x: &BigRational,
        right: bool,
    ) -> Result<[Option<BigRational>; 5], ExactSpanError> {
        let g = &self.node_force_local;
        let [mut n, mut qy, mut qz, mx, my] =
            [-g[0], g[1], g[2], -g[3], -g[4]].map(BigRational::from_float);
        let mut my = my.zip(qz.as_ref()).map(|(my, qz)| my - qz * x);
        for load in &self.map.loads {
            for (axis, comp) in resolve(load, self.map.frame()).into_iter().enumerate() {
                let Some(comp) = comp else { continue };
                let (force, moment) = exact_load_about_cut(comp, x, right)?;
                match axis {
                    0 => {
                        if let Some(n) = &mut n {
                            *n -= force;
                        }
                    }
                    1 => {
                        if let Some(qy) = &mut qy {
                            *qy += force;
                        }
                    }
                    2 => {
                        if let Some(qz) = &mut qz {
                            *qz += force;
                        }
                        if let Some(my) = &mut my {
                            *my -= moment;
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        Ok([n, qy, qz, mx, my])
    }

    fn exact_cut(&self, x: &BigRational, right: bool) -> Result<[BigRational; 5], ExactSpanError> {
        let [Some(n), Some(qy), Some(qz), Some(mx), Some(my)] = self.partial_cut(x, right)? else {
            return Err(ExactSpanError::Unconfirmed);
        };
        Ok([n, qy, qz, mx, my])
    }

    pub(super) fn verify_partial_bending_stations(
        &self,
        stations: &[(f64, [f64; 6])],
    ) -> Result<(), String> {
        let verify = || {
            let cuts = stations
                .iter()
                .map(|&(x, force)| {
                    let x = rational(x)?;
                    let [n, qy, qz, mx, my] = self.exact_cut(&x, false)?;
                    let mz = self.exact_static_moment(&x)?;
                    Ok((x, force, [n, qy, qz, mx, my, mz]))
                })
                .collect::<Result<Vec<_>, ExactSpanError>>()?;
            let observed = |index: usize| {
                cuts.iter().filter_map(move |(x, force, loads)| {
                    BigRational::from_float(force[index])
                        .map(|f| (x, &f - &loads[index], force[index], &loads[index]))
                })
            };
            let origin_shear = |index| observed(index).next().map(|(_, qy, _, _)| qy);
            for index in 0..6 {
                let mut entries = observed(index);
                let Some((x0, f0, _, _)) = entries.next() else {
                    continue;
                };
                let slope = match index {
                    4 => origin_shear(2).map(|qz| -qz),
                    5 => origin_shear(1),
                    _ => Some(BigRational::zero()),
                };
                let slope = match (slope, entries.clone().next_back()) {
                    (Some(slope), _) => slope,
                    (None, Some((x, f, _, _))) if x != x0 => (f - &f0) / (x - x0),
                    (None, None) => continue,
                    _ => return Err(ExactSpanError::Unconfirmed),
                };
                let scale = observed(index)
                    .map(|(_, _, saved, _)| saved.abs())
                    .fold(1.0, f64::max);
                for (x, _, saved, load) in entries {
                    let predicted = &slope * (x - x0) + &f0 + load;
                    let tolerance = rational(1e-8 * scale.max(finite(&predicted)?.abs()))?;
                    if (predicted - rational(saved)?).abs() > tolerance {
                        return Err(ExactSpanError::Unconfirmed);
                    }
                }
            }
            Ok(())
        };
        verify()
            .map_err(|_| "厳密な部分回復値。解析断面力と部材荷重の釣合いを確認できません".into())
    }

    fn exact_peak(
        &self,
        x: BigRational,
        mz: Option<&BigRational>,
        length_mm: &BigRational,
        partial: bool,
    ) -> Result<FrameStrongBendingPeak, ExactSpanError> {
        let is_i_face = x.is_zero();
        let is_j_face = &x == length_mm;
        let x_mm = finite(&x)?;
        let left = self.partial_cut(&x, false)?;
        let convert = |cut| partial_forces_to_f64(cut, mz, partial);
        // 表示距離が荷重境界に丸まっても、実際の切断位置と左右を変えてはならない。
        let at_point_load = !is_j_face
            && self.map.loads.iter().any(|load| {
                matches!(load.kind, MemberLoadKind::Point { a, .. }
                    if a == x_mm && rational(a).is_ok_and(|a| a == x))
            });
        let right = at_point_load
            .then(|| self.partial_cut(&x, true))
            .transpose()?
            .filter(|right| right != &left)
            .map(convert)
            .transpose()?;
        Ok(FrameStrongBendingPeak {
            x_mm,
            xi: finite(&(x / length_mm))?,
            left: convert(left)?,
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
    partial_forces_to_f64(force.map(Some), Some(mz), false)
}

fn partial_forces_to_f64(
    force: [Option<BigRational>; 5],
    mz: Option<&BigRational>,
    partial: bool,
) -> Result<[f64; 6], ExactSpanError> {
    let mut result = [0.0; 6];
    for (out, value) in result
        .iter_mut()
        .zip(force.iter().map(Option::as_ref).chain([mz]))
    {
        if let Some(value) = value {
            *out = finite(value)?;
            if *out == 0.0 && !value.is_zero() {
                return Err(ExactSpanError::Unconfirmed);
            }
        } else if partial {
            *out = f64::NAN;
        } else {
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
