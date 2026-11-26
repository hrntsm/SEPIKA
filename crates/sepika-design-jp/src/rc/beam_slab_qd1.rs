use super::beam::{
    beam_qd1_face_distance, frame_end_positions, validate_beam_qd1_section_geometry,
};
use super::{axis_props_from_shape, beam_supports_slab};
use crate::material_strength::{main_rebar_grade, rebar_sigma_y_of};
use crate::ultimate::rc_props::RcDirection;
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::geom::vec3::{dist, dot, local_frame_axes, sub};
use sepika_core::ids::SlabId;
use sepika_core::material_grade::rebar_tension_limits;
use sepika_core::model::{
    ElementData, ElementKind, EndCondition, Model, PlateAssignment, SlabShape,
    SlabTopReinforcement, SupportMemberId,
};
use sepika_core::section_shape::SectionShape;

const TOL_MM: f64 = 1e-6;

struct Panel<'a> {
    id: SlabId,
    x_mm: [f64; 2],
    y_mm: [f64; 2],
    thickness_mm: f64,
    fc_n_mm2: f64,
    layout: &'a SlabTopReinforcement,
}

/// 両端それぞれの上端引張時に算入する床板筋の曲げモーメント [N·mm]。
/// 床板がない場合は両端0、寄与が未確認の場合は None。
pub fn beam_qd1_slab_end_moments(
    model: &Model,
    adjacency: &NodeAdjacency,
    elem: &ElementData,
) -> Option<[f64; 2]> {
    if model.slabs.iter().any(|slab| {
        slab.is_attached()
            && model
                .attached_slab_supports(slab)
                .is_ok_and(|supports| supports.iter().any(|part| part.elem == elem.id))
    }) {
        return None;
    }
    if !beam_supports_slab(model, elem) {
        return Some([0.0; 2]);
    }
    let section = model.element_section(elem)?;
    let width_mm = section.width;
    let shape @ SectionShape::RcBeamRect { .. } = section.shape.as_ref()? else {
        return None;
    };
    let props = axis_props_from_shape(shape, RcDirection::Strong, true)?;
    let [start, end] = frame_end_positions(model, elem)?;
    let [ex, ey, ez] = local_frame_axes(start, end, elem.local_axis.ref_vector).ok()?;
    let span_mm = dist(start, end);
    if ey != [0.0, 0.0, 1.0]
        || elem.end_cond != [EndCondition::Fixed; 2]
        || !width_mm.is_finite()
        || width_mm <= 0.0
    {
        return None;
    }
    let project = |coord| {
        let delta = sub(coord, start);
        [dot(delta, ex), dot(delta, ez), dot(delta, ey)]
    };
    let top_z_mm = start[2] + props.d_full / 2.0;
    let face_mm = [
        beam_qd1_face_distance(model, adjacency, elem, 0, ez, None).ok()?,
        span_mm - beam_qd1_face_distance(model, adjacency, elem, 1, ez, None).ok()?,
    ];
    let mat = model.element_rebar_material(elem);
    let grade = main_rebar_grade(mat);
    let sigma_y_n_mm2 = rebar_sigma_y_of(mat);
    let support = SupportMemberId::Primary(elem.id);
    let panel = |id| -> Option<Panel<'_>> {
        let slab = model.slab(id)?;
        if !matches!(slab.shape, SlabShape::Enclosed) {
            return None;
        }
        let layout = slab.plate.top_reinforcement.as_ref()?;
        if layout.monolithic_pour.trim().is_empty() {
            return None;
        }
        let p = model.resolved_slab_placement(id).ok()?;
        let material = model.slab_plate_material(slab)?;
        let fc_n_mm2 = material.fc.filter(|fc| (18.0..=60.0).contains(fc))?;
        if !material.is_standard_concrete()
            || (p.top_z_mm - top_z_mm).abs() > TOL_MM
            || p.thickness_mm > props.d_full
        {
            return None;
        }
        let points = p.top_boundary.into_iter().map(project).collect::<Vec<_>>();
        let x_mm = bounds(&points, 0)?;
        let y_mm = bounds(&points, 1)?;
        if points.len() != 4
            || ![x_mm[0], x_mm[1]].into_iter().all(|x| {
                [y_mm[0], y_mm[1]].into_iter().all(|y| {
                    points
                        .iter()
                        .any(|p| (p[0] - x).abs() <= TOL_MM && (p[1] - y).abs() <= TOL_MM)
                })
            })
        {
            return None;
        }
        Some(Panel {
            id,
            x_mm,
            y_mm,
            thickness_mm: p.thickness_mm,
            fc_n_mm2,
            layout,
        })
    };

    let recorded_layouts = model.slabs.iter().filter_map(|slab| {
        slab.plate
            .top_reinforcement
            .as_ref()
            .map(|layout| (slab.id, layout))
    });
    let mut claimed_sides = [false; 2];
    let mut seen_bars = Vec::new();
    let mut sum_my_nmm = [0.0; 2];
    for region in &model.floor_assignment_regions.regions {
        let Some(edge) = region.boundary.iter().find(|edge| edge.support == support) else {
            continue;
        };
        let id = match region.assignment {
            PlateAssignment::NoPlate => continue,
            PlateAssignment::Unset => return None,
            PlateAssignment::Plate(id) => id,
        };
        if !matches!(edge.span, [0.0, 1.0] | [1.0, 0.0]) {
            return None;
        }
        let p = panel(id)?;
        if p.x_mm[0].abs() > TOL_MM || (p.x_mm[1] - span_mm).abs() > TOL_MM {
            return None;
        }
        let cross_boundary = |at_mm: f64| {
            let mut edges = region.boundary.iter().filter(|edge| {
                model
                    .support_member_axis(edge.support)
                    .is_some_and(|(a, b)| {
                        [a, b]
                            .into_iter()
                            .map(project)
                            .all(|point| (point[0] - at_mm).abs() <= TOL_MM)
                    })
            });
            let edge = edges.next()?;
            let SupportMemberId::Primary(id) = edge.support else {
                return None;
            };
            let crossing = model.element(id)?;
            if edges.next().is_some() {
                return None;
            }
            let [a, b] = model
                .primary_frame_axis_end_positions(crossing)
                .ok()??
                .map(project);
            if [a, b].iter().any(|p| (p[0] - at_mm).abs() > TOL_MM)
                || a[1].min(b[1]) > p.y_mm[0] + TOL_MM
                || a[1].max(b[1]) < p.y_mm[1] - TOL_MM
            {
                return None;
            }
            let width_mm = slab_support_width(model, crossing, top_z_mm, p.thickness_mm)?;
            Some((edge.support, width_mm / 2.0))
        };
        let (side, near, far) = if p.y_mm[0].abs() <= TOL_MM {
            (0, p.y_mm[0], p.y_mm[1])
        } else if p.y_mm[1].abs() <= TOL_MM {
            (1, p.y_mm[1], p.y_mm[0])
        } else {
            return None;
        };
        if std::mem::replace(&mut claimed_sides[side], true) {
            return None;
        }
        let mut opposite_beams = region.boundary.iter().filter_map(|edge| {
            let SupportMemberId::Primary(id) = edge.support else {
                return None;
            };
            let opposite = model.element(id)?;
            let [a, b] = frame_end_positions(model, opposite)?.map(project);
            (opposite.kind == ElementKind::Beam
                && [a, b].iter().all(|point| (point[1] - far).abs() <= TOL_MM))
            .then_some((edge.span, opposite, [a[0], b[0]]))
        });
        let (opposing_span, opposing, [a_mm, b_mm]) = opposite_beams.next()?;
        if opposite_beams.next().is_some()
            || !matches!(opposing_span, [0.0, 1.0] | [1.0, 0.0])
            || a_mm.min(b_mm).abs() > TOL_MM
            || (a_mm.max(b_mm) - span_mm).abs() > TOL_MM
        {
            return None;
        }
        let opposing_width_mm = slab_support_width(model, opposing, top_z_mm, p.thickness_mm)?;
        let clear_width_mm = (far - near).abs() - (width_mm + opposing_width_mm) / 2.0;
        if !opposing_width_mm.is_finite()
            || opposing_width_mm <= 0.0
            || !clear_width_mm.is_finite()
            || clear_width_mm <= 0.0
        {
            return None;
        }
        let flange_width_mm = if clear_width_mm < span_mm / 2.0 {
            (0.5 - 0.6 * clear_width_mm / span_mm) * clear_width_mm
        } else {
            0.1 * span_mm
        };
        let adjacent: Vec<_> = model
            .slabs
            .iter()
            .filter_map(|slab| panel(slab.id))
            .filter(|other| {
                other.layout.monolithic_pour == p.layout.monolithic_pour
                    && other.thickness_mm == p.thickness_mm
                    && other.fc_n_mm2 == p.fc_n_mm2
            })
            .collect();
        let anchorage_end = |at_mm: f64, is_i: bool| {
            let (cross_support, half_width_mm) = cross_boundary(at_mm)?;
            let y_mm = p.y_mm;
            let matches = |other: &&Panel<'_>| {
                other.id != p.id
                    && [0, 1]
                        .into_iter()
                        .all(|i| (other.y_mm[i] - y_mm[i]).abs() <= TOL_MM)
                    && if is_i {
                        (other.x_mm[1] - at_mm).abs() <= TOL_MM
                    } else {
                        (other.x_mm[0] - at_mm).abs() <= TOL_MM
                    }
                    && model
                        .slab_assignment_region(other.id)
                        .is_some_and(|region| {
                            region.boundary.iter().any(|e| e.support == cross_support)
                        })
            };
            let mut panels = adjacent.iter().filter(matches);
            let anchorage_panel = panels.next()?;
            if panels.next().is_some() {
                return None;
            }
            Some((
                anchorage_panel,
                if is_i {
                    at_mm - half_width_mm
                } else {
                    at_mm + half_width_mm
                },
            ))
        };
        let (anchor_i, base_i_mm) = anchorage_end(0.0, true)?;
        let (anchor_j, base_j_mm) = anchorage_end(span_mm, false)?;
        let owner_ids = [p.id, anchor_i.id, anchor_j.id];
        for (owner_id, layout) in recorded_layouts.clone() {
            for bar in &layout.straight_bars {
                let [a, b] = bar.ends_mm.map(project);
                if !a.into_iter().chain(b).all(f64::is_finite) || (a[1] - b[1]).abs() > TOL_MM {
                    continue;
                }
                let rel_top_mm = top_z_mm - start[2];
                let y_mm = (a[1] + b[1]) / 2.0;
                if !(p.y_mm[0]..=p.y_mm[1]).contains(&y_mm)
                    || a[2].max(b[2]) < rel_top_mm - p.thickness_mm
                    || a[2].min(b[2]) > rel_top_mm
                {
                    continue;
                }
                let x_mm = [a[0].min(b[0]), a[0].max(b[0])];
                if x_mm[0] >= face_mm[1] || x_mm[1] <= face_mm[0] {
                    continue;
                }
                let lateral_mm = y_mm.abs() - width_mm / 2.0;
                if x_mm[0] > face_mm[0]
                    || x_mm[1] < face_mm[1]
                    || (!owner_ids.contains(&owner_id) && lateral_mm <= flange_width_mm + TOL_MM)
                {
                    return None;
                }
            }
        }
        for owner in [&p, anchor_i, anchor_j] {
            for bar in &owner.layout.straight_bars {
                let [a, b] = bar.ends_mm.map(project);
                if !a.iter().chain(&b).all(|x| x.is_finite()) {
                    return None;
                }
                let center = (a[1] + b[1]) / 2.0;
                let lateral_mm = center * if side == 0 { 1.0 } else { -1.0 } - width_mm / 2.0;
                if !(-TOL_MM..=flange_width_mm + TOL_MM).contains(&lateral_mm) {
                    continue;
                }
                if (a[1] - b[1]).abs() > TOL_MM {
                    continue;
                }
                let min_x_mm = a[0].min(b[0]);
                let max_x_mm = a[0].max(b[0]);
                let hits = face_mm.map(|x| (min_x_mm..=max_x_mm).contains(&x));
                if !hits.into_iter().any(|hit| hit) {
                    continue;
                }
                let radius_mm = bar.size.nominal_diameter_mm() / 2.0;
                let does_not_overlap = |(other_y, other_z, x): (f64, f64, [f64; 2])| {
                    (other_y - center).abs() > TOL_MM
                        || (other_z - a[2]).abs() > TOL_MM
                        || x[0] >= max_x_mm
                        || x[1] <= min_x_mm
                };
                if bar.grade != grade
                    || bar.yield_strength_n_mm2 != sigma_y_n_mm2
                    || !sigma_y_n_mm2.is_finite()
                    || sigma_y_n_mm2 <= 0.0
                    || (a[2] - b[2]).abs() > TOL_MM
                    || !contains_interior(owner, a, radius_mm, top_z_mm - start[2])
                    || !seen_bars.iter().copied().all(does_not_overlap)
                {
                    return None;
                }
                let (bar_i, bar_j) = if a[0] < b[0] { (a, b) } else { (b, a) };
                if !contains_interior(anchor_i, bar_i, radius_mm, top_z_mm - start[2])
                    || !contains_interior(anchor_j, bar_j, radius_mm, top_z_mm - start[2])
                    || hits != [true, true]
                {
                    return None;
                }
                let panels = [&p, anchor_i, anchor_j];
                let rel_top_mm = top_z_mm - start[2];
                if !panels
                    .iter()
                    .all(|panel| contains_section(panel, center, a[2], radius_mm, rel_top_mm))
                {
                    return None;
                }
                seen_bars.push((center, a[2], [min_x_mm, max_x_mm]));
                let d_mm = a[2] + props.d_full / 2.0;
                let db_mm = bar.size.designation_number();
                if !matches!(grade, "SD295" | "SD345" | "SD390" | "SD490") {
                    return None;
                }
                let (long_ft_n_mm2, short_ft_n_mm2) = rebar_tension_limits(grade, db_mm)?;
                let bond_yield_n_mm2 = bar
                    .bond_yield_strength_n_mm2
                    .filter(|fy| fy.is_finite() && *fy > sigma_y_n_mm2.max(short_ft_n_mm2))?;
                let required_length = |panel: &Panel<'_>, through: bool| {
                    let min_side_cover_mm = [
                        center - panel.y_mm[0] - radius_mm,
                        panel.y_mm[1] - center - radius_mm,
                        rel_top_mm - a[2] - radius_mm,
                        a[2] - (rel_top_mm - panel.thickness_mm) - radius_mm,
                    ]
                    .into_iter()
                    .fold(f64::INFINITY, f64::min);
                    if min_side_cover_mm <= 0.0 {
                        return None;
                    }
                    let mut c_mm = (3.0 * min_side_cover_mm).min(5.0 * db_mm);
                    for (other_owner, other) in recorded_layouts.clone().flat_map(|(id, layout)| {
                        layout.straight_bars.iter().map(move |bar| (id, bar))
                    }) {
                        if std::ptr::eq(other, bar) {
                            continue;
                        }
                        let [e, f] = other.ends_mm.map(project);
                        if !e.iter().chain(&f).all(|coord| coord.is_finite()) {
                            if owner_ids.contains(&other_owner) {
                                return None;
                            }
                            continue;
                        }
                        if e[0].max(f[0]) <= panel.x_mm[0]
                            || e[0].min(f[0]) >= panel.x_mm[1]
                            || e[1].max(f[1]) < panel.y_mm[0]
                            || e[1].min(f[1]) > panel.y_mm[1]
                            || e[2].min(f[2]) > rel_top_mm
                            || e[2].max(f[2]) < rel_top_mm - panel.thickness_mm
                        {
                            continue;
                        }
                        if e[2].max(f[2]) > a[2] + TOL_MM {
                            return None;
                        }
                        if (e[1] - f[1]).abs() > TOL_MM {
                            continue;
                        }
                        if (e[2] - a[2]).abs() > TOL_MM || (f[2] - a[2]).abs() > TOL_MM {
                            return None;
                        }
                        let gap_mm = (e[1] - center).abs()
                            - radius_mm
                            - other.size.nominal_diameter_mm() / 2.0;
                        if gap_mm <= 0.0 {
                            return None;
                        }
                        c_mm = c_mm.min(gap_mm);
                    }
                    let top_bar = a[2] - (rel_top_mm - panel.thickness_mm) >= 300.0;
                    let cover_factor = (min_side_cover_mm / (1.5 * db_mm)).min(1.0);
                    let fa_n_mm2 = |long| {
                        cover_factor * slab_concrete_allowable_bond(panel.fc_n_mm2, top_bar, long)
                    };
                    let fb_n_mm2 =
                        (if top_bar { 0.8 } else { 1.0 }) * (panel.fc_n_mm2 / 40.0 + 0.9);
                    // 横補強筋や仕口拘束による強度の増加には依存しない。
                    let k = 0.3 * (c_mm / db_mm) + 0.4;
                    let (allowable_factor, stress_factor) =
                        if through { (0.8, 2.0) } else { (1.0, 1.0) };
                    let required_at_allowable_mm =
                        |ft, fa| ft * db_mm / (4.0 * allowable_factor * fa);
                    let length_mm = required_at_allowable_mm(long_ft_n_mm2, fa_n_mm2(true))
                        .max(required_at_allowable_mm(short_ft_n_mm2, fa_n_mm2(false)))
                        .max(stress_factor * bond_yield_n_mm2 * db_mm / (4.0 * k * fb_n_mm2))
                        .max(300.0);
                    length_mm.is_finite().then_some(length_mm)
                };
                let checked_i_mm = face_mm[0].max(-base_i_mm);
                let checked_j_mm = face_mm[1].min(2.0 * span_mm - base_j_mm);
                if base_i_mm - min_x_mm < required_length(anchor_i, false)?
                    || max_x_mm - base_j_mm < required_length(anchor_j, false)?
                    || checked_j_mm - checked_i_mm < required_length(&p, true)? + d_mm
                {
                    return None;
                }
                for moment in &mut sum_my_nmm {
                    *moment += 0.9 * bar.size.area_mm2() * sigma_y_n_mm2 * d_mm;
                }
            }
        }
    }
    if !claimed_sides.into_iter().any(|side| side) || !sum_my_nmm.into_iter().all(f64::is_finite) {
        return None;
    }
    Some(sum_my_nmm)
}

