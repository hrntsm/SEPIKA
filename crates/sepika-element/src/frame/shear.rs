use super::{beam::BeamElement, member_load::ShearSpan, shear_capacity::ShearCapacity};
use crate::{
    behavior::{
        downcast_snapshot, validate_trial_increment, CheckpointError, Ctx, ElementBehavior,
        ElementUpdateError, LoadedFrameResponse, LocalMat, LocalVec, MemberShearEnd,
        MemberShearStatus,
    },
    transform::LocalFrame,
};
use sepika_core::model::{ElementData, MemberLoad, Model};
use sepika_math::dense_lu::CheckedDenseLu;
use smallvec::SmallVec;
use std::any::Any;

const SLIP_COMPONENTS: [usize; 4] = [1, 2, 7, 8];
const POST_YIELD_RATIO: f64 = 0.01;

#[cfg(test)]
mod tests;

pub(crate) fn connect(
    inner: Box<dyn ElementBehavior>,
    data: &ElementData,
    model: &Model,
) -> Box<dyn ElementBehavior> {
    let capacity = match ShearCapacity::from_model(model, data) {
        Ok(Some(capacity)) => capacity,
        Ok(None) => return inner,
        Err(reason) => {
            let is_rc = sepika_core::structure_kind::member_structure_kind(model, data)
                == sepika_core::structure_kind::StructureKind::Rc;
            return Box::new(ShearMember {
                inner,
                mechanics: Err(reason),
                committed: State::new(is_rc),
                trial: State::new(is_rc),
            });
        }
    };
    let beam = BeamElement::new(data, model);
    let (li, lj) = beam.rigid_lengths();
    let length_mm = beam.length - li - lj;
    let hardening = [beam.g * beam.as_y, beam.g * beam.as_z]
        .map(|ga| 2.0 * ga / length_mm * POST_YIELD_RATIO / (1.0 - POST_YIELD_RATIO));
    let is_rc = capacity.is_rc();
    if !hardening.iter().all(|h| h.is_finite() && *h > 0.0) {
        return Box::new(ShearMember {
            inner,
            mechanics: Err("線材せん断ばねの初期剛性を確定できません".into()),
            committed: State::new(is_rc),
            trial: State::new(is_rc),
        });
    }
    let state = State::new(is_rc);
    Box::new(ShearMember {
        inner,
        mechanics: Ok(Mechanics {
            capacity,
            axis: beam.axis,
            length_mm: beam.length,
            rigid_mm: [li, lj],
            hardening,
        }),
        committed: state.clone(),
        trial: state,
    })
}

struct Mechanics {
    capacity: ShearCapacity,
    axis: LocalFrame,
    length_mm: f64,
    rigid_mm: [f64; 2],
    hardening: [f64; 2],
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct State {
    displacement: [f64; 12],
    slip_mm: [f64; 4],
    loads: Vec<MemberLoad>,
    tangent: Option<Vec<f64>>,
    status: MemberShearStatus,
}

impl State {
    fn new(is_rc: bool) -> Self {
        Self {
            displacement: [0.0; 12],
            slip_mm: [0.0; 4],
            loads: Vec::new(),
            tangent: None,
            status: MemberShearStatus {
                is_rc,
                ends: [None, None],
                end_issues: [None, None],
                span_issue: None,
            },
        }
    }
}

struct ShearMember {
    inner: Box<dyn ElementBehavior>,
    mechanics: Result<Mechanics, String>,
    committed: State,
    trial: State,
}

#[cfg(test)]
pub(crate) fn axial_bending_snapshot(state: &dyn Any) -> &dyn Any {
    state
        .downcast_ref::<(State, State, Box<dyn Any>)>()
        .map_or(state, |(_, _, inner)| inner.as_ref())
}

#[derive(Clone, Copy)]
struct Face([f64; 6], usize);

impl Face {
    fn with_point_loads(mut self, forces: [f64; 3]) -> Self {
        for (i, force) in forces.into_iter().enumerate() {
            self.0[i] += force;
        }
        self
    }

    fn shear_sign(self) -> f64 {
        if self.1 == 0 {
            1.0
        } else {
            -1.0
        }
    }

    fn shear_n(self) -> [f64; 2] {
        [self.0[1], self.0[2]].map(|q| self.shear_sign() * q)
    }

