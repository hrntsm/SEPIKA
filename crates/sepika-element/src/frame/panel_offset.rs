//! 仕口パネルへ接合する部材の適合。
//!
//! 全体系基底の係数 `γ` を柱直交面へ投影し、節点回転へ加える変換を部材へ被せる。
//! 水平材は `ζ = −0.5`、鉛直材は `ζ = +0.5`。水平材と鉛直材のみを対象とする。

use crate::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use sepika_core::dof::DofMap;
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::panel_zone::{
    is_horizontal_or_vertical_member, member_orientation, resolve_explicit_panel_analysis_joint,
    MemberOrientation,
};
use smallvec::SmallVec;

/// 水平材（はり）が仕口パネルへ接合するときの ζ。
const ZETA_BEAM: f64 = -0.5;
/// 鉛直材（柱）が仕口パネルへ接合するときの ζ。
const ZETA_COLUMN: f64 = 0.5;

/// 部材の一方の端が仕口パネルへ接合することを表す。
#[derive(Clone, Copy, Debug)]
pub struct PanelEnd {
    /// パネルが設けられた節点（追加自由度 `γX`・`γY` の持ち主）。
    pub node: NodeId,
    /// 追加自由度を全体座標の回転へ写す行列。梁は柱直交面への投影の `−0.5` 倍、柱は `+0.5` 倍。
    pub rotation: [[f64; 2]; 3],
}

/// 各端の仕口パネルへの接続を調べ、採用柱の直交面への投影を含む回転変換を返す。
///
/// どちらの端もパネルへ接合しない場合は `None`。
pub fn resolve(data: &ElementData, model: &Model) -> Option<[Option<PanelEnd>; 2]> {
    if !data.kind.is_primary_frame_support() || data.nodes.len() < 2 {
        return None;
    }
    if !is_horizontal_or_vertical_member(model, data) {
        return None;
    }
    let zeta = match member_orientation(model, data)? {
        MemberOrientation::Column => ZETA_COLUMN,
        MemberOrientation::Beam => ZETA_BEAM,
    };

    let panel_end = |nid: NodeId| {
        if !model.elements.iter().any(|element| {
            matches!(element.kind, ElementKind::PanelZone) && element.nodes.first() == Some(&nid)
        }) {
            return None;
        }
        let joint = resolve_explicit_panel_analysis_joint(model, nid, &model.elements).ok()??;
        let axes = joint.moment_axes;
        let rotation = std::array::from_fn(|row| {
            std::array::from_fn(|col| {
                zeta * (axes[0][row] * axes[0][col] + axes[1][row] * axes[1][col])
            })
        });
        Some(PanelEnd {
            node: nid,
            rotation,
        })
    };
    let ends = [panel_end(data.nodes[0]), panel_end(data.nodes[1])];
    if ends[0].is_none() && ends[1].is_none() {
        return None;
    }
    Some(ends)
}

/// 仕口パネルへ接合する部材。
///
/// 自由度の並びは `[内側の 12 自由度, (γX, γY)_i?, (γX, γY)_j?]`。
/// `γ` は全体系 X/Y 基底を柱直交面へ投影する前の係数で、鉛直柱以外では全体回転 X/Y と異なる。
pub struct PanelOffsetMember {
    inner: Box<dyn ElementBehavior>,
    ends: [Option<PanelEnd>; 2],
}

impl PanelOffsetMember {
    /// `inner` は 12 自由度（節点 2 × 6）の部材要素であること。
    pub fn new(inner: Box<dyn ElementBehavior>, ends: [Option<PanelEnd>; 2]) -> Self {
        debug_assert_eq!(inner.n_dof(), 12, "仕口パネルを被せる要素は 12 自由度");
        Self { inner, ends }
    }

    /// パネル自由度の個数（0・2・4）。
    fn n_panel_dof(&self) -> usize {
        self.ends.iter().filter(|e| e.is_some()).count() * 2
    }

    /// `C`（12 × パネル自由度数）の非零成分を `(内側自由度, パネル自由度, 係数)` で列挙する。
    fn coupling(&self) -> SmallVec<[(usize, usize, f64); 12]> {
        let mut out = SmallVec::new();
        let mut col = 12;
        for (side, end) in self.ends.iter().enumerate() {
            let Some(end) = end else { continue };
            let rot0 = if side == 0 { 3 } else { 9 };
            for row in 0..3 {
                for axis in 0..2 {
                    let coefficient = end.rotation[row][axis];
                    if coefficient != 0.0 {
                        out.push((rot0 + row, col + axis, coefficient));
                    }
                }
            }
            col += 2;
        }
        out
    }

