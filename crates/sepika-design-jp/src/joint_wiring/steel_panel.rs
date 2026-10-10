//! S 造パネルゾーンのせん断検定配線。

use super::common::MemberInfo;
use crate::steel::panel_zone::{s_panel_zone_check, SPanelInput};
use crate::CheckOutcome;
use sepika_core::ids::NodeId;
use sepika_core::model::Model;
use sepika_core::panel_zone::resolve_panel_joint;

/// 面別の標準形式判定を行い、適用済みの面だけ検定する。
pub(super) fn check_s_panel(
    model: &Model,
    cols: &[&MemberInfo<'_>],
    beams: &[&MemberInfo<'_>],
    nid: NodeId,
    panel_moment: Option<[f64; 2]>,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
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
    let project =
        |member: &MemberInfo<'_>, values: &[f64; 6], start: usize, direction: [f64; 3]| {
            let coords = member
                .elem
                .nodes
                .iter()
                .take(2)
                .map(|n| model.nodes[n.index()].coord)
                .collect::<Vec<_>>();
            let frame = sepika_element::transform::LocalFrame::from_nodes(
                coords[0],
                coords[1],
                member.elem.local_axis.ref_vector,
            );
            (0..3)
                .map(|k| values[start + k] * sepika_core::geom::vec3::dot(frame.rot[k], direction))
                .sum::<f64>()
                .abs()
        };
    let planes = sepika_core::panel_zone::panel_joint_planes(model, nid, &model.elements);
    for plane in &planes {
        let plane_beams: Vec<_> = beams
            .iter()
            .copied()
            .filter(|b| plane.beams.iter().any(|p| p.element == b.elem.id))
            .collect();
        let members = model.elements.iter().filter(|e| {
            e.nodes.contains(&nid)
                && (sepika_core::panel_zone::member_orientation(model, e)
                    == Some(sepika_core::panel_zone::MemberOrientation::Column)
                    || plane.beams.iter().any(|p| p.element == e.id))
        });
        let label = if planes.len() == 1 {
            "パネルゾーン(S)".into()
        } else {
            format!("パネルゾーン(S) {}", plane.label())
        };
        let joint = match resolve_panel_joint(model, nid, members) {
            Ok(Some(joint)) => joint,
            Ok(None) => continue,
            Err(reason) => {
                out.push((nid, label, CheckOutcome::Skipped { reason }));
                continue;
            }
        };
        if let Err(reason) = plane.standard_applicability(nid) {
            out.push((nid, label, CheckOutcome::Skipped { reason }));
            continue;
        }
        let Some(col) = cols.iter().find(|c| c.elem.id == joint.column) else {
            out.push((
                nid,
                label,
                CheckOutcome::Skipped {
                    reason: format!(
                        "接合部 ID {} {}: 採用柱の検定用応力が未取得",
                        nid.0,
                        plane.label()
                    ),
                },
            ));
            continue;
        };
        if panel_moment.is_none()
            && plane.beams.iter().any(|p| {
                !plane_beams
                    .iter()
                    .any(|b| b.elem.id == p.element && b.end_forces(nid).is_some())
            })
        {
            out.push((
                nid,
                label,
                CheckOutcome::Skipped {
                    reason: format!(
                        "接合部 ID {} {}: 梁の検定用応力が未取得",
                        nid.0,
                        plane.label()
                    ),
                },
            ));
            continue;
        }
        if col.end_forces(nid).is_none()
            || (panel_moment.is_none()
                && model.elements.iter().any(|e| {
                    e.nodes.contains(&nid)
                        && sepika_core::panel_zone::member_orientation(model, e)
                            == Some(sepika_core::panel_zone::MemberOrientation::Column)
                        && sepika_core::panel_zone::is_horizontal_or_vertical_member(model, e)
                        && !cols
                            .iter()
                            .any(|c| c.elem.id == e.id && c.end_forces(nid).is_some())
                }))
        {
            out.push((
                nid,
                label,
                CheckOutcome::Skipped {
                    reason: format!(
                        "接合部 ID {} {}: 柱の検定用応力が未取得",
                        nid.0,
                        plane.label()
                    ),
                },
            ));
            continue;
        }
        let fy = crate::steel::steel_f_value_prefix(&col.mat.name, 40.0)
            .or(col.mat.fy)
            .unwrap_or(235.0);
        let n_axial = col
            .end_forces(nid)
            .map(|f| (-f[0]).max(0.0) / (fy * col.sec.area.max(1e-9)))
            .unwrap_or(0.0);
        let moment_of = |p: &sepika_core::panel_zone::PanelPlaneBeam| {
            plane_beams
                .iter()
                .find(|b| b.elem.id == p.element)
                .and_then(|b| b.end_forces(nid))
                .map(|f| {
                    let b = plane_beams.iter().find(|b| b.elem.id == p.element).unwrap();
                    project(b, f, 3, [-plane.axis[1], plane.axis[0], 0.0])
                })
                .unwrap_or(0.0)
        };
        let m_left = plane
            .beams
            .iter()
            .filter(|p| sepika_core::geom::vec3::dot(p.outward_axis, plane.axis) < 0.0)
            .map(moment_of)
            .sum();
        let m_right = plane
            .beams
            .iter()
            .filter(|p| sepika_core::geom::vec3::dot(p.outward_axis, plane.axis) > 0.0)
            .map(moment_of)
            .sum();
        let col_shear = |upper: bool| {
            cols.iter()
                .filter(|c| {
                    let Some(axis) = sepika_core::panel_zone::member_unit_axis(model, c.elem)
                    else {
                        return false;
                    };
                    let outward_z = if c.elem.nodes[0] == nid {
                        axis[2]
                    } else {
                        -axis[2]
                    };
                    (outward_z > 0.0) == upper
                })
                .filter_map(|c| c.end_forces(nid).map(|f| project(c, f, 0, plane.axis)))
                .sum()
        };
        let flanges = plane
            .beams
            .iter()
            .map(|b| *b.flanges.as_ref().unwrap())
            .collect::<Vec<_>>();
        let inp = SPanelInput {
            beam_flanges: Some([flanges[0], *flanges.get(1).unwrap_or(&flanges[0])]),
            geometry: joint.geometry,
            db: joint.db,
            fy,
            axial_ratio: n_axial,
            beam_moment_left: m_left,
            beam_moment_right: m_right,
            col_shear_upper: col_shear(true),
            col_shear_lower: col_shear(false),
            design_moment: panel_moment.map(|m| -m[0] * plane.axis[1] + m[1] * plane.axis[0]),
        };
        let outcome = match s_panel_zone_check(&inp) {
            Ok(result) => CheckOutcome::Checked(result),
            Err(reason) => CheckOutcome::Skipped {
                reason: format!("接合部 ID {} {}: {reason}", nid.0, plane.label()),
            },
        };
        out.push((nid, label, outcome));
    }
}
