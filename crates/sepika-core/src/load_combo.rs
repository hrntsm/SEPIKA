use crate::ids::LoadCaseId;
use crate::model::LoadCombination;

/// 多雪区域組合せの積雪係数。標準外の直接入力は検定継続時間を自動確認しない。
///
/// - `delta1`: 長期積雪 `DL+LL+δ1・SL` の低減係数（既定 0.7）
/// - `delta3`: 地震時 `DL+LL+δ3・SL±EX/EY` の低減係数（既定 0.35）
///
/// 本実装では直接入力が可能（デフォルト δ1=0.7、δ3=0.35）。
/// 暴風時の δ2 は、風荷重の組合せを生成しないため持たない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnowFactors {
    pub delta1: f64,
    pub delta3: f64,
}

impl Default for SnowFactors {
    fn default() -> Self {
        SnowFactors {
            delta1: 0.7,
            delta3: 0.35,
        }
    }
}

/// [`standard_combinations`] への入力ケース指定。
pub struct ComboInput {
    pub dl: LoadCaseId,
    pub ll: LoadCaseId,
    pub seismic_x: Option<LoadCaseId>,
    pub seismic_y: Option<LoadCaseId>,
    pub snow: Option<LoadCaseId>,
    /// 多雪区域か否か（建築基準法施行令86条・同82条）。
    /// `true` の場合、長期に `δ1・S` を加算し、短期地震に `δ3・S` を
    /// 加算した組合せも追加で生成する。
    pub heavy_snow_zone: bool,
    /// 多雪区域の積雪荷重低減係数。`None` は既定値（δ1=0.7、δ2=δ3=0.35）。
    pub snow_factors: Option<SnowFactors>,
}

fn push_gp(combos: &mut Vec<LoadCombination>, dl: LoadCaseId, ll: LoadCaseId) {
    combos.push(LoadCombination {
        name: "DL + LL".into(),
        terms: vec![(dl, 1.0), (ll, 1.0)],
    });
}

/// 建築基準法施行令82条の標準荷重組合せを生成する。
///
/// 組合せ名は荷重ケースの直接的な名前（DL：固定・LL：積載・EX/EY：地震 X/Y・
/// SL：積雪）で表す。算定式との対応は
/// G→DL・P→LL・K→EX/EY・S→SL（積雪は単一ケースのため方向なし）。
///
/// - 長期: `DL+LL`。多雪区域はさらに `DL+LL+0.7SL`。
/// - 短期積雪: `DL+LL+SL`（`snow` が指定されている場合）。
/// - 短期地震: `DL+LL±EX`・`DL+LL±EY`（±両方向）。多雪区域はさらに
///   `DL+LL+0.35SL±EX`（X・Y 各方向）。
///
/// 各ケースは `seismic_x`/`seismic_y`/`snow` が `Some` の場合のみ生成される。
///
/// 風荷重は算定・生成の対象外のため、暴風の組合せは作らない。風荷重ケースを
/// 定義しても、そのケースを含む組合せは自動生成されない。
pub fn standard_combinations(input: &ComboInput) -> Vec<LoadCombination> {
    let mut combos = Vec::new();
    let dl = input.dl;
    let ll = input.ll;
    let sf = input.snow_factors.unwrap_or_default();

    push_gp(&mut combos, dl, ll);

    if input.heavy_snow_zone {
        if let Some(snow) = input.snow {
            combos.push(LoadCombination {
                name: format!("DL + LL + {}SL", trim_f64(sf.delta1)),
                terms: vec![(dl, 1.0), (ll, 1.0), (snow, sf.delta1)],
            });
        }
    }

    if let Some(snow) = input.snow {
        combos.push(LoadCombination {
            name: "DL + LL + SL".into(),
            terms: vec![(dl, 1.0), (ll, 1.0), (snow, 1.0)],
        });
    }

    push_directional(
        &mut combos,
        dl,
        ll,
        input.seismic_x,
        "EX",
        input.snow,
        input.heavy_snow_zone,
        sf.delta3,
    );
    push_directional(
        &mut combos,
        dl,
        ll,
        input.seismic_y,
        "EY",
        input.snow,
        input.heavy_snow_zone,
        sf.delta3,
    );

    combos
}

