//! SRC耐震壁の上下の内法断面と同一状態の断面力。

use crate::transform::LocalFrame;
use sepika_core::geom::vec3::{cross, dot, sub, unit};
use sepika_core::ids::ElemId;
use sepika_core::model::{
    ElementData, ElementKind, LoadCase, Model, SupportMemberId, WallSideColumnIndex,
};
use sepika_core::src_wall_capacity::{resolve_src_wall, SrcWallCapacity, SrcWallEndForces};

type ForceRows<'a> = &'a [(f64, [f64; 6])];

struct ColumnCut {
    id: ElemId,
    positions: [f64; 2],
    orientation: f64,
    frame: LocalFrame,
    arm_mm: f64,
}

/// 内法端の水平切断面。曲げは壁全断面の図心を基準とする。
pub struct SrcWallFaces {
    wall: ElemId,
    columns: [ColumnCut; 2],
    y: [f64; 3],
    z: [f64; 3],
    wall_midpoint_arm_mm: f64,
    face_offsets_mm: [f64; 2],
}

impl SrcWallFaces {
    /// 同じ壁展開済みモデルの壁・支持梁・両側柱の上下フェースを照合する。
    pub fn new(wall: &ElementData, model: &Model) -> Result<Self, String> {
        let fail = |why: &str| format!("SRC耐震壁 ID {}: {why}", wall.id.0);
        let capacity = resolve_src_wall(model, wall)?.ok_or_else(|| fail("SRC壁に該当しません"))?;
        let geom = sepika_core::model::wall_element_geometry(wall, model)
            .ok_or_else(|| fail("壁の幾何が不明です"))?;
        let vertical = [0.0, 0.0, 1.0];
        let y = geom.ex_bottom;
        if y[2].abs() > 1e-9
            || (geom.lw_bottom - geom.lw_top).abs() > 1e-9 * geom.lw
            || unit(sub(geom.top_center, geom.bottom_center)).is_none_or(|x| x[2] < 1.0 - 1e-9)
        {
            return Err(fail("矩形鉛直壁が必要です"));
        }
        let mut face_mm = [0.0; 2];
        let mut girders = [None; 2];
        for (end, edge) in [geom.bottom, geom.top].into_iter().enumerate() {
            let mut supports = model.elements.iter().filter(|e| {
                e.kind.is_primary_frame_support()
                    && e.nodes.len() == 2
                    && edge.iter().all(|n| e.nodes.contains(n))
            });
            let support = supports
                .next()
                .filter(|_| supports.next().is_none())
                .ok_or_else(|| fail("上下の支持梁を一意に解決できません"))?;
            girders[end] = Some(support.id);
            face_mm[end] = sepika_core::face_distance::wall_support_face(
                model,
                SupportMemberId::Primary(support.id),
                [0.0, 0.0, 1.0 - 2.0 * end as f64],
            )
            .map_err(|why| fail(&why))?;
        }
        if face_mm
            .iter()
            .any(|d| !d.is_finite() || *d < 0.0 || *d >= 0.5 * geom.h)
        {
            return Err(fail("上下の内法フェース位置が無効です"));
        }
        let column_index = WallSideColumnIndex::build(model);
        let bindings = column_index
            .wall_replacement_bindings(wall.id)
            .map_err(|issues| fail(&format!("側柱の対応が不正です {issues:?}")))?;
        let mut columns = Vec::with_capacity(2);
        let centroid_mm = capacity.centroid_from_column_midpoint_mm();
        let depths = capacity.column_depths_mm();
        let panel_shift_mm = (depths[0] - depths[1]) / 4.0;
        for side in 0..2 {
            let edge = [geom.bottom[side], geom.top[side]];
            let binding = bindings
                .iter()
                .find(|b| b.side == side)
                .ok_or_else(|| fail("両側柱が必要です"))?;
            let column = model
                .element(binding.column)
                .ok_or_else(|| fail("側柱の参照がありません"))?;
            if model
                .elements
                .iter()
                .filter(|e| e.kind == ElementKind::Wall && edge.iter().all(|n| e.nodes.contains(n)))
                .count()
                != 1
            {
                return Err(fail("側柱を共有する壁の個別断面力を分離できません"));
            }
            let col_fail = |why: &str| fail(&format!("側柱 ID {} {why}", column.id.0));
            let lower_i = column.nodes[0] == edge[0];
            let zone = column.rigid_zone;
            let actual = if lower_i {
                [zone.face_i, zone.face_j]
            } else {
                [zone.face_j, zone.face_i]
            };
            for end in 0..2 {
                if actual[end]
                    .is_none_or(|d| !d.is_finite() || (d - face_mm[end]).abs() > 1e-9 * geom.h)
                {
                    return Err(col_fail("上下支持梁と同じ切断面にフェースがありません"));
                }
                for other in model.elements.iter().filter(|e| {
                    e.kind.is_primary_frame_support()
                        && e.nodes.len() == 2
                        && e.id != column.id
                        && Some(e.id) != girders[end]
                        && e.nodes.contains(&edge[end])
                }) {
                    let opposite = other.nodes[usize::from(other.nodes[0] == edge[end])];
                    let delta = sub(
                        model.nodes[opposite.index()].coord,
                        model.nodes[edge[end].index()].coord,
                    );
                    let axis = unit(delta).ok_or_else(|| col_fail("接続材の材軸が不明です"))?;
                    if axis[2].abs() < 1e-9 {
                        let other_face = sepika_core::face_distance::wall_support_face(
                            model,
                            SupportMemberId::Primary(other.id),
                            [0.0, 0.0, 1.0 - 2.0 * end as f64],
                        )
                        .map_err(|why| col_fail(&why))?;
                        if other_face > face_mm[end] {
                            return Err(col_fail("別の接続梁のフェースが支配します"));
                        }
                    } else if axis[2].abs() < 1.0 - 1e-9 {
                        return Err(col_fail("傾斜した接続材のフェースを証明できません"));
                    }
                }
            }
            let geometry = crate::transform::EndGeometry::try_of_element(column, model)
                .map_err(|why| col_fail(&why))?;
            if geometry.coords
                != [
                    model.nodes[column.nodes[0].index()].coord,
                    model.nodes[column.nodes[1].index()].coord,
                ]
            {
                return Err(col_fail(
                    "節点心と材軸端が異なる側柱の切断面を証明できません",
                ));
            }
            let frame = geometry
                .local_frame(column.local_axis.ref_vector)
                .map_err(col_fail)?;
            let fractions = if lower_i {
                [face_mm[0] / geom.h, 1.0 - face_mm[1] / geom.h]
            } else {
                [1.0 - face_mm[0] / geom.h, face_mm[1] / geom.h]
            };
            columns.push(ColumnCut {
                id: column.id,
                positions: fractions,
                orientation: if lower_i { 1.0 } else { -1.0 },
                frame,
                arm_mm: (side as f64 - 0.5) * geom.lw - centroid_mm,
            });
        }
        let [left, right]: [ColumnCut; 2] =
            columns.try_into().map_err(|_| fail("両側柱が必要です"))?;
        Ok(Self {
            wall: wall.id,
            columns: [left, right],
            y,
            z: cross(vertical, y),
            wall_midpoint_arm_mm: panel_shift_mm - centroid_mm,
            face_offsets_mm: [face_mm[0], -face_mm[1]],
        })
    }

