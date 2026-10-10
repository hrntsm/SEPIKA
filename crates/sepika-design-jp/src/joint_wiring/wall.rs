//! 耐震壁（Wall 要素 × RcWall 形状）のせん断検定配線。

use super::common::{ForcesAt, MemberInfo};
use crate::rc::wall::{rc_wall_shear_check, RcWallInput, WallSideColumn};
use crate::rc::wall_nonlinear::{wall_shear_trilinear, WallShearTrilinearInput};
use crate::wall_opening::equivalent_opening;
use crate::{CheckComponent, CheckKind, CheckOutcome, CheckResult, LoadTerm};
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementKind, Model};
use sepika_core::section_shape::SectionShape;

/// 耐震壁（Wall 要素 × RcWall 形状）のせん断検定を一括で `out` へ追加する。
pub(super) fn check_walls(
    model: &Model,
    member_forces: &[(ElemId, ForcesAt<'_>)],
    members: &[MemberInfo<'_>],
    term: LoadTerm,
    out: &mut Vec<(NodeId, String, CheckOutcome)>,
) {
    for (eid, forces) in member_forces {
        let Some(elem) = model.element(*eid) else {
            continue;
        };
        if elem.kind != ElementKind::Wall {
            continue;
        }
        let Some(sec) = model.element_section(elem) else {
            continue;
        };
        let Some(SectionShape::RcWall {
            thickness,
            ps,
            pwh_ratio,
        }) = sec.shape
        else {
            continue;
        };
        let Some(mat) = model.element_material(elem) else {
            for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
                out.push((
                    elem.nodes[0],
                    label.into(),
                    CheckOutcome::Skipped {
                        reason: format!(
                            "耐震壁 ID {} のコンクリート材料が未割当または解決不能です（Fc不足）",
                            elem.id.0
                        ),
                    },
                ));
            }
            continue;
        };
        if mat.category != sepika_core::model::MaterialCategory::Concrete {
            for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
                out.push((
                    elem.nodes[0],
                    label.into(),
                    CheckOutcome::Skipped {
                        reason: format!(
                            "耐震壁 ID {} のコンクリート主材 ID {} の材料区分 {:?} はコンクリート役割に不適合です",
                            elem.id.0, mat.id.0, mat.category
                        ),
                    },
                ));
            }
            continue;
        }
        let fc = mat.fc.unwrap_or(0.0);
        if !fc.is_finite() || fc <= 0.0 {
            for label in ["耐震壁(RC)", "耐震壁(RC)せん断非線形"] {
                out.push((elem.nodes[0], label.into(), CheckOutcome::Skipped {
                    reason: format!("耐震壁 ID {} のコンクリート材料 ID {} の Fc が欠落または有限の正値ではありません", elem.id.0, mat.id.0),
                }));
            }
            continue;
        }
        let wall_shear_mat = model.element_shear_rebar_material(elem);
        let wall_vertical_mat = model.element_rebar_material(elem);
        let coords: Vec<[f64; 3]> = elem
            .nodes
            .iter()
            .filter_map(|nid| model.nodes.get(nid.index()))
            .map(|n| n.coord)
            .collect();
        if coords.len() < 3 {
            continue;
        }
        let mut l = 0.0_f64;
        for i in 0..coords.len() {
            for jj in (i + 1)..coords.len() {
                let dx = coords[i][0] - coords[jj][0];
                let dy = coords[i][1] - coords[jj][1];
                l = l.max((dx * dx + dy * dy).sqrt());
            }
        }
        if l < 1e-9 {
            continue;
        }
        let h = coords.iter().map(|c| c[2]).fold(f64::MIN, f64::max)
            - coords.iter().map(|c| c[2]).fold(f64::MAX, f64::min);

        let attr = model.wall_attrs.iter().find(|w| w.elem == elem.id);

        let (mut l0p, mut h0p) = if h > 1e-9 && l > 1e-9 {
            match attr.and_then(|a| a.opening_dims_for(model.multi_opening_mode)) {
                Some(dims) if dims.len() == 1 => dims[0],
                Some(dims) => equivalent_opening(&dims, l, h),
                None => {
                    let area = attr
                        .map(|a| a.total_opening_area_for(model.multi_opening_mode))
                        .unwrap_or(0.0);
                    if area > 0.0 {
                        equivalent_opening(&[(area / h, h)], l, h)
                    } else {
                        (0.0, 0.0)
                    }
                }
            }
        } else {
            (0.0, 0.0)
        };
        l0p = l0p.clamp(0.0, l);
        h0p = h0p.clamp(0.0, h);

        if !sepika_element::wall::misc_wall::wall_is_seismic(elem, model) {
            continue;
        }
        if !sepika_element::wall::misc_wall::is_rc_wall(elem, model) {
            continue;
        }
        let sigma_wh = match wall_rebar_strength(wall_shear_mat, "横筋") {
            Ok(strength) => Some(strength),
            Err(msg) => {
                let labels: &[&str] = if term == LoadTerm::Long {
                    &["耐震壁(RC)せん断非線形"]
                } else {
                    &["耐震壁(RC)", "耐震壁(RC)せん断非線形"]
                };
                for label in labels {
                    out.push((
                        elem.nodes[0],
                        (*label).into(),
                        CheckOutcome::Skipped {
                            reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
                        },
                    ));
                }
                if term != LoadTerm::Long {
                    continue;
                }
                None
            }
        };
        let mut allowable_issue = if term == LoadTerm::Long {
            None
        } else {
            wall_rebar_strength(wall_vertical_mat, "縦筋").err()
        };
        if term != LoadTerm::Long && (!ps.is_finite() || ps < 0.0) {
            allowable_issue = Some("直交最小筋比 ps が有限の非負値ではありません".into());
        }
        let wall_nodes = &elem.nodes;
        let mut side_columns = Vec::new();
        let mut sum_col_depth = 0.0;
        let mut nonlinear_issue = (mat.concrete_class != sepika_core::units::ConcreteClass::Normal)
            .then(|| "通常コンクリート以外の参考骨格は未対応です".to_string());
        let mut column_geometry = [None; 2];
        let wall_geometry = sepika_core::model::wall_element_geometry(elem, model);
        if !wall_geometry.as_ref().is_some_and(|geometry| {
            sepika_core::geom::vec3::unit_from(geometry.bottom_center, geometry.top_center)
                .is_some_and(|direction| (direction[2].abs() - 1.0).abs() <= 1e-9)
                && geometry.ex_bottom[2].abs() <= 1e-9
        }) {
            nonlinear_issue = Some("原研究で適用未確認の傾斜壁または不正な壁幾何の参考骨格は未対応です（鉛直壁のみ対応）".into());
        }
        let mut col_main_area_max = 0.0_f64;
        let mut dc_max = 0.0_f64;
        for m in members {
            if !m.is_column() {
                continue;
            }
            let n0 = m.elem.nodes[0];
            let n1 = m.elem.nodes[1];
            if !(wall_nodes.contains(&n0) && wall_nodes.contains(&n1)) {
                continue;
            }
            let steel_shear = match m.sec.shape {
                Some(
                    SectionShape::SrcBeamRect {
                        steel_height,
                        steel_web_thick,
                        steel_flange_thick,
                        ..
                    }
                    | SectionShape::SrcColumnRect {
                        steel_height,
                        steel_web_thick,
                        steel_flange_thick,
                        ..
                    },
                ) => {
                    let as_web =
                        (steel_web_thick * (steel_height - 2.0 * steel_flange_thick)).max(0.0);
                    let steel_name = m.steel_mat.map(|mm| mm.name.as_str()).unwrap_or("");
                    let f = crate::steel::steel_f_value_prefix(
                        steel_name,
                        steel_flange_thick.max(steel_web_thick),
                    )
                    .unwrap_or(235.0);
                    crate::steel::steel_fs(f, term) * as_web
                }
                _ => 0.0,
            };
            let Some((b, d, d_eff, pw, _)) = wall_side_column_props(m.sec.shape.as_ref()) else {
                continue;
            };
            if let Err(msg) = wall_rebar_strength(m.shear_mat, "側柱帯筋") {
                if term != LoadTerm::Long {
                    allowable_issue = Some(format!("側柱 ID {} の{}", m.elem.id.0, msg));
                }
            }
            side_columns.push(WallSideColumn {
                b,
                d_eff,
                pw,
                w_ft: crate::rc::rebar_allowable_shear(
                    m.shear_mat.map(|mm| mm.name.as_str()).unwrap_or(""),
                    term == LoadTerm::Long,
                ),
                steel_shear,
            });
            sum_col_depth += d;
        }
        let mut reference_column_shape = None;
        let mut reference_column_depth = 0.0;
        if let Some(geometry) = &wall_geometry {
            for (side, column_dimensions) in column_geometry.iter_mut().enumerate() {
                let edge = [geometry.bottom[side], geometry.top[side]];
                let columns: Vec<_> = model
                    .elements
                    .iter()
                    .filter(|column| {
                        sepika_element::wall::side_column::is_line_member(column.kind)
                            && column.nodes.len() == 2
                            && column.nodes.contains(&edge[0])
                            && column.nodes.contains(&edge[1])
                    })
                    .collect();
                if columns.len() > 1 {
                    nonlinear_issue = Some("同じ壁辺に複数の側柱がある参考骨格は未対応です".into());
                }
                for column in columns {
                    let Some(section) = model.element_section(column) else {
                        nonlinear_issue = Some(format!(
                            "側柱 ID {} の断面が未割当または解決不能です",
                            column.id.0
                        ));
                        continue;
                    };
                    let Some(column_mat) = model.element_material(column) else {
                        nonlinear_issue = Some(format!(
                            "側柱 ID {} のコンクリート主材が未割当または解決不能です",
                            column.id.0
                        ));
                        continue;
                    };
                    if column_mat.category != mat.category
                        || column_mat.concrete_class != mat.concrete_class
                        || column_mat.fc != mat.fc
                        || column_mat.young != mat.young
                        || column_mat.poisson != mat.poisson
                        || column_mat.shear != mat.shear
                    {
                        nonlinear_issue = Some(format!("側柱 ID {} のコンクリート主材 ID {} と壁の材料領域・コンクリート種別が異なる参考骨格は未対応です", column.id.0, column_mat.id.0));
                    }
                    let Some(shape @ SectionShape::RcColumnRect { b, d, rebar }) =
                        section.shape.as_ref()
                    else {
                        nonlinear_issue = Some(format!("側柱 ID {} の主筋量・骨格用有効断面を解決できません（同質の正方形RC側柱のみ対応）", column.id.0));
                        continue;
                    };
                    if b != d {
                        nonlinear_issue = Some(format!(
                            "側柱 ID {} の骨格用有効断面は未対応です（正方形RC側柱のみ対応）",
                            column.id.0
                        ));
                    }
                    if !side_column_section_is_parallel(column, model, geometry) {
                        nonlinear_issue = Some(format!("側柱 ID {} の断面方向が壁面内方向・壁面法線に平行ではないため、骨格用有効断面は未対応です", column.id.0));
                    }
                    if reference_column_shape.is_some_and(|first| first != shape) {
                        nonlinear_issue = Some(format!(
                            "側柱 ID {} の断面寸法・配筋が反対側の柱と非対称な参考骨格は未対応です",
                            column.id.0
                        ));
                    }
                    reference_column_shape = Some(shape);
                    *column_dimensions = Some([*d, *b]);
                    reference_column_depth += d;
                    dc_max = dc_max.max(*d);
                    col_main_area_max = col_main_area_max.max(rebar.total_main_area());
                }
            }
        }
        if nonlinear_issue.is_none() && column_geometry[0].is_some() != column_geometry[1].is_some()
        {
            nonlinear_issue = Some("片側のみ側柱がある非対称な参考骨格は未対応です".into());
        }
        let l_clear = (l - sum_col_depth / 2.0).max(0.1 * l);
        let q_design = forces
            .iter()
            .map(|(_, f)| f[1].abs().max(f[2].abs()))
            .fold(0.0, f64::max);
        let inp = RcWallInput {
            t: thickness,
            l,
            l_clear,
            fc,
            concrete_class: mat.concrete_class,
            ps,
            w_ft: crate::rc::rebar_allowable_shear(
                wall_shear_mat.map(|mm| mm.name.as_str()).unwrap_or(""),
                term == LoadTerm::Long,
            )
            .min(crate::rc::rebar_allowable_shear(
                wall_vertical_mat.map(|mm| mm.name.as_str()).unwrap_or(""),
                term == LoadTerm::Long,
            )),
            side_columns,
            opening: if l0p > 1e-9 && h0p > 1e-9 {
                Some((l0p, h0p, h, l))
            } else {
                None
            },
            q_design,
            long_term: term == LoadTerm::Long,
        };
        let outcome = match allowable_issue {
            Some(msg) => CheckOutcome::Skipped {
                reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
            },
            None => CheckOutcome::Checked(rc_wall_shear_check(&inp)),
        };
        out.push((elem.nodes[0], "耐震壁(RC)".into(), outcome));

        let Some(sigma_wh) = sigma_wh else {
            continue;
        };
        let horizontal_ratio = match pwh_ratio {
            Some(ratio) if ratio.is_finite() && ratio >= 0.0 && ps.is_finite() && ratio >= ps => {
                Ok(ratio)
            }
            Some(_) => Err("横筋比 pwh_ratio が有限・非負・直交最小筋比以上ではありません"),
            None => Err("横筋比 pwh_ratio が未入力です（直交最小筋比 ps からは推定しません）"),
        };
        let pwh_ratio = match horizontal_ratio {
            Ok(ratio) => ratio,
            Err(msg) => {
                out.push((
                    elem.nodes[0],
                    "耐震壁(RC)せん断非線形".into(),
                    CheckOutcome::Skipped {
                        reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
                    },
                ));
                continue;
            }
        };
        if !(0.33..=1.63).contains(&(h / l)) {
            out.push((
                elem.nodes[0],
                "耐震壁(RC)せん断非線形".into(),
                CheckOutcome::Skipped {
                    reason: format!(
                        "耐震壁 ID {} の高さ/壁長が剛性比の原研究範囲 0.33〜1.63 外です",
                        elem.id.0
                    ),
                },
            ));
            continue;
        }
        let section_properties = sepika_core::section_shape::wall_rectangular_section_properties(
            l,
            thickness,
            column_geometry,
        );
        let aw = section_properties.map(|p| p.area).unwrap_or(0.0);
        let shear_area_mm2 = section_properties.map(|p| p.shear_area).unwrap_or(0.0);
        if let Some(msg) = nonlinear_issue {
            out.push((
                elem.nodes[0],
                "耐震壁(RC)せん断非線形".into(),
                CheckOutcome::Skipped {
                    reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
                },
            ));
            continue;
        }
        let d_wall = l + reference_column_depth / 2.0;
        if col_main_area_max > 0.0 && aw > 0.0 && d_wall > 0.0 {
            let te = (aw / d_wall).clamp(thickness, 1.5 * thickness);
            let n_comp = forces.iter().map(|(_, f)| -f[0]).fold(0.0_f64, f64::max);
            let sigma_0 = n_comp / aw;
            let shear_span_ratio = forces
                .iter()
                .max_by(|a, b| {
                    a.1[5]
                        .abs()
                        .partial_cmp(&b.1[5].abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .and_then(|(_, f)| {
                    let q = f[1].abs().max(f[2].abs());
                    (q > 1e-6).then(|| f[5].abs() / q / d_wall)
                })
                .unwrap_or_else(|| h / (2.0 * d_wall));
            let tri_inp = WallShearTrilinearInput {
                wall_height_mm: h,
                wall_length_mm: l,
                fc,
                aw,
                tension_column_main_area: col_main_area_max,
                te,
                t: thickness,
                d_wall,
                dc_compression: dc_max,
                tension_column_at: col_main_area_max,
                sigma_wh,
                pwh_ratio,
                sigma_0,
                shear_span_ratio,
                opening: if l0p > 1e-9 && h0p > 1e-9 {
                    Some((l0p, h0p, h, l))
                } else {
                    None
                },
            };
            let tri = match wall_shear_trilinear(&tri_inp) {
                Ok(tri) => tri,
                Err(msg) => {
                    out.push((
                        elem.nodes[0],
                        "耐震壁(RC)せん断非線形".into(),
                        CheckOutcome::Skipped {
                            reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
                        },
                    ));
                    continue;
                }
            };
            let shear_mpa = mat.shear.unwrap_or(mat.young / (2.0 * (1.0 + mat.poisson)));
            let points = match tri.displacement_points(shear_mpa, shear_area_mm2, h) {
                Ok(points) => points,
                Err(msg) => {
                    out.push((
                        elem.nodes[0],
                        "耐震壁(RC)せん断非線形".into(),
                        CheckOutcome::Skipped {
                            reason: format!("耐震壁 ID {} の{}", elem.id.0, msg),
                        },
                    ));
                    continue;
                }
            };
            let ratio = q_design / tri.qu;
            let detail = format!(
                "Qc={:.1} kN, βs={:.3}, Qu={:.1} kN, r={:.3}, QD={:.1} kN, δc={:.4} mm, δu={:.4} mm（純せん断の参考骨格）",
                tri.qc / 1000.0,
                tri.beta_s,
                tri.qu / 1000.0,
                tri.r_opening,
                q_design / 1000.0,
                points[1].0,
                points[2].0
            );
            out.push((
                elem.nodes[0],
                "耐震壁(RC)せん断非線形".to_string(),
                CheckOutcome::Checked(CheckResult {
                    basis: "耐震壁せん断参考骨格（横筋βs・終局割線接続）".to_string(),
                    detail: String::new(),
                    components: vec![CheckComponent {
                        kind: CheckKind::Shear,
                        ratio,
                        detail,
                    }],
                }),
            ));
        } else {
            out.push((
                elem.nodes[0],
                "耐震壁(RC)せん断非線形".into(),
                CheckOutcome::Skipped {
                    reason: format!(
                        "耐震壁 ID {} の側柱主筋量または有効断面が不足し、Qc/Qu を算定できません",
                        elem.id.0
                    ),
                },
            ));
        }
    }
}

fn side_column_section_is_parallel(
    column: &sepika_core::model::ElementData,
    model: &Model,
    wall: &sepika_core::model::WallElementGeometry,
) -> bool {
    use sepika_core::geom::vec3::{dot, unit_from};
    let Some(height_direction) = unit_from(wall.bottom_center, wall.top_center) else {
        return false;
    };
    let Some(first) = column.nodes.first().and_then(|id| model.node(*id)) else {
        return false;
    };
    let Some(second) = column.nodes.get(1).and_then(|id| model.node(*id)) else {
        return false;
    };
    let frame = sepika_element::transform::LocalFrame::from_nodes(
        first.coord,
        second.coord,
        column.local_axis.ref_vector,
    );
    let parallel = |a, b| (dot(a, b).abs() - 1.0).abs() <= 1e-9;
    dot(wall.ex_bottom, height_direction).abs() <= 1e-9
        && parallel(frame.rot[0], height_direction)
        && (parallel(frame.rot[1], wall.ex_bottom) || parallel(frame.rot[2], wall.ex_bottom))
}

/// 材料役割の割当・対応材種・有限正 fy を検査し、名称から強度を推定しない。
fn wall_rebar_strength(
    mat: Option<&sepika_core::model::Material>,
    role: &str,
) -> Result<f64, String> {
    let mat = mat.ok_or_else(|| format!("{role}材料が未割当です"))?;
    if mat.category != sepika_core::model::MaterialCategory::Rebar
        || !sepika_core::material_grade::is_supported_shear_rebar_grade(&mat.name)
    {
        return Err(format!(
            "{role}材料 ID {} の{}",
            mat.id.0,
            sepika_core::material_grade::unsupported_shear_rebar_message(&mat.name)
        ));
    }
    mat.fy
        .filter(|fy| fy.is_finite() && *fy > 0.0)
        .ok_or_else(|| {
            format!(
                "{role}材料 ID {}「{}」の降伏強度 fy が未設定または有限の正値ではありません",
                mat.id.0, mat.name
            )
        })
}

/// 壁側柱の RC 諸元 `(b, d, d_eff, pw, 主筋総面積)` を形状から引く。
///
/// 実配筋モデルの API から算定する。円形柱は等価正方形断面として扱う。
fn wall_side_column_props(shape: Option<&SectionShape>) -> Option<(f64, f64, f64, f64, f64)> {
    match shape? {
        SectionShape::RcColumnRect { b, d, rebar }
        | SectionShape::SrcColumnRect { b, d, rebar, .. } => {
            let dt = rebar.cover + rebar.hoop.dia + rebar.main_dia / 2.0;
            let pw = if *b > 0.0 && rebar.hoop.pitch > 0.0 {
                rebar.aw_x_mm2() / (*b * rebar.hoop.pitch)
            } else {
                0.0
            };
            Some((*b, *d, *d - dt, pw, rebar.total_main_area()))
        }
        SectionShape::RcColumnCircle { d, rebar } => {
            let side = rebar.equivalent_square_side_mm(*d);
            let d_eff = rebar.equivalent_effective_depth_mm(*d);
            Some((side, side, d_eff, rebar.pw(side), rebar.total_main_area()))
        }
        SectionShape::RcBeamRect { b, d, rebar }
        | SectionShape::SrcBeamRect { b, d, rebar, .. } => {
            let dt = rebar
                .top_centroid_from_edge()
                .max(rebar.bottom_centroid_from_edge());
            Some((*b, *d, *d - dt, rebar.pw(*b), rebar.total_main_area()))
        }
        _ => None,
    }
}

pub(super) fn collect_wall_design_checks(
    model: &Model,
    forces: &[(ElemId, ForcesAt<'_>)],
    members: &[MemberInfo<'_>],
    term: LoadTerm,
    index: Option<&sepika_load::wall_expand::WallExpansionIndex>,
    case: &str,
) -> Vec<crate::wall_check::WallCheck> {
    use crate::wall_check::{WallCheck, WallCheckKind, WallSkipKind};
    let mut out = Vec::new();
    let mut candidates: Vec<_> = model
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Wall)
        .map(|e| (index.and_then(|i| i.plate_of(e.id)), Some(e)))
        .collect();
    for plate in &model.wall_plates {
        if !candidates.iter().any(|(id, _)| *id == Some(plate.id)) {
            candidates.push((Some(plate.id), None));
        }
    }
    for (plate_id, elem) in candidates {
        let plate = plate_id.and_then(|id| model.wall_plate(id));
        let input_invalid = elem
            .is_some_and(|e| sepika_core::model::wall_element_geometry(e, model).is_none())
            || elem
                .and_then(|e| model.element_section(e))
                .and_then(|s| s.thickness)
                .is_some_and(|t| !t.is_finite() || t <= 0.0);
        let known_slit = plate.is_some_and(|p| p.slit.any())
            || elem.is_some_and(|e| {
                model
                    .wall_attrs
                    .iter()
                    .any(|a| a.elem == e.id && a.slit.any())
            });
        let seismic_target = !known_slit
            && (input_invalid
                || match elem {
                    Some(e) => sepika_element::wall::misc_wall::wall_is_seismic(e, model),
                    None => plate.is_some_and(|p| {
                        model.wall_plate_covers_region(p)
                            || (!p.is_attached() && p.boundary_nodes(model).is_none())
                    }),
                });
        let section = elem
            .and_then(|e| model.element_section(e))
            .or_else(|| plate.and_then(|p| model.wall_plate_section(p)));
        let response =
            elem.and_then(|e| forces.iter().find(|(id, _)| *id == e.id).map(|(_, f)| *f));
        let issue = if input_invalid {
            Some((
                WallSkipKind::InvalidInput,
                "壁幾何または板厚が不正です（入力不足を自重のみへ読み替えません）".into(),
            ))
        } else if !seismic_target {
            Some((
                WallSkipKind::NotApplicable,
                "自重・雑壁のみの壁版は耐震壁検定対象外です".to_string(),
            ))
        } else if elem.is_none() {
            Some((
                WallSkipKind::MissingInput,
                "壁要素を生成できません（断面割当または壁領域・境界入力不足）".into(),
            ))
        } else if section.is_none() {
            Some((
                WallSkipKind::MissingInput,
                "壁断面が未割当または解決不能です".into(),
            ))
        } else if section
            .and_then(|s| s.material)
            .and_then(|id| model.materials.get(id.index()).filter(|m| m.id == id))
            .is_none()
        {
            Some((
                WallSkipKind::MissingInput,
                "壁主材料が未割当または解決不能です".into(),
            ))
        } else {
            let sec = section.unwrap();
            let mat = model.materials.get(sec.material.unwrap().index()).unwrap();
            match sec.shape.as_ref() {
                _ if mat.category == sepika_core::model::MaterialCategory::Steel => Some((WallSkipKind::NotImplemented, "純鋼板壁の国内許容応力度・終局検定式は未確定です（壁板・接合未検定。RC/SRC式は適用しません）".into())),
                _ if sec.steel_material.is_some() => Some((WallSkipKind::NotImplemented, "SRC内蔵鋼板壁の検定は未実装です（純鋼板壁・RC壁の式を流用しません）".into())),
                Some(SectionShape::RcWall { .. }) if mat.category != sepika_core::model::MaterialCategory::Concrete => Some((WallSkipKind::InvalidInput, "RC壁の主材料区分がコンクリートではありません".into())),
                Some(SectionShape::RcWall { .. }) if mat.fc.is_none() => Some((WallSkipKind::MissingInput, "コンクリート強度 Fc が未入力です".into())),
                Some(SectionShape::RcWall { .. }) if !mat.fc.is_some_and(|fc| fc.is_finite() && fc > 0.0) => Some((WallSkipKind::InvalidInput, "Fc が有限の正値ではありません".into())),
                Some(SectionShape::RcWall { .. }) => if response.is_none_or(|f| f.is_empty()) { Some((WallSkipKind::MissingResponse, "壁応答を取得できません（空応答をゼロ応力として扱いません）".into())) } else if response.is_some_and(|f| f.iter().any(|(_, v)| v.iter().any(|x| !x.is_finite()))) { Some((WallSkipKind::InvalidInput, "壁応答に非有限値があります".into())) } else { None },
                _ => Some((WallSkipKind::NotApplicable, "RC/SRC/純鋼板を区別し、この壁形状・材料への検定式は適用できません".into())),
            }
        };
        let mut computed = Vec::new();
        if issue.is_none() {
            let e = elem.unwrap();
            check_walls(
                model,
                &[(e.id, response.unwrap())],
                members,
                term,
                &mut computed,
            );
        }
        for kind in [
            WallCheckKind::AllowableShear,
            WallCheckKind::ReferenceSkeleton,
        ] {
            let (outcome, skip_kind) = if let Some((why, reason)) = &issue {
                (
                    CheckOutcome::Skipped {
                        reason: reason.clone(),
                    },
                    Some(*why),
                )
            } else {
                let row = computed.iter().find(|(_, label, _)| {
                    label.ends_with("せん断非線形") == (kind == WallCheckKind::ReferenceSkeleton)
                });
                match row {
                    Some((_, _, outcome @ CheckOutcome::Checked(_))) => (outcome.clone(), None),
                    Some((_, _, outcome @ CheckOutcome::Skipped { reason })) => {
                        let required_material_issue = elem.and_then(|e| {
                            if kind == WallCheckKind::ReferenceSkeleton || term != LoadTerm::Long {
                                wall_rebar_skip_kind(model.element_shear_rebar_material(e)).or_else(
                                    || {
                                        (kind == WallCheckKind::AllowableShear)
                                            .then(|| {
                                                wall_rebar_skip_kind(
                                                    model.element_rebar_material(e),
                                                )
                                            })
                                            .flatten()
                                    },
                                )
                            } else {
                                None
                            }
                        });
                        let why = if let Some(why) = required_material_issue {
                            why
                        } else if reason.contains("未割当")
                            || reason.contains("未入力")
                            || reason.contains("不足")
                            || reason.contains("未設定")
                        {
                            WallSkipKind::MissingInput
                        } else if reason.contains("未対応") {
                            WallSkipKind::NotImplemented
                        } else if reason.contains("範囲") {
                            WallSkipKind::NotApplicable
                        } else {
                            WallSkipKind::InvalidInput
                        };
                        (outcome.clone(), Some(why))
                    }
                    None => (
                        CheckOutcome::Skipped {
                            reason: "壁幾何・材料役割・参考骨格の必要入力を解決できません".into(),
                        },
                        Some(WallSkipKind::MissingInput),
                    ),
                }
            };
            out.push(WallCheck {
                plate: plate_id,
                elem: elem.map(|e| e.id),
                node: elem.and_then(|e| e.nodes.first().copied()).or_else(|| {
                    plate
                        .and_then(|p| p.boundary_nodes(model))
                        .and_then(|n| n.first().copied())
                }),
                case: case.into(),
                kind,
                seismic_target,
                skip_kind,
                outcome,
            });
        }
    }
    out
}

fn wall_rebar_skip_kind(
    material: Option<&sepika_core::model::Material>,
) -> Option<crate::wall_check::WallSkipKind> {
    use crate::wall_check::WallSkipKind;
    let Some(material) = material else {
        return Some(WallSkipKind::MissingInput);
    };
    if material.category != sepika_core::model::MaterialCategory::Rebar
        || !sepika_core::material_grade::is_supported_shear_rebar_grade(&material.name)
    {
        return Some(WallSkipKind::InvalidInput);
    }
    match material.fy {
        None => Some(WallSkipKind::MissingInput),
        Some(fy) if !fy.is_finite() || fy <= 0.0 => Some(WallSkipKind::InvalidInput),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::wall_side_column_props;
    use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar, SectionShape};

    #[test]
    fn wall_side_circle_column_uses_hoop_ratio() {
        let rebar = RcCircleColumnRebar {
            count: 8,
            main_dia: 22.0,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 100.0,
            },
        };
        let shape = SectionShape::RcColumnCircle { d: 600.0, rebar };
        let (_, _, _, pw, _) = wall_side_column_props(Some(&shape)).expect("円形側柱諸元");
        let side = shape
            .circle_column_rebar()
            .expect("円形柱配筋")
            .equivalent_square_side_mm(600.0);
        let expected = shape.circle_column_rebar().expect("円形柱配筋").pw(side);

        assert!(pw > 0.0);
        assert_eq!(pw, expected);
    }
}