    fn axial_compression_n(self) -> f64 {
        self.shear_sign() * self.0[0]
    }

    fn moments_nmm(self) -> [f64; 2] {
        [self.0[5], self.0[4]].map(|m| -self.shear_sign() * m)
    }

    fn has_shear(self) -> bool {
        let shear = self.shear_n();
        has_shear(shear, self.0[0].abs())
    }
}

fn has_shear(shear: [f64; 2], axial_scale: f64) -> bool {
    shear[0].hypot(shear[1]) > 64.0 * f64::EPSILON * axial_scale.max(1.0)
}

impl Mechanics {
    fn assessed_face(&self, face: Face) -> (Face, Option<String>) {
        if !self.capacity.is_web() {
            return (face, None);
        }
        let issue = self
            .capacity
            .validate_response(
                face.shear_n(),
                face.axial_compression_n(),
                face.moments_nmm(),
            )
            .err()
            .map(|reason| format!("{reason}（Qz={:.3} N）", face.shear_n()[1]));
        let mut web = face;
        web.0[2] = 0.0;
        (web, issue)
    }

    fn end_status(&self, face: Face) -> Result<MemberShearEnd, String> {
        let shear_n = face.shear_n();
        let axial_n = face.axial_compression_n();
        let moments = face.moments_nmm();
        self.capacity
            .validate_response(shear_n, axial_n, moments)
            .map_err(|reason| {
                format!(
                    "{reason}（Qy={:.3} N、Qz={:.3} N、圧縮正の軸力={axial_n:.3} N）",
                    shear_n[0], shear_n[1],
                )
            })?;
        let limit = self.capacity.limit(shear_n, axial_n, moments)?;
        let force_n = shear_n[0].hypot(shear_n[1]);
        let limit_n = if limit.utilization > 0.0 {
            force_n / limit.utilization
        } else {
            0.0
        };
        let governing_axis = match limit.normal_per_n.map(|g| g != 0.0) {
            [true, false] => Some(0),
            [false, true] => Some(1),
            _ => None,
        };
        if !limit_n.is_finite() || !limit.utilization.is_finite() {
            return Err("線材せん断ばねの到達比を確定できません".into());
        }
        Ok(MemberShearEnd {
            shear_n,
            limit_n,
            utilization: limit.utilization,
            reached: limit.utilization >= 1.0,
            exactly_at_limit: limit.utilization == 1.0,
            governing_axis,
        })
    }

    fn projected_slip(&self, face: Face, old_slip: [f64; 2]) -> Result<[f64; 2], String> {
        let sign = face.shear_sign();
        let shear_n = face.shear_n();
        let axial_n = face.axial_compression_n();
        let moment = face.moments_nmm();
        if old_slip == [0.0; 2]
            && self.capacity.limit(shear_n, axial_n, moment)?.utilization <= 1.0 + 1e-12
        {
            return Ok(old_slip);
        }
        let scale = self.hardening.map(f64::sqrt);
        let target = std::array::from_fn(|i| {
            (shear_n[i] - sign * self.hardening[i] * old_slip[i]) / scale[i]
        });
        let projection_direction = if shear_n == [0.0; 2] {
            std::array::from_fn(|i| target[i] * scale[i])
        } else {
            shear_n
        };
        let surface = self
            .capacity
            .prepare_projection(projection_direction, axial_n, moment)?;
        let mut point = target;
        let mut planes = SmallVec::<[[f64; 3]; 8]>::new();
        for _ in 0..48 {
            let q = std::array::from_fn(|i| point[i] * scale[i]);
            let limit = surface.limit(q)?;
            if limit.utilization <= 1.0 + 1e-12 {
                return Ok(std::array::from_fn(|i| {
                    old_slip[i] + sign * (target[i] - point[i]) / scale[i]
                }));
            }
            let normal = std::array::from_fn::<_, 2, _>(|i| limit.normal_per_n[i] * scale[i]);
            let norm = normal[0].hypot(normal[1]);
            if !norm.is_finite() || norm == 0.0 {
                return Err("線材せん断ばねの降伏面の法線を確定できません".into());
            }
            planes.push([normal[0] / norm, normal[1] / norm, 1.0 / norm]);
            point = nearest_feasible_point(target, &planes)
                .ok_or("線材せん断ばねの降伏面に射影できません")?;
        }
        Err("線材せん断ばねの降伏面への射影が収束しません".into())
    }

