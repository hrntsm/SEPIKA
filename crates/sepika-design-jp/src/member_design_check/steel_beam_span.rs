use std::collections::HashMap;

use sepika_core::geom::vec3::dist;
use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{
    ElementData, EndCondition, MemberLoad, MemberLoadExtent, MemberLoadKind, Model, Section,
    SteelDesignAttr,
};
use sepika_core::structure_kind::StructureKind;
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{
    frame_strong_bending_span, frame_strong_bending_span_mm, FrameStrongBendingPeak,
};
use sepika_element::transform::LocalFrame;

use crate::steel::{beam_lateral_buckling_length, SteelBeamMomentDiagram};
use crate::{DesignCtx, MemberKind};

pub(super) fn requires_bending_span(
    sec: &Section,
    structure: StructureKind,
    ctx: &DesignCtx,
) -> bool {
    structure == StructureKind::S
        && ctx.kind == MemberKind::Girder
        && crate::steel::is_symmetric_h_section(sec)
        && [0.0, 0.5, 1.0].into_iter().any(|pos| {
            let (lb, partial) = beam_lateral_buckling_length(ctx, pos);
            lb > 1e-9 && !partial
        })
}

pub(super) struct BeamSpanLoads<'a>(HashMap<ElemId, Vec<(f64, &'a MemberLoad)>>);

impl<'a> BeamSpanLoads<'a> {
    pub(super) fn from_terms(
        model: &'a Model,
        terms: Option<&[(LoadCaseId, f64)]>,
    ) -> Result<Self, &'static str> {
        let terms = terms
            .filter(|terms| !terms.is_empty())
            .ok_or("解析応答と同じ荷重ケース・荷重組合せの由来が未確認です")?;
        let mut by_member = HashMap::<_, Vec<_>>::new();
        for &(id, factor) in terms {
            let mut cases = model.load_cases.iter().filter(|case| case.id == id);
            let case = cases.next().ok_or("解析に用いた荷重ケースがありません")?;
            if cases.next().is_some() || !factor.is_finite() {
                return Err("荷重ケースIDまたは荷重係数が不正です");
            }
            if factor != 0.0 {
                for load in &case.member {
                    by_member.entry(load.elem).or_default().push((factor, load));
                }
            }
        }
        Ok(Self(by_member))
    }

    pub(super) fn bending_span(
        &self,
        model: &Model,
        elem: &ElementData,
        forces: &MemberForces,
        ctx: &DesignCtx,
    ) -> Result<SteelBeamSpan, String> {
        if [0.0, 0.5, 1.0].into_iter().any(|pos| {
            let (lb, partial) = beam_lateral_buckling_length(ctx, pos);
            partial || !lb.is_finite() || (lb - ctx.length).abs() > 1e-9
        }) {
            return Err("補剛区間と実材軸全長が一致しません".into());
        }
        if ctx
            .steel_attr
            .as_ref()
            .is_some_and(|a| a.lateral_brace_count.is_some_and(|n| n > 0))
        {
            return Err("中間横補剛の設定と単一補剛区間が一致しません".into());
        }
        let [i, j] = model
            .primary_frame_axis_ends(elem)?
            .or_else(|| {
                let &[i, j] = elem.nodes.as_slice() else {
                    return None;
                };
                Some([model.node(i)?.coord, model.node(j)?.coord])
            })
            .ok_or("梁の実材軸が未確認です")?;
        let frame = LocalFrame::from_nodes(i, j, elem.local_axis.ref_vector)?;
        let loads = self.0.get(&elem.id).map_or(&[][..], Vec::as_slice);
        let [Some(face_i_mm), Some(face_j_mm)] = [elem.rigid_zone.face_i, elem.rigid_zone.face_j]
        else {
            return Err("鋼梁の両端の材端フェース位置が未確認です".into());
        };
        let end_mm = ctx.length - face_j_mm;
        if !face_i_mm.is_finite()
            || !face_j_mm.is_finite()
            || face_i_mm < 0.0
            || face_j_mm < 0.0
            || face_i_mm >= end_mm
        {
            return Err("鋼梁の材端フェース間に正の区間を確認できません".into());
        }
        let span =
            frame_strong_bending_span(&frame, ctx.length, [face_i_mm, end_mm], loads, forces)?;
        Ok(SteelBeamSpan::from(span))
    }
}

