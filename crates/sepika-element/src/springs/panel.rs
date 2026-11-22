//! 仕口パネル（柱梁接合部パネル）要素。
//!
//! 柱の強軸・弱軸に対応するせん断ばねを持つ。
//! 変形を柱の曲げ回転軸へ写し、応力は同じ変換の転置で全体座標へ戻す。
//!
//! `{γX, γY}` は全体系 X/Y 基底を柱直交面へ投影する前の係数で、
//! 節点の標準 6 自由度とは別枠の追加自由度である。
//! グローバル自由度空間の末尾へ払い出される。本要素はその 2 自由度に対して
//! のみ剛性を与える。

use crate::behavior::{
    Ctx, ElementBehavior, ElementUpdateError, LocalMat, LocalVec, MassOption, PanelColumnSection,
};
use sepika_core::dof::DofMap;
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, Model};
use sepika_core::panel_zone::{resolve_explicit_panel_analysis_joint, PanelGeometry};
use smallvec::SmallVec;

mod nonlinear;
use nonlinear::{NonlinearPanel, NonlinearPanelState};

/// パネル諸元を解決できなかった場合に用いる剛性 [N·mm/rad]。
const PANEL_RIGID_STIFFNESS: f64 = 1.0e14;

/// 仕口パネル要素。方向別の諸元・応答は `[強軸, 弱軸]` の順。
pub struct PanelZone {
    /// 接合部の節点（追加自由度 `γX`・`γY` の持ち主）。
    pub node: NodeId,
    /// 諸元未算定の理由。剛パネルへの数値的代用を算定成功と扱わない。
    pub resolution_error: Option<String>,
    /// 方向別パネル寸法 `dc` `[mm]`。H 形の弱軸はフランジ幅。
    pub dc: [f64; 2],
    /// 梁フランジ板厚中心間距離 `db` `[mm]`。
    pub db: [f64; 2],
    /// 抵抗板 1 枚あたりの有効厚 `tp` `[mm]`。
    pub tp: [f64; 2],
    /// 降伏耐力の算定に用いる実効体積 `Ve` `[mm³]`。
    pub ve: [f64; 2],
    /// パネルの形状係数 κ。
    pub kappa: [f64; 2],
    /// せん断弾性係数 `G` [N/mm²]。
    pub g: f64,
    /// パネルの基準強度 `F` [N/mm²]。
    pub fy: [f64; 2],
    /// 柱の強軸・弱軸に対応する初期せん断剛性 [N·mm/rad]。
    pub k_panel: [f64; 2],
    /// 柱の強軸・弱軸の順に並ぶ、全体座標系での曲げ回転軸。
    pub axes: [[f64; 3]; 2],
    /// 確定・トライアルの投影前係数 `{γX, γY}`。
    committed_disp: [f64; 2],
    trial_disp: [f64; 2],
    nonlinear: Option<NonlinearPanel>,
}

type PanelState = ([f64; 2], [f64; 2], Option<NonlinearPanelState>);

/// パネルに取り付く柱・梁から解決した諸元。
struct ResolvedPanel {
    geom: [PanelGeometry; 2],
    db: f64,
    ve: [f64; 2],
    k_panel: [f64; 2],
    g: f64,
    fy: [f64; 2],
    axes: [[f64; 3]; 2],
}

/// 接合部節点 `node` に取り付く柱・梁からパネル諸元を解決する。
fn resolve(model: &Model, node: NodeId) -> Result<Option<ResolvedPanel>, String> {
    let Some(joint) = resolve_explicit_panel_analysis_joint(model, node, &model.elements)
        .map_err(|reason| reason.to_string())?
    else {
        return Ok(None);
    };
    Ok(Some(ResolvedPanel {
        geom: joint.panels.map(|panel| panel.geometry),
        db: joint.db,
        ve: joint.panels.map(|panel| panel.ve),
        k_panel: joint.panels.map(|panel| panel.stiffness_n_mm),
        g: joint.g,
        fy: joint.panels.map(|panel| panel.fy),
        axes: joint.moment_axes,
    }))
}

impl PanelZone {
    pub(crate) fn nonlinear_issue(data: &ElementData, model: &Model) -> Option<String> {
        let Some(&node) = data.nodes.first() else {
            return Some(format!(
                "仕口パネル要素 ID {} に節点がありません。",
                data.id.0
            ));
        };
        match resolve_explicit_panel_analysis_joint(model, node, &model.elements) {
            Err(reason) => Some(reason.to_string()),
            Ok(Some(joint)) => NonlinearPanel::new(model, node, joint).err(),
            Ok(None) => None,
        }
    }

    /// 線形解析用の弾性パネルを生成する（追加自由度 2 個のみを持つ）。
    pub fn new(data: &ElementData, model: &Model) -> Self {
        let node = data.nodes.first().copied().unwrap_or(NodeId(0));
        let (resolved, resolution_error) = match resolve(model, node) {
            Ok(resolved) => (resolved, None),
            Err(reason) => (None, Some(reason)),
        };

        let (dc, tp, db, ve, kappa, g, fy) = match &resolved {
            Some(r) => (
                r.geom.map(|geometry| geometry.dc),
                r.geom.map(|geometry| geometry.tp),
                [r.db; 2],
                r.ve,
                r.geom.map(|geometry| geometry.kappa()),
                r.g,
                r.fy,
            ),
            None => (
                [0.0; 2], [0.0; 2], [0.0; 2], [0.0; 2], [1.0; 2], 0.0, [0.0; 2],
            ),
        };
        let k_panel = match &resolved {
            Some(r) => r.k_panel,
            None if resolution_error.is_some() => [0.0; 2],
            None => [PANEL_RIGID_STIFFNESS; 2],
        };
        Self {
            node,
            resolution_error,
            dc,
            db,
            tp,
            ve,
            kappa,
            g,
            fy,
            k_panel,
            axes: resolved
                .as_ref()
                .map_or([[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], |resolved| {
                    resolved.axes
                }),
            committed_disp: [0.0; 2],
            trial_disp: [0.0; 2],
            nonlinear: None,
        }
    }

    pub(crate) fn new_nonlinear(data: &ElementData, model: &Model) -> Self {
        let mut panel = Self::new(data, model);
        match resolve_explicit_panel_analysis_joint(model, panel.node, &model.elements) {
            Ok(Some(joint)) => match NonlinearPanel::new(model, panel.node, joint) {
                Ok(state) => {
                    panel.axes = state.axes;
                    panel.nonlinear = Some(state);
                }
                Err(error) => panel.resolution_error = Some(error),
            },
            Ok(None) => {}
            Err(error) => panel.resolution_error = Some(error.to_string()),
        }
        panel
    }

    fn state_data(&self) -> PanelState {
        (
            self.committed_disp,
            self.trial_disp,
            self.nonlinear.as_ref().map(|panel| panel.state.clone()),
        )
    }

    fn to_panel_frame(&self, gamma: [f64; 2]) -> [f64; 2] {
        self.axes
            .map(|axis| axis[0] * gamma[0] + axis[1] * gamma[1])
    }

    fn to_node_frame(&self, moment: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| self.axes[0][i] * moment[0] + self.axes[1][i] * moment[1])
    }

