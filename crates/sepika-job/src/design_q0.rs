//! 断面検定 QD1 用の単純梁せん断 Q0 と、重力ケース選択（GUI・MCP 共用）。

use std::collections::HashMap;

use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{LoadCaseKind, MemberLoadKind, Model, LL_FRAME_CASE_NAME};
use sepika_element::frame::beam::MemberForces;
use sepika_load::self_weight::SELF_WEIGHT_AUTO_LOAD_CASE_NAME;

/// 地震用重量に使う重力ケース ID 列。検定用QL・QD1用Q0とは区別する。
/// 自重専用ケースと骨組用積載ケースは除外し、種別未設定なら先頭ケースのみを用いる。
pub fn gravity_case_ids_for_seismic_weight(model: &Model) -> Vec<LoadCaseId> {
    let any_kind_set = model
        .load_cases
        .iter()
        .any(|lc| lc.kind != LoadCaseKind::Other);
    if !any_kind_set {
        return model.load_cases.first().map(|c| c.id).into_iter().collect();
    }

    let mut result: Vec<LoadCaseId> = model
        .load_cases
        .iter()
        .filter(|lc| lc.kind == LoadCaseKind::Dead && lc.name != SELF_WEIGHT_AUTO_LOAD_CASE_NAME)
        .map(|lc| lc.id)
        .collect();

    let live_seismic: Vec<LoadCaseId> = model
        .load_cases
        .iter()
        .filter(|lc| lc.kind == LoadCaseKind::LiveSeismic)
        .map(|lc| lc.id)
        .collect();
    if !live_seismic.is_empty() {
        result.extend(live_seismic);
    } else {
        result.extend(
            model
                .load_cases
                .iter()
                .filter(|lc| lc.kind == LoadCaseKind::Live && lc.name != LL_FRAME_CASE_NAME)
                .map(|lc| lc.id),
        );
    }

    result
}

/// 断面検定用G+Pのケース。固定と架構用積載を種別で選び、地震重量用を除く。
pub fn gravity_case_ids_for_design(model: &Model) -> Vec<LoadCaseId> {
    model
        .load_cases
        .iter()
        .filter(|c| matches!(c.kind, LoadCaseKind::Dead | LoadCaseKind::Live))
        .map(|c| c.id)
        .collect()
}

/// 全指定ケースの解析済み内力を加算する。1件でも欠落すれば不足ケースを返す。
pub fn complete_gravity_member_forces<F>(
    ids: &[LoadCaseId],
    mut force_of: F,
) -> Result<Vec<(ElemId, MemberForces)>, Vec<LoadCaseId>>
where
    F: FnMut(LoadCaseId) -> Option<Vec<(ElemId, MemberForces)>>,
{
    let mut lists = Vec::new();
    let mut missing = Vec::new();
    for id in ids {
        match force_of(*id) {
            Some(forces) => lists.push(forces),
            None => missing.push(*id),
        }
    }
    if ids.is_empty() || !missing.is_empty() {
        return Err(missing);
    }
    let complete = |list: &Vec<(ElemId, MemberForces)>| {
        !list.is_empty()
            && list.iter().all(|(_, mf)| {
                !mf.at.is_empty()
                    && mf
                        .at
                        .iter()
                        .all(|(p, f)| p.is_finite() && f.iter().all(|v| v.is_finite()))
            })
    };
    let signature_matches = |list: &Vec<(ElemId, MemberForces)>| {
        list.len() == lists[0].len()
            && lists[0].iter().all(|(id, mf)| {
                list.iter()
                    .find(|(candidate, _)| candidate == id)
                    .is_some_and(|(_, other)| {
                        mf.at.len() == other.at.len()
                            && mf
                                .at
                                .iter()
                                .all(|(p, _)| other.at.iter().any(|(q, _)| (p - q).abs() <= 1e-9))
                    })
            })
    };
    let incomplete: Vec<_> = lists
        .iter()
        .zip(ids)
        .filter_map(|(list, id)| (!complete(list) || !signature_matches(list)).then_some(*id))
        .collect();
    if incomplete.is_empty() {
        Ok(sum_member_forces_lists(&lists))
    } else {
        Err(incomplete)
    }
}