    /// 上下断面から高さ内の応力を評価できる荷重条件を確認する。
    pub fn ensure_loads(&self, cases: &[LoadCase]) -> Result<(), String> {
        for load in cases.iter().flat_map(|case| &case.member) {
            if load.elem == self.wall || self.columns.iter().any(|column| column.id == load.elem) {
                return Err(format!(
                    "SRC耐震壁 ID {}: 部材 ID {} の直接部材荷重があるため高さ内の最大断面力を証明できません",
                    self.wall.0, load.elem.0,
                ));
            }
        }
        Ok(())
    }

    /// 指定した同一状態の側柱圧縮軸力 [N]。下端・上端順、それぞれ壁下辺a・b順。
    pub fn recover_column_compression(
        &self,
        batch: &[(ElemId, ForceRows<'_>)],
    ) -> Result<[[f64; 2]; 2], String> {
        let recover_end = |end: usize| -> Result<[f64; 2], String> {
            let [a, b] = &self.columns;
            Ok([
                -self.row(batch, a.id, a.positions[end])?[0],
                -self.row(batch, b.id, b.positions[end])?[0],
            ])
        };
        Ok([recover_end(0)?, recover_end(1)?])
    }

    /// 非線形側柱の同一状態から直接回収する位置。壁下辺a・b順、それぞれ下端・上端順。
    pub fn column_cut_positions(&self) -> [(ElemId, [f64; 2]); 2] {
        self.columns
            .each_ref()
            .map(|column| (column.id, column.positions))
    }

    /// 静的または同一確定ステップの断面内力を内法の下端・上端順に返す。
    pub fn recover(
        &self,
        batch: &[(ElemId, ForceRows<'_>)],
    ) -> Result<[SrcWallEndForces; 2], String> {
        let recover_end = |end: usize| -> Result<SrcWallEndForces, String> {
            let wall = self.row(batch, self.wall, end as f64)?;
            let mut axial_n = -wall[0];
            let mut columns_compression_n = 0.0;
            let mut shear_n = wall[1];
            let mut moment_nmm =
                wall[5] + wall[1] * self.face_offsets_mm[end] - wall[0] * self.wall_midpoint_arm_mm;
            for column in &self.columns {
                let f = self.row(batch, column.id, column.positions[end])?;
                let axes = column.frame.rot;
                let q = column.orientation
                    * (f[1] * dot(axes[1], self.y) + f[2] * dot(axes[2], self.y));
                let m = column.orientation
                    * (f[4] * dot(axes[1], self.z) + f[5] * dot(axes[2], self.z));
                columns_compression_n -= f[0];
                axial_n -= f[0];
                shear_n += q;
                moment_nmm += m - column.arm_mm * f[0];
            }
            if [axial_n, columns_compression_n, shear_n, moment_nmm]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err(format!(
                    "SRC耐震壁 ID {}: 合成断面力が非有限です",
                    self.wall.0
                ));
            }
            Ok(SrcWallEndForces {
                axial_n,
                columns_compression_n,
                shear_n,
                moment_nmm,
            })
        };
        Ok([recover_end(0)?, recover_end(1)?])
    }

    fn row(
        &self,
        batch: &[(ElemId, ForceRows<'_>)],
        id: ElemId,
        pos: f64,
    ) -> Result<[f64; 6], String> {
        let missing = || {
            format!(
                "SRC耐震壁 ID {}: 部材 ID {} の必要断面 (ξ={pos}) が不足または重複しています",
                self.wall.0, id.0
            )
        };
        let mut matches = batch.iter().filter(|(candidate, _)| *candidate == id);
        let (_, rows) = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(missing)?;
        let mut matched = rows
            .iter()
            .filter(|(x, _)| (*x - pos).abs() <= 16.0 * f64::EPSILON);
        let (_, values) = matched
            .next()
            .filter(|_| matched.next().is_none())
            .ok_or_else(missing)?;
        if !values.iter().all(|v| v.is_finite()) {
            return Err(format!(
                "SRC耐震壁 ID {}: 部材 ID {} の断面力が非有限です",
                self.wall.0, id.0
            ));
        }
        Ok(*values)
    }
}

/// 上下の内法断面から38条の同一状態のせん断耐力 [N] を返す。両端ともQ=0なら None。
pub fn src_wall_shear_strength(
    rule: &SrcWallCapacity,
    ends: [SrcWallEndForces; 2],
) -> Result<Option<f64>, String> {
    if ends.iter().any(|end| {
        [
            end.axial_n,
            end.columns_compression_n,
            end.moment_nmm,
            end.shear_n,
        ]
        .iter()
        .any(|v| !v.is_finite())
    }) {
        return Err("38条せん断照査の端部応力が非有限です".into());
    }
    let [bottom, top] = ends;
    let max_shear_n = bottom.shear_n.abs().max(top.shear_n.abs());
    if max_shear_n == 0.0 {
        return Ok(None);
    }
    let max_moment_nmm = bottom.moment_nmm.abs().max(top.moment_nmm.abs());
    let end_strength = |end| rule.ultimate_shear_with_span_n(end, max_moment_nmm, max_shear_n);
    Ok(Some(end_strength(bottom)?.min(end_strength(top)?)))
}
