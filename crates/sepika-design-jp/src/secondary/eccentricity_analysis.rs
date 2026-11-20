//! 偏心率の精算層（応力解析結果に基づく。令82条の6・昭55建告1792号）。
//!
//! [`crate::secondary::eccentricity`] のモデル抽出層（D値法の略算）に対し、本
//! モジュールは弾性応力解析結果から柱の水平剛性・長期軸力による重心を直接算定
//! する精算ルートを提供する。剛心・重心・ねじり剛性の合成自体は
//! [`crate::secondary::eccentricity`] の計算コア（`center_of_rigidity` /
//! `eccentricity` / `center_of_mass`）と雑壁剛性（`append_misc_wall_stiffnesses`）
//! をそのまま再利用する。

use sepika_core::dof::Dof;
use sepika_core::ids::{ElemId, NodeId, StoryId};
use sepika_core::model::{ElementData, ElementKind, Model};
use std::collections::HashMap;

use super::story_columns::{analysis_story_columns, story_layer, StoryColumn};
use sepika_element::frame::beam::MemberForces;
use sepika_element::transform::LocalFrame;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::StaticOnce;

use super::eccentricity::{
    append_misc_wall_stiffnesses, center_of_mass, center_of_rigidity, eccentricity,
    ColumnStiffness, Eccentricity,
};

fn actual_axis_ends(model: &Model, elem: &ElementData) -> Result<[[f64; 3]; 2], String> {
    model
        .primary_frame_axis_ends(elem)
        .map(|ends| {
            ends.unwrap_or_else(|| {
                [
                    model.nodes[elem.nodes[0].index()].coord,
                    model.nodes[elem.nodes[1].index()].coord,
                ]
            })
        })
        .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", elem.id.0))
}

