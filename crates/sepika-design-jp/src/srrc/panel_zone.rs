//! SRC規準20条の柱梁接合部の許容せん断と応力伝達の検討省略条件。

use crate::rc::joint::JointShape;
use crate::{CheckComponent, CheckKind, CheckResult, LoadTerm};

/// 検討構面内の梁種別と力の作用点間距離。寸法は mm。
#[derive(Clone, Copy)]
pub enum SrcPanelBeam {
    Src { width: f64, m_bd: f64, s_bd: f64 },
    Rc { width: f64, m_bd: f64 },
    Steel { s_bd: f64 },
}

/// 荷重期間に必要な材料・接合部入力。応力度は N/mm²、比は小数。
#[derive(Clone, Copy)]
pub enum SrcPanelTerm {
    Long {
        n_ratio: f64,
    },
    Short {
        hoop_ratio: f64,
        hoop_allowable_tension: f64,
        web_allowable_shear: f64,
    },
}

/// 普通コンクリートの矩形SRC柱の接合部入力。長さは mm、Fc は N/mm²。
pub struct SrcPanelInput {
    pub shape: JointShape,
    pub beam: SrcPanelBeam,
    pub term: SrcPanelTerm,
    pub fc: f64,
    pub col_width: f64,
    pub m_cd: f64,
    pub j_tw: f64,
    pub s_cd: f64,
    /// 柱内法高さと階高の比 h′/h。
    pub h_ratio: f64,
    /// 各梁が接合部に及ぼす柱面曲げモーメントの符号付き合計 [N·mm]。
    pub sum_beam_moments: f64,
}

/// パネルの許容値・設計値 [N·mm] と採用式番号。
pub struct SrcPanelResult {
    pub allowable_moment_n_mm: f64,
    pub design_moment_n_mm: f64,
    pub equation: &'static str,
}

impl SrcPanelResult {
    pub(crate) fn into_check_result(self, shape: JointShape, term: LoadTerm) -> CheckResult {
        let shape = match shape {
            JointShape::Cross => "十字形",
            JointShape::Tee => "T字形",
            JointShape::Knee => "ト字形",
            JointShape::Corner => "L字形",
        };
        let term = match term {
            LoadTerm::Long => "長期",
            LoadTerm::Short => "短期",
        };
        CheckResult {
            basis: format!("SRC規準20条 {} {shape}・{term}", self.equation),
            detail: String::new(),
            components: vec![CheckComponent {
                kind: CheckKind::Shear,
                ratio: self.design_moment_n_mm / self.allowable_moment_n_mm,
                detail: format!(
                    "許容値 {:.3} kN·m、設計値 {:.3} kN·m",
                    self.allowable_moment_n_mm / 1e6,
                    self.design_moment_n_mm / 1e6
                ),
            }],
        }
    }
}

