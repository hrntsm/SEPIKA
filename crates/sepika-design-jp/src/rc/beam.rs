//! 鉄筋コンクリート造梁の断面検定（RC 規準13条: 曲げ・せん断・付着）。
//! 強軸曲げ（`mz`）とそれに対のせん断（`qy`）のみを検定する。

use super::{
    axis_props_from_shape, main_rebar_grade, rc_allow, rc_beam_bond_check_1991_with_rebar,
    rc_beam_bond_check_with_rebar, rebar_allowable_tension, rebar_info_from_shape,
    rebar_sigma_y_of, shear_alpha, shear_capacity, shear_rebar_grade, AxisProps,
};
use crate::member_design_check::combine_incomplete_reasons;
use crate::ultimate::rc_props::RcDirection;
use crate::{
    BondMethod, CheckComponent, CheckKind, CheckOutcome, CheckResult, DesignCtx, LoadTerm,
    MemberForcesAt, QdMethod,
};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::Dof;
use sepika_core::face_distance::girder_column_faces;
use sepika_core::geom::vec3::{cross, dist, dot, local_frame_axes, unit, unit_from};
use sepika_core::model::{
    ElementData, EndCondition, FrameSectionUse, Material, MaterialCategory, Model, Node,
    SupportMemberId,
};
use sepika_core::section_shape::{RcBeamRebar, SectionShape};

pub(crate) struct BeamMoment {
    /// 引張鉄筋支配の許容曲げモーメント MA_t = at・ft・j。
    pub(crate) ma_t: f64,
    /// 圧縮縁コンクリート支配の許容曲げモーメント MA_c。
    pub(crate) ma_c: f64,
    /// 圧縮縁から中立軸までの距離 [mm]。
    xn_mm: f64,
    /// MA = min(MA_t, MA_c)。
    pub(crate) ma: f64,
}

/// 梁の許容曲げモーメント MA を算定する（RC 規準 13条）。
pub(crate) fn beam_moment_capacity(
    props: &AxisProps,
    ft: f64,
    fc: f64,
    n_ratio: f64,
) -> Option<BeamMoment> {
    let ma_t = props.at * ft * props.j;

    let dc = props.dt;
    let d = props.d;
    let b = props.b;
    let ac = props.ac;
    let at = props.at;

    let b_coef = (n_ratio - 1.0) * ac + n_ratio * at;
    let c_coef = (n_ratio - 1.0) * ac * dc + n_ratio * at * d;
    let xn_mm = ((b_coef * b_coef + 2.0 * b * c_coef).sqrt() - b_coef) / b;
    if !xn_mm.is_finite() || xn_mm <= 0.0 || xn_mm >= d {
        return None;
    }
    let icr = b * xn_mm.powi(3) / 3.0
        + (n_ratio - 1.0) * ac * (xn_mm - dc).powi(2)
        + n_ratio * at * (d - xn_mm).powi(2);
    let ma_c = fc * icr / xn_mm;
    if !ma_t.is_finite() || !ma_c.is_finite() || ma_t <= 0.0 || ma_c <= 0.0 {
        return None;
    }
    Some(BeamMoment {
        ma_t,
        ma_c,
        xn_mm,
        ma: ma_t.min(ma_c),
    })
}

fn layer_count_and_first_moment(layers: &[u32]) -> (u128, u128) {
    layers
        .iter()
        .enumerate()
        .fold((0, 0), |(total, first_moment), (layer, &count)| {
            let count = u128::from(count);
            (total + count, first_moment + (layer as u128) * count)
        })
}

