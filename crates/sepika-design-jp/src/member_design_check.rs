//! 部材断面検定の共通オーケストレーション。
//!
//! 線形静的解析の部材内力を使って許容応力度検定を行う。検定式そのものは [`crate::DesignCheck`]
//! 各実装が担い、本モジュールは次の**配線**を一箇所に集約する。
//!
//! - 部材単位の [`crate::DesignCtx`] 組み立て（部材長・座屈長さ・せん断スパン比代表値等）
//! - 構造種別に応じた [`crate::checker_for`] による検定器選択
//! - 危険断面位置（[`crate::design_position`]）での断面検定ループ
//! - 節点単位検定（[`crate::joint_wiring`]）
//!
//! 呼び出し側は解析結果（部材内力・パネルモーメント）と
//! [`MemberDesignCheckOptions`] を渡す。地震時短期 QD 割増・一本部材グループ合成・
//! 床設計など app 固有の前処理は呼び出し側で行い、本関数は与えられた
//! オプションをそのまま検定文脈へ反映する。

use std::collections::{HashMap, HashSet};

use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::section_shape::SectionShape;
use sepika_core::structure_kind::{structure_kind_of, StructureKind};
use sepika_element::frame::beam::MemberForces;
use sepika_solver::statics::linear::{GirderColumnFaceEnds, SteelPanelColumnFaceForce};

mod rc_column_ductility;
pub(crate) mod rc_column_long_term_shear;
mod steel_beam_span;
mod steel_column_span;
pub use rc_column_ductility::{
    RcColumnDuctilityAdvisory, RcColumnDuctilityApplicability, RcColumnDuctilityComparison,
};
use rc_column_long_term_shear::{missing_design_positions, ColumnShearLoads};

use crate::design_position::{design_positions, is_near_design_position};
use crate::{
    beam_has_attached_slab, checker_for, girder_deflection, BondMethod, CheckOutcome, CheckResult,
    DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind, QdMethod, SeismicQd,
    SrcColumnRoute21Shear,
};

fn with_qd1_column_faces(
    id: ElemId,
    rows: &MemberForces,
    geometry: crate::rc::BeamQd1Geometry,
    saved_faces: &[(ElemId, GirderColumnFaceEnds)],
    allow_partial: bool,
) -> MemberForces {
    use crate::design_position::same_qd1_station;
    let mut at = rows.at.clone();
    let mut candidates = saved_faces.iter().filter(|(elem_id, _)| *elem_id == id);
    if let (Some((_, faces)), None) = (candidates.next(), candidates.next()) {
        for (side, col) in faces.iter().enumerate() {
            let xi = geometry.end_positions[side];
            if geometry.uses_material_end[side] || at.iter().any(|(p, _)| same_qd1_station(xi, *p))
            {
                continue;
            }
            let Ok(face) = col else { continue };
            let Ok(forces) = face.forces else { continue };
            if (face.face.length_mm - geometry.axis_length_mm).abs()
                <= 32.0 * f64::EPSILON * geometry.axis_length_mm
                && same_qd1_station(xi, face.face.position_mm / face.face.length_mm)
                && (allow_partial || forces.iter().all(|value| value.is_finite()))
            {
                at.push((xi, forces));
            }
        }
    }
    at.sort_by(|a, b| a.0.total_cmp(&b.0));
    MemberForces { at }
}

/// 一本部材グループ合成値（断面検定の採用応力上書き用）。
///
/// `Model.girder_groups` から合成した部材長・端部/中央モーメント等を、
/// 所属する分割梁要素の検定文脈へ上書きする。合成ロジックは [`crate::girder_group::girder_group_overrides`]。
#[derive(Clone, Debug, PartialEq)]
pub struct GirderGroupContextOverride {
    /// 一本部材の全長 L `[mm]`（分割部材長の総和）。
    pub length: f64,
    /// 一本部材両端の強軸曲げ `(M_i端, M_j端)` [N·mm]。
    pub end_moments_z: Option<(f64, f64)>,
    /// 一本部材中央の強軸曲げ Mc [N·mm]。
    pub mid_moment_z: Option<f64>,
    /// グループ内 |Mz| 最大位置の `(|M|, |Q|)`（せん断スパン比の代表値）。
    pub shear_span: Option<(f64, f64)>,
    /// 一本部材の内法長（両外端の剛域控除後）`[mm]`。
    pub clear_length: f64,
}

/// 部材断面検定オーケストレーションの実行オプション。
#[derive(Clone, Debug, Default)]
pub struct MemberDesignCheckOptions<'a> {
    /// 検定条件（長期/短期）。
    pub term: LoadTerm,
    /// 同じ解析結果の大梁実柱面断面力。欠落時は対応するSRC接合部を未算定とする。
    pub girder_column_face_forces: &'a [(ElemId, GirderColumnFaceEnds)],
    /// RC梁実材端検定と同じ保存応答の実柱面内力。通常の評価断面に同位置がない場合だけ使う。
    pub rc_beam_qd1_face_forces: &'a [(ElemId, GirderColumnFaceEnds)],
    /// 上記と同じ実柱面の重力参照内力。保存位置を推定して補わない。
    pub rc_beam_qd1_gravity_faces: &'a [(ElemId, GirderColumnFaceEnds)],
    /// 同じ解析結果のS造仕口フランジ位置における荷重込み柱軸力。
    pub steel_panel_column_face_forces: &'a [SteelPanelColumnFaceForce],
    /// 壁展開時の元壁版と生成要素の対応。壁版のない要素fixtureではNoneを許す。
    pub wall_index: Option<&'a sepika_core::wall_expand::WallExpansionIndex>,
    /// 検定する荷重ケース・組合せの既存キー。
    pub wall_case: &'a str,
    /// 同じ解析条件でSRC建物の保有水平耐力の検討が成立済みの場合にtrue。検討の合否を問わない。
    pub src_holding_reviewed: bool,
    /// SRC壁の検定対象組合せに含まれる重力内力。欠落時は必要な検定を未算定にする。
    pub src_wall_gravity_forces: Option<&'a [(ElemId, MemberForces)]>,
    /// 同じ応力バッチを生成した荷重terms。ケース文字列からは推定しない。
    /// None では大梁の長期たわみは未算定とし、壁は全荷重ケースの未対応部材荷重を検査する。
    pub load_terms: Option<&'a [(sepika_core::ids::LoadCaseId, f64)]>,
    /// 鋼梁・鋼柱の補剛区間の曲げを照合する、解析に用いた荷重ケースID・係数。モデルは解析時の部材荷重を持つこと。
    /// 不明ならNoneとし、強軸曲げ分布は未確認とする。
    pub steel_beam_span_load_terms: Option<&'a [(sepika_core::ids::LoadCaseId, f64)]>,
    /// ハンチ付きRC梁の許容せん断力を確認する、同じ線形静的応答の荷重ケースID・係数。
    /// モデルは解析時の部材荷重を持つこと。未指定時は正確な曲げ分布を確認できない。
    pub rc_beam_qd1_haunch_load_terms: Option<&'a [(sepika_core::ids::LoadCaseId, f64)]>,
    /// 通常の長期RC柱検定に渡す線形静的解析結果の由来。
    pub rc_column_long_term_shear: RcColumnLongTermShearInput<'a>,
    /// 設計者が判断したRC柱ごとの短期圧縮靱性推奨の適用。未指定の柱は判別不能とする。
    pub rc_column_ductility_applicability:
        Option<&'a HashMap<ElemId, RcColumnDuctilityApplicability>>,
    /// RC短期せん断を損傷制御で検定する場合はtrue。断面力には長期・水平荷重を同じ位置で合算して渡す。
    pub rc_damage_control: bool,
    /// RC 梁付着検定の方式（既定は 1999）。
    pub bond_method: BondMethod,
    /// 地震時短期 QD の決定方法（`long_member_forces` が与えられた場合のみ有効）。
    pub qd_method: QdMethod,
    /// 今回の短期地震荷重組合せのSRC柱ルート2-1確認済み入力（部材 ID → 入力）。
    /// 呼出側が解析対象の組合せ・部材・局所加力の向きとの一致を確認する。
    /// Some は当該組合せのルート2-1検定を指定し、該当部材の入力がなければ未算定とする。
    pub src_column_route21_shear: Option<&'a HashMap<ElemId, SrcColumnRoute21Shear>>,
    /// 地震時短期 QD 用の長期（DL+LL 等）内力。None なら QD 割増なし。
    pub long_member_forces: Option<&'a [(ElemId, MemberForces)]>,
    /// RC 梁 QD1 用の単純梁せん断 Q0 `[N]`（部材 ID → 絶対値）。RC梁のQD1で不足すれば未検定。
    pub q_simple_by_elem: Option<&'a HashMap<ElemId, f64>>,
    /// 一本部材として指定したRC矩形梁のスパン全体のQ0 [N] または未算定理由（全区間の要素IDで参照）。
    /// 不足時は区間のQ0を代用しない。
    pub rc_grouped_beam_q0: Option<&'a HashMap<ElemId, Result<f64, String>>>,
    /// 一本部材グループの検定文脈上書き（梁のみ適用）。None なら部材単体の値を用いる。
    pub girder_group_overrides: Option<&'a HashMap<ElemId, GirderGroupContextOverride>>,
}

