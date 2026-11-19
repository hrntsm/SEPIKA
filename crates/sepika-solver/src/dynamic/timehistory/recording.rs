//! 時刻歴応答の詳細記録（[`ThRecording`]）を組み立てる補助。
//!
//! - [`auto_record_every`] — 記録フレーム数が概ね 1000 になるよう `record_every` を自動決定
//! - [`ThRecorder`] — 記録の状態機械（層別集計・部材内力包絡・フレーム間引き）
//! - [`member_forces_linear`] / [`member_forces_nonlinear`] — 部材端力分布の算定
//!   （線形は `recover_forces`、非線形は `state_member_forces`）

use std::collections::BTreeMap;

use super::result::{AxialForcePeaks, StoryResponse, StrongAxisMomentPeaks, ThRecording};
use sepika_core::dof::{DofMap, DOF_PER_NODE};
use sepika_core::model::{ElementData, ElementKind, FrameSectionUse, Model};
use sepika_core::structure_kind::{structure_kind_of, StructureKind};
use sepika_element::behavior::{Ctx, ElementBehavior};
use sepika_element::frame::beam::MemberForces;

/// 時刻歴簡易検定で正負の局所 Mz 極値を記録する対象かを返す。
pub fn uses_signed_bending_peaks(model: &Model, elem: &ElementData) -> bool {
    matches!(
        elem.kind,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
    ) && model.element_section(elem).is_some_and(|section| {
        section.frame_use == Some(FrameSectionUse::Column)
            && section
                .shape
                .as_ref()
                .is_some_and(|shape| shape.is_asymmetric_built_h())
            && structure_kind_of(
                Some(section),
                model
                    .element_material(elem)
                    .map(|material| material.category),
            ) == StructureKind::S
    })
}

/// 記録フレーム数が概ね 1000 になるよう `record_every`（間引き係数）を自動決定する。
/// `n_steps` はステップ数（フレーム数は `n_steps+1`）。
pub(crate) fn auto_record_every(n_steps: usize) -> usize {
    (n_steps / 1000).max(1)
}

/// 線形解析用: 全自由節点変位 `u_free` から各要素の部材内力分布を復元する
/// （`model.elements` と同じ並び。`behaviors` は事前に `build_behavior` で
/// 構築済みのもの）。
pub(crate) fn member_forces_linear(
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    u_free: &[f64],
) -> Vec<Option<MemberForces>> {
    behaviors
        .iter()
        .map(|b| {
            let gdofs = b.global_dofs(dofmap);
            let u_elem = crate::common::elem_loop::gather_u_elem(&gdofs, u_free);
            b.recover_forces(&u_elem)
        })
        .collect()
}

/// 同一試行状態の荷重込み断面力を既存評価位置で返す。欠落・不正応答はエラー。
pub(crate) fn member_forces_nonlinear(
    model: &Model,
    behaviors: &[Box<dyn ElementBehavior>],
) -> Result<Vec<Option<MemberForces>>, sepika_math::solver::SolveError> {
    use sepika_math::solver::SolveError;
    let responses = crate::common::member_recording::member_recording_responses(
        model,
        behaviors,
        "非線形時刻歴解析",
    )
    .map_err(SolveError::InvalidInput)?;
    let ctx = Ctx { model };
    model
        .elements
        .iter()
        .zip(behaviors)
        .zip(responses)
        .map(|((elem, behavior), response)| {
            let error = |reason: &str| {
                SolveError::InvalidInput(format!(
                    "要素 ID {} の非線形時刻歴解析部材記録: {reason}",
                    elem.id.0
                ))
            };
            let forces = behavior.state_member_forces(&ctx);
            let required = response.is_some()
                || matches!(
                    elem.kind,
                    ElementKind::Beam
                        | ElementKind::Fiber
                        | ElementKind::MultiSpring
                        | ElementKind::Brace { .. }
                );
            let Some(forces) = forces else {
                return if required {
                    Err(error("部材内力を取得できません"))
                } else {
                    Ok(None)
                };
            };
            if forces.at.is_empty()
                || forces.at.iter().any(|(xi, f)| {
                    !xi.is_finite() || !(0.0..=1.0).contains(xi) || f.iter().any(|v| !v.is_finite())
                })
            {
                return Err(error("断面位置または部材内力が不正です"));
            }
            match response {
                Some(response) => {
                    let positions: Vec<_> = forces.at.iter().map(|(xi, _)| *xi).collect();
                    response
                        .section
                        .recover(&positions)
                        .map(Some)
                        .map_err(|reason| error(&reason))
                }
                None => Ok(Some(forces)),
            }
        })
        .collect()
}