fn bending_component(
    forces: &MemberForcesAt,
    ctx: &DesignCtx,
    rebar: &RcBeamRebar,
    props: &AxisProps,
    tension_is_top: bool,
    ft: f64,
    allow: &super::RcAllow,
) -> Result<CheckComponent, &'static str> {
    let demand_nmm = forces.mz.abs();
    if !demand_nmm.is_finite() {
        return Err("解析曲げモーメントMzが非有限です");
    }
    let (ratio, detail) = if demand_nmm == 0.0 {
        (0.0, "強軸曲げ需要=0 N·mm".into())
    } else if props.at <= 0.0 {
        return Err("引張側の主筋がありません");
    } else if (forces.pos - 0.5).abs() < 1e-6 && ctx.beam_has_slab && forces.mz > 0.0 {
        let ma_t = props.at * ft * props.j;
        (
            demand_nmm / ma_t,
            format!("MA_t={ma_t:.1} N·mm, MA={ma_t:.1} N·mm, |mz|={demand_nmm:.1} N·mm, 中央+スラブ正曲げ: MA=at·ft·j"),
        )
    } else {
        let (tension, compression) = if tension_is_top {
            (&rebar.top, &rebar.bottom)
        } else {
            (&rebar.bottom, &rebar.top)
        };
        let (tension_count, tension_moment) = layer_count_and_first_moment(tension);
        let (compression_count, compression_moment) = layer_count_and_first_moment(compression);
        // 共通の段間隔を消去すれば、浮動小数点で重心の一致を判定する必要がない。
        if compression_moment * tension_count != tension_moment * compression_count {
            return Err(
                "圧縮縁から圧縮筋重心までの距離と引張縁から引張筋重心までの距離が一致しません",
            );
        }
        let n_ratio = allow
            .n_ratio
            .ok_or(super::allowable::UNSUPPORTED_YOUNG_RATIO)?;
        let bm = beam_moment_capacity(props, ft, allow.fc, n_ratio)
            .ok_or("矩形断面の許容曲げを求められません")?;
        let (top_inner, bottom_inner) = rebar.innermost_bar_distances_from_edges_mm();
        let (tension_inner, compression_inner) = if tension_is_top {
            (top_inner, bottom_inner)
        } else {
            (bottom_inner, top_inner)
        };
        if compression_inner.is_some_and(|distance_mm| distance_mm >= bm.xn_mm)
            || tension_inner.is_some_and(|distance_mm| props.d_full - distance_mm <= bm.xn_mm)
        {
            return Err("主筋の全段が中立軸の想定側に収まりません（中立軸上の段を含む）");
        }
        (
            demand_nmm / bm.ma,
            format!(
                "MA_t={:.1} N·mm, MA_c={:.1} N·mm, MA={:.1} N·mm, |mz|={demand_nmm:.1} N·mm",
                bm.ma_t, bm.ma_c, bm.ma,
            ),
        )
    };
    Ok(CheckComponent {
        kind: CheckKind::Bending,
        ratio,
        detail,
    })
}

/// RC矩形梁のQD1に必要な両端の強軸曲げ伝達と内法長さ [mm] を確認する。
/// `adjacency` は同一モデルの `NodeAdjacency::primary_frame` を渡す。
pub fn beam_qd1_clear_span(
    model: &Model,
    adjacency: &NodeAdjacency,
    elem: &ElementData,
) -> Result<f64, String> {
    if model
        .girder_groups
        .iter()
        .any(|group| group.contains(&elem.id))
    {
        return Err("QD1用に一本部材グループの両端断面とQ0を確定できません".into());
    }
    if model.member_detail(elem.id).is_some_and(|detail| {
        [detail.haunch_i, detail.haunch_j]
            .into_iter()
            .flatten()
            .any(|haunch| haunch.depth_increase != 0.0 || haunch.width_increase != 0.0)
    }) {
        return Err("QD1用にハンチ付きの梁の端部断面を確定できません".into());
    }
    if beam_supports_slab(model, elem) {
        return Err("QD1用の梁端降伏曲げモーメントに必要なスラブ筋の寄与を確定できません".into());
    }
    if !elem.kind.is_primary_frame_support() {
        return Err("QD1用に両端の強軸曲げを伝達する梁要素を確認できません".into());
    }
    let [i, j] = frame_end_positions(model, elem).ok_or("QD1用の梁の材軸を確定できません")?;
    let [_, _, ez] = local_frame_axes(i, j, elem.local_axis.ref_vector)?;
    let length_mm = dist(i, j);
    let mut distances_mm = [0.0; 2];
    let column_faces = girder_column_faces(model, elem, adjacency);
    for (index, end) in ["i端", "j端"].into_iter().enumerate() {
        if !transmits_bending(elem.end_cond[index]) {
            return Err(format!(
                "QD1用の梁の{end}で強軸曲げが解放されているか接合剛性が不正です"
            ));
        }
        let node = elem
            .nodes
            .get(index)
            .and_then(|id| model.node(*id))
            .ok_or_else(|| format!("QD1用の梁の{end}に接続する節点を確認できません"))?;
        let restrained = restrains_direction(node, ez, [Dof::Rx, Dof::Ry, Dof::Rz]);
        let others = || {
            adjacency
                .elements_at(model, node.id)
                .filter(|other| other.id != elem.id)
        };
        let column_connected = others().any(|other| {
            model
                .element_section(other)
                .is_some_and(|section| section.frame_use == Some(FrameSectionUse::Column))
        });
        let unsupported = || {
            format!(
            "QD1用の梁の{end}が自由端・回転自由な支点、または強軸曲げの伝達先を確認できない接合です"
        )
        };
        let distance_mm = if column_connected {
            let face = column_faces
                .as_ref()
                .ok_or_else(|| format!("QD1用の内法長さ: {end}の実柱面を確認できません"))?[index]
                .as_ref()
                .map_err(|reason| format!("QD1用の内法長さ: {reason}"))?;
            if !restrained
                && !others().any(|column| {
                    face.columns.contains(&column.id)
                        && column_transmits_bending(model, column, node, ez)
                })
            {
                return Err(unsupported());
            }
            if index == 0 {
                face.position_mm
            } else {
                length_mm - face.position_mm
            }
        } else {
            if !restrained {
                return Err(unsupported());
            }
            if others().next().is_some() {
                return Err(format!(
                    "QD1用の内法長さ: {end}で柱以外の接続部材との実端面を確認できません"
                ));
            }
            [elem.rigid_zone.face_i, elem.rigid_zone.face_j][index]
                .ok_or_else(|| format!("QD1用の内法長さ: {end}のフェース距離が未算定です"))?
        };
        if !distance_mm.is_finite() || distance_mm < 0.0 {
            return Err(format!(
                "QD1用の内法長さ: {end}のフェース距離が有限な非負値ではありません"
            ));
        }
        distances_mm[index] = distance_mm;
    }
    let span_mm = length_mm - distances_mm[0] - distances_mm[1];
    if !span_mm.is_finite() || span_mm <= 0.0 {
        return Err("QD1に必要な有限で正の内法長さが成立しません".into());
    }
    Ok(span_mm)
}

