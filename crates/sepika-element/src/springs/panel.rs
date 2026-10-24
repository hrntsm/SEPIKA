//! 仕口パネル（柱梁接合部パネル）要素。
//!
//! 柱の強軸・弱軸に対応するせん断ばねを持つ。
//! 変形を柱の曲げ回転軸へ写し、応力は同じ変換の転置で全体座標へ戻す。
//!
//! `{γX, γY}` は全体系 X/Y 基底を柱直交面へ投影する前の係数で、
//! 節点の標準 6 自由度とは別枠の追加自由度である。
//! グローバル自由度空間の末尾へ払い出される。本要素はその 2 自由度に対して
//! のみ剛性を与える。

use crate::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use sepika_core::dof::DofMap;
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, Model};
use sepika_core::panel_zone::{resolve_explicit_panel_analysis_joint, PanelGeometry};
use sepika_material::uniaxial::{Bilinear, UniaxialMaterial};
use smallvec::SmallVec;

/// パネル降伏後の二次勾配比。
pub const PANEL_HARDENING: f64 = 0.01;

/// パネル諸元を解決できなかった場合に用いる剛性 [N·mm/rad]。
const PANEL_RIGID_STIFFNESS: f64 = 1.0e14;

/// パネルの降伏モーメント `pMy` の軸力比 `n` を追従するための柱の情報。
///
/// パネル要素は自身の 2 自由度に加えて柱の 12 自由度を自由度写像へ含め、
/// 剛性 0 のまま変位だけを受け取る。剛性寄与は 0 のため、
/// 全体剛性行列には一切影響しない。
#[derive(Clone, Debug)]
struct ColumnAxial {
    /// 柱の 2 節点（i 端・j 端）。
    nodes: [NodeId; 2],
    /// 弾性軸剛性 `E・A/L` [N/mm]。
    ea_over_l: f64,
    /// 全体系での柱の単位軸ベクトル。
    axis: [f64; 3],
    /// 各方向の基準軸耐力 F・A [N]。
    n_ref: [f64; 2],
    committed: [f64; 12],
    trial: [f64; 12],
}

impl ColumnAxial {
    /// 柱の弾性軸力を基準軸耐力で除した比（圧縮正、引張負）。
    fn compression_ratio(&self) -> [f64; 2] {
        let mut d = 0.0;
        for k in 0..3 {
            d += (self.trial[6 + k] - self.trial[k]) * self.axis[k];
        }
        let n = self.ea_over_l * d;
        self.n_ref.map(|reference| -n / reference)
    }

    fn axial_ratio(&self) -> [f64; 2] {
        self.compression_ratio().map(|ratio| ratio.clamp(0.0, 1.0))
    }
}

/// 仕口パネル要素。方向別の諸元・応答は `[強軸, 弱軸]` の順。
pub struct PanelZone {
    /// 接合部の節点（追加自由度 `γX`・`γY` の持ち主）。
    pub node: NodeId,
    /// 諸元未算定の理由。剛パネルへの数値的代用を算定成功と扱わない。
    pub resolution_error: Option<String>,
    /// 方向別パネル寸法 `dc` [mm]。H 形の弱軸はフランジ幅。
    pub dc: [f64; 2],
    /// 梁フランジ板厚中心間距離 `db` `[mm]`。
    pub db: [f64; 2],
    /// 抵抗板 1 枚あたりの有効厚 `tp` [mm]。
    pub tp: [f64; 2],
    /// パネルの実効体積 `Ve` `[mm³]`。
    pub ve: [f64; 2],
    /// パネルの形状係数 κ。
    pub kappa: [f64; 2],
    /// せん断弾性係数 `G` [N/mm²]。
    pub g: f64,
    /// パネルの基準強度 `F` [N/mm²]。
    pub fy: [f64; 2],
    /// 柱の強軸・弱軸に対応するせん断剛性 `G・Ve` [N·mm/rad]。
    pub k_panel: [f64; 2],
    /// 柱の強軸・弱軸の順に並ぶ、全体座標系での曲げ回転軸。
    pub axes: [[f64; 3]; 2],
    /// 軸力比 `n = 0` における降伏モーメント `pMy0 = (Ve/κ)・Fy/√3` [N·mm]。
    pub pmy0: [f64; 2],
    /// 柱の強軸・弱軸に対応する弾塑性ばね。`None` は弾性（線形解析）。
    springs: Option<[Box<dyn UniaxialMaterial>; 2]>,
    /// 軸力比 `n` の追従に用いる柱。`None` は `n = 0` 固定。
    column: Option<ColumnAxial>,
    /// 確定・トライアルの投影前係数 `{γX, γY}`。
    committed_disp: [f64; 2],
    trial_disp: [f64; 2],
}

/// パネルに取り付く柱・梁から解決した諸元。
struct ResolvedPanel {
    geom: [PanelGeometry; 2],
    db: f64,
    ve: [f64; 2],
    g: f64,
    fy: [f64; 2],
    axes: [[f64; 3]; 2],
    column: Option<ColumnAxial>,
}

/// 接合部節点 `node` に取り付く柱・梁からパネル諸元を解決する。
fn resolve(model: &Model, node: NodeId) -> Result<Option<ResolvedPanel>, String> {
    let Some(joint) = resolve_explicit_panel_analysis_joint(model, node, &model.elements)
        .map_err(|reason| reason.to_string())?
    else {
        return Ok(None);
    };
    let col_elem = &model.elements[joint.column.index()];
    let invalid = |reason: &str| {
        format!(
            "接合部 ID {} のパネル未算定: 柱 ID {}: {reason}",
            node.0, joint.column.0
        )
    };
    let mat = model
        .element_material(col_elem)
        .ok_or_else(|| invalid("材料未設定または参照不明"))?;
    let sec = model
        .element_section(col_elem)
        .ok_or_else(|| invalid("断面未設定または参照不明"))?;

    let fy = joint.panels.map(|panel| panel.fy);
    let g = joint.g;
    let column = column_axial(model, col_elem, mat, sec, fy).map_err(|reason| invalid(&reason))?;
    Ok(Some(ResolvedPanel {
        geom: joint.panels.map(|panel| panel.geometry),
        db: joint.db,
        ve: joint.panels.map(|panel| panel.ve),
        g,
        fy,
        axes: joint.moment_axes,
        column: Some(column),
    }))
}

/// 正の有限値の柱諸元から軸力追従情報を解決し、算定不能の理由を返す。
fn column_axial(
    model: &Model,
    col: &ElementData,
    mat: &sepika_core::model::Material,
    sec: &sepika_core::model::Section,
    fy: [f64; 2],
) -> Result<ColumnAxial, String> {
    if col.nodes.len() < 2 {
        return Err("柱の両端節点が必要です".into());
    }
    let coordinate = |node: NodeId| {
        model
            .nodes
            .get(node.index())
            .map(|node| node.coord)
            .ok_or_else(|| format!("柱節点 ID {} を参照できません", node.0))
    };
    let d = sepika_core::geom::vec3::sub(coordinate(col.nodes[1])?, coordinate(col.nodes[0])?);
    let length_mm = sepika_core::geom::vec3::norm(d);
    for (name, value) in [
        ("柱長 L [mm]", length_mm),
        ("断面積 A [mm²]", sec.area),
        ("ヤング係数 E [N/mm²]", mat.young),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(format!("{name}={value} は正の有限値である必要があります"));
        }
    }
    let ea_over_l = mat.young * sec.area / length_mm;
    if !ea_over_l.is_finite() || ea_over_l <= 0.0 {
        return Err(format!(
            "軸剛性 EA/L={ea_over_l} N/mm は正の有限値である必要があります"
        ));
    }
    let mut n_ref = [0.0; 2];
    for (i, direction) in ["強軸", "弱軸"].into_iter().enumerate() {
        if !fy[i].is_finite() || fy[i] <= 0.0 {
            return Err(format!(
                "{direction}の基準強度 F={} N/mm² は正の有限値である必要があります",
                fy[i]
            ));
        }
        n_ref[i] = fy[i] * sec.area;
        if !n_ref[i].is_finite() || n_ref[i] <= 0.0 {
            return Err(format!(
                "{direction}の基準軸耐力 F・A={} N は正の有限値である必要があります",
                n_ref[i]
            ));
        }
    }
    let axis = d.map(|component| component / length_mm);
    if axis.iter().any(|value| !value.is_finite()) {
        return Err("柱の単位軸ベクトルが非有限です".into());
    }
    Ok(ColumnAxial {
        nodes: [col.nodes[0], col.nodes[1]],
        ea_over_l,
        axis,
        n_ref,
        committed: [0.0; 12],
        trial: [0.0; 12],
    })
}