/// 部材端力の包絡を更新する（各成分・各評価位置の絶対値最大。符号は極値の符号を保持）。
/// 評価位置数が一致しない場合は短い方の長さまでのみ更新する。
fn merge_peak_member_forces(peak: &mut [Option<MemberForces>], current: &[Option<MemberForces>]) {
    for (p, c) in peak.iter_mut().zip(current.iter()) {
        match (p.as_mut(), c) {
            (Some(pf), Some(cf)) => {
                let n = pf.at.len().min(cf.at.len());
                for i in 0..n {
                    for k in 0..6 {
                        if cf.at[i].1[k].abs() > pf.at[i].1[k].abs() {
                            pf.at[i].1[k] = cf.at[i].1[k];
                        }
                    }
                }
            }
            (None, Some(cf)) => *p = Some(cf.clone()),
            _ => {}
        }
    }
}

/// 部材内力分布を両端 2 点（最小 ξ・最大 ξ）のみへ間引く。
///
/// 包絡 `peak_member_forces` は全評価断面を保持する（本関数は適用しない）。
/// `at` が 2 点以下の要素はそのまま返す。
fn trim_member_forces_to_endpoints(forces: &[Option<MemberForces>]) -> Vec<Option<MemberForces>> {
    forces
        .iter()
        .map(|mf| {
            mf.as_ref().map(|f| {
                if f.at.len() <= 2 {
                    return f.clone();
                }
                let min =
                    f.at.iter()
                        .cloned()
                        .fold(f.at[0], |acc, x| if x.0 < acc.0 { x } else { acc });
                let max =
                    f.at.iter()
                        .cloned()
                        .fold(f.at[0], |acc, x| if x.0 > acc.0 { x } else { acc });
                MemberForces { at: vec![min, max] }
            })
        })
        .collect()
}

/// 階に属する節点の自由 DOF の一覧。
///
/// - `avg`: 階応答の代表値算定に使う当該方向並進 DOF と質量重みの組。
/// - `force`: 層せん断力の慣性力集計に使う DOF。当該方向の並進 DOF のみを含む。
#[derive(Default)]
struct StoryDofGroup {
    avg: Vec<(usize, f64)>,
    force: Vec<usize>,
}

/// 時刻歴応答の詳細記録（[`ThRecording`]）を組み立てる状態機械。
///
/// 呼び出し側（`linear`・`nonlinear` の各時刻歴ループ）は、各ステップ確定後に
/// [`Self::record_step`] を呼ぶ。フレーム間引き・部材内力包絡・層別集計をここに集約し、
/// 3 つの積分スキームで同一のロジックを共有する。
pub(crate) struct ThRecorder {
    record_every: usize,
    n_steps: u64,
    /// `weight_above[i]` = 当該層以上（i 番目〜最上層）の地震用重量の累積和。
    weight_above: Vec<f64>,
    groups_x: Vec<StoryDofGroup>,
    groups_y: Vec<StoryDofGroup>,

    frame_time: Vec<f64>,
    node_disp: Vec<Vec<[f64; 6]>>,
    member_forces: Vec<Vec<Option<MemberForces>>>,
    peak_member_forces: Vec<Option<MemberForces>>,
    axial_force_peaks: Vec<AxialForcePeaks>,
    strong_axis_moment_peaks: BTreeMap<usize, StrongAxisMomentPeaks>,

    story_x: StoryResponse,
    story_y: StoryResponse,
}

