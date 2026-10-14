//! 線形静的バッチの壁脚共通切断面。

use sepika_core::geom::vec3::{cross, dot, sub, unit};
use sepika_core::ids::ElemId;
use sepika_core::model::{ElementData, ElementKind, EndCondition, Model, SupportMemberId};
use sepika_core::section_shape::SectionShape;

type ForcesAt<'a> = &'a [(f64, [f64; 6])];

/// 同じ壁脚faceの壁全体内力。Nは圧縮正 `[N]`、Qは局所面内 `[N]`、Mは `[Nmm]`。
/// 共通壁軸はxが下→上、yが下辺a→b、z=x×yで、Qはy成分、Mはz成分。
#[derive(Clone, Copy, Debug)]
pub struct WallFaceForces {
    pub compression_n: f64,
    pub shear_n: f64,
    pub moment_nmm: f64,
    pub physical_area_mm2: f64,
    pub depth_mm: f64,
}

/// 同質正方形RC側柱を持つ単独無開口鉛直壁の共通faceを解決する。
pub struct StaticWallFace {
    wall: ElemId,
    columns: [ElemId; 2],
    column_positions: [f64; 2],
    column_signs: [f64; 2],
    column_axes: [[[f64; 3]; 3]; 2],
    y: [f64; 3],
    z: [f64; 3],
    face_mm: f64,
    arms_mm: [f64; 2],
    pub physical_area_mm2: f64,
    pub depth_mm: f64,
}