/// 部材の一部でも、実際に割り当てられた床板を支持する場合に true。
pub(crate) fn beam_supports_slab(model: &Model, elem: &ElementData) -> bool {
    model.beam_has_attached_slab(elem)
        || model.floor_assignment_regions.regions.iter().any(|region| {
            region.assignment.plate().is_some()
                && region
                    .boundary
                    .iter()
                    .any(|edge| edge.support == SupportMemberId::Primary(elem.id))
        })
}

pub(super) fn transmits_bending(condition: EndCondition) -> bool {
    match condition {
        EndCondition::Fixed => true,
        EndCondition::Pinned => false,
        EndCondition::SemiRigid { k_theta } => k_theta.is_finite() && k_theta > 0.0,
    }
}

pub(super) fn restrains_direction(node: &Node, direction: [f64; 3], dofs: [Dof; 3]) -> bool {
    direction.into_iter().zip(dofs).all(|(component, dof)| {
        component.abs() < 1e-9
            || node.restraint.is_fixed(dof)
            || node.support_spring.is_some_and(|spring| {
                let k = spring[dof as usize];
                k.is_finite() && k > 0.0
            })
    })
}

pub(super) fn column_transmits_bending(
    model: &Model,
    column: &ElementData,
    near: &Node,
    ez: [f64; 3],
) -> bool {
    let side = usize::from(column.nodes[0] != near.id);
    if !transmits_bending(column.end_cond[side]) {
        return false;
    }
    let Some(ex) = frame_end_positions(model, column).and_then(|[a, b]| unit_from(a, b)) else {
        return false;
    };
    if dot(ex, ez).abs() >= 1.0 - 1e-9 {
        return false;
    }
    let Some(translation_axis) = unit(cross(ez, ex)) else {
        return false;
    };
    let rotation_axis = cross(ex, translation_axis);
    let far_side = 1 - side;
    let Some(far) = model.node(column.nodes[far_side]) else {
        return false;
    };

    restrains_direction(far, translation_axis, [Dof::Ux, Dof::Uy, Dof::Uz])
        || (transmits_bending(column.end_cond[far_side])
            && restrains_direction(far, rotation_axis, [Dof::Rx, Dof::Ry, Dof::Rz]))
}

fn frame_end_positions(model: &Model, elem: &ElementData) -> Option<[[f64; 3]; 2]> {
    match model.primary_frame_axis_end_positions(elem).ok()? {
        Some(ends) => Some(ends),
        None => {
            let [i, j] = elem.nodes.as_slice() else {
                return None;
            };
            Some([model.node(*i)?.coord, model.node(*j)?.coord])
        }
    }
}

fn beam_qd1_sum_my(
    shape: &SectionShape,
    ctx: &DesignCtx,
    main_rebar_valid: bool,
) -> Result<f64, String> {
    if ctx.beam_has_slab {
        return Err("QD1用の梁端降伏曲げモーメントに必要なスラブ筋の寄与を確定できません".into());
    }
    let sigma_y = rebar_sigma_y_of(ctx.rebar_material.as_ref());
    if !main_rebar_valid || !sigma_y.is_finite() || sigma_y <= 0.0 {
        return Err("QD1用の梁端降伏曲げモーメントに必要な主筋の降伏強度を確定できません".into());
    }

    let mut sum_my_nmm = 0.0;
    for (tension_is_top, side) in [(true, "上端"), (false, "下端")] {
        let props = axis_props_from_shape(shape, RcDirection::Strong, tension_is_top)
            .ok_or_else(|| format!("QD1用の梁端の{side}引張断面を確定できません"))?;
        let my_nmm = 0.9 * props.at * sigma_y * props.d;
        if !my_nmm.is_finite() || my_nmm <= 0.0 {
            return Err(format!(
                "QD1用の梁端の{side}引張降伏曲げモーメントを確定できません"
            ));
        }
        sum_my_nmm += my_nmm;
    }
    // 両端に同じ断面を使うため、逆符号の二つの組合せは同じ和になる。
    Ok(sum_my_nmm)
}