/// 通常の長期RC柱検定に用いる部材内力の由来。
#[derive(Clone, Copy, Debug, Default)]
pub enum RcColumnLongTermShearInput<'a> {
    /// 由来が未確認のため長期せん断を未算定とする。
    #[default]
    Unverified,
    /// 単独長期ケースの断面位置での検定。
    SingleCase,
    /// 保存組合せ名。同じ組合せの線形静的解析結果と生成・展開済み解析モデルを渡す。
    SavedCombination(&'a str),
}

/// 部材断面検定オーケストレーションの結果。
#[derive(Clone, Debug, Default)]
pub struct MemberDesignCheckReport {
    /// 部材断面検定（危険断面位置を含む）。
    /// RC・SRC・CFTでねじりモーメントが非零または非有限の位置には未算定行を返す。
    pub member_checks: Vec<(ElemId, f64, CheckOutcome)>,
    /// RC柱の短期圧縮靱性推奨の付記。長期は空で、許容応力度の検定比・合否に含めない。
    pub rc_column_ductility_advisories: Vec<RcColumnDuctilityAdvisory>,
    /// 必要横補剛箇所数の参考結果。検定比・合否には含めない。
    pub lateral_bracing_references: Vec<(
        ElemId,
        crate::steel::lateral_bracing::LateralBracingReference,
    )>,
    /// 鋼大梁の長期たわみの参考値または理由。検定比・合否には含めない。
    pub steel_deflection_references: Vec<(ElemId, String)>,
    /// 節点単位検定（柱梁接合部・パネルゾーン・冷間成形耐力比）。
    pub joint_checks: Vec<(NodeId, String, CheckOutcome)>,
    /// 元壁版・生成要素・荷重キー・検定種別ごとの壁検定。
    pub wall_checks: Vec<crate::wall_check::WallCheck>,
}

impl MemberDesignCheckReport {
    /// 荷重状態・必要応力が不足する場合、全検定を理由付き未検定にする。
    pub fn skip_for_load_state(&mut self, reason: &str) {
        self.skip_except_members(reason, |_| false);
    }

    /// 荷重状態・必要応力が不足する場合、独立に照査できる指定部材の断面検定以外を未検定にする。
    pub fn skip_except_members(&mut self, reason: &str, mut preserve: impl FnMut(ElemId) -> bool) {
        let skipped = || CheckOutcome::Skipped {
            reason: reason.into(),
        };
        for advisory in &mut self.rc_column_ductility_advisories {
            for (_, comparison) in &mut advisory.positions {
                *comparison = RcColumnDuctilityComparison::Unavailable;
            }
        }
        for (_, detail) in &mut self.steel_deflection_references {
            *detail = format!("たわみ参考 未算定（{reason}）");
        }
        for (elem, _, outcome) in &mut self.member_checks {
            if preserve(*elem) {
                continue;
            }
            match outcome {
                CheckOutcome::Checked(_) => *outcome = skipped(),
                CheckOutcome::Skipped { reason: prior } => prior.push_str(&format!("。{reason}")),
            }
        }
        for (_, _, outcome) in &mut self.joint_checks {
            if matches!(outcome, CheckOutcome::Checked(_)) {
                *outcome = skipped();
            }
        }
        for row in &mut self.wall_checks {
            if matches!(row.outcome, CheckOutcome::Checked(_)) {
                row.reference_skeleton = None;
                row.outcome = skipped();
                row.skip_kind = Some(crate::wall_check::WallSkipKind::MissingInput);
            }
        }
    }
}

pub(crate) fn is_steel_input_target(
    sec: Option<&sepika_core::model::Section>,
    mat: Option<&sepika_core::model::Material>,
) -> bool {
    use sepika_core::model::MaterialCategory;
    use sepika_core::structure_kind::{shape_default_kind, StructureKind};
    let shape = sec.and_then(|s| s.shape.as_ref());
    if shape
        .and_then(sepika_core::structure_kind::shape_composite_kind)
        .is_some()
    {
        return false;
    }
    shape.is_some_and(|s| shape_default_kind(s) == StructureKind::S)
        || mat.is_some_and(|m| m.category == MaterialCategory::Steel)
        || shape.is_none() && mat.is_none()
}

fn is_rc_column_target(model: &Model, elem: &ElementData) -> bool {
    elem.kind.is_primary_frame_support()
        && model.element_section(elem).is_some_and(|sec| {
            sec.frame_use == Some(sepika_core::model::FrameSectionUse::Column)
                && matches!(
                    sec.shape,
                    Some(SectionShape::RcColumnRect { .. } | SectionShape::RcColumnCircle { .. })
                )
                && model.element_material(elem).is_some_and(|material| {
                    structure_kind_of(Some(sec), Some(material.category)) == StructureKind::Rc
                })
        })
}

fn frame_check_target(elem: &ElementData) -> bool {
    matches!(
        elem.kind,
        ElementKind::Beam
            | ElementKind::Fiber
            | ElementKind::MultiSpring
            | ElementKind::Brace { .. }
    )
}

/// RC・SRC・CFT部材の断面力に対する材軸ねじりの未算定理由を返す。厳密にゼロのMxは対象外とする。
pub fn concrete_member_torsion_reason(
    structure: StructureKind,
    &MemberForcesAt {
        n,
        qy,
        qz,
        mx,
        my,
        mz,
        ..
    }: &MemberForcesAt,
) -> Option<&'static str> {
    if !matches!(
        structure,
        StructureKind::Rc | StructureKind::Src | StructureKind::Cft
    ) || mx == 0.0
    {
        return None;
    }
    if !mx.is_finite() {
        Some("ねじりモーメントMxが非有限のため、応力度検定は未算定です")
    } else if [n, qy, qz, my, mz].into_iter().all(|force| force == 0.0) {
        Some("純ねじり（Mx）が作用しています。ねじりの許容耐力と適用条件が未確定のため、応力度検定は未算定です")
    } else {
        Some("ねじりモーメントMxが作用しています。ねじりと他の断面力が同時に作用する場合の検定条件が未確定のため、応力度検定は未算定です")
    }
}

fn push_strength_outcome(
    checks: &mut Vec<(ElemId, f64, CheckOutcome)>,
    elem: ElemId,
    pos: f64,
    outcome: CheckOutcome,
    incomplete_reason: Option<&str>,
) {
    checks
        .extend(strength_outcomes(outcome, incomplete_reason).map(|outcome| (elem, pos, outcome)));
}

/// 未算定理由がある位置では独立に確定した不適合と理由を返す。理由がなければ元の結果を返す。
pub fn strength_outcomes(
    outcome: CheckOutcome,
    incomplete_reason: Option<&str>,
) -> impl Iterator<Item = CheckOutcome> {
    let Some(incomplete_reason) = incomplete_reason else {
        return [Some(outcome), None].into_iter().flatten();
    };

    let (known_ng, reason) = match outcome {
        CheckOutcome::Checked(mut result) => {
            result.components.retain(|component| component.ratio > 1.0);
            (
                (!result.components.is_empty()).then_some(CheckOutcome::Checked(result)),
                incomplete_reason.into(),
            )
        }
        CheckOutcome::Skipped { reason } => (None, format!("{reason}。{incomplete_reason}")),
    };
    [known_ng, Some(CheckOutcome::Skipped { reason })]
        .into_iter()
        .flatten()
}

pub(crate) fn combine_incomplete_reasons(
    incomplete: Option<String>,
    torsion: Option<&str>,
) -> Option<String> {
    match (incomplete, torsion) {
        (Some(incomplete), Some(torsion)) => Some(format!("{incomplete}。{torsion}")),
        (Some(incomplete), None) => Some(incomplete),
        (None, torsion) => torsion.map(str::to_owned),
    }
}