/// 選択組合せのG/P/S参照terms。地震・風の作用を重力応力へ含めない。
pub fn design_gravity_terms(model: &Model, terms: &[(LoadCaseId, f64)]) -> Vec<(LoadCaseId, f64)> {
    terms
        .iter()
        .copied()
        .filter(|(id, _)| {
            model.load_cases.iter().any(|c| {
                c.id == *id
                    && matches!(
                        c.kind,
                        LoadCaseKind::Dead | LoadCaseKind::Live | LoadCaseKind::Snow
                    )
            })
        })
        .collect()
}

/// 選択重力termsの全応力を係数付きで合成する。欠落ケースは拒否する。
pub fn complete_design_gravity_forces<F>(
    terms: &[(LoadCaseId, f64)],
    mut force_of: F,
) -> Result<Vec<(ElemId, MemberForces)>, Vec<LoadCaseId>>
where
    F: FnMut(LoadCaseId) -> Option<Vec<(ElemId, MemberForces)>>,
{
    let ids: Vec<_> = terms.iter().map(|(id, _)| *id).collect();
    let mut index = 0;
    complete_gravity_member_forces(&ids, |id| {
        let factor = terms[index].1;
        index += 1;
        force_of(id).map(|mut forces| {
            for (_, mf) in &mut forces {
                for (_, values) in &mut mf.at {
                    for value in values {
                        *value *= factor;
                    }
                }
            }
            forces
        })
    })
}

/// 選択重力termsのQD1専用Q0 `[N]`。RC 梁は端別の係数付き合成後に最大絶対値を採る。
/// 空terms・参照不明ケースはRC結果なし。有効な無荷重・相殺・Qy直交は0を返す。
/// 算定対象の RC 梁で局所基底を解決できなければ、部材 ID と理由を含む `Err` を返す。
pub fn simple_beam_q0_by_terms(
    model: &Model,
    terms: &[(LoadCaseId, f64)],
) -> Result<HashMap<ElemId, f64>, String> {
    let mut map = HashMap::new();
    for (id, factor) in terms {
        for (elem, q) in other_beam_q0_by_elem(model, *id) {
            *map.entry(elem).or_insert(0.0) += q * factor;
        }
    }
    map.extend(rc_beam_q0_by_terms(model, terms)?);
    Ok(map)
}

/// 1 荷重ケースの部材荷重から、単純梁支持の端部せん断 Q0 `[N]` を算定する。
/// RC 梁は符号付き local Qy 端反力の最大絶対値とし、入力不足の部材は返さない。
/// その他の部材は荷重の絶対値と材軸直交成分から算定する。
/// 算定対象の RC 梁で局所基底を解決できなければ、部材 ID と理由を含む `Err` を返す。
pub fn simple_beam_q0_by_elem(
    model: &Model,
    lc: LoadCaseId,
) -> Result<HashMap<ElemId, f64>, String> {
    let mut map = other_beam_q0_by_elem(model, lc);
    map.extend(rc_beam_q0_by_terms(model, &[(lc, 1.0)])?);
    Ok(map)
}

fn is_rc_beam(model: &Model, elem: &sepika_core::model::ElementData) -> bool {
    matches!(
        model.element_section(elem).and_then(|s| s.shape.as_ref()),
        Some(sepika_core::section_shape::SectionShape::RcBeamRect { .. })
    ) && sepika_core::structure_kind::member_structure_kind(model, elem)
        == sepika_core::structure_kind::StructureKind::Rc
}