impl ThRecorder {
    /// `n_steps` は解析の全積分区間数（地震波のサンプル数 − 1）。`record_every` が
    /// `None` の場合は [`auto_record_every`] で自動決定する。
    pub(crate) fn new(
        model: &Model,
        dofmap: &DofMap,
        n_steps: usize,
        record_every: Option<usize>,
    ) -> Self {
        let record_every = record_every
            .unwrap_or_else(|| auto_record_every(n_steps))
            .max(1);
        let layers = model.layers();
        let n_story = layers.len();

        let story_repr_z: Vec<f64> = layers
            .iter()
            .map(|layer| {
                let mut sum_z = 0.0;
                let mut cnt = 0.0f64;
                for &nid in &layer.node_ids {
                    if let Some(node) = model.nodes.get(nid.index()) {
                        sum_z += node.coord[2];
                        cnt += 1.0;
                    }
                }
                if cnt > 0.0 {
                    sum_z / cnt
                } else {
                    0.0
                }
            })
            .collect();

        let build_groups = |dir_idx: usize| -> Vec<StoryDofGroup> {
            let mut groups: Vec<StoryDofGroup> = layers
                .iter()
                .map(|layer| {
                    let mut g = StoryDofGroup::default();
                    for &nid in &layer.node_ids {
                        let ni = nid.index();
                        let Some(node) = model.nodes.get(ni) else {
                            continue;
                        };
                        if let Some(a) = dofmap.active(ni * DOF_PER_NODE + dir_idx) {
                            let w = node.mass.map(|m| m[dir_idx]).unwrap_or(0.0).max(0.0);
                            g.avg.push((a as usize, w));
                            g.force.push(a as usize);
                        }
                    }
                    g
                })
                .collect();
            if !groups.is_empty() {
                let mut assigned = vec![false; model.nodes.len()];
                for layer in &layers {
                    for &nid in &layer.node_ids {
                        if let Some(f) = assigned.get_mut(nid.index()) {
                            *f = true;
                        }
                    }
                }
                for (ni, done) in assigned.iter().enumerate() {
                    if *done {
                        continue;
                    }
                    let Some(node) = model.nodes.get(ni) else {
                        continue;
                    };
                    if let Some(a) = dofmap.active(ni * DOF_PER_NODE + dir_idx) {
                        let nearest = story_repr_z
                            .iter()
                            .enumerate()
                            .min_by(|(_, za), (_, zb)| {
                                (node.coord[2] - *za)
                                    .abs()
                                    .partial_cmp(&(node.coord[2] - *zb).abs())
                                    .unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .map(|(i, _)| i);
                        if let Some(i) = nearest {
                            groups[i].force.push(a as usize);
                        }
                    }
                }
            }
            groups
        };

        let weights: Vec<f64> = layers.iter().map(|l| l.weight.unwrap_or(0.0)).collect();
        let mut weight_above = vec![0.0; weights.len()];
        {
            let mut acc = 0.0;
            for i in (0..weights.len()).rev() {
                acc += weights[i];
                weight_above[i] = acc;
            }
        }

        let story = StoryResponse {
            stories: layers.iter().map(|l| l.bottom).collect(),
            story_weight: weights,
            peak_shear_coeff: vec![0.0; n_story],
            peak_story_shear: vec![0.0; n_story],
            peak_floor_accel: vec![0.0; n_story],
            peak_floor_vel: vec![0.0; n_story],
            peak_floor_disp: vec![0.0; n_story],
            ..Default::default()
        };

        Self {
            record_every,
            n_steps: n_steps as u64,
            weight_above,
            groups_x: build_groups(0),
            groups_y: build_groups(1),
            frame_time: Vec::new(),
            node_disp: Vec::new(),
            member_forces: Vec::new(),
            peak_member_forces: vec![None; model.elements.len()],
            axial_force_peaks: vec![AxialForcePeaks::default(); model.elements.len()],
            strong_axis_moment_peaks: model
                .elements
                .iter()
                .enumerate()
                .filter(|(_, elem)| uses_signed_bending_peaks(model, elem))
                .map(|(index, _)| (index, StrongAxisMomentPeaks::default()))
                .collect(),
            story_x: story.clone(),
            story_y: story,
        }
    }

    /// 階ごとの層せん断力・階絶対加速度・階速度・階変位を求める（1 方向分）。
    /// `m_r` は当該方向の `M·r`（自由 DOF 空間）、`ma_free` は自由 DOF 空間の
    /// `M·a_free`（[`super::common::sparse_matvec_into`] で 1 ステップに 1 回だけ
    /// 算定したものを共有する）、`xg` は当該時刻・当該方向の地動加速度。
    ///
    /// 層せん断力は「当該層以上に属する節点の並進 DOF の慣性力」の累積の符号反転
    /// （節点慣性力ベクトル `f_abs = M·a_free + ẍg・M・r` の当該方向並進成分のみを
    /// 集計、`history::record_history_step` のベースシアと同じ定義）。
    #[allow(clippy::too_many_arguments)]
    fn compute_story_dir(
        groups: &[StoryDofGroup],
        m_r: &[f64],
        u_free: &[f64],
        v_free: &[f64],
        a_free: &[f64],
        ma_free: &[f64],
        xg: f64,
    ) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let n = groups.len();
        let mut level_force = vec![0.0; n];
        let mut accel = vec![0.0; n];
        let mut vel = vec![0.0; n];
        let mut disp = vec![0.0; n];
        for (i, g) in groups.iter().enumerate() {
            if g.avg.is_empty() && g.force.is_empty() {
                continue;
            }
            let (mut total_w, mut sum_a, mut sum_v, mut sum_u) = (0.0, 0.0, 0.0, 0.0);
            let (mut simple_a, mut simple_v, mut simple_u) = (0.0, 0.0, 0.0);
            for &(dof, w) in &g.avg {
                let a_abs = a_free.get(dof).copied().unwrap_or(0.0) + xg;
                let v_val = v_free.get(dof).copied().unwrap_or(0.0);
                let u_val = u_free.get(dof).copied().unwrap_or(0.0);
                simple_a += a_abs;
                simple_v += v_val;
                simple_u += u_val;
                sum_a += w * a_abs;
                sum_v += w * v_val;
                sum_u += w * u_val;
                total_w += w;
            }
            if total_w > 0.0 {
                accel[i] = sum_a / total_w;
                vel[i] = sum_v / total_w;
                disp[i] = sum_u / total_w;
            } else if !g.avg.is_empty() {
                let cnt = g.avg.len() as f64;
                accel[i] = simple_a / cnt;
                vel[i] = simple_v / cnt;
                disp[i] = simple_u / cnt;
            }
            let mut lf = 0.0;
            for &dof in &g.force {
                let f_abs = ma_free.get(dof).copied().unwrap_or(0.0)
                    + xg * m_r.get(dof).copied().unwrap_or(0.0);
                lf += f_abs;
            }
            level_force[i] = lf;
        }
        let mut shear = vec![0.0; n];
        let mut acc = 0.0;
        for i in (0..n).rev() {
            acc += level_force[i];
            shear[i] = -acc;
        }
        (shear, accel, vel, disp)
    }

    /// 1 ステップ分の記録を追加する。`step` は確定した時刻ステップ番号
    /// （0 が初期状態、`n_steps` が最終ステップ）。`u_free`/`v_free`/`a_free` は
    /// 呼び出し側で展開済みの自由 DOF 空間の変位・速度・加速度（`dofmap` の
    /// アクティブ添字順）で、`m_r_x`/`m_r_y`・`ma_free` も呼び出し側で算定したものを共有する。
    /// `member_forces_now` は当該ステップの全要素の部材内力分布
    /// （[`member_forces_linear`] / [`member_forces_nonlinear`]）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_step(
        &mut self,
        step: u64,
        time: f64,
        model: &Model,
        dofmap: &DofMap,
        m_r_x: &[f64],
        m_r_y: &[f64],
        ma_free: &[f64],
        u_free: &[f64],
        v_free: &[f64],
        a_free: &[f64],
        xg_x: f64,
        xg_y: f64,
        member_forces_now: &[Option<MemberForces>],
    ) {
        merge_peak_member_forces(&mut self.peak_member_forces, member_forces_now);
        for (peaks, forces) in self.axial_force_peaks.iter_mut().zip(member_forces_now) {
            if let Some(forces) = forces {
                peaks.record(time, forces);
            }
        }
        for (&index, peaks) in &mut self.strong_axis_moment_peaks {
            if let Some(forces) = member_forces_now.get(index).and_then(Option::as_ref) {
                peaks.record(time, forces);
            }
        }

        let (shear_x, accel_x, vel_x, disp_x) =
            Self::compute_story_dir(&self.groups_x, m_r_x, u_free, v_free, a_free, ma_free, xg_x);
        let (shear_y, accel_y, vel_y, disp_y) =
            Self::compute_story_dir(&self.groups_y, m_r_y, u_free, v_free, a_free, ma_free, xg_y);

        for i in 0..self.story_x.story_weight.len() {
            let above = self.weight_above[i];
            if above > 0.0 {
                let ci_x = shear_x[i].abs() / above;
                if ci_x > self.story_x.peak_shear_coeff[i] {
                    self.story_x.peak_shear_coeff[i] = ci_x;
                }
                let ci_y = shear_y[i].abs() / above;
                if ci_y > self.story_y.peak_shear_coeff[i] {
                    self.story_y.peak_shear_coeff[i] = ci_y;
                }
            }
        }

        Self::update_peak_abs(&mut self.story_x.peak_story_shear, &shear_x);
        Self::update_peak_abs(&mut self.story_y.peak_story_shear, &shear_y);
        Self::update_peak_abs(&mut self.story_x.peak_floor_accel, &accel_x);
        Self::update_peak_abs(&mut self.story_y.peak_floor_accel, &accel_y);
        Self::update_peak_abs(&mut self.story_x.peak_floor_vel, &vel_x);
        Self::update_peak_abs(&mut self.story_y.peak_floor_vel, &vel_y);
        Self::update_peak_abs(&mut self.story_x.peak_floor_disp, &disp_x);
        Self::update_peak_abs(&mut self.story_y.peak_floor_disp, &disp_y);

        if self.frame_time.is_empty()
            || step.is_multiple_of(self.record_every as u64)
            || step == self.n_steps
        {
            self.frame_time.push(time);
            self.node_disp
                .push(dofmap.expand_to_nodes(u_free, model.nodes.len()));
            self.member_forces
                .push(trim_member_forces_to_endpoints(member_forces_now));
            self.story_x.story_shear.push(shear_x);
            self.story_y.story_shear.push(shear_y);
            self.story_x.floor_accel.push(accel_x);
            self.story_y.floor_accel.push(accel_y);
            self.story_x.floor_vel.push(vel_x);
            self.story_y.floor_vel.push(vel_y);
            self.story_x.floor_disp.push(disp_x);
            self.story_y.floor_disp.push(disp_y);
        }
    }