/// 鉄骨部材の検定属性の参照と、通常鋼線材・鋼小梁・S/CFTパネルの材料・実板厚・Fを検証する。
/// 入力不備は対象IDと理由を付けて返す。幾何未対応や検定荷重不足の判定は含めない。
pub fn validate_model_steel_inputs(model: &Model) -> Result<(), String> {
    model
        .validate_steel_design_attr_refs()
        .map_err(|error| error.to_string())?;
    for elem in model
        .elements
        .iter()
        .filter(|elem| frame_check_target(elem))
    {
        let sec = model.element_section(elem);
        let mat = model.element_material(elem);
        if !is_steel_input_target(sec, mat) {
            continue;
        }
        let sec =
            sec.ok_or_else(|| format!("部材 {}: 断面が未割当または参照先が不明です", elem.id.0))?;
        crate::material_strength::section_steel_f(sec, mat)
            .map_err(|reason| format!("部材 {}: {reason}", elem.id.0))?;
    }
    for beam in model.beams() {
        crate::floor::secondary_beam_steel_e_and_ft(model, beam)?;
    }
    crate::joint_wiring::validate_steel_panel_inputs(model)
}

/// 部材内力に対する許容応力度検定を一括実行する。
///
/// 部材ループ（DesignCtx 組み立て → 危険断面位置での検定）のほか、
/// 大梁の長期たわみと節点単位検定も含む。床の小梁・スラブ設計は
/// 呼び出し側（`sepika-app`）の責務とする。鋼材入力不備は対象ID付きエラー。
pub fn run_member_design_checks(
    model: &Model,
    member_forces: &[(ElemId, MemberForces)],
    panel_moments: &[(NodeId, [f64; 2])],
    options: &MemberDesignCheckOptions<'_>,
) -> Result<MemberDesignCheckReport, String> {
    let mut elem_by_id = HashMap::with_capacity(model.elements.len());
    for e in &model.elements {
        elem_by_id.entry(e.id).or_insert(e);
    }
    model.validate_local_frames()?;
    validate_model_steel_inputs(model)?;

    let long_mf_by_id: Option<HashMap<ElemId, &MemberForces>> =
        options.long_member_forces.map(|list| {
            let mut m = HashMap::with_capacity(list.len());
            for (id, mf) in list {
                m.entry(*id).or_insert(mf);
            }
            m
        });

    let beam_span_loads =
        steel_beam_span::BeamSpanLoads::from_terms(model, options.steel_beam_span_load_terms);
    let mut haunch_span_loads = None;
    let mut column_mechanism_topology = None;
    let mut rc_beam_adjacency = None;
    let mut member_checks: Vec<(ElemId, f64, CheckOutcome)> = Vec::new();
    let mut lateral_bracing_references = Vec::new();
    let mut steel_deflection_references = Vec::new();
    let mut standalone_deflection_checks = Vec::new();
    let grouped_girders: HashSet<_> = model.girder_groups.iter().flatten().copied().collect();
    let mut grouped_rc_qd1 = HashMap::new();
    if options.term == LoadTerm::Short
        && options.long_member_forces.is_some()
        && options.qd_method != QdMethod::Qd2
        && !options.rc_damage_control
    {
        let adjacency = sepika_core::adjacency::NodeAdjacency::primary_frame(model);
        for group in &model.girder_groups {
            let Some(id) = group.first() else { continue };
            let Some(q0) = options.rc_grouped_beam_q0.and_then(|q0| q0.get(id)) else {
                continue;
            };
            let value = crate::rc::grouped_beam_qd1_mechanism(model, &adjacency, group).and_then(
                |(span_mm, sum_my_nmm)| {
                    q0.clone()
                        .map(|q0_n| (span_mm, Some(sum_my_nmm), Some(q0_n)))
                },
            );
            for id in group {
                grouped_rc_qd1.insert(*id, value.clone());
            }
        }
    }
    let column_shear_loads = (options.term == LoadTerm::Long
        && model
            .elements
            .iter()
            .any(|elem| is_rc_column_target(model, elem)))
    .then(|| ColumnShearLoads::from_input(model, options.rc_column_long_term_shear));
    let requires_column_envelope = column_shear_loads
        .as_ref()
        .is_some_and(ColumnShearLoads::requires_envelope);
    let mut checked_column_ids = HashSet::new();
    let mut seen_column_ids = HashSet::new();
    let duplicate_column_ids: HashSet<_> = member_forces
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| {
            requires_column_envelope
                && elem_by_id
                    .get(id)
                    .is_some_and(|elem| is_rc_column_target(model, elem))
                && !seen_column_ids.insert(*id)
        })
        .collect();

    let mut force_rows = HashMap::<ElemId, usize>::with_capacity(member_forces.len());
    for (id, _) in member_forces {
        *force_rows.entry(*id).or_default() += 1;
    }
    use steel_beam_span::{GroupBending, RecoveredBending};
    let mut grouped_beam_spans = steel_beam_span::grouped_bending_spans(
        model,
        member_forces,
        &force_rows,
        beam_span_loads.as_ref(),
    );
    let route21_inputs = options
        .src_column_route21_shear
        .filter(|_| options.term == LoadTerm::Short);
    let mut unavailable_src_ids = HashSet::new();
    let mut unavailable_beam_ids = HashSet::new();
    let mut unavailable_steel_column_ids = HashSet::new();
    for elem in model
        .elements
        .iter()
        .filter(|elem| frame_check_target(elem))
    {
        let row_count = force_rows.get(&elem.id).copied().unwrap_or_default();
        if row_count == 1 {
            continue;
        }
        let (Some(sec), Some(mat)) = (model.element_section(elem), model.element_material(elem))
        else {
            continue;
        };
        let structure = structure_kind_of(Some(sec), Some(mat.category));
        if route21_inputs.is_some()
            && structure == StructureKind::Src
            && MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
            && unavailable_src_ids.insert(elem.id)
        {
            let reason = if row_count == 0 {
                "ルート2-1のSRC柱の解析断面力がありません"
            } else {
                "ルート2-1のSRC柱の解析断面力が重複し、一意の解析応答を確認できません"
            };
            member_checks.push((
                elem.id,
                0.5,
                CheckOutcome::Skipped {
                    reason: reason.into(),
                },
            ));
            continue;
        }
        if structure != StructureKind::S || !crate::steel::is_symmetric_h_section(sec) {
            continue;
        }
        if MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
            && unavailable_steel_column_ids.insert(elem.id)
        {
            let reason = if row_count == 0 {
                "鋼柱の解析断面力がありません"
            } else {
                "鋼柱の解析断面力が重複し、一意の解析応答を確認できません"
            };
            member_checks.push((
                elem.id,
                0.5,
                CheckOutcome::Skipped {
                    reason: reason.into(),
                },
            ));
            continue;
        }
        let Ok(MemberKind::Girder) = MemberKind::try_of_element(elem, model) else {
            continue;
        };
        let group = options
            .girder_group_overrides
            .and_then(|groups| groups.get(&elem.id));
        let ctx = DesignCtx {
            kind: MemberKind::Girder,
            length: group.map_or_else(|| model.member_length(elem), |group| group.length),
            node_span_mm: Some(crate::steel::frame_node_span_mm(model, elem)),
            steel_attr: model
                .steel_design_attrs
                .iter()
                .find(|attr| attr.elem == elem.id)
                .cloned(),
            ..Default::default()
        };
        let needs_span = if group.is_some() || grouped_girders.contains(&elem.id) {
            !matches!(
                grouped_beam_spans.get(&elem.id),
                Some(Ok(GroupBending::WithoutWholeSpan { length_mm: None }))
            )
        } else {
            steel_beam_span::requires_bending_span(sec, structure, &ctx)
        };
        if needs_span && unavailable_beam_ids.insert(elem.id) {
            let reason = if row_count == 0 {
                "鋼梁の解析断面力がありません"
            } else {
                "鋼梁の解析断面力が重複し、一意の解析応答を確認できません"
            };
            member_checks.push((
                elem.id,
                0.5,
                CheckOutcome::Skipped {
                    reason: reason.into(),
                },
            ));
        }
    }

    for (elem_id, mf) in member_forces {
        if unavailable_beam_ids.contains(elem_id)
            || unavailable_src_ids.contains(elem_id)
            || unavailable_steel_column_ids.contains(elem_id)
        {
            continue;
        }
        let Some(elem) = elem_by_id.get(elem_id).copied() else {
            continue;
        };
        if !frame_check_target(elem) {
            continue;
        }
        if duplicate_column_ids.contains(elem_id) {
            if checked_column_ids.insert(*elem_id) {
                member_checks.push((
                    *elem_id,
                    0.5,
                    CheckOutcome::Skipped {
                        reason: "RC柱の解析内力が重複し、一意の断面力を確認できません".into(),
                    },
                ));
            }
            continue;
        }
        let sec = elem
            .section
            .and_then(|sid| model.sections.get(sid.index()))
            .filter(|s| s.id == elem.section.unwrap());
        let mat = model.element_material(elem);
        let (Some(sec), Some(mat)) = (sec, mat) else {
            continue;
        };
        let structure = structure_kind_of(Some(sec), Some(mat.category));
        let long_term_shear = column_shear_loads
            .as_ref()
            .filter(|_| is_rc_column_target(model, elem))
            .and_then(|loads| {
                checked_column_ids.insert(*elem_id);
                loads.envelope(model, elem, mf)
            });
        let recovered_shear_detail = long_term_shear
            .as_ref()
            .and_then(|shear| shear.as_ref().ok())
            .map(crate::rc::recovered_long_shear_detail)
            .unwrap_or_default();
        let torsion_reason =
            |forces: &MemberForcesAt| concrete_member_torsion_reason(structure, forces);
        let length = model.member_length(elem);
        if let Err(reason) = sec.ensure_properties_resolved() {
            if structure == StructureKind::S
                && MemberKind::try_of_element(elem, model) == Ok(MemberKind::Column)
                && crate::steel::is_symmetric_h_section(sec)
            {
                let kind = MemberKind::Column;
                let steel_attr = model
                    .steel_design_attrs
                    .iter()
                    .find(|attr| attr.elem == *elem_id);
                let (lk_y, lk_z) =
                    crate::steel::frame_buckling_lengths(model, elem, kind, steel_attr);
                let ctx = DesignCtx {
                    kind,
                    length,
                    term: options.term,
                    lk_y: Some(lk_y),
                    lk_z: Some(lk_z),
                    node_span_mm: Some(crate::steel::frame_node_span_mm(model, elem)),
                    column_lateral_buckling_length: Some(
                        crate::steel::column_lateral_buckling_length(model, elem, steel_attr),
                    ),
                    steel_attr: steel_attr.cloned(),
                    ..Default::default()
                };
                if steel_column_span::check(
                    model,
                    elem,
                    mf,
                    &ctx,
                    beam_span_loads.as_ref(),
                    Some(&reason),
                    &mut member_checks,
                ) {
                    continue;
                }
            }
            for (pos, forces) in &mf.at {
                push_strength_outcome(
                    &mut member_checks,
                    *elem_id,
                    *pos,
                    crate::CheckOutcome::Skipped {
                        reason: format!("{reason}{recovered_shear_detail}"),
                    },
                    torsion_reason(&MemberForcesAt::from_local(*pos, *forces)),
                );
            }
            if long_term_shear.is_some() {
                for pos in missing_design_positions(&design_positions(elem, model, length), mf) {
                    member_checks.push((*elem_id, pos, CheckOutcome::Skipped {
                            reason: format!("RC柱の軸力・曲げを検定する断面位置の解析内力がありません。{reason}{recovered_shear_detail}"),
                    }));
                }
            }
            continue;
        }

        let reference = (mat.category == sepika_core::model::MaterialCategory::Steel
            && sec.frame_use != Some(sepika_core::model::FrameSectionUse::Column)
            && sec.frame_use != Some(sepika_core::model::FrameSectionUse::Brace))
        .then(|| crate::steel::lateral_bracing::resolve_lateral_bracing_reference(model, *elem_id));
        if let Some(reference) = &reference {
            lateral_bracing_references.push((*elem_id, reference.clone()));
        }
        let kind = MemberKind::of_element(elem, model);
        let rc_damage_control = structure == StructureKind::Rc
            && options.term == LoadTerm::Short
            && options.rc_damage_control;
        let rc_column_damage_control = rc_damage_control && kind == MemberKind::Column;
        let shear_span = |moment: usize, shear: usize| {
            mf.at
                .iter()
                .map(|(_, f)| (f[moment].abs(), f[shear].abs()))
                .max_by(|a, b| {
                    if rc_column_damage_control {
                        // 最大曲げを確定できない方向では、有限の試料を採用できない。
                        b.0.is_finite()
                            .cmp(&a.0.is_finite())
                            .then_with(|| a.0.total_cmp(&b.0))
                    } else {
                        a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
                    }
                })
        };
        let shear_span_y = shear_span(4, 2);
        let shear_span = shear_span(5, 1);
        let m_at = |target: f64| {
            mf.at
                .iter()
                .find(|(p, _)| (p - target).abs() < 1e-9)
                .map(|(_, f)| f[5])
        };
        let end_moments_z = match (m_at(0.0), m_at(1.0)) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        let steel_attr = model
            .steel_design_attrs
            .iter()
            .find(|a| a.elem == *elem_id)
            .cloned();
        let group = if kind == MemberKind::Girder {
            options.girder_group_overrides.and_then(|m| m.get(elem_id))
        } else {
            None
        };
        let frame_buckling = structure == StructureKind::S
            || (structure == StructureKind::Src && kind == MemberKind::Column);
        let buckling_lengths = match structure {
            StructureKind::Cft => {
                crate::cft::direct_column_buckling_lengths(*elem_id, steel_attr.as_ref()).map(Some)
            }
            _ if frame_buckling => Ok(Some(crate::steel::frame_buckling_lengths(
                model,
                elem,
                kind,
                steel_attr.as_ref(),
            ))),
            _ => Ok(None),
        };
        let buckling_lengths = match buckling_lengths {
            Ok(lengths) => lengths,
            Err(reason) => {
                let positions = design_positions(elem, model, group.map_or(length, |g| g.length));
                for (pos, forces) in &mf.at {
                    if is_near_design_position(*pos, &positions) {
                        push_strength_outcome(
                            &mut member_checks,
                            *elem_id,
                            *pos,
                            CheckOutcome::Skipped {
                                reason: reason.clone(),
                            },
                            torsion_reason(&MemberForcesAt::from_local(*pos, *forces)),
                        );
                    }
                }
                continue;
            }
        };
        let (lk_y, lk_z) = match buckling_lengths {
            Some((strong, weak)) => (Some(strong), Some(weak)),
            None => steel_attr
                .as_ref()
                .map(|attr| (attr.lk_y_direct, attr.lk_z_direct))
                .unwrap_or_default(),
        };
        let (length, shear_span, end_moments_z, mid_moment_z) = match group {
            Some(g) => (g.length, g.shear_span, g.end_moments_z, g.mid_moment_z),
            None => (length, shear_span, end_moments_z, m_at(0.5)),
        };

        let face_sum = elem.rigid_zone.face_i_or_zero() + elem.rigid_zone.face_j_or_zero();
        let clear_span = match group {
            Some(g) => g.clear_length,
            None if length - face_sum > 0.0 => length - face_sum,
            None => length,
        };

        let mut single_qd1_geometry = None;
        let rc_beam_qd1 = (!rc_damage_control
            && structure == StructureKind::Rc
            && matches!(sec.shape, Some(SectionShape::RcBeamRect { .. }))
            && options.term == LoadTerm::Short
            && options.long_member_forces.is_some()
            && options.qd_method != QdMethod::Qd2)
            .then(|| {
                if grouped_girders.contains(elem_id) {
                    return grouped_rc_qd1.get(elem_id).cloned().unwrap_or_else(|| {
                        Err("QD1用の一本部材全スパンの重力反力を確認できません".into())
                    });
                }
                let adjacency = rc_beam_adjacency.get_or_insert_with(|| {
                    sepika_core::adjacency::NodeAdjacency::primary_frame(model)
                });
                crate::rc::beam_qd1_geometry(model, adjacency, elem).and_then(|geometry| {
                    single_qd1_geometry = Some(geometry);
                    let q0_n = options
                        .q_simple_by_elem
                        .and_then(|m| m.get(elem_id).copied());
                    geometry
                        .demand_clear_span_mm()
                        .map(|span_mm| (span_mm, None, q0_n))
                })
            });
        let qd1_positions = single_qd1_geometry.and_then(|geometry| {
            crate::design_position::rc_beam_qd1_positions(model, elem, geometry)
        });
        let has_qd1_haunch = rc_beam_qd1.is_some() && crate::rc::has_beam_qd1_haunch(model, elem);
        let station_issue = |pos, rows: &[_]| {
            if has_qd1_haunch {
                crate::design_position::qd1_haunch_station_issue(pos, rows)
            } else {
                crate::design_position::qd1_saved_station_issue(pos, rows)
            }
        };
        let mf_long = long_mf_by_id
            .as_ref()
            .filter(|_| !rc_damage_control)
            .and_then(|map| map.get(elem_id).copied());
        let verified_geometry = single_qd1_geometry.filter(|_| qd1_positions.is_some());
        let response_with_faces = verified_geometry.map(|geometry| {
            with_qd1_column_faces(
                *elem_id,
                mf,
                geometry,
                options.rc_beam_qd1_face_forces,
                has_qd1_haunch,
            )
        });
        let gravity_with_faces = verified_geometry.zip(mf_long).map(|(geometry, long)| {
            with_qd1_column_faces(
                *elem_id,
                long,
                geometry,
                options.rc_beam_qd1_gravity_faces,
                has_qd1_haunch,
            )
        });
        let analysis_mf = mf;
        let mf = response_with_faces.as_ref().unwrap_or(mf);
        let mf_long = gravity_with_faces.as_ref().or(mf_long);
        let checks_grouped_rc_qd1 = grouped_girders.contains(elem_id) && rc_beam_qd1.is_some();
        let seismic_qd = (mf_long.is_some() || checks_grouped_rc_qd1).then(|| SeismicQd {
            long_at: mf_long.map_or_else(Vec::new, |mf| match &qd1_positions {
                Some(positions) => mf
                    .at
                    .iter()
                    .filter(|(pos, _)| {
                        positions.iter().any(|expected| {
                            crate::design_position::same_qd1_station(*expected, *pos)
                                && station_issue(*expected, &mf.at).is_none()
                        })
                    })
                    .copied()
                    .collect(),
                None => mf.at.clone(),
            }),
            n_factor: 1.5,
            n_mechanism: 1.0,
            q_simple: match &rc_beam_qd1 {
                Some(Ok((_, _, q0))) => *q0,
                Some(Err(_)) if grouped_girders.contains(elem_id) => None,
                _ => options
                    .q_simple_by_elem
                    .and_then(|m| m.get(elem_id).copied()),
            },
            clear_length: match &rc_beam_qd1 {
                Some(Ok((span_mm, _, _))) => *span_mm,
                Some(Err(_)) => f64::NAN,
                None => clear_span,
            },
            method: options.qd_method,
        });

        let rebar_material = model.element_rebar_material(elem);
        let wrong_main_rebar = rebar_material.is_some_and(|material| {
            material.category != sepika_core::model::MaterialCategory::Rebar
        });
        let shear_rebar_material = model.element_shear_rebar_material(elem);
        let wrong_shear_rebar = shear_rebar_material.is_some_and(|material| {
            material.category != sepika_core::model::MaterialCategory::Rebar
        });
        let (column_sum_my, column_end_axial_issue, response_end_axial_issue) = if structure
            == StructureKind::Rc
            && kind == MemberKind::Column
            && options.term == LoadTerm::Short
            && !rc_damage_control
            && options.long_member_forces.is_some()
            && options.qd_method != QdMethod::Qd2
            && !wrong_main_rebar
        {
            let axial_factor = 1.0;
            let long_at = seismic_qd
                .as_ref()
                .map_or(&[][..], |qd| qd.long_at.as_slice());
            let closest_axial = |at: &[(f64, [f64; 6])], end: f64| {
                at.iter()
                    .min_by(|a, b| (a.0 - end).abs().total_cmp(&(b.0 - end).abs()))
                    .map(|(pos, values)| (*pos, values[0]))
            };
            let end_forces = [0.0, 1.0]
                .map(|end| (end, closest_axial(long_at, end), closest_axial(&mf.at, end)));
            let response_end_axial_issue = (seismic_qd.is_some()
                && end_forces
                    .iter()
                    .any(|(_, _, response)| response.is_some_and(|(_, n)| !n.is_finite())))
            .then_some("設計用せん断力に必要な柱端の応答軸力が非有限です");
            let [(n_long_i, n_combo_i), (n_long_j, n_combo_j)] =
                end_forces.map(|(_, long, response)| {
                    (
                        long.map_or(0.0, |(_, n)| n),
                        response.map_or(0.0, |(_, n)| n),
                    )
                });
            let mut column_sum_my = crate::rc::compute_column_mechanism_sum_my(
                model,
                column_mechanism_topology
                    .get_or_insert_with(|| crate::rc::ColumnMechanismTopology::new(model)),
                elem,
                n_long_i,
                n_long_j,
                n_combo_i,
                n_combo_j,
                axial_factor,
            )
            .filter(|mechanism| {
                if seismic_qd.is_none() {
                    mechanism.excluded_girder_axes.contains(&true)
                } else {
                    response_end_axial_issue.is_none()
                        || mechanism.girder_yield_holds.iter().any(Option::is_some)
                }
            });
            let column_end_axial_issue =
                end_forces.into_iter().find_map(|(end, long, response)| {
                    let end_axial = |point: Option<(f64, f64)>| {
                        point
                            .filter(|(pos, n)| *pos == end && n.is_finite())
                            .map(|(_, n)| n)
                    };
                    match (end_axial(long), end_axial(response)) {
                        (None, _) => {
                            Some("設計用せん断力に必要な柱端の重力参照軸力が不足または非有限です")
                        }
                        (_, None) => {
                            Some("設計用せん断力に必要な柱端の応答軸力が不足または非有限です")
                        }
                        (Some(n_long), Some(n_response)) => {
                            (!crate::rc::design_axial_for_mechanism(
                                n_long,
                                n_response,
                                axial_factor,
                            )
                            .is_finite())
                            .then_some("設計用せん断力に必要な柱端の設計軸力が非有限です")
                        }
                    }
                });
            if let (Some(mechanism), Some(reason)) = (&mut column_sum_my, column_end_axial_issue) {
                for ((direction, hold), excluded_axis) in [&mut mechanism.qy, &mut mechanism.qz]
                    .into_iter()
                    .zip(&mut mechanism.girder_yield_holds)
                    .zip(mechanism.excluded_girder_axes)
                {
                    if seismic_qd.is_none() && !excluded_axis {
                        *direction = Ok(None);
                        *hold = None;
                    } else if hold.is_some() {
                        *direction = Err(combine_incomplete_reasons(hold.take(), Some(reason))
                            .expect("梁降伏候補を除外した需要を確定できない理由"));
                    }
                }
            }
            (
                column_sum_my,
                column_end_axial_issue,
                response_end_axial_issue,
            )
        } else {
            (None, None, None)
        };

        let mut rc_beam_qd1_haunches = rc_beam_qd1
            .as_ref()
            .and_then(|_| crate::rc::beam_qd1_haunches(model, elem, single_qd1_geometry));
        if let Some(haunches) = &mut rc_beam_qd1_haunches {
            let loads = haunch_span_loads.get_or_insert_with(|| {
                steel_beam_span::BeamSpanLoads::from_terms(
                    model,
                    options.rc_beam_qd1_haunch_load_terms,
                )
            });
            haunches.set_static_shear_span_reference(
                model,
                elem,
                single_qd1_geometry,
                loads
                    .as_ref()
                    .map(|loads| loads.for_member(*elem_id))
                    .map_err(|reason| *reason),
                analysis_mf,
            );
        }
        let slab_moments_nmm = (matches!(rc_beam_qd1, Some(Ok(_)))
            && rc_beam_qd1_haunches.is_none()
            && !grouped_girders.contains(elem_id))
        .then(|| crate::rc::beam_qd1_slab_end_moments(model, rc_beam_adjacency.as_ref()?, elem))
        .flatten();
        let mut ctx = DesignCtx {
            rebar_material: rebar_material.cloned(),
            shear_rebar_material: shear_rebar_material.cloned(),
            steel_material: model.element_steel_material(elem).cloned(),
            term: options.term,
            kind,
            length,
            node_span_mm: (structure == StructureKind::S)
                .then(|| crate::steel::frame_node_span_mm(model, elem)),
            column_lateral_buckling_length: (structure == StructureKind::S
                && kind == MemberKind::Column
                && crate::steel::is_symmetric_h_section(sec))
            .then(|| {
                crate::steel::column_lateral_buckling_length(model, elem, steel_attr.as_ref())
            }),
            clear_length: Some(clear_span),
            lb: None,
            lk_y,
            lk_z,
            shear_span,
            shear_span_y,
            rc_damage_control: options.rc_damage_control,
            bond_method: options.bond_method,
            end_moments_z,
            mid_moment_z,
            steel_beam_moment_diagram: crate::steel::SteelBeamMomentDiagram::Unverified,
            seismic_qd,
            src_column_seismic_bending: None,
            src_column_route21_shear: route21_inputs
                .filter(|_| structure == StructureKind::Src && kind == MemberKind::Column)
                .map(|inputs| {
                    inputs
                        .get(elem_id)
                        .cloned()
                        .ok_or("ルート2-1の確認済み入力が対象SRC柱にありません")
                }),
            column_sum_my,
            beam_has_slab: kind == MemberKind::Girder && beam_has_attached_slab(model, elem),
            rc_grouped_beam_qd1_sum_my_nmm: rc_beam_qd1
                .as_ref()
                .and_then(|value| value.as_ref().ok())
                .and_then(|(_, sum_my, _)| *sum_my),
            rc_beam_qd1_slab_end_moments_nmm: slab_moments_nmm,
            rc_beam_qd1_at_material_end: qd1_positions.is_some(),
            rc_beam_qd1_slab_unverified: slab_moments_nmm.is_none()
                && matches!(rc_beam_qd1, Some(Ok(_)))
                && rc_beam_qd1_haunches.is_none()
                && !grouped_girders.contains(elem_id),
            rc_beam_qd1_issue: rc_beam_qd1.and_then(Result::err),
            rc_beam_qd1_haunches,
            steel_attr,
        };
        if ctx.column_lateral_buckling_length.is_some()
            && steel_column_span::check(
                model,
                elem,
                mf,
                &ctx,
                beam_span_loads.as_ref(),
                None,
                &mut member_checks,
            )
        {
            continue;
        }

        let grouped_steel_beam = structure == StructureKind::S
            && kind == MemberKind::Girder
            && crate::steel::is_symmetric_h_section(sec)
            && (group.is_some() || grouped_girders.contains(elem_id));
        let grouped_beam_span = grouped_steel_beam.then(|| {
            grouped_beam_spans
                .remove(elem_id)
                .unwrap_or_else(|| Err("一本部材の構成要素が未確認です".into()))
        });
        let grouped_partial = matches!(
            grouped_beam_span,
            Some(Ok(GroupBending::WithoutWholeSpan { length_mm: Some(_) }))
        );
        let needs_beam_span = match &grouped_beam_span {
            Some(Ok(GroupBending::WithoutWholeSpan { length_mm: None })) => false,
            Some(Ok(GroupBending::WithoutWholeSpan { length_mm: Some(_) })) => true,
            Some(_) => true,
            None => steel_beam_span::requires_bending_span(sec, structure, &ctx),
        };
        if let Some(Ok(
            GroupBending::WithoutWholeSpan {
                length_mm: Some(length_mm),
            }
            | GroupBending::Whole { length_mm, .. },
        )) = &grouped_beam_span
        {
            ctx.length = *length_mm;
        }
        if let Some(Ok(GroupBending::Whole { length_mm, .. })) = &grouped_beam_span {
            ctx.lb = Some(*length_mm);
        }
        let station_key = |xi: f64| if xi == 0.0 { 0 } else { xi.to_bits() };
        let repeated_stations: HashSet<_> = if needs_beam_span {
            let mut observed = HashSet::new();
            mf.at
                .iter()
                .map(|(pos, _)| station_key(*pos))
                .filter(|key| !observed.insert(*key))
                .collect()
        } else {
            HashSet::new()
        };
        let beam_span = needs_beam_span.then(|| {
            if !repeated_stations.is_empty() {
                return Err(
                    "同一の評価位置で解析断面力が重複し、強軸曲げ分布を確認できません".into(),
                );
            }
            if let Some(grouped) = grouped_beam_span {
                return match grouped? {
                    GroupBending::Whole { span, .. } => Ok(RecoveredBending::Single(span)),
                    GroupBending::WithoutWholeSpan { .. } => {
                        Err("一本部材に指定した横座屈長さから補剛端の位置を確認できません".into())
                    }
                };
            }
            beam_span_loads
                .as_ref()
                .map_err(|reason| (*reason).to_owned())
                .and_then(|loads| loads.bending_span(model, elem, mf, &ctx))
        });
        let middle_bracing = steel_beam_span::has_middle_bracing(&ctx);
        let mut beam_peaks = Vec::new();
        let mut intervals = &[][..];
        match &beam_span {
            Some(Ok(RecoveredBending::Single(span))) => {
                ctx.steel_beam_moment_diagram = span.diagram;
                beam_peaks.extend(
                    span.demand_peaks
                        .iter()
                        .filter(|peak| peak.left[5] != 0.0)
                        .map(|peak| (peak, None)),
                );
            }
            Some(Ok(RecoveredBending::Multiple(spans))) => {
                intervals = spans;
                beam_peaks.extend(spans.iter().flat_map(|interval| {
                    interval
                        .span
                        .demand_peaks
                        .iter()
                        .map(move |peak| (peak, Some(interval)))
                }));
            }
            Some(Err(_)) if grouped_steel_beam && !grouped_partial => {
                ctx.steel_beam_moment_diagram =
                    crate::steel::SteelBeamMomentDiagram::UnverifiedGroup;
            }
            Some(Err(_)) if middle_bracing || grouped_partial => {
                ctx.steel_beam_moment_diagram =
                    crate::steel::SteelBeamMomentDiagram::UnverifiedBracing;
            }
            _ => {}
        }
        let checker: Box<dyn DesignCheck> = checker_for(structure);

        let position_length = if grouped_steel_beam || checks_grouped_rc_qd1 {
            model.member_length(elem)
        } else {
            length
        };
        let positions = qd1_positions
            .clone()
            .unwrap_or_else(|| design_positions(elem, model, position_length));
        if let Some(positions) = &qd1_positions {
            for &pos in positions {
                if let Some(reason) = station_issue(pos, &mf.at) {
                    member_checks.push((
                        *elem_id,
                        pos,
                        CheckOutcome::Skipped {
                            reason: ctx
                                .rc_beam_qd1_haunches
                                .as_ref()
                                .and_then(|haunches| haunches.section_issue(pos))
                                .unwrap_or_else(|| reason.into()),
                        },
                    ));
                }
            }
        }
        let route21_matches = ctx.src_column_route21_shear.as_ref().map(|_| {
            let mut counts = vec![0; positions.len()];
            let stations: Vec<_> = mf
                .at
                .iter()
                .map(|(pos, _)| {
                    let nearest = positions
                        .iter()
                        .enumerate()
                        .filter(|(_, target)| {
                            is_near_design_position(*pos, std::slice::from_ref(*target))
                        })
                        .min_by(|(_, a), (_, b)| (*pos - *a).abs().total_cmp(&(*pos - *b).abs()))
                        .map(|(i, _)| i);
                    if let Some(i) = nearest {
                        counts[i] += 1;
                    }
                    nearest
                })
                .collect();
            (stations, counts)
        });
        let missing_positions = if long_term_shear.is_some() {
            missing_design_positions(&positions, mf)
        } else {
            Vec::new()
        };
        let long_shear_check = long_term_shear
            .as_ref()
            .map(|envelope| crate::RcDesign::long_column_shear_check(sec, mat, &ctx, envelope));
        let confirmed_long_shear = long_shear_check
            .as_ref()
            .and_then(|check| check.as_ref().ok());
        let mut has_long_shear_row = false;

        let first_row = member_checks.len();
        let mut peak_sections = Vec::new();
        for &(peak, interval) in &beam_peaks {
            if !matches!(peak.right, Ok(None)) {
                if !peak.is_i_face {
                    peak_sections.push((peak.xi, peak.left, Some("集中荷重の左側"), interval));
                }
                match &peak.right {
                    Ok(Some(right)) => {
                        peak_sections.push((peak.xi, *right, Some("集中荷重の右側"), interval))
                    }
                    Ok(None) => unreachable!(),
                    Err(reason) => member_checks.push((
                        *elem_id,
                        peak.xi,
                        CheckOutcome::Skipped {
                            reason: format!("集中荷重の右側の組合せは未算定です。{reason}"),
                        },
                    )),
                }
            } else {
                peak_sections.push((peak.xi, peak.left, None, interval));
            }
        }
        // 既定のjフェースのξと、物理座標から求めるξは演算順序の違いで一致しない場合がある。
        let sampled_j_face = elem
            .rigid_zone
            .face_j
            .map(|face| (1.0 - face / position_length).clamp(0.5 + 1e-9, 1.0));
        let section_forces = mf
            .at
            .iter()
            .enumerate()
            .filter(|(row, (pos, _))| {
                let selected_position = match &route21_matches {
                    Some((stations, counts)) => stations[*row].is_some_and(|i| counts[i] == 1),
                    None if qd1_positions.is_some() => positions.iter().any(|expected| {
                        crate::design_position::same_qd1_station(*expected, *pos)
                            && station_issue(*expected, &mf.at).is_none()
                    }),
                    None => is_near_design_position(*pos, &positions),
                };
                selected_position
                    && !repeated_stations.contains(&station_key(*pos))
                    && !beam_peaks.iter().any(|(peak, _)| {
                        peak.xi == *pos
                            || (peak.is_j_face
                                && sampled_j_face == Some(*pos)
                                && (peak.xi - pos).abs() <= f64::EPSILON)
                    })
            })
            .map(|(_, &(pos, forces))| {
                let interval = if intervals.is_empty() {
                    None
                } else {
                    let index = intervals.partition_point(|interval| interval.bounds_xi[1] < pos);
                    intervals.get(index.min(intervals.len() - 1))
                };
                (pos, forces, None, interval)
            })
            .chain(peak_sections);
        for (pos, forces, cut_side, interval) in section_forces {
            if let Some(interval) = interval {
                ctx.lb = Some(interval.length_mm);
                ctx.steel_beam_moment_diagram =
                    if interval.span.diagram == crate::steel::SteelBeamMomentDiagram::Unverified {
                        crate::steel::SteelBeamMomentDiagram::UnverifiedBracing
                    } else {
                        interval.span.diagram
                    };
            }
            let mfa = MemberForcesAt::from_local(pos, forces);
            let missing_gravity = !rc_damage_control
                && options.long_member_forces.is_some()
                && ctx.seismic_qd.is_none()
                && (options.term == LoadTerm::Short
                    && matches!(sec.shape, Some(SectionShape::RcBeamRect { .. }))
                    && structure == StructureKind::Rc
                    || matches!(
                        (&sec.shape, kind),
                        (Some(SectionShape::SrcColumnRect { .. }), MemberKind::Column)
                            | (Some(SectionShape::SrcBeamRect { .. }), MemberKind::Girder)
                    ) && sepika_core::structure_kind::structure_kind_of(
                        Some(sec),
                        Some(mat.category),
                    ) == sepika_core::structure_kind::StructureKind::Src);
            let eccentric_brace_compression = structure != StructureKind::S
                && kind == MemberKind::Brace
                && mfa.n < 0.0
                && elem.end_offsets_mm != [[0.0; 3]; 2]
                && !ctx
                    .steel_attr
                    .as_ref()
                    .is_some_and(|a| a.lk_y_direct.is_some() && a.lk_z_direct.is_some());
            let mut check_incomplete = None;
            let mut outcome = if eccentric_brace_compression {
                CheckOutcome::Skipped { reason: "非0XYZ偏心ブレースの圧縮検定には強軸・弱軸の座屈長さの直接指定が必要です。実材軸長・剛体接続長からは自動算定しません".into() }
            } else if structure == StructureKind::Src {
                let (outcome, incomplete) =
                    crate::SrcDesign.check_with_incomplete(&mfa, sec, mat, &ctx, missing_gravity);
                check_incomplete = incomplete;
                outcome
            } else if structure == StructureKind::Rc {
                let column_without_gravity = !rc_damage_control
                    && kind == MemberKind::Column
                    && options.long_member_forces.is_some()
                    && ctx.seismic_qd.is_none();
                let missing_column_reference =
                    (wrong_main_rebar || wrong_shear_rebar) && column_without_gravity;
                let missing_shear_reference = if missing_gravity || missing_column_reference {
                    Some("設計用せん断力に必要な重力参照内力が不足しています")
                } else {
                    None
                };
                let boundary_shear_issue =
                    if options.term == LoadTerm::Short && column_without_gravity {
                        Some("設計用せん断力に必要な重力参照内力が不足しています")
                    } else {
                        column_end_axial_issue
                    };
                let (outcome, incomplete) = crate::RcDesign.check_ordinary_with_incomplete(
                    &mfa,
                    sec,
                    mat,
                    &ctx,
                    missing_shear_reference,
                    crate::rc::ColumnGravityMode::Ordinary {
                        boundary_shear_issue,
                        response_end_axial_issue,
                        saved_long_term: long_term_shear.is_some(),
                    },
                );
                check_incomplete = incomplete;
                outcome
            } else if structure == StructureKind::S {
                let (outcome, incomplete) =
                    crate::SteelDesign.check_with_incomplete(&mfa, sec, mat, &ctx);
                check_incomplete = incomplete;
                outcome
            } else {
                checker.check(&mfa, sec, mat, &ctx)
            };
            if let (Some(reference), CheckOutcome::Checked(result)) = (&reference, &mut outcome) {
                let unconfirmed = crate::steel::lateral_bracing::UNCONFIRMED_DETAIL;
                if result.detail.starts_with(unconfirmed) {
                    result
                        .detail
                        .replace_range(..unconfirmed.len(), &reference.detail());
                } else {
                    result.detail.push_str(&format!(", {}", reference.detail()));
                }
            }
            if frame_buckling {
                if let (Some((strong, weak)), CheckOutcome::Checked(result)) = (
                    lk_y.zip(lk_z)
                        .filter(|(y, z)| [*y, *z].into_iter().all(|v| v.is_finite() && v > 0.0)),
                    &mut outcome,
                ) {
                    if !result.detail.is_empty() {
                        result.detail.push_str(", ");
                    }
                    result
                        .detail
                        .push_str(&format!("採用座屈長さ [mm]　強軸 {strong} ／ 弱軸 {weak}"));
                }
            }
            if wrong_shear_rebar && matches!(structure, StructureKind::Rc | StructureKind::Src) {
                check_incomplete =
                    check_incomplete.map(|reason| format!("部材 ID {}: {reason}", elem.id.0));
            }
            if let Some(check) = confirmed_long_shear {
                check_incomplete =
                    combine_incomplete_reasons(check_incomplete, check.incomplete.as_deref());
                if let (Some((_, shear)), CheckOutcome::Checked(result)) =
                    (&check.exact, &mut outcome)
                {
                    result.components.push(shear.clone());
                    has_long_shear_row = true;
                }
            } else if let (Some(Err(issue)), CheckOutcome::Checked(_)) =
                (&long_shear_check, &outcome)
            {
                let reason = format!("RC柱の長期せん断は未算定です（{issue}）");
                check_incomplete = combine_incomplete_reasons(check_incomplete, Some(&reason));
            }
            let mut incomplete = combine_incomplete_reasons(check_incomplete, torsion_reason(&mfa));
            if structure == StructureKind::Src {
                crate::srrc::preserve_route21_shear_detail(sec, &ctx, &outcome, &mut incomplete);
            }
            if long_term_shear.is_some() {
                for (value, reason) in [
                    (mfa.n, "軸力Nが非有限のため、軸力・曲げを検定できません"),
                    (
                        mfa.my,
                        "曲げMyが非有限のため、当該方向の曲げを検定できません",
                    ),
                    (
                        mfa.mz,
                        "曲げMzが非有限のため、当該方向の曲げを検定できません",
                    ),
                ] {
                    if !value.is_finite() {
                        incomplete = combine_incomplete_reasons(incomplete, Some(reason));
                    }
                }
                match (&mut incomplete, &mut outcome) {
                    (_, CheckOutcome::Skipped { reason }) => {
                        reason.push_str(&recovered_shear_detail)
                    }
                    (Some(reason), CheckOutcome::Checked(result)) => {
                        match result
                            .components
                            .iter()
                            .find(|component| component.kind == crate::CheckKind::Shear)
                        {
                            Some(shear) if shear.ratio <= 1.0 => {
                                reason.push_str(&format!("。せん断の確認結果（{}）", shear.detail))
                            }
                            None => reason.push_str(&recovered_shear_detail),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            if let Some(interval) = interval {
                let [lo, hi] = interval.bounds_mm;
                let label = format!("補剛区間 [{lo:.3}, {hi:.3}] mm");
                match &mut outcome {
                    CheckOutcome::Checked(result) => result.detail.push_str(&format!(", {label}")),
                    CheckOutcome::Skipped { reason } => {
                        reason.insert_str(0, &format!("{label}。"));
                    }
                }
                if let Some(reason) = &mut incomplete {
                    reason.insert_str(0, &format!("{label}。"));
                }
            }
            if let Some(side) = cut_side {
                match &mut outcome {
                    CheckOutcome::Checked(result) => result.detail.push_str(&format!(", {side}")),
                    CheckOutcome::Skipped { reason } => reason.insert_str(0, &format!("{side}。")),
                }
                if let Some(reason) = &mut incomplete {
                    reason.insert_str(0, &format!("{side}。"));
                }
            }
            push_strength_outcome(
                &mut member_checks,
                *elem_id,
                pos,
                outcome,
                incomplete.as_deref(),
            );
        }

        if let Some(span) = beam_span {
            let issue = match span {
                Err(reason) => Some(reason),
                Ok(RecoveredBending::Single(span)) if span.diagram == crate::steel::SteelBeamMomentDiagram::Unverified => {
                    Some(
                        "符号付き強軸曲げの単調性または区間内部の絶対値最大を確認できません".into(),
                    )
                }
                Ok(RecoveredBending::Multiple(spans)) => spans.iter().find_map(|interval| {
                    (interval.span.diagram == crate::steel::SteelBeamMomentDiagram::Unverified)
                        .then_some("符号付き強軸曲げの単調性または区間内部の絶対値最大を確認できない補剛区間があります".into())
                }),
                Ok(_) => None,
            };
            if let Some(reason) = issue {
                member_checks.push((
                    *elem_id,
                    0.5,
                    CheckOutcome::Skipped {
                        reason: if middle_bracing || grouped_partial {
                            format!("各補剛区間の強軸曲げ・横座屈は未算定です。{reason}")
                        } else {
                            format!("単一補剛区間の強軸曲げ・横座屈は未算定です。{reason}")
                        },
                    },
                ));
            }
        }
        if let Some(check) = confirmed_long_shear {
            if let Some((xi, result)) = &check.bound_failure {
                member_checks.push((*elem_id, *xi, CheckOutcome::Checked(result.clone())));
            }
            if !has_long_shear_row {
                if let Some((xi, shear)) =
                    check.exact.as_ref().filter(|(_, shear)| shear.ratio > 1.0)
                {
                    let result = crate::rc::shear_check_result("RC柱の長期せん断", shear.clone());
                    member_checks.push((*elem_id, *xi, CheckOutcome::Checked(result)));
                }
            }
        }
        if let Some((_, counts)) = route21_matches {
            for (pos, count) in positions.iter().copied().zip(counts) {
                let reason = match count {
                    0 => "ルート2-1のSRC柱の検定位置の解析断面力がありません",
                    1 => continue,
                    _ => {
                        "ルート2-1のSRC柱の検定位置で解析断面力が重複し、一意の応答を確認できません"
                    }
                };
                member_checks.push((
                    *elem_id,
                    pos,
                    CheckOutcome::Skipped {
                        reason: reason.into(),
                    },
                ));
            }
        }
        if !missing_positions.is_empty() {
            let mut reason = "RC柱の軸力・曲げを検定する断面位置の解析内力がありません".to_owned();
            if let Some(issue) = long_shear_check.as_ref().and_then(|check| match check {
                Ok(check) => check.incomplete.as_deref(),
                Err(issue) => Some(issue.as_str()),
            }) {
                reason.push_str(&format!("。{issue}"));
            }
            if let Some((_, shear)) = confirmed_long_shear.and_then(|check| check.exact.as_ref()) {
                reason.push_str(&format!("。せん断の確認結果（{}）", shear.detail));
            } else {
                reason.push_str(&recovered_shear_detail);
            }
            for pos in missing_positions {
                member_checks.push((
                    *elem_id,
                    pos,
                    CheckOutcome::Skipped {
                        reason: reason.clone(),
                    },
                ));
            }
        }

        let rc_girder = structure == StructureKind::Rc
            && matches!(sec.shape, Some(SectionShape::RcBeamRect { .. }));
        if options.term != LoadTerm::Long
            || kind != MemberKind::Girder
            || !(structure == StructureKind::S || rc_girder)
        {
            continue;
        }
        let deflection = if group.is_some() || grouped_girders.contains(elem_id) {
            Err("一本部材のたわみは対象外です".into())
        } else {
            girder_deflection::maximum(model, elem, sec, mat, mf, options.load_terms)
        };
        if structure == StructureKind::S {
            let detail = match deflection {
                Ok(value) => value.steel_detail(),
                Err(reason) => format!("たわみ参考 未算定（{reason}）"),
            };
            let nearest = member_checks[first_row..]
                .iter_mut()
                .min_by(|(_, a, _), (_, b, _)| (a - 0.5).abs().total_cmp(&(b - 0.5).abs()))
                .map(|(_, _, outcome)| outcome);
            match nearest {
                Some(CheckOutcome::Checked(result)) => {
                    result.detail.push_str(&format!(", {detail}"))
                }
                Some(CheckOutcome::Skipped { reason }) => reason.push_str(&format!("。{detail}")),
                None => {}
            }
            steel_deflection_references.push((*elem_id, detail));
        } else {
            let component = deflection.and_then(|value| {
                value
                    .rc_component()
                    .ok_or_else(|| "検定比を有限な数値で算定できません".into())
            });
            let center_rows = &mut member_checks[first_row..];
            let unchecked_center = center_rows.iter().position(|(_, pos, outcome)| {
                *pos == 0.5 && matches!(outcome, CheckOutcome::Skipped { .. })
            });
            let center = if let Some(index) = unchecked_center.filter(|_| component.is_err()) {
                Some(&mut center_rows[index])
            } else {
                center_rows.iter_mut().find(|(_, pos, _)| *pos == 0.5)
            }
            .map(|(_, _, outcome)| outcome);
            match (component, center) {
                (Ok(component), _) if wrong_shear_rebar && component.ratio <= 1.0 => {}
                (Ok(component), Some(CheckOutcome::Checked(result))) => {
                    result.components.push(component)
                }
                (Ok(component), _) => standalone_deflection_checks.push((
                    *elem_id,
                    0.5,
                    CheckOutcome::Checked(CheckResult {
                        basis: "RC大梁の長期たわみ".into(),
                        detail: String::new(),
                        components: vec![component],
                    }),
                )),
                (
                    Err(reason),
                    Some(CheckOutcome::Skipped {
                        reason: strength_reason,
                    }),
                ) => {
                    strength_reason.push_str(&format!("。長期たわみ未算定（{reason}）"));
                }
                (Err(reason), _) => standalone_deflection_checks.push((
                    *elem_id,
                    0.5,
                    CheckOutcome::Skipped {
                        reason: format!("長期たわみ未算定（{reason}）"),
                    },
                )),
            }
        }
    }

    if requires_column_envelope {
        for elem in &model.elements {
            if !checked_column_ids.contains(&elem.id) && is_rc_column_target(model, elem) {
                for pos in design_positions(elem, model, model.member_length(elem)) {
                    member_checks.push((
                        elem.id,
                        pos,
                        CheckOutcome::Skipped {
                            reason:
                                "RC柱の長期せん断は未算定です（対象部材の解析内力がありません）"
                                    .into(),
                        },
                    ));
                }
            }
        }
    }

    let mf_slices: Vec<(ElemId, crate::joint_wiring::ForcesAt)> = member_forces
        .iter()
        .filter(|(id, _)| !duplicate_column_ids.contains(id) && !unavailable_beam_ids.contains(id))
        .map(|(id, mf)| (*id, mf.at.as_slice()))
        .collect();
    // 同じ位置の既存行を先に置き、たわみ算定可否で比較時の順序を変えない。
    member_checks.extend(standalone_deflection_checks);

    let joint_checks = crate::joint_wiring::collect_joint_checks_impl(
        model,
        &mf_slices,
        panel_moments,
        crate::joint_wiring::GirderFaceForces::Analysis(options.girder_column_face_forces),
        Some(options.steel_panel_column_face_forces),
        options.term,
        false,
    );

    let gravity_wall = options.src_wall_gravity_forces.map(|forces| {
        forces
            .iter()
            .map(|(id, mf)| (*id, mf.at.as_slice()))
            .collect::<Vec<_>>()
    });
    let wall_checks = crate::joint_wiring::collect_wall_design_checks_for_case(
        model,
        &mf_slices,
        crate::joint_wiring::WallDesignInputs {
            term: options.term,
            index: options.wall_index,
            case: options.wall_case,
            load_terms: options.load_terms,
            gravity_forces: gravity_wall.as_deref(),
            src_holding_reviewed: options.src_holding_reviewed,
        },
    );
    let rc_column_ductility_advisories = if options.term == LoadTerm::Short {
        rc_column_ductility::collect(
            model,
            member_forces,
            options.rc_column_ductility_applicability,
        )
    } else {
        Vec::new()
    };
    Ok(MemberDesignCheckReport {
        member_checks,
        rc_column_ductility_advisories,
        lateral_bracing_references,
        steel_deflection_references,
        joint_checks,
        wall_checks,
    })
}