/// SRC規準20条のパネルせん断を計算する。適用範囲外・不正な諸元は理由付きエラー。
pub fn src_panel_zone_check(inp: &SrcPanelInput) -> Result<SrcPanelResult, String> {
    if !positive([inp.fc, inp.col_width, inp.m_cd, inp.j_tw, inp.s_cd])
        || !positive([inp.h_ratio])
        || inp.h_ratio >= 1.0
        || !inp.sum_beam_moments.is_finite()
    {
        return Err("Fc・柱とウェブの寸法・階高と内法の比・柱面曲げの諸元が不正です".into());
    }
    let (concrete_depth, effective_width, steel_depth) = match inp.beam {
        SrcPanelBeam::Src { width, m_bd, s_bd } if positive([width, m_bd, s_bd]) => {
            (m_bd, (inp.col_width + width) / 2.0, Some(s_bd))
        }
        SrcPanelBeam::Rc { width, m_bd } if positive([width, m_bd]) => {
            (m_bd, (inp.col_width + width) / 2.0, None)
        }
        SrcPanelBeam::Steel { s_bd } if positive([s_bd]) => (s_bd, inp.col_width / 2.0, Some(s_bd)),
        _ => return Err("梁幅または主筋・鉄骨フランジ間距離が不正です".into()),
    };
    let cv = inp.col_width * concrete_depth * inp.m_cd;
    let design_moment_n_mm = (inp.h_ratio * inp.sum_beam_moments).abs();
    let (allowable_moment_n_mm, equation) = match inp.term {
        SrcPanelTerm::Long { n_ratio } => {
            if !positive([n_ratio]) {
                return Err("鉄骨とコンクリートのヤング係数比 n が未確定または不正です".into());
            }
            let beta = n_ratio * inp.j_tw * inp.s_cd / (inp.col_width * inp.m_cd);
            let fs = crate::rc::concrete_allowable_shear(inp.fc, true);
            (cv * 3.0 * fs * (1.0 + beta), "(20.1)")
        }
        SrcPanelTerm::Short {
            hoop_ratio,
            hoop_allowable_tension,
            web_allowable_shear,
        } => {
            let s_bd = steel_depth.ok_or(
                "RC梁の短期式(20.2)に用いる梁鉄骨フランジ間距離に代わる有効高さと鉄骨ウェブへの応力伝達が未確認です",
            )?;
            if !positive([hoop_ratio, hoop_allowable_tension, web_allowable_shear])
                || hoop_ratio >= 1.0
            {
                return Err("接合部の帯筋比・帯筋またはウェブの許容応力度が不正です".into());
            }
            let j_delta = match inp.shape {
                JointShape::Cross => 3.0,
                JointShape::Tee | JointShape::Knee => 2.0,
                JointShape::Corner => 1.0,
            };
            let cve = effective_width * concrete_depth * inp.m_cd;
            let steel = inp.j_tw * s_bd * inp.s_cd * web_allowable_shear;
            let hoops = hoop_ratio * hoop_allowable_tension;
            let fs = crate::rc::concrete_allowable_shear(inp.fc, false);
            let crack_stress = inp.fc / 10.0;
            if matches!(inp.beam, SrcPanelBeam::Steel { .. })
                && cve * (j_delta * crack_stress + hoops) < cv * crack_stress
            {
                (cv * 2.0 * fs + steel, "(解20.12)")
            } else {
                (cve * (2.0 * fs * j_delta + hoops) + steel, "(20.2)")
            }
        }
    };
    if !positive([allowable_moment_n_mm])
        || !(design_moment_n_mm / allowable_moment_n_mm).is_finite()
    {
        return Err("許容値または検定比を有限に算定できません".into());
    }
    Ok(SrcPanelResult {
        allowable_moment_n_mm,
        design_moment_n_mm,
        equation,
    })
}

fn positive<const N: usize>(values: [f64; N]) -> bool {
    values.into_iter().all(|x| x.is_finite() && x > 0.0)
}

