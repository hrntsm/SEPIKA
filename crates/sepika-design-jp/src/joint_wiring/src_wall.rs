use super::common::ForcesAt;
use crate::wall_check::{WallCheck, WallCheckKind, WallSkipKind};
use crate::{CheckComponent, CheckKind, CheckOutcome, CheckResult, LoadTerm};
use sepika_core::ids::{ElemId, WallPlateId};
use sepika_core::model::{ElementData, LoadCaseKind, Model};
use sepika_core::src_wall_capacity::resolve_src_wall;
use sepika_element::wall::src_face::{src_wall_shear_strength, SrcWallFaces};

pub(super) fn check_wall(
    model: &Model,
    plate: Option<WallPlateId>,
    wall: Option<&ElementData>,
    forces: &[(ElemId, ForcesAt<'_>)],
    inputs: super::WallDesignInputs<'_>,
) -> Vec<WallCheck> {
    use WallCheckKind::*;
    let super::WallDesignInputs {
        term,
        case,
        load_terms: terms,
        gravity_forces: gravity,
        src_holding_reviewed: holding_reviewed,
        ..
    } = inputs;
    let long_term = term == LoadTerm::Long;
    let resolved = (|| {
        let wall = wall.ok_or("SRC耐震壁の断面割当または境界入力が不足しています")?;
        let capacity = resolve_src_wall(model, wall)?.ok_or("SRC耐震壁の周辺構面が未解決です")?;
        let face = SrcWallFaces::new(wall, model)?;
        let terms = terms.ok_or("SRC耐震壁の荷重の由来が不足しています")?;
        let mut cases = Vec::new();
        for &(id, factor) in terms {
            if !factor.is_finite() {
                return Err("SRC耐震壁の荷重係数が非有限です".to_owned());
            }
            if factor != 0.0 {
                let mut matches = model.load_cases.iter().filter(|c| c.id == id);
                let case = matches
                    .next()
                    .filter(|_| matches.next().is_none())
                    .ok_or("SRC耐震壁の荷重ケースが不足または重複しています")?;
                cases.push(case.clone());
            }
        }
        if cases.is_empty() {
            return Err("SRC耐震壁の荷重ケースが不足しています".to_owned());
        }
        face.ensure_loads(&cases)?;
        let ends = face.recover(forces)?;
        let seismic = cases.iter().any(|c| c.kind == LoadCaseKind::Seismic);
        if seismic && long_term {
            return Err("SRC耐震壁の地震作用には短期の検定条件が必要です".into());
        }
        Ok::<_, String>((capacity, face, ends, seismic))
    })();
    let node = wall.and_then(|w| w.nodes.first().copied());
    let mut checks = Vec::new();
    let mut push = |kind, basis, result: Result<Vec<CheckComponent>, String>| {
        let (outcome, skip_kind) = match result {
            Ok(components) => (
                CheckOutcome::Checked(CheckResult {
                    basis,
                    detail: String::new(),
                    components,
                }),
                None,
            ),
            Err(reason) => {
                let why = if reason.contains("応答") || reason.contains("内力行") {
                    WallSkipKind::MissingResponse
                } else if reason.contains("不足")
                    || reason.contains("未確認")
                    || reason.contains("未割当")
                {
                    WallSkipKind::MissingInput
                } else if reason.contains("未対応") || reason.contains("範囲") {
                    WallSkipKind::NotApplicable
                } else {
                    WallSkipKind::InvalidInput
                };
                (CheckOutcome::Skipped { reason }, Some(why))
            }
        };
        checks.push(WallCheck {
            plate,
            elem: wall.map(|w| w.id),
            node,
            case: case.into(),
            kind,
            shear_method: None,
            seismic_target: true,
            skip_kind,
            outcome,
            reference_skeleton: None,
        });
    };
    let label = match &resolved {
        Ok((cap, _, _, _)) if cap.has_steel_plate() => "鋼板内蔵SRC耐震壁",
        _ => "SRC耐震壁",
    };
    let load_label = if long_term { "長期" } else { "短期" };
    push(
        AllowableShear,
        format!("{label} SRC規準25条 許容せん断（{load_label}）"),
        resolved
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(cap, face, ends, seismic)| {
                let (demands, demand_name) = if *seismic {
                    let gravity = gravity.ok_or(
                        "地震時の25条設計用せん断力には同じ組合せの重力内力が必要です（応答不足）",
                    )?;
                    let gravity_ends = face.recover(gravity)?;
                    let factor = if holding_reviewed { 1.0 } else { 1.5 };
                    let label = if holding_reviewed {
                        "Qd=QE（保有検討済み）"
                    } else {
                        "Qd=1.5QE（保有検討未確認）"
                    };
                    (
                        [0, 1].map(|i| factor * (ends[i].shear_n - gravity_ends[i].shear_n).abs()),
                        label,
                    )
                } else {
                    (ends.map(|end| end.shear_n.abs()), "Qd")
                };
                let demand = demands[0].max(demands[1]);
                let capacity = cap.allowable_shear_n(long_term);
                check_ratio(CheckKind::Shear, demand, capacity, demand_name, "Qa", "N")
                    .map(|c| vec![c])
            }),
    );
    push(
        AllowableFlexure,
        format!("{label} SRC規準25条 両側柱分担法（{load_label}）"),
        resolved
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|(cap, face, ends, seismic)| {
                let gravity = if long_term {
                    forces
                } else {
                    gravity.ok_or(
                        "曲げの両側柱分担法に同じ組合せの柱別長期軸力が必要です（応答不足）",
                    )?
                };
                let columns = face.recover_column_compression(gravity)?;
                ["壁脚", "壁頭"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, location)| {
                        let ratio =
                            cap.allowable_bending_ratio(ends[i], columns[i], long_term, *seismic)?;
                        Ok(CheckComponent {
                            kind: CheckKind::AxialBending,
                            ratio,
                            detail: format!(
                                "{location} M={:.1} Nmm, 柱別NL=[{:.1}, {:.1}] N",
                                ends[i].moment_nmm, columns[i][0], columns[i][1]
                            ),
                        })
                    })
                    .collect()
            }),
    );
    if !long_term {
        push(
            UltimateShear,
            format!("{label} SRC規準38条2項 参照断面力の終局せん断照査"),
            resolved
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|(cap, _, ends, _)| {
                    let qu = src_wall_shear_strength(cap, *ends)?
                        .ok_or("階内最大せん断力Qが0のため、38条2項のせん断スパン比の適用範囲を確認できません")?;
                    let q = ends[0].shear_n.abs().max(ends[1].shear_n.abs());
                    check_ratio(CheckKind::Shear, q, qu, "Q", "Qu", "N").map(|c| vec![c])
                }),
        );
        for (i, location) in ["壁脚", "壁頭"].into_iter().enumerate() {
            push(
                UltimateFlexure,
                format!("{label} SRC規準38条1項 {location}の終局曲げ照査"),
                resolved
                    .as_ref()
                    .map_err(Clone::clone)
                    .and_then(|(cap, _, ends, _)| {
                        let end = ends[i];
                        let mu = cap
                            .ultimate_moment_nmm(end)
                            .map_err(|reason| format!("{location}: {reason}"))?;
                        check_ratio(
                            CheckKind::AxialBending,
                            end.moment_nmm.abs(),
                            mu,
                            "M",
                            "Mu",
                            "Nmm",
                        )
                        .map(|c| vec![c])
                    }),
            );
        }
    }
    checks
}

fn check_ratio(
    kind: CheckKind,
    demand: f64,
    limit: f64,
    demand_name: &str,
    limit_name: &str,
    unit: &str,
) -> Result<CheckComponent, String> {
    let ratio = demand / limit;
    if !ratio.is_finite() || !limit.is_finite() || limit <= 0.0 {
        return Err("壁の作用内力または耐力が不正です".into());
    }
    Ok(CheckComponent {
        kind,
        ratio,
        detail: format!("{demand_name}={demand:.1} {unit}, {limit_name}={limit:.1} {unit}"),
    })
}