    /// 要素自由度の変位を内側 12 自由度の変位へ写す（`u_inner = T · u_elem`）。
    fn to_inner(&self, u: &[f64]) -> [f64; 12] {
        let mut inner = [0.0; 12];
        for (i, slot) in inner.iter_mut().enumerate() {
            *slot = u.get(i).copied().unwrap_or(0.0);
        }
        for (r, c, z) in self.coupling() {
            inner[r] += z * u.get(c).copied().unwrap_or(0.0);
        }
        inner
    }

    /// 内側 12×12 の行列を要素自由度へ写す（`K_elem = Tᵀ · K_inner · T`）。
    fn transform_matrix(&self, k: &LocalMat) -> LocalMat {
        let n = self.n_dof();
        let mut out = LocalMat::zeros(n);
        for i in 0..12 {
            for j in 0..12 {
                out.set(i, j, k.get(i, j));
            }
        }
        let cpl = self.coupling();
        for &(r, c, z) in &cpl {
            for i in 0..12 {
                let v = out.get(i, c) + k.get(i, r) * z;
                out.set(i, c, v);
                let v = out.get(c, i) + z * k.get(r, i);
                out.set(c, i, v);
            }
        }
        for &(r1, c1, z1) in &cpl {
            for &(r2, c2, z2) in &cpl {
                let v = out.get(c1, c2) + z1 * k.get(r1, r2) * z2;
                out.set(c1, c2, v);
            }
        }
        out
    }
}