/// 確認済みの柱側・梁側曲げ耐力和 [N·mm] に対する省略条件。RC梁ではRC部分、それ以外は鉄骨部分を用いる。
pub fn src_panel_transfer_omission(
    beam: SrcPanelBeam,
    column_moments_n_mm: f64,
    beam_moments_n_mm: f64,
) -> Result<(f64, bool), String> {
    let ratio = column_moments_n_mm / beam_moments_n_mm;
    if !positive([column_moments_n_mm, beam_moments_n_mm, ratio]) {
        return Err("応力伝達の省略判定に必要な柱・梁の曲げ耐力和を確認できません".into());
    }
    let can_omit = match beam {
        SrcPanelBeam::Rc { .. } => ratio >= 0.4,
        _ => (0.4..=2.5).contains(&ratio),
    };
    Ok((ratio, can_omit))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: SrcPanelInput = SrcPanelInput {
        shape: JointShape::Cross,
        beam: SrcPanelBeam::Src {
            width: 300.0,
            m_bd: 500.0,
            s_bd: 400.0,
        },
        term: SrcPanelTerm::Long { n_ratio: 15.0 },
        fc: 24.0,
        col_width: 600.0,
        m_cd: 500.0,
        j_tw: 10.0,
        s_cd: 400.0,
        h_ratio: 0.8,
        sum_beam_moments: -300e6,
    };
    const SHORT: SrcPanelTerm = SrcPanelTerm::Short {
        hoop_ratio: 0.002,
        hoop_allowable_tension: 295.0,
        web_allowable_shear: 135.67731325956206,
    };

    #[test]
    fn long_has_the_same_three_times_fs_for_all_shapes_and_uses_beam_lever_arm() {
        for shape in [
            JointShape::Cross,
            JointShape::Tee,
            JointShape::Knee,
            JointShape::Corner,
        ] {
            for (beam, expected_knm) in [
                (INPUT.beam, 394.2),
                (
                    SrcPanelBeam::Rc {
                        width: 300.0,
                        m_bd: 500.0,
                    },
                    394.2,
                ),
                (SrcPanelBeam::Steel { s_bd: 400.0 }, 315.36),
            ] {
                let result = src_panel_zone_check(&SrcPanelInput {
                    shape,
                    beam,
                    ..INPUT
                })
                .unwrap();
                assert_eq!(result.equation, "(20.1)");
                assert!((result.allowable_moment_n_mm / 1e6 - expected_knm).abs() < 1e-9);
                assert_eq!(result.design_moment_n_mm, 240e6);
            }
        }
    }

    #[test]
    fn short_src_and_steel_beam_have_independent_section_and_shape_capacities() {
        for (shape, src_knm, steel_knm, equation) in [
            (
                JointShape::Cross,
                1022.5837012152993,
                646.6837012152993,
                "(20.2)",
            ),
            (
                JointShape::Tee,
                776.2087012152993,
                515.2837012152993,
                "(20.2)",
            ),
            (
                JointShape::Knee,
                776.2087012152993,
                515.2837012152993,
                "(20.2)",
            ),
            (
                JointShape::Corner,
                529.8337012152993,
                479.8837012152993,
                "(解20.12)",
            ),
        ] {
            for (beam, expected_knm, equation) in [
                (INPUT.beam, src_knm, "(20.2)"),
                (SrcPanelBeam::Steel { s_bd: 400.0 }, steel_knm, equation),
            ] {
                let result = src_panel_zone_check(&SrcPanelInput {
                    shape,
                    beam,
                    term: SHORT,
                    ..INPUT
                })
                .unwrap();
                assert_eq!(result.equation, equation);
                assert!((result.allowable_moment_n_mm / 1e6 - expected_knm).abs() < 1e-9);
                assert_eq!(result.design_moment_n_mm, 240e6);
            }
            let rc = src_panel_zone_check(&SrcPanelInput {
                shape,
                beam: SrcPanelBeam::Rc {
                    width: 300.0,
                    m_bd: 500.0,
                },
                term: SHORT,
                ..INPUT
            });
            assert!(rc.err().unwrap().contains("RC梁の短期式(20.2)"));
        }
    }

    #[test]
    fn aij_example_10_short_and_steel_transfer() {
        let input = SrcPanelInput {
            shape: JointShape::Cross,
            beam: SrcPanelBeam::Src {
                width: 550.0,
                m_bd: 635.0,
                s_bd: 583.0,
            },
            term: SrcPanelTerm::Short {
                hoop_ratio: 0.00075,
                hoop_allowable_tension: 295.0,
                web_allowable_shear: 187.6,
            },
            fc: 24.0,
            col_width: 800.0,
            m_cd: 1840.0 / 3.0,
            j_tw: 10.0,
            s_cd: 484.0,
            h_ratio: 3150.0 / 4000.0,
            sum_beam_moments: 2000e6,
        };
        let result = src_panel_zone_check(&input).unwrap();
        assert_eq!(result.design_moment_n_mm, 1575e6);
        assert!((result.allowable_moment_n_mm / 1e6 - 2314.7063845).abs() < 1e-9);
        assert_eq!((result.allowable_moment_n_mm / 1e6).round(), 2315.0);
        assert_eq!(result.equation, "(20.2)");
        let (ratio, can_omit) =
            src_panel_transfer_omission(input.beam, 620.75e6, 841.75e6).unwrap();
        assert!((ratio - 0.7374517374517374).abs() < 1e-12);
        assert!(can_omit);
    }

    #[test]
    fn transfer_omission_has_inclusive_limits_and_rc_has_no_upper_limit() {
        let rc = SrcPanelBeam::Rc {
            width: 300.0,
            m_bd: 500.0,
        };
        for (capacity, steel_expected, rc_expected) in [
            (39.0, false, false),
            (40.0, true, true),
            (250.0, true, true),
            (251.0, false, true),
        ] {
            assert_eq!(
                src_panel_transfer_omission(INPUT.beam, capacity, 100.0)
                    .unwrap()
                    .1,
                steel_expected
            );
            assert_eq!(
                src_panel_transfer_omission(rc, capacity, 100.0).unwrap().1,
                rc_expected
            );
        }
        assert!(src_panel_transfer_omission(INPUT.beam, 250.0, 0.0).is_err());
    }

    #[test]
    fn missing_or_degenerate_input_is_not_a_zero_ratio() {
        for input in [
            SrcPanelInput {
                h_ratio: 1.0,
                ..INPUT
            },
            SrcPanelInput { m_cd: 0.0, ..INPUT },
            SrcPanelInput {
                term: SrcPanelTerm::Short {
                    hoop_ratio: 0.0,
                    hoop_allowable_tension: 295.0,
                    web_allowable_shear: 135.67731325956206,
                },
                ..INPUT
            },
            SrcPanelInput {
                term: SrcPanelTerm::Long { n_ratio: f64::NAN },
                ..INPUT
            },
            SrcPanelInput {
                sum_beam_moments: f64::NAN,
                ..INPUT
            },
        ] {
            assert!(src_panel_zone_check(&input).is_err());
        }
    }
}