impl From<sepika_element::frame::member_load::FrameStrongBendingSpan> for SteelBeamSpan {
    fn from(span: sepika_element::frame::member_load::FrameStrongBendingSpan) -> Self {
        let diagram = if span.interior_maximum {
            SteelBeamMomentDiagram::InteriorMaximum
        } else if span.monotonic {
            SteelBeamMomentDiagram::Monotonic(span.end_moments_nmm)
        } else {
            SteelBeamMomentDiagram::Unverified
        };
        Self {
            diagram,
            demand_peaks: span.demand_peaks,
        }
    }
}

pub(super) struct SteelBeamSpan {
    pub diagram: SteelBeamMomentDiagram,
    pub demand_peaks: Vec<FrameStrongBendingPeak>,
}

pub(super) enum GroupBending {
    WithoutWholeSpan { length_mm: Option<f64> },
    Whole { length_mm: f64, span: SteelBeamSpan },
}

struct GroupMember<'a> {
    elem: &'a ElementData,
    attr: Option<&'a SteelDesignAttr>,
    start_mm: f64,
    end_mm: f64,
    length_mm: f64,
}

pub(super) fn grouped_bending_spans(
    model: &Model,
    member_forces: &[(ElemId, MemberForces)],
    force_rows: &HashMap<ElemId, usize>,
    loads: Result<&BeamSpanLoads<'_>, &&'static str>,
) -> HashMap<ElemId, Result<GroupBending, String>> {
    if model.girder_groups.is_empty() {
        return HashMap::new();
    }
    let forces: HashMap<_, _> = member_forces.iter().map(|(id, f)| (*id, f)).collect();
    let mut memberships = HashMap::<_, usize>::new();
    for id in model.girder_groups.iter().flatten() {
        *memberships.entry(*id).or_default() += 1;
    }

    let mut spans = HashMap::new();
    for group in &model.girder_groups {
        let result = if group.iter().any(|id| memberships[id] != 1) {
            Err("一本部材の構成要素が重複しています".into())
        } else {
            grouped_bending_span(model, group, &forces, force_rows, loads, &mut spans)
        };
        if let Err(reason) = result {
            for &id in group {
                spans.entry(id).or_insert_with(|| Err(reason.clone()));
            }
        }
    }
    spans
}