crate::behavior::forward_element_behavior!(PanelOffsetMember, inner, {
    n_dof: custom,
    global_dofs: custom,
    tangent_stiffness: custom,
    internal_force: custom,
    update_state: custom,
    update_state_with_member_loads: custom,
    mass_matrix: custom,
    try_mass_matrix: custom,
    owns_frame_member_loads: forward,
    frame_load_response: custom,
    state_frame_load_response: custom,
    recover_forces: custom,
    state_member_forces: forward,
    geometric_stiffness: custom,
    snapshot_state: forward,
    restore_state: forward,
    commit_state: forward,
    revert_state: forward,
    serialize_checkpoint: forward,
    deserialize_checkpoint: forward,
    panel_moments_from: forward,
    ductility_probe: forward,
    fiber_section_states: forward,
    end_spring_rotations: forward,
    set_time_step: forward,
}, custom {
    fn frame_load_response(&self, u_elem: &[f64], loads: &[sepika_core::model::MemberLoad]) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        if u_elem.len() != self.n_dof() || u_elem.iter().any(|v| !v.is_finite()) {
            return Err("仕口パネル接続部材の荷重応答変位が不正です".into());
        }
        let Some(mut response) = self.inner.frame_load_response(&self.to_inner(u_elem), loads)? else {
            return if self.owns_frame_member_loads() {
                Err("部材荷重所有要素が応答を返しませんでした".into())
            } else { Ok(None) };
        };
        if response.nodal_residual.data.len() != 12 {
            return Err("仕口パネル接続部材の内側荷重応答は12自由度が必要です".into());
        }
        response.nodal_residual.data.resize(self.n_dof(), 0.0);
        for (r, c, z) in self.coupling() {
            response.nodal_residual.data[c] += z * response.nodal_residual.data[r];
        }
        if response.nodal_residual.data.iter().any(|v| !v.is_finite()) {
            return Err("仕口パネル接続部材の荷重応答が非有限です".into());
        }
        Ok(Some(response))
    }

    fn state_frame_load_response(&self) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        let Some(mut response) = self.inner.state_frame_load_response()? else {
            return if self.owns_frame_member_loads() {
                Err("部材荷重所有要素が状態応答を返しませんでした".into())
            } else { Ok(None) };
        };
        if response.nodal_residual.data.len() != 12 {
            return Err("仕口パネル接続部材の内側荷重応答は12自由度が必要です".into());
        }
        response.nodal_residual.data.resize(self.n_dof(), 0.0);
        for (r, c, z) in self.coupling() {
            response.nodal_residual.data[c] += z * response.nodal_residual.data[r];
        }
        if response.nodal_residual.data.iter().any(|v| !v.is_finite()) {
            return Err("仕口パネル接続部材の荷重応答が非有限です".into());
        }
        Ok(Some(response))
    }

    fn update_state_with_member_loads(&mut self, du: &LocalVec, loads: &[sepika_core::model::MemberLoad], commit: bool, ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let du_inner = LocalVec { data: SmallVec::from_slice(&self.to_inner(&du.data)) };
        let previous = self.snapshot_state();
        let result = self.inner.update_state_with_member_loads(&du_inner, loads, false, ctx)
            .and_then(|()| self.state_frame_load_response().map(|_| ()).map_err(crate::behavior::ElementUpdateError));
        if let Err(error) = result {
            self.restore_state(previous.as_ref());
            return Err(error);
        }
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn n_dof(&self) -> usize {
        12 + self.n_panel_dof()
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        let mut gdofs = self.inner.global_dofs(dof);
        for end in self.ends.iter().flatten() {
            crate::behavior::push_panel_global_dofs(&mut gdofs, end.node.index(), dof);
        }
        gdofs
    }

    fn tangent_stiffness(&self, ctx: &Ctx) -> LocalMat {
        self.transform_matrix(&self.inner.tangent_stiffness(ctx))
    }

    fn geometric_stiffness(&self, n: f64) -> LocalMat {
        self.transform_matrix(&self.inner.geometric_stiffness(n))
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        self.try_mass_matrix(opt).unwrap_or_else(|error| panic!("{error}"))
    }

    fn try_mass_matrix(&self, opt: MassOption) -> Result<LocalMat, String> {
        Ok(self.transform_matrix(&self.inner.try_mass_matrix(opt)?))
    }

    fn internal_force(&self, ctx: &Ctx) -> LocalVec {
        let f_inner = self.inner.internal_force(ctx);
        let mut f = LocalVec {
            data: SmallVec::from_elem(0.0, self.n_dof()),
        };
        for (i, v) in f_inner.data.iter().take(12).enumerate() {
            f.data[i] = *v;
        }
        for (r, c, z) in self.coupling() {
            f.data[c] += z * f_inner.data.get(r).copied().unwrap_or(0.0);
        }
        f
    }

    fn update_state(&mut self, du: &LocalVec, commit: bool, ctx: &Ctx) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let inner = self.to_inner(&du.data);
        let du_inner = LocalVec {
            data: SmallVec::from_slice(&inner),
        };
        let previous = self.snapshot_state();
        let result = self.inner.update_state(&du_inner, false, ctx)
            .and_then(|()| self.state_frame_load_response().map(|_| ()).map_err(crate::behavior::ElementUpdateError));
        if let Err(error) = result {
            self.restore_state(previous.as_ref());
            return Err(error);
        }
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn recover_forces(&self, u_elem: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        self.inner.recover_forces(&self.to_inner(u_elem))
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, SectionId};
    use sepika_core::model::{ElementKind, MaterialCategory};
    use sepika_core::model::{
        EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Material, Node, RigidZone, Section,
    };
    use sepika_core::section_shape::SectionShape;

    /// 柱 1 本＋梁 1 本、接合部（節点 0）に仕口パネルを持つモデル。
    fn model_with_panel(offset: f64) -> Model {
        let node = |id: u32, coord: [f64; 3]| Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let sec = Section {
            frame_use: Some(FrameSectionUse::Girder),
            id: SectionId(0),
            name: String::new(),
            area: 1.0e4,
            iy: 1.0e8,
            iz: 1.0e8,
            j: 1.0e8,
            depth: 400.0,
            width: 400.0,
            as_y: 5.0e3,
            as_z: 5.0e3,
            floor: None,
            panel_thickness: None,
            thickness: None,
            shape: Some(SectionShape::SteelH {
                root_r: Some(0.0),
                height: 400.0,
                width: 400.0,
                web_thick: 13.0,
                flange_thick: 21.0,
            }),
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        };
        let member = |id: u32, n0: u32, n1: u32, rigid: RigidZone| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
            section: Some(if n0 == 2 || n1 == 2 {
                SectionId(1)
            } else {
                SectionId(0)
            }),
            local_axis: LocalAxis {
                ref_vector: if n0 == 2 || n1 == 2 {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, 0.0, 1.0]
                },
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: rigid,
            plastic_zone: None,
            spring: None,
        };
        let rigid = RigidZone {
            length_i: offset,
            length_j: offset,
            face_i: Some(offset * 2.0),
            face_j: Some(offset * 2.0),
            ..Default::default()
        };
        Model {
            nodes: vec![
                node(0, [0.0, 0.0, 3000.0]),
                node(1, [6000.0, 0.0, 3000.0]),
                node(2, [0.0, 0.0, 0.0]),
            ],
            sections: vec![
                sec.clone(),
                Section {
                    frame_use: Some(FrameSectionUse::Column),
                    id: SectionId(1),
                    ..sec
                },
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
                member(0, 0, 1, rigid),
                member(1, 2, 0, rigid),
                ElementData {
                    id: ElemId(2),
                    kind: ElementKind::PanelZone,
                    nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2)],
                    section: None,
                    local_axis: LocalAxis {
                        ref_vector: [0.0, 1.0, 0.0],
                    },
                    end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                    force_regime: ForceRegime::Auto,
                    rigid_zone: Default::default(),
                    plastic_zone: None,
                    spring: None,
                },
            ],
            ..Default::default()
        }
    }

    /// 水平材は ζ = −0.5、鉛直材は ζ = +0.5。パネルが付く端だけが対象になる。
    #[test]
    fn test_resolve_assigns_zeta_by_member_direction() {
        let model = model_with_panel(200.0);
        let beam_ends = resolve(&model.elements[0], &model).expect("梁の i 端がパネル");
        assert_eq!(
            beam_ends[0].map(|e| e.rotation),
            Some([[-0.5, 0.0], [0.0, -0.5], [0.0, 0.0]])
        );
        assert!(beam_ends[1].is_none(), "j 端（節点 1）にパネルはない");

        let col_ends = resolve(&model.elements[1], &model).expect("柱の j 端がパネル");
        assert!(col_ends[0].is_none(), "i 端（節点 2）にパネルはない");
        assert_eq!(
            col_ends[1].map(|e| e.rotation),
            Some([[0.5, 0.0], [0.0, 0.5], [0.0, 0.0]])
        );
    }

    /// `resolve` は危険断面位置（`face_i`/`face_j`）を一切参照しない。
    /// 危険断面位置は将来任意位置を取りうるため、パネルの接合位置とは独立させる。
    #[test]
    fn test_resolve_is_independent_of_face_distance() {
        let base = model_with_panel(200.0);
        let ends = resolve(&base.elements[0], &base).expect("梁");

        let mut moved = base.clone();
        for e in &mut moved.elements {
            e.rigid_zone.face_i = Some(9999.0);
            e.rigid_zone.face_j = Some(0.0);
        }
        let moved_ends = resolve(&moved.elements[0], &moved).expect("梁");
        assert_eq!(
            ends[0].map(|e| e.rotation),
            moved_ends[0].map(|e| e.rotation),
            "危険断面位置を動かしても接合面の ζ は変わらない"
        );
        assert!(moved_ends[1].is_none());
    }

    /// パネルが 1 つもないモデルでは `None` を返す。
    #[test]
    fn test_resolve_returns_none_without_panel() {
        let mut model = model_with_panel(200.0);
        model
            .elements
            .retain(|e| !matches!(e.kind, ElementKind::PanelZone));
        assert!(resolve(&model.elements[0], &model).is_none());
    }

    /// 斜材はオフセット・ζ が資料で定義されないため対象外とする。
    #[test]
    fn test_resolve_skips_diagonal_member() {
        let mut model = model_with_panel(200.0);
        model.nodes[1].coord = [4000.0, 0.0, 6000.0];
        assert!(resolve(&model.elements[0], &model).is_none());
    }

    /// 変換 `T = [I | C]` の性質:
    /// - パネル自由度を 0 に固定すれば、剛性・内力は内側 12 自由度と完全に一致する
    /// - 対称性が保たれる（合同変換 Tᵀ K T）
    #[test]
    fn test_transform_preserves_inner_block_and_symmetry() {
        let model = model_with_panel(200.0);
        let ctx = Ctx { model: &model };
        let ends = resolve(&model.elements[0], &model).expect("梁");
        let inner = crate::frame::beam::BeamElement::new(&model.elements[0], &model);
        let k_inner = inner.tangent_stiffness(&ctx);
        let wrapped = PanelOffsetMember::new(Box::new(inner), ends);

        assert_eq!(wrapped.n_dof(), 14, "12 ＋ パネル 2");
        let k = wrapped.tangent_stiffness(&ctx);

        for i in 0..12 {
            for j in 0..12 {
                assert!(
                    (k.get(i, j) - k_inner.get(i, j)).abs()
                        <= 1e-6 * k_inner.get(i, j).abs().max(1.0),
                    "({i},{j}) が内側と一致しない"
                );
            }
        }
        for i in 0..14 {
            for j in 0..14 {
                let (a, b) = (k.get(i, j), k.get(j, i));
                assert!(
                    (a - b).abs() <= 1e-6 * a.abs().max(1.0),
                    "非対称 ({i},{j}): {a} vs {b}"
                );
            }
        }
    }

    /// パネル自由度 γ に単位変形を与えると、内側の部材は節点回転 ζ·γ を受ける
    /// （`{φ} = {Φ} + [Bp]{S}`）。梁は −0.5·γ、柱は +0.5·γ。
    #[test]
    fn test_panel_rotation_enters_member_end_rotation() {
        let model = model_with_panel(200.0);
        let ends = resolve(&model.elements[0], &model).expect("梁");
        let inner = crate::frame::beam::BeamElement::new(&model.elements[0], &model);
        let wrapped = PanelOffsetMember::new(Box::new(inner), ends);

        let mut u = vec![0.0; wrapped.n_dof()];
        u[12] = 1.0;
        let inner_u = wrapped.to_inner(&u);
        assert!((inner_u[3] - ZETA_BEAM).abs() < 1e-12, "ΘX へ ζ·γX");
        assert_eq!(inner_u[4], 0.0);

        let mut v = vec![0.0; wrapped.n_dof()];
        v[13] = 1.0;
        let inner_v = wrapped.to_inner(&v);
        assert!((inner_v[4] - ZETA_BEAM).abs() < 1e-12, "ΘY へ ζ·γY");
        assert_eq!(inner_v[3], 0.0);
    }

    /// 内力は `f_elem = Tᵀ · f_inner`。パネル自由度には ζ×（節点モーメント）が集まる。
    /// これがパネル要素の内力と釣り合うことで資料 (2.10.3-3) が満たされる。
    #[test]
    fn test_internal_force_collects_panel_moment() {
        let model = model_with_panel(200.0);
        let ctx = Ctx { model: &model };
        let ends = resolve(&model.elements[0], &model).expect("梁");
        let inner = crate::frame::beam::BeamElement::new(&model.elements[0], &model);
        let mut wrapped = PanelOffsetMember::new(Box::new(inner), ends);

        let mut du = LocalVec {
            data: SmallVec::from_elem(0.0, wrapped.n_dof()),
        };
        du.data[11] = 1.0e-4;
        wrapped.update_state(&du, true, &ctx).unwrap();

        let f = wrapped.internal_force(&ctx);
        assert!((f.data[12] - ZETA_BEAM * f.data[3]).abs() <= 1e-6 * f.data[3].abs().max(1.0));
        assert!((f.data[13] - ZETA_BEAM * f.data[4]).abs() <= 1e-6 * f.data[4].abs().max(1.0));
    }

    #[test]
    fn panel_757_sloped_column_connections_project_into_the_same_physical_plane() {
        let normals = [
            [0.6, 0.0, 0.8],
            [0.6 * 0.99_f64.sqrt(), 0.8 * 0.99_f64.sqrt(), 0.1],
            [0.6 * 0.19_f64.sqrt(), 0.8 * 0.19_f64.sqrt(), 0.9],
        ];
        for normal in normals {
            let mut model = model_with_panel(200.0);
            model.nodes[2].coord = [
                -3_000.0 * normal[0],
                -3_000.0 * normal[1],
                3_000.0 - 3_000.0 * normal[2],
            ];
            model.materials[0].density = 7.85e-9;
            let ctx = Ctx { model: &model };
            for (member_index, end_index, zeta) in [(0, 0, -0.5), (1, 1, 0.5)] {
                let data = &model.elements[member_index];
                let ends = resolve(data, &model).expect("既存の柱傾斜適用域");
                let rotation = ends[end_index].unwrap().rotation;
                for row in 0..3 {
                    for col in 0..2 {
                        let expected = zeta
                            * ((if row == col { 1.0 } else { 0.0 }) - normal[row] * normal[col]);
                        assert!((rotation[row][col] - expected).abs() < 1e-14);
                    }
                }
                for axis in 0..2 {
                    let axial_rotation: f64 =
                        (0..3).map(|row| normal[row] * rotation[row][axis]).sum();
                    assert!(axial_rotation.abs() < 1e-14);
                }
                if normal == [0.6, 0.0, 0.8] {
                    assert!((rotation[0][0] / zeta - 0.64).abs() < 1e-14);
                    assert!((rotation[2][0] / zeta + 0.48).abs() < 1e-14);
                }
                let mut control = crate::frame::beam::BeamElement::new(data, &model);
                let inner = crate::frame::beam::BeamElement::new(data, &model);
                let mut wrapped = PanelOffsetMember::new(Box::new(inner), ends);
                let increment = LocalVec {
                    data: smallvec::smallvec![
                        0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001, -0.002
                    ],
                };
                let axial_input = normal[0] * 0.001 - normal[1] * 0.002;
                let mut expected_inner = [0.0; 12];
                let first_rotation = 6 * end_index + 3;
                for row in 0..3 {
                    let input = [0.001, -0.002, 0.0][row];
                    expected_inner[first_rotation + row] =
                        zeta * (input - normal[row] * axial_input);
                }
                control
                    .update_state(
                        &LocalVec {
                            data: SmallVec::from_slice(&expected_inner),
                        },
                        false,
                        &ctx,
                    )
                    .unwrap();
                wrapped.update_state(&increment, false, &ctx).unwrap();
                let force = wrapped.internal_force(&ctx);
                let control_force = control.internal_force(&ctx);
                for (actual, expected) in force.data[..12].iter().zip(control_force.data.iter()) {
                    assert!((actual - expected).abs() < 1e-10 * expected.abs().max(1.0));
                }
                let work: f64 = force
                    .data
                    .iter()
                    .zip(increment.data.iter())
                    .map(|(force, displacement)| force * displacement)
                    .sum();
                let inner_work: f64 = control_force
                    .data
                    .iter()
                    .zip(expected_inner.iter())
                    .map(|(force, displacement)| force * displacement)
                    .sum();
                assert!((work - inner_work).abs() < 1e-10 * inner_work.abs().max(1.0));
                let actual_sections = wrapped.recover_forces(&increment.data).unwrap();
                let expected_sections = control.recover_forces(&expected_inner);
                let local_stiffness = control.local_stiffness();
                let input_scale = increment
                    .data
                    .iter()
                    .map(|value| value.abs())
                    .fold(0.0, f64::max);
                let force_roundoff: [f64; 12] = std::array::from_fn(|row| {
                    64.0 * f64::EPSILON
                        * input_scale
                        * (0..12)
                            .map(|col| local_stiffness.get(row, col).abs())
                            .sum::<f64>()
                });
                for ((actual_x, actual), (expected_x, expected)) in
                    actual_sections.at.iter().zip(expected_sections.at.iter())
                {
                    assert_eq!(actual_x, expected_x);
                    let first = if *actual_x < 0.5 { 0 } else { 6 };
                    let section_roundoff = [
                        force_roundoff[first],
                        force_roundoff[first + 1],
                        force_roundoff[first + 2],
                        force_roundoff[first + 3],
                        force_roundoff[first + 4] + control.length * force_roundoff[first + 2],
                        force_roundoff[first + 5] + control.length * force_roundoff[first + 1],
                    ];
                    for (component, (actual, expected)) in
                        actual.iter().zip(expected.iter()).enumerate()
                    {
                        assert!((actual - expected).abs() <= 1e-10 * expected.abs() + section_roundoff[component],
                            "normal={normal:?} member={member_index} xi={actual_x} component={component} actual={actual} expected={expected}");
                    }
                }
                let matrices = [
                    (
                        wrapped.tangent_stiffness(&ctx),
                        control.tangent_stiffness(&ctx),
                    ),
                    (
                        wrapped.geometric_stiffness(200.0),
                        control.geometric_stiffness(200.0),
                    ),
                    (
                        wrapped.mass_matrix(MassOption::Consistent),
                        control.mass_matrix(MassOption::Consistent),
                    ),
                    (
                        wrapped.mass_matrix(MassOption::Lumped),
                        control.mass_matrix(MassOption::Lumped),
                    ),
                ];
                for (actual, expected) in matrices {
                    let mut energy = 0.0;
                    let mut inner_energy = 0.0;
                    for i in 0..wrapped.n_dof() {
                        for j in 0..wrapped.n_dof() {
                            energy += increment.data[i] * actual.get(i, j) * increment.data[j];
                        }
                    }
                    for i in 0..12 {
                        for j in 0..12 {
                            inner_energy +=
                                expected_inner[i] * expected.get(i, j) * expected_inner[j];
                        }
                    }
                    assert!((energy - inner_energy).abs() < 1e-10 * inner_energy.abs().max(1.0));
                }
            }
        }
    }

    #[test]
    fn panel_757_loaded_residual_and_state_use_the_same_sloped_projection() {
        use crate::wall::side_column::{InPlaneReleasedColumn, ReleaseAxis};
        use sepika_core::model::{MemberLoad, MemberLoadKind};

        let mut model = model_with_panel(200.0);
        model.nodes[2].coord = [-1_800.0, 0.0, 600.0];
        let data = &model.elements[1];
        let ctx = Ctx { model: &model };
        let make_inner = || {
            InPlaneReleasedColumn::new(
                crate::frame::beam::BeamElement::new(data, &model),
                ReleaseAxis::LocalY,
            )
        };
        let mut control = make_inner();
        let ends = resolve(data, &model).unwrap();
        let mut wrapped = PanelOffsetMember::new(Box::new(make_inner()), ends);
        let displacement = LocalVec {
            data: smallvec::smallvec![
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.001, -0.002
            ],
        };
        let mut inner_displacement = [0.0; 12];
        inner_displacement[9] = 0.00032;
        inner_displacement[10] = -0.001;
        inner_displacement[11] = -0.00024;
        let loads = [MemberLoad::manual(
            data.id,
            [0.0, 1.0, -1.0],
            MemberLoadKind::Point {
                a: 1_000.0,
                p: 10_000.0,
            },
        )];
        let expected_query = control
            .frame_load_response(&inner_displacement, &loads)
            .unwrap()
            .unwrap();
        let query = wrapped
            .frame_load_response(&displacement.data, &loads)
            .unwrap()
            .unwrap();
        let expected_generalized = [
            0.32 * expected_query.nodal_residual.data[9]
                - 0.24 * expected_query.nodal_residual.data[11],
            0.5 * expected_query.nodal_residual.data[10],
        ];
        for (actual, expected) in query.nodal_residual.data[12..]
            .iter()
            .zip(expected_generalized)
        {
            assert!((actual - expected).abs() < 1e-10 * expected.abs().max(1.0));
        }
        let work = displacement.data[12] * query.nodal_residual.data[12]
            + displacement.data[13] * query.nodal_residual.data[13];
        let expected_work: f64 = inner_displacement
            .iter()
            .zip(expected_query.nodal_residual.data.iter())
            .map(|(displacement, force)| displacement * force)
            .sum();
        assert!((work - expected_work).abs() < 1e-10 * expected_work.abs().max(1.0));
        control
            .update_state_with_member_loads(
                &LocalVec {
                    data: SmallVec::from_slice(&inner_displacement),
                },
                &loads,
                true,
                &ctx,
            )
            .unwrap();
        wrapped
            .update_state_with_member_loads(&displacement, &loads, true, &ctx)
            .unwrap();
        let state = wrapped.state_frame_load_response().unwrap().unwrap();
        let expected_state = control.state_frame_load_response().unwrap().unwrap();
        for (actual, expected) in state.nodal_residual.data[..12]
            .iter()
            .zip(expected_state.nodal_residual.data.iter())
        {
            assert!((actual - expected).abs() < 1e-10 * expected.abs().max(1.0));
        }
        for (actual, expected) in state
            .nodal_residual
            .data
            .iter()
            .zip(query.nodal_residual.data.iter())
        {
            assert!((actual - expected).abs() < 1e-10 * expected.abs().max(1.0));
        }
        let checkpoint = wrapped.serialize_checkpoint();
        for commit in [false, true] {
            let mut invalid_loads = loads.clone();
            invalid_loads[0].kind = MemberLoadKind::Point {
                a: 1_000.0,
                p: f64::NAN,
            };
            assert!(wrapped
                .update_state_with_member_loads(&displacement, &invalid_loads, commit, &ctx)
                .is_err());
            assert_eq!(wrapped.serialize_checkpoint(), checkpoint);
        }
    }
}
