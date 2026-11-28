//! 部材照査から参照する確定解析応答。

use std::collections::HashMap;

use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{ElementData, MemberDetailAttr, MemberLoad, Model, RigidZone};
use sepika_element::behavior::LoadedFrameResponse;
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::{
    confirm_frame_recorded_section_cut, frame_strong_bending_intervals, frame_strong_bending_span,
    FrameStrongBendingSpan, LoadedFrameSection,
};
use sepika_element::transform::LocalFrame;

/// 応答を確定した荷重状態と解析時点。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConfirmedLoadState<'a> {
    /// 荷重ケース ID と係数。時点は線形静的解析の最終結果。
    LinearStatic(&'a [(LoadCaseId, f64)]),
    /// 実行内の確定ステップ、時刻 [s]、長期初期載荷の設定。
    TimeHistory {
        step: u64,
        time_s: f64,
        applied_long_term: bool,
    },
}

/// 同じ解析結果・荷重状態・時点における一部材の応答。
pub struct ConfirmedMemberResponse<'a> {
    id: ElemId,
    state: ConfirmedLoadState<'a>,
    forces: Option<&'a MemberForces>,
    loaded_section: Option<(
        &'a LoadedFrameSection,
        &'a RigidZone,
        Option<&'a MemberDetailAttr>,
    )>,
    bending: BendingSource<'a>,
}

enum BendingSource<'a> {
    Static(&'a StaticMemberLoads<'a>, &'a ElementData),
    TimeHistory(&'a TimeHistoryBending<'a>, usize),
}

impl ConfirmedMemberResponse<'_> {
    pub fn member_id(&self) -> ElemId {
        self.id
    }

    pub fn load_state(&self) -> ConfirmedLoadState<'_> {
        self.state
    }

    /// 解析が指定の評価位置 ξ で記録した局所 [N, Qy, Qz, Mx, My, Mz] [N, N·mm]。物理位置や切断側を選ぶ API ではない。
    /// 集中部材荷重の切断側を確定できなければエラー。非線形時刻歴では同じ確定時刻の荷重込み応答で確認する。
    /// 欠落・重複・非有限値は補わない。
    pub fn recorded_section_forces(&self, xi: f64) -> Result<[f64; 6], &'static str> {
        let forces = self.forces.ok_or("部材の解析断面力がありません")?;
        if let BendingSource::Static(loads, elem) = self.bending {
            return loads.recorded_section_forces(elem, forces, xi);
        }
        let local = recorded_forces_at(forces, xi)?;
        if let Some((section, rigid_zone, detail)) = self.loaded_section {
            section.confirm_recorded_section_cut(xi, rigid_zone, detail)?;
        }
        Ok(local)
    }

    /// 両実材端を補剛端とする強軸曲げ分布と、照査範囲の最大曲げ位置の断面力を返す。
    /// 座標はi端起点 [mm]。時刻歴では解析応答の全長と厳密に一致する区間のみ指定できる。
    /// 分布の不足は `unverified_distribution` またはエラーで返す。
    pub fn strong_bending_span(
        &self,
        bounds_mm: [f64; 2],
        check_mm: [f64; 2],
    ) -> Result<FrameStrongBendingSpan, String> {
        let [start_mm, length_mm] = bounds_mm;
        if start_mm != 0.0 {
            return Err("単一補剛区間はi端から指定してください".into());
        }
        match self.bending {
            BendingSource::Static(loads, elem) => loads.recover_strong_bending_span(
                elem,
                length_mm,
                check_mm,
                self.forces.ok_or("部材の解析断面力がありません")?,
            ),
            BendingSource::TimeHistory(bending, index) => {
                if check_mm != bounds_mm {
                    return Err("時刻歴の補剛区間と照査範囲は両実材端間のみ指定できます".into());
                }
                bending(index, length_mm)
            }
        }
    }

    /// 両実材端を含む境界で区切った各補剛区間の曲げ分布を、境界順に返す。
    /// 座標はi端起点 [mm]。線形静的応答のみ。
    pub fn strong_bending_intervals(
        &self,
        boundaries_mm: &[f64],
        check_mm: [f64; 2],
    ) -> Result<Vec<FrameStrongBendingSpan>, String> {
        let BendingSource::Static(loads, elem) = self.bending else {
            return Err("複数補剛区間の曲げ分布は線形静的応答のみ取得できます".into());
        };
        loads.recover_strong_bending_intervals(
            elem,
            boundaries_mm,
            check_mm,
            self.forces.ok_or("部材の解析断面力がありません")?,
        )
    }
}