/// 係数を組合せ名向けに整形する（末尾の 0 を落とす。例: 0.70 → "0.7"）。
fn trim_f64(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 地震・暴風いずれかの片方向（EX/EY/WX/WY）について、`DL+LL±X` と
/// 多雪区域なら `DL+LL+δ・SL±X`（δ: 暴風時 δ2／地震時 δ3）を追加する共通ヘルパー。
#[allow(clippy::too_many_arguments)]
fn push_directional(
    combos: &mut Vec<LoadCombination>,
    dl: LoadCaseId,
    ll: LoadCaseId,
    case: Option<LoadCaseId>,
    label: &str,
    snow: Option<LoadCaseId>,
    heavy_snow_zone: bool,
    delta: f64,
) {
    let Some(case) = case else {
        return;
    };
    combos.push(LoadCombination {
        name: format!("DL + LL + {label}"),
        terms: vec![(dl, 1.0), (ll, 1.0), (case, 1.0)],
    });
    combos.push(LoadCombination {
        name: format!("DL + LL - {label}"),
        terms: vec![(dl, 1.0), (ll, 1.0), (case, -1.0)],
    });
    if heavy_snow_zone {
        if let Some(snow) = snow {
            let d = trim_f64(delta);
            combos.push(LoadCombination {
                name: format!("DL + LL + {d}SL + {label}"),
                terms: vec![(dl, 1.0), (ll, 1.0), (snow, delta), (case, 1.0)],
            });
            combos.push(LoadCombination {
                name: format!("DL + LL + {d}SL - {label}"),
                terms: vec![(dl, 1.0), (ll, 1.0), (snow, delta), (case, -1.0)],
            });
        }
    }
}

/// 断面検定などから使う単純版
/// （長期 DL+LL / 短期積雪 DL+LL+SL / 短期地震 DL+LL±EX/EY の正負両加力）。
/// 内部では [`standard_combinations`] に委譲する。多雪区域の係数付き組合せが
/// 必要な場合は [`standard_combinations`] を直接使う。
pub fn auto_combinations(
    dl_case: LoadCaseId,
    ll_case: LoadCaseId,
    seismic_x: Option<LoadCaseId>,
    seismic_y: Option<LoadCaseId>,
    snow_case: Option<LoadCaseId>,
) -> Vec<LoadCombination> {
    let input = ComboInput {
        dl: dl_case,
        ll: ll_case,
        seismic_x,
        seismic_y,
        snow: snow_case,
        heavy_snow_zone: false,
        snow_factors: None,
    };
    standard_combinations(&input)
}

/// 許容応力度に用いる荷重継続時間。名称とは独立に判定する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadDuration {
    Long,
    Short,
}

/// 検定対象の作用。地震用重量の積載ケースは検定用重力荷重に含めない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadAction {
    Gravity,
    Snow,
    Wind,
    Seismic,
}

/// 荷重状態の継続時間と作用。単独ケースは法令の組合せ検定を表さない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DesignLoadState {
    pub duration: LoadDuration,
    pub action: LoadAction,
    pub combination: bool,
}

/// 保存種別から単独ケースの荷重状態を判定する。不明種別・地震重量用は未判定。
pub fn case_design_state(kind: crate::model::LoadCaseKind) -> Result<DesignLoadState, String> {
    use crate::model::LoadCaseKind as K;
    let (duration, action) = match kind {
        K::Dead | K::Live => (LoadDuration::Long, LoadAction::Gravity),
        K::Snow => (LoadDuration::Short, LoadAction::Snow),
        K::Wind => (LoadDuration::Short, LoadAction::Wind),
        K::Seismic => (LoadDuration::Short, LoadAction::Seismic),
        K::LiveSeismic => return Err("地震重量用積載は検定用の重力荷重ではありません".into()),
        K::Other => return Err("荷重種別が未指定のため継続時間を判定できません".into()),
    };
    Ok(DesignLoadState {
        duration,
        action,
        combination: false,
    })
}