/// 梁の断面検定（RC 規準 13条）。強軸曲げ mz とそれに対のせん断 qy のみを扱う。
#[allow(clippy::too_many_arguments)]
pub(crate) fn beam_check(
    forces: &MemberForcesAt,
    mat: &Material,
    ctx: &DesignCtx,
    shape: &SectionShape,
    rebar: &RcBeamRebar,
    fc_raw: f64,
    main_rebar_valid: bool,
    shear_rebar_valid: bool,
    shear_reference_valid: bool,
    section_issue: &mut Option<String>,
) -> CheckOutcome {
    let long_term = ctx.term == LoadTerm::Long;
    let deformed_rebar = if matches!(
        rebar.main_dia,
        sepika_core::section_shape::RebarSize::Standard(_)
    ) {
        Some(true)
    } else {
        main_rebar_valid.then(|| {
            ctx.rebar_material.as_ref().is_some_and(|m| {
                m.category == MaterialCategory::Rebar && !matches!(m.name.trim(), "SR235" | "SR295")
            })
        })
    };
    let allow = rc_allow(
        fc_raw,
        mat.concrete_class,
        shear_rebar_valid.then(|| shear_rebar_grade(ctx.shear_rebar_material.as_ref())),
        long_term,
    );

    let tension_is_top = if forces.mz < 0.0 {
        true
    } else if forces.mz > 0.0 {
        false
    } else {
        let at_top = axis_props_from_shape(shape, RcDirection::Strong, true).map(|p| p.at);
        let at_bottom = axis_props_from_shape(shape, RcDirection::Strong, false).map(|p| p.at);
        match (at_top, at_bottom) {
            (Some(t), Some(b)) => t < b,
            (Some(_), None) => true,
            _ => false,
        }
    };
    let mut props = axis_props_from_shape(shape, RcDirection::Strong, tension_is_top)
        .expect("梁の断面諸元を算定できる形状のみ来る");
    let info =
        rebar_info_from_shape(shape, tension_is_top).expect("梁の鉄筋情報を算定できる形状のみ来る");
    let ft = main_rebar_valid.then(|| {
        let grade = main_rebar_grade(ctx.rebar_material.as_ref());
        rebar_allowable_tension(grade, info.main_dia.designation_or_diameter_mm(), long_term)
    });
    let mut components = Vec::new();
    if let Some(ft) = ft {
        match bending_component(forces, ctx, rebar, &props, tension_is_top, ft, &allow) {
            Ok(bending) => components.push(bending),
            Err(reason) => {
                let direction = if !forces.mz.is_finite() {
                    ""
                } else if tension_is_top {
                    "（負曲げ）"
                } else {
                    "（正曲げ）"
                };
                *section_issue = Some(format!("RC 梁曲げ検定不能{direction}: {reason}"));
            }
        }
    }

    let (m_for_alpha, q_for_alpha) = ctx.shear_span.unwrap_or((forces.mz.abs(), forces.qy.abs()));
    let alpha = shear_alpha(m_for_alpha, q_for_alpha, props.d, 2.0);
    let valid_tension_props = [props.b, props.d, props.j, props.at, fc_raw]
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0);
    let seismic_qd = ctx.seismic_qd.as_ref().filter(|_| !long_term);
    let uses_qd1 = seismic_qd.is_some_and(|qd| qd.method != QdMethod::Qd2);
    let q_design = if !shear_reference_valid {
        None
    } else if let Some(qd) = seismic_qd {
        match super::beam_design_shear(
            qd,
            forces.pos,
            forces.qy,
            ctx.rc_beam_qd1_issue.as_deref(),
            || beam_qd1_sum_my(shape, ctx, main_rebar_valid),
        ) {
            Ok(demand_n) => Some(demand_n),
            Err(reason) => {
                *section_issue = combine_incomplete_reasons(section_issue.take(), Some(&reason));
                None
            }
        }
    } else {
        Some(forces.qy.abs())
    }
    .filter(|_| forces.mz.is_finite() && forces.qy.is_finite());
    let shear_input_issue = if !forces.qy.is_finite() {
        Some("RC 梁せん断検定不能: 解析せん断力Qyが非有限です")
    } else if forces.mz == 0.0
        && props.at <= 0.0
        && q_design.map_or(forces.qy != 0.0 || uses_qd1, |q| q > 0.0)
    {
        Some("RC 梁せん断検定不能: 引張側に主筋がなく有効せいを確定できません")
    } else {
        None
    };
    if let Some(reason) = shear_input_issue {
        *section_issue = combine_incomplete_reasons(section_issue.take(), Some(reason));
    }
    // 間隔の診断を先に追加すると、間隔を使わない QD1 も未算定になる。
    let check_stirrup_spacing = info.shear_pitch.is_finite();
    let has_stirrups = info.shear_legs > 0 && info.shear_dia.area_mm2() > 0.0;
    let stirrup_effect_available = check_stirrup_spacing || !has_stirrups;
    if !check_stirrup_spacing {
        *section_issue = combine_incomplete_reasons(
            section_issue.take(),
            Some("RC 梁検定: あばら筋間隔が非有限です"),
        );
        if !has_stirrups {
            props.pw = 0.0;
        }
    }
    let qa = (stirrup_effect_available
        && [m_for_alpha, q_for_alpha, props.pw]
            .into_iter()
            .all(f64::is_finite))
    .then(|| {
        shear_capacity(
            &props,
            &allow,
            alpha,
            ctx.term,
            ctx.rc_damage_control,
            false,
        )
    })
    .flatten();
    let q_design = q_design.filter(|_| valid_tension_props);
    if let Some((q_design, qa)) = q_design
        .zip(qa)
        .filter(|&(_, qa)| qa.is_finite() && qa > 0.0)
    {
        components.push(CheckComponent {
            kind: CheckKind::Shear,
            ratio: q_design / qa,
            detail: format!(
                "QA={:.1} N, |qy|={:.1} N, α={:.3}, pw={:.5}",
                qa, forces.qy, alpha, props.pw,
            ),
        });
    }

    let (ratio_bond, bond_detail) =
        match (ctx.bond_method, deformed_rebar, q_design) {
            (BondMethod::Rc1999, Some(deformed_rebar), _)
                if valid_tension_props
                    && (long_term || stirrup_effect_available)
                    && [forces.mz, forces.pos, ctx.length]
                        .into_iter()
                        .all(f64::is_finite)
                    && (shear_rebar_valid
                        || long_term
                        || info.shear_pitch <= 0.0
                        || info.shear_legs == 0
                        || info.shear_dia.area_mm2() == 0.0) =>
            {
                let bond = rc_beam_bond_check_with_rebar(
                    forces.pos,
                    ctx.length,
                    props.b,
                    props.d,
                    props.j,
                    props.at,
                    forces.mz.abs(),
                    &info,
                    fc_raw,
                    long_term,
                    tension_is_top,
                    deformed_rebar,
                )
                .filter(|bond| bond.ratio.is_finite());
                let ratio = bond.as_ref().map(|b| b.ratio).unwrap_or(0.0);
                let detail =
                    bond.as_ref().map(|b| {
                        format!(
                    "1999: ld={:.1} mm, ldb={:.1} mm, K={:.3}, W={:.3} mm, fb={:.3} N/mm², \
                     σt={:.1} N/mm², 付着検定比={:.3}（{}・{}）",
                    b.ld,
                    b.ldb,
                    b.k,
                    b.w,
                    b.fb,
                    b.sigma_t,
                    b.ratio,
                    if b.is_end { "端部" } else { "中央" },
                    if tension_is_top { "上端筋" } else { "下端筋" }
                )
                    });
                (ratio, detail)
            }
            (BondMethod::Rc1991, Some(deformed_rebar), Some(q_design)) => {
                let n_t = info.tension_count_1991;
                let phi = n_t * info.main_dia.perimeter_mm();
                let is_end = !(0.25 < forces.pos && forces.pos < 0.75);
                let bond = rc_beam_bond_check_1991_with_rebar(
                    q_design,
                    props.j,
                    phi,
                    fc_raw,
                    tension_is_top,
                    long_term,
                    deformed_rebar,
                )
                .filter(|bond| bond.ratio.is_finite());
                let ratio = bond.as_ref().map(|b| b.ratio).unwrap_or(0.0);
                let detail = bond.as_ref().map(|b| {
                    format!(
                        "1991: τa={:.3} N/mm², fa={:.3} N/mm², ψ={:.1} mm, \
                     Q={:.1} N, 付着検定比={:.3}（{}・{}）",
                        b.tau,
                        b.fa,
                        phi,
                        q_design,
                        b.ratio,
                        if is_end { "端部" } else { "中央" },
                        if tension_is_top {
                            "上端筋"
                        } else {
                            "下端筋"
                        }
                    )
                });
                (ratio, detail)
            }
            _ => (0.0, None),
        };

    let basis = "RC 規準13条（梁の曲げ・せん断・付着）".to_string();
    let detail = format!(
        "at={:.1} mm², d={:.1} mm, j={:.1} mm{}",
        props.at,
        props.d,
        props.j,
        if bond_detail.is_some() || !main_rebar_valid || !shear_rebar_valid {
            ""
        } else {
            ", 付着検定: 部材長(柱面間距離)が未設定、または入力不足のため省略"
        },
    );

    if let Some(bond_detail) = bond_detail {
        components.push(CheckComponent {
            kind: CheckKind::Bond,
            ratio: ratio_bond,
            detail: bond_detail,
        });
    }

    let mut detail = detail;
    if super::provisions::is_member_level_station(forces.pos) {
        let mz_for_at = {
            let mut m = forces.mz.abs();
            if let Some((mi, mj)) = ctx.end_moments_z {
                m = m.max(mi.abs()).max(mj.abs());
            }
            if let Some(mc) = ctx.mid_moment_z {
                m = m.max(mc.abs());
            }
            m
        };
        let prov = super::provisions::beam_provisions(
            props.d_full,
            &props,
            &info,
            long_term,
            mz_for_at,
            ft,
            check_stirrup_spacing,
        );
        if let Some(c) = prov.provision_component() {
            components.push(c);
        }
        detail.push_str(&prov.warning_suffix());
    }

    CheckOutcome::Checked(CheckResult {
        basis,
        detail,
        components,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rc::tests::{ctx_beam, make_material, make_section, rc_beam_shape};
    use crate::DesignCheck;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, SectionShape};
    use sepika_core::units::ConcreteClass;

    fn make_material_class(fc: f64, grade: &str, class: ConcreteClass) -> Material {
        Material {
            concrete_class: class,
            ..make_material(fc, grade)
        }
    }

    fn checked_beam(
        forces: &MemberForcesAt,
        mat: &Material,
        ctx: &DesignCtx,
        shape: &SectionShape,
    ) -> CheckResult {
        let SectionShape::RcBeamRect { rebar, .. } = shape else {
            unreachable!()
        };
        beam_check(
            forces, mat, ctx, shape, rebar, 24.0, true, true, true, &mut None,
        )
        .unwrap_checked()
    }

    #[test]
    fn test_beam_check_seismic_qd_increases_shear_ratio() {
        use crate::{QdMethod, SeismicQd};
        // 短期・地震時: QL=20kN、当該組合せ Q=60kN → QE=40kN、
        // QD2 = 20+1.5×40 = 80kN（QD1 は ΣMy が大きく効かないよう長スパン）。
        let mat = make_material(24.0, "SD345");
        let shape = rc_beam_shape(400.0, 700.0, 6, 22.0, 1, 40.0, 10.0, 100.0, 2);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 60_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 10.0e6,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Short,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            ..Default::default()
        };
        let base = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let ctx_qd = DesignCtx {
            term: LoadTerm::Short,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            seismic_qd: Some(SeismicQd {
                long_at: vec![(0.0, [0.0, 20_000.0, 0.0, 0.0, 0.0, 0.0])],
                n_factor: 1.5,
                n_mechanism: 1.0,
                q_simple: None,
                clear_length: 6000.0,
                method: QdMethod::Qd2,
            }),
            ..Default::default()
        };
        let with_qd = checked_beam(&forces, &mat, &ctx_qd, &shape);
        // QD=80kN > 解析値 60kN なのでせん断検定比が 4/3 倍になる
        // （曲げ・付着が支配しない前提の内力設定）。
        assert!(
            with_qd.ratio() > base.ratio(),
            "with_qd={} <= base={}",
            with_qd.ratio(),
            base.ratio()
        );
    }

    #[test]
    fn test_beam_moment_light_reinforcement_tension_governs() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let props = crate::rc::section_props::axis_props_from_shape(
            &shape,
            crate::ultimate::rc_props::RcDirection::Strong,
            false,
        )
        .unwrap();
        let ft = rebar_allowable_tension("SD345", 19.0, true);
        let fc = super::super::concrete_allowable_compression(24.0, true);
        let n_ratio = super::super::young_ratio_n(24.0);
        let bm = beam_moment_capacity(&props, ft, fc, n_ratio).unwrap();

        let expected_ma_t = props.at * ft * props.j;
        assert!((bm.ma_t - expected_ma_t).abs() < 1e-6);
        assert!(bm.ma_t <= bm.ma_c, "軽配筋では MA_t が支配するはず");
        assert!((bm.ma - bm.ma_t).abs() < 1e-6);
    }

    #[test]
    fn test_beam_moment_heavy_reinforcement_compression_governs() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let props = crate::rc::section_props::axis_props_from_shape(
            &shape,
            crate::ultimate::rc_props::RcDirection::Strong,
            false,
        )
        .unwrap();
        let ft = rebar_allowable_tension("SD345", 32.0, true);
        let fc = super::super::concrete_allowable_compression(24.0, true);
        let n_ratio = super::super::young_ratio_n(24.0);
        let bm = beam_moment_capacity(&props, ft, fc, n_ratio).unwrap();

        assert!(bm.ma_c < bm.ma_t, "過大配筋では MA_c が支配するはず");
        assert!((bm.ma - bm.ma_c).abs() < 1e-6);
    }

    #[test]
    fn test_beam_mid_with_slab_uses_ma_t_only() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let mat = make_material(24.0, "SD345");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 1_000.0, // 曲げ支配にするためせん断を小さく
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 50_000_000.0,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: false,
            ..Default::default()
        };
        let ctx_slab = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: true,
            ..Default::default()
        };
        let r0 = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let r1 = checked_beam(&forces, &mat, &ctx_slab, &shape);
        let bend0 = r0
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        let bend1 = r1
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        assert!(
            bend1 < bend0 - 1e-9,
            "中央+スラブは MA_t 採用で検定比が下がるはず: plain={bend0}, slab={bend1}"
        );
        assert!(r1.components[0].detail.contains("中央+スラブ"));
    }

    #[test]
    fn test_beam_check_trait_reports_ratio_basis_components_and_details() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(result.ratio() > 0.0);
        assert!(result.basis.contains("13条"));

        assert!(!result.components.is_empty());
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Bending));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Shear));

        let bending = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .expect("Bending component が存在するはず");
        let shear = result
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Shear)
            .expect("Shear component が存在するはず");
        assert!(bending.detail.contains("MA_t="));
        assert!(!shear.detail.contains("MA_t="));
        assert!(shear.detail.contains("QA="));
        assert!(!bending.detail.contains("QA="));
        assert!(result.detail.contains("at="));
    }

    /// 軽量1種の RcDesign 検定は、普通コンクリートより検定比が大きくなる
    /// （許容せん断応力度 fs の 0.9 倍低減が `mat.concrete_class` 経由で効いている）。
    #[test]
    fn test_beam_check_lightweight_reduces_capacity() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat_n = make_material(24.0, "SD345");
        let mat_l = make_material_class(24.0, "SD345", ConcreteClass::Lightweight1);
        let ctx = ctx_beam(LoadTerm::Short);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 100_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 5_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let r_n = design.check(&forces, &sec, &mat_n, &ctx).unwrap_checked();
        let r_l = design.check(&forces, &sec, &mat_l, &ctx).unwrap_checked();
        assert!(
            r_l.ratio() > r_n.ratio(),
            "軽量1種は許容応力度低減により検定比が大きくなるはず: normal={}, light={}",
            r_n.ratio(),
            r_l.ratio()
        );
    }

    #[test]
    fn test_beam_check_provision_error_on_cover() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 20.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1_000_000.0,
        };
        let result = crate::rc::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Provision));
        assert!(!result.ok());
    }

    #[test]
    fn test_beam_mid_negative_with_slab_keeps_min_ma() {
        let shape = rc_beam_shape(400.0, 600.0, 4, 25.0, 2, 40.0, 10.0, 100.0, 2);
        let mat = make_material(24.0, "SD345");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 1_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: -50_000_000.0,
        };
        let ctx_plain = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: false,
            ..Default::default()
        };
        let ctx_slab = DesignCtx {
            term: LoadTerm::Long,
            kind: crate::MemberKind::Girder,
            length: 6000.0,
            beam_has_slab: true,
            ..Default::default()
        };
        let r0 = checked_beam(&forces, &mat, &ctx_plain, &shape);
        let r1 = checked_beam(&forces, &mat, &ctx_slab, &shape);
        let bend0 = r0
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        let bend1 = r1
            .components
            .iter()
            .find(|c| c.kind == crate::CheckKind::Bending)
            .unwrap()
            .ratio;
        assert!(
            (bend0 - bend1).abs() < 1e-12,
            "負曲げではスラブの有無で曲げ検定比が変わらないはず: {bend0} vs {bend1}"
        );
    }

    #[test]
    fn test_beam_check_trait_reports_ratio_basis_components_and_details_includes_bond_detail() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Short);
        ctx.length = 3000.0;
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(crate::full_detail(&result).contains("ld="));
        assert!(crate::full_detail(&result).contains("付着検定比"));
        assert!(result
            .components
            .iter()
            .any(|c| c.kind == crate::CheckKind::Bond));
    }

    #[test]
    fn test_beam_bond_1991_phi_uses_total_tension_count_for_new_model() {
        let shape = SectionShape::RcBeamRect {
            b: 400.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(22.0),
                top: vec![4, 2],
                bottom: vec![2, 1],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(10.0),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Short);
        ctx.bond_method = BondMethod::Rc1991;
        ctx.length = 3000.0;

        let bond_detail = |mz: f64| {
            let forces = MemberForcesAt {
                pos: 0.0,
                n: 0.0,
                qy: 20_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz,
            };
            let r = crate::rc::RcDesign
                .check(&forces, &sec, &mat, &ctx)
                .unwrap_checked();
            r.components
                .iter()
                .find(|c| c.kind == crate::CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone()
        };

        let dia = 22.0;
        let expected_top = format!("ψ={:.1}", 6.0 * std::f64::consts::PI * dia);
        let expected_bottom = format!("ψ={:.1}", 3.0 * std::f64::consts::PI * dia);
        assert!(
            bond_detail(-30_000_000.0).contains(&expected_top),
            "上端引張の φ は 6 本分: {}",
            bond_detail(-30_000_000.0)
        );
        assert!(
            bond_detail(30_000_000.0).contains(&expected_bottom),
            "下端引張の φ は 3 本分: {}",
            bond_detail(30_000_000.0)
        );
    }

    #[test]
    fn test_beam_check_bond_skipped_without_length_regression() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let ctx = ctx_beam(LoadTerm::Long);
        let forces = MemberForcesAt {
            pos: 0.0,
            n: 0.0,
            qy: 0.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 1_000_000.0,
        };
        let design = crate::rc::RcDesign;
        let result = design.check(&forces, &sec, &mat, &ctx).unwrap_checked();
        assert!(crate::full_detail(&result).contains("省略"));
    }

    #[test]
    fn test_beam_bond_detail_uses_bending_sign_and_position_independently() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        for (pos, mz, expected) in [
            (0.0, -30.0e6, "端部・上端筋"),
            (0.5, -30.0e6, "中央・上端筋"),
            (0.0, 30.0e6, "端部・下端筋"),
            (0.5, 30.0e6, "中央・下端筋"),
        ] {
            let forces = MemberForcesAt {
                pos,
                n: 0.0,
                qy: 20_000.0,
                qz: 0.0,
                mx: 0.0,
                my: 0.0,
                mz,
            };
            let result = crate::RcDesign
                .check(&forces, &sec, &mat, &ctx)
                .unwrap_checked();
            let detail = result
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone();
            assert!(detail.contains(expected), "{detail}");
        }
    }

    #[test]
    fn test_beam_bond_1991_corrected_allowable_rejects_excess_demand() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mat = make_material(24.0, "SD345");
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        // 4本D19、j=7/8×540.5 mmに対してτa=2.3125 N/mm²。
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 261125.3034629518,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30.0e6,
        };
        let result = crate::RcDesign
            .check(&forces, &sec, &mat, &ctx)
            .unwrap_checked();
        let bond = result
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Bond)
            .unwrap();
        assert!(
            (bond.ratio - 1.001082251082251).abs() < 1e-12,
            "{}",
            bond.ratio
        );
        assert!(bond.ratio > 1.0);
        assert!(bond.detail.contains("fa=2.310"));
    }

    #[test]
    fn test_beam_bond_selects_rebar_category_and_grade() {
        let shape = rc_beam_shape(300.0, 600.0, 4, 19.0, 1, 40.0, 10.0, 100.0, 2);
        let sec = make_section(shape);
        let mut ctx = ctx_beam(LoadTerm::Long);
        ctx.length = 3000.0;
        ctx.bond_method = BondMethod::Rc1991;
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 20_000.0,
            qz: 0.0,
            mx: 0.0,
            my: 0.0,
            mz: 30.0e6,
        };
        let mut detail = |grade: &str| {
            ctx.rebar_material = Some(crate::rc::tests::make_rebar_material(grade, 235.0));
            crate::RcDesign
                .check(&forces, &sec, &make_material(24.0, grade), &ctx)
                .unwrap_checked()
                .components
                .iter()
                .find(|c| c.kind == CheckKind::Bond)
                .expect("Bond component")
                .detail
                .clone()
        };
        assert!(detail("SD345").contains("fa=2.310"));
        assert!(detail("SR235").contains("fa=1.350"));
        assert!(detail("任意名称").contains("fa=2.310"));
    }
}
