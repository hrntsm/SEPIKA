//! S/CFT 造仕口パネルの柱2主軸への検定配線。

use super::common::MemberInfo;
use crate::steel::panel_zone::{s_panel_zone_check, SPanelInput};
use crate::CheckOutcome;
use sepika_core::geom::vec3::{cross, dot};
use sepika_core::ids::NodeId;
use sepika_core::model::Model;
use sepika_core::panel_zone::{
    panel_column_moment_axes, panel_joint_planes, resolve_panel_joint, standard_flange_differences,
    BeamFlangePositions, MemberOrientation, PanelGeometry, PanelPlane, PANEL_DIRECTION_TOL,
};
use sepika_core::section_shape::SectionShape;

enum PanelCheckError {
    Joint(String),
    Input(String),
}

impl From<String> for PanelCheckError {
    fn from(reason: String) -> Self {
        Self::Input(reason)
    }
}

impl From<&str> for PanelCheckError {
    fn from(reason: &str) -> Self {
        Self::Input(reason.into())
    }
}

/// 元の梁構面の標準形式を確認し、柱の強軸・弱軸で検定する。
pub(super) fn check_s_panel(
    model: &Model,
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
    panel_moment: Option<[f64; 2]>,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    let planes = panel_joint_planes(model, nid, &model.elements);
    if planes.is_empty() {
        return;
    }
    let plane_members = |plane: &PanelPlane| {
        model
            .elements
            .iter()
            .filter(move |e| {
                e.nodes.iter().take(2).any(|n| *n == nid)
                    && (sepika_core::panel_zone::member_orientation(model, e)
                        != Some(MemberOrientation::Beam)
                        || plane.beams.iter().any(|p| p.element == e.id))
            })
            .collect::<Vec<_>>()
    };
    let joints = planes
        .iter()
        .map(|plane| resolve_panel_joint(model, nid, plane_members(plane)))
        .collect::<Vec<_>>();
    let plane_errors = planes
        .iter()
        .zip(&joints)
        .filter_map(|(plane, joint)| {
            joint
                .as_ref()
                .err()
                .map(|reason| (plane.label(), reason.to_string()))
        })
        .collect::<Vec<_>>();
    let Some(seed) = joints
        .iter()
        .find_map(|joint| joint.as_ref().ok().and_then(|j| *j))
    else {
        append_input_diagnostics(nid, &plane_errors, out);
        return;
    };
    if model
        .elements
        .iter()
        .filter(|e| {
            e.nodes.iter().take(2).any(|n| *n == nid)
                && sepika_core::panel_zone::member_orientation(model, e).is_some()
        })
        .any(|e| !sepika_core::structure_kind::member_structure_kind(model, e).is_steel_like())
    {
        return;
    }
    let Some(column) = model.element(seed.column) else {
        return;
    };
    let axes = panel_column_moment_axes(model, column);
    for index in 0..2 {
        let axis_label = if index == 0 { "強軸" } else { "弱軸" };
        let label = format!("パネルゾーン(S) {axis_label}");
        let result = (|| -> Result<_, PanelCheckError> {
            let axes = axes.ok_or("採用柱の曲げ回転軸を解決できません")?;
            let axis = axes[index];
            let horizontal_length = axis[0].hypot(axis[1]);
            if !horizontal_length.is_finite() || horizontal_length == 0.0 {
                return Err("柱主軸の水平投影方向を解決できません".into());
            }
            let participation_direction = [
                axis[0] / horizontal_length,
                axis[1] / horizontal_length,
                0.0,
            ];
            let participating = planes
                .iter()
                .enumerate()
                .filter(|(_, plane)| {
                    dot(
                        [-plane.axis[1], plane.axis[0], 0.0],
                        participation_direction,
                    )
                    .abs()
                        > PANEL_DIRECTION_TOL
                })
                .collect::<Vec<_>>();
            let sources = if participating.is_empty() {
                if planes.len() != 1 {
                    return Err(
                        "参加する梁構面がなく、単一構面の参照フランジを特定できません".into(),
                    );
                }
                vec![(0, &planes[0])]
            } else {
                participating
            };
            let mut flange = None;
            for &(plane_index, plane) in &sources {
                joints[plane_index]
                    .as_ref()
                    .map_err(|reason| PanelCheckError::Joint(reason.to_string()))?
                    .ok_or("接合部の採用柱が未解決です")?;
                plane.standard_applicability(nid)?;
                for beam in &plane.beams {
                    let positions = *beam.flanges.as_ref().map_err(Clone::clone)?;
                    if let Some(previous) = flange {
                        let previous: BeamFlangePositions = previous;
                        if !standard_flange_differences(
                            previous.upper - positions.upper,
                            previous.lower - positions.lower,
                        ) {
                            return Err(format!("柱{axis_label}へ集約する梁構面間のフランジ位置が不一致: 上位置差={} mm、下位置差={} mm。段違い接合の標準形式は未対応",previous.upper-positions.upper,previous.lower-positions.lower).into());
                        }
                    } else {
                        flange = Some(positions);
                    }
                }
            }
            let db = flange.ok_or("梁フランジ位置が未解決です")?.db();
            let col = cols
                .iter()
                .find(|c| c.elem.id == seed.column)
                .ok_or("採用柱の検定用応力が未取得")?;
            let force = col.end_forces(nid).ok_or("採用柱の検定用応力が未取得")?;
            if !force[0].is_finite() {
                return Err("採用柱の軸力は有限値が必要です".into());
            }
            let geometry = PanelGeometry::from_column_axes(col.sec)
                .ok_or("柱2主軸のパネル諸元が未解決")?[index];
            let thickness = match col.sec.shape.as_ref().unwrap() {
                SectionShape::SteelH {
                    web_thick,
                    flange_thick,
                    ..
                } => [*web_thick, *flange_thick][index],
                SectionShape::SteelBox { thick, .. }
                | SectionShape::SteelPipe { thick, .. }
                | SectionShape::CftBox { thick, .. }
                | SectionShape::CftPipe { thick, .. } => *thick,
                _ => return Err("抵抗板の実板厚が未解決".into()),
            };
            if [
                geometry.dc,
                geometry.tp,
                thickness,
                geometry.effective_volume(db),
                geometry.kappa(),
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
            {
                return Err("パネル寸法・実板厚・実効体積・形状係数は有限の正値が必要です".into());
            }
            let material = if geometry.filled {
                col.steel_mat.ok_or_else(|| {
                    format!(
                        "採用柱 ID {} の鋼管専用材料・基準強度Fが未解決",
                        seed.column.0
                    )
                })?
            } else {
                col.mat
            };
            let fy = sepika_core::material_grade::resolve_steel_f(Some(material), Some(thickness))?;
            let demand = if let Some(moment) = panel_moment {
                let analysis_db = planes
                    .iter()
                    .flat_map(|plane| plane.beams.iter())
                    .filter_map(|beam| {
                        model
                            .element(beam.element)
                            .and_then(|e| model.element_section(e))
                    })
                    .filter_map(|section| sepika_core::panel_zone::beam_panel_depth(section).ok())
                    .fold(0.0_f64, f64::max);
                if (analysis_db - db).abs() > 2.0 * sepika_core::panel_zone::FLANGE_POSITION_TOL_MM
                {
                    return Err(format!("解析MS応答の共通db={analysis_db} mmと柱{axis_label}のフランジ間距離db={db} mmが不整合です").into());
                }
                principal_moments(moment, axes)?[index]
            } else {
                let mut total = 0.0;
                for (_, plane) in &sources {
                    let normal = [-plane.axis[1], plane.axis[0], 0.0];
                    let coefficient = dot(normal, axis);
                    if coefficient == 0.0 {
                        continue;
                    }
                    for beam in &plane.beams {
                        let member = beams
                            .iter()
                            .find(|b| b.elem.id == beam.element)
                            .ok_or("梁の検定用応力が未取得")?;
                        let values = member.end_forces(nid).ok_or("梁の検定用応力が未取得")?;
                        let moment = project(model, member, values, 3, normal)?;
                        let end_sign = if member.elem.nodes[0] == nid {
                            -1.0
                        } else {
                            1.0
                        };
                        total += end_sign * moment * coefficient;
                    }
                }
                total + column_correction(model, cols, nid, axis, db)?
            };
            if !demand.is_finite() {
                return Err("設計用パネルモーメントは有限値が必要です".into());
            }
            s_panel_zone_check(&SPanelInput {
                beam_flanges: flange.map(|f| [f; 2]),
                geometry,
                db,
                fy,
                axial_ratio: (-force[0]).max(0.0) / (fy * col.sec.area.max(1e-9)),
                beam_moment_left: 0.0,
                beam_moment_right: 0.0,
                col_shear_upper: 0.0,
                col_shear_lower: 0.0,
                design_moment: Some(demand),
            })
            .map_err(PanelCheckError::Input)
        })();
        let outcome = match result {
            Ok(result) => CheckOutcome::Checked(result),
            Err(PanelCheckError::Joint(reason)) => CheckOutcome::Skipped { reason },
            Err(PanelCheckError::Input(reason)) => CheckOutcome::Skipped {
                reason: format!("接合部 ID {} 柱{axis_label}: {reason}", nid.0),
            },
        };
        out.push((nid, label, outcome));
    }
    append_input_diagnostics(nid, &plane_errors, out);
}

fn append_input_diagnostics(
    node: NodeId,
    errors: &[(String, String)],
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    for (plane, reason) in errors {
        if out.iter().any(|(nid, label, outcome)| {
            *nid == node
                && label.starts_with("パネルゾーン(S)")
                && matches!(outcome,CheckOutcome::Skipped{reason:recorded} if recorded==reason)
        }) {
            continue;
        }
        out.push((
            node,
            format!("パネルゾーン(S) 入力診断 {plane}"),
            CheckOutcome::Skipped {
                reason: reason.clone(),
            },
        ));
    }
}

fn principal_moments(moment: [f64; 2], axes: [[f64; 3]; 2]) -> Result<[f64; 2], String> {
    let determinant = axes[0][0] * axes[1][1] - axes[0][1] * axes[1][0];
    if moment.iter().any(|v| !v.is_finite()) || !determinant.is_finite() || determinant == 0.0 {
        return Err("解析MS応答・柱主軸の逆変換は有限で非退化の入力が必要です".into());
    }
    let principal = [
        (axes[1][1] * moment[0] - axes[1][0] * moment[1]) / determinant,
        (-axes[0][1] * moment[0] + axes[0][0] * moment[1]) / determinant,
    ];
    if principal.iter().any(|v| !v.is_finite()) {
        return Err("柱主軸の解析MS応答が非有限です".into());
    }
    Ok(principal)
}

fn project(
    model: &Model,
    member: &MemberInfo<'_>,
    values: &[f64; 6],
    start: usize,
    direction: [f64; 3],
) -> Result<f64, String> {
    if values[start..start + 3]
        .iter()
        .chain(member.elem.local_axis.ref_vector.iter())
        .any(|v| !v.is_finite())
    {
        return Err(format!(
            "部材 ID {} の応力・断面参照方向は有限値が必要です",
            member.elem.id.0
        ));
    }
    let frame = sepika_element::transform::LocalFrame::from_nodes(
        model.nodes[member.elem.nodes[0].index()].coord,
        model.nodes[member.elem.nodes[1].index()].coord,
        member.elem.local_axis.ref_vector,
    )
    .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", member.elem.id.0))?;
    let result = (0..3)
        .map(|k| values[start + k] * dot(frame.rot[k], direction))
        .sum::<f64>();
    if !result.is_finite() {
        return Err(format!(
            "部材 ID {} の応力射影が非有限です",
            member.elem.id.0
        ));
    }
    Ok(result)
}

fn column_correction(
    model: &Model,
    cols: &[&MemberInfo<'_>],
    nid: NodeId,
    axis: [f64; 3],
    db: f64,
) -> Result<f64, String> {
    let mut total = 0.0;
    for element in model.elements.iter().filter(|e| {
        e.nodes.iter().take(2).any(|n| *n == nid)
            && sepika_core::panel_zone::member_orientation(model, e)
                == Some(MemberOrientation::Column)
    }) {
        let member = cols
            .iter()
            .find(|c| c.elem.id == element.id)
            .ok_or("柱の検定用応力が未取得")?;
        let force = member.end_forces(nid).ok_or("柱の検定用応力が未取得")?;
        let normal =
            sepika_core::panel_zone::member_unit_axis(model, element).ok_or("柱材軸が未解決")?;
        if (normal[2].abs() - 1.0).abs() > PANEL_DIRECTION_TOL {
            return Err(
                "非鉛直柱の材端力からの仕口パネル需要組立は未対応です。解析MS応答が必要です".into(),
            );
        }
        let direction = cross(axis, [0.0, 0.0, 1.0]);
        total += project(model, member, force, 0, direction)? * normal[2].signum() * db / 2.0;
    }
    Ok(total)
}

pub(crate) fn validate_steel_panel_inputs(model: &Model) -> Result<(), String> {
    use sepika_core::panel_zone::{member_orientation, MemberOrientation};
    use sepika_core::structure_kind::member_structure_kind;
    for node in &model.nodes {
        let attached = model
            .elements
            .iter()
            .filter(|e| {
                e.nodes.iter().take(2).any(|id| *id == node.id)
                    && member_orientation(model, e).is_some()
            })
            .collect::<Vec<_>>();
        if !attached
            .iter()
            .any(|e| member_orientation(model, e) == Some(MemberOrientation::Beam))
            || attached
                .iter()
                .any(|e| !member_structure_kind(model, e).is_steel_like())
        {
            continue;
        }
        let columns = attached
            .iter()
            .copied()
            .filter(|e| member_orientation(model, e) == Some(MemberOrientation::Column))
            .collect::<Vec<_>>();
        if columns.is_empty() {
            continue;
        }
        let Ok(column) = sepika_core::panel_zone::select_panel_column(model, node.id, &columns)
        else {
            continue;
        };
        let Some(sec) = model.element_section(column) else {
            continue;
        };
        let thicknesses = match sec.shape.as_ref() {
            Some(SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            }) => vec![*web_thick, *flange_thick],
            Some(
                SectionShape::SteelBox { thick, .. }
                | SectionShape::SteelPipe { thick, .. }
                | SectionShape::CftBox { thick, .. }
                | SectionShape::CftPipe { thick, .. },
            ) => vec![*thick],
            None => {
                return Err(format!(
                    "接合部 {} 採用柱 {}: パネル計算に必要な断面形状が未設定です",
                    node.id.0, column.id.0
                ));
            }
            Some(_) => continue,
        };
        let material = if matches!(
            sec.shape,
            Some(SectionShape::CftBox { .. } | SectionShape::CftPipe { .. })
        ) {
            model.element_steel_material(column)
        } else {
            model.element_material(column)
        };
        crate::material_strength::section_steel_f(sec, material)
            .map_err(|reason| format!("接合部 {} 採用柱 {}: {reason}", node.id.0, column.id.0))?;
        if sec
            .panel_thickness
            .is_some_and(|t| !t.is_finite() || t <= 0.0)
        {
            return Err(format!(
                "接合部 {} 採用柱 {}: パネル有効板厚は正の有限値 [mm] が必要です",
                node.id.0, column.id.0
            ));
        }
        for thickness in thicknesses {
            if !thickness.is_finite() || thickness <= 0.0 {
                return Err(format!(
                    "接合部 {} 採用柱 {}: 抵抗板の実板厚は正の有限値 [mm] が必要です",
                    node.id.0, column.id.0
                ));
            }
            sepika_core::material_grade::resolve_steel_f(material, Some(thickness)).map_err(
                |reason| format!("接合部 {} 採用柱 {}: {reason}", node.id.0, column.id.0),
            )?;
        }
    }
    Ok(())
}