    /// 全体 X/Y 軸まわりのパネルせん断モーメント `{MSX, MSY}` [N·mm]。
    pub fn panel_moments(&self) -> [f64; 2] {
        self.nonlinear.as_ref().map_or_else(
            || self.panel_moments_at(self.trial_disp),
            |panel| self.to_node_frame(panel.state.trial.moment),
        )
    }

    /// 投影前の全体系基底係数に対する、全体 X/Y 軸まわりのせん断モーメント [N·mm]。
    pub fn panel_moments_at(&self, gamma: [f64; 2]) -> [f64; 2] {
        let gp = self.to_panel_frame(gamma);
        self.to_node_frame(std::array::from_fn(|i| self.k_panel[i] * gp[i]))
    }
}

impl ElementBehavior for PanelZone {
    fn n_dof(&self) -> usize {
        2
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        let mut gdofs = SmallVec::new();
        crate::behavior::push_panel_global_dofs(&mut gdofs, self.node.index(), dof);
        gdofs
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        let t = self.axes;
        let stiffness = self
            .nonlinear
            .as_ref()
            .map_or(self.k_panel, |panel| panel.state.trial.tangent);
        let mut k = LocalMat::zeros(self.n_dof());
        for i in 0..2 {
            for j in 0..2 {
                let mut v = 0.0;
                for p in 0..2 {
                    v += t[p][i] * stiffness[p] * t[p][j];
                }
                k.set(i, j, v);
            }
        }
        k
    }

    fn internal_force(&self, _ctx: &Ctx) -> LocalVec {
        let ms = self.panel_moments();
        let mut f = LocalVec {
            data: SmallVec::from_elem(0.0, self.n_dof()),
        };
        f.data[0] = ms[0];
        f.data[1] = ms[1];
        f
    }

    fn update_state(
        &mut self,
        du: &LocalVec,
        commit: bool,
        _ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        if let Some(reason) = &self.resolution_error {
            return Err(crate::behavior::ElementUpdateError(reason.clone()));
        }
        if self.nonlinear.is_some() {
            return Err(crate::behavior::ElementUpdateError(format!(
                "接合部 ID {} の仕口パネルには同一試行の上下柱軸力が必要です",
                self.node.0
            )));
        }
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let mut trial_disp = self.trial_disp;
        for (i, displacement) in trial_disp.iter_mut().enumerate() {
            *displacement += du.data[i];
        }
        if !trial_disp.iter().all(|value| value.is_finite()) {
            return Err(crate::behavior::ElementUpdateError(format!(
                "接合部 ID {} のパネル変形角が非有限です",
                self.node.0
            )));
        }
        let gp = self.to_panel_frame(trial_disp);
        if !gp.iter().all(|value| value.is_finite()) {
            return Err(crate::behavior::ElementUpdateError(format!(
                "接合部 ID {} の柱主軸パネル変形角が非有限です",
                self.node.0
            )));
        }
        self.trial_disp = trial_disp;
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn panel_column_sections(&self) -> Option<[PanelColumnSection; 2]> {
        self.nonlinear.as_ref().map(|panel| panel.columns)
    }

    fn update_panel_state(
        &mut self,
        du: &LocalVec,
        axial_n: f64,
        axial_gradient: Vec<(usize, f64)>,
    ) -> Result<(), ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let trial_disp = std::array::from_fn(|index| self.trial_disp[index] + du.data[index]);
        let panel = self.nonlinear.as_mut().ok_or_else(|| {
            ElementUpdateError(format!(
                "接合部 ID {} の非線形仕口パネルが未設定です",
                self.node.0
            ))
        })?;
        panel.update(self.node, trial_disp, axial_n, axial_gradient)?;
        self.trial_disp = trial_disp;
        Ok(())
    }

    fn panel_coupled_tangent(&self, dof: &DofMap) -> Vec<sepika_math::sparse::Triplet> {
        let Some(panel) = &self.nonlinear else {
            return Vec::new();
        };
        let trial = &panel.state.trial;
        let local_slope = self.to_node_frame(trial.axial_slope);
        let mut tangent = Vec::new();
        for (row, slope) in self.global_dofs(dof).into_iter().zip(local_slope) {
            if row != usize::MAX && slope != 0.0 {
                tangent.extend(trial.axial_gradient.iter().map(|&(col, axial_derivative)| {
                    sepika_math::sparse::Triplet {
                        row,
                        col,
                        val: slope * axial_derivative,
                    }
                }));
            }
        }
        tangent
    }

    fn commit_state(&mut self) {
        self.committed_disp = self.trial_disp;
        if let Some(panel) = &mut self.nonlinear {
            panel.state.commit();
        }
    }

    fn revert_state(&mut self) {
        self.trial_disp = self.committed_disp;
        if let Some(panel) = &mut self.nonlinear {
            panel.state.revert();
        }
    }

    fn snapshot_state(&self) -> Box<dyn std::any::Any> {
        Box::new(self.state_data())
    }