/// 線形静的解析の荷重ケース・荷重係数による部材荷重の索引。
pub struct StaticMemberLoads<'a> {
    model: &'a Model,
    terms: &'a [(LoadCaseId, f64)],
    by_member: HashMap<ElemId, Vec<(f64, &'a MemberLoad)>>,
}

fn recorded_forces_at(forces: &MemberForces, xi: f64) -> Result<[f64; 6], &'static str> {
    let mut matches = forces.at.iter().filter(|(x, _)| *x == xi);
    let &(_, local) = matches
        .next()
        .ok_or("指定した評価位置の解析断面力がありません")?;
    if matches.next().is_some() {
        return Err("同一評価位置の断面力が重複しています");
    }
    if local.iter().any(|v| !v.is_finite()) {
        return Err("断面力が非有限です");
    }
    Ok(local)
}

impl<'a> StaticMemberLoads<'a> {
    /// `model` と `terms` は対象の解析と同じ入力を指定する。由来が不明ならエラー。
    pub fn from_terms(
        model: &'a Model,
        terms: Option<&'a [(LoadCaseId, f64)]>,
    ) -> Result<Self, &'static str> {
        let terms = terms
            .filter(|terms| !terms.is_empty())
            .ok_or("解析応答と同じ荷重ケース・荷重組合せの由来が未確認です")?;
        Self::from_effective_terms(model, terms)
    }

    fn from_effective_terms(
        model: &'a Model,
        terms: &'a [(LoadCaseId, f64)],
    ) -> Result<Self, &'static str> {
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
        Ok(Self {
            model,
            terms,
            by_member,
        })
    }

    pub fn for_member(&self, id: ElemId) -> &[(f64, &'a MemberLoad)] {
        self.by_member.get(&id).map_or(&[], Vec::as_slice)
    }

    /// 同じ部材・荷重状態と照合済みの断面力から評価位置 ξ の局所断面力 [N, N·mm] を取得する。
    /// 荷重点と評価断面の実距離が一致する記録は左極限。欠落・重複・非有限値・切断側不明はエラー。
    /// 解析の由来は認定しない。
    pub fn recorded_section_forces(
        &self,
        elem: &ElementData,
        forces: &MemberForces,
        xi: f64,
    ) -> Result<[f64; 6], &'static str> {
        let local = recorded_forces_at(forces, xi)?;
        let length_mm = self.model.member_length(elem);
        if !length_mm.is_finite() || length_mm <= 0.0 {
            return Err("有効な材軸長を取得できません");
        }
        let loads = self.for_member(elem.id);
        if !loads.is_empty() {
            let frame = self
                .frame(elem, length_mm)
                .map_err(|_| "梁の実材軸が未確認です")?;
            confirm_frame_recorded_section_cut(
                &frame,
                length_mm,
                &elem.rigid_zone,
                self.model.member_detail(elem.id),
                xi,
                loads.iter().copied(),
            )?;
        }
        Ok(local)
    }

    /// 両実材端間の強軸曲げを、指定した断面力と荷重の釣合いから回復する低水準操作。
    /// 断面力の解析実行・荷重状態の由来は認定しない。座標・長さは [mm]。
    pub fn recover_strong_bending_span(
        &self,
        elem: &ElementData,
        length_mm: f64,
        check_mm: [f64; 2],
        forces: &MemberForces,
    ) -> Result<FrameStrongBendingSpan, String> {
        let frame = self.frame(elem, length_mm)?;
        frame_strong_bending_span(
            &frame,
            length_mm,
            check_mm,
            self.for_member(elem.id),
            forces,
        )
    }

    /// 断面力の由来を認定せず、両実材端を含む境界 [mm] 順に強軸曲げを回復する。
    pub fn recover_strong_bending_intervals(
        &self,
        elem: &ElementData,
        boundaries_mm: &[f64],
        check_mm: [f64; 2],
        forces: &MemberForces,
    ) -> Result<Vec<FrameStrongBendingSpan>, String> {
        let &length_mm = boundaries_mm.last().ok_or("補剛区間の境界がありません")?;
        let frame = self.frame(elem, length_mm)?;
        frame_strong_bending_intervals(
            &frame,
            length_mm,
            check_mm,
            boundaries_mm,
            self.for_member(elem.id),
            forces,
        )
    }

    fn frame(&self, elem: &ElementData, length_mm: f64) -> Result<LocalFrame, String> {
        let [i, j] = self
            .model
            .primary_frame_axis_ends(elem)?
            .or_else(|| {
                let &[i, j] = elem.nodes.as_slice() else {
                    return None;
                };
                Some([self.model.node(i)?.coord, self.model.node(j)?.coord])
            })
            .ok_or("梁の実材軸が未確認です")?;
        if length_mm != self.model.member_length(elem) {
            return Err("指定した補剛区間と解析の実材軸全長が一致しません".into());
        }
        Ok(LocalFrame::from_nodes(i, j, elem.local_axis.ref_vector)?)
    }
}