fn rc_beam_geometry(
    model: &Model,
    elem: &sepika_core::model::ElementData,
) -> Result<Option<(f64, [f64; 3])>, String> {
    use sepika_core::geom::vec3;
    if model.element_material(elem).is_none() {
        return Ok(None);
    }
    if elem.nodes.len() != 2 {
        return Ok(None);
    }
    let Some(ni) = model.nodes.get(elem.nodes[0].index()) else {
        return Ok(None);
    };
    let Some(nj) = model.nodes.get(elem.nodes[1].index()) else {
        return Ok(None);
    };
    let length_mm = vec3::dist(ni.coord, nj.coord);
    if ni.coord.iter().chain(&nj.coord).any(|v| !v.is_finite()) || length_mm == 0.0 {
        return sepika_element::transform::LocalFrame::from_nodes(
            ni.coord,
            nj.coord,
            elem.local_axis.ref_vector,
        )
        .map(|_| None)
        .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", elem.id.0));
    }
    if !length_mm.is_finite() || length_mm <= 1e-9 {
        return Ok(None);
    }
    let frame = sepika_element::transform::LocalFrame::from_nodes(
        ni.coord,
        nj.coord,
        elem.local_axis.ref_vector,
    )
    .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", elem.id.0))?;
    Ok(Some((length_mm, frame.rot[1])))
}

fn rc_beam_q0_by_terms(
    model: &Model,
    terms: &[(LoadCaseId, f64)],
) -> Result<HashMap<ElemId, f64>, String> {
    use sepika_core::geom::vec3;

    if terms.is_empty() {
        return Ok(HashMap::new());
    }
    for (id, factor) in terms {
        if !factor.is_finite() || !model.load_cases.iter().any(|case| case.id == *id) {
            return Ok(HashMap::new());
        }
    }
    let mut reactions: HashMap<ElemId, Option<(f64, f64)>> = HashMap::new();
    for elem in model.elements.iter().filter(|elem| is_rc_beam(model, elem)) {
        reactions.insert(elem.id, rc_beam_geometry(model, elem)?.map(|_| (0.0, 0.0)));
    }
    for (id, factor) in terms {
        let Some(case) = model.load_cases.iter().find(|case| case.id == *id) else {
            return Ok(HashMap::new());
        };
        if !factor.is_finite() {
            return Ok(HashMap::new());
        }
        for load in &case.member {
            let Some(elem) = model.element(load.elem) else {
                return Ok(HashMap::new());
            };
            let Some(acc) = reactions.get_mut(&elem.id) else {
                continue;
            };
            let geometry = rc_beam_geometry(model, elem)?;
            let contribution = (|| {
                let (length_mm, ey) = geometry?;
                if !load.dir.iter().all(|v| v.is_finite()) {
                    return None;
                }
                let dir_norm = vec3::norm(load.dir);
                if !dir_norm.is_finite() || dir_norm <= 0.0 {
                    return None;
                }
                let projection = vec3::dot(vec3::scale(load.dir, 1.0 / dir_norm), ey);
                let (total_n, moment_n_mm) = match load.kind {
                    MemberLoadKind::Point { a, p } => {
                        if !a.is_finite() || !p.is_finite() {
                            return None;
                        }
                        (p, p * a.clamp(0.0, length_mm))
                    }
                    MemberLoadKind::Distributed { a, b, w1, w2 } => {
                        if ![a, b, w1, w2].iter().all(|v| v.is_finite()) {
                            return None;
                        }
                        let a = a.clamp(0.0, length_mm);
                        let b = b.clamp(0.0, length_mm);
                        if b <= a {
                            return Some((0.0, 0.0));
                        }
                        let h_mm = b - a;
                        let total_n = (w1 + w2) * h_mm / 2.0;
                        let moment_n_mm = a * total_n + h_mm * h_mm * (w1 + 2.0 * w2) / 6.0;
                        (total_n, moment_n_mm)
                    }
                };
                let rj_n = moment_n_mm / length_mm;
                let ri_n = total_n - rj_n;
                let ri_n = ri_n * projection * factor;
                let rj_n = rj_n * projection * factor;
                [total_n, moment_n_mm, projection, ri_n, rj_n]
                    .iter()
                    .all(|v| v.is_finite())
                    .then_some((ri_n, rj_n))
            })();
            *acc = acc.zip(contribution).and_then(|((ri, rj), (di, dj))| {
                let sum = (ri + di, rj + dj);
                (sum.0.is_finite() && sum.1.is_finite()).then_some(sum)
            });
        }
    }
    Ok(reactions
        .into_iter()
        .filter_map(|(id, reactions)| reactions.map(|(ri, rj)| (id, ri.abs().max(rj.abs()))))
        .collect())
}