fn bounds(points: &[[f64; 3]], coordinate: usize) -> Option<[f64; 2]> {
    let mut values = points.iter().map(|p| p[coordinate]);
    let start = values.next()?;
    let [min, max] = values.try_fold([start, start], |[min, max], x| {
        x.is_finite().then_some([min.min(x), max.max(x)])
    })?;
    (start.is_finite() && max - min > TOL_MM).then_some([min, max])
}

fn contains_interior(panel: &Panel<'_>, coord: [f64; 3], radius_mm: f64, top_mm: f64) -> bool {
    (panel.x_mm[0] + radius_mm..=panel.x_mm[1] - radius_mm).contains(&coord[0])
        && contains_section(panel, coord[1], coord[2], radius_mm, top_mm)
}

fn contains_section(panel: &Panel<'_>, y_mm: f64, z_mm: f64, radius_mm: f64, top_mm: f64) -> bool {
    (panel.y_mm[0] + radius_mm..=panel.y_mm[1] - radius_mm).contains(&y_mm)
        && (top_mm - panel.thickness_mm + radius_mm..=top_mm - radius_mm).contains(&z_mm)
}

fn slab_support_width(
    model: &Model,
    elem: &ElementData,
    slab_top_z_mm: f64,
    slab_thickness_mm: f64,
) -> Option<f64> {
    if elem.kind != ElementKind::Beam
        || !model
            .element_material(elem)
            .is_some_and(|mat| mat.is_standard_concrete())
    {
        return None;
    }
    validate_beam_qd1_section_geometry(model, elem).ok()?;
    let section = model.element_section(elem)?;
    let SectionShape::RcBeamRect { b, d, .. } = *section.shape.as_ref()? else {
        return None;
    };
    let [start, end] = model.primary_frame_axis_end_positions(elem).ok()??;
    let [_, vertical, _] = local_frame_axes(start, end, elem.local_axis.ref_vector).ok()?;
    let top_z_mm = start[2] + d / 2.0;
    (vertical == [0.0, 0.0, 1.0]
        && b.is_finite()
        && b > 0.0
        && d.is_finite()
        && d >= slab_thickness_mm
        && (top_z_mm - slab_top_z_mm).abs() <= TOL_MM)
        .then_some(b)
}

fn slab_concrete_allowable_bond(fc_n_mm2: f64, top_bar: bool, long_term: bool) -> f64 {
    let long_n_mm2 = if top_bar {
        (fc_n_mm2 / 15.0).min(0.9 + 2.0 * fc_n_mm2 / 75.0)
    } else {
        (fc_n_mm2 / 10.0).min(1.35 + fc_n_mm2 / 25.0)
    };
    long_n_mm2 * if long_term { 1.0 } else { 1.5 }
}