    fn slip_increment(
        &self,
        face: Face,
        old_slip: [f64; 2],
    ) -> Result<([f64; 2], [[f64; 6]; 2]), String> {
        let target = self.projected_slip(face, old_slip)?;
        let mut derivative = [[0.0; 6]; 2];
        if target == old_slip {
            return Ok((target, derivative));
        }
        let force_scale_n = face.0[1].hypot(face.0[2]).max(1.0);
        for i in [0, 1, 2, 4, 5] {
            if (i == 0 && !self.capacity.uses_axial_force()) || (i >= 4 && !self.capacity.is_rc()) {
                continue;
            }
            let scale = if i >= 4 {
                force_scale_n * self.length_mm
            } else {
                force_scale_n
            };
            let delta = f64::EPSILON.cbrt() * face.0[i].abs().max(scale);
            let mut plus = face;
            let mut minus = face;
            plus.0[i] += delta;
            minus.0[i] -= delta;
            let p = self.projected_slip(plus, old_slip);
            let m = self.projected_slip(minus, old_slip);
            // 非対称配筋では曲げの符号が変わると引張側が変わる。
            let crosses_moment_sign = i >= 4 && face.0[i].abs() <= delta;
            let use_positive_branch = if crosses_moment_sign && face.0[i] == 0.0 {
                let distance =
                    |value: &[f64; 2]| (value[0] - target[0]).hypot(value[1] - target[1]);
                match (&p, &m) {
                    (Ok(p), Ok(m)) => distance(p) <= distance(m),
                    _ => p.is_ok(),
                }
            } else {
                face.0[i].is_sign_positive()
            };
            // 切替点を跨ぐ差分は弾性側を含むため、現在の塑性側の勾配を使う。
            let (upper, lower, interval) = match (p, m) {
                (Ok(p), Ok(_)) if crosses_moment_sign && use_positive_branch => (p, target, delta),
                (Ok(_), Ok(m)) if crosses_moment_sign => (target, m, delta),
                (Ok(p), Ok(m)) if m == old_slip && p != old_slip => (p, target, delta),
                (Ok(p), Ok(m)) if p == old_slip && m != old_slip => (target, m, delta),
                (Ok(p), Ok(m)) => (p, m, 2.0 * delta),
                (Ok(p), Err(_)) => (p, target, delta),
                (Err(_), Ok(m)) => (target, m, delta),
                (Err(p), Err(m)) => return Err(format!("線材せん断接線: {p} / {m}")),
            };
            for (j, row) in derivative.iter_mut().enumerate() {
                row[i] = (upper[j] - lower[j]) / interval;
            }
        }
        Ok((target, derivative))
    }