fn other_beam_q0_by_elem(model: &Model, lc: LoadCaseId) -> HashMap<ElemId, f64> {
    let mut acc: HashMap<ElemId, (f64, f64)> = HashMap::new();
    let Some(case) = model.load_cases.iter().find(|c| c.id == lc) else {
        return HashMap::new();
    };
    for ml in &case.member {
        let Some(elem) = model.element(ml.elem) else {
            continue;
        };
        if is_rc_beam(model, elem) {
            continue;
        }
        if elem.nodes.len() < 2 {
            continue;
        }
        let (Some(n0), Some(n1)) = (
            model.nodes.get(elem.nodes[0].index()),
            model.nodes.get(elem.nodes[elem.nodes.len() - 1].index()),
        ) else {
            continue;
        };
        let dx = [
            n1.coord[0] - n0.coord[0],
            n1.coord[1] - n0.coord[1],
            n1.coord[2] - n0.coord[2],
        ];
        let l = (dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2]).sqrt();
        if l <= 0.0 {
            continue;
        }
        let e = [dx[0] / l, dx[1] / l, dx[2] / l];
        let dn = (ml.dir[0] * ml.dir[0] + ml.dir[1] * ml.dir[1] + ml.dir[2] * ml.dir[2]).sqrt();
        if dn <= 0.0 {
            continue;
        }
        let d = [ml.dir[0] / dn, ml.dir[1] / dn, ml.dir[2] / dn];
        let ax = d[0] * e[0] + d[1] * e[1] + d[2] * e[2];
        let trans = (1.0 - ax * ax).max(0.0).sqrt();
        if trans <= 1e-12 {
            continue;
        }
        let (w_total, x_bar) = match ml.kind {
            MemberLoadKind::Point { a, p } => (p.abs(), a.clamp(0.0, l)),
            MemberLoadKind::Distributed { a, b, w1, w2 } => {
                let (a, b) = (a.clamp(0.0, l), b.clamp(0.0, l));
                if b <= a {
                    continue;
                }
                let w_sum = w1 + w2;
                let total = w_sum / 2.0 * (b - a);
                let xb = if w_sum.abs() > 1e-12 {
                    a + (b - a) * (w1 + 2.0 * w2) / (3.0 * w_sum)
                } else {
                    (a + b) / 2.0
                };
                (total.abs(), xb)
            }
        };
        let entry = acc.entry(ml.elem).or_insert((0.0, 0.0));
        entry.0 += trans * w_total * (l - x_bar) / l;
        entry.1 += trans * w_total * x_bar / l;
    }
    acc.into_iter()
        .map(|(k, (ri, rj))| (k, ri.max(rj)))
        .collect()
}

/// 検定用G+PのQD1専用Q0 `[N]`。RC 梁は端別合成後に最大絶対値、他はケース別Q0の和。
/// 算定対象の RC 梁で局所基底を解決できなければ、部材 ID と理由を含む `Err` を返す。
pub fn simple_beam_q0_by_gravity_cases(model: &Model) -> Result<HashMap<ElemId, f64>, String> {
    let terms: Vec<_> = gravity_case_ids_for_design(model)
        .into_iter()
        .map(|id| (id, 1.0))
        .collect();
    simple_beam_q0_by_terms(model, &terms)
}