    /// 絶対値最大の更新（`peak[i] = max(peak[i], |current[i]|)`）。長さが異なる場合は短い方まで。
    fn update_peak_abs(peak: &mut [f64], current: &[f64]) {
        for (p, &c) in peak.iter_mut().zip(current.iter()) {
            let c_abs = c.abs();
            if c_abs > *p {
                *p = c_abs;
            }
        }
    }

    /// 記録を確定し [`ThRecording`] を返す。
    pub(crate) fn finish(self) -> ThRecording {
        ThRecording {
            record_every: self.record_every,
            frame_time: self.frame_time,
            node_disp: self.node_disp,
            story_x: self.story_x,
            story_y: self.story_y,
            member_forces: self.member_forces,
            peak_member_forces: self.peak_member_forces,
            axial_force_peaks: self.axial_force_peaks,
            strong_axis_moment_peaks: self.strong_axis_moment_peaks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId, StoryId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
        Node, Section, Story,
    };

    #[test]
    fn axial_force_peaks_keep_both_signs_with_concurrent_forces_without_frame_decimation() {
        let model = two_story_density_mass_model(7.85e-9);
        let dofmap = DofMap::build(&model);
        let zeros = vec![0.0; dofmap.n_active()];
        let forces = |n, mz, middle_n| {
            vec![
                Some(MemberForces {
                    at: vec![
                        (0.0, [n, 0.0, 0.0, 0.0, 0.0, mz]),
                        (0.5, [middle_n, 0.0, 0.0, 0.0, 0.0, -mz]),
                        (1.0, [n, 0.0, 0.0, 0.0, 0.0, mz]),
                    ],
                }),
                None,
            ]
        };
        let history = [
            forces(0.0, 0.0, 0.0),
            forces(-3_322_800.0, 10_000_000.0, -2.0),
            forces(3_400_000.0, 0.0, 3.0),
            forces(-3_322_800.0, 11_000_000.0, -4.0),
            forces(0.0, 0.0, 0.0),
        ];
        let record_part = |every, steps: std::ops::RangeInclusive<usize>| {
            let mut recorder = ThRecorder::new(&model, &dofmap, 4, Some(every));
            for step in steps {
                recorder.record_step(
                    step as u64,
                    step as f64,
                    &model,
                    &dofmap,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    0.0,
                    0.0,
                    &history[step],
                );
            }
            recorder.finish()
        };
        for (every, expected_frames) in [(1, vec![0.0, 1.0, 2.0, 3.0, 4.0]), (4, vec![0.0, 4.0])] {
            let recording = record_part(every, 0..=4);
            assert_eq!(recording.frame_time, expected_frames);
            let envelope = recording.peak_member_forces[0].as_ref().unwrap();
            assert_eq!(
                envelope.at[0].1,
                [3_400_000.0, 0.0, 0.0, 0.0, 0.0, 11_000_000.0]
            );
            let snapshots = &recording.axial_force_peaks[0].snapshots;
            assert_eq!(snapshots.len(), 3);
            for (snapshot, step, positions) in [
                (&snapshots[0], 1, &[0, 2][..]),
                (&snapshots[1], 2, &[0, 1, 2][..]),
                (&snapshots[2], 3, &[1][..]),
            ] {
                assert_eq!(snapshot.time, step as f64);
                assert_eq!(&snapshot.forces, history[step][0].as_ref().unwrap());
                assert_eq!(snapshot.positions, positions);
            }
            assert!(recording.axial_force_peaks[1].snapshots.is_empty());

            let mut first = record_part(every, 0..=2).axial_force_peaks;
            let last = record_part(every, 2..=4).axial_force_peaks;
            for (before, after) in first.iter_mut().zip(last) {
                before.merge(after);
            }
            assert_eq!(first, recording.axial_force_peaks);
            assert!(recording.strong_axis_moment_peaks.is_empty());
        }
    }

    #[test]
    fn signed_bending_peaks_keep_the_whole_section_force_at_non_displayed_steps() {
        let mut model = two_story_density_mass_model(7.85e-9);
        let mut section = sepika_core::section_shape::SectionShape::SteelBuiltH {
            height: 400.0,
            upper_width: 80.0,
            upper_thick: 12.0,
            lower_width: 180.0,
            lower_thick: 20.0,
            web_thick: 8.0,
        }
        .to_section(SectionId(1), "上下非対称組立H".into());
        section.material = model.sections[0].material;
        section.frame_use = Some(FrameSectionUse::Column);
        model.sections.push(section);
        model.elements[0].section = Some(SectionId(1));
        let dofmap = DofMap::build(&model);
        let zeros = vec![0.0; dofmap.n_active()];
        let history = [
            MemberForces {
                at: vec![
                    (0.0, [-78e3, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    (0.5, [-78e3, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    (1.0, [0.0; 6]),
                ],
            },
            MemberForces {
                at: vec![
                    (0.0, [-77e3, 2.0, -3.0, 4.0, -5.0, -80e6]),
                    (0.5, [-77e3, 12.0, -13.0, 14.0, -15.0, 150e6]),
                    (1.0, [0.0; 6]),
                ],
            },
            MemberForces {
                at: vec![
                    (0.0, [-77e3, -2.0, 3.0, -4.0, 5.0, 90e6]),
                    (0.5, [-77e3, -12.0, 13.0, -14.0, 15.0, -155e6]),
                    (1.0, [0.0; 6]),
                ],
            },
            MemberForces {
                at: vec![
                    (0.0, [-78e3, 0.0, 0.0, 0.0, 0.0, 0.0]),
                    (0.5, [-78e3, 1.0, 2.0, 3.0, 4.0, 150e6]),
                    (1.0, [0.0; 6]),
                ],
            },
        ];
        let record = |first: usize, last: usize| {
            let mut recorder = ThRecorder::new(&model, &dofmap, 3, Some(3));
            for (step, forces) in history.iter().enumerate().take(last + 1).skip(first) {
                recorder.record_step(
                    step as u64,
                    step as f64,
                    &model,
                    &dofmap,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    &zeros,
                    0.0,
                    0.0,
                    &[Some(forces.clone()), Some(forces.clone())],
                );
            }
            recorder.finish()
        };
        let full = record(0, 3);
        assert_eq!(full.frame_time, vec![0.0, 3.0]);
        assert!(full
            .member_forces
            .iter()
            .all(|frame| frame[0].as_ref().unwrap().at.len() == 2));
        assert_eq!(full.strong_axis_moment_peaks.len(), 1);
        let peaks = &full.strong_axis_moment_peaks[&0];
        assert_eq!(peaks.snapshots.len(), 2);
        for (snapshot, step) in peaks.snapshots.iter().zip([1, 2]) {
            assert_eq!(snapshot.time, step as f64);
            assert_eq!(snapshot.forces, history[step]);
            assert_eq!(snapshot.positions, vec![0, 1]);
        }
        let mut earlier = record(0, 1).strong_axis_moment_peaks.remove(&0).unwrap();
        let later = record(1, 3).strong_axis_moment_peaks.remove(&0).unwrap();
        earlier.merge(later);
        assert_eq!(&earlier, peaks);
    }

    /// 密度による一貫質量を持つ 2 層（3 節点）の柱列。曲げ回転（Ry）を解放し、一貫質量行列の並進-回転連成を有効にする。
    fn two_story_density_mass_model(density: f64) -> Model {
        let free_ux_ry = Dof6Mask(0b101110);
        Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: [0.0, 0.0, 0.0],
                    restraint: Dof6Mask::FIXED,
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: [0.0, 0.0, 3000.0],
                    restraint: free_ux_ry,
                    mass: None,
                    story: Some(StoryId(1)),
                    support_spring: None,
                },
                Node {
                    id: NodeId(2),
                    coord: [0.0, 0.0, 6000.0],
                    restraint: free_ux_ry,
                    mass: None,
                    story: Some(StoryId(2)),
                    support_spring: None,
                },
            ],
            elements: vec![
                ElementData {
                    id: ElemId(0),
                    kind: ElementKind::Beam,
                    nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                    section: Some(SectionId(0)),
                    local_axis: LocalAxis {
                        ref_vector: [1.0, 0.0, 0.0],
                    },
                    end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
                    plastic_zone: None,
                    spring: None,
                },
                ElementData {
                    id: ElemId(1),
                    kind: ElementKind::Beam,
                    nodes: smallvec::smallvec![NodeId(1), NodeId(2)],
                    section: Some(SectionId(0)),
                    local_axis: LocalAxis {
                        ref_vector: [1.0, 0.0, 0.0],
                    },
                    end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
                    plastic_zone: None,
                    spring: None,
                },
            ],
            sections: vec![Section {
                frame_use: None,
                id: SectionId(0),
                name: "col".into(),
                area: 10000.0,
                iy: 8.333e6,
                iz: 8.333e6,
                j: 1.0e6,
                depth: 100.0,
                width: 100.0,
                as_y: 0.0,
                as_z: 0.0,
                floor: None,
                panel_thickness: None,
                thickness: None,
                shape: None,
                material: Some(MaterialId(0)),
                rebar_material: None,
                shear_rebar_material: None,
                steel_material: None,
                property_basis: Default::default(),
            }],
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "steel".into(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density,
                shear: None,
                fc: None,
                fy: None,
            }],
            stories: vec![
                Story {
                    wall_weights: Vec::new(),
                    level_kind: Default::default(),
                    structure: Default::default(),
                    id: StoryId(0),
                    name: "1F".into(),
                    elevation: Some(0.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(0)],
                    seismic_weight: None,
                    weight_override: None,
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
                Story {
                    wall_weights: Vec::new(),
                    level_kind: Default::default(),
                    structure: Default::default(),
                    id: StoryId(1),
                    name: "2F".into(),
                    elevation: Some(3000.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(1)],
                    seismic_weight: Some(1000.0),
                    weight_override: None,
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
                Story {
                    wall_weights: Vec::new(),
                    level_kind: Default::default(),
                    structure: Default::default(),
                    id: StoryId(2),
                    name: "3F".into(),
                    elevation: Some(6000.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(2)],
                    seismic_weight: Some(1000.0),
                    weight_override: None,
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
            ],
            ..Default::default()
        }
    }

    /// 2 層・一貫質量モデルの層せん断力と階変位は、並進応答だけを集計する。
    #[test]
    fn test_story_shear_sum_matches_base_shear_with_consistent_mass() {
        let model = two_story_density_mass_model(7.85e-9);
        let dofmap = DofMap::build(&model);
        let reducer = crate::common::constraint::Reducer::build(&model, &dofmap);

        let damping = crate::dynamic::damping::Damping::StiffnessProportional {
            h: 0.02,
            omega: 10.0,
            basis: crate::dynamic::damping::StiffnessKind::Initial,
        };
        let dt = 0.001;
        let n_steps = 30;
        let wave = super::super::GroundMotion {
            dt,
            accel_x: {
                let mut a = vec![0.0; n_steps];
                a[0] = 2000.0;
                a[1] = -1000.0;
                a
            },
            accel_y: None,
            accel_theta: None,
        };
        let newmark = super::super::NewmarkCfg {
            beta: 0.25,
            gamma: 0.5,
            dt,
        };

        let result = super::super::linear_time_history_analysis(
            &model,
            &dofmap,
            &reducer,
            &wave,
            &newmark,
            &damping,
            &[0.0],
            &[0.0],
            None,
        )
        .expect("should run");

        let recording = result.recording.expect("recording");
        assert_eq!(recording.story_x.stories.len(), 2);
        assert_eq!(
            recording.story_x.floor_disp.len(),
            recording.frame_time.len()
        );
        assert_eq!(recording.node_disp.len(), recording.frame_time.len());
        let mut nontrivial_translation = [false; 2];
        let mut checked = 0;
        for (k, &t) in recording.frame_time.iter().enumerate() {
            // 時刻を再計算すると丸め誤差で一致しないため、記録済みの刻みそのものを照合する。
            let Some(step) = result.time.iter().position(|&tt| tt == t) else {
                continue;
            };
            assert_eq!(recording.story_x.floor_disp[k].len(), 2);
            for (layer, nontrivial) in nontrivial_translation.iter_mut().enumerate() {
                let node = &recording.node_disp[k][layer + 1];
                assert_eq!(recording.story_x.floor_disp[k][layer], node[0]);
                *nontrivial |= node[0].abs() > 1e-9 && (node[0] - node[4]).abs() > 1e-9;
            }
            let layer0_shear = recording.story_x.story_shear[k][0];
            let expected = result.history.base_shear[step];
            assert!(
                (layer0_shear - expected).abs() < expected.abs().max(1.0) * 1e-6,
                "frame {k} (t={t}): layer0 shear {layer0_shear} should match base shear {expected}"
            );
            checked += 1;
        }
        assert_eq!(checked, recording.frame_time.len());
        assert_eq!(nontrivial_translation, [true; 2]);
    }
}