    fn evaluate(
        &self,
        inner: &mut dyn ElementBehavior,
        committed: &State,
        trial: &mut State,
        ctx: &Ctx,
    ) -> Result<(), String> {
        let external: [f64; 12] =
            std::array::from_fn(|i| trial.displacement[i] - committed.displacement[i]);
        let mut slip = trial.slip_mm;
        let mut previous_residual = f64::INFINITY;
        let mut previous_slip = slip;
        for _ in 0..64 {
            let mut local_slip = [0.0; 12];
            for (j, &i) in SLIP_COMPONENTS.iter().enumerate() {
                local_slip[i] = slip[j] - committed.slip_mm[j];
            }
            let slip_global = self.axis.rotate_to_global(&local_slip);
            let increment: [f64; 12] = std::array::from_fn(|i| external[i] - slip_global[i]);
            inner.revert_state();
            inner
                .update_state_with_member_loads(
                    &LocalVec {
                        data: SmallVec::from_slice(&increment),
                    },
                    &trial.loads,
                    false,
                    ctx,
                )
                .map_err(|error| error.to_string())?;
            let response = inner
                .state_frame_load_response()?
                .ok_or("線材せん断ばねの部材荷重応答がありません")?;
            let cuts = response.section.flex_face_point_loads();
            let mut residual = [0.0; 4];
            let mut compliance = [[0.0; 12]; 4];
            for (end, probe) in trial.status.ends.iter_mut().enumerate() {
                let face = Face(
                    std::array::from_fn(|i| response.flex_face_force_local[end * 6 + i]),
                    end,
                );
                let cut = &cuts[end];
                let face = face.with_point_loads(cut.as_ref().copied().unwrap_or([0.0; 3]));
                let (assessed, issue) = self.assessed_face(face);
                trial.status.end_issues[end] = issue;
                let status = cut
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|_| self.end_status(assessed));
                let old_slip = [committed.slip_mm[2 * end], committed.slip_mm[2 * end + 1]];
                let (target, derivative) = if status.is_ok() {
                    self.slip_increment(assessed, old_slip)?
                } else {
                    (old_slip, [[0.0; 6]; 2])
                };
                *probe = if status.is_err() && !face.has_shear() && cut.is_ok() {
                    None
                } else {
                    Some(status)
                };
                for (i, (target, derivative)) in target.into_iter().zip(derivative).enumerate() {
                    residual[end * 2 + i] = target - slip[end * 2 + i];
                    compliance[end * 2 + i][end * 6..end * 6 + 6].copy_from_slice(&derivative);
                }
            }
            let converged = residual
                .iter()
                .zip(slip)
                .all(|(r, x)| r.abs() <= 1e-10 * x.abs().max(1.0));
            let residual_norm = residual.into_iter().map(f64::abs).fold(0.0, f64::max);
            if !converged && residual_norm >= previous_residual {
                for (current, previous) in slip.iter_mut().zip(previous_slip) {
                    *current = current.midpoint(previous);
                }
                continue;
            }
            if converged {
                trial.status.span_issue = self.span_issue(&response, &trial.status);
            }
            if converged && compliance.iter().flatten().all(|&v| v == 0.0) {
                trial.slip_mm = slip;
                trial.tangent = None;
                return Ok(());
            }
            let stiffness = self.axis.to_local(&inner.tangent_stiffness(ctx));
            let [li, lj] = self.rigid_mm;
            let mut mapped = [[0.0; 12]; 4];
            for j in 0..12 {
                let nodal = std::array::from_fn(|i| stiffness.get(i, j));
                let flex = super::rigid_arm::to_flex_force(&nodal, li, lj);
                for (row, d) in mapped.iter_mut().zip(&compliance) {
                    row[j] = d.iter().zip(flex).map(|(d, f)| d * f).sum();
                }
            }
            let jacobian: [f64; 16] = std::array::from_fn(|ij| {
                let (i, j) = (ij / 4, ij % 4);
                mapped[i][SLIP_COMPONENTS[j]] + if i == j { 1.0 } else { 0.0 }
            });
            let lu = CheckedDenseLu::factorize(&jacobian, 4).map_err(|err| err.to_string())?;
            if converged {
                let rhs: Vec<_> = mapped.into_iter().flatten().collect();
                let gradient = lu.solve(&rhs, 12).map_err(|err| err.to_string())?;
                let mut condensed = stiffness;
                for i in 0..12 {
                    let row = SLIP_COMPONENTS.map(|j| condensed.get(i, j));
                    for j in 0..12 {
                        let correction: f64 = (0..4).map(|k| row[k] * gradient[k * 12 + j]).sum();
                        condensed.set(i, j, condensed.get(i, j) - correction);
                    }
                }
                trial.slip_mm = slip;
                trial.tangent = Some(self.axis.to_global(&condensed).data);
                return Ok(());
            }
            let increment = lu.solve(&residual, 1).map_err(|err| err.to_string())?;
            previous_residual = residual_norm;
            previous_slip = slip;
            for (value, delta) in slip.iter_mut().zip(increment) {
                *value += delta;
            }
        }
        Err("線材せん断ばねの局所釣合いが収束しません".into())
    }

    fn span_issue(
        &self,
        response: &LoadedFrameResponse,
        status: &MemberShearStatus,
    ) -> Option<String> {
        let ends = &status.ends;
        if ends.iter().flatten().any(Result::is_err) {
            return None;
        }
        let max_end = ends
            .iter()
            .filter_map(|end| end.as_ref()?.as_ref().ok())
            .map(|end| end.utilization)
            .fold(0.0, f64::max);
        if (self.capacity.is_rc() && max_end > 1.0)
            || (!self.capacity.is_rc() && response.section.is_unloaded())
        {
            return None;
        }
        let spans = match response.section.flex_shear_spans() {
            Ok(spans) => spans,
            Err(reason) => return Some(reason),
        };
        let end_weak_issue =
            self.capacity.is_web() && status.end_issues.iter().any(Option::is_some);
        spans
            .iter()
            .find_map(|span| self.span_is_bounded(span, max_end, end_weak_issue, 8).err())
    }