fn resolved_columns(model: &Model, story: StoryId) -> Result<Vec<StoryColumn>, String> {
    let columns = analysis_story_columns(model, story)?;
    for column in &columns {
        let members = column
            .segments
            .iter()
            .map(|id| {
                model.element(*id).ok_or_else(|| {
                    format!(
                        "Re: 階{}の柱{}の分割部材{}がありません",
                        story.0, column.top_elem.0, id.0
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if members.iter().any(|elem| elem.kind != ElementKind::Beam) {
            for elem in members {
                let [p0, p1] = actual_axis_ends(model, elem)?;
                if p0[..2] != p1[..2] || p0[2] == p1[2] {
                    return Err(format!(
                        "Re: 階{}の柱{}の分割部材{}の実材軸が鉛直でないため、ファイバー柱・マルチスプリング柱を含む連なりの水平剛性を算定できません",
                        story.0, column.top_elem.0, elem.id.0
                    ));
                }
                if [p0, p1]
                    .iter()
                    .zip(&elem.nodes)
                    .any(|(end, node)| end[..2] != model.nodes[node.index()].coord[..2])
                {
                    return Err(format!(
                        "Re: 階{}の柱{}の分割部材{}の実材軸端と接続節点の平面位置が異なるため、ファイバー柱・マルチスプリング柱を含む連なりの水平剛性を算定できません",
                        story.0, column.top_elem.0, elem.id.0
                    ));
                }
            }
        }
    }
    Ok(columns)
}

fn member_start(forces: &HashMap<ElemId, &MemberForces>, elem: ElemId) -> Option<[f64; 6]> {
    let &(xi, local) = forces.get(&elem)?.at.first()?;
    (xi == 0.0).then_some(local)
}

/// 部材始端（xi=0）の局所内力 [N, Qy, Qz] を全体座標の力ベクトルへ
/// 変換して返す。`ref_vec` は要素の局所軸参照ベクトル。
fn station_force_global(
    p0: [f64; 3],
    p1: [f64; 3],
    ref_vec: [f64; 3],
    local: [f64; 6],
) -> Result<[f64; 3], &'static str> {
    let frame = LocalFrame::from_nodes(p0, p1, ref_vec)?;
    let ex = frame.rot[0];
    let ey = frame.rot[1];
    let ez = frame.rot[2];
    let (n, qy, qz) = (local[0], local[1], local[2]);
    Ok([
        n * ex[0] + qy * ey[0] + qz * ez[0],
        n * ex[1] + qy * ey[1] + qz * ez[1],
        n * ex[2] + qy * ey[2] + qz * ez[2],
    ])
}

/// 地震時応力解析結果から柱の方向別水平剛性 `ki = Qi/δi` を算定する（精算層）。
/// 剛心はその階の柱の水平方向剛性の中心であり、各柱の水平剛性は地震時応力解析
/// 結果の負担水平力と層間変位から算定する。
///
/// - `res_x` / `res_y`: X / Y 方向加力時の弾性応力解析結果
/// - `kX` は X 加力時の（負担水平力 Qx, 層間変位 δx）から、`kY` は Y 加力時から。
///   剛床のねじれによる変位の相殺、変位差0又は商の非有限は、方向・部材ID付きでErr。
///
/// 上下階SL間の区間が曖昧、必要変位・内力が欠損・非有限、又は局所基底が不正なら部材IDと理由を返す。
pub fn column_stiffnesses_from_analysis(
    model: &Model,
    story: StoryId,
    res_x: &StaticOnce,
    res_y: &StaticOnce,
) -> Result<Vec<ColumnStiffness>, String> {
    let columns = resolved_columns(model, story)?;
    let x = horizontal_stiffnesses(model, story, &columns, res_x, SeismicDir::X)?;
    let y = horizontal_stiffnesses(model, story, &columns, res_y, SeismicDir::Y)?;
    Ok(columns
        .iter()
        .zip(x)
        .zip(y)
        .map(|((column, dx), dy)| {
            let [x, y, _] = model.nodes[column.top.index()].coord;
            ColumnStiffness {
                pos: [x, y],
                dx,
                dy,
            }
        })
        .collect())
}

/// 指定方向の柱水平剛性 [N/mm] を層内柱の順序で返す。他方向の応答は要求しない。
/// 層区間の曖昧さ、剛床のねじれによる変位の相殺、変位差0、必要応答・演算の不正はErr。
pub fn column_horizontal_stiffnesses(
    model: &Model,
    story: StoryId,
    response: &StaticOnce,
    direction: SeismicDir,
) -> Result<Vec<f64>, String> {
    let columns = resolved_columns(model, story)?;
    horizontal_stiffnesses(model, story, &columns, response, direction)
}

fn layer_rotations(
    model: &Model,
    story: StoryId,
    columns: &[StoryColumn],
    response: &StaticOnce,
    direction: SeismicDir,
) -> Result<Option<[f64; 2]>, String> {
    let Some(layer) = story_layer(model, story) else {
        return Ok(None);
    };
    let dof = match direction {
        SeismicDir::X => Dof::Ux,
        SeismicDir::Y => Dof::Uy,
    };
    let single_master = |story, end: fn(&StoryColumn) -> NodeId| {
        let mut diaphragms = model.diaphragms_of(story);
        let diaphragm = diaphragms.next()?;
        if diaphragms.next().is_some()
            || !columns.iter().map(end).all(|id| {
                id == diaphragm.master
                    || (diaphragm.slaves.contains(&id)
                        && !model.nodes[id.index()].restraint.is_fixed(dof))
            })
        {
            return None;
        }
        Some(diaphragm.master)
    };
    let Some(top) = single_master(layer.top, |c| c.top) else {
        return Ok(None);
    };
    let bottom = single_master(layer.bottom, |c| c.bottom);
    if bottom.is_none()
        && !columns.iter().all(|c| {
            model.nodes[c.bottom.index()].restraint.is_fixed(dof)
                && response
                    .disp
                    .get(c.bottom.index())
                    .is_some_and(|u| u[dof as usize] == 0.0)
        })
    {
        return Ok(None);
    }
    let rotation = |id: NodeId| {
        response
            .disp
            .get(id.index())
            .map(|u| u[Dof::Rz as usize])
            .filter(|u| u.is_finite())
            .ok_or_else(|| {
                format!(
                    "Re: 階{}・{direction:?}方向の剛床代表節点{}の回転変位が欠損・非有限です",
                    story.0, id.0
                )
            })
    };
    Ok(Some([
        rotation(top)?,
        bottom.map(rotation).transpose()?.unwrap_or(0.0),
    ]))
}

fn horizontal_stiffnesses(
    model: &Model,
    story: StoryId,
    columns: &[StoryColumn],
    response: &StaticOnce,
    direction: SeismicDir,
) -> Result<Vec<f64>, String> {
    let forces: HashMap<_, _> = response
        .member_forces
        .iter()
        .map(|(id, f)| (*id, f))
        .collect();
    let end_forces: HashMap<_, _> = response
        .column_end_horizontal_forces
        .iter()
        .map(|(id, force)| (*id, force))
        .collect();
    let axis = match direction {
        SeismicDir::X => 0,
        SeismicDir::Y => 1,
    };
    let projected_coordinate = |id: NodeId| {
        let coord = model.nodes[id.index()].coord;
        [-coord[1], coord[0]][axis]
    };
    let cancellation_input = if let Some(first) = columns.first() {
        layer_rotations(model, story, columns, response, direction)?
            .filter(|angles| *angles != [0.0, 0.0])
            .map(|rotations| {
                let anchor = projected_coordinate(first.top);
                let center = columns
                    .iter()
                    .map(|column| {
                        (projected_coordinate(column.top) - anchor) / columns.len() as f64
                    })
                    .sum::<f64>();
                (rotations, anchor, center)
            })
    } else {
        None
    };
    columns
        .iter()
        .map(|column| {
            let elem = model.element(column.top_elem).expect("列挙時に確認した柱");
            let [p0, p1] = actual_axis_ends(model, elem)?;
            let failure = |reason: &str| {
                format!(
                    "Re: 階{}・{direction:?}方向の柱{}: {reason}",
                    story.0, elem.id.0
                )
            };
            let displacement = |id: NodeId| {
                response
                    .disp
                    .get(id.index())
                    .map(|u| u[axis])
                    .filter(|u| u.is_finite())
                    .ok_or_else(|| failure(&format!("SL節点{}の変位が欠損・非有限です", id.0)))
            };
            let signed_delta = displacement(column.top)? - displacement(column.bottom)?;
            let delta = signed_delta.abs();
            if !delta.is_finite() {
                return Err(failure("層間変位差が非有限です"));
            }
            let horizontal_force = if elem.kind == ElementKind::Beam {
                let top = column_cut_end(model, column, elem, [p0, p1])
                    .map_err(|reason| failure(&reason))?;
                end_forces
                    .get(&elem.id)
                    .ok_or_else(|| failure("上端の負担水平力が欠損しています"))?
                    .as_ref()
                    .map_err(|reason| failure(reason))?[top][axis]
            } else {
                let local = member_start(&forces, elem.id)
                    .filter(|local| local[..3].iter().all(|value| value.is_finite()))
                    .ok_or_else(|| failure("部材始端の内力が欠損・不正です"))?;
                station_force_global(p0, p1, elem.local_axis.ref_vector, local)
                    .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", elem.id.0))?[axis]
            };
            if !horizontal_force.is_finite() {
                return Err(failure("上端の負担水平力が不正です"));
            }
            if let Some(([top, bottom], anchor, center)) = cancellation_input {
                let arm = |id| (projected_coordinate(id) - anchor) - center;
                let rotation = top * arm(column.top) - bottom * arm(column.bottom);
                let translation = signed_delta - rotation;
                if !rotation.is_finite() || !translation.is_finite() {
                    return Err(failure("剛床のねじれによる層間変位を確認できません"));
                }
                if rotation.is_sign_positive() != translation.is_sign_positive()
                    && delta < rotation.abs().min(translation.abs())
                {
                    return Err(failure(
                        "剛床のねじれが層間変位を相殺するため柱の水平剛性の算定を保留しました",
                    ));
                }
            }
            if delta == 0.0 {
                return Err(failure("層間変位が0のため剛性を算定できません"));
            }
            let stiffness = horizontal_force.abs() / delta;
            if !stiffness.is_finite() {
                return Err(failure("負担水平力と層間変位の商が非有限です"));
            }
            Ok(stiffness)
        })
        .collect()
}

fn column_cut_end(
    model: &Model,
    column: &StoryColumn,
    elem: &ElementData,
    ends: [[f64; 3]; 2],
) -> Result<usize, String> {
    LocalFrame::from_nodes(ends[0], ends[1], elem.local_axis.ref_vector)
        .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", elem.id.0))?;
    let top = usize::from(elem.nodes[1] == column.top);
    let z = model.nodes[column.top.index()].coord[2];
    if ends[top][2] != z {
        return Err(
            "上端の実材軸端とSL接続節点の高さが異なり、層側の切断位置を特定できません".into(),
        );
    }
    if ends[1 - top][2] >= z {
        return Err("上端の実材軸が層側へ伸びず、層側の切断位置を特定できません".into());
    }
    for id in column.segments.iter().skip(1) {
        let segment = model.element(*id).expect("列挙時に確認した柱");
        if actual_axis_ends(model, segment)?
            .iter()
            .any(|end| end[2] >= z)
        {
            return Err(format!(
                "分割部材{}の実材軸が上端高さに達するため、層側の切断位置を特定できません",
                id.0
            ));
        }
    }
    Ok(top)
}

/// 柱の圧縮軸力を重みにした平面重心 `[mm]` を返す。
/// 柱なし・全柱引張では Ok(None)。上下階SL間の区間が曖昧、必要内力・位置が不正、又は集計・除算不能なら Err。
pub fn center_of_gravity_from_axial(
    model: &Model,
    story: StoryId,
    long_term: &StaticOnce,
) -> Result<Option<[f64; 2]>, String> {
    let columns = resolved_columns(model, story)?;
    let forces: HashMap<_, _> = long_term
        .member_forces
        .iter()
        .map(|(id, f)| (*id, f))
        .collect();
    let mut sum_n = 0.0;
    let mut sum_nx = 0.0;
    let mut sum_ny = 0.0;
    for column in &columns {
        let id = column.top_elem;
        let Some(local) = member_start(&forces, id) else {
            return Err(format!(
                "Re: 階{}の柱{}の部材始端の長期軸力がありません",
                story.0, id.0
            ));
        };
        let top = &model.nodes[column.top.index()];
        if !local[0].is_finite() || top.coord[..2].iter().any(|value| !value.is_finite()) {
            return Err(format!(
                "Re: 階{}の柱{}の長期軸力又は位置が不正です",
                story.0, id.0
            ));
        }
        let ni = (-local[0]).max(0.0);
        sum_n += ni;
        sum_nx += ni * top.coord[0];
        sum_ny += ni * top.coord[1];
    }
    if [sum_n, sum_nx, sum_ny]
        .iter()
        .any(|value| !value.is_finite())
    {
        return Err(format!(
            "Re: 階{}の長期軸力の合計又は位置一次モーメントを算定できません",
            story.0
        ));
    }
    if sum_n > 0.0 {
        let center = [sum_nx / sum_n, sum_ny / sum_n];
        if center.iter().any(|value| !value.is_finite()) {
            return Err(format!(
                "Re: 階{}の長期軸力による重心座標を算定できません",
                story.0
            ));
        }
        Ok(Some(center))
    } else {
        Ok(None)
    }
}

/// 応力解析結果に基づく当該層の偏心率（精算ルート）。
///
/// - 剛心: `column_stiffnesses_from_analysis`（ki = Qi/δi）
/// - 重心: 長期結果が未指定又は圧縮軸力の合計が非正なら質量重心を用いる。
///
/// `res_x`/`res_y` には弾性解析結果が必要。階や柱の変位・内力が欠損又は不正なら Err。
pub fn story_eccentricity_from_analysis(
    model: &Model,
    story: StoryId,
    res_x: &StaticOnce,
    res_y: &StaticOnce,
    long_term: Option<&StaticOnce>,
) -> Result<Eccentricity, String> {
    if !model.stories.iter().any(|item| item.id == story) {
        return Err(format!("Re: 上端階{}に対応する層がありません", story.0));
    }
    let mut cols = column_stiffnesses_from_analysis(model, story, res_x, res_y)?;
    if cols.is_empty() {
        return Err(format!(
            "Re: 階{}の上下階SLを結ぶ対象柱がありません",
            story.0
        ));
    }
    append_misc_wall_stiffnesses(model, story, &mut cols)?;
    let cor = center_of_rigidity(&cols).map_err(|reason| format!("階{}: {reason}", story.0))?;
    let com = match long_term
        .map(|lt| center_of_gravity_from_axial(model, story, lt))
        .transpose()?
        .flatten()
    {
        Some(center) => center,
        None => center_of_mass(model, story)?,
    };
    if cols.iter().any(|column| {
        column.pos.iter().any(|p| !p.is_finite())
            || !column.dx.is_finite()
            || !column.dy.is_finite()
    }) {
        return Err(format!(
            "Re: 階{}の柱水平剛性又は位置を算定できません",
            story.0
        ));
    }
    eccentricity(&cols, com, cor).map_err(|reason| format!("階{}: {reason}", story.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secondary::eccentricity::test_support::build_symmetric_frame;
    use sepika_core::ids::{LoadCaseId, NodeId};
    use sepika_core::model::{LoadCase, LoadCaseKind, NodalLoad};
    use sepika_solver::statics::linear::linear_static_once;

    #[test]
    fn static_rigidity_and_eccentricity_agree_for_beam_fiber_and_multi_spring_columns() {
        use ElementKind::{Beam, Fiber, MultiSpring};

        let (mut model, story) = build_symmetric_frame(None);
        model.elements.truncate(4);
        model.elements[2].nodes.swap(0, 1);
        for (axis, kind) in [
            LoadCaseKind::Seismic,
            LoadCaseKind::Seismic,
            LoadCaseKind::Dead,
        ]
        .into_iter()
        .enumerate()
        {
            model.load_cases.push(LoadCase {
                id: LoadCaseId(axis as u32),
                name: ["EX", "EY", "G"][axis].into(),
                kind,
                origin: Default::default(),
                nodal: (0..4)
                    .map(|i| {
                        let mut load = [0.0; 6];
                        load[axis] = if axis == 2 {
                            -1000.0 * (i + 1) as f64
                        } else {
                            1000.0
                        };
                        NodalLoad::manual(NodeId(i + 4), load)
                    })
                    .collect(),
                member: vec![],
            });
        }
        let evaluate = |model: &Model| {
            let [x, y, gravity] =
                [0, 1, 2].map(|id| linear_static_once(model, LoadCaseId(id)).unwrap());
            let columns = column_stiffnesses_from_analysis(model, story, &x, &y).unwrap();
            let center = center_of_rigidity(&columns).unwrap();
            let gravity_center = center_of_gravity_from_axial(model, story, &gravity).unwrap();
            let re =
                story_eccentricity_from_analysis(model, story, &x, &y, Some(&gravity)).unwrap();
            (columns, center, gravity_center, re)
        };
        let ordinary = evaluate(&model);
        assert_eq!(ordinary.0.len(), 4);
        for (actual, expected) in ordinary.1.into_iter().zip([3000.0; 2]) {
            assert!((actual - expected).abs() < 1e-8, "{actual}");
        }
        for (actual, expected) in ordinary.2.unwrap().into_iter().zip([3600.0, 4200.0]) {
            assert!((actual - expected).abs() < 1e-8, "{actual}");
        }
        assert!((ordinary.3.re_x.as_ref().unwrap() - 1200.0 / 18e6_f64.sqrt()).abs() < 1e-10);
        assert!((ordinary.3.re_y.as_ref().unwrap() - 600.0 / 18e6_f64.sqrt()).abs() < 1e-10);
        for kinds in [
            [Fiber; 4],
            [MultiSpring; 4],
            [Beam, Fiber, MultiSpring, Beam],
        ] {
            for (elem, kind) in model.elements.iter_mut().zip(kinds) {
                elem.kind = kind;
            }
            assert_eq!(evaluate(&model), ordinary, "{kinds:?}");
        }
    }

    #[test]
    fn nonlinear_column_geometry_and_missing_start_prevent_a_partial_story_evaluation() {
        let (mut model, story) = build_symmetric_frame(None);
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let mut gravity = fabricate_static([0.0; 2], [[-1000.0, 0.0, 0.0]; 4]);
        model.elements[0].kind = ElementKind::Fiber;
        let mut missing = x.clone();
        missing.member_forces[0].1.at[0].0 = 0.5;
        let reason =
            story_eccentricity_from_analysis(&model, story, &missing, &y, None).unwrap_err();
        assert!(
            reason.contains("X方向の柱0") && reason.contains("始端の内力"),
            "{reason}"
        );
        gravity.member_forces[0].1.at.clear();
        let reason =
            story_eccentricity_from_analysis(&model, story, &x, &y, Some(&gravity)).unwrap_err();
        assert!(
            reason.contains("柱0") && reason.contains("始端の長期軸力"),
            "{reason}"
        );

        for (ends, expected) in [
            ([[0.0; 3], [1.0, 0.0, 0.0]], "実材軸が鉛直でない"),
            ([[100.0, 0.0, 0.0]; 2], "接続節点の平面位置が異なる"),
        ] {
            model.elements[0].end_offsets_mm = ends;
            let reason = story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap_err();
            assert!(
                reason.contains("柱0") && reason.contains(expected),
                "{reason}"
            );
        }
        assert!(crate::secondary::eccentricity::story_centers(&model, story)
            .unwrap_err()
            .contains("D値略算法は未対応"));
    }

    #[test]
    fn a_sloped_segment_in_a_mixed_column_is_reported_even_with_aligned_sl_ends() {
        let (mut model, story) = build_symmetric_frame(None);
        let mut middle = model.nodes[4].clone();
        middle.id = NodeId(8);
        middle.coord = [100.0, 0.0, 1500.0];
        model.nodes.push(middle);
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(8);
        upper.nodes[0] = NodeId(8);
        model.elements.push(upper);
        model.elements[0].nodes[1] = NodeId(8);
        model.elements[0].kind = ElementKind::MultiSpring;
        let [x, y] = [
            fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
            fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]),
        ]
        .map(|mut response| {
            response.member_forces[0].0 = ElemId(8);
            response.column_end_horizontal_forces[0].0 = ElemId(8);
            response
        });
        let reason = story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap_err();
        assert!(
            reason.contains("柱8") && reason.contains("実材軸が鉛直でない"),
            "{reason}"
        );
        let drifts =
            crate::secondary::stiffness_ratio::column_drifts(&model, &x.disp, 0, story).unwrap();
        assert!(drifts
            .iter()
            .any(|col| col.elem == ElemId(8) && col.drift == 10.0));
        model.nodes[8].coord[0] = 0.0;
        let columns = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns[0].dx, 100.0);
        assert_eq!(columns[0].dy, 100.0);
    }

    #[test]
    fn branching_at_an_interior_node_prevents_a_partial_story_evaluation() {
        for branch_height_mm in [0.0, 1500.0, 2200.0, 3000.0] {
            let (mut model, story) = build_symmetric_frame(None);
            for (id, coord) in [(8, [0.0, 0.0, 1500.0]), (9, [300.0, 0.0, branch_height_mm])] {
                let mut node = model.nodes[4].clone();
                node.id = NodeId(id);
                node.coord = coord;
                model.nodes.push(node);
            }
            model.elements[0].nodes[1] = NodeId(8);
            for (id, top) in [(8, 4), (9, 9)] {
                let mut segment = model.elements[0].clone();
                segment.id = ElemId(id);
                segment.nodes = [NodeId(8), NodeId(top)].into_iter().collect();
                model.elements.push(segment);
            }
            let [x, y] = [
                fabricate_static([18.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
                fabricate_static([0.0, 18.0], [[0.0, 1000.0, 0.0]; 4]),
            ]
            .map(|mut response| {
                response.disp.push([0.0; 6]);
                response.disp.push(response.disp[4]);
                copy_column_forces(&mut response, &[ElemId(8), ElemId(9)]);
                response
            });
            let reason = story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap_err();
            for expected in ["階1", "柱8", "中間節点で分岐", "節点8"] {
                assert!(reason.contains(expected), "{reason}");
            }
            for (axis, response) in [(SeismicDir::X, &x), (SeismicDir::Y, &y)] {
                assert_eq!(
                    column_horizontal_stiffnesses(&model, story, response, axis),
                    Err(reason.clone())
                );
            }
            assert_eq!(
                center_of_gravity_from_axial(&model, story, &x),
                Err(reason.clone())
            );
            for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
                for elem in [0, 1] {
                    model.elements[elem].kind = kind;
                    assert_eq!(
                        story_eccentricity_from_analysis(&model, story, &x, &y, None),
                        Err(reason.clone())
                    );
                    model.elements[elem].kind = ElementKind::Beam;
                }
                model.elements[0].kind = kind;
                let drifts =
                    crate::secondary::stiffness_ratio::column_drifts(&model, &x.disp, 0, story)
                        .unwrap();
                assert_eq!(
                    drifts
                        .into_iter()
                        .map(|drift| drift.elem)
                        .collect::<Vec<_>>(),
                    [ElemId(1), ElemId(2), ElemId(3)]
                );
                model.elements[0].kind = ElementKind::Beam;
            }
        }
    }

    #[test]
    fn re_1640_counts_an_ordinary_split_column_once_when_its_sl_ends_are_unique() {
        let (mut model, story) = build_symmetric_frame(None);
        let mut split = model.nodes[4].clone();
        split.id = NodeId(8);
        split.coord[2] = 2998.0;
        split.mass = None;
        split.story = None;
        model.nodes.push(split);
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(8);
        upper.nodes[0] = NodeId(8);
        model.elements.push(upper);
        model.elements[0].nodes[1] = NodeId(8);

        let [x, y, mut gravity] = [
            fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
            fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]),
            fabricate_static([0.0, 0.0], [[-1000.0, 0.0, 0.0]; 4]),
        ]
        .map(|mut response| {
            response.disp.push(response.disp[4]);
            copy_column_forces(&mut response, &[ElemId(8)]);
            response
        });
        gravity.member_forces[0].1.at[0].1[0] = -9000.0;

        let columns = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap();
        assert_eq!(columns.len(), 4);
        assert_eq!(columns.iter().map(|c| c.dx).sum::<f64>(), 400.0);
        assert_eq!(columns.iter().map(|c| c.dy).sum::<f64>(), 400.0);
        assert_eq!(
            center_of_gravity_from_axial(&model, story, &gravity).unwrap(),
            Some([3000.0, 3000.0])
        );
        for long_term in [None, Some(&gravity)] {
            let value = story_eccentricity_from_analysis(&model, story, &x, &y, long_term).unwrap();
            assert_eq!((value.re_x, value.re_y), (Ok(0.0), Ok(0.0)));
        }

        model.nodes[8].coord[2] = 2999.5;
        let reason = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap_err();
        for expected in ["階1", "上階SLの柱8", "一意に特定できません"] {
            assert!(reason.contains(expected), "{reason}");
        }
        for (direction, response) in [(SeismicDir::X, &x), (SeismicDir::Y, &y)] {
            assert_eq!(
                column_horizontal_stiffnesses(&model, story, response, direction),
                Err(reason.clone())
            );
        }
        assert_eq!(
            center_of_gravity_from_axial(&model, story, &gravity),
            Err(reason.clone())
        );
        for long_term in [None, Some(&gravity)] {
            assert_eq!(
                story_eccentricity_from_analysis(&model, story, &x, &y, long_term),
                Err(reason.clone())
            );
        }
    }

    #[test]
    fn re_1640_rejects_sl_links_even_when_branching_prevents_column_trace() {
        for (split_mm, end, link, branch, boundary) in [
            (2999.5, 1, [8, 4], [1, 8], "上階SLの柱8"),
            (0.5, 0, [0, 8], [1, 4], "下階SLの柱8"),
        ] {
            let (mut model, story) = build_symmetric_frame(None);
            let mut split = model.nodes[4].clone();
            split.id = NodeId(8);
            split.coord[2] = split_mm;
            split.mass = None;
            split.story = None;
            model.nodes.push(split);
            for (id, nodes) in [(8, link), (9, branch)] {
                let mut segment = model.elements[0].clone();
                segment.id = ElemId(id);
                segment.nodes = nodes.map(NodeId).into_iter().collect();
                model.elements.push(segment);
            }
            model.elements[0].nodes[end] = NodeId(8);
            let [x, y] = [
                fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
                fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]),
            ]
            .map(|mut response| {
                response
                    .disp
                    .push(if end == 1 { response.disp[4] } else { [0.0; 6] });
                copy_column_forces(&mut response, &[ElemId(8), ElemId(9)]);
                response
            });
            let reason = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap_err();
            assert!(reason.contains(boundary), "{reason}");
            assert!(reason.contains("一意に特定できません"), "{reason}");
            assert_eq!(
                story_eccentricity_from_analysis(&model, story, &x, &y, None),
                Err(reason)
            );
        }
    }

    #[test]
    fn a_mixed_column_cannot_share_its_lower_segment_with_another_start_on_the_same_sl() {
        let (mut model, story) = build_symmetric_frame(None);
        let mut lower_top = model.nodes[4].clone();
        lower_top.id = NodeId(8);
        lower_top.coord[2] = 2999.5;
        assert!(model.on_diaphragm_level(story, model.nodes[4].coord[2]));
        assert!(model.on_diaphragm_level(story, lower_top.coord[2]));
        model.nodes.push(lower_top);
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(8);
        upper.nodes[0] = NodeId(8);
        model.elements.push(upper);
        model.elements[0].nodes[1] = NodeId(8);

        let [x, y] = [
            fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
            fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]),
        ]
        .map(|mut response| {
            response.disp.push(response.disp[4]);
            copy_column_forces(&mut response, &[ElemId(8)]);
            response
        });
        for has_branch in [false, true] {
            if has_branch {
                let mut end = model.nodes[4].clone();
                end.id = NodeId(9);
                end.coord = [300.0, 0.0, 1500.0];
                model.nodes.push(end);
                let mut branch = model.elements[8].clone();
                branch.id = ElemId(9);
                branch.nodes = [NodeId(4), NodeId(9)].into_iter().collect();
                model.elements.push(branch);
            }
            let reason = story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap_err();
            assert!(reason.contains("上階SLの柱8"), "{reason}");
            let mixed_elements: &[usize] = if has_branch { &[0, 8, 9] } else { &[0, 8] };
            for &elem in mixed_elements {
                for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
                    model.elements[elem].kind = kind;
                    let reason =
                        story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap_err();
                    assert!(
                        reason.contains("柱8") && reason.contains("層境界のSL節点どうし"),
                        "{reason}"
                    );
                }
                model.elements[elem].kind = ElementKind::Beam;
            }
        }

        model.elements.retain(|elem| elem.id != ElemId(0));
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            model
                .elements
                .iter_mut()
                .find(|elem| elem.id == ElemId(8))
                .unwrap()
                .kind = kind;
            let columns = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap();
            assert_eq!(columns.len(), 3);
            let eccentricity =
                story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap();
            assert!(eccentricity.re_x.unwrap().is_finite());
            assert!(eccentricity.re_y.unwrap().is_finite());
        }
    }

    #[test]
    fn re_and_drift_hold_for_explicit_links_at_either_sl() {
        use crate::secondary::stiffness_ratio::column_drifts;

        for (top_link, bottom_link) in [(true, false), (false, true), (true, true), (false, false)]
        {
            let (mut model, story) = build_symmetric_frame(None);
            let mut top_inside = model.nodes[4].clone();
            top_inside.id = NodeId(8);
            top_inside.coord[2] = 2999.5;
            model.nodes.push(top_inside);
            let mut bottom_inside = model.nodes[0].clone();
            bottom_inside.id = NodeId(9);
            bottom_inside.coord[2] = 0.5;
            bottom_inside.restraint = sepika_core::dof::Dof6Mask::FREE;
            model.nodes.push(bottom_inside);

            for (linked, id, nodes, end, inside) in
                [(top_link, 8, [8, 4], 1, 8), (bottom_link, 9, [0, 9], 0, 9)]
            {
                if linked {
                    let mut segment = model.elements[0].clone();
                    segment.id = ElemId(id);
                    segment.nodes = nodes.map(NodeId).into_iter().collect();
                    model.elements.push(segment);
                    model.elements[0].nodes[end] = NodeId(inside);
                }
            }
            if !top_link && !bottom_link {
                model.elements[1].nodes[0] = NodeId(0);
            }
            let [x, y] = [0, 1].map(|axis| {
                let top_disp = if axis == 0 { [10.0, 0.0] } else { [0.0, 10.0] };
                let local_force = if axis == 0 {
                    [0.0, 0.0, 1000.0]
                } else {
                    [0.0, 1000.0, 0.0]
                };
                let mut response = fabricate_static(top_disp, [local_force; 4]);
                response.disp.push(response.disp[4]);
                let mut bottom_inside_disp = [0.0; 6];
                bottom_inside_disp[axis] = 4.0;
                response.disp.push(bottom_inside_disp);
                copy_column_forces(&mut response, &[ElemId(8), ElemId(9)]);
                response
            });
            for kind in [
                ElementKind::Beam,
                ElementKind::Fiber,
                ElementKind::MultiSpring,
            ] {
                model.elements[0].kind = kind;
                let re = story_eccentricity_from_analysis(&model, story, &x, &y, None);
                let drift = column_drifts(&model, &x.disp, 0, story);
                if top_link || bottom_link {
                    let reason = re.unwrap_err();
                    assert_eq!(reason.contains("上階SLの柱8"), top_link, "{reason}");
                    assert_eq!(reason.contains("下階SLの柱9"), bottom_link, "{reason}");
                    assert_eq!(drift.unwrap_err(), reason);
                } else {
                    assert!(re.unwrap().re_x.unwrap().is_finite());
                    assert_eq!(drift.unwrap().len(), 4);
                }
            }
        }
    }

    #[test]
    fn a_return_to_the_upper_sl_cannot_add_a_second_copy_of_a_mixed_column() {
        use crate::secondary::stiffness_ratio::{column_drifts, max_column_drift};

        let (mut model, story) = build_symmetric_frame(None);
        for (id, z) in [(8, 2999.5), (9, 1500.0)] {
            let mut node = model.nodes[4].clone();
            node.id = NodeId(id);
            node.coord[2] = z;
            model.nodes.push(node);
        }
        model.elements[0].nodes[1] = NodeId(8);
        for (id, nodes) in [(8, [4, 9]), (9, [9, 8])] {
            let mut segment = model.elements[0].clone();
            segment.id = ElemId(id);
            segment.nodes = nodes.map(NodeId).into_iter().collect();
            model.elements.push(segment);
        }
        let [x, y] = [
            fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]),
            fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]),
        ]
        .map(|mut response| {
            let axis = usize::from(response.disp[4][0] == 0.0);
            response.disp[4][axis] = 40.0;
            let mut lower_start = [0.0; 6];
            lower_start[axis] = 18.0;
            response.disp.push(lower_start);
            response.disp.push([999.0; 6]);
            response
        });
        let mut independent = model.clone();
        independent.elements.truncate(8);
        let expected = story_eccentricity_from_analysis(&independent, story, &x, &y, None).unwrap();

        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            model.elements[4].kind = kind;
            let old = max_column_drift(&model, &x.disp, 0, story)
                .unwrap()
                .unwrap();
            assert_eq!((old.elem, old.drift), (ElemId(8), 40.0));
        }
        model.elements[4].kind = ElementKind::Beam;
        let ordinary_re = story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap();
        let ordinary_drifts = column_drifts(&model, &x.disp, 0, story).unwrap();
        model.stories.push(sepika_core::model::Story::new(
            StoryId(2),
            "上階".into(),
            Some(6000.0),
            Some(0.0),
        ));
        for (id, z) in [(10, 3000.0), (11, 6000.0)] {
            let mut node = model.nodes[4].clone();
            node.id = NodeId(id);
            node.coord = [12000.0, 0.0, z];
            node.story = None;
            model.nodes.push(node);
        }
        let mut upper_story_column = model.elements[0].clone();
        upper_story_column.id = ElemId(10);
        upper_story_column.nodes = [NodeId(10), NodeId(11)].into_iter().collect();
        model.elements.push(upper_story_column);
        for index in [1, 10] {
            for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
                model.elements[index].kind = kind;
                assert_eq!(
                    story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap(),
                    ordinary_re
                );
                assert_eq!(
                    column_drifts(&model, &x.disp, 0, story).unwrap(),
                    ordinary_drifts
                );
            }
            model.elements[index].kind = ElementKind::Beam;
        }
        for index in [0, 8, 9] {
            for kind in [ElementKind::Fiber, ElementKind::MultiSpring] {
                model.elements[index].kind = kind;
                let columns = column_drifts(&model, &x.disp, 0, story).unwrap();
                assert_eq!(columns.len(), 4);
                let max = max_column_drift(&model, &x.disp, 0, story)
                    .unwrap()
                    .unwrap();
                assert_eq!((max.elem, max.drift), (ElemId(0), 18.0));
                assert_eq!(
                    story_eccentricity_from_analysis(&model, story, &x, &y, None).unwrap(),
                    expected
                );
            }
            model.elements[index].kind = ElementKind::Beam;
        }
    }

    fn copy_column_forces(response: &mut StaticOnce, ids: &[ElemId]) {
        let section = response.member_forces[0].1.clone();
        let ends = response.column_end_horizontal_forces[0].1.clone();
        for &id in ids {
            response.member_forces.push((id, section.clone()));
            response
                .column_end_horizontal_forces
                .push((id, ends.clone()));
        }
    }

    /// 柱 4 本（ElemId 0..4、節点 i(底)→i+4(頂)）へ、指定の局所内力
    /// [n, qy, qz] を持つ member_forces と頂部一様変位 disp を合成する。
    /// 柱の ref_vector=[0,1,0] より ex=[0,0,1], ey=[0,1,0], ez=[1,0,0]。
    /// → 柱頭側からの全体 X 方向力 = -qz、全体 Y 方向力 = -qy。
    pub(super) fn fabricate_static(
        top_disp: [f64; 2],
        col_local_forces: [[f64; 3]; 4],
    ) -> StaticOnce {
        let mut disp = vec![[0.0; 6]; 8];
        for d in disp.iter_mut().skip(4) {
            d[0] = top_disp[0];
            d[1] = top_disp[1];
        }
        let member_forces = col_local_forces
            .iter()
            .enumerate()
            .map(|(i, &[n, qy, qz])| {
                (
                    ElemId(i as u32),
                    MemberForces {
                        at: vec![(0.0, [n, qy, qz, 0.0, 0.0, 0.0])],
                    },
                )
            })
            .collect();
        StaticOnce {
            story_shears: None,
            stiffness_ratio_disp: None,
            disp,
            member_forces,
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
            column_end_horizontal_forces: col_local_forces
                .iter()
                .enumerate()
                .map(|(i, &[_, qy, qz])| (ElemId(i as u32), Ok([[qz, qy], [-qz, -qy]])))
                .collect(),
        }
    }

    /// ki = Qi/δi の精算: X 加力（δ=10, Qx=qz=1000 → kX=100）、
    /// Y 加力（δ=10, 左 qy=1000/右 qy=3000 → kY=100/300）→ 剛心 Xs=4500, Ys=3000。
    #[test]
    fn test_column_stiffnesses_from_analysis() {
        let (model, s0) = build_symmetric_frame(None);
        let res_x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let res_y = fabricate_static(
            [0.0, 10.0],
            [
                [0.0, 1000.0, 0.0],
                [0.0, 3000.0, 0.0],
                [0.0, 1000.0, 0.0],
                [0.0, 3000.0, 0.0],
            ],
        );
        let cols = column_stiffnesses_from_analysis(&model, s0, &res_x, &res_y).unwrap();
        assert_eq!(cols.len(), 4);
        for c in &cols {
            assert!((c.dx - 100.0).abs() < 1e-9, "kX={}", c.dx);
        }
        let cor = center_of_rigidity(&cols).unwrap();
        assert!((cor[0] - 4500.0).abs() < 1e-9, "Xs={}", cor[0]);
        assert!((cor[1] - 3000.0).abs() < 1e-9, "Ys={}", cor[1]);
    }

    #[test]
    fn analyzed_eccentricity_ignores_unused_girder_reference() {
        let (mut model, story) = build_symmetric_frame(None);
        let res_x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let res_y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let expected =
            story_eccentricity_from_analysis(&model, story, &res_x, &res_y, None).unwrap();
        model.elements[4].local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
        assert_eq!(
            story_eccentricity_from_analysis(&model, story, &res_x, &res_y, None).unwrap(),
            expected
        );
    }

    #[test]
    fn analyzed_eccentricity_rejects_consumed_column_reference() {
        let (mut model, story) = build_symmetric_frame(None);
        let res_x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let res_y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        model.elements[0].local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
        let error =
            story_eccentricity_from_analysis(&model, story, &res_x, &res_y, None).unwrap_err();
        assert!(
            error.contains("部材 ID 0") && error.contains("非有限"),
            "{error}"
        );

        model.nodes[4].coord = [3000.0, 3000.0, 3000.0];
        model.elements[0].local_axis.ref_vector = [1e308, 1e308, 1e308 - 1e292];
        let error =
            story_eccentricity_from_analysis(&model, story, &res_x, &res_y, None).unwrap_err();
        assert!(
            error.contains("部材 ID 0") && error.contains("単位直交基底"),
            "{error}"
        );
    }

    /// 長期軸力による重心: 全柱等圧縮 → 幾何中央 (3000, 3000)。
    /// 右側 2 本を 3 倍圧縮 → gx = 4500。引張柱は重み 0。
    #[test]
    fn test_center_of_gravity_from_axial() {
        let (model, s0) = build_symmetric_frame(None);
        let uniform = fabricate_static([0.0, 0.0], [[-200.0, 0.0, 0.0]; 4]);
        let g = center_of_gravity_from_axial(&model, s0, &uniform)
            .unwrap()
            .unwrap();
        assert!((g[0] - 3000.0).abs() < 1e-9 && (g[1] - 3000.0).abs() < 1e-9);

        let biased = fabricate_static(
            [0.0, 0.0],
            [
                [-100.0, 0.0, 0.0],
                [-300.0, 0.0, 0.0],
                [-100.0, 0.0, 0.0],
                [-300.0, 0.0, 0.0],
            ],
        );
        let g = center_of_gravity_from_axial(&model, s0, &biased)
            .unwrap()
            .unwrap();
        assert!((g[0] - 4500.0).abs() < 1e-9, "gx={}", g[0]);

        let tension = fabricate_static([0.0, 0.0], [[200.0, 0.0, 0.0]; 4]);
        assert!(center_of_gravity_from_axial(&model, s0, &tension)
            .unwrap()
            .is_none());
    }

    /// 精算ルートの統合: 剛心 Xs=4500・重心 gx=3000 → ex=1500, Rey=ex/rey。
    #[test]
    fn test_story_eccentricity_from_analysis() {
        let (model, s0) = build_symmetric_frame(None);
        let res_x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let res_y = fabricate_static(
            [0.0, 10.0],
            [
                [0.0, 1000.0, 0.0],
                [0.0, 3000.0, 0.0],
                [0.0, 1000.0, 0.0],
                [0.0, 3000.0, 0.0],
            ],
        );
        let long_term = fabricate_static([0.0, 0.0], [[-200.0, 0.0, 0.0]; 4]);
        let ecc =
            story_eccentricity_from_analysis(&model, s0, &res_x, &res_y, Some(&long_term)).unwrap();
        assert!((ecc.ex - 1500.0).abs() < 1e-9, "ex={}", ecc.ex);
        assert!(ecc.ey.abs() < 1e-9, "ey={}", ecc.ey);
        // KR = ΣkX·ȳ² + ΣkY·x̄²
        //   ȳ = ±3000（kX=100×4）→ 100·9e6·4 = 3.6e9
        //   x̄ = x−4500 = [-4500,1500,-4500,1500] → 100·4500²+300·1500²（×2組）
        //     = 2·(100·2.025e7 + 300·2.25e6) = 2·(2.025e9+0.675e9) = 5.4e9
        let kr_expect = 3.6e9 + 5.4e9;
        assert!(
            (ecc.kr - kr_expect).abs() / kr_expect < 1e-12,
            "KR={}",
            ecc.kr
        );
        let rey = (kr_expect / 800.0_f64).sqrt();
        assert!(
            (ecc.re_y.as_ref().unwrap() - 1500.0 / rey).abs() < 1e-9,
            "Rey={}",
            ecc.re_y.as_ref().unwrap()
        );
    }

    /// 中間節点で分割された柱は 1 本として数えること。
    #[test]
    fn test_column_stiffnesses_count_split_column_once() {
        use sepika_core::dof::Dof6Mask;
        use sepika_core::ids::{NodeId, SectionId};
        use sepika_core::model::{
            ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LocalAxis, Node,
            RigidZone, Section, Story,
        };
        use smallvec::SmallVec;

        let base = StoryId(0);
        let top = StoryId(1);
        let nodes = vec![
            Node {
                id: NodeId(0),
                coord: [0.0, 0.0, 0.0],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(base),
                support_spring: None,
            },
            Node {
                id: NodeId(1),
                coord: [0.0, 0.0, 2000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(top),
                support_spring: None,
            },
            Node {
                id: NodeId(2),
                coord: [0.0, 0.0, 4000.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: Some(top),
                support_spring: None,
            },
        ];
        let mk_elem = |id: u32, n0: u32, n1: u32| ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: {
                let mut v: SmallVec<[NodeId; 8]> = SmallVec::new();
                v.push(NodeId(n0));
                v.push(NodeId(n1));
                v
            },
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0, 0.0, 0.0]; 2],
            plastic_zone: None,
            spring: None,
        };
        let model = Model {
            nodes,
            elements: vec![mk_elem(0, 1, 2), mk_elem(1, 0, 1)],
            sections: vec![{
                let mut section = Section::zero(SectionId(0), "C".into());
                section.frame_use = Some(FrameSectionUse::Column);
                section
            }],
            stories: vec![
                Story {
                    wall_weights: Vec::new(),
                    id: base,
                    name: "1F".to_string(),
                    elevation: Some(0.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(0)],
                    seismic_weight: None,
                    weight_override: None,
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
                Story {
                    wall_weights: Vec::new(),
                    id: top,
                    name: "2F".to_string(),
                    elevation: Some(4000.0),
                    sl_offset_mm: Some(0.0),
                    source_story: None,
                    node_ids: vec![NodeId(2)],
                    seismic_weight: None,
                    weight_override: None,
                    structure: Default::default(),
                    level_kind: Default::default(),
                    dynamic_mass: None,
                    standard_floor_load: None,
                    column_finish_area_weight: 0.0,
                    fireproof: Default::default(),
                },
            ],
            ..Default::default()
        };
        let mut disp_x = vec![[0.0; 6]; 3];
        disp_x[2][0] = 10.0;
        disp_x[0][0] = 0.0;
        let res_x = StaticOnce {
            story_shears: None,
            stiffness_ratio_disp: None,
            disp: disp_x,
            member_forces: vec![(
                ElemId(0),
                MemberForces {
                    at: vec![(0.0, [0.0, 0.0, 1000.0, 0.0, 0.0, 0.0])],
                },
            )],
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
            column_end_horizontal_forces: vec![(ElemId(0), Ok([[1000.0, 0.0], [-1000.0, -0.0]]))],
        };
        let mut disp_y = vec![[0.0; 6]; 3];
        disp_y[2][1] = 10.0;
        let res_y = StaticOnce {
            story_shears: None,
            stiffness_ratio_disp: None,
            disp: disp_y,
            member_forces: vec![(
                ElemId(0),
                MemberForces {
                    at: vec![(0.0, [0.0, 1000.0, 0.0, 0.0, 0.0, 0.0])],
                },
            )],
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
            column_end_horizontal_forces: vec![(ElemId(0), Ok([[0.0, 1000.0], [-0.0, -1000.0]]))],
        };
        let cols = column_stiffnesses_from_analysis(&model, top, &res_x, &res_y).unwrap();
        assert_eq!(cols.len(), 1, "分割柱は 1 本と数える");
        assert!((cols[0].dx - 100.0).abs() < 1e-9);
    }
    #[test]
    fn fes718_analysis_rejects_missing_force_displacement_story_and_nonfinite() {
        let (model, story) = build_symmetric_frame(None);
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        assert!(story_eccentricity_from_analysis(&model, StoryId(99), &x, &y, None).is_err());
        let mut missing = x.clone();
        missing.disp.pop();
        assert!(story_eccentricity_from_analysis(&model, story, &missing, &y, None).is_err());
        let mut missing = x.clone();
        missing.column_end_horizontal_forces.pop();
        assert!(story_eccentricity_from_analysis(&model, story, &missing, &y, None).is_err());
        let mut invalid = x.clone();
        invalid.disp[4][0] = f64::NAN;
        assert!(story_eccentricity_from_analysis(&model, story, &invalid, &y, None).is_err());
        invalid = x.clone();
        invalid.column_end_horizontal_forces[0].1.as_mut().unwrap()[1][0] = f64::INFINITY;
        assert!(story_eccentricity_from_analysis(&model, story, &invalid, &y, None).is_err());
    }
    #[test]
    fn fes718_review_rejects_missing_or_nonfinite_long_term_axial_force() {
        let (model, story) = build_symmetric_frame(None);
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let long_term = fabricate_static([0.0, 0.0], [[-200.0, 0.0, 0.0]; 4]);
        let mut missing = long_term.clone();
        missing.member_forces.pop();
        let mut rejected =
            vec![story_eccentricity_from_analysis(&model, story, &x, &y, Some(&missing)).is_err()];
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut result = long_term.clone();
            result.member_forces[0].1.at[0].1[0] = invalid;
            rejected.push(
                story_eccentricity_from_analysis(&model, story, &x, &y, Some(&result)).is_err(),
            );
        }
        assert_eq!(rejected, vec![true; 4]);
    }

    #[test]
    fn fes718_review_rejects_nonfinite_displacement_difference() {
        let (model, story) = build_symmetric_frame(None);
        let mut x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        x.disp[4][0] = f64::MAX;
        x.disp[0][0] = -f64::MAX;
        assert!(story_eccentricity_from_analysis(&model, story, &x, &y, None).is_err());
    }

    #[test]
    fn fes718_review_preserves_mass_center_without_long_term_or_with_all_tension() {
        let (model, story) = build_symmetric_frame(None);
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let tension = fabricate_static([0.0, 0.0], [[200.0, 0.0, 0.0]; 4]);
        for long_term in [None, Some(&tension)] {
            let result =
                story_eccentricity_from_analysis(&model, story, &x, &y, long_term).unwrap();
            assert_eq!(result.ex, 0.0);
            assert_eq!(result.ey, 0.0);
            assert_eq!(result.re_x.unwrap(), 0.0);
            assert_eq!(result.re_y.unwrap(), 0.0);
        }
    }
    #[test]
    fn fes718_axial_sum_overflow_cannot_generate_finite_re_or_fes() {
        let (mut model, story) = build_symmetric_frame(None);
        for (i, node) in model.nodes.iter_mut().enumerate() {
            node.coord[0] = if i % 2 == 0 { 0.0 } else { 0.5 };
            node.coord[1] = if i % 4 < 2 { 0.0 } else { 0.5 };
        }
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let long_term = fabricate_static([0.0, 0.0], [[-1e308, 0.0, 0.0]; 4]);
        let result = story_eccentricity_from_analysis(&model, story, &x, &y, Some(&long_term));
        assert!(result.is_err(), "{result:?}");
        assert!(center_of_gravity_from_axial(&model, story, &long_term).is_err());
    }
    #[test]
    fn fes718_mass_sum_overflow_is_unavailable_in_both_mass_center_fallbacks() {
        let (mut model, story) = build_symmetric_frame(None);
        for (i, node) in model.nodes.iter_mut().enumerate() {
            node.coord[0] = if i % 2 == 0 { 0.0 } else { 0.5 };
            node.coord[1] = if i % 4 < 2 { 0.0 } else { 0.5 };
            if node.story == Some(story) {
                node.mass = Some([1e308; 6]);
            }
        }
        let x = fabricate_static([10.0, 0.0], [[0.0, 0.0, 1000.0]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        let tension = fabricate_static([0.0, 0.0], [[200.0, 0.0, 0.0]; 4]);
        let rejected: Vec<_> = [None, Some(&tension)]
            .into_iter()
            .map(|long_term| {
                story_eccentricity_from_analysis(&model, story, &x, &y, long_term).is_err()
            })
            .collect();
        assert_eq!(rejected, vec![true; 2]);
    }
    #[test]
    fn fes718_rigidity_overflow_stops_story_before_recomputing_e_over_r() {
        let (mut model, story) = build_symmetric_frame(None);
        for (i, node) in model.nodes.iter_mut().enumerate() {
            node.coord[0] = if i % 2 == 0 { 0.0 } else { 0.5 };
            node.coord[1] = 0.0;
        }
        let mut mass_node = model.nodes[4].clone();
        mass_node.id = sepika_core::ids::NodeId(8);
        mass_node.coord = [0.25, 1.4e153, 3000.0];
        mass_node.mass = Some([4.0; 6]);
        model.nodes.push(mass_node);
        let x = fabricate_static([1.0, 0.0], [[0.0, 0.0, 1.0]; 4]);
        let y = fabricate_static([0.0, 1.0], [[0.0, 1e308, 0.0]; 4]);
        let result = story_eccentricity_from_analysis(&model, story, &x, &y, None);
        assert!(result.is_err(), "{result:?}");
    }
}

#[cfg(test)]
mod misc620_tests {
    use super::tests::fabricate_static;
    use super::*;
    use crate::secondary::eccentricity::test_support::build_symmetric_frame;

    #[test]
    fn misc740_有限な微小変位でも同比例荷重なら柱剛性は不変() {
        let (model, story) = build_symmetric_frame(None);
        for scale in [1.0, 1e-12] {
            let x = fabricate_static([10.0 * scale, 0.0], [[0.0, 0.0, 1000.0 * scale]; 4]);
            let y = fabricate_static([0.0, 10.0 * scale], [[0.0, 1000.0 * scale, 0.0]; 4]);
            let columns = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap();
            for column in columns {
                assert!((column.dx - 100.0).abs() < 1e-9);
                assert!((column.dy - 100.0).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn misc740_ゼロ分母と商のoverflowは対象方向と理由を返す() {
        let (mut model, story) = build_symmetric_frame(None);
        let y = fabricate_static([0.0, 10.0], [[0.0, 1000.0, 0.0]; 4]);
        for q in [0.0, 1000.0] {
            let x = fabricate_static([0.0, 0.0], [[0.0, 0.0, q]; 4]);
            let reason = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap_err();
            for expected in ["階1", "X方向", "柱", "層間変位が0"] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
        let x = fabricate_static([f64::MIN_POSITIVE, 0.0], [[0.0, 0.0, f64::MAX]; 4]);
        let reason = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap_err();
        assert!(
            reason.contains("X方向") && reason.contains("商が非有限"),
            "{reason}"
        );
        model.elements[0].local_axis.ref_vector = [f64::NAN, 0.0, 0.0];
        let zero = fabricate_static([0.0, 0.0], [[0.0; 3]; 4]);
        let reason = column_stiffnesses_from_analysis(&model, story, &zero, &y).unwrap_err();
        assert!(reason.contains("局所座標"), "{reason}");
    }

    #[test]
    fn missing_response_is_rejected_but_measured_zero_is_available() {
        let (model, story) = build_symmetric_frame(None);
        let x = fabricate_static([10.0, 0.0], [[0.0; 3]; 4]);
        let y = fabricate_static([0.0, 10.0], [[0.0; 3]; 4]);
        let cols = column_stiffnesses_from_analysis(&model, story, &x, &y).unwrap();
        assert!(cols.iter().all(|c| c.dx == 0.0 && c.dy == 0.0));
        assert!(story_eccentricity_from_analysis(&model, story, &x, &y, None).is_ok());
        let mut missing = x.clone();
        missing.column_end_horizontal_forces.pop();
        assert!(
            story_eccentricity_from_analysis(&model, story, &missing, &y, None)
                .unwrap_err()
                .contains("欠損")
        );
    }
}