/// 保存係数・荷重種別から検定状態を判定する。標準外係数・不足・混合作用は理由付き未判定。
/// 状態の判定は、多雪区域指定や風時の実況等の法的適用条件の確認を代替しない。
pub fn combination_design_state(
    combo: &LoadCombination,
    cases: &[crate::model::LoadCase],
) -> Result<DesignLoadState, String> {
    use crate::model::LoadCaseKind as K;
    let mut coefficients = std::collections::BTreeMap::new();
    for (id, factor) in &combo.terms {
        if !factor.is_finite() {
            return Err("荷重組合せ係数が非有限です".into());
        }
        *coefficients.entry(id.0).or_insert(0.0) += factor;
    }
    let mut dead = false;
    let mut live = false;
    let mut snow = 0.0;
    let mut snow_cases = 0;
    let mut lateral = None;
    for (id, factor) in coefficients {
        if factor == 0.0 {
            continue;
        }
        let case = cases
            .iter()
            .find(|c| c.id.0 == id)
            .ok_or_else(|| format!("荷重ケース {id} がありません"))?;
        match case.kind {
            K::Dead | K::Live if factor == 1.0 => {
                dead |= case.kind == K::Dead;
                live |= case.kind == K::Live;
            }
            K::Snow if factor > 0.0 => {
                snow = factor;
                snow_cases += 1;
            }
            K::Wind | K::Seismic if factor.abs() == 1.0 && lateral.is_none() => {
                lateral = Some(if case.kind == K::Wind {
                    LoadAction::Wind
                } else {
                    LoadAction::Seismic
                });
            }
            _ => {
                return Err(format!(
                    "荷重ケース {id} の種別・係数から検定状態を一意に判定できません"
                ))
            }
        }
    }
    if !dead || !live {
        return Err("固定荷重Gと架構用積載Pを含む組合せが必要です（P=0もケースで明示）".into());
    }
    if snow_cases > 1 {
        return Err("複数の積雪ケースの適用条件が未判定です".into());
    }
    let (duration, action) = match lateral {
        Some(action) if snow == 0.0 || snow == 0.35 => (LoadDuration::Short, action),
        Some(_) => return Err("水平作用時の積雪係数が標準0.35と異なり適用条件が未判定です".into()),
        None if snow == 0.0 => (LoadDuration::Long, LoadAction::Gravity),
        None if snow == 0.7 => (LoadDuration::Long, LoadAction::Snow),
        None if snow == 1.0 => (LoadDuration::Short, LoadAction::Snow),
        None => return Err("積雪係数が標準0.7/1.0と異なり継続時間が未判定です".into()),
    };
    Ok(DesignLoadState {
        duration,
        action,
        combination: true,
    })
}