/// 複数ケースの部材内力を位置ごとに加算する（線形重ね合わせ）。
/// 近傍位置（絶対差 1e-9 以内）は足し合わせ、欠ける側は 0 とみなす。
pub fn sum_member_forces_lists(
    lists: &[Vec<(ElemId, MemberForces)>],
) -> Vec<(ElemId, MemberForces)> {
    let mut by_elem: HashMap<ElemId, Vec<(f64, [f64; 6])>> = HashMap::new();
    const POS_EPS: f64 = 1e-9;
    for list in lists {
        for (id, mf) in list {
            let entry = by_elem.entry(*id).or_default();
            for (p, f) in &mf.at {
                if let Some((_, acc)) = entry.iter_mut().find(|(q, _)| (*q - *p).abs() <= POS_EPS) {
                    for i in 0..6 {
                        acc[i] += f[i];
                    }
                } else {
                    entry.push((*p, *f));
                }
            }
        }
    }
    let mut out: Vec<(ElemId, MemberForces)> = by_elem
        .into_iter()
        .map(|(id, mut at)| {
            at.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            (id, MemberForces { at })
        })
        .collect();
    out.sort_by_key(|(id, _)| id.0);
    out
}

/// [`gravity_case_ids_for_seismic_weight`] と同じ集合の解析済み内力を加算する。
///
/// `force_of` が `None` を返すケースは飛ばす。1 件も取れなければ `None`。
pub fn sum_analyzed_gravity_member_forces<F>(
    model: &Model,
    mut force_of: F,
) -> Option<Vec<(ElemId, MemberForces)>>
where
    F: FnMut(LoadCaseId) -> Option<Vec<(ElemId, MemberForces)>>,
{
    let lists: Vec<_> = gravity_case_ids_for_seismic_weight(model)
        .into_iter()
        .filter_map(&mut force_of)
        .collect();
    if lists.is_empty() {
        None
    } else {
        Some(sum_member_forces_lists(&lists))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_member_forces_merges_near_positions() {
        let a = vec![(
            ElemId(1),
            MemberForces {
                at: vec![(0.0, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
            },
        )];
        let b = vec![(
            ElemId(1),
            MemberForces {
                at: vec![(1e-12, [2.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
            },
        )];
        let sum = sum_member_forces_lists(&[a, b]);
        assert_eq!(sum.len(), 1);
        assert_eq!(sum[0].1.at.len(), 1);
        assert!((sum[0].1.at[0].1[0] - 3.0).abs() < 1e-12);
    }

    /// 地震用重量に使う重力ケースの選択が、並び順ではなく
    /// `LoadCaseKind` に基づくこと。Dead+LiveSeismic 優先、LiveSeismic がなければ
    /// Dead+Live、種別が一つも設定されていなければ後方互換で先頭ケースのみ。
    /// 自動生成の自重ケースと骨組用積載ケースは除外する。
    #[test]
    fn test_gravity_case_ids_for_seismic_weight_selection() {
        use sepika_core::model::LoadCase;

        let mk_lc = |i: u32, name: &str, kind: LoadCaseKind| LoadCase {
            id: LoadCaseId(i),
            name: name.to_string(),
            nodal: Vec::new(),
            member: Vec::new(),
            kind,
        };

        // 種別が一つも設定されていない（全て既定値 Other） → 先頭ケースのみ
        let model_no_kind = Model {
            load_cases: vec![
                mk_lc(0, "LC0", LoadCaseKind::Other),
                mk_lc(1, "LC1", LoadCaseKind::Other),
            ],
            ..Default::default()
        };
        assert_eq!(
            gravity_case_ids_for_seismic_weight(&model_no_kind),
            vec![LoadCaseId(0)],
            "種別未設定モデルは従来互換で先頭ケースのみ"
        );

        // LiveSeismic がない → Dead + Live。
        // ただし自重(自動)ケースと骨組用積載ケースは地震用重量から除外する。
        let model_dead_live = Model {
            load_cases: vec![
                mk_lc(0, "固定", LoadCaseKind::Dead),
                mk_lc(1, LL_FRAME_CASE_NAME, LoadCaseKind::Live),
                mk_lc(2, SELF_WEIGHT_AUTO_LOAD_CASE_NAME, LoadCaseKind::Dead),
                mk_lc(3, "積載(長期)", LoadCaseKind::Live),
                mk_lc(4, "積雪", LoadCaseKind::Snow),
            ],
            ..Default::default()
        };
        assert_eq!(
            gravity_case_ids_for_seismic_weight(&model_dead_live),
            vec![LoadCaseId(0), LoadCaseId(3)],
            "LiveSeismic がなければ Dead+Live（自重(自動)・骨組用積載・積雪は除外）"
        );

        // LiveSeismic があれば Live ではなく LiveSeismic を優先
        let model_dead_live_seismic = Model {
            load_cases: vec![
                mk_lc(0, "固定", LoadCaseKind::Dead),
                mk_lc(1, "積載(長期)", LoadCaseKind::Live),
                mk_lc(2, "積載(地震用)", LoadCaseKind::LiveSeismic),
            ],
            ..Default::default()
        };
        assert_eq!(
            gravity_case_ids_for_seismic_weight(&model_dead_live_seismic),
            vec![LoadCaseId(0), LoadCaseId(2)],
            "LiveSeismic があれば Live ではなく LiveSeismic を採用"
        );

        // 複数 Dead ケースも全て対象
        let model_multi_dead = Model {
            load_cases: vec![
                mk_lc(0, "固定1", LoadCaseKind::Dead),
                mk_lc(1, "固定2", LoadCaseKind::Dead),
                mk_lc(2, "地震荷重", LoadCaseKind::Seismic),
            ],
            ..Default::default()
        };
        assert_eq!(
            gravity_case_ids_for_seismic_weight(&model_multi_dead),
            vec![LoadCaseId(0), LoadCaseId(1)],
            "複数の Dead ケースは全て対象、Seismic は対象外"
        );
    }
}

#[cfg(test)]
mod load_contract_tests {
    use super::*;
    use sepika_core::model::{LoadCase, LoadCombination};

    #[test]
    fn design_gravity_preserves_selected_snow_and_rejects_partial_or_total_missing_results() {
        let model = Model {
            load_cases: [
                LoadCaseKind::Dead,
                LoadCaseKind::Live,
                LoadCaseKind::LiveSeismic,
                LoadCaseKind::Snow,
                LoadCaseKind::Seismic,
                LoadCaseKind::Dead,
            ]
            .into_iter()
            .enumerate()
            .map(|(i, kind)| LoadCase {
                id: LoadCaseId(i as u32),
                name: "改名".into(),
                kind,
                nodal: vec![],
                member: vec![],
            })
            .collect(),
            ..Default::default()
        };
        let combo = LoadCombination {
            name: "常時という名前".into(),
            terms: vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(3), 0.35),
                (LoadCaseId(4), -1.0),
            ],
        };
        let terms = design_gravity_terms(&model, &combo.terms);
        assert_eq!(
            terms,
            vec![
                (LoadCaseId(0), 1.0),
                (LoadCaseId(1), 1.0),
                (LoadCaseId(3), 0.35)
            ]
        );
        let force = |id: LoadCaseId| {
            let n = [100_000.0, 20_000.0, 8_000.0, 40_000.0, 30_000.0, 900_000.0][id.index()];
            Some(vec![(
                ElemId(0),
                MemberForces {
                    at: vec![(0.5, [n, n, n, n * 1000.0, n * 1000.0, n * 1000.0])],
                },
            )])
        };
        let reference = complete_design_gravity_forces(&terms, force).unwrap();
        assert_eq!(reference[0].1.at[0].1[0] / 1000.0, 134.0);
        assert_eq!(reference[0].1.at[0].1[5] / 1_000_000.0, 134.0);
        assert_eq!(
            complete_design_gravity_forces(&terms, |id| if id == LoadCaseId(1) {
                None
            } else {
                force(id)
            })
            .unwrap_err(),
            vec![LoadCaseId(1)]
        );
        assert_eq!(
            complete_design_gravity_forces(&terms, |_| None).unwrap_err(),
            vec![LoadCaseId(0), LoadCaseId(1), LoadCaseId(3)]
        );
        let frame =
            complete_design_gravity_forces(&[(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)], force)
                .unwrap();
        assert_eq!(frame[0].1.at[0].1[0] / 1000.0, 120.0);
        let weight =
            complete_design_gravity_forces(&[(LoadCaseId(0), 1.0), (LoadCaseId(2), 1.0)], force)
                .unwrap();
        assert_eq!(weight[0].1.at[0].1[0] / 1000.0, 108.0);
    }
}

#[cfg(test)]
mod q0_contract_tests {
    use super::*;
    use sepika_core::ids::NodeId;
    use sepika_core::model::{ElementData, ElementKind, LoadCase, MemberLoad, Node};

    #[test]
    fn q0_uses_selected_member_loads_and_frame_live_separately_from_weight_and_fem_ql() {
        let mut model = Model {
            nodes: [0.0, 2000.0]
                .into_iter()
                .enumerate()
                .map(|(i, x)| Node {
                    id: NodeId(i as u32),
                    coord: [x, 0.0, 0.0],
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                })
                .collect(),
            ..Default::default()
        };
        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: [NodeId(0), NodeId(1)].into_iter().collect(),
            section: None,
            local_axis: sepika_core::model::LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [sepika_core::model::EndCondition::Fixed; 2],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
        model.load_cases = [
            (LoadCaseKind::Dead, 100.0),
            (LoadCaseKind::Live, 20.0),
            (LoadCaseKind::LiveSeismic, 8.0),
            (LoadCaseKind::Snow, 40.0),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (kind, w))| LoadCase {
            id: LoadCaseId(i as u32),
            name: "任意名称".into(),
            kind,
            nodal: vec![],
            member: vec![MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 2000.0,
                    w1: w,
                    w2: w,
                },
            )],
        })
        .collect();
        assert_eq!(
            simple_beam_q0_by_gravity_cases(&model).unwrap()[&ElemId(0)],
            120_000.0
        );
        assert_eq!(
            simple_beam_q0_by_terms(&model, &[(LoadCaseId(0), 1.0), (LoadCaseId(2), 1.0)]).unwrap()
                [&ElemId(0)],
            108_000.0
        );
        assert_eq!(
            simple_beam_q0_by_terms(
                &model,
                &[
                    (LoadCaseId(0), 1.0),
                    (LoadCaseId(1), 1.0),
                    (LoadCaseId(3), 0.35)
                ]
            )
            .unwrap()[&ElemId(0)],
            134_000.0
        );
        let fem = |_| {
            Some(vec![(
                ElemId(0),
                MemberForces {
                    at: vec![(0.0, [0.0, 7000.0, 0.0, 0.0, 0.0, 0.0])],
                },
            )])
        };
        assert_eq!(
            complete_design_gravity_forces(&[(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)], fem)
                .unwrap()[0]
                .1
                .at[0]
                .1[1],
            14_000.0
        );
        assert!(
            complete_gravity_member_forces(&[LoadCaseId(0), LoadCaseId(1)], |id| {
                if id == LoadCaseId(0) {
                    fem(id)
                } else {
                    Some(vec![])
                }
            })
            .is_err()
        );
    }
}
