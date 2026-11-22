//! RC部材のせん断・付着余裕度と CFT 柱の軸終局耐力の結果表示。

use crate::app::{member_kind_label, App};
use crate::table_util::Col;
use sepika_core::units::to_display::{force_kn, moment_kn_m};
use sepika_design_jp::ultimate::UltimateDemandMode;

/// 余裕度セルの色（1.0 未満＝せん断先行で NG を赤系に）。
fn margin_color(margin: f64) -> egui::Color32 {
    if margin.is_finite() {
        crate::theme::status_color(if margin > 1e-9 { 1.0 / margin } else { 9.9 })
    } else {
        crate::theme::GOOD_GREEN
    }
}

fn demand_source_note(app: &App) -> Result<String, String> {
    app.ultimate_demand_by_elem()?;
    if !app.core.ultimate_use_pushover {
        return Ok("静的重力参照: N/M/QL は保存長期組合せの応力。両端ヒンジ略算は全体の保有水平耐力判定を完了しません".into());
    }
    let direction = app.core.scoped.pushover_view_dir;
    let point = app
        .pushover_for(direction)
        .and_then(|run| run.capacity_evaluation.as_ref())
        .ok_or("確定総応答の採用方向・stepが未指定です")?;
    Ok(format!("確定総応答: {:?}方向・採用step={} のN/M/Q（同一runの初期重力を含む）。倍率1、総QへQLを追加しません", point.direction, point.step))
}

