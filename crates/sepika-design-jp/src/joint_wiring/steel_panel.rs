//! S/CFT 造仕口パネルの柱2主軸への検定配線。

use super::common::MemberInfo;
use crate::steel::panel_zone::{check_panel, supported_steel_panel_yield, SPanelInput};
use crate::CheckOutcome;
use sepika_core::geom::vec3::{cross, dot};
use sepika_core::ids::NodeId;
use sepika_core::model::Model;
use sepika_core::panel_zone::{
    panel_column_moment_axes, panel_joint_planes, resolve_panel_joint, standard_flange_differences,
    steel_panel_column_sections, BeamFlangePositions, MemberOrientation, PanelColumnSection,
    PanelGeometry, PanelPlane, PanelShapeKind, PANEL_DIRECTION_TOL,
};
use sepika_core::section_shape::SectionShape;
use sepika_core::structure_kind::{member_structure_kind, StructureKind};
use sepika_solver::statics::linear::SteelPanelColumnFaceForce;

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
    steel_panel_face_forces: Option<&[SteelPanelColumnFaceForce]>,
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
    let attached = model
        .elements
        .iter()
        .filter(|e| {
            e.nodes.iter().take(2).any(|n| *n == nid)
                && sepika_core::panel_zone::member_orientation(model, e).is_some()
        })
        .collect::<Vec<_>>();
    if attached
        .iter()
        .any(|e| !member_structure_kind(model, e).is_steel_like())
    {
        return;
    }
    let pure_steel = attached
        .iter()
        .all(|e| member_structure_kind(model, e) == StructureKind::S);
    if pure_steel
        && attached
            .iter()
            .filter(|e| {
                sepika_core::panel_zone::member_orientation(model, e)
                    == Some(MemberOrientation::Column)
            })
            .count()
            != 2
    {
        if has_panel_action(panel_moment, cols, beams, nid) {
            append_scope_diagnostic(nid, "上下に柱が1本ずつある標準形式の対象外です", out);
        }
        append_input_diagnostics(nid, &plane_errors, out);
        return;
    }
    let Some(column) = model.element(seed.column) else {
        return;
    };
    let axes = panel_column_moment_axes(model, column);
    let geometry = model
        .element_section(column)
        .and_then(PanelGeometry::from_column_axes);
    let circular =
        pure_steel && geometry.is_some_and(|directions| directions[0].kind == PanelShapeKind::Pipe);
    let square = pure_steel
        && geometry.is_some_and(|directions| directions[0].kind == PanelShapeKind::SquareBox);
    if pure_steel && geometry.is_some_and(|directions| !supported_steel_panel_yield(directions[0]))
    {
        if has_panel_action(panel_moment, cols, beams, nid) {
            append_scope_diagnostic(nid, "標準形式の対象断面ではありません", out);
        }
        append_input_diagnostics(nid, &plane_errors, out);
        return;
    }
    let mut demands = [0.0; 2];
    let mut axis_results = Vec::new();
    for index in 0..if circular { 1 } else { 2 } {
        let unsupported_axis = pure_steel
            && geometry.is_some_and(|directions| !supported_steel_panel_yield(directions[index]));
        let axis_label = if circular {
            "合成"
        } else if index == 0 {
            "強軸"
        } else {
            "弱軸"
        };
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
            let sources = if circular {
                planes.iter().enumerate().collect()
            } else if participating.is_empty() {
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
            let flange = flange.ok_or("梁フランジ位置が未解決です")?;
            let db = flange.db();
            let col = cols.iter().find(|c| c.elem.id == seed.column);
            let axial_n = if unsupported_axis {
                None
            } else if pure_steel {
                let cuts = steel_panel_column_sections(
                    model,
                    nid,
                    seed.column,
                    flange,
                    attached.iter().copied(),
                )?;
                Some(mean_panel_axial_n(cuts, cols, steel_panel_face_forces)?.abs())
            } else {
                Some((-panel_end_axial_n(col.ok_or("採用柱の検定用応力が未取得")?, nid)?).max(0.0))
            };
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
                if circular {
                    moment[0].hypot(moment[1])
                } else {
                    principal_moments(moment, axes)?[index]
                }
            } else if circular {
                let strong =
                    directional_panel_demand(model, cols, beams, nid, &sources, axes[0], db)?;
                let weak =
                    directional_panel_demand(model, cols, beams, nid, &sources, axes[1], db)?;
                strong.hypot(weak)
            } else {
                directional_panel_demand(model, cols, beams, nid, &sources, axis, db)?
            };
            if !demand.is_finite() {
                return Err("設計用パネルモーメントは有限値が必要です".into());
            }
            demands[index] = demand;
            if unsupported_axis {
                return Ok(None);
            }
            let col = col.ok_or("採用柱の検定用応力が未取得")?;
            let axial_n = axial_n.ok_or("パネルの軸力が未取得")?;
            let geometry = geometry.ok_or("柱2主軸のパネル諸元が未解決")?[index];
            let thickness = match col.sec.shape.as_ref().unwrap() {
                SectionShape::SteelH {
                    web_thick,
                    flange_thick,
                    ..
                } => [*web_thick, *flange_thick][index],
                SectionShape::SteelBox {
                    thick,
                    horizontal_thick,
                    ..
                }
                | SectionShape::CftBox {
                    thick,
                    horizontal_thick,
                    ..
                } => [*thick, *horizontal_thick][index],
                SectionShape::SteelPipe { thick, .. } | SectionShape::CftPipe { thick, .. } => {
                    *thick
                }
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
            let area_mm2 = col.sec.area;
            if !area_mm2.is_finite() || area_mm2 <= 0.0 {
                return Err("採用柱の断面積Aは有限の正値 [mm²] が必要です".into());
            }
            let axial_capacity_n = fy * area_mm2;
            if !axial_capacity_n.is_finite() || axial_capacity_n <= 0.0 {
                return Err("採用柱の基準軸耐力F×Aは有限の正値 [N] が必要です".into());
            }
            let axial_ratio = axial_n / axial_capacity_n;
            if !axial_ratio.is_finite() || (axial_n > 0.0 && axial_ratio == 0.0) {
                return Err("パネルの軸力比N/(F×A)を有限の正値で表現できません".into());
            }
            let input = SPanelInput {
                beam_flanges: Some([flange; 2]),
                geometry,
                db,
                fy,
                axial_ratio,
                beam_moment_left: 0.0,
                beam_moment_right: 0.0,
                col_shear_upper: 0.0,
                col_shear_lower: 0.0,
                design_moment: Some(demand),
            };
            let mut result = check_panel(&input, pure_steel)?;
            if pure_steel {
                if let Some((plastic_moment, _)) =
                    geometry.steel_plastic_moment(db, fy, area_mm2, axial_ratio, thickness)
                {
                    result.detail = format!("（参考）全塑性耐力 pMp={plastic_moment:.1} N·mm");
                }
            }
            Ok(Some(result))
        })();
        if unsupported_axis {
            let known_demands = axes.and_then(|axes| {
                panel_moment.and_then(|moment| principal_moments(moment, axes).ok())
            });
            let observed = known_demands.unwrap_or(demands);
            if observed[index].abs() > moment_roundoff(observed)
                || (known_demands.is_none()
                    && result.is_err()
                    && has_panel_action(panel_moment, cols, beams, nid))
            {
                append_scope_diagnostic(nid, "H形弱軸に作用があり標準形式の検定対象外です", out);
            }
            continue;
        }
        let outcome = match result {
            Ok(Some(result)) => CheckOutcome::Checked(result),
            Ok(None) => continue,
            Err(PanelCheckError::Joint(reason)) => CheckOutcome::Skipped { reason },
            Err(PanelCheckError::Input(reason)) => CheckOutcome::Skipped {
                reason: format!("接合部 ID {} 柱{axis_label}: {reason}", nid.0),
            },
        };
        axis_results.push((nid, label, outcome));
    }
    let combined_demand = panel_moment
        .and_then(|moment| axes.and_then(|axes| principal_moments(moment, axes).ok()))
        .unwrap_or(demands);
    let tolerance = moment_roundoff(combined_demand);
    if square && combined_demand.iter().all(|value| value.abs() > tolerance) {
        axis_results.retain(|(_, _, outcome)| matches!(outcome, CheckOutcome::Skipped { .. }));
        append_scope_diagnostic(
            nid,
            "正方形等厚鋼管の45度または二方向同時作用に対する初降伏検定は標準形式の対象外です",
            out,
        );
    }
    out.extend(axis_results);
    append_input_diagnostics(nid, &plane_errors, out);
}

