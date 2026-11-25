use std::collections::HashMap;

use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{ElementData, MemberLoad, Model, Section};
use sepika_core::structure_kind::StructureKind;
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{frame_strong_bending_span, FrameStrongBendingPeak};
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
        grouped: bool,
    ) -> Result<SteelBeamSpan, String> {
        if grouped {
            return Err("一本部材グループの全長に対応する強軸曲げ分布が未確認です".into());
        }
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
        let diagram = if span.interior_maximum {
            SteelBeamMomentDiagram::InteriorMaximum
        } else if span.monotonic {
            SteelBeamMomentDiagram::Monotonic(span.end_moments_nmm)
        } else {
            SteelBeamMomentDiagram::Unverified
        };
        Ok(SteelBeamSpan {
            diagram,
            demand_peaks: span.demand_peaks,
        })
    }
}

pub(super) struct SteelBeamSpan {
    pub diagram: SteelBeamMomentDiagram,
    pub demand_peaks: Vec<FrameStrongBendingPeak>,
}