/// 一つの線形静的結果の確定部材応答。
pub struct StaticMemberResponses<'a> {
    loads: StaticMemberLoads<'a>,
    members: MemberIndices<'a>,
    rows: HashMap<ElemId, Option<&'a MemberForces>>,
}

impl<'a> StaticMemberResponses<'a> {
    pub(crate) fn from_solved(
        model: &'a Model,
        terms: &'a [(LoadCaseId, f64)],
        forces: &'a [(ElemId, MemberForces)],
    ) -> Result<Self, &'static str> {
        let loads = StaticMemberLoads::from_effective_terms(model, terms)?;
        let mut rows = HashMap::with_capacity(forces.len());
        for (id, mf) in forces {
            rows.entry(*id)
                .and_modify(|row| *row = None)
                .or_insert(Some(mf));
        }
        Ok(Self {
            loads,
            members: MemberIndices::new(model),
            rows,
        })
    }

    /// 指定部材の確定応答。部材または一意な解析断面力がなければエラー。
    pub fn member(&self, id: ElemId) -> Result<ConfirmedMemberResponse<'_>, &'static str> {
        let elem = &self.loads.model.elements[self.members.index(id)?];
        let forces = self
            .rows
            .get(&id)
            .ok_or("部材の解析断面力がありません")?
            .ok_or("部材の解析断面力が重複しています")?;
        Ok(ConfirmedMemberResponse {
            id,
            state: ConfirmedLoadState::LinearStatic(self.loads.terms),
            forces: Some(forces),
            loaded_section: None,
            bending: BendingSource::Static(&self.loads, elem),
        })
    }
}

pub(crate) type TimeHistoryBending<'a> =
    dyn Fn(usize, f64) -> Result<FrameStrongBendingSpan, String> + 'a;

pub(crate) struct MemberIndices<'a> {
    model: &'a Model,
    indices: HashMap<ElemId, Option<usize>>,
}

impl<'a> MemberIndices<'a> {
    pub(crate) fn new(model: &'a Model) -> Self {
        let mut indices = HashMap::with_capacity(model.elements.len());
        for (index, elem) in model.elements.iter().enumerate() {
            indices
                .entry(elem.id)
                .and_modify(|row| *row = None)
                .or_insert(Some(index));
        }
        Self { model, indices }
    }

    fn index(&self, id: ElemId) -> Result<usize, &'static str> {
        self.indices
            .get(&id)
            .ok_or("指定した部材がありません")?
            .ok_or("部材 ID が重複し、一意の解析応答を確認できません")
    }
}

/// 一つの時刻歴実行の、表示間引き前の確定時刻の応答。
pub struct ConfirmedTimeHistoryStep<'a> {
    pub(crate) members: &'a MemberIndices<'a>,
    pub(crate) step: u64,
    pub(crate) time_s: f64,
    pub(crate) applied_long_term: bool,
    pub(crate) forces: &'a [Option<MemberForces>],
    pub(crate) loaded: &'a [Option<LoadedFrameResponse>],
    pub(crate) bending: &'a TimeHistoryBending<'a>,
}

impl ConfirmedTimeHistoryStep<'_> {
    pub fn step(&self) -> u64 {
        self.step
    }

    /// 初期状態からの解析時刻 [s]。
    pub fn time_s(&self) -> f64 {
        self.time_s
    }

    /// 指定部材をこの確定時刻で参照する。応答成分の有無は個別の取得結果で確認する。
    pub fn member(&self, id: ElemId) -> Result<ConfirmedMemberResponse<'_>, &'static str> {
        let index = self.members.index(id)?;
        Ok(ConfirmedMemberResponse {
            id,
            state: ConfirmedLoadState::TimeHistory {
                step: self.step,
                time_s: self.time_s,
                applied_long_term: self.applied_long_term,
            },
            forces: self.forces.get(index).and_then(Option::as_ref),
            loaded_section: self.loaded.get(index).and_then(Option::as_ref).map(|load| {
                (
                    &load.section,
                    &self.members.model.elements[index].rigid_zone,
                    self.members.model.member_detail(id),
                )
            }),
            bending: BendingSource::TimeHistory(self.bending, index),
        })
    }
}