fn moment_roundoff(moment: [f64; 2]) -> f64 {
    32.0 * f64::EPSILON * moment[0].abs().max(moment[1].abs())
}

fn directional_panel_demand(
    model: &Model,
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
    sources: &[(usize, &PanelPlane)],
    axis: [f64; 3],
    db: f64,
) -> Result<f64, String> {
    let mut total = 0.0;
    for (_, plane) in sources {
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
    Ok(total + column_correction(model, cols, nid, axis, db)?)
}

fn has_panel_action(
    panel_moment: Option<[f64; 2]>,
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
) -> bool {
    if let Some(moment) = panel_moment {
        return moment.iter().any(|value| *value != 0.0);
    }
    cols.iter()
        .filter_map(|member| member.end_forces(nid))
        .any(|f| f[1] != 0.0 || f[2] != 0.0)
        || beams
            .iter()
            .filter_map(|member| member.end_forces(nid))
            .any(|f| f[4] != 0.0 || f[5] != 0.0)
}

fn append_scope_diagnostic(
    node: NodeId,
    reason: &str,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    out.push((
        node,
        "仕口パネル（S） 適用範囲".into(),
        CheckOutcome::Skipped {
            reason: format!("接合部 ID {}: {reason}", node.0),
        },
    ));
}

fn panel_end_axial_n(column: &MemberInfo<'_>, node: NodeId) -> Result<f64, String> {
    let force = column
        .end_forces(node)
        .ok_or_else(|| format!("柱 ID {} の検定用応力が未取得", column.elem.id.0))?;
    if force[0].is_finite() {
        Ok(force[0])
    } else {
        Err(format!(
            "柱 ID {} の軸力は有限値が必要です",
            column.elem.id.0
        ))
    }
}

fn mean_panel_axial_n(
    cuts: [PanelColumnSection; 2],
    columns: &[&MemberInfo<'_>],
    steel_panel_face_forces: Option<&[SteelPanelColumnFaceForce]>,
) -> Result<f64, String> {
    let force = |cut: PanelColumnSection| {
        let missing = || {
            format!(
                "柱 ID {} の仕口フランジ位置 {} mm の軸力が未取得",
                cut.element.0, cut.position_mm
            )
        };
        let value = match steel_panel_face_forces {
            Some(rows) => rows
                .iter()
                .find(|row| row.element == cut.element && row.position_mm == cut.position_mm)
                .ok_or_else(missing)?
                .axial_n
                .clone()?,
            None => {
                let position = cut.position_mm / cut.length_mm;
                columns
                    .iter()
                    .find(|column| column.elem.id == cut.element)
                    .and_then(|column| column.forces.iter().find(|(xi, _)| *xi == position))
                    .ok_or_else(missing)?
                    .1[0]
            }
        };
        if !value.is_finite() {
            return Err(format!(
                "柱 ID {} の仕口フランジ位置で軸力は有限値が必要です",
                cut.element.0
            ));
        }
        Ok(value)
    };
    let first = force(cuts[0])?;
    let second = force(cuts[1])?;
    let mean = first.midpoint(second);
    if mean == 0.0 && first != -second {
        return Err("上下柱の軸力平均の非零値を表現できません".into());
    }
    Ok(mean)
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
                SectionShape::SteelBox {
                    thick,
                    horizontal_thick,
                    ..
                }
                | SectionShape::CftBox {
                    thick,
                    horizontal_thick,
                    ..
                },
            ) => vec![*thick, *horizontal_thick],
            Some(SectionShape::SteelPipe { thick, .. } | SectionShape::CftPipe { thick, .. }) => {
                vec![*thick]
            }
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