/// 表示結果名を保存組合せへ照合して常時G+Pを選ぶ。同名が重複すれば未判定。
pub fn is_gravity_combination(name: &str, model: &crate::model::Model) -> bool {
    let mut matches = model.combinations.iter().filter(|c| c.name == name);
    let Some(combo) = matches.next() else {
        return false;
    };
    matches.next().is_none()
        && combination_design_state(combo, &model.load_cases)
            .is_ok_and(|s| s.duration == LoadDuration::Long && s.action == LoadAction::Gravity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_combos() {
        let combos = auto_combinations(
            LoadCaseId(1),
            LoadCaseId(2),
            Some(LoadCaseId(3)),
            Some(LoadCaseId(4)),
            None,
        );
        // DL+LL, DL+LL±EX, DL+LL±EY の 5 組合せ
        assert_eq!(combos.len(), 5);
        assert_eq!(combos[0].name, "DL + LL");
        assert_eq!(combos[1].name, "DL + LL + EX");
        assert_eq!(combos[2].name, "DL + LL - EX");
        assert_eq!(combos[3].name, "DL + LL + EY");
        assert_eq!(combos[4].name, "DL + LL - EY");
        // 負側加力は係数 -1.0
        assert_eq!(combos[2].terms[2].1, -1.0);
        assert_eq!(combos[4].terms[2].1, -1.0);
    }

    #[test]
    fn test_standard_combinations_all_cases_heavy_snow() {
        let input = ComboInput {
            dl: LoadCaseId(1),
            ll: LoadCaseId(2),
            seismic_x: Some(LoadCaseId(3)),
            seismic_y: Some(LoadCaseId(4)),
            snow: Some(LoadCaseId(7)),
            heavy_snow_zone: true,
            snow_factors: None,
        };
        let combos = standard_combinations(&input);
        // DL+LL(1) + DL+LL+0.7SL(1) + DL+LL+SL(1) + EX系4 + EY系4 = 3 + 8 = 11
        assert_eq!(combos.len(), 11);

        let by_name = |n: &str| {
            combos
                .iter()
                .find(|c| c.name == n)
                .unwrap_or_else(|| panic!("missing combo {n}"))
        };

        assert_eq!(
            by_name("DL + LL").terms,
            vec![(LoadCaseId(1), 1.0), (LoadCaseId(2), 1.0)]
        );
        assert_eq!(
            by_name("DL + LL + 0.7SL").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(7), 0.7)
            ]
        );
        assert_eq!(
            by_name("DL + LL + SL").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(7), 1.0)
            ]
        );
        assert_eq!(
            by_name("DL + LL + EX").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(3), 1.0)
            ]
        );
        assert_eq!(
            by_name("DL + LL - EX").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(3), -1.0)
            ]
        );
        assert_eq!(
            by_name("DL + LL + 0.35SL + EX").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(7), 0.35),
                (LoadCaseId(3), 1.0)
            ]
        );
        assert_eq!(
            by_name("DL + LL + 0.35SL - EY").terms,
            vec![
                (LoadCaseId(1), 1.0),
                (LoadCaseId(2), 1.0),
                (LoadCaseId(7), 0.35),
                (LoadCaseId(4), -1.0)
            ]
        );
        // 風荷重は生成対象外のため、暴風の組合せは 1 件も作られない。
        assert!(combos.iter().all(|c| !c.name.contains('W')));
    }

    #[test]
    fn test_snow_factors_direct_input() {
        // δ1/δ3 の直接入力（デフォルト 0.7/0.35、直接入力可能）。
        // 名前・係数の両方に反映される。
        let input = ComboInput {
            dl: LoadCaseId(1),
            ll: LoadCaseId(2),
            seismic_x: Some(LoadCaseId(3)),
            seismic_y: None,
            snow: Some(LoadCaseId(7)),
            heavy_snow_zone: true,
            snow_factors: Some(SnowFactors {
                delta1: 0.65,
                delta3: 0.4,
            }),
        };
        let combos = standard_combinations(&input);
        let by_name = |n: &str| {
            combos
                .iter()
                .find(|c| c.name == n)
                .unwrap_or_else(|| panic!("missing combo {n}"))
        };
        // 長期積雪: δ1=0.65
        assert_eq!(by_name("DL + LL + 0.65SL").terms[2], (LoadCaseId(7), 0.65));
        // 地震時: δ3=0.4
        assert_eq!(
            by_name("DL + LL + 0.4SL + EX").terms[2],
            (LoadCaseId(7), 0.4)
        );
    }

    #[test]
    fn test_standard_combinations_no_heavy_snow() {
        let input = ComboInput {
            dl: LoadCaseId(1),
            ll: LoadCaseId(2),
            seismic_x: Some(LoadCaseId(3)),
            seismic_y: Some(LoadCaseId(4)),
            snow: Some(LoadCaseId(5)),
            heavy_snow_zone: false,
            snow_factors: None,
        };
        let combos = standard_combinations(&input);
        // DL+LL(1) + DL+LL+SL(1) + EX系2 + EY系2 = 6（多雪でないので 0.7SL・0.35SL 系は無し）
        assert_eq!(combos.len(), 6);
        assert!(combos.iter().all(|c| !c.name.contains("0.35SL")));
        assert!(combos.iter().all(|c| !c.name.contains("0.7SL")));
        assert!(combos.iter().all(|c| !c.name.contains('W')));
    }

    #[test]
    fn test_default_combinations_matches_auto_combinations() {
        let expected = auto_combinations(
            LoadCaseId(0),
            LoadCaseId(1),
            Some(LoadCaseId(3)),
            Some(LoadCaseId(4)),
            None,
        );
        let actual = crate::model::default_combinations();
        assert_eq!(
            actual, expected,
            "default_combinations が auto_combinations（DL/LL/EX/EY）と一致していない"
        );
    }

    #[test]
    fn test_standard_combinations_empty_optional_cases() {
        let input = ComboInput {
            dl: LoadCaseId(1),
            ll: LoadCaseId(2),
            seismic_x: None,
            seismic_y: None,
            snow: None,
            heavy_snow_zone: false,
            snow_factors: None,
        };
        let combos = standard_combinations(&input);
        assert_eq!(combos.len(), 1);
        assert_eq!(combos[0].name, "DL + LL");
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::model::{LoadCase, LoadCaseKind as K, LoadPurpose, SlabUsage};

    fn cases() -> Vec<LoadCase> {
        [
            K::Dead,
            K::Live,
            K::Snow,
            K::Seismic,
            K::Wind,
            K::LiveSeismic,
            K::Other,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| LoadCase {
            id: LoadCaseId(i as u32),
            name: "任意名称".into(),
            nodal: vec![],
            member: vec![],
            kind,
        })
        .collect()
    }
    #[test]
    fn load_state_uses_saved_terms_and_kind_after_named_roundtrip() {
        let cases = cases();
        for (extra, duration, action) in [
            (vec![], LoadDuration::Long, LoadAction::Gravity),
            (
                vec![(LoadCaseId(2), 0.7)],
                LoadDuration::Long,
                LoadAction::Snow,
            ),
            (
                vec![(LoadCaseId(2), 1.0)],
                LoadDuration::Short,
                LoadAction::Snow,
            ),
            (
                vec![(LoadCaseId(4), -1.0)],
                LoadDuration::Short,
                LoadAction::Wind,
            ),
            (
                vec![(LoadCaseId(2), 0.35), (LoadCaseId(4), 1.0)],
                LoadDuration::Short,
                LoadAction::Wind,
            ),
            (
                vec![(LoadCaseId(2), 0.35), (LoadCaseId(3), -1.0)],
                LoadDuration::Short,
                LoadAction::Seismic,
            ),
        ] {
            for name in ["常時", "LOADCASE", "DL + LL", "任意に改名したケース"] {
                let mut terms = vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)];
                terms.extend(extra.clone());
                let combo = LoadCombination {
                    name: name.into(),
                    terms,
                };
                let bytes = rmp_serde::to_vec_named(&combo).unwrap();
                let saved: LoadCombination = rmp_serde::from_slice(&bytes).unwrap();
                assert_eq!(
                    combination_design_state(&saved, &cases).unwrap(),
                    DesignLoadState {
                        duration,
                        action,
                        combination: true
                    }
                );
            }
        }
        for extra in [
            vec![(LoadCaseId(2), 0.65)],
            vec![(LoadCaseId(5), 1.0)],
            vec![(LoadCaseId(6), 1.0)],
            vec![(LoadCaseId(3), 1.0), (LoadCaseId(4), 1.0)],
            vec![(LoadCaseId(99), 1.0)],
        ] {
            let mut terms = vec![(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0)];
            terms.extend(extra);
            assert!(combination_design_state(
                &LoadCombination {
                    name: "DL + LL".into(),
                    terms
                },
                &cases
            )
            .is_err());
        }
        for (kind, duration) in [
            (K::Dead, LoadDuration::Long),
            (K::Snow, LoadDuration::Short),
            (K::Wind, LoadDuration::Short),
            (K::Seismic, LoadDuration::Short),
        ] {
            let state = case_design_state(kind).unwrap();
            assert_eq!(state.duration, duration);
            assert!(!state.combination);
        }
        assert!(case_design_state(K::Other).is_err());
        assert!(case_design_state(K::LiveSeismic).is_err());
    }

    #[test]
    fn office_10m2_and_actual_input_keep_purpose_columns() {
        for (purpose, kn) in [
            (LoadPurpose::Floor, 29.0),
            (LoadPurpose::Beam, 29.0),
            (LoadPurpose::Frame, 18.0),
            (LoadPurpose::Seismic, 8.0),
        ] {
            assert!(
                (SlabUsage::Office.live_load(purpose) * 10_000_000.0 / 1000.0 - kn).abs() < 1e-12
            );
        }
        let actual = SlabUsage::Custom {
            floor: 0.0033,
            beam: 0.0031,
            frame: 0.0022,
            seismic: 0.0009,
        };
        let saved: SlabUsage =
            rmp_serde::from_slice(&rmp_serde::to_vec_named(&actual).unwrap()).unwrap();
        for (purpose, kn) in [
            (LoadPurpose::Floor, 33.0),
            (LoadPurpose::Beam, 31.0),
            (LoadPurpose::Frame, 22.0),
            (LoadPurpose::Seismic, 9.0),
        ] {
            assert!((saved.live_load(purpose) * 10_000_000.0 / 1000.0 - kn).abs() < 1e-12);
        }
    }
}