    fn span_is_bounded(
        &self,
        span: &ShearSpan,
        max_end: f64,
        end_weak_issue: bool,
        depth: u8,
    ) -> Result<(), String> {
        let (shear, [min_axial, max_axial], moments) = span.bounds();
        if !has_shear(shear, min_axial.abs().max(max_axial.abs())) {
            return Ok(());
        }
        let bounded = [shear, [shear[0], -shear[1]]]
            .into_iter()
            .try_for_each(|mut q| {
                if end_weak_issue {
                    q[1] = 0.0;
                }
                self.capacity
                    .validate_response(q, max_axial, moments.map(|[lo, _]| lo))?;
                let ratio = self.capacity.bound_utilization(q, min_axial, moments)?;
                if ratio > max_end.max(1.0) || (max_end < 1.0 && ratio == 1.0) {
                    return Err(format!(
                        "せん断利用率の上界 {ratio:.6} と端部の最大値 {max_end:.6} から先行到達を除外できません"
                    ));
                }
                Ok(())
            });
        match (bounded, depth) {
            (Ok(()), _) => Ok(()),
            (Err(reason), 0) => Err(format!(
                "可撓区間で材端より先にせん断耐力へ達しないことを確認できません。{reason}（Qy上界={:.3} N、Qz上界={:.3} N、圧縮軸力の下界={min_axial:.3} N・上界={max_axial:.3} N）",
                shear[0], shear[1],
            )),
            (Err(_), _) => span
                .bisect()
                .iter()
                .try_for_each(|child| self.span_is_bounded(child, max_end, end_weak_issue, depth - 1)),
        }
    }
}

fn nearest_feasible_point(target: [f64; 2], planes: &[[f64; 3]]) -> Option<[f64; 2]> {
    let mut best: Option<([f64; 2], f64)> = None;
    let mut consider = |point: [f64; 2]| {
        if planes.iter().any(|&[x, y, c]| {
            let value = x * point[0] + y * point[1];
            value > c + 128.0 * f64::EPSILON * c.abs().max(value.abs())
        }) {
            return;
        }
        let distance = (point[0] - target[0]).hypot(point[1] - target[1]);
        if distance.is_finite() && best.is_none_or(|(_, old)| distance < old) {
            best = Some((point, distance));
        }
    };
    for (i, &[x, y, c]) in planes.iter().enumerate() {
        let distance = target[0] * x + target[1] * y - c;
        consider([target[0] - distance * x, target[1] - distance * y]);
        for &[a, b, d] in &planes[..i] {
            let det = x * b - y * a;
            if det.abs() > 64.0 * f64::EPSILON {
                consider([(c * b - y * d) / det, (x * d - c * a) / det]);
            }
        }
    }
    best.map(|(point, _)| point)
}