impl StaticWallFace {
    /// 内力バッチと同じ検証・壁展開済みModelの壁を渡す。
    /// 共通face・対応側柱の条件が未解決の場合、壁ID付きのErrを返す。
    pub fn new(wall: &ElementData, model: &Model) -> Result<Self, String> {
        let fail = |reason: &str| format!("壁 ID {}: {reason}", wall.id.0);
        let g = sepika_core::model::wall_element_geometry(wall, model)
            .ok_or_else(|| fail("壁幾何が不明です"))?;
        if g.ex_bottom[2].abs() > 1e-9
            || unit(sub(g.top_center, g.bottom_center)).is_none_or(|x| x[2] < 1.0 - 1e-9)
            || (g.lw_bottom - g.lw_top).abs() > 1e-9 * g.lw
        {
            return Err(fail("矩形鉛直壁以外は未対応です"));
        }
        if model
            .wall_attrs
            .iter()
            .any(|a| a.elem == wall.id && a.total_opening_area_for(model.multi_opening_mode) != 0.0)
        {
            return Err(fail("開口壁は未対応です"));
        }
        let beams: Vec<_> = model
            .elements
            .iter()
            .filter(|e| {
                e.kind == ElementKind::Beam
                    && e.nodes.len() == 2
                    && g.bottom.iter().all(|n| e.nodes.contains(n))
            })
            .collect();
        if beams.len() != 1 {
            return Err(fail("壁脚支持梁を一意に解決できません"));
        }
        if model
            .element_section(beams[0])
            .and_then(|s| s.shape.as_ref())
            .is_none()
        {
            return Err(fail("壁脚支持梁の断面形状が不足しています"));
        }
        let face_mm = sepika_core::face_distance::wall_support_face(
            model,
            SupportMemberId::Primary(beams[0].id),
            [0.0, 0.0, 1.0],
        )
        .map_err(|e| fail(&e))?;
        if !face_mm.is_finite() || face_mm < 0.0 || face_mm >= g.h / 2.0 {
            return Err(fail("壁脚支持梁のfaceが不正です"));
        }
        let mut columns = [ElemId(0); 2];
        let mut positions = [0.0; 2];
        let mut signs = [0.0; 2];
        let mut axes = [[[0.0; 3]; 3]; 2];
        let mut dimensions = [None; 2];
        let mut first_shape = None;
        for side in 0..2 {
            let edge = [g.bottom[side], g.top[side]];
            let matches: Vec<_> = model
                .elements
                .iter()
                .filter(|e| {
                    e.kind.is_primary_frame_support()
                        && e.nodes.len() == 2
                        && edge.iter().all(|n| e.nodes.contains(n))
                })
                .collect();
            if matches.len() != 1 {
                return Err(fail("必要な両側柱を一意に解決できません"));
            }
            let col = matches[0];
            let col_fail = |reason: &str| fail(&format!("側柱 ID {}: {reason}", col.id.0));
            for support in model.elements.iter().filter(|e| {
                e.kind.is_primary_frame_support()
                    && e.nodes.len() == 2
                    && e.id != col.id
                    && e.id != beams[0].id
                    && e.nodes.contains(&edge[0])
            }) {
                let other = *support
                    .nodes
                    .iter()
                    .find(|n| **n != edge[0])
                    .ok_or_else(|| col_fail("接続材が退化しています"))?;
                let delta = sub(
                    model.nodes[other.index()].coord,
                    model.nodes[edge[0].index()].coord,
                );
                if unit(delta).is_some_and(|axis| axis[2].abs() < 1e-9) {
                    let other_face = sepika_core::face_distance::wall_support_face(
                        model,
                        SupportMemberId::Primary(support.id),
                        [0.0, 0.0, 1.0],
                    )
                    .map_err(|e| col_fail(&e))?;
                    if other_face > face_mm {
                        return Err(col_fail(&format!(
                            "接続梁 ID {} の別faceが支配します",
                            support.id.0
                        )));
                    }
                } else if unit(delta).is_none_or(|axis| axis[2].abs() < 1.0 - 1e-9) {
                    return Err(col_fail("接続材のface平面・方向を同定できません"));
                }
            }
            if col.kind != ElementKind::Beam
                || col.end_cond != [EndCondition::Fixed; 2]
                || super::side_column::wall_side_column_release(col, model).is_none()
            {
                return Err(col_fail("既存の両端面内解放Beam経路以外は未対応です"));
            }
            let shared = model
                .elements
                .iter()
                .filter(|e| {
                    e.kind == ElementKind::Wall
                        && e.nodes.contains(&edge[0])
                        && e.nodes.contains(&edge[1])
                })
                .count();
            if shared != 1 {
                return Err(col_fail("共有側柱は未対応です"));
            }
            let shape = model
                .element_section(col)
                .and_then(|s| s.shape.as_ref())
                .ok_or_else(|| col_fail("断面形状が不足しています"))?;
            let SectionShape::RcColumnRect { b, d, .. } = shape else {
                return Err(col_fail("正方形RC側柱以外は未対応です"));
            };
            if b != d || first_shape.is_some_and(|s| s != shape) {
                return Err(col_fail("対称な同質正方形RC側柱以外は未対応です"));
            }
            first_shape = Some(shape);
            let mat = model
                .element_material(col)
                .ok_or_else(|| col_fail("材料が不足しています"))?;
            let wall_mat = model
                .element_material(wall)
                .ok_or_else(|| fail("壁材料が不足しています"))?;
            if mat.category != wall_mat.category
                || mat.fc != wall_mat.fc
                || mat.concrete_class != wall_mat.concrete_class
                || mat.young != wall_mat.young
                || mat.poisson != wall_mat.poisson
                || mat.shear != wall_mat.shear
            {
                return Err(col_fail("壁と同質の材料ではありません"));
            }
            let lower_i = col.nodes[0] == edge[0];
            let face = if lower_i {
                col.rigid_zone.face_i
            } else {
                col.rigid_zone.face_j
            };
            if face.is_none_or(|f| (f - face_mm).abs() > 1e-9 * g.h) {
                return Err(col_fail("共通壁脚faceと側柱faceが不一致または由来不明です"));
            }
            let frame = crate::transform::LocalFrame::from_nodes(
                model.nodes[col.nodes[0].index()].coord,
                model.nodes[col.nodes[1].index()].coord,
                col.local_axis.ref_vector,
            );
            if dot(frame.rot[1], g.ex_bottom)
                .abs()
                .max(dot(frame.rot[2], g.ex_bottom).abs())
                < 1.0 - 1e-9
            {
                return Err(col_fail("断面方向が壁面内軸に平行ではありません"));
            }
            columns[side] = col.id;
            positions[side] = if lower_i {
                face_mm / g.h
            } else {
                1.0 - face_mm / g.h
            };
            signs[side] = if lower_i { 1.0 } else { -1.0 };
            axes[side] = frame.rot;
            dimensions[side] = Some([*d, *b]);
        }
        let t = match model.element_section(wall).and_then(|s| s.shape.as_ref()) {
            Some(SectionShape::RcWall { thickness, .. }) => *thickness,
            _ => return Err(fail("RC壁断面が不足しています")),
        };
        let p =
            sepika_core::section_shape::wall_rectangular_section_properties(g.lw, t, dimensions)
                .ok_or_else(|| fail("物理断面を解決できません"))?;
        Ok(Self {
            wall: wall.id,
            columns,
            column_positions: positions,
            column_signs: signs,
            column_axes: axes,
            y: g.ex_bottom,
            z: cross([0.0, 0.0, 1.0], g.ex_bottom),
            face_mm,
            arms_mm: [-g.lw / 2.0, g.lw / 2.0],
            physical_area_mm2: p.area,
            depth_mm: g.lw + dimensions.iter().flatten().map(|d| d[0]).sum::<f64>() / 2.0,
        })
    }

