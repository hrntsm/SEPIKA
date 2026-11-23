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

pub(crate) mod rc_column_long_term_shear;
use rc_column_long_term_shear::{missing_design_positions, ColumnShearLoads};

use crate::design_position::{design_positions, is_near_design_position};
use crate::{
    beam_has_attached_slab, checker_for, girder_deflection, BondMethod, CheckOutcome, CheckResult,
    DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind, QdMethod, SeismicQd,
};

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
    /// 通常の長期RC柱検定に渡す線形静的解析結果の由来。
    pub rc_column_long_term_shear: RcColumnLongTermShearInput<'a>,
    /// RC 短期許容せん断力で損傷制御式（2/3·α）を使うか。
    pub rc_damage_control: bool,
    /// RC 梁付着検定の方式（既定は 1999）。
    pub bond_method: BondMethod,
    /// 地震時短期 QD の決定方法（`long_member_forces` が与えられた場合のみ有効）。
    pub qd_method: QdMethod,
    /// 地震時短期 QD 用の長期（DL+LL 等）内力。None なら QD 割増なし。
    pub long_member_forces: Option<&'a [(ElemId, MemberForces)]>,
    /// RC 梁 QD1 用の単純梁せん断 Q0 `[N]`（部材 ID → 絶対値）。RC梁のQD1で不足すれば未検定。
    pub q_simple_by_elem: Option<&'a HashMap<ElemId, f64>>,
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
        let skipped = || CheckOutcome::Skipped {
            reason: reason.into(),
        };
        for (_, detail) in &mut self.steel_deflection_references {
            *detail = format!("たわみ参考 未算定（{reason}）");
        }
        for (_, _, outcome) in &mut self.member_checks {
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

    let mut column_mechanism_topology = None;
    let mut member_checks: Vec<(ElemId, f64, CheckOutcome)> = Vec::new();
    let mut lateral_bracing_references = Vec::new();
    let mut steel_deflection_references = Vec::new();
    let mut standalone_deflection_checks = Vec::new();
    let grouped_girders: HashSet<_> = model.girder_groups.iter().flatten().copied().collect();
    let column_shear_loads = (options.term == LoadTerm::Long
        && model
            .elements
            .iter()
            .any(|elem| rc_column_long_term_shear::is_target(model, elem)))
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
                    .is_some_and(|elem| rc_column_long_term_shear::is_target(model, elem))
                && !seen_column_ids.insert(*id)
        })
        .collect();

    for (elem_id, mf) in member_forces {
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
            .filter(|_| rc_column_long_term_shear::is_target(model, elem))
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
        let rc_column_damage_control = structure == StructureKind::Rc
            && kind == MemberKind::Column
            && options.term == LoadTerm::Short
            && options.rc_damage_control;
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

        let seismic_qd = long_mf_by_id
            .as_ref()
            .and_then(|map| map.get(elem_id))
            .map(|mf_long| SeismicQd {
                long_at: mf_long.at.clone(),
                n_factor: 1.5,
                n_mechanism: 1.0,
                q_simple: options
                    .q_simple_by_elem
                    .and_then(|m| m.get(elem_id).copied()),
                clear_length: clear_span,
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
            && seismic_qd
                .as_ref()
                .is_some_and(|qd| qd.method != QdMethod::Qd2)
            && !wrong_main_rebar
        {
            let axial_factor = 1.0;
            let qd = seismic_qd
                .as_ref()
                .expect("柱のQD1に使う重力参照を設定済み");
            let closest_axial = |at: &[(f64, [f64; 6])], end: f64| {
                at.iter()
                    .min_by(|a, b| (a.0 - end).abs().total_cmp(&(b.0 - end).abs()))
                    .map(|(pos, values)| (*pos, values[0]))
            };
            let end_forces = [0.0, 1.0].map(|end| {
                (
                    end,
                    closest_axial(&qd.long_at, end),
                    closest_axial(&mf.at, end),
                )
            });
            let response_end_axial_issue = end_forces
                .iter()
                .any(|(_, _, response)| response.is_some_and(|(_, n)| !n.is_finite()))
                .then_some("設計用せん断力に必要な柱端の応答軸力が非有限です");
            let [(n_long_i, n_combo_i), (n_long_j, n_combo_j)] =
                end_forces.map(|(_, long, response)| {
                    (
                        long.map_or(0.0, |(_, n)| n),
                        response.map_or(0.0, |(_, n)| n),
                    )
                });
            let column_sum_my = if response_end_axial_issue.is_some() {
                None
            } else {
                crate::rc::compute_column_mechanism_sum_my(
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
            };
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
            (
                column_sum_my,
                column_end_axial_issue,
                response_end_axial_issue,
            )
        } else {
            (None, None, None)
        };

        let ctx = DesignCtx {
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
            seismic_qd,
            src_column_seismic_bending: None,
            column_sum_my,
            beam_has_slab: kind == MemberKind::Girder && beam_has_attached_slab(model, elem),
            steel_attr,
        };

        let checker: Box<dyn DesignCheck> = checker_for(structure);

        let positions = design_positions(elem, model, length);
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
        for (pos, forces) in &mf.at {
            if !is_near_design_position(*pos, &positions) {
                continue;
            }
            let mfa = MemberForcesAt::from_local(*pos, *forces);
            let missing_q0 = kind == MemberKind::Girder
                && options.long_member_forces.is_some()
                && options.qd_method != QdMethod::Qd2
                && matches!(
                    sepika_core::structure_kind::structure_kind_of(Some(sec), Some(mat.category)),
                    sepika_core::structure_kind::StructureKind::Rc
                )
                && options
                    .q_simple_by_elem
                    .and_then(|q| q.get(elem_id))
                    .is_none();
            let missing_gravity = options.long_member_forces.is_some()
                && ctx.seismic_qd.is_none()
                && (matches!(
                    sec.shape,
                    Some(sepika_core::section_shape::SectionShape::RcBeamRect { .. })
                ) && sepika_core::structure_kind::structure_kind_of(
                    Some(sec),
                    Some(mat.category),
                ) == sepika_core::structure_kind::StructureKind::Rc
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
                let column_without_gravity = kind == MemberKind::Column
                    && options.long_member_forces.is_some()
                    && ctx.seismic_qd.is_none();
                let missing_column_reference =
                    (wrong_main_rebar || wrong_shear_rebar) && column_without_gravity;
                let missing_shear_reference = if missing_gravity || missing_column_reference {
                    Some("設計用せん断力に必要な重力参照内力が不足しています")
                } else if missing_q0 {
                    Some("QD1用の部材荷重由来Q0が不足しています。FEMのQLでは代替しません")
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
            push_strength_outcome(
                &mut member_checks,
                *elem_id,
                *pos,
                outcome,
                incomplete.as_deref(),
            );
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
            if !checked_column_ids.contains(&elem.id)
                && rc_column_long_term_shear::is_target(model, elem)
            {
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
        .filter(|(id, _)| !duplicate_column_ids.contains(id))
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
    Ok(MemberDesignCheckReport {
        member_checks,
        lateral_bracing_references,
        steel_deflection_references,
        joint_checks,
        wall_checks,
    })
}