impl PanelZone {
    /// 線形解析用の弾性パネルを生成する（追加自由度 2 個のみを持つ）。
    pub fn new(data: &ElementData, model: &Model) -> Self {
        Self::build(data, model, false)
    }

    /// 増分解析・時刻歴応答解析用の弾塑性パネルを生成する。
    pub fn new_nonlinear(data: &ElementData, model: &Model) -> Self {
        Self::build(data, model, true)
    }

    fn build(data: &ElementData, model: &Model, nonlinear: bool) -> Self {
        let node = data.nodes.first().copied().unwrap_or(NodeId(0));
        let (resolved, resolution_error) = match resolve(model, node) {
            Ok(resolved) => (resolved, None),
            Err(reason) => (None, Some(reason)),
        };

        let (dc, tp, db, ve, kappa, g, fy, column) = match &resolved {
            Some(r) => (
                r.geom.map(|geometry| geometry.dc),
                r.geom.map(|geometry| geometry.tp),
                [r.db; 2],
                r.ve,
                r.geom.map(|geometry| geometry.kappa()),
                r.g,
                r.fy,
                r.column.clone(),
            ),
            None => (
                [0.0; 2], [0.0; 2], [0.0; 2], [0.0; 2], [1.0; 2], 0.0, [0.0; 2], None,
            ),
        };
        let k_panel = ve.map(|volume| {
            let stiffness = g * volume;
            if resolution_error.is_some() {
                0.0
            } else if stiffness.is_finite() && stiffness > 0.0 {
                stiffness
            } else {
                PANEL_RIGID_STIFFNESS
            }
        });
        let pmy0 = std::array::from_fn(|i| {
            if kappa[i] > 0.0 {
                (ve[i] / kappa[i]) * fy[i] / 3.0_f64.sqrt()
            } else {
                0.0
            }
        });
        let springs = (nonlinear && pmy0.iter().all(|moment| *moment > 0.0)).then(|| {
            std::array::from_fn(|i| -> Box<dyn UniaxialMaterial> {
                Box::new(Bilinear::new(k_panel[i], pmy0[i], PANEL_HARDENING))
            })
        });

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
            pmy0,
            springs,
            column: if nonlinear { column } else { None },
            committed_disp: [0.0; 2],
            trial_disp: [0.0; 2],
        }
    }

    fn to_panel_frame(&self, gamma: [f64; 2]) -> [f64; 2] {
        self.axes
            .map(|axis| axis[0] * gamma[0] + axis[1] * gamma[1])
    }

    fn to_node_frame(&self, moment: [f64; 2]) -> [f64; 2] {
        std::array::from_fn(|i| self.axes[0][i] * moment[0] + self.axes[1][i] * moment[1])
    }

    /// 全体せん断変形角に対する柱の強軸・弱軸の接線剛性とモーメント。状態は書き換えない。
    fn response_at(&self, gamma: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        let gp = self.to_panel_frame(gamma);
        match &self.springs {
            Some(sp) => {
                let (m0, k0) = sp[0].probe(gp[0]);
                let (m1, k1) = sp[1].probe(gp[1]);
                ([k0, k1], [m0, m1])
            }
            None => (
                self.k_panel,
                std::array::from_fn(|i| self.k_panel[i] * gp[i]),
            ),
        }
    }

    /// 現在のトライアル状態での接線剛性とモーメント。
    fn panel_response(&self) -> ([f64; 2], [f64; 2]) {
        self.response_at(self.trial_disp)
    }

    /// 全体 X/Y 軸まわりのパネルせん断モーメント `{MSX, MSY}` [N·mm]。
    pub fn panel_moments(&self) -> [f64; 2] {
        let (_, m) = self.panel_response();
        self.to_node_frame(m)
    }

    /// 投影前の全体系基底係数に対する、全体 X/Y 軸まわりのせん断モーメント [N·mm]。
    pub fn panel_moments_at(&self, gamma: [f64; 2]) -> [f64; 2] {
        let (_, m) = self.response_at(gamma);
        self.to_node_frame(m)
    }

    /// 各方向の `F・A` に対する軸力比 `[強軸, 弱軸]`。追従対象の柱がなければ両成分 0。
    pub fn axial_ratio(&self) -> [f64; 2] {
        self.column
            .as_ref()
            .map_or([0.0; 2], ColumnAxial::axial_ratio)
    }

    /// 現在の軸力比における降伏モーメント `pMy = pMy0・√(1−n²)` [N·mm]。順序は `[強軸, 弱軸]`。
    pub fn yield_moment(&self) -> [f64; 2] {
        let n = self.axial_ratio();
        std::array::from_fn(|i| self.pmy0[i] * (1.0 - n[i] * n[i]).max(0.0).sqrt())
    }

    /// 軸力比の変化を弾塑性ばねの降伏値へ反映する。
    fn apply_axial_interaction(&mut self) {
        if self.springs.is_none() || self.column.is_none() {
            return;
        }
        let pmy = self.yield_moment();
        if let Some(sp) = self.springs.as_mut() {
            sp[0].set_yield(pmy[0]);
            sp[1].set_yield(pmy[1]);
        }
    }
}