    /// 壁への直接部材荷重を拒否する。側柱の軸荷重は長期N回収時だけ許す。
    pub fn ensure_loads(
        &self,
        cases: &[sepika_core::model::LoadCase],
        axial_only: bool,
    ) -> Result<(), String> {
        for load in cases.iter().flat_map(|c| &c.member) {
            if load.elem == self.wall
                || (self.columns.contains(&load.elem)
                    && (!axial_only || load.dir[0] != 0.0 || load.dir[1] != 0.0))
            {
                return Err(format!(
                    "壁 ID {}: 部材 ID {} の部材荷重は未対応です",
                    self.wall.0, load.elem.0
                ));
            }
        }
        Ok(())
    }

    /// 中央は梁心行から一度輸送し、側柱は由来を照合したface行をそのまま使う。
    pub fn recover(&self, batch: &[(ElemId, ForcesAt<'_>)]) -> Result<WallFaceForces, String> {
        let row = |id: ElemId, pos: f64| -> Result<[f64; 6], String> {
            let matches: Vec<_> = batch.iter().filter(|(eid, _)| *eid == id).collect();
            if matches.len() != 1 {
                return Err(format!(
                    "壁 ID {}: 部材 ID {} の応答不足または重複です",
                    self.wall.0, id.0
                ));
            }
            let rows: Vec<_> = matches[0]
                .1
                .iter()
                .filter(|(p, _)| (*p - pos).abs() <= 16.0 * f64::EPSILON)
                .collect();
            if rows.len() != 1 || rows[0].1.iter().any(|v| !v.is_finite()) {
                return Err(format!(
                    "壁 ID {}: 部材 ID {} の必要face応答が不足・重複・非有限です",
                    self.wall.0, id.0
                ));
            }
            Ok(rows[0].1)
        };
        let central = row(self.wall, 0.0)?;
        let mut compression_n = -central[0];
        let mut moment_nmm = central[5] + central[1] * self.face_mm;
        for side in 0..2 {
            let f = row(self.columns[side], self.column_positions[side])?;
            let axis = self.column_axes[side];
            let s = self.column_signs[side];
            let q = s * (f[1] * dot(axis[1], self.y) + f[2] * dot(axis[2], self.y));
            let m = s * (f[4] * dot(axis[1], self.z) + f[5] * dot(axis[2], self.z));
            // 縮約の丸め残差を厳密ゼロとは同一視しない。力・モーメント各スケールで照合する。
            let force_scale = central[1].abs().max(f[0].abs()).max(1.0);
            let moment_scale = central[5].abs().max(force_scale * self.depth_mm).max(1.0);
            if q.abs() > 1e-9 * force_scale || m.abs() > 1e-9 * moment_scale {
                return Err(format!(
                    "壁 ID {}: 側柱 ID {} の面内解放残差が許容範囲外です",
                    self.wall.0, self.columns[side].0
                ));
            }
            compression_n -= f[0];
            moment_nmm -= self.arms_mm[side] * f[0];
        }
        if !compression_n.is_finite() || !moment_nmm.is_finite() {
            return Err(format!("壁 ID {}: 合成内力が非有限です", self.wall.0));
        }
        Ok(WallFaceForces {
            compression_n,
            moment_nmm,
            shear_n: central[1],
            physical_area_mm2: self.physical_area_mm2,
            depth_mm: self.depth_mm,
        })
    }
}

#[cfg(test)]
#[path = "../../../../test_fixtures/rc_wall617.rs"]
pub(super) mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    type ForceRows = Vec<(f64, [f64; 6])>;
    fn rows() -> Vec<(ElemId, ForceRows)> {
        vec![
            (
                ElemId(4),
                vec![
                    (0.0, [-400_000.0, 200_000.0, 0.0, 0.0, 0.0, -1.75e9]),
                    (0.5, [-9e9, -9e9, 0.0, 0.0, 0.0, 9e15]),
                ],
            ),
            (
                ElemId(0),
                vec![(0.1, [-300_000.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
            ),
            (
                ElemId(1),
                vec![(0.1, [-500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
            ),
        ]
    }
    fn recover(model: &Model, rows: &[(ElemId, ForceRows)]) -> Result<WallFaceForces, String> {
        let batch: Vec<_> = rows
            .iter()
            .map(|(id, rows)| (*id, rows.as_slice()))
            .collect();
        StaticWallFace::new(&model.elements[4], model)?.recover(&batch)
    }
    #[test]
    fn same_face_total_forces_and_physical_area_keep_distinct_owners() {
        let state = recover(&fixture::expanded_model(), &rows()).unwrap();
        assert_eq!(state.compression_n, 1_200_000.0);
        assert_eq!(state.shear_n, 200_000.0);
        assert_eq!(state.moment_nmm, -1.29e9);
        assert_eq!(state.physical_area_mm2, 1_332_000.0);
        assert_eq!(state.depth_mm, 4600.0);
        let mut m = fixture::expanded_model();
        for (method, expected) in [
            (
                sepika_core::model::RcWallShearMethod::ArakawaMin,
                2_006_534.471365995,
            ),
            (
                sepika_core::model::RcWallShearMethod::ArakawaMean,
                2_676_420.1354640694,
            ),
        ] {
            m.stress_cfg.rc_wall_shear_method = method;
            let qu =
                super::super::wall_element::WallElement::directional_shear_capacity_at_reference(
                    &m.elements[4],
                    &m,
                    crate::factory::StrengthBasis::Nominal,
                    state.compression_n,
                    [6450.0 / 4600.0; 2],
                )
                .unwrap();
            assert!((qu[0] - expected).abs() < 1e-6);
            assert_eq!(qu[0], qu[1]);
        }
    }
    #[test]
    fn rotation_end_order_and_sign_changes_preserve_physical_state() {
        for rotation in [false, true] {
            for reversed in [false, true] {
                for sign in [-1.0, 1.0] {
                    let mut m = fixture::expanded_model();
                    let mut r = rows();
                    r[0].1[0].1[1] *= sign;
                    r[0].1[0].1[5] *= sign;
                    if sign == -1.0 {
                        r.swap(1, 2);
                        r[1].0 = ElemId(0);
                        r[2].0 = ElemId(1);
                    }
                    if reversed {
                        for c in &mut m.elements[..2] {
                            c.nodes.reverse();
                        }
                        r[1].1[0].0 = 0.9;
                        r[2].1[0].0 = 0.9;
                    }
                    if rotation {
                        for n in &mut m.nodes {
                            let [x, y, z] = n.coord;
                            n.coord = [-y, x, z];
                        }
                        for e in &mut m.elements {
                            let [x, y, z] = e.local_axis.ref_vector;
                            e.local_axis.ref_vector = [-y, x, z];
                        }
                    }
                    let state = recover(&m, &r).unwrap();
                    assert_eq!(state.moment_nmm.abs(), 1.29e9);
                    assert_eq!(state.shear_n.abs(), 200_000.0);
                    assert_eq!(state.compression_n, 1_200_000.0);
                }
            }
        }
    }
    #[test]
    fn missing_nonfinite_near_face_and_unproven_face_are_not_zero_response() {
        let m = fixture::expanded_model();
        for bad in 0..5 {
            let mut r = rows();
            match bad {
                0 => {
                    r.pop();
                }
                1 => r[1].1.clear(),
                2 => r[1].1[0].1[0] = f64::NAN,
                3 => r[1].1[0].0 += 1e-10,
                _ => r[0].1.remove(0).0 = 0.1,
            }
            assert!(recover(&m, &r).is_err());
        }
        for face in [None, Some(301.0)] {
            let mut m = m.clone();
            m.elements[0].rigid_zone.face_i = face;
            assert!(recover(&m, &rows()).is_err());
        }
        let mut zero = rows();
        for (_, r) in &mut zero {
            r[0].1[0] = 0.0;
        }
        assert_eq!(recover(&m, &zero).unwrap().compression_n, 0.0);
    }

    #[test]
    fn common_face_does_not_use_rigidity_length_or_apply_side_transport_twice() {
        let mut m = fixture::expanded_model();
        for c in &mut m.elements[..2] {
            c.rigid_zone.length_i = 900.0;
        }
        let state = recover(&m, &rows()).unwrap();
        assert_eq!(state.moment_nmm, -1.29e9);
        let mut wrong = rows();
        wrong[1].1[0].1[5] = 90_000_000.0;
        assert!(recover(&m, &wrong).unwrap_err().contains("面内解放残差"));
    }

    #[test]
    fn another_connected_beam_cannot_supply_common_wall_face() {
        let mut m = fixture::expanded_model();
        let mut node = m.nodes[1].clone();
        node.id = sepika_core::ids::NodeId(4);
        node.coord = [0.0, 4000.0, 0.0];
        m.nodes.push(node);
        let mut section = m.sections[1].clone();
        section.id = sepika_core::ids::SectionId(3);
        let Some(SectionShape::RcColumnRect { d, .. }) = &mut section.shape else {
            panic!()
        };
        *d = 1000.0;
        m.sections.push(section);
        let mut beam = m.elements[3].clone();
        beam.id = ElemId(5);
        beam.nodes = [sepika_core::ids::NodeId(0), sepika_core::ids::NodeId(4)]
            .into_iter()
            .collect();
        beam.section = Some(sepika_core::ids::SectionId(3));
        m.elements.push(beam);
        assert!(recover(&m, &rows()).unwrap_err().contains("別faceが支配"));
        m.sections[1].shape = None;
        assert!(recover(&m, &rows()).unwrap_err().contains("断面形状が不足"));
    }

    #[test]
    fn direct_factory_requires_explicit_finite_positive_run_capacity() {
        let m = fixture::expanded_model();
        for qu in [None, Some([0.0, 1.0]), Some([1.0, f64::NAN])] {
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::factory::build_nonlinear_behavior(
                    &m.elements[4],
                    &m,
                    crate::factory::StrengthBasis::Nominal,
                    sepika_core::model::AnalysisKind::TimeHistory,
                    qu,
                )
            }))
            .is_err());
        }
        for n in [-1.0, 1_332_000.0 * 24.0 * 0.4 + 1.0, f64::NAN] {
            assert!(
                super::super::wall_element::WallElement::directional_shear_capacity_at_reference(
                    &m.elements[4],
                    &m,
                    crate::factory::StrengthBasis::Nominal,
                    n,
                    [1.0; 2]
                )
                .is_err()
            );
        }
    }
}