    fn restore_state(&mut self, state: &dyn std::any::Any) {
        let (committed, trial, nonlinear) =
            crate::behavior::downcast_snapshot::<PanelState>("PanelZone", state);
        self.committed_disp = *committed;
        self.trial_disp = *trial;
        if let Some(panel) = &mut self.nonlinear {
            panel.state = nonlinear.as_ref().expect("非線形仕口パネル状態").clone();
        }
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        bincode::serialize(&self.state_data()).expect("serialize checkpoint")
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        if data.is_empty() {
            return Ok(());
        }
        let (committed, trial, nonlinear): PanelState = bincode::deserialize(data)
            .map_err(|e| crate::behavior::CheckpointError::Decode(e.to_string()))?;
        match (&mut self.nonlinear, nonlinear) {
            (Some(panel), Some(state)) => panel.state = state,
            (None, None) => {}
            _ => {
                return Err(crate::behavior::CheckpointError::Decode(
                    "仕口パネル状態と要素形式が不一致".into(),
                ))
            }
        }
        self.committed_disp = committed;
        self.trial_disp = trial;
        Ok(())
    }

    fn mass_matrix(&self, _opt: MassOption) -> LocalMat {
        LocalMat::zeros(self.n_dof())
    }

    fn panel_moments_from(&self, u_elem: &[f64]) -> Option<[f64; 2]> {
        let gamma = [
            u_elem.first().copied().unwrap_or(0.0),
            u_elem.get(1).copied().unwrap_or(0.0),
        ];
        Some(self.panel_moments_at(gamma))
    }
}

/// 接合部に取り付く部材の材端応力（フェイスモーメント算定の入力）。
pub struct PanelConnection {
    pub ml_b: f64,
    pub mr_b: f64,
    pub bql: f64,
    pub bqr: f64,
    pub bnl: f64,
    pub bnr: f64,
    pub ml_c: f64,
    pub mu_c: f64,
    pub cql: f64,
    pub cqu: f64,
}

/// フェイスモーメントとパネルせん断の算定結果。
pub struct PanelResult {
    pub b_ml: f64,
    pub b_mr: f64,
    pub c_ml: f64,
    pub c_mu: f64,
    pub pqc: f64,
    pub pqb: f64,
    pub tau: f64,
}