impl ElementBehavior for PanelZone {
    fn n_dof(&self) -> usize {
        2 + if self.column.is_some() { 12 } else { 0 }
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        let mut gdofs = SmallVec::new();
        crate::behavior::push_panel_global_dofs(&mut gdofs, self.node.index(), dof);
        if let Some(col) = &self.column {
            crate::behavior::push_node_global_dofs(&mut gdofs, &col.nodes, dof);
        }
        gdofs
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        let (k_panel_frame, _) = self.panel_response();
        let t = self.axes;
        let mut k = LocalMat::zeros(self.n_dof());
        for i in 0..2 {
            for j in 0..2 {
                let mut v = 0.0;
                for p in 0..2 {
                    v += t[p][i] * k_panel_frame[p] * t[p][j];
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
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let mut trial_disp = self.trial_disp;
        for (i, displacement) in trial_disp.iter_mut().enumerate() {
            *displacement += du.data[i];
        }
        let mut column = self.column.clone();
        if let Some(col) = column.as_mut() {
            for i in 0..12 {
                col.trial[i] += du.data[2 + i];
            }
            if self.springs.is_some() {
                let n = col.compression_ratio();
                let clamped_n = col.axial_ratio();
                let pmy: [f64; 2] = std::array::from_fn(|i| {
                    self.pmy0[i] * (1.0 - clamped_n[i] * clamped_n[i]).max(0.0).sqrt()
                });
                if n.iter().any(|ratio| !ratio.is_finite())
                    || pmy
                        .iter()
                        .any(|moment| !moment.is_finite() || *moment < 1e-9)
                {
                    return Err(crate::behavior::ElementUpdateError(format!(
                        "接合部 ID {} のパネル未対応: 軸力比 n={n:?} の降伏モーメント pMy={pmy:?} N·mm は有限かつ 1e-9 N·mm 以上である必要があります（軸耐力到達・超過を含む）",
                        self.node.0
                    )));
                }
            }
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
        self.column = column;
        self.apply_axial_interaction();
        if let Some(sp) = self.springs.as_mut() {
            sp[0].trial(gp[0]);
            sp[1].trial(gp[1]);
        }
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn commit_state(&mut self) {
        self.committed_disp = self.trial_disp;
        if let Some(col) = self.column.as_mut() {
            col.committed = col.trial;
        }
        if let Some(sp) = self.springs.as_mut() {
            sp[0].commit();
            sp[1].commit();
        }
    }

    fn revert_state(&mut self) {
        self.trial_disp = self.committed_disp;
        if let Some(col) = self.column.as_mut() {
            col.trial = col.committed;
        }
        self.apply_axial_interaction();
        if let Some(sp) = self.springs.as_mut() {
            sp[0].revert();
            sp[1].revert();
        }
    }

    fn snapshot_state(&self) -> Box<dyn std::any::Any> {
        let col = self
            .column
            .as_ref()
            .map(|c| (c.committed, c.trial))
            .unwrap_or(([0.0; 12], [0.0; 12]));
        let springs = self
            .springs
            .as_ref()
            .map(|sp| [sp[0].serialize_state(), sp[1].serialize_state()]);
        Box::new((self.committed_disp, self.trial_disp, col, springs))
    }

    fn restore_state(&mut self, state: &dyn std::any::Any) {
        type Snapshot = (
            [f64; 2],
            [f64; 2],
            ([f64; 12], [f64; 12]),
            Option<[Vec<u8>; 2]>,
        );
        let (committed, trial, col, springs) =
            crate::behavior::downcast_snapshot::<Snapshot>("PanelZone", state);
        self.committed_disp = *committed;
        self.trial_disp = *trial;
        if let Some(c) = self.column.as_mut() {
            c.committed = col.0;
            c.trial = col.1;
        }
        if let (Some(sp), Some(data)) = (self.springs.as_mut(), springs.as_ref()) {
            sp[0]
                .deserialize_state(&data[0])
                .expect("PanelZone::restore_state: せん断ばね状態の復元");
            sp[1]
                .deserialize_state(&data[1])
                .expect("PanelZone::restore_state: せん断ばね状態の復元");
        }
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        let col = self
            .column
            .as_ref()
            .map(|c| (c.committed, c.trial))
            .unwrap_or(([0.0; 12], [0.0; 12]));
        let springs = self
            .springs
            .as_ref()
            .map(|sp| [sp[0].serialize_state(), sp[1].serialize_state()]);
        bincode::serialize(&(self.committed_disp, self.trial_disp, col, springs))
            .expect("serialize checkpoint")
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        if data.is_empty() {
            return Ok(());
        }
        type Checkpoint = (
            [f64; 2],
            [f64; 2],
            ([f64; 12], [f64; 12]),
            Option<[Vec<u8>; 2]>,
        );
        let (committed, trial, col, springs): Checkpoint = bincode::deserialize(data)
            .map_err(|e| crate::behavior::CheckpointError::Decode(e.to_string()))?;
        self.committed_disp = committed;
        self.trial_disp = trial;
        if let Some(c) = self.column.as_mut() {
            c.committed = col.0;
            c.trial = col.1;
        }
        if let (Some(sp), Some(bytes)) = (self.springs.as_mut(), springs.as_ref()) {
            sp[0].deserialize_state(&bytes[0])?;
            sp[1].deserialize_state(&bytes[1])?;
        }
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
    use sepika_core::model::MaterialCategory;
    use sepika_core::model::{
        ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material, Node, Section,
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
            plastic_zone: None,
            spring: None,
        };

        let model = Model {
            nodes: vec![
                node(0, [0.0, 0.0, 3000.0]),
                node(1, [-5000.0, 0.0, 3000.0]),
                node(2, [5000.0, 0.0, 3000.0]),
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
            plastic_zone: None,
            spring: None,
        };
        (model, panel)
    }

    /// 諸元の解決: dc は柱（鉛直材）から、db は梁（水平材）から取る。
    /// 取り違えると Ve が誤り、剛性・降伏耐力の双方が狂う（回帰テスト）。
    #[test]
    fn test_resolve_takes_dc_from_column_and_db_from_beam() {
        let (model, data) = cross_joint_model();
        let pz = PanelZone::new(&data, &model);
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
    }

    /// 各方向の弾性剛性は G·Ve で、自由度は 2 個。
    #[test]
    fn test_elastic_stiffness_is_g_times_ve() {
        let (model, data) = cross_joint_model();
        let pz = PanelZone::new(&data, &model);
        assert_eq!(pz.n_dof(), 2, "弾性パネルは 2 自由度");

        let k = pz.tangent_stiffness(&Ctx { model: &model });
        let expected = pz.g * pz.ve[0];
        assert!((k.get(0, 0) - expected).abs() / expected < 1e-12);
        let weak_expected = pz.g * pz.ve[1];
        assert!((k.get(1, 1) - weak_expected).abs() / weak_expected < 1e-12);
        assert!(
            k.get(0, 1).abs() < 1e-6,
            "柱主軸と全体軸が一致すると非対角は 0"
        );
    }

    /// 内力は M = K·γ（弾性）。トライアル変位に追従する。
    #[test]
    fn test_internal_force_follows_trial_rotation() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut pz = PanelZone::new(&data, &model);
        let k = pz.g * pz.ve[0];

        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, 2),
        };
        du.data[0] = 1.0e-4;
        pz.update_state(&du, false, &ctx).unwrap();

        let f = pz.internal_force(&ctx);
        assert!((f.data[0] - k * 1.0e-4).abs() / (k * 1.0e-4) < 1e-12);
        assert!(f.data[1].abs() < 1e-6);
        assert!((pz.panel_moments()[0] - f.data[0]).abs() < 1e-6);
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
        for (actual, expected) in panel
            .pmy0
            .into_iter()
            .zip([373_111_832.498_692_4, 885_918_584.659_636_4])
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

    #[test]
    fn panel_757_product_thickness_strength_controls_each_axial_ratio_and_rejection() {
        let (mut model, data) = cross_joint_model();
        if let Some(SectionShape::SteelH {
            web_thick,
            flange_thick,
            ..
        }) = &mut model.sections[1].shape
        {
            *web_thick = 39.0;
            *flange_thick = 41.0;
        }
        model.sections[1].panel_thickness = Some(100.0);
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        assert_eq!(panel.resolution_error, None);
        assert_eq!(panel.tp, [100.0; 2]);
        assert_eq!(panel.fy, [235.0, 215.0]);
        assert_eq!(
            panel.column.as_ref().unwrap().n_ref,
            [5_139_450.0, 4_702_050.0]
        );
        for (actual, expected) in panel
            .pmy0
            .into_iter()
            .zip([2_504_891_676.354_933, 3_859_625_039.550_594_3])
        {
            assert!((actual / expected - 1.0).abs() < 1e-12);
        }
        panel
            .update_state(&panel_724_increment(0.0, 0.5), true, &ctx)
            .unwrap();
        for (actual, expected) in panel
            .axial_ratio()
            .into_iter()
            .zip([0.5, 0.5465116279069767])
        {
            assert!((actual - expected).abs() < 1e-12);
        }
        for (actual, expected) in panel
            .yield_moment()
            .into_iter()
            .zip([2_169_299_825.451_56, 3_232_248_838.055_497])
        {
            assert!((actual / expected - 1.0).abs() < 1e-12);
        }
        let before = panel.serialize_checkpoint();
        let mut increment = panel_724_increment(0.0, 0.0);
        increment.data[10] = -107.5 * 3_000.0 / 205_000.0;
        for commit in [false, true] {
            assert!(panel.update_state(&increment, commit, &ctx).is_err());
            assert_eq!(panel.serialize_checkpoint(), before);
        }
    }

    fn panel_757_directional_fixture(nonlinear: bool) -> (Model, PanelZone) {
        let (mut model, data) = cross_joint_model();
        for column in &mut model.elements[2..] {
            column.local_axis.ref_vector = [0.6, 0.8, 0.0];
        }
        let mut panel = PanelZone::build(&data, &model, nonlinear);
        panel.k_panel = [1_000.0, 4_000.0];
        panel.pmy0 = [10.0, 20.0];
        if nonlinear {
            panel.springs = Some(std::array::from_fn(|i| -> Box<dyn UniaxialMaterial> {
                Box::new(Bilinear::new(
                    panel.k_panel[i],
                    panel.pmy0[i],
                    PANEL_HARDENING,
                ))
            }));
        }
        (model, panel)
    }

    #[test]
    fn panel_757_directional_stiffness_and_global_moments_are_work_conjugate() {
        let (model, mut panel) = panel_757_directional_fixture(false);
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

    fn panel_757_increment(normalized_rotation: f64, axial_ratio: f64) -> LocalVec {
        let mut increment = panel_724_increment(-0.011 * normalized_rotation, axial_ratio);
        increment.data[1] = 0.002 * normalized_rotation;
        increment
    }

    #[test]
    fn panel_757_two_axis_cycle_matches_independent_vertices_and_work() {
        let (model, mut panel) = panel_757_directional_fixture(true);
        let ctx = Ctx { model: &model };
        panel
            .update_state(&panel_757_increment(0.0, 0.6), true, &ctx)
            .unwrap();
        for (actual, expected) in panel.yield_moment().into_iter().zip([8.0, 16.0]) {
            assert!((actual - expected).abs() < 1e-12);
        }
        let vertices = [
            (0.4, 0.4, Some(1.0)),
            (0.8, 0.8, None),
            (1.6, 0.808, Some(0.01)),
            (0.8, 0.008, Some(1.0)),
            (0.0, -0.792, None),
            (-0.8, -0.8, Some(0.01)),
            (-1.6, -0.808, Some(0.01)),
            (-0.8, -0.008, Some(1.0)),
            (0.0, 0.792, None),
            (0.8, 0.8, Some(0.01)),
            (1.6, 0.808, Some(0.01)),
        ];
        let mut previous_rotation = 0.0;
        let mut previous_moment = [0.0; 2];
        let mut work = 0.0;
        for (index, (rotation, expected, tangent_ratio)) in vertices.into_iter().enumerate() {
            let increment = panel_757_increment(rotation - previous_rotation, 0.0);
            panel.update_state(&increment, false, &ctx).unwrap();
            let moment = panel.panel_moments();
            assert!((moment[0] + 20.0 * expected).abs() < 1e-12);
            assert!((moment[1] + 10.0 * expected).abs() < 1e-12);
            if let Some(ratio) = tangent_ratio {
                let tangent = panel.tangent_stiffness(&ctx);
                for (i, row) in [[2_080.0, 1_440.0], [1_440.0, 2_920.0]].iter().enumerate() {
                    for (j, expected) in row.iter().enumerate() {
                        assert!((tangent.get(i, j) - ratio * expected).abs() < 1e-10);
                    }
                }
            }
            if index >= 3 {
                work += 0.5
                    * ((previous_moment[0] + moment[0]) * increment.data[0]
                        + (previous_moment[1] + moment[1]) * increment.data[1]);
            }
            panel.commit_state();
            previous_rotation = rotation;
            previous_moment = moment;
        }
        assert!((work - 0.50688).abs() < 1e-12);
    }

    #[test]
    fn panel_757_directional_history_restores_uncommitted_state_and_retries() {
        let (model, mut panel) = panel_757_directional_fixture(true);
        let (_, mut control) = panel_757_directional_fixture(true);
        let ctx = Ctx { model: &model };
        let initial = panel_757_increment(1.6, 0.6);
        panel.update_state(&initial, true, &ctx).unwrap();
        control.update_state(&initial, true, &ctx).unwrap();
        let trial = panel_757_increment(-0.8, 0.2);
        panel.update_state(&trial, false, &ctx).unwrap();
        let snapshot = panel.snapshot_state();
        let checkpoint = panel.serialize_checkpoint();
        let trial_moments = panel.panel_moments();
        let trial_yield = panel.yield_moment();
        for use_checkpoint in [false, true] {
            panel
                .update_state(&panel_757_increment(-3.2, -0.3), true, &ctx)
                .unwrap();
            if use_checkpoint {
                panel.deserialize_checkpoint(&checkpoint).unwrap();
            } else {
                panel.restore_state(snapshot.as_ref());
            }
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
            assert_eq!(panel.panel_moments(), trial_moments);
            assert_eq!(panel.yield_moment(), trial_yield);
            panel.revert_state();
            assert_panel_751_same_state(&panel, &control, &ctx);
            panel.revert_state();
            assert_panel_751_same_state(&panel, &control, &ctx);
            panel.update_state(&trial, false, &ctx).unwrap();
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
        }
    }

    #[test]
    fn panel_757_each_direction_rejects_invalid_yield_without_mutation() {
        let (model, _) = panel_757_directional_fixture(true);
        let ctx = Ctx { model: &model };
        for axis in 0..2 {
            for moment in [f64::from_bits(1e-9_f64.to_bits() - 1), f64::INFINITY] {
                for commit in [false, true] {
                    let (_, mut panel) = panel_757_directional_fixture(true);
                    panel
                        .update_state(&panel_757_increment(1.6, 0.6), true, &ctx)
                        .unwrap();
                    panel
                        .update_state(&panel_757_increment(-0.8, 0.0), false, &ctx)
                        .unwrap();
                    panel.pmy0[axis] = moment;
                    let before = panel.serialize_checkpoint();
                    let moments = panel.panel_moments();
                    let tangent = panel.tangent_stiffness(&ctx);
                    assert!(panel
                        .update_state(&panel_757_increment(0.1, 0.0), commit, &ctx)
                        .is_err());
                    assert_eq!(panel.serialize_checkpoint(), before);
                    assert_eq!(panel.panel_moments(), moments);
                    let after_tangent = panel.tangent_stiffness(&ctx);
                    for i in 0..panel.n_dof() {
                        for j in 0..panel.n_dof() {
                            assert_eq!(after_tangent.get(i, j), tangent.get(i, j));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn panel_757_nonfinite_principal_rotation_is_rejected_atomically() {
        let (model, mut panel) = panel_757_directional_fixture(false);
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
                    assert_eq!(PanelZone::new(&data, &model).axes, axes);
                }
            }
        }
    }

    /// 弾塑性パネル: 降伏点は pMy0 = (Ve/κ)·Fy/√3。降伏後の接線剛性は
    /// 二次勾配比 PANEL_HARDENING 倍になる。
    #[test]
    fn test_nonlinear_panel_yields_at_pmy() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut pz = PanelZone::new_nonlinear(&data, &model);

        let expected_pmy0 = (pz.ve[0] / pz.kappa[0]) * pz.fy[0] / 3.0_f64.sqrt();
        assert!((pz.pmy0[0] - expected_pmy0).abs() / expected_pmy0 < 1e-12);

        let gamma_y = pz.pmy0[0] / (pz.g * pz.ve[0]);

        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, pz.n_dof()),
        };
        du.data[0] = gamma_y * 0.5;
        pz.update_state(&du, true, &ctx).unwrap();
        let k = pz.tangent_stiffness(&ctx);
        assert!((k.get(0, 0) - pz.g * pz.ve[0]).abs() / (pz.g * pz.ve[0]) < 1e-9);

        let mut du2 = LocalVec {
            data: SmallVec::from_elem(0.0, pz.n_dof()),
        };
        du2.data[0] = gamma_y * 2.0;
        pz.update_state(&du2, false, &ctx).unwrap();
        let k2 = pz.tangent_stiffness(&ctx);
        let k_expected = PANEL_HARDENING * pz.g * pz.ve[0];
        assert!(
            (k2.get(0, 0) - k_expected).abs() / k_expected < 1e-9,
            "降伏後の接線剛性 {} は {} のはず",
            k2.get(0, 0),
            k_expected
        );
        let m = pz.panel_moments()[0];
        assert!(
            m > pz.pmy0[0] && m < 1.05 * pz.pmy0[0],
            "M={} pMy0={}",
            m,
            pz.pmy0[0]
        );
    }

    /// 弾塑性パネルは軸力比の追従のため柱の 12 自由度を自由度写像へ含めるが、
    /// 剛性寄与は 0（全体剛性行列に一切影響しない）。
    #[test]
    fn test_nonlinear_panel_tracks_column_without_stiffness() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let pz = PanelZone::new_nonlinear(&data, &model);
        assert_eq!(pz.n_dof(), 14, "パネル 2 ＋ 柱 12");

        let k = pz.tangent_stiffness(&ctx);
        for i in 0..14 {
            for j in 0..14 {
                if i < 2 && j < 2 {
                    continue;
                }
                assert_eq!(
                    k.get(i, j),
                    0.0,
                    "柱自由度へ剛性を与えてはならない ({i},{j})"
                );
            }
        }
    }

    /// 軸力比 n の増加で降伏モーメントが pMy0·√(1−n²) へ低下する。
    #[test]
    fn test_yield_moment_reduces_with_axial_ratio() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut pz = PanelZone::new_nonlinear(&data, &model);
        assert!(
            (pz.yield_moment()[0] - pz.pmy0[0]).abs() / pz.pmy0[0] < 1e-12,
            "初期 n=0"
        );

        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, pz.n_dof()),
        };
        du.data[2 + 6 + 2] = -1.0;
        pz.update_state(&du, true, &ctx).unwrap();

        let n = pz.axial_ratio()[0];
        assert!(n > 0.0, "圧縮軸力で n > 0 になるはず: n={n}");
        let expected = pz.pmy0[0] * (1.0 - n * n).sqrt();
        assert!(
            (pz.yield_moment()[0] - expected).abs() / pz.pmy0[0] < 1e-12,
            "pMy={} 期待={}",
            pz.yield_moment()[0],
            expected
        );
        assert!(pz.yield_moment()[0] < pz.pmy0[0], "軸力で降伏耐力が下がる");
    }

    /// 引張軸力では降伏耐力を低減しない（検定側と同じ規約）。
    #[test]
    fn test_tension_does_not_reduce_yield_moment() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut pz = PanelZone::new_nonlinear(&data, &model);
        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, pz.n_dof()),
        };
        du.data[2 + 6 + 2] = 1.0;
        pz.update_state(&du, true, &ctx).unwrap();
        assert_eq!(pz.axial_ratio()[0], 0.0);
        assert!((pz.yield_moment()[0] - pz.pmy0[0]).abs() / pz.pmy0[0] < 1e-12);
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
    fn lower_column_supplies_geometry_material_and_nonlinear_axis_after_reordering() {
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
            for p in [
                PanelZone::new(&data, &model),
                PanelZone::new_nonlinear(&data, &model),
            ] {
                assert_eq!(p.resolution_error, None);
                assert_eq!(p.dc[0], 379.0);
                assert_eq!(p.tp[0], 13.0);
                assert_eq!(p.fy[0], 235.0);
                assert_eq!(p.g, 205_000.0 / 2.6);
                if let Some(col) = p.column {
                    assert_eq!(
                        col.nodes,
                        if reverse {
                            [NodeId(0), NodeId(3)]
                        } else {
                            [NodeId(3), NodeId(0)]
                        }
                    );
                    assert_eq!(
                        col.axis,
                        if reverse {
                            [0.0, 0.0, -1.0]
                        } else {
                            [0.0, 0.0, 1.0]
                        }
                    );
                    assert_eq!(col.n_ref, [235.0 * 21_870.0; 2]);
                }
            }
        }
    }
    #[test]
    fn invalid_lower_column_rejects_direct_linear_and_nonlinear_state_updates() {
        let (mut model, data) = cross_joint_model();
        let mut lower = model.sections[1].clone();
        lower.id = SectionId(2);
        lower.material = None;
        model.sections.push(lower);
        model.elements[2].section = Some(SectionId(2));
        let reason = resolve_panel_joint(&model, NodeId(0), &model.elements)
            .unwrap_err()
            .to_string();
        for mut panel in [
            PanelZone::new(&data, &model),
            PanelZone::new_nonlinear(&data, &model),
        ] {
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
        for panel in [
            PanelZone::new(&data, &model),
            PanelZone::new_nonlinear(&data, &model),
        ] {
            assert!(panel
                .resolution_error
                .as_ref()
                .is_some_and(|reason| reason.contains("接合部 ID 0")
                    && reason.contains("参照柱が存在しません")));
            assert_eq!(panel.k_panel, [0.0; 2]);
        }
    }

    #[test]
    fn panel_751_revert_restores_committed_moment_after_axial_trial() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, moment, gamma) = panel_724_reference_scales();
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        panel
            .update_state(&panel_724_increment(1.6 * gamma, 0.6), true, &ctx)
            .unwrap();
        assert!((panel.panel_moments()[0] / moment - 0.808).abs() < 1e-12);
        panel
            .update_state(&panel_724_increment(0.0, 0.2), false, &ctx)
            .unwrap();
        assert!((panel.panel_moments()[0] / moment - 0.61).abs() < 1e-12);
        panel.revert_state();
        assert!((panel.axial_ratio()[0] - 0.6).abs() < 1e-12);
        assert!((panel.yield_moment()[0] / moment - 0.8).abs() < 1e-12);
        assert!((panel.panel_moments()[0] / moment - 0.808).abs() < 1e-12);
    }

    fn assert_panel_751_same_state(actual: &PanelZone, expected: &PanelZone, ctx: &Ctx) {
        assert_eq!(
            actual.serialize_checkpoint(),
            expected.serialize_checkpoint()
        );
        assert_eq!(actual.axial_ratio(), expected.axial_ratio());
        assert_eq!(actual.yield_moment(), expected.yield_moment());
        assert_eq!(actual.panel_moments(), expected.panel_moments());
        assert_eq!(
            actual.internal_force(ctx).data,
            expected.internal_force(ctx).data
        );
        let actual_k = actual.tangent_stiffness(ctx);
        let expected_k = expected.tangent_stiffness(ctx);
        for i in 0..actual.n_dof() {
            for j in 0..actual.n_dof() {
                assert_eq!(actual_k.get(i, j), expected_k.get(i, j));
            }
        }
    }

    #[test]
    fn panel_751_revert_and_retry_match_control_in_both_directions() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, moment, gamma) = panel_724_reference_scales();
        let weak_moment = 9_794_400.0 * 235.0 / (1.5 * 3.0_f64.sqrt());
        let weak_gamma = 235.0 / (1.5 * 3.0_f64.sqrt() * (205_000.0 / 2.6));
        let moments = [moment, weak_moment];
        for directions in [
            [1.0, 0.0],
            [-1.0, 0.0],
            [0.0, 1.0],
            [0.0, -1.0],
            [1.0, -1.0],
        ] {
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            let mut control = PanelZone::new_nonlinear(&data, &model);
            let mut initial = panel_724_increment(directions[0] * 1.6 * gamma, 0.6);
            initial.data[1] = directions[1] * 1.6 * weak_gamma;
            panel.update_state(&initial, true, &ctx).unwrap();
            control.update_state(&initial, true, &ctx).unwrap();
            for (i, direction) in directions.into_iter().enumerate() {
                assert!((panel.panel_moments()[i] / moments[i] - direction * 0.808).abs() < 1e-12);
            }
            for delta_n in [0.2, -0.3] {
                panel
                    .update_state(&panel_724_increment(-3.2 * gamma, delta_n), false, &ctx)
                    .unwrap();
                panel.revert_state();
                assert_panel_751_same_state(&panel, &control, &ctx);
                panel.revert_state();
                assert_panel_751_same_state(&panel, &control, &ctx);
            }
            for (delta_gamma, delta_n) in [(-0.8, 0.0), (-3.2, 0.2), (0.4, -0.3)] {
                let mut retry = panel_724_increment(delta_gamma * gamma, delta_n);
                retry.data[1] = -delta_gamma * weak_gamma;
                panel.update_state(&retry, false, &ctx).unwrap();
                control.update_state(&retry, false, &ctx).unwrap();
                assert_panel_751_same_state(&panel, &control, &ctx);
                panel.revert_state();
                control.revert_state();
                assert_panel_751_same_state(&panel, &control, &ctx);
            }
        }
    }

    #[test]
    fn panel_751_restored_uncommitted_snapshot_and_checkpoint_can_revert() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, _, gamma) = panel_724_reference_scales();
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let mut control = PanelZone::new_nonlinear(&data, &model);
        let initial = panel_724_increment(1.6 * gamma, 0.6);
        panel.update_state(&initial, true, &ctx).unwrap();
        control.update_state(&initial, true, &ctx).unwrap();
        panel
            .update_state(&panel_724_increment(-0.8 * gamma, 0.2), false, &ctx)
            .unwrap();
        let snapshot = panel.snapshot_state();
        let checkpoint = panel.serialize_checkpoint();
        for use_checkpoint in [false, true] {
            panel
                .update_state(&panel_724_increment(-3.2 * gamma, -0.3), true, &ctx)
                .unwrap();
            if use_checkpoint {
                panel.deserialize_checkpoint(&checkpoint).unwrap();
            } else {
                panel.restore_state(snapshot.as_ref());
            }
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
            assert!((panel.axial_ratio()[0] - 0.8).abs() < 1e-12);
            panel.revert_state();
            assert_panel_751_same_state(&panel, &control, &ctx);
            let retry = panel_724_increment(-0.8 * gamma, 0.0);
            panel.update_state(&retry, false, &ctx).unwrap();
            control.update_state(&retry, false, &ctx).unwrap();
            assert_panel_751_same_state(&panel, &control, &ctx);
            control.revert_state();
        }
    }

    fn panel_724_reference_scales() -> (f64, f64, f64) {
        let volume_mm3 = 379.0 * 583.0 * 13.0;
        let kappa = 1.0 / (2.0 / 3.0 + 33_600.0 / 4_927.0) + 1.0 / (1.0 + 4_927.0 / 50_400.0);
        let stiffness_nmm = 205_000.0 / 2.6 * volume_mm3;
        let moment_nmm = volume_mm3 * 235.0 / (kappa * 3.0_f64.sqrt());
        (stiffness_nmm, moment_nmm, moment_nmm / stiffness_nmm)
    }

    fn panel_724_increment(gamma_rad: f64, axial_ratio: f64) -> LocalVec {
        let mut increment = LocalVec {
            data: SmallVec::from_elem(0.0, 14),
        };
        increment.data[0] = gamma_rad;
        increment.data[10] = -axial_ratio * 235.0 * 3_000.0 / 205_000.0;
        increment
    }

    #[test]
    fn panel_724_constant_compression_cycle_matches_independent_vertices_and_work() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let (stiffness, moment, gamma) = panel_724_reference_scales();
        panel
            .update_state(&panel_724_increment(0.0, 0.6), true, &ctx)
            .unwrap();
        assert!((panel.axial_ratio()[0] - 0.6).abs() < 1e-12);
        assert!((panel.yield_moment()[0] / moment - 0.8).abs() < 1e-12);
        let vertices = [
            (0.4, 0.4, Some(1.0)),
            (0.8, 0.8, None),
            (1.6, 0.808, Some(0.01)),
            (0.8, 0.008, Some(1.0)),
            (0.0, -0.792, None),
            (-0.8, -0.8, Some(0.01)),
            (-1.6, -0.808, Some(0.01)),
            (-0.8, -0.008, Some(1.0)),
            (0.0, 0.792, None),
            (0.8, 0.8, Some(0.01)),
            (1.6, 0.808, Some(0.01)),
        ];
        let (mut previous_x, mut previous_m, mut loop_work) = (0.0, 0.0, 0.0);
        for (index, (x, expected_m, expected_k)) in vertices.into_iter().enumerate() {
            panel
                .update_state(
                    &panel_724_increment((x - previous_x) * gamma, 0.0),
                    false,
                    &ctx,
                )
                .unwrap();
            let actual_m = panel.internal_force(&ctx).data[0] / moment;
            assert!(
                (actual_m - expected_m).abs() < 1e-12,
                "vertex {index}: {actual_m}"
            );
            if let Some(expected_k) = expected_k {
                assert!(
                    (panel.tangent_stiffness(&ctx).get(0, 0) / stiffness - expected_k).abs()
                        < 1e-12
                );
            }
            if index >= 3 {
                loop_work += 0.5 * (previous_m + actual_m) * (x - previous_x);
            }
            panel.commit_state();
            assert!((panel.panel_moments()[0] / moment - expected_m).abs() < 1e-12);
            (previous_x, previous_m) = (x, actual_m);
        }
        // 閉ループ面積は 4(1-b)My(γmax-γy)、b=0.01、My=0.8M0。
        assert!((loop_work - 2.5344).abs() < 1e-12);
        println!("panel724 constant-n: K={stiffness}, M0={moment}, gamma0={gamma}, normalized_loop_work={loop_work}");
    }

    #[test]
    fn panel_724_changed_axial_yield_retains_committed_plastic_rotation() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let (stiffness, moment, gamma) = panel_724_reference_scales();
        panel
            .update_state(&panel_724_increment(1.6 * gamma, 0.6), true, &ctx)
            .unwrap();
        for (delta_gamma, delta_n, expected_m, expected_k) in [
            (0.0, 0.2, 0.61, 0.01),
            (0.0, -0.2, 0.61, 1.0),
            (-0.8, 0.0, -0.19, 1.0),
            (0.8, 0.0, 0.61, 1.0),
        ] {
            panel
                .update_state(
                    &panel_724_increment(delta_gamma * gamma, delta_n),
                    false,
                    &ctx,
                )
                .unwrap();
            assert!((panel.panel_moments()[0] / moment - expected_m).abs() < 1e-12);
            assert!(
                (panel.tangent_stiffness(&ctx).get(0, 0) / stiffness - expected_k).abs() < 1e-12
            );
            panel.commit_state();
        }
    }

    #[test]
    fn panel_724_snapshot_and_checkpoint_restore_axial_yield_and_plastic_history() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, moment, gamma) = panel_724_reference_scales();
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        panel
            .update_state(&panel_724_increment(1.6 * gamma, 0.6), true, &ctx)
            .unwrap();
        let snapshot = panel.snapshot_state();
        let checkpoint = panel.serialize_checkpoint();
        for use_checkpoint in [false, true] {
            panel
                .update_state(&panel_724_increment(-3.2 * gamma, 0.2), true, &ctx)
                .unwrap();
            if use_checkpoint {
                panel.deserialize_checkpoint(&checkpoint).unwrap();
            } else {
                panel.restore_state(snapshot.as_ref());
            }
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
            assert!((panel.axial_ratio()[0] - 0.6).abs() < 1e-12);
            assert!((panel.yield_moment()[0] / moment - 0.8).abs() < 1e-12);
            assert!((panel.panel_moments()[0] / moment - 0.808).abs() < 1e-12);
            panel
                .update_state(&panel_724_increment(-0.8 * gamma, 0.0), false, &ctx)
                .unwrap();
            assert!((panel.panel_moments()[0] / moment - 0.008).abs() < 1e-12);
        }
    }

    #[test]
    fn panel_724_elastic_column_axial_force_matches_with_rigid_arms_and_reversed_ends() {
        use crate::frame::beam::BeamElement;

        for reversed in [false, true] {
            for rigid_arm_mm in [0.0, 300.0] {
                let (mut model, data) = cross_joint_model();
                model.elements[2].rigid_zone.panel_offset_j = rigid_arm_mm;
                if reversed {
                    model.elements[2].nodes.reverse();
                    model.elements[2].rigid_zone.panel_offset_j = 0.0;
                    model.elements[2].rigid_zone.panel_offset_i = rigid_arm_mm;
                }
                let ctx = Ctx { model: &model };
                let mut panel = PanelZone::new_nonlinear(&data, &model);
                let column = BeamElement::new(&model.elements[2], &model);
                let mut increment = panel_724_increment(0.0, 0.6);
                if reversed {
                    increment.data.swap(4, 10);
                }
                panel.update_state(&increment, false, &ctx).unwrap();
                let mut displacements = [0.0; 12];
                displacements.copy_from_slice(&increment.data[2..]);
                let response = column.recover_forces(&displacements);
                let expected_axial_n = -3_083_670.0;
                for (_, force) in response.at {
                    assert!((force[0] - expected_axial_n).abs() < 1e-6);
                }
                assert!((panel.axial_ratio()[0] - 0.6).abs() < 1e-12);
            }
        }
    }
    #[test]
    fn panel_753_high_compression_matches_public_yield_without_floor() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, moment, gamma) = panel_724_reference_scales();
        for (n, expected_yield, expected_response) in [
            (0.9997, 0.0244930602416276, 0.0252481296392114),
            (0.9998, 0.0199989999749987, 0.0207990099752487),
            (0.9999, 0.0141417820659208, 0.0150003642452616),
        ] {
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            panel
                .update_state(&panel_724_increment(0.1 * gamma, n), false, &ctx)
                .unwrap();
            let published = panel.yield_moment()[0] / moment;
            let reference_moments = [moment, 9_794_400.0 * 235.0 / (1.5 * 3.0_f64.sqrt())];
            for (spring, reference) in panel
                .springs
                .as_ref()
                .unwrap()
                .iter()
                .zip(reference_moments)
            {
                assert!((spring.reference_stress() / reference - expected_yield).abs() < 1e-11);
            }
            let response = panel.panel_moments()[0] / moment;
            println!("panel753 corrected: n={n}, published={published}, response={response}");
            assert!((published - expected_yield).abs() < 1e-11);
            assert!((response - expected_response).abs() < 1e-11);
        }
    }

    #[test]
    fn panel_753_rejects_axial_limit_without_changing_committed_or_trial_state() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (_, _, gamma) = panel_724_reference_scales();
        for commit in [false, true] {
            for n in [1.0, 1.1] {
                let mut panel = PanelZone::new_nonlinear(&data, &model);
                panel
                    .update_state(&panel_724_increment(0.1 * gamma, 0.9999), true, &ctx)
                    .unwrap();
                panel
                    .update_state(&panel_724_increment(-0.01 * gamma, 0.0), false, &ctx)
                    .unwrap();
                let before = panel.serialize_checkpoint();
                let moments = panel.panel_moments();
                let error = panel
                    .update_state(&panel_724_increment(-0.1 * gamma, n - 0.9999), commit, &ctx)
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("接合部 ID 0") && error.contains("軸耐力到達・超過"),
                    "{error}"
                );
                assert_eq!(panel.serialize_checkpoint(), before);
                assert_eq!(panel.panel_moments(), moments);
                panel.revert_state();
                assert!(
                    (panel.panel_moments()[0] / panel.pmy0[0] - 0.0150003642452616).abs() < 1e-11
                );
            }
        }
    }

    #[test]
    fn panel_753_material_setting_boundary_is_inclusive_without_residual_floor() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        for (moment, accepted) in [
            (1e-9, true),
            (f64::from_bits(1e-9_f64.to_bits() - 1), false),
            (f64::INFINITY, false),
        ] {
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            panel.pmy0 = [moment; 2];
            let before = panel.serialize_checkpoint();
            let result = panel.update_state(&panel_724_increment(0.0, 0.0), true, &ctx);
            assert_eq!(result.is_ok(), accepted);
            if accepted {
                assert_eq!(panel.yield_moment()[0], 1e-9);
                for spring in panel.springs.as_ref().unwrap() {
                    assert_eq!(spring.reference_stress(), 1e-9);
                }
            } else {
                assert!(result.unwrap_err().to_string().contains("1e-9 N·mm"));
                assert_eq!(panel.serialize_checkpoint(), before);
            }
        }
    }

    #[test]
    fn panel_753_high_compression_cycle_restores_and_retries_in_both_directions() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        let (stiffness, moment, gamma) = panel_724_reference_scales();
        for sign in [-1.0, 1.0] {
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            panel
                .update_state(&panel_724_increment(sign * 0.1 * gamma, 0.9999), true, &ctx)
                .unwrap();
            assert!((panel.panel_moments()[0] / moment - sign * 0.0150003642452616).abs() < 1e-11);
            assert!((panel.tangent_stiffness(&ctx).get(0, 0) / stiffness - 0.01).abs() < 1e-12);
            let committed = panel.serialize_checkpoint();
            panel
                .update_state(&panel_724_increment(-sign * 0.01 * gamma, 0.0), false, &ctx)
                .unwrap();
            assert!((panel.panel_moments()[0] / moment - sign * 0.0050003642452616).abs() < 1e-11);
            let snapshot = panel.snapshot_state();
            let checkpoint = panel.serialize_checkpoint();
            for use_checkpoint in [false, true] {
                panel
                    .update_state(&panel_724_increment(-sign * 0.19 * gamma, 0.0), true, &ctx)
                    .unwrap();
                assert!(
                    (panel.panel_moments()[0] / moment + sign * 0.0150003642452616).abs() < 1e-11
                );
                if use_checkpoint {
                    panel.deserialize_checkpoint(&checkpoint).unwrap();
                } else {
                    panel.restore_state(snapshot.as_ref());
                }
                assert_eq!(panel.serialize_checkpoint(), checkpoint);
                panel.revert_state();
                assert_eq!(panel.serialize_checkpoint(), committed);
                panel
                    .update_state(&panel_724_increment(-sign * 0.01 * gamma, 0.0), false, &ctx)
                    .unwrap();
                assert_eq!(panel.serialize_checkpoint(), checkpoint);
            }
        }
    }

    #[test]
    fn panel_753_nonfinite_axial_arithmetic_is_rejected_atomically() {
        let (model, data) = cross_joint_model();
        let ctx = Ctx { model: &model };
        for displacement in [f64::MAX, -f64::MAX] {
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            let before = panel.serialize_checkpoint();
            let mut increment = panel_724_increment(0.0, 0.0);
            increment.data[10] = displacement;
            assert!(panel.update_state(&increment, false, &ctx).is_err());
            assert_eq!(panel.serialize_checkpoint(), before);
        }
    }

    fn panel_761_model(length_mm: f64, area_mm2: f64) -> (Model, ElementData) {
        let (mut model, data) = cross_joint_model();
        for node in &mut model.nodes {
            node.coord = [
                node.coord[0] * length_mm / 3000.0,
                0.0,
                (node.coord[2] - 3000.0) * length_mm / 3000.0,
            ];
        }
        let scale = (area_mm2 / 21_870.0).sqrt();
        for section in &mut model.sections {
            if let Some(SectionShape::SteelH {
                height,
                width,
                web_thick,
                flange_thick,
                ..
            }) = &mut section.shape
            {
                *height *= scale;
                *width *= scale;
                *web_thick *= scale;
                *flange_thick *= scale;
            }
            section.depth *= scale;
            section.width *= scale;
            section.area *= scale * scale;
            section.as_y *= scale * scale;
            section.as_z *= scale * scale;
            section.iy *= scale.powi(4);
            section.iz *= scale.powi(4);
            section.j *= scale.powi(4);
        }
        model.sections[1].area = area_mm2;
        model.materials[0].name = "direct".into();
        model.materials[0].young = 200_000.0;
        model.materials[0].fy = Some(200.0);
        (model, data)
    }

    #[test]
    fn panel_761_actual_dimensions_control_public_axial_ratio() {
        let mut observations = vec![];
        for (length, area, shortening) in [
            (0.5, 0.001, 0.00025),
            (0.5, 100.0, 0.00025),
            (3000.0, 0.001, 1.5),
            (3000.0, 100.0, 1.5),
            (3000.0, 36_000.0, 1.5),
        ] {
            let (model, data) = panel_761_model(length, area);
            let mut panel = PanelZone::new_nonlinear(&data, &model);
            assert_eq!(panel.resolution_error, None);
            let mut increment = LocalVec {
                data: SmallVec::from_elem(0.0, 14),
            };
            increment.data[10] = -shortening;
            panel
                .update_state(&increment, true, &Ctx { model: &model })
                .unwrap();
            println!(
                "L={length}, A={area}: n={:?}, reduction={:?}",
                panel.axial_ratio(),
                std::array::from_fn::<_, 2, _>(|i| panel.yield_moment()[i] / panel.pmy0[i])
            );
            observations.push((panel.axial_ratio(), panel.yield_moment()[0] / panel.pmy0[0]));
        }
        for (ratios, reduction) in observations {
            assert!((ratios[0] - 0.5).abs() < 1e-12);
            assert!((ratios[1] - 0.5).abs() < 1e-12);
            assert!((reduction - 0.8660254037844386).abs() < 1e-12);
        }
    }

    #[test]
    fn panel_761_scaled_history_rejects_high_axial_force_and_restores() {
        let (model, data) = panel_761_model(0.5, 0.001);
        let ctx = Ctx { model: &model };
        let mut panel = PanelZone::new_nonlinear(&data, &model);
        let mut control = PanelZone::new_nonlinear(&data, &model);
        let increment = |gamma: f64, shortening: f64| {
            let mut du = LocalVec {
                data: SmallVec::from_elem(0.0, 14),
            };
            du.data[0] = gamma;
            du.data[10] = -shortening;
            du
        };
        let initial = increment(0.01, 0.00025);
        panel.update_state(&initial, true, &ctx).unwrap();
        control.update_state(&initial, true, &ctx).unwrap();
        let committed = panel.serialize_checkpoint();
        panel
            .update_state(&increment(-0.003, 0.00005), false, &ctx)
            .unwrap();
        let checkpoint = panel.serialize_checkpoint();
        let snapshot = panel.snapshot_state();
        for commit in [false, true] {
            let reason = panel
                .update_state(&increment(0.001, 0.0003), commit, &ctx)
                .unwrap_err()
                .to_string();
            assert!(
                reason.contains("接合部 ID 0") && reason.contains("軸耐力到達"),
                "{reason}"
            );
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
        }
        for use_checkpoint in [false, true] {
            panel
                .update_state(&increment(-0.01, -0.0001), true, &ctx)
                .unwrap();
            if use_checkpoint {
                panel.deserialize_checkpoint(&checkpoint).unwrap();
            } else {
                panel.restore_state(snapshot.as_ref());
            }
            assert_eq!(panel.serialize_checkpoint(), checkpoint);
            panel.revert_state();
            assert_eq!(panel.serialize_checkpoint(), committed);
            let retry = increment(-0.003, 0.00005);
            panel.update_state(&retry, false, &ctx).unwrap();
            control.update_state(&retry, false, &ctx).unwrap();
            assert_panel_751_same_state(&panel, &control, &ctx);
            control.revert_state();
        }
    }

    #[test]
    fn panel_761_invalid_axial_inputs_and_arithmetic_have_reasons() {
        for (area, young, strength, expected) in [
            (0.0, 200000.0, 200.0, "断面積"),
            (-1.0, 200000.0, 200.0, "断面積"),
            (f64::NAN, 200000.0, 200.0, "断面積"),
            (1.0, 0.0, 200.0, "ヤング係数"),
            (1.0, f64::INFINITY, 200.0, "ヤング係数"),
            (1.0, 200000.0, -1.0, "強軸の基準強度"),
            (1.0, 200000.0, f64::NAN, "強軸の基準強度"),
            (1e304, 200000.0, 200.0, "軸剛性 EA/L"),
            (1e-320, 1e-100, 200.0, "軸剛性 EA/L"),
            (1e-200, 200000.0, 1e-200, "強軸の基準軸耐力"),
            (1e200, 1.0, 1e200, "強軸の基準軸耐力"),
        ] {
            let (mut model, _) = panel_761_model(0.5, 0.001);
            model.sections[1].area = area;
            model.materials[0].young = young;
            let error = column_axial(
                &model,
                &model.elements[2],
                &model.materials[0],
                &model.sections[1],
                [strength; 2],
            )
            .unwrap_err();
            assert!(error.contains(expected), "{error}");
        }
        let (mut model, data) = panel_761_model(0.5, 0.001);
        model.sections[1].area = f64::MIN_POSITIVE;
        for mut panel in [
            PanelZone::new(&data, &model),
            PanelZone::new_nonlinear(&data, &model),
        ] {
            assert_eq!(panel.resolution_error, None);
            panel
                .update_state(
                    &LocalVec {
                        data: SmallVec::from_elem(0.0, panel.n_dof()),
                    },
                    false,
                    &Ctx { model: &model },
                )
                .unwrap();
        }
        model.sections[1].area = 1e304;
        for mut panel in [
            PanelZone::new(&data, &model),
            PanelZone::new_nonlinear(&data, &model),
        ] {
            let reason = panel.resolution_error.as_ref().unwrap();
            assert!(
                reason.contains("接合部 ID 0")
                    && reason.contains("柱 ID 2")
                    && reason.contains("軸剛性"),
                "{reason}"
            );
            let before = panel.serialize_checkpoint();
            assert!(panel
                .update_state(
                    &LocalVec {
                        data: SmallVec::from_elem(0.0, panel.n_dof())
                    },
                    false,
                    &Ctx { model: &model }
                )
                .is_err());
            assert_eq!(panel.serialize_checkpoint(), before);
        }
    }
}