crate::behavior::forward_element_behavior!(ShearMember, inner, {
    n_dof: forward,
    global_dofs: forward,
    tangent_stiffness: custom,
    internal_force: forward,
    update_state: custom,
    update_state_with_member_loads: custom,
    mass_matrix: forward,
    try_mass_matrix: forward,
    owns_frame_member_loads: forward,
    frame_load_response: forward,
    state_frame_load_response: forward,
    member_shear_status: custom,
    member_shear_uses_nonsymmetric_tangent: custom,
    recover_forces: forward,
    state_member_forces: forward,
    snapshot_state: custom,
    restore_state: custom,
    commit_state: custom,
    revert_state: custom,
    serialize_checkpoint: custom,
    deserialize_checkpoint: custom,
    panel_moments_from: forward,
    ductility_probe: forward,
    fiber_section_states: forward,
    end_spring_rotations: forward,
    set_time_step: forward,
}, custom {
    fn member_shear_status(&self) -> Option<&MemberShearStatus> {
        Some(&self.trial.status)
    }
    fn member_shear_uses_nonsymmetric_tangent(&self) -> bool {
        self.mechanics.is_ok()
    }
    fn tangent_stiffness(&self, ctx: &Ctx) -> LocalMat {
        match &self.trial.tangent {
            Some(data) => LocalMat { n: 12, data: data.clone() },
            None => self.inner.tangent_stiffness(ctx),
        }
    }
    fn update_state(
        &mut self, du: &LocalVec, commit: bool, ctx: &Ctx,
    ) -> Result<(), ElementUpdateError> {
        self.update_state_with_member_loads(du, &self.trial.loads.clone(), commit, ctx)
    }
    fn update_state_with_member_loads(
        &mut self, du: &LocalVec, loads: &[MemberLoad], commit: bool, ctx: &Ctx,
    ) -> Result<(), ElementUpdateError> {
        if validate_trial_increment(du, 12).is_err() {
            return self.inner.update_state_with_member_loads(du, loads, commit, ctx);
        }
        let original = self.inner.snapshot_state();
        let mut trial = self.trial.clone();
        trial.loads = loads.to_vec();
        for (u, delta) in trial.displacement.iter_mut().zip(&du.data) {
            *u += delta;
        }
        let update = match &self.mechanics {
            Ok(mechanics) => mechanics.evaluate(self.inner.as_mut(), &self.committed, &mut trial, ctx),
            Err(reason) => self.inner.update_state_with_member_loads(du, loads, false, ctx)
                .map_err(|err| err.to_string())
                .and_then(|()| {
                    let response = self.inner.state_frame_load_response()?.ok_or("線材の部材荷重応答がありません")?;
                    let cuts = response.section.flex_face_point_loads();
                    for (end, probe) in trial.status.ends.iter_mut().enumerate() {
                        let base = end * 6;
                        let face = Face(std::array::from_fn(|i| response.flex_face_force_local[base + i]), end)
                            .with_point_loads(cuts[end].as_ref().copied().unwrap_or([0.0; 3]));
                        *probe = if let Err(cut) = &cuts[end] {
                            Some(Err(cut.clone()))
                        } else {
                            face.has_shear().then(|| Err(reason.clone()))
                        };
                    }
                    trial.status.span_issue = if trial.status.ends.iter().all(Option::is_none) && !response.section.is_unloaded() {
                        match response.section.flex_shear_spans() {
                            Ok(spans) => spans.iter().any(|span| {
                                let (shear, axial, _) = span.bounds();
                                has_shear(shear, axial[0].abs().max(axial[1].abs()))
                            }).then(|| reason.clone()),
                            Err(error) => Some(error),
                        }
                    } else { None };
                    Ok(())
                }),
        };
        if let Err(reason) = update {
            self.inner.restore_state(original.as_ref());
            return Err(ElementUpdateError(reason));
        }
        self.trial = trial;
        if commit { self.commit_state(); }
        Ok(())
    }
    fn snapshot_state(&self) -> Box<dyn Any> {
        Box::new((self.committed.clone(), self.trial.clone(), self.inner.snapshot_state()))
    }
    fn restore_state(&mut self, state: &dyn Any) {
        let (committed, trial, inner) = downcast_snapshot::<(State, State, Box<dyn Any>)>("ShearMember", state);
        self.inner.restore_state(inner.as_ref());
        self.committed = committed.clone();
        self.trial = trial.clone();
    }
    fn commit_state(&mut self) {
        self.inner.commit_state();
        self.committed = self.trial.clone();
    }
    fn revert_state(&mut self) {
        self.inner.revert_state();
        self.trial = self.committed.clone();
    }
    fn serialize_checkpoint(&self) -> Vec<u8> {
        bincode::serialize(&(&self.committed, &self.trial, self.inner.serialize_checkpoint()))
            .expect("serialize member shear checkpoint")
    }
    fn deserialize_checkpoint(&mut self, data: &[u8]) -> Result<(), CheckpointError> {
        let (committed, trial, inner): (State, State, Vec<u8>) =
            bincode::deserialize(data).map_err(|err| CheckpointError::Decode(err.to_string()))?;
        if [&committed, &trial].into_iter().any(|s| {
            !s.displacement.iter().chain(&s.slip_mm).all(|v| v.is_finite()) ||
            s.tangent.as_ref().is_some_and(|k| k.len() != 144 || !k.iter().all(|v| v.is_finite()))
        }) {
            return Err(CheckpointError::Decode("線材せん断ばねの状態が不正です".into()));
        }
        self.inner.deserialize_checkpoint(&inner)?;
        self.committed = committed;
        self.trial = trial;
        Ok(())
    }
});