/// 材端応力からフェイスモーメント（柱面・梁面へ換算したモーメント）と
/// パネルせん断・せん断応力度を算定する。
///
/// ```text
/// bml = ml_b − bql・dc/2
/// pQc = ((bml + bmr) − (cql + cqu)・db/2) / db
/// pQb = ((cmu + cml) − (bql + bqr)・dc/2) / dc
/// τ   = pQc / (dc・tp)
/// ```
///
/// 節点のモーメント釣り合い `ml_b + mr_b = ml_c + mu_c` が成立するとき、
/// 整合条件 `pQc・db = pQb・dc` が自動的に満たされる。
pub fn face_moments(dc: f64, db: f64, tp: f64, conn: &PanelConnection) -> PanelResult {
    let dc2 = dc / 2.0;
    let db2 = db / 2.0;

    let b_ml = conn.ml_b - conn.bql * dc2;
    let b_mr = conn.mr_b - conn.bqr * dc2;
    let c_ml = conn.ml_c - conn.cql * db2;
    let c_mu = conn.mu_c - conn.cqu * db2;

    let pqc = ((b_ml + b_mr) - (conn.cql + conn.cqu) * db2) / db;
    let pqb = ((c_mu + c_ml) - (conn.bql + conn.bqr) * dc2) / dc;
    let tau = if tp > 0.0 { pqc / (dc * tp) } else { 0.0 };
    PanelResult {
        b_ml,
        b_mr,
        c_ml,
        c_mu,
        pqc,
        pqb,
        tau,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, SectionId};
    use sepika_core::model::{
        ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material,
        MaterialCategory, Node, PropertyBasis, Section,
    };
    use sepika_core::panel_zone::{panel_column_moment_axes, resolve_panel_joint};
    use sepika_core::section_shape::SectionShape;

    /// 資料ケース1 の数値例照合。単位系は資料に合わせ kN, m, kN·m。
    /// 確定値: pQc = 851.135 kN, pQb = 1702.273 kN, τc = τb（整合）。
    #[test]
    fn test_face_moments_reference_case1() {
        let (dc, db, tp) = (0.2_f64, 0.4_f64, 0.02_f64);
        let conn = PanelConnection {
            ml_b: 218.182,
            mr_b: 181.818,
            bql: 72.727,
            bqr: 72.727,
            bnl: 0.0,
            bnr: 0.0,
            ml_c: 150.0,
            mu_c: 250.0,
            cql: 100.0,
            cqu: 125.0,
        };
        let res = face_moments(dc, db, tp, &conn);
        assert!((res.b_ml - 210.909).abs() < 1e-3, "bML={}", res.b_ml);
        assert!((res.b_mr - 174.545).abs() < 1e-3, "bMR={}", res.b_mr);
        assert!((res.c_ml - 130.0).abs() < 1e-9, "cML={}", res.c_ml);
        assert!((res.c_mu - 225.0).abs() < 1e-9, "cMU={}", res.c_mu);
        assert!((res.pqc - 851.135).abs() < 0.05, "pQc={}", res.pqc);
        assert!((res.pqb - 1702.273).abs() < 0.05, "pQb={}", res.pqb);
        let tau_b = res.pqb / (db * tp);
        assert!((res.tau - tau_b).abs() / res.tau.abs() < 1e-4);
    }

    /// 資料ケース2（ト型＝梁が片側のみ）。欠落部材の項を 0 として同一式で計算できる。
    /// 確定値: pQc=854.168 kN, pQb=1708.334 kN。
    #[test]
    fn test_face_moments_reference_case2_t_joint() {
        let (dc, db, tp) = (0.2_f64, 0.4_f64, 1.0_f64);
        let conn = PanelConnection {
            ml_b: 400.0,
            mr_b: 0.0,
            bql: 133.333,
            bqr: 0.0,
            bnl: 0.0,
            bnr: 0.0,
            ml_c: 150.0,
            mu_c: 250.0,
            cql: 100.0,
            cqu: 125.0,
        };
        let res = face_moments(dc, db, tp, &conn);
        assert!((res.pqc - 854.168).abs() < 0.05, "pQc={}", res.pqc);
        assert!((res.pqb - 1708.334).abs() < 0.05, "pQb={}", res.pqb);
    }

    /// 十字型の S 造接合部モデル（柱: H-400×400×13×21、梁: H-600×200×11×17）。
    /// 節点 0 が接合部、1/2 が梁の遠端、3/4 が柱の遠端。
    fn cross_joint_model() -> (Model, ElementData) {
        let node = |id: u32, coord: [f64; 3]| Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let section = |id: u32, shape: SectionShape, depth: f64, area: f64| Section {
            frame_use: Some(if id == 0 {
                FrameSectionUse::Girder
            } else {
                FrameSectionUse::Column
            }),
            id: SectionId(id),
            name: String::new(),
            area,
            iy: 1.0e8,
            iz: 1.0e8,
            j: 1.0e8,
            depth,
            width: depth,
            as_y: 0.0,
            as_z: 0.0,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: Some(shape),
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        };
        let member = |id: u32, n0: u32, n1: u32, sec: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
            section: Some(SectionId(sec)),
            local_axis: LocalAxis {
                ref_vector: if sec == 0 {
                    [0.0, 0.0, 1.0]
                } else {
                    [0.0, 1.0, 0.0]
                },
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        };

        let model = Model {
            nodes: vec![
                node(0, [0.0, 0.0, 3000.0]),
                node(1, [0.0, -5000.0, 3000.0]),
                node(2, [0.0, 5000.0, 3000.0]),
                node(3, [0.0, 0.0, 0.0]),
                node(4, [0.0, 0.0, 6000.0]),
            ],
            sections: vec![
                section(
                    0,
                    SectionShape::SteelH {
                        root_r: Some(0.0),
                        height: 600.0,
                        width: 200.0,
                        web_thick: 11.0,
                        flange_thick: 17.0,
                    },
                    600.0,
                    1.0e4,
                ),
                section(
                    1,
                    SectionShape::SteelH {
                        root_r: Some(0.0),
                        height: 400.0,
                        width: 400.0,
                        web_thick: 13.0,
                        flange_thick: 21.0,
                    },
                    400.0,
                    2.187e4,
                ),
            ],
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "SN400B".into(),
                category: MaterialCategory::Steel,
                young: 205_000.0,
                poisson: 0.3,
                density: 0.0,
                shear: None,
                fc: None,
                fy: None,
            }],
            elements: vec![
                member(0, 1, 0, 0),
                member(1, 0, 2, 0),
                member(2, 3, 0, 1),
                member(3, 0, 4, 1),
            ],
            ..Default::default()
        };

        let panel = ElementData {
            id: ElemId(10),
            kind: ElementKind::PanelZone,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3), NodeId(4)],
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        };
        (model, panel)
    }

    #[test]
    fn test_linear_panel_geometry_stiffness_and_trial_moments() {
        let (model, data) = cross_joint_model();
        let mut pz = PanelZone::new(&data, &model);
        assert!(
            (pz.dc[0] - (400.0 - 21.0)).abs() < 1e-9,
            "dc は柱: {}",
            pz.dc[0]
        );
        assert!(
            (pz.db[0] - (600.0 - 17.0)).abs() < 1e-9,
            "db は梁: {}",
            pz.db[0]
        );
        assert!(
            (pz.tp[0] - 13.0).abs() < 1e-9,
            "tp は柱ウェブ厚: {}",
            pz.tp[0]
        );
        assert!((pz.ve[0] - pz.dc[0] * pz.db[0] * pz.tp[0]).abs() / pz.ve[0] < 1e-12);
        let ctx = Ctx { model: &model };
        assert_eq!(pz.n_dof(), 2, "弾性パネルは 2 自由度");

        let k = pz.tangent_stiffness(&ctx);
        let expected = pz.g * pz.ve[0];
        assert!((k.get(0, 0) - expected).abs() / expected < 1e-12);
        let weak_expected = pz.g * pz.ve[1];
        assert!((k.get(1, 1) - weak_expected).abs() / weak_expected < 1e-12);
        assert!(
            k.get(0, 1).abs() < 1e-6,
            "柱主軸と全体軸が一致すると非対角は 0"
        );

        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, 2),
        };
        du.data[0] = 1.0e-4;
        pz.update_state(&du, false, &ctx).unwrap();

        let f = pz.internal_force(&ctx);
        assert!((f.data[0] - expected * 1.0e-4).abs() / (expected * 1.0e-4) < 1e-12);
        assert!(f.data[1].abs() < 1e-6);
        assert!((pz.panel_moments()[0] - f.data[0]).abs() < 1e-6);
    }

    fn round_steel_cross_joint_model() -> (Model, ElementData) {
        let (mut model, data) = cross_joint_model();
        model.sections[0].shape = Some(SectionShape::SteelH {
            root_r: Some(0.0),
            height: 517.0,
            width: 200.0,
            web_thick: 11.0,
            flange_thick: 17.0,
        });
        model.sections[0].depth = 517.0;
        model.sections[1].shape = Some(SectionShape::SteelPipe {
            outer_dia: 400.0,
            thick: 16.0,
        });
        model.materials[0].shear = Some(78_500.0);
        (model, data)
    }

    fn square_steel_cross_joint_model() -> (Model, ElementData) {
        let (mut model, data) = cross_joint_model();
        let column = &mut model.sections[1];
        column.shape = Some(SectionShape::SteelBox {
            height: 400.0,
            width: 400.0,
            thick: 16.0,
            horizontal_thick: 16.0,
            corner_r: None,
        });
        column.area = 23_700.0;
        column.property_basis.area = PropertyBasis::Supplied;
        model.materials[0].name = "BCR295".into();
        model.materials[0].shear = Some(78_500.0);
        (model, data)
    }

    #[test]
    fn standard_panel_uses_full_plastic_moment_and_restores_coupled_history() {
        let (mut model, data) = square_steel_cross_joint_model();
        crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        let dof = DofMap::build(&model);
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let [lower, upper] = panel.panel_column_sections().unwrap();
        assert_eq!((lower.element, upper.element), (ElemId(2), ElemId(3)));
        assert_eq!([lower.position_mm, upper.position_mm], [2708.5, 291.5]);
        let axes = panel.axes[0];
        let increment = |gamma| LocalVec {
            data: SmallVec::from_slice(&[gamma * axes[0], gamma * axes[1]]),
        };
        let gamma = 0.01;
        let gradient = dof.active(0).unwrap() as usize;
        let axial_capacity = 295.0 * 23_700.0;
        let applied_axial = -0.8 * axial_capacity;
        let k = 542_321_175_000.0;
        let expected_p_mp = 976_115_329.1;
        panel
            .update_panel_state(&increment(gamma), applied_axial, vec![(gradient, 1.0)])
            .unwrap();
        let node_moment = panel.panel_moments();
        let moment = axes[0] * node_moment[0] + axes[1] * node_moment[1];
        let expected = expected_p_mp + 0.01 * (k * gamma - expected_p_mp);
        assert!((moment - expected).abs() < 1.0, "{moment} vs {expected}");
        let principal = usize::from(axes[1].abs() > axes[0].abs());
        assert!((panel.tangent_stiffness(&ctx).get(principal, principal) - 0.01 * k).abs() < 1e-5);
        let tangent = panel.panel_coupled_tangent(&dof);
        assert_eq!(tangent.len(), 1);
        let mut perturbed = PanelZone::new_nonlinear(&data, &model);
        perturbed
            .update_panel_state(&increment(gamma), applied_axial + 1.0, vec![])
            .unwrap();
        assert!(
            (perturbed.panel_moments()[principal] - node_moment[principal] - tangent[0].val).abs()
                < 0.001
        );

        panel.commit_state();
        let checkpoint = panel.serialize_checkpoint();
        assert!(panel
            .update_panel_state(&increment(-0.04), axial_capacity, vec![])
            .is_err());
        assert_eq!(panel.serialize_checkpoint(), checkpoint);
        let mut restored = PanelZone::new_nonlinear(&data, &model);
        restored.deserialize_checkpoint(&checkpoint).unwrap();
        for spring in [&mut panel, &mut restored] {
            spring
                .update_panel_state(&increment(-0.04), applied_axial, vec![(gradient, 2.0)])
                .unwrap();
        }
        assert_eq!(restored.panel_moments(), panel.panel_moments());
        assert_eq!(
            restored.serialize_checkpoint(),
            panel.serialize_checkpoint()
        );
        panel.revert_state();
        assert_eq!(panel.panel_moments(), node_moment);
    }

    #[test]
    fn round_panel_response_is_invariant_when_the_loading_plane_rotates() {
        let mut response = Vec::new();
        for angle in [0.0_f64, std::f64::consts::FRAC_PI_4] {
            let (mut model, data) = round_steel_cross_joint_model();
            let (s, c) = angle.sin_cos();
            let rotate = |position: [f64; 3]| {
                [
                    c * position[0] - s * position[1],
                    s * position[0] + c * position[1],
                    position[2],
                ]
            };
            for node in &mut model.nodes {
                node.coord = rotate(node.coord);
            }
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            let active = panel.axes[0];
            let orthogonal = panel.axes[1];
            let increment = |axis: [f64; 3], gamma| LocalVec {
                data: SmallVec::from_slice(&[axis[0] * gamma, axis[1] * gamma]),
            };
            panel
                .update_panel_state(&increment(active, 0.01), -100_000.0, vec![])
                .unwrap();
            let moment = panel.panel_moments();
            response.push(active[0] * moment[0] + active[1] * moment[1]);
            let state = panel.serialize_checkpoint();
            assert!(panel
                .update_panel_state(&increment(orthogonal, 0.001), -100_000.0, vec![])
                .unwrap_err()
                .0
                .contains("直交"));
            assert_eq!(panel.serialize_checkpoint(), state);
        }
        assert!((response[0] - response[1]).abs() < 1e-5, "{response:?}");
    }

    #[test]
    fn diagonal_square_panel_uses_the_rounded_corner_capacity_and_axis() {
        let (mut model, data) = square_steel_cross_joint_model();
        if let Some(SectionShape::SteelBox { corner_r, .. }) = &mut model.sections[1].shape {
            *corner_r = Some(32.0);
        }
        if let Some(SectionShape::SteelH { height, .. }) = &mut model.sections[0].shape {
            *height = 517.0;
        }
        model.sections[0].depth = 517.0;
        for node in &mut model.nodes[1..=2] {
            node.coord[0] = node.coord[1];
        }
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let strain = 0.01;
        let axis = panel.axes[0];
        let increment = LocalVec {
            data: SmallVec::from_slice(&[strain * axis[0], strain * axis[1]]),
        };
        panel
            .update_panel_state(&increment, -0.25 * 295.0 * 23_700.0, vec![])
            .unwrap();
        let first_branch_k = 465_112_500_000.0;
        let plastic_moment = 1_380_430_736.949_147_5;
        let expected = plastic_moment + 0.01 * (first_branch_k * strain - plastic_moment);
        let ms = panel.panel_moments();
        assert!((ms[0].hypot(ms[1]) - expected).abs() < 0.01);
    }

    #[test]
    fn square_steel_panel_matches_reference_example_in_both_elastic_responses() {
        let (mut model, _) = square_steel_cross_joint_model();
        let generated = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        let [generated] = generated.as_slice() else {
            panic!("接合部のパネルが1つ必要です");
        };
        let data = model
            .elements
            .iter()
            .find(|element| element.kind == ElementKind::PanelZone)
            .unwrap();
        let ctx = Ctx { model: &model };
        // 『鋼構造接合部設計指針』例題1の A=23,700 mm²、db=583 mm、G=78,500 N/mm² に式(5.11)を適用。
        let expected_k_n_mm = 542_321_175_000.0;
        for k_n_mm in generated.k_panel {
            assert!((k_n_mm / expected_k_n_mm - 1.0).abs() < 1e-12);
        }
        assert_eq!(generated.db, [583.0; 2]);
        for axis in 0..2 {
            let mut panel = PanelZone::new(data, &model);
            assert_eq!(panel.resolution_error, None);
            let tangent = panel.tangent_stiffness(&ctx);
            assert!((tangent.get(axis, axis) / expected_k_n_mm - 1.0).abs() < 1e-12);
            assert_eq!(tangent.get(axis, 1 - axis), 0.0);
            let mut increment = LocalVec {
                data: SmallVec::from_elem(0.0, panel.n_dof()),
            };
            increment.data[axis] = 0.001;
            panel.update_state(&increment, false, &ctx).unwrap();
            let force = panel.internal_force(&ctx);
            assert!((force.data[axis] / 542_321_175.0 - 1.0).abs() < 1e-12);
            assert_eq!(force.data[1 - axis], 0.0);
        }
    }

    #[test]
    fn square_steel_panel_uses_the_full_shape_area_including_specified_corners() {
        let (mut model, data) = square_steel_cross_joint_model();
        model.sections[1].property_basis.area = PropertyBasis::Shape;
        model.sections[1].panel_thickness = Some(16.0);
        for (radius_mm, expected_k_n_mm) in
            [(40.0, 542_252_317_996.986_2), (0.0, 562_366_464_000.0)]
        {
            model.sections[1] = model.sections[1]
                .with_surface_radius(Some(radius_mm))
                .unwrap();
            model.sections[1].area = 24_576.0;
            let panel = PanelZone::new(&data, &model);
            assert_eq!(panel.resolution_error, None);
            for actual_k_n_mm in panel.k_panel {
                assert!((actual_k_n_mm / expected_k_n_mm - 1.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn square_steel_panel_requires_a_resolved_area_and_the_actual_wall_thickness() {
        let (baseline, data) = square_steel_cross_joint_model();
        for (basis, radius_mm, area_mm2, panel_thickness, expected_reason) in [
            (PropertyBasis::PendingShape, None, 0.0, None, "断面積A"),
            (
                PropertyBasis::PendingShape,
                None,
                23_700.0,
                None,
                "A は未算定",
            ),
            (PropertyBasis::Shape, None, 23_700.0, None, "角Rが未知"),
            (PropertyBasis::Supplied, Some(201.0), 23_700.0, None, "半径"),
            (PropertyBasis::Supplied, None, f64::NAN, None, "断面積A"),
            (
                PropertyBasis::Supplied,
                None,
                23_700.0,
                Some(20.0),
                "実板厚と異なり",
            ),
        ] {
            let mut model = baseline.clone();
            let column = &mut model.sections[1];
            let Some(SectionShape::SteelBox { corner_r, .. }) = &mut column.shape else {
                unreachable!();
            };
            *corner_r = radius_mm;
            column.property_basis.area = basis;
            column.area = area_mm2;
            column.panel_thickness = panel_thickness;
            let reason = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap_err();
            assert!(
                reason.contains("接合部 ID 0 のパネル未算定: 柱 ID 2"),
                "{reason}"
            );
            assert!(
                reason.contains(expected_reason),
                "{expected_reason}: {reason}"
            );
            assert_eq!(PanelZone::new(&data, &model).resolution_error, Some(reason));
        }
    }

    #[test]
    fn square_steel_panel_accepts_a_saved_model_when_the_current_stiffness_is_finite() {
        let (mut model, _) = square_steel_cross_joint_model();
        model.materials[0].shear = Some(2.55e301);
        let generated = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        assert_eq!(generated.len(), 1);
        assert!(generated[0].k_panel.iter().all(|k| k.is_finite()));
        model.validate_for_project_load().unwrap();
    }

    #[test]
    fn round_steel_panel_uses_half_the_annular_area_in_elastic_response() {
        let (model, data) = round_steel_cross_joint_model();
        let ctx = Ctx { model: &model };
        // 外径 400 mm・内径 368 mm の環状断面積から式 (5.11) で計算。
        let k_n_mm = 378_800_675_799.242_9;
        for axis in 0..2 {
            let mut panel = PanelZone::new(&data, &model);
            assert_eq!(panel.resolution_error, None);
            assert_eq!(panel.ve, [6_144_000.0; 2]);
            let initial = panel.tangent_stiffness(&ctx);
            assert!((initial.get(axis, axis) / k_n_mm - 1.0).abs() < 1e-12);
            assert_eq!(initial.get(axis, 1 - axis), 0.0);

            let mut increment = LocalVec {
                data: smallvec::smallvec![0.0, 0.0],
            };
            increment.data[axis] = 0.0017;
            panel.update_state(&increment, true, &ctx).unwrap();
            let force = panel.internal_force(&ctx);
            assert!((force.data[axis] / 643_961_148.858_713 - 1.0).abs() < 1e-12);
            assert_eq!(force.data[1 - axis], 0.0);
        }
    }

    #[test]
    fn round_steel_panel_preparation_and_analysis_use_the_specified_uniform_thickness() {
        let (mut model, data) = round_steel_cross_joint_model();
        model.sections[1].panel_thickness = Some(20.0);
        let panels = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        let generated = panels.iter().find(|panel| panel.node == NodeId(0)).unwrap();
        let panel = PanelZone::new(&data, &model);
        assert_eq!(panel.resolution_error, None);
        assert_eq!(generated.dc, [384.0; 2]);
        assert_eq!(generated.tp, [20.0; 2]);
        assert_eq!(generated.ve, [7_680_000.0; 2]);
        assert_eq!(generated.k_panel, panel.k_panel);
        // 板厚中心径 384 mm のまま外径 404 mm・内径 364 mm とした環状断面積を使用。
        for stiffness in generated.k_panel {
            assert!((stiffness / 473_500_844_749.053_65 - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn round_steel_panel_checks_the_finiteness_of_the_stiffness_it_uses() {
        let (mut model, data) = round_steel_cross_joint_model();
        model.materials[0].shear = Some(3e301);
        let panels = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        let generated = panels.iter().find(|panel| panel.node == NodeId(0)).unwrap();
        let panel = PanelZone::new(&data, &model);
        assert_eq!(panel.resolution_error, None);
        assert_eq!(generated.k_panel, panel.k_panel);
        assert!(panel.k_panel.iter().all(|k| k.is_finite() && *k > 0.0));

        model.materials[0].shear = Some(4e301);
        let result = crate::springs::panel_gen::apply_auto_panel_zones(&mut model);
        let reason = result.unwrap_err();
        assert!(reason.contains("強軸のせん断剛性 G・db・A/2 は有限の正値が必要です"));
        assert_eq!(PanelZone::new(&data, &model).resolution_error, Some(reason));
    }

    #[test]
    fn round_steel_panel_requires_a_positive_inner_diameter() {
        let (mut model, data) = round_steel_cross_joint_model();
        for (thickness, valid) in [(383.0, true), (384.0, false), (385.0, false)] {
            model.sections[1].panel_thickness = Some(thickness);
            let result = crate::springs::panel_gen::apply_auto_panel_zones(&mut model);
            let panel = PanelZone::new(&data, &model);
            if valid {
                assert!(result.is_ok());
                assert_eq!(panel.resolution_error, None);
            } else {
                let reason = result.unwrap_err();
                assert!(reason.contains("接合部 ID 0 のパネル未算定: 柱 ID 2"));
                assert!(reason.contains("有効板厚から求める内径 dc−tp は正値が必要です"));
                assert_eq!(panel.resolution_error, Some(reason));
            }
        }
    }

    #[test]
    fn round_steel_panel_override_cannot_mask_a_nonpositive_pipe_bore() {
        let (mut model, data) = round_steel_cross_joint_model();
        model.sections[1].panel_thickness = Some(16.0);
        for (thick, valid) in [(79.0, true), (80.0, false), (85.0, false)] {
            model.sections[1].shape = Some(SectionShape::SteelPipe {
                outer_dia: 160.0,
                thick,
            });
            let result = crate::springs::panel_gen::apply_auto_panel_zones(&mut model);
            let panel = PanelZone::new(&data, &model);
            if valid {
                assert!(result.is_ok());
                assert_eq!(panel.resolution_error, None);
            } else {
                let reason = result.unwrap_err();
                assert!(reason.contains("接合部 ID 0 のパネル未算定: 柱 ID 2"));
                assert!(reason.contains("実壁厚から求める内径は正値が必要です"));
                assert_eq!(panel.resolution_error, Some(reason));
            }
        }
    }

    #[test]
    fn panel_757_constructor_connects_actual_two_axis_geometry_and_response() {
        let (mut model, data) = cross_joint_model();
        for column in &mut model.elements[2..] {
            column.local_axis.ref_vector = [0.6, 0.8, 0.0];
        }
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new(&data, &model);
        assert_eq!(panel.resolution_error, None);
        assert_eq!(panel.dc, [379.0, 400.0]);
        assert_eq!(panel.db, [583.0; 2]);
        assert_eq!(panel.tp, [13.0, 21.0]);
        assert_eq!(panel.ve, [2_872_441.0, 9_794_400.0]);
        assert_eq!(panel.fy, [235.0; 2]);
        assert_eq!(panel.kappa[1], 1.5);
        for (actual, expected) in panel
            .k_panel
            .into_iter()
            .zip([226_480_925_000.0, 772_250_769_230.769_2])
        {
            assert!((actual / expected - 1.0).abs() < 1e-12);
        }
        let increment = LocalVec {
            data: smallvec::smallvec![0.0001, -0.0002],
        };
        panel.update_state(&increment, false, &ctx).unwrap();
        let expected_matrix = [
            [422_958_068_923.076_9, 261_969_525_230.769_17],
            [261_969_525_230.769_17, 575_773_625_307.692_3],
        ];
        let tangent = panel.tangent_stiffness(&ctx);
        for (i, row) in expected_matrix.iter().enumerate() {
            for (j, expected) in row.iter().enumerate() {
                assert!((tangent.get(i, j) / expected - 1.0).abs() < 1e-12);
            }
        }
        let moments = panel.panel_moments();
        for (actual, expected) in moments
            .into_iter()
            .zip([-10_098_098.153_846_145, -88_957_772.538_461_54])
        {
            assert!((actual / expected - 1.0).abs() < 1e-12);
        }
        let work = increment.data[0] * moments[0] + increment.data[1] * moments[1];
        assert!((work / 16_781.744_692_307_693 - 1.0).abs() < 1e-12);
        let generated = crate::springs::panel_gen::apply_auto_panel_zones(&mut model).unwrap();
        let generated = generated
            .iter()
            .find(|generated| generated.node == NodeId(0))
            .unwrap();
        assert_eq!(generated.dc, panel.dc);
        assert_eq!(generated.db, panel.db);
        assert_eq!(generated.tp, panel.tp);
        assert_eq!(generated.ve, panel.ve);
        assert_eq!(generated.k_panel, panel.k_panel);
    }

    fn panel_757_directional_fixture() -> (Model, PanelZone) {
        let (mut model, data) = cross_joint_model();
        for column in &mut model.elements[2..] {
            column.local_axis.ref_vector = [0.6, 0.8, 0.0];
        }
        let mut panel = PanelZone::new(&data, &model);
        panel.k_panel = [1_000.0, 4_000.0];
        (model, panel)
    }

    #[test]
    fn panel_757_directional_stiffness_and_global_moments_are_work_conjugate() {
        let (model, mut panel) = panel_757_directional_fixture();
        let ctx = Ctx { model: &model };
        assert_eq!(panel.axes, [[-0.8, 0.6, 0.0], [0.6, 0.8, 0.0]]);
        let increment = LocalVec {
            data: smallvec::smallvec![0.001, 0.002],
        };
        panel.update_state(&increment, false, &ctx).unwrap();
        let stiffness = panel.tangent_stiffness(&ctx);
        for (i, expected_row) in [[2_080.0, 1_440.0], [1_440.0, 2_920.0]].iter().enumerate() {
            for (j, expected) in expected_row.iter().enumerate() {
                assert!((stiffness.get(i, j) - expected).abs() < 1e-10);
            }
        }
        let moments = panel.panel_moments();
        for (actual, expected) in moments.into_iter().zip([4.96, 7.28]) {
            assert!((actual - expected).abs() < 1e-12);
        }
        assert_eq!(panel.internal_force(&ctx).data.as_slice(), &moments);
        assert_eq!(panel.panel_moments_from(&increment.data), Some(moments));
        let work = increment.data[0] * moments[0] + increment.data[1] * moments[1];
        assert!((work - 0.01952).abs() < 1e-14);
    }

    #[test]
    fn panel_757_nonfinite_principal_rotation_is_rejected_atomically() {
        let (model, mut panel) = panel_757_directional_fixture();
        let ctx = Ctx { model: &model };
        let before = panel.serialize_checkpoint();
        let increment = LocalVec {
            data: smallvec::smallvec![f64::MAX, f64::MAX],
        };
        let error = panel.update_state(&increment, true, &ctx).unwrap_err();
        assert!(error.to_string().contains("柱主軸パネル変形角が非有限"));
        assert_eq!(panel.serialize_checkpoint(), before);
    }

    #[test]
    fn panel_757_constructor_axes_match_member_frame_including_fallback() {
        let (mut model, data) = cross_joint_model();
        for coordinates in [
            ([0.0, 0.0, 0.0], [0.0, 0.0, 3_000.0]),
            ([1.0, -2.0, 3.0], [101.0, 198.0, 3_003.0]),
        ] {
            model.nodes[3].coord = coordinates.0;
            model.nodes[0].coord = coordinates.1;
            for reference in [[0.6, 0.8, 0.0], [0.0, 0.0, 1.0], [100.0, 200.0, 3_000.0]] {
                model.elements[2].local_axis.ref_vector = reference;
                let column = &model.elements[2];
                let axes = panel_column_moment_axes(&model, column).unwrap();
                let frame = crate::transform::LocalFrame::from_nodes(
                    coordinates.0,
                    coordinates.1,
                    reference,
                )
                .unwrap();
                assert_eq!(axes, [frame.rot[2], frame.rot[1]]);
                if coordinates.0 == [0.0, 0.0, 0.0] {
                    for (node, sign) in [(1, -1.0), (2, 1.0)] {
                        model.nodes[node].coord =
                            std::array::from_fn(|i| coordinates.1[i] + sign * 5000.0 * axes[1][i]);
                    }
                    assert_eq!(PanelZone::new(&data, &model).axes, axes);
                }
            }
        }
    }

    #[test]
    fn invalid_selected_column_is_not_a_rigid_panel() {
        let (mut model, data) = cross_joint_model();
        model.sections[1].shape = None;
        let pz = PanelZone::new(&data, &model);
        assert_eq!(pz.k_panel, [0.0; 2]);
        assert!(pz
            .resolution_error
            .as_ref()
            .unwrap()
            .contains("接合部 ID 0"));
    }
    #[test]
    fn lower_column_supplies_geometry_and_material_after_reordering() {
        for reverse in [false, true] {
            let (mut model, data) = cross_joint_model();
            let mut upper_section = model.sections[1].clone();
            upper_section.id = SectionId(2);
            upper_section.shape = Some(SectionShape::SteelH {
                height: 300.0,
                width: 300.0,
                web_thick: 9.0,
                flange_thick: 16.0,
                root_r: Some(0.0),
            });
            upper_section.material = Some(MaterialId(1));
            model.sections.push(upper_section);
            model.elements[3].section = Some(SectionId(2));
            let mut upper_material = model.materials[0].clone();
            upper_material.id = MaterialId(1);
            upper_material.name = "SN490B".into();
            upper_material.young = 150_000.0;
            model.materials.push(upper_material);
            if reverse {
                for element in &mut model.elements {
                    element.nodes.reverse();
                }
                model.elements.reverse();
                for (i, element) in model.elements.iter_mut().enumerate() {
                    element.id = ElemId(i as u32);
                }
            }
            let p = PanelZone::new(&data, &model);
            assert_eq!(p.resolution_error, None);
            assert_eq!(p.dc[0], 379.0);
            assert_eq!(p.tp[0], 13.0);
            assert_eq!(p.fy[0], 235.0);
            assert_eq!(p.g, 205_000.0 / 2.6);
        }
    }
    #[test]
    fn invalid_lower_column_rejects_direct_linear_state_updates() {
        let (mut model, data) = cross_joint_model();
        let mut lower = model.sections[1].clone();
        lower.id = SectionId(2);
        lower.material = None;
        model.sections.push(lower);
        model.elements[2].section = Some(SectionId(2));
        let reason = resolve_panel_joint(&model, NodeId(0), &model.elements)
            .unwrap_err()
            .to_string();
        let mut panel = PanelZone::new(&data, &model);
        assert_eq!(panel.resolution_error, Some(reason.clone()));
        assert_eq!(panel.k_panel, [0.0; 2]);
        let increment = LocalVec {
            data: smallvec::smallvec![0.0, 0.0],
        };
        assert_eq!(
            panel
                .update_state(&increment, false, &Ctx { model: &model })
                .unwrap_err()
                .to_string(),
            reason
        );
    }
    #[test]
    fn standalone_explicit_panel_without_column_retains_joint_error() {
        let (mut model, data) = cross_joint_model();
        model.elements.retain(|e| {
            model.sections[e.section.unwrap().index()].frame_use != Some(FrameSectionUse::Column)
        });
        for (i, element) in model.elements.iter_mut().enumerate() {
            element.id = ElemId(i as u32);
        }
        let panel = PanelZone::new(&data, &model);
        assert!(panel.resolution_error.as_ref().is_some_and(|reason| reason
            .contains("接合部 ID 0")
            && reason.contains("参照柱が存在しません")));
        assert_eq!(panel.k_panel, [0.0; 2]);
    }

    #[test]
    fn explicit_cft_joint_remains_outside_the_nonlinear_panel_gate() {
        let (mut model, data) = cross_joint_model();
        model.sections[1].shape = Some(SectionShape::CftBox {
            height: 400.0,
            width: 400.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(0.0),
        });
        assert!(PanelZone::nonlinear_issue(&data, &model).is_none());
        let behavior = crate::factory::build_nonlinear_behavior(
            &data,
            &model,
            crate::factory::StrengthBasis::Nominal,
            crate::factory::AnalysisKind::TimeHistory,
            None,
        );
        let ctx = Ctx { model: &model };
        assert_eq!(behavior.n_dof(), 2);
        assert_eq!(behavior.tangent_stiffness(&ctx).get(0, 0), 1.0e14);
    }
}