/// 終局検定表を描画する（設計タブ「終局検定」サブビュー）。
pub fn ultimate_table(ui: &mut egui::Ui, app: &mut App) {
    ui.strong("RC部材の終局検定（せん断・付着余裕度）");
    ui.add_space(4.0);
    let mut selection = app.core.model.ultimate_gravity_combination;
    ui.add_enabled_ui(app.study_is_main(), |ui| {
        egui::ComboBox::from_id_salt("ultimate_gravity_combination")
            .selected_text(
                selection
                    .and_then(|i| app.core.model.combinations.get(i))
                    .map(|c| c.name.as_str())
                    .unwrap_or("長期架構用組合せを選択"),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selection, None, "未指定（未検定）");
                for (index, combo) in app.core.model.combinations.iter().enumerate() {
                    ui.selectable_value(&mut selection, Some(index), &combo.name);
                }
            });
    });
    if selection != app.core.model.ultimate_gravity_combination {
        app.core.scoped.undo.run(
            &mut app.core.model,
            Box::new(sepika_edit::SetUltimateGravityCombination { index: selection }),
        );
        app.core.scoped.staleness.mark_edited();
    }
    if let Ok(terms) = sepika_core::load_combo::ultimate_gravity_terms(&app.core.model) {
        let sources = terms
            .iter()
            .map(|(id, factor)| {
                let name = app
                    .core
                    .model
                    .load_cases
                    .iter()
                    .find(|case| case.id == *id)
                    .map(|case| case.name.as_str())
                    .unwrap_or("不明");
                format!("{factor} × {name} (case {})", id.0)
            })
            .collect::<Vec<_>>()
            .join(" + ");
        ui.label(format!("長期架構用: {sources}"));
    }
    ui.add_enabled_ui(app.study_is_main(), |ui| {
        ui.horizontal(|ui| {
            ui.label("上限強度倍率:");
            ui.add(
                egui::DragValue::new(&mut app.core.ultimate_upper_factor)
                    .speed(0.05)
                    .range(0.1..=2.0)
                    .fixed_decimals(2),
            )
            .on_hover_text("Qmu = 上限強度倍率·2·Mu/内法 の倍率。");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.core.ultimate_include_bond, "大地震時の付着を照査");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.core.ultimate_biaxial_shear, "柱を2軸せん断で検定")
                .on_hover_text(
                    "確定総応答を採用し、両軸の耐力を確認できる場合に、初期重力を含む両軸の総せん断力で算定します。",
                );
            ui.checkbox(&mut app.core.ultimate_biaxial_bending, "柱を2軸曲げで検定")
                .on_hover_text(
                    "RC 柱の曲げ余裕度を 2 軸曲げ 1/((Mmx/Mux)²+(Mmy/Muy)²)^(1/2) として検定します\
                 （採用応力）。需要曲げは応答（増分解析／静的）の値を用います。",
                );
        });
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut app.core.ultimate_use_pushover,
                "設計用応力を増分解析応答から反映",
            )
            .on_hover_text(
                "総せん断力・需要曲げ・軸力を、明示した採用stepの\
             確定総応答から直接反映します。不足時は未検定です。",
            );
            if app.core.ultimate_use_pushover {
                let has_po = app.ultimate_demand_by_elem().is_ok();
                if has_po {
                    ui.colored_label(crate::theme::GOOD_GREEN, "● 応答反映中");
                } else {
                    ui.colored_label(crate::theme::UI_MUTED, "（確定総応答が不足・未検定）");
                }
            }
        });
        ui.separator();
    });
    ui.label(
        demand_source_note(app).unwrap_or_else(|reason| format!("需要由来は未検定: {reason}")),
    );
    match app.compute_ultimate_checks() {
        Err(msg) => {
            ui.colored_label(crate::theme::UI_MUTED, format!("未検定（{msg}）"));
            let needs_analysis = msg.contains("静的") || msg.contains("解析");
            if needs_analysis && ui.button("▶ 解析タブへ").clicked() {
                app.ui.view.active_tab = crate::app::Tab::Analysis;
            }
        }
        Ok(checks) => {
            let unchecked = checks
                .iter()
                .filter(|c| c.unchecked_reason.is_some())
                .count();
            let ng = checks
                .iter()
                .filter(|c| !c.ok && c.unchecked_reason.is_none())
                .count();
            ui.horizontal(|ui| {
                ui.label(format!("対象部材 {} 本", checks.len()));
                if ng > 0 {
                    ui.colored_label(crate::theme::ERROR_RED, format!("NG {} 本", ng));
                }
                if unchecked > 0 {
                    ui.colored_label(crate::theme::UI_MUTED, format!("未検定 {} 本", unchecked))
                        .on_hover_text(
                            "各部材の「未検定」にカーソルを合わせると理由を確認できます。",
                        );
                }
                if ng == 0 && unchecked == 0 {
                    ui.colored_label(
                        crate::theme::GOOD_GREEN,
                        "対象部材の限定検定 OK（全体判定は未完了）",
                    );
                }
            });
            ui.add_space(4.0);

            let include_bond = app.core.ultimate_include_bond;
            let biaxial_shear = app.core.ultimate_biaxial_shear;
            let mode = if app.core.ultimate_use_pushover {
                UltimateDemandMode::ConfirmedTotal
            } else {
                UltimateDemandMode::StaticHinge
            };
            let shear_symbol = "Qsu";
            let demand_symbol = mode.strong_shear_demand_symbol();
            let (table_id, demand_note) = match mode {
                UltimateDemandMode::StaticHinge => (
                    "ultimate_checks_static_hinge",
                    "静的・両端ヒンジ略算。QL は長期せん断力の最大絶対値です。RC柱の参考需要 Qmu は上限強度倍率·2·Mu/内法で求めます。終局せん断耐力と余裕度の算定には確定総応答が必要です。",
                ),
                UltimateDemandMode::ConfirmedTotal => (
                    "ultimate_checks_confirmed_total",
                    "増分・確定総応答。|Qtotal| は採用stepの総せん断力の絶対値（初期重力込み）。",
                ),
            };
            ui.label(demand_note);
            if mode == UltimateDemandMode::ConfirmedTotal {
                ui.label("強軸せん断余裕度 Qsu/|Qtotal|。付着余裕度 Kfb/τD");
            }
            crate::table_util::standard_table(
                ui,
                table_id,
                &[
                    Col::id_named("部材"),
                    Col::label("種別"),
                    Col::wide_num("Mu[kN·m]（梁は参考）"),
                    Col::num(&format!("{demand_symbol}[kN]")),
                    Col::num(&format!("{shear_symbol}[kN]")),
                    Col::num("せん断余裕度"),
                    Col::num("τD[N/mm²]"),
                    Col::num("Kfb[N/mm²]"),
                    Col::num("付着余裕度"),
                    Col::label("検定結果"),
                ],
                checks.len(),
                |row| {
                    let i = row.index();
                    let c = &checks[i];
                    row.col(|ui| {
                        ui.label(format!("{}", c.elem.0)).on_hover_text(&c.detail);
                    });
                    row.col(|ui| {
                        ui.label(member_kind_label(c.kind));
                    });
                    let format_capacity = |value: f64| {
                        if value.is_nan() {
                            "-".into()
                        } else {
                            format!("{value:.1}")
                        }
                    };
                    row.col(|ui| {
                        let value = format_capacity(moment_kn_m(c.mu));
                        if c.kind == sepika_design_jp::MemberKind::Girder {
                            let value = if c.mu.is_finite() {
                                format!("{value}（参考）")
                            } else {
                                value
                            };
                            ui.label(value).on_hover_text(&c.detail);
                        } else {
                            ui.label(value);
                        }
                    });
                    row.col(|ui| {
                        ui.label(format_capacity(force_kn(c.qmu)));
                    });
                    row.col(|ui| {
                        ui.label(format_capacity(force_kn(c.qsu)));
                    });
                    row.col(|ui| {
                        if c.shear_margin.is_nan() {
                            ui.label("-");
                        } else if let Some(m) = c.biaxial_shear_margin {
                            ui.colored_label(margin_color(m), format!("二軸 {m:.2}"));
                        } else if biaxial_shear && c.kind == sepika_design_jp::MemberKind::Column {
                            let m = c.shear_margin;
                            let color = if m < 1.0 {
                                crate::theme::ERROR_RED
                            } else {
                                crate::theme::UI_MUTED
                            };
                            ui.colored_label(color, format!("強軸 {m:.2}"));
                        } else {
                            let m = c.shear_margin;
                            ui.colored_label(margin_color(m), format!("{m:.2}"));
                        }
                    });
                    row.col(|ui| {
                        if !include_bond {
                            ui.label("無効");
                        } else if let Some(stress) = c.bond_stress {
                            ui.label(format!("{:.3}", stress.demand_mpa));
                        } else {
                            ui.label("未算定");
                        }
                    });
                    row.col(|ui| {
                        if !include_bond {
                            ui.label("無効");
                        } else if let Some(stress) = c.bond_stress {
                            ui.label(format!("{:.3}", stress.capacity_mpa));
                        } else {
                            ui.label("未算定");
                        }
                    });
                    row.col(|ui| {
                        if !include_bond {
                            ui.label("無効");
                        } else if c.bond_margin.is_nan() {
                            ui.label("未算定");
                        } else {
                            ui.colored_label(
                                margin_color(c.bond_margin),
                                format!("{:.2}", c.bond_margin),
                            );
                        }
                    });
                    row.col(|ui| {
                        if let Some(reason) = &c.unchecked_reason {
                            ui.colored_label(crate::theme::UI_MUTED, "未検定")
                                .on_hover_text(reason);
                        } else if c.ok {
                            ui.colored_label(crate::theme::GOOD_GREEN, "OK");
                        } else {
                            ui.colored_label(crate::theme::ERROR_RED, "NG");
                        }
                    });
                },
            );

            ui.add_space(4.0);
            ui.colored_label(
                crate::theme::UI_MUTED,
                "Qsu=荒川mean式の終局せん断耐力。τDは大地震時の付着応力度、Kfbはその許容値。\
                     余裕度が 1.0 未満（赤）の場合、採用した需要に対して NG。\
                     対象は RC梁・RC矩形柱・RC円形柱。",
            );
        }
    }

    ui.add_space(12.0);
    ui.strong("CFT柱の軸終局耐力（CFT指針）");
    ui.add_space(4.0);
    match app.compute_cft_ultimate_checks() {
        Err(msg) => {
            ui.colored_label(crate::theme::UI_MUTED, format!("未検定（{msg}）"));
        }
        Ok(checks) => {
            let ng = checks.iter().filter(|c| !c.ok).count();
            ui.horizontal(|ui| {
                ui.label(format!("対象 CFT 柱 {} 本", checks.len()));
                if ng > 0 {
                    ui.colored_label(crate::theme::ERROR_RED, format!("軸NG {} 本", ng));
                } else {
                    ui.colored_label(crate::theme::GOOD_GREEN, "全柱 軸OK");
                }
                let mu_unavailable = checks.iter().filter(|c| c.mu_nm.is_err()).count();
                if mu_unavailable > 0 {
                    ui.colored_label(
                        crate::theme::UI_MUTED,
                        format!("曲げ未算定 {} 本", mu_unavailable),
                    );
                }
            });
            ui.add_space(4.0);
            crate::table_util::standard_table(
                ui,
                "cft_ultimate_checks",
                &[
                    Col::id_named("部材"),
                    Col::label("軸圧縮の支配分類"),
                    Col::num("Ncu[kN]"),
                    Col::num("Ntu[kN]"),
                    Col::num("強軸Mu(N-M)[kN·m]"),
                    Col::num("N[kN]"),
                    Col::num("軸余裕度"),
                    Col::label("判定"),
                ],
                checks.len(),
                |row| {
                    let i = row.index();
                    let c = &checks[i];
                    row.col(|ui| {
                        ui.label(format!("{}", c.elem.0)).on_hover_text(&c.detail);
                    });
                    row.col(|ui| {
                        ui.label(cft_class_label(c.class));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}", force_kn(c.ncu)));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}", force_kn(c.ntu)));
                    });
                    row.col(|ui| match &c.mu_nm {
                        Ok(mu) => {
                            ui.label(format!("{:.1}", moment_kn_m(*mu)));
                        }
                        Err(reason) => {
                            ui.label("未算定").on_hover_text(reason);
                        }
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.0}", force_kn(c.n_design)));
                    });
                    row.col(|ui| {
                        ui.colored_label(
                            margin_color(c.axial_margin),
                            format!("{:.2}", c.axial_margin),
                        );
                    });
                    row.col(|ui| {
                        if c.ok {
                            ui.colored_label(crate::theme::GOOD_GREEN, "軸OK");
                        } else {
                            ui.colored_label(crate::theme::ERROR_RED, "軸NG");
                        }
                    });
                },
            );
            ui.add_space(4.0);
            let source = demand_source_note(app)
                .unwrap_or_else(|reason| format!("需要由来は未検定: {reason}"));
            ui.colored_label(
                crate::theme::UI_MUTED,
                format!("Ncu=軸圧縮終局耐力（短柱=cNc+(1+ξ)sNc、長柱=座屈耐力、中柱=線形補間）、\
                 Ntu=軸引張終局耐力（sA·Fy）。N は採用需要の軸力（圧縮正）。{source}。座屈長さは両軸の直接入力を用います。"),
            );
        }
    }
}

