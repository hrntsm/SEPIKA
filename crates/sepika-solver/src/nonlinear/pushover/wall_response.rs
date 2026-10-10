//! 壁要素単体の基準点付き合力と確定ステップ応答。
use crate::statics::analysis::SeismicDir;
use sepika_core::dof::DofMap;
use sepika_core::ids::{ElemId, NodeId, WallPlateId};
use sepika_core::model::{wall_element_geometry, ElementKind, Model};
use sepika_element::behavior::{Ctx, ElementBehavior};
use sepika_element::frame::beam::MemberForces;

/// 力 [N]・モーメント [Nmm] は全体座標、基準点は未変形座標 [mm]。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WallResultant {
    pub origin_mm: [f64; 3],
    pub force_n: [f64; 3],
    pub moment_nmm: [f64; 3],
}
impl WallResultant {
    /// 同じ力系を新しい基準点 [mm] へ移す。
    pub fn at_origin(&self, origin_mm: [f64; 3]) -> Self {
        let arm = std::array::from_fn(|d| self.origin_mm[d] - origin_mm[d]);
        let shift = cross(arm, self.force_n);
        Self {
            origin_mm,
            force_n: self.force_n,
            moment_nmm: std::array::from_fn(|d| self.moment_nmm[d] + shift[d]),
        }
    }
}

/// 未提供値の理由。物理的にゼロの記録とは別に扱う。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallUnavailableReason {
    InvalidGeometry,
    UnsupportedGeometry,
    MissingBehavior,
    InvalidForce,
    UnbalancedForces,
    InvalidDisplacement,
    DofOrderMismatch,
    MaterialShearNotRecovered,
    VirtualColumnNotRecorded,
    LineEventsNotApplicable,
    LegacyNotRecorded,
    InputIdentityNotRecorded,
}
impl WallUnavailableReason {
    pub fn description(self) -> &'static str {
        match self {
            Self::InvalidGeometry => "壁幾何が不正・退化・節点参照欠落",
            Self::UnsupportedGeometry => "矩形鉛直壁以外の応答契約は未検証",
            Self::MissingBehavior => "壁要素の解析状態が未記録",
            Self::UnbalancedForces => "壁節点内力の力・モーメント釣合いが許容差を超えています",
            Self::InvalidForce => "24成分の有限な壁節点内力がありません",
            Self::InvalidDisplacement => "有限な壁節点変位がありません",
            Self::DofOrderMismatch => "壁の正規化節点と内力自由度順が一致しません",
            Self::MaterialShearNotRecovered => {
                "剛体回転・曲げを除いた材料せん断ひずみを復元していません"
            }
            Self::VirtualColumnNotRecorded => "仮想壁柱の断面力が未記録",
            Self::LineEventsNotApplicable => {
                "線材ヒンジ・せん断降伏イベントは壁内部状態の判定に適用できません"
            }
            Self::LegacyNotRecorded => "壁専用の確定ステップ応答が未記録です。再解析してください",
            Self::InputIdentityNotRecorded => "結果の入力識別が未記録です。再解析してください",
        }
    }
}

/// 壁単体。境界柱梁の内力を含まない。節点順は下a・下b・上a・上b。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WallResponse {
    pub nodes: [NodeId; 4],
    pub inplane_axis: [f64; 3],
    pub column_axis: [f64; 3],
    /// 仮想壁柱の局所x/y/z軸。断面力のQ・M成分に対応する。
    pub virtual_column_axes: [[f64; 3]; 3],
    pub bottom: WallResultant,
    pub top: WallResultant,
    /// 上下合力の残差。下辺中点を共通基準点とする。
    pub equilibrium_residual: WallResultant,
    pub qw_n: f64,
    pub qdir_n: f64,
    pub height_mm: f64,
    pub delta_wall_mm: f64,
    pub chord_rotation_rad: f64,
    pub material_shear_strain: Option<f64>,
    pub material_shear_unavailable: Option<WallUnavailableReason>,
    /// 仮想壁柱の局所断面力。位置0→1、Nは引張正。既存の断面力規約を保持。
    pub virtual_column: Option<MemberForces>,
    pub virtual_column_unavailable: Option<WallUnavailableReason>,
    pub line_events: Option<Vec<super::types::HingeEvent>>,
    pub line_events_unavailable: Option<WallUnavailableReason>,
}