fn grouped_bending_span(
    model: &Model,
    group: &[ElemId],
    saved: &HashMap<ElemId, &MemberForces>,
    force_rows: &HashMap<ElemId, usize>,
    loads: Result<&BeamSpanLoads<'_>, &&'static str>,
    spans: &mut HashMap<ElemId, Result<GroupBending, String>>,
) -> Result<(), String> {
    if group.len() < 2 {
        return Err("一本部材を構成する梁要素が不足しています".into());
    }
    let attrs = group
        .iter()
        .map(|id| {
            let mut rows = model
                .steel_design_attrs
                .iter()
                .filter(|attr| attr.elem == *id);
            let attr = rows.next();
            if rows.next().is_some() {
                return Err("一本部材の横座屈属性が重複しています");
            }
            Ok(attr)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let section_only = |attr: Option<&SteelDesignAttr>| {
        attr.is_some_and(|attr| attr.lb_direct == Some((0.0, 0.0, 0.0)))
    };
    if attrs.iter().copied().all(section_only) {
        spans.extend(
            group
                .iter()
                .map(|&id| (id, Ok(GroupBending::WithoutWholeSpan { length_mm: None }))),
        );
        return Ok(());
    }

    let mut members = Vec::with_capacity(group.len());
    let mut total_mm = 0.0;
    let mut previous: Option<(&ElementData, [f64; 3])> = None;
    let mut outer_frame = None;

    for (id, attr) in group.iter().zip(attrs) {
        let elem = model.element(*id).ok_or("一本部材の梁要素がありません")?;
        let section = model
            .element_section(elem)
            .ok_or("一本部材の断面がありません")?;
        let material = model
            .element_material(elem)
            .ok_or("一本部材の材料がありません")?;
        if MemberKind::try_of_element(elem, model) != Ok(MemberKind::Girder)
            || !crate::steel::is_symmetric_h_section(section)
            || sepika_core::structure_kind::structure_kind_of(
                Some(section),
                Some(material.category),
            ) != StructureKind::S
        {
            return Err("一本部材が上下対称H形鋼大梁のみで構成されていません".into());
        }
        let [i, j] = model
            .primary_frame_axis_ends(elem)?
            .ok_or("一本部材の実材軸が未確認です")?;
        let frame = LocalFrame::from_nodes(i, j, elem.local_axis.ref_vector)?;
        let length_mm = dist(i, j);
        if !length_mm.is_finite() || length_mm <= 1e-9 {
            return Err("一本部材の実材軸長が不正です".into());
        }
        let root = outer_frame.get_or_insert(frame);
        if root
            .rot
            .iter()
            .zip(frame.rot)
            .any(|(a, b)| dist(*a, b) > 1e-12)
        {
            return Err("一本部材の実材軸が共線かつ同方向ではありません".into());
        }
        if let Some((prev, prev_j)) = previous {
            if prev.end_cond[1] != EndCondition::Fixed || elem.end_cond[0] != EndCondition::Fixed {
                return Err("一本部材の内部に端部解放があります".into());
            }
            if prev.nodes.last() != elem.nodes.first()
                || dist(prev_j, i) > 1e-9
                || prev.section != elem.section
            {
                return Err("一本部材の接続節点・実材軸または断面が連続していません".into());
            }
        }
        let end_mm = total_mm + length_mm;
        members.push(GroupMember {
            elem,
            attr,
            start_mm: total_mm,
            end_mm,
            length_mm,
        });
        total_mm = end_mm;
        previous = Some((elem, j));
    }
    if !total_mm.is_finite() {
        return Err("一本部材の実材軸長が不正です".into());
    }
    let partial = |attr: &SteelDesignAttr| {
        let ctx = DesignCtx {
            length: total_mm,
            steel_attr: Some(attr.clone()),
            ..Default::default()
        };
        [0.0, 0.5, 1.0].into_iter().all(|pos| {
            let (lb, partial) = beam_lateral_buckling_length(&ctx, pos);
            lb.is_finite() && (lb == 0.0 || lb > 0.0 && partial)
        })
    };
    for member in &members {
        let length_mm = if section_only(member.attr) {
            None
        } else if member.attr.is_some_and(partial) {
            Some(total_mm)
        } else {
            continue;
        };
        spans.insert(
            member.elem.id,
            Ok(GroupBending::WithoutWholeSpan { length_mm }),
        );
    }
    if group.iter().any(|id| spans.contains_key(id)) {
        return Err("一本部材の全要素に共通の単一補剛区間が指定されていません".into());
    }
    let beam_loads = loads.map_err(|reason| (**reason).to_owned())?;
    let mut previous_end: Option<[f64; 6]> = None;
    let mut rows = Vec::new();
    let mut loads = Vec::new();
    let mut faces = [0.0; 2];
    let c_direct = members[0].attr.and_then(|attr| attr.c_direct);

    for (index, member) in members.iter().enumerate() {
        let GroupMember {
            elem,
            attr,
            start_mm,
            length_mm,
            ..
        } = *member;
        let id = &elem.id;
        let attr = attr.ok_or("一本部材の有効支点間距離が指定されていません")?;
        let lb = attr
            .lb_direct
            .ok_or("一本部材の有効支点間距離が指定されていません")?;
        if [lb.0, lb.1, lb.2]
            .into_iter()
            .any(|lb| !lb.is_finite() || (lb - total_mm).abs() > 1e-9)
        {
            return Err("一本部材の実材軸全長と指定した単一補剛区間が一致しません".into());
        }
        if attr.has_section_loss()
            || attr.lateral_brace_count.is_some_and(|n| n > 0)
            || attr.c_direct.is_some_and(|c| !c.is_finite() || c <= 0.0)
            || c_direct != attr.c_direct
        {
            return Err("一本部材で共通の無欠損断面と単一補剛区間の条件を確認できません".into());
        }
        let [Some(face_i_mm), Some(face_j_mm)] = [elem.rigid_zone.face_i, elem.rigid_zone.face_j]
        else {
            return Err("一本部材の材端フェース位置が未確認です".into());
        };
        if !(face_i_mm >= 0.0 && face_j_mm >= 0.0 && face_i_mm + face_j_mm < length_mm) {
            return Err("一本部材の材端フェース位置が不正です".into());
        }
        if index != 0
            && (face_i_mm != 0.0
                || elem.rigid_zone.rigid_length_i() != 0.0
                || elem.end_cond[0] != EndCondition::Fixed)
            || index + 1 != group.len()
                && (face_j_mm != 0.0
                    || elem.rigid_zone.rigid_length_j() != 0.0
                    || elem.end_cond[1] != EndCondition::Fixed)
        {
            return Err("一本部材の内部に材端フェースまたは剛域があります".into());
        }

        if index == 0 {
            faces[0] = face_i_mm;
        }
        if index + 1 == group.len() {
            faces[1] = face_j_mm;
        }
        if force_rows.get(id) != Some(&1) {
            return Err("一本部材の一意な解析断面力がありません".into());
        }
        let mf = saved[id];
        let Some(&(0.0, start)) = mf.at.first() else {
            return Err("一本部材の要素i端の解析断面力がありません".into());
        };
        let Some(&(1.0, end)) = mf.at.last() else {
            return Err("一本部材の要素j端の解析断面力がありません".into());
        };
        if !mf
            .at
            .iter()
            .all(|(xi, f)| (0.0..=1.0).contains(xi) && f.iter().all(|v| v.is_finite()))
            || mf.at.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err("一本部材の解析断面力または評価位置が不正です".into());
        }
        if let Some(prev_end) = previous_end {
            if prev_end
                .into_iter()
                .zip(start)
                .any(|(a, b)| (a - b).abs() > 1e-8 * a.abs().max(b.abs()).max(1.0))
            {
                return Err("一本部材の要素境界で解析断面力が連続していません".into());
            }
        }
        for &(factor, original) in beam_loads.0.get(id).into_iter().flatten() {
            original.validate_extent(length_mm)?;
            let mut load = original.clone();
            match &mut load.kind {
                MemberLoadKind::Point { a, .. } => {
                    if !(0.0..=length_mm).contains(a)
                        || index != 0 && *a == 0.0
                        || index + 1 != group.len() && *a == length_mm
                    {
                        return Err("一本部材の要素境界と集中部材荷重の対応が未確認です".into());
                    }
                    *a += start_mm;
                }
                MemberLoadKind::Distributed { a, b, .. } => {
                    if !(0.0 <= *a && *a < *b && *b <= length_mm) {
                        return Err("一本部材の要素と分布荷重の範囲が一致しません".into());
                    }
                    *a += start_mm;
                    *b += start_mm;
                }
            }
            load.extent = MemberLoadExtent::FixedDistance;
            loads.push((factor, load));
        }
        // 同じ境界の二つの保存応答は照合済みなので、全長分布には一行だけを渡す。
        rows.extend(
            mf.at
                .iter()
                .skip(usize::from(index != 0))
                .map(|(xi, f)| (start_mm + xi * length_mm, *f)),
        );
        previous_end = Some(end);
    }

    let demand = [faces[0], total_mm - faces[1]];
    let loads: Vec<_> = loads.iter().map(|(factor, load)| (*factor, load)).collect();
    let span = SteelBeamSpan::from(frame_strong_bending_span_mm(
        &outer_frame.expect("一本部材の構成要素あり"),
        total_mm,
        demand,
        &loads,
        &rows,
    )?);
    spans.extend(members.into_iter().map(|member| {
        let demand_peaks = span
            .demand_peaks
            .iter()
            .filter_map(|peak| {
                if !(member.start_mm..=member.end_mm).contains(&peak.x_mm) {
                    return None;
                }
                let x_mm = (peak.x_mm - member.start_mm).clamp(0.0, member.length_mm);
                Some(FrameStrongBendingPeak {
                    xi: x_mm / member.length_mm,
                    x_mm,
                    left: peak.left,
                    right: peak.right.clone(),
                    is_i_face: peak.is_i_face,
                    is_j_face: peak.is_j_face,
                })
            })
            .collect();
        (
            member.elem.id,
            Ok(GroupBending::Whole {
                length_mm: total_mm,
                span: SteelBeamSpan {
                    diagram: span.diagram,
                    demand_peaks,
                },
            }),
        )
    }));
    Ok(())
}