/// CFT 柱分類のラベル。
fn cft_class_label(class: sepika_design_jp::ultimate::CftColumnClass) -> &'static str {
    use sepika_design_jp::ultimate::CftColumnClass;
    match class {
        CftColumnClass::Short => "短柱",
        CftColumnClass::Medium => "中柱",
        CftColumnClass::Long => "長柱",
    }
}

#[cfg(test)]
use crate::app::cft_column_buckling_tests::input as cft_column_buckling_input;
#[cfg(test)]
#[path = "../../sepika-design-jp/tests/fixtures/cft_ultimate_strength_model.rs"]
mod cft_ultimate_strength_input;

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination};
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    use sepika_solver::statics::analysis::SeismicDir;

    fn ultimate_table_text(app: &mut App) -> String {
        egui::Context::default()
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1800.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| ultimate_table(ui, app),
            )
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn circular_cft_ultimate_view_distinguishes_axial_status_from_unavailable_mu() {
        use sepika_core::ids::ElemId;

        for (lk, n, axial_ok, message) in [
            (7200.0, 6_500_000.0, false, "軸圧縮終局耐力"),
            (1200.0, -4_000_000.0, false, "軸引張終局耐力"),
            (
                1200.0,
                -std::f64::consts::PI * (200.0_f64.powi(2) - 188.0_f64.powi(2)) * 235.0,
                true,
                "純引張限界",
            ),
        ] {
            let mut model = cft_column_buckling_input::cantilever(true, n);
            // 3000 mmの入力では弾性解析で復元した引張力が1刻み内側となり、端点に達しない。
            model.nodes[1].coord[2] = 1024.0;
            model.steel_design_attrs = vec![cft_column_buckling_input::direct_lengths(
                ElemId(0),
                Some(lk),
                Some(lk),
            )];
            let mut app = App::default();
            assert!(app.load_model(model));
            app.run_linear_static(LoadCaseId(1));
            app.run_linear_static(LoadCaseId(0));
            let checks = app.compute_cft_ultimate_checks().unwrap();
            let row = &checks[0];
            assert_eq!(row.ok, axial_ok, "{row:?}");
            if axial_ok {
                assert_eq!(row.n_design, -row.ntu, "{row:?}");
            }
            let reason = row.mu_nm.as_ref().unwrap_err();
            assert!(
                reason.contains(message) && row.detail.contains(reason),
                "{row:?}"
            );

            let text = ultimate_table_text(&mut app);
            assert!(
                text.contains(if axial_ok {
                    "全柱 軸OK"
                } else {
                    "軸NG 1 本"
                }),
                "{text}"
            );
            assert!(text.lines().any(|s| s == "未算定"), "{text}");
            assert!(text.contains("曲げ未算定 1 本"), "{text}");
        }
    }

    #[test]
    fn cft_ultimate_rejects_concrete_strength_below_statutory_minimum() {
        for (fc_n_mm2, calculated) in [(23.999, false), (24.0, true), (24.001, true)] {
            let mut app = App::default();
            assert!(app.load_model(cft_ultimate_strength_input::circular_column(fc_n_mm2)));
            app.run_linear_static(LoadCaseId(1));
            app.run_linear_static(LoadCaseId(0));
            assert!(
                app.core.scoped.last_error.is_none(),
                "{:?}",
                app.core.scoped.last_error
            );

            let text = ultimate_table_text(&mut app);
            for label in [
                "全柱 軸OK",
                "軸OK",
                "Ncu[kN]",
                "Ntu[kN]",
                "強軸Mu(N-M)[kN·m]",
            ] {
                assert_eq!(text.contains(label), calculated, "{text}");
            }
            if !calculated {
                for message in [
                    "未検定（CFT 部材 0",
                    "Fc=23.999 N/mm²",
                    "告示464号第三2",
                    "下限24 N/mm²",
                ] {
                    assert!(text.contains(message), "{text}");
                }
            }
            assert_eq!(app.core.model.materials[0].fc, Some(fc_n_mm2));
        }
    }

    #[test]
    fn source_note_follows_nonfinal_confirmed_adoption_without_legacy_display_response() {
        let mut model = crate::sample::portal_frame();
        model.load_cases.push(LoadCase {
            origin: Default::default(),
            id: LoadCaseId(2),
            name: "明示P0".into(),
            kind: LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
        model.combinations = vec![LoadCombination {
            name: "保存架構用".into(),
            terms: vec![(LoadCaseId(0), 1.0), (LoadCaseId(2), 1.0)],
        }];
        model.ultimate_gravity_combination = Some(0);
        let mut app = App::default();
        app.load_model(model);
        app.core.analysis_cfg.threads = 1;
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.core.analysis_cfg.push_steps = 3;
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.core.ultimate_use_pushover = true;
        app.core.ultimate_upper_factor = 1.0;
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let run = bundle.pushover_x.as_mut().unwrap();
        let history = run.confirmed_history.as_ref().unwrap();
        assert!(history.len() >= 3);
        let step = history[1].step;
        assert_ne!(step, history.last().unwrap().step);
        run.capacity_evaluation = Some(
            run.evaluation_point(
                EvaluationPurpose::HoldingCapacity,
                SeismicDir::X,
                step,
                "表示説明回帰の非最終採用点".into(),
            )
            .unwrap(),
        );
        let adopted = demand_source_note(&app).unwrap();
        assert!(adopted.contains("X方向"), "{adopted}");
        assert!(adopted.contains(&format!("step={step}")), "{adopted}");
        assert!(adopted.contains("確定総応答"), "{adopted}");
        assert!(!adopted.contains("終局応答"), "{adopted}");
        let bundle = app.core.scoped.results.as_mut().unwrap();
        let run = bundle.pushover_x.as_mut().unwrap();
        run.member_response.clear();
        assert_eq!(demand_source_note(&app).unwrap(), adopted);
        println!("ultimate source note: {adopted}");
    }
}