/// 0始まりの確定stepと生成要素。壁版対応は解析入口が壁展開インデックスで設定。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WallStepResponse {
    pub step: u32,
    pub elem: ElemId,
    pub plate: Option<WallPlateId>,
    pub response: Option<WallResponse>,
    pub unavailable: Option<WallUnavailableReason>,
}

/// 解析実行ごとの識別。入力世代はGUI/MCPの既存入力照合データで識別する。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WallRunIdentity {
    pub run_id: String,
    pub input_generation: Option<Vec<u8>>,
    pub input_unavailable: Option<WallUnavailableReason>,
}
impl WallRunIdentity {
    pub(super) fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("解析実行時刻")
            .as_nanos();
        Self {
            run_id: format!("{time}-{}", SEQUENCE.fetch_add(1, Ordering::Relaxed)),
            input_generation: None,
            input_unavailable: Some(WallUnavailableReason::InputIdentityNotRecorded),
        }
    }
}

impl super::types::PushoverResult {
    /// 指定確定stepの壁単体応答。未記録・不正をゼロへ置換しない。
    pub fn wall_response_at(
        &self,
        elem: ElemId,
        step: u32,
    ) -> Result<&WallResponse, WallUnavailableReason> {
        if step as usize >= self.steps.len() {
            return Err(WallUnavailableReason::LegacyNotRecorded);
        }
        let record = self
            .wall_history
            .as_ref()
            .and_then(|records| records.iter().find(|r| r.elem == elem && r.step == step))
            .ok_or(WallUnavailableReason::LegacyNotRecorded)?;
        if let Some(reason) = record.unavailable {
            return Err(reason);
        }
        let response = record.response.as_ref().ok_or(
            record
                .unavailable
                .unwrap_or(WallUnavailableReason::LegacyNotRecorded),
        )?;
        if [
            response.qw_n,
            response.qdir_n,
            response.height_mm,
            response.delta_wall_mm,
            response.chord_rotation_rad,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || [
                &response.bottom,
                &response.top,
                &response.equilibrium_residual,
            ]
            .iter()
            .any(|r| {
                r.force_n
                    .iter()
                    .chain(&r.moment_nmm)
                    .chain(&r.origin_mm)
                    .any(|v| !v.is_finite())
            })
        {
            return Err(WallUnavailableReason::InvalidForce);
        }
        if response.height_mm <= 0.0 {
            return Err(WallUnavailableReason::InvalidGeometry);
        }
        let force_scale = response
            .top
            .force_n
            .iter()
            .chain(&response.bottom.force_n)
            .fold(1.0_f64, |a, b| a.max(b.abs()));
        let moment_scale = response
            .top
            .moment_nmm
            .iter()
            .chain(&response.bottom.moment_nmm)
            .fold(force_scale * response.height_mm, |a, b| a.max(b.abs()));
        if response
            .equilibrium_residual
            .force_n
            .iter()
            .any(|v| v.abs() > 1e-6 * force_scale)
            || response
                .equilibrium_residual
                .moment_nmm
                .iter()
                .any(|v| v.abs() > 1e-6 * moment_scale.max(1.0))
        {
            return Err(WallUnavailableReason::UnbalancedForces);
        }
        Ok(response)
    }
    pub fn identify_wall_input(&mut self, input: Vec<u8>) {
        if let Some(run) = &mut self.wall_run {
            run.input_generation = Some(input);
            run.input_unavailable = None;
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|d| a[d] * b[d]).sum()
}
fn resultant(coords: &[[f64; 3]], force: &[f64], origin_mm: [f64; 3]) -> WallResultant {
    let mut result = WallResultant {
        origin_mm,
        force_n: [0.0; 3],
        moment_nmm: [0.0; 3],
    };
    for (p, f) in coords.iter().zip(force.as_chunks::<6>().0) {
        let arm = std::array::from_fn(|d| p[d] - origin_mm[d]);
        let moment = cross(arm, [f[0], f[1], f[2]]);
        for d in 0..3 {
            result.force_n[d] += f[d];
            result.moment_nmm[d] += f[3 + d] + moment[d];
        }
    }
    result
}

pub(crate) fn record_wall_step(
    model: &Model,
    dofmap: &DofMap,
    behaviors: &[Box<dyn ElementBehavior>],
    disp: &[f64],
    dir: SeismicDir,
    step: u32,
) -> Vec<WallStepResponse> {
    model
        .elements
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e.kind, ElementKind::Wall))
        .map(|(index, elem)| {
            let compute = || -> Result<WallResponse, WallUnavailableReason> {
                let g = wall_element_geometry(elem, model)
                    .ok_or(WallUnavailableReason::InvalidGeometry)?;
                if elem.nodes.len() != 4 || !g.h.is_finite() {
                    return Err(WallUnavailableReason::InvalidGeometry);
                }
                let nodes = [g.bottom[0], g.bottom[1], g.top[0], g.top[1]];
                let coords = nodes.map(|id| model.nodes[id.index()].coord);
                if coords.iter().flatten().any(|x| !x.is_finite()) {
                    return Err(WallUnavailableReason::InvalidGeometry);
                }
                let tolerance = g.h.max(g.lw) * 1e-8;
                if (coords[0][2] - coords[1][2]).abs() > tolerance
                    || (coords[2][2] - coords[3][2]).abs() > tolerance
                    || coords[2][2] - coords[0][2] <= tolerance
                    || (0..2).any(|d| {
                        (coords[0][d] - coords[2][d]).abs() > tolerance
                            || (coords[1][d] - coords[3][d]).abs() > tolerance
                    })
                {
                    return Err(WallUnavailableReason::UnsupportedGeometry);
                }
                let b = behaviors
                    .get(index)
                    .ok_or(WallUnavailableReason::MissingBehavior)?;
                let expected = sepika_element::behavior::node_global_dofs(&nodes, dofmap);
                if b.n_dof() != 24 || b.global_dofs(dofmap) != expected {
                    return Err(WallUnavailableReason::DofOrderMismatch);
                }
                let ctx = Ctx { model };
                let f = b.internal_force(&ctx);
                if f.data.len() != 24 || f.data.iter().any(|x| !x.is_finite()) {
                    return Err(WallUnavailableReason::InvalidForce);
                }
                let bottom = resultant(&coords[..2], &f.data[..12], g.bottom_center);
                let top = resultant(&coords[2..], &f.data[12..], g.top_center);
                let equilibrium_residual = resultant(&coords, &f.data, g.bottom_center);
                let mut delta = [0.0; 3];
                for (i, node) in nodes.iter().enumerate() {
                    for (d, value) in delta.iter_mut().enumerate() {
                        let u = match dofmap.active(node.index() * 6 + d) {
                            Some(a) => *disp
                                .get(a as usize)
                                .ok_or(WallUnavailableReason::InvalidDisplacement)?,
                            None => 0.0,
                        };
                        if !u.is_finite() {
                            return Err(WallUnavailableReason::InvalidDisplacement);
                        }
                        *value += if i < 2 { -0.5 * u } else { 0.5 * u };
                    }
                }
                let delta_wall_mm = dot(delta, g.ex_bottom);
                let virtual_column = b.state_member_forces(&ctx).filter(|mf| {
                    !mf.at.is_empty()
                        && mf
                            .at
                            .iter()
                            .all(|(p, f)| p.is_finite() && f.iter().all(|x| x.is_finite()))
                });
                Ok(WallResponse {
                    nodes,
                    inplane_axis: g.ex_bottom,
                    virtual_column_axes: sepika_element::transform::LocalFrame::from_nodes(
                        g.bottom_center,
                        g.top_center,
                        g.ex_bottom,
                    )
                    .rot,
                    column_axis: std::array::from_fn(|d| {
                        (g.top_center[d] - g.bottom_center[d]) / g.h
                    }),
                    qw_n: dot(top.force_n, g.ex_bottom),
                    qdir_n: top.force_n[match dir {
                        SeismicDir::X => 0,
                        SeismicDir::Y => 1,
                    }],
                    bottom,
                    top,
                    equilibrium_residual,
                    height_mm: g.h,
                    delta_wall_mm,
                    chord_rotation_rad: delta_wall_mm / g.h,
                    material_shear_strain: None,
                    material_shear_unavailable: Some(
                        WallUnavailableReason::MaterialShearNotRecovered,
                    ),
                    virtual_column_unavailable: virtual_column
                        .is_none()
                        .then_some(WallUnavailableReason::VirtualColumnNotRecorded),
                    virtual_column,
                    line_events: None,
                    line_events_unavailable: Some(WallUnavailableReason::LineEventsNotApplicable),
                })
            };
            match compute() {
                Ok(response) => WallStepResponse {
                    step,
                    elem: elem.id,
                    plate: None,
                    response: Some(response),
                    unavailable: None,
                },
                Err(reason) => WallStepResponse {
                    step,
                    elem: elem.id,
                    plate: None,
                    response: None,
                    unavailable: Some(reason),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_element::behavior::{LocalMat, LocalVec, MassOption};
    struct PrescribedWall {
        nodes: [NodeId; 4],
        forces: Vec<f64>,
    }
    impl ElementBehavior for PrescribedWall {
        fn n_dof(&self) -> usize {
            24
        }
        fn global_dofs(&self, map: &DofMap) -> smallvec::SmallVec<[usize; 24]> {
            sepika_element::behavior::node_global_dofs(&self.nodes, map)
        }
        fn tangent_stiffness(&self, _: &Ctx) -> LocalMat {
            LocalMat::zeros(24)
        }
        fn internal_force(&self, _: &Ctx) -> LocalVec {
            LocalVec {
                data: self.forces.iter().copied().collect(),
            }
        }
        fn mass_matrix(&self, _: MassOption) -> LocalMat {
            LocalMat::zeros(24)
        }
        fn state_member_forces(&self, _: &Ctx) -> Option<MemberForces> {
            Some(MemberForces {
                at: vec![
                    (0.0, [-123.0, 456.0, 0.0, 0.0, 0.0, 789.0]),
                    (1.0, [-123.0, 456.0, 0.0, 0.0, 0.0, -789.0]),
                ],
            })
        }
    }
    fn model() -> Model {
        let mut m = super::super::tests::wall_story_model_with(4000.0, 100_000.0);
        m.elements.truncate(1);
        for n in &mut m.nodes {
            n.restraint = sepika_core::dof::Dof6Mask::FREE;
        }
        m
    }
    fn response(
        m: &Model,
        forces_by_node: [[f64; 6]; 4],
        disp: &[f64],
        dir: SeismicDir,
    ) -> WallResponse {
        let g = wall_element_geometry(&m.elements[0], m).unwrap();
        let nodes = [g.bottom[0], g.bottom[1], g.top[0], g.top[1]];
        let b: Vec<Box<dyn ElementBehavior>> = vec![Box::new(PrescribedWall {
            nodes,
            forces: nodes
                .iter()
                .flat_map(|id| forces_by_node[id.index()])
                .collect(),
        })];
        record_wall_step(m, &DofMap::build(m), &b, disp, dir, 0)
            .remove(0)
            .response
            .unwrap()
    }
    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }
    fn close3(a: [f64; 3], b: [f64; 3]) {
        for d in 0..3 {
            close(a[d], b[d]);
        }
    }

    #[test]
    fn upper_100kn_at_3000mm_has_300knm_arm_plus_nodal_moments() {
        let m = model();
        let f = [
            [-50_000.0, 0.0, 0.0, 0.0, -155e6, 0.0],
            [-50_000.0, 0.0, 0.0, 0.0, -155e6, 0.0],
            [50_000.0, 0.0, 0.0, 0.0, 7e6, 0.0],
            [50_000.0, 0.0, 0.0, 0.0, 3e6, 0.0],
        ];
        let r = response(&m, f, &[0.0; 24], SeismicDir::X);
        close(r.qw_n, 100_000.0);
        close(r.qdir_n, 100_000.0);
        close3(r.top.origin_mm, [2000.0, 0.0, 3000.0]);
        close(r.top.moment_nmm[1], 10e6);
        close(r.top.at_origin(r.bottom.origin_mm).moment_nmm[1], 310e6);
        close3(r.equilibrium_residual.force_n, [0.0; 3]);
        close3(r.equilibrium_residual.moment_nmm, [0.0; 3]);
        close3(
            r.top
                .at_origin(r.bottom.origin_mm)
                .at_origin(r.top.origin_mm)
                .moment_nmm,
            r.top.moment_nmm,
        );
        let mut force_only = f;
        for row in &mut force_only {
            row[4] = 0.0;
        }
        let r = response(&m, force_only, &[0.0; 24], SeismicDir::X);
        close(r.top.at_origin(r.bottom.origin_mm).moment_nmm[1], 300e6);
        close(r.top.moment_nmm[1], 0.0);
    }

    #[test]
    fn all_24_components_preserve_independent_force_and_moment_equilibrium() {
        let m = model();
        let f = [
            [-50_000.0, -3000.0, 2000.0, 1e6, -100e6, -2e6],
            [-50_000.0, 3000.0, -2000.0, -2e6, -100e6, -1e6],
            [50_000.0, -2000.0, 1000.0, -2e6, -54e6, -3e6],
            [50_000.0, 2000.0, -1000.0, 3e6, -50e6, 2e6],
        ];
        let r = response(&m, f, &[0.0; 24], SeismicDir::X);
        close3(r.top.moment_nmm, [1e6, -108e6, -9e6]);
        close3(r.bottom.moment_nmm, [-1e6, -192e6, 9e6]);
        close3(r.equilibrium_residual.force_n, [0.0; 3]);
        close3(r.equilibrium_residual.moment_nmm, [0.0; 3]);
        let reverse = f.map(|row| row.map(|v| -v));
        let rr = response(&m, reverse, &[0.0; 24], SeismicDir::X);
        close(rr.qw_n, -100_000.0);
        close3(rr.top.moment_nmm, [-1e6, 108e6, 9e6]);
        assert_eq!(
            r.virtual_column.unwrap().at[0],
            (0.0, [-123.0, 456.0, 0.0, 0.0, 0.0, 789.0])
        );
    }

    #[test]
    fn normalized_input_orders_and_xy_rotation_preserve_physical_forces() {
        let m = model();
        let f = [
            [-50_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [-50_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [50_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [50_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ];
        for order in [[0, 1, 2, 3], [2, 3, 0, 1], [1, 0, 3, 2], [3, 1, 2, 0]] {
            let mut m = m.clone();
            m.elements[0].nodes = order.map(NodeId).into_iter().collect();
            let r = response(&m, f, &[0.0; 24], SeismicDir::X);
            close(r.qdir_n, 100_000.0);
            close(r.qw_n.abs(), 100_000.0);
            close(r.qw_n.signum(), r.inplane_axis[0]);
            close(r.top.at_origin(r.bottom.origin_mm).moment_nmm[1], 300e6);
            for node in &mut m.nodes {
                let [x, y, z] = node.coord;
                node.coord = [-y, x, z];
            }
            let rotated = f.map(|[x, y, z, mx, my, mz]| [-y, x, z, -my, mx, mz]);
            let rr = response(&m, rotated, &[0.0; 24], SeismicDir::Y);
            close(rr.qdir_n, 100_000.0);
            close(rr.qw_n, r.qw_n);
            close(rr.top.at_origin(rr.bottom.origin_mm).moment_nmm[0], -300e6);
            close(response(&m, rotated, &[0.0; 24], SeismicDir::X).qdir_n, 0.0);
        }
    }

    #[test]
    fn chord_rotation_distinguishes_translation_rotation_and_unrecovered_material_shear() {
        let m = model();
        let mut u = [0.0; 24];
        for i in 0..4 {
            u[i * 6] = 12.0;
            u[i * 6 + 1] = 8.0;
        }
        close(
            response(&m, [[0.0; 6]; 4], &u, SeismicDir::X).chord_rotation_rad,
            0.0,
        );
        for i in [2, 3] {
            u[i * 6] += 30.0;
        }
        let r = response(&m, [[0.0; 6]; 4], &u, SeismicDir::X);
        close(r.delta_wall_mm, 30.0);
        close(r.chord_rotation_rad, 0.01);
        for (i, node) in m.nodes.iter().enumerate() {
            u[i * 6] = 0.01 * node.coord[2];
            u[i * 6 + 2] = -0.01 * node.coord[0];
            u[i * 6 + 4] = 0.01;
        }
        let r = response(&m, [[0.0; 6]; 4], &u, SeismicDir::X);
        close(r.chord_rotation_rad, 0.01);
        assert_eq!(r.material_shear_strain, None);
        assert_eq!(
            r.material_shear_unavailable,
            Some(WallUnavailableReason::MaterialShearNotRecovered)
        );
        assert!(r.line_events.is_none());
        assert_eq!(
            r.line_events_unavailable,
            Some(WallUnavailableReason::LineEventsNotApplicable)
        );
    }

    #[test]
    fn actual_elastic_wall_column_nm_follow_ea_and_constant_curvature_and_commit_history() {
        let m = model();
        let map = DofMap::build(&m);
        let ctx = Ctx { model: &m };
        let element =
            sepika_element::wall::wall_element::WallElement::try_new(&m.elements[0], &m).unwrap();
        let mut b: Vec<Box<dyn ElementBehavior>> = vec![Box::new(element)];
        let mut u = [0.0; 24];
        u[2] = 0.2;
        u[8] = -0.2;
        u[14] = 0.5;
        u[20] = 0.1;
        let gdofs = b[0].global_dofs(&map);
        let delta = LocalVec {
            data: gdofs.iter().map(|&a| u[a]).collect(),
        };
        b[0].update_state(&delta, false, &ctx);
        b[0].commit_state();
        let mut recorder = super::super::step_record::StepRecorder::new(
            &m,
            SeismicDir::X,
            &[3000.0],
            super::super::types::DuctilityMethod::default(),
        );
        recorder.record(0.0, &m, &map, &b, &u);
        b[0].update_state(&delta, false, &ctx);
        b[0].commit_state();
        recorder.record(1.0, &m, &map, &b, &u.map(|v| 2.0 * v));
        let recorded = recorder.finish();
        let first = recorded.wall_history[0].response.as_ref().unwrap();
        let second = recorded.wall_history[1].response.as_ref().unwrap();
        // EA=23,000·150·4,000 + (205,000−23,000)·0.0025·150·4,000。
        // N=EA·0.3/3000、一定曲率M=−2·(EA·4000²/12)·0.0001/3000。
        for (_, f) in &first.virtual_column.as_ref().unwrap().at {
            close(f[0], 1_407_300.0);
            close(f[1], 0.0);
            assert!((f[5] + 1_250_933_333.3333333).abs() < 1e-4);
        }
        for (_, f) in &second.virtual_column.as_ref().unwrap().at {
            close(f[0], 2_814_600.0);
            assert!((f[5] + 2_501_866_666.6666665).abs() < 1e-4);
        }
        close(first.qw_n, 0.0);
        close3(first.equilibrium_residual.force_n, [0.0; 3]);
        close3(first.equilibrium_residual.moment_nmm, [0.0; 3]);
        assert_eq!(recorded.wall_history[0].step, 0);
        assert_eq!(recorded.wall_history[1].step, 1);
    }

    #[test]
    fn directional_capacity_indices_follow_tension_side_when_bottom_axis_reverses() {
        let mut m = super::super::tests::wall_story_model_with(4000.0, 100_000.0);
        if let Some(sepika_core::section_shape::SectionShape::RcColumnRect { rebar, .. }) =
            &mut m.sections[1].shape
        {
            rebar.x = vec![4];
            rebar.y = vec![3];
        }
        let mut strong = m.sections[1].clone();
        strong.id = sepika_core::ids::SectionId(2);
        if let Some(sepika_core::section_shape::SectionShape::RcColumnRect { rebar, .. }) =
            &mut strong.shape
        {
            for count in rebar.x.iter_mut().chain(&mut rebar.y) {
                *count *= 2;
            }
        }
        m.sections.push(strong);
        m.elements[4].section = Some(sepika_core::ids::SectionId(2));
        let capacities =
            sepika_element::wall::wall_element::WallElement::directional_shear_capacity_of(
                &m.elements[0],
                &m,
            );
        assert!(
            capacities[1] > capacities[0] && capacities[0] > 0.0,
            "{capacities:?}"
        );
        m.elements[0].nodes.swap(0, 1);
        let reversed =
            sepika_element::wall::wall_element::WallElement::directional_shear_capacity_of(
                &m.elements[0],
                &m,
            );
        assert_eq!(capacities, [reversed[1], reversed[0]]);
        for n in &mut m.nodes {
            let [x, y, z] = n.coord;
            n.coord = [-y, x, z];
        }
        let rotated =
            sepika_element::wall::wall_element::WallElement::directional_shear_capacity_of(
                &m.elements[0],
                &m,
            );
        assert_eq!(reversed, rotated);
    }

    #[test]
    fn skew_plan_wall_has_distinct_local_and_load_direction_shear() {
        let mut m = model();
        let c = std::f64::consts::FRAC_1_SQRT_2;
        for node in &mut m.nodes {
            let [x, y, z] = node.coord;
            node.coord = [c * (x - y), c * (x + y), z];
        }
        let f = [
            [-50_000.0 * c, -50_000.0 * c, 0.0, 0.0, 0.0, 0.0],
            [-50_000.0 * c, -50_000.0 * c, 0.0, 0.0, 0.0, 0.0],
            [50_000.0 * c, 50_000.0 * c, 0.0, 0.0, 0.0, 0.0],
            [50_000.0 * c, 50_000.0 * c, 0.0, 0.0, 0.0, 0.0],
        ];
        let r = response(&m, f, &[0.0; 24], SeismicDir::X);
        close(r.qw_n, 100_000.0);
        close(r.qdir_n, 100_000.0 * c);
        assert!(r.qdir_n < r.qw_n);
    }

    #[test]
    fn unsupported_invalid_missing_and_real_zero_are_distinct() {
        let mut m = model();
        let map = DofMap::build(&m);
        assert_eq!(
            record_wall_step(&m, &map, &[], &[0.0; 24], SeismicDir::X, 0)[0].unavailable,
            Some(WallUnavailableReason::MissingBehavior)
        );
        let g = wall_element_geometry(&m.elements[0], &m).unwrap();
        let nodes = [g.bottom[0], g.bottom[1], g.top[0], g.top[1]];
        let b: Vec<Box<dyn ElementBehavior>> = vec![Box::new(PrescribedWall {
            nodes,
            forces: vec![0.0; 24],
        })];
        let r = record_wall_step(&m, &map, &b, &[0.0; 24], SeismicDir::X, 0).remove(0);
        assert!(r.response.is_some());
        assert_eq!(r.unavailable, None);
        let mut short = vec![0.0; 24];
        short[0] = f64::NAN;
        let bad: Vec<Box<dyn ElementBehavior>> = vec![Box::new(PrescribedWall {
            nodes,
            forces: short,
        })];
        assert_eq!(
            record_wall_step(&m, &map, &bad, &[0.0; 24], SeismicDir::X, 0)[0].unavailable,
            Some(WallUnavailableReason::InvalidForce)
        );
        assert_eq!(
            record_wall_step(&m, &map, &b, &[], SeismicDir::X, 0)[0].unavailable,
            Some(WallUnavailableReason::InvalidDisplacement)
        );
        m.nodes[2].coord[0] += 100.0;
        assert_eq!(
            record_wall_step(&m, &map, &b, &[0.0; 24], SeismicDir::X, 0)[0].unavailable,
            Some(WallUnavailableReason::UnsupportedGeometry)
        );
        m.nodes[2].coord[2] = 0.0;
        m.nodes[3].coord[2] = 0.0;
        assert!(
            record_wall_step(&m, &map, &b, &[0.0; 24], SeismicDir::X, 0)[0]
                .response
                .is_none()
        );
    }
}
