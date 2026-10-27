//! 終局検定（靭性保証型耐震設計指針）の結果表示ビュー。
//!
//! RC 矩形部材について、塑性理論式による終局せん断強度 Qsu・付着割裂耐力 Qbu と、
//! 両端ヒンジ時せん断力 Qmu との余裕度（Qsu/Qmu, Qbu/Qmu）を一覧表示する。
//! 算定本体は [`sepika_design_jp::ultimate`]、部材内力（軸力）の収集は
//! [`crate::app::App::compute_ultimate_checks`] による。

use crate::app::{member_kind_label, App};
use crate::table_util::Col;
use sepika_core::units::to_display::{force_kn, moment_kn_m};

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
        return Ok("静的重力参照: N/M/QL は保存長期組合せの応力、Rp は一律指定。両端ヒンジ略算は全体の保有水平耐力判定を完了しません".into());
    }
    let direction = app.core.scoped.pushover_view_dir;
    let point = app
        .pushover_for(direction)
        .and_then(|run| run.capacity_evaluation.as_ref())
        .ok_or("確定総応答の採用方向・stepが未指定です")?;
    Ok(format!("確定総応答: {:?}方向・採用step={} のN/M/Q/Rp（同一runの初期重力を含む）。倍率1、総QへQLを追加しません", point.direction, point.step))
}

/// 終局検定表を描画する（設計タブ「終局検定」サブビュー）。
pub fn ultimate_table(ui: &mut egui::Ui, app: &mut App) {
    ui.strong("終局検定（塑性理論式による終局せん断・付着余裕度）");
    ui.add_space(4.0);
    let mut selection = app.core.model.ultimate_gravity_combination;
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
    ui.label(
        demand_source_note(app).unwrap_or_else(|reason| format!("需要由来は未検定: {reason}")),
    );

    ui.horizontal(|ui| {
        ui.label("ヒンジ回転角 Rp:");
        ui.add(
            egui::DragValue::new(&mut app.core.ultimate_rp)
                .speed(0.001)
                .range(0.0..=0.1)
                .fixed_decimals(3),
        )
        .on_hover_text(
            "終局限界状態でのヒンジ領域の回転角 Rp [rad]。ν=(1−15Rp)ν0、\
             cotφ=2−50Rp に効きます。0 で塑性化前（cotφ=2.0, ν=ν0）の終局強度。",
        );
        ui.separator();
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
        ui.checkbox(
            &mut app.core.ultimate_lightweight,
            "軽量コンクリート（Qsu・Qbu を 0.9 倍）",
        )
        .on_hover_text("軽量コンクリート使用時のせん断終局耐力 0.9 倍低減（共通事項）。");
        ui.checkbox(&mut app.core.ultimate_include_bond, "付着割裂 Qbu を検定");
    });
    ui.horizontal(|ui| {
        ui.label("終局せん断強度:");
        ui.selectable_value(
            &mut app.core.ultimate_shear_ductility,
            false,
            "塑性理論式(Qsu)",
        )
        .on_hover_text(
            "藤井・森田式系の塑性理論式（終局強度型設計指針）で終局せん断強度を算定します。",
        );
        ui.selectable_value(
            &mut app.core.ultimate_shear_ductility,
            true,
            "靭性指針式(Vu)",
        )
        .on_hover_text(
            "AIJ「靭性保証型耐震設計指針」6.4 の Vu=min(Vu1,Vu2,Vu3)（トラス＋アーチ機構）\
                 で終局せん断信頼強度を算定します。Qsu 列に Vu を表示します。",
        );
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.core.ultimate_biaxial_shear, "柱を2軸せん断で検定")
            .on_hover_text(
                "RC 柱のせん断余裕度を 2 軸せん断 1/((Qmx/Qux)²+(Qmy/Quy)²)^(1/2) として\
                 検定します（採用応力）。Qsu/Qmu 列は 2 軸合成値を表示します。",
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
            "設計用応力・Rp を増分解析応答から反映",
        )
        .on_hover_text(
            "設計用せん断 Qmu・需要曲げ・軸力・部材別 Rp を、明示した採用stepの\
             確定総応答から直接反映します。不足時は未検定です。",
        );
        if app.core.ultimate_use_pushover {
            let has_po = app.ultimate_demand_by_elem().is_ok();
            if has_po {
                ui.colored_label(crate::theme::GOOD_GREEN, "● 応答反映中");
            } else {
                ui.colored_label(crate::theme::GRAY_600, "（確定総応答が不足・未検定）");
            }
        }
    });
    ui.separator();

    match app.compute_ultimate_checks() {
        Err(msg) => {
            ui.colored_label(crate::theme::GRAY_600, &msg);
            let needs_analysis = msg.contains("静的") || msg.contains("解析");
            if needs_analysis && ui.button("▶ 解析タブへ").clicked() {
                app.ui.view.active_tab = crate::app::Tab::Analysis;
            }
        }
        Ok(checks) => {
            let ng = checks.iter().filter(|c| !c.ok).count();
            ui.horizontal(|ui| {
                ui.label(format!("対象部材 {} 本", checks.len()));
                if ng > 0 {
                    ui.colored_label(crate::theme::ERROR_RED, format!("NG {} 本", ng));
                } else {
                    ui.colored_label(
                        crate::theme::GOOD_GREEN,
                        "対象部材の限定検定 OK（全体判定は未完了）",
                    );
                }
            });
            ui.add_space(4.0);

            let bond = app.core.ultimate_include_bond;
            let (qsu_hdr, ratio_hdr, qbu_hdr, bond_ratio_hdr) = if app.core.ultimate_shear_ductility
            {
                ("Vu[kN]", "Vu/Qmu", "Vbu[kN]", "Vbu/Qmu")
            } else {
                ("Qsu[kN]", "Qsu/Qmu", "Qbu[kN]", "Qbu/Qmu")
            };
            crate::table_util::standard_table(
                ui,
                "ultimate_checks",
                &[
                    Col::id_named("部材"),
                    Col::label("種別"),
                    Col::num("Mu[kN·m]"),
                    Col::num("Qmu[kN]"),
                    Col::num(qsu_hdr),
                    Col::num(ratio_hdr),
                    Col::num(qbu_hdr),
                    Col::num(bond_ratio_hdr),
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
                        ui.label(member_kind_label(c.kind));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.1}", moment_kn_m(c.mu)));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.1}", force_kn(c.qmu)));
                    });
                    row.col(|ui| {
                        ui.label(format!("{:.1}", force_kn(c.qsu)));
                    });
                    row.col(|ui| {
                        let m = c.biaxial_shear_margin.unwrap_or(c.shear_margin);
                        ui.colored_label(margin_color(m), format!("{m:.2}"));
                    });
                    row.col(|ui| {
                        if bond {
                            ui.label(format!("{:.1}", force_kn(c.qbu)));
                        } else {
                            ui.label("-");
                        }
                    });
                    row.col(|ui| {
                        if bond {
                            ui.colored_label(
                                margin_color(c.bond_margin),
                                format!("{:.2}", c.bond_margin),
                            );
                        } else {
                            ui.label("-");
                        }
                    });
                    row.col(|ui| {
                        if c.ok {
                            ui.colored_label(crate::theme::GOOD_GREEN, "OK");
                        } else {
                            ui.colored_label(crate::theme::ERROR_RED, "NG");
                        }
                    });
                },
            );

            ui.add_space(4.0);
            let (shear_note, bond_note) = if app.core.ultimate_shear_ductility {
                (
                    "Vu=靭性指針式の終局せん断信頼強度 min(Vu1,Vu2,Vu3)（トラス＋アーチ機構）",
                    "Vbu=付着考慮せん断信頼強度 min(Vbu1,Vbu2)",
                )
            } else {
                ("Qsu=塑性理論式の終局せん断強度", "Qbu=付着割裂耐力")
            };
            let source = demand_source_note(app)
                .unwrap_or_else(|reason| format!("需要由来は未検定: {reason}"));
            let shear_demand = if app.core.ultimate_use_pushover {
                "Qmu は採用stepの確定総せん断力の絶対値"
            } else {
                "Qmu=上限強度倍率·2·Mu/内法（両端ヒンジ）"
            };
            ui.colored_label(
                crate::theme::GRAY_600,
                format!(
                    "{source}。{shear_demand}。{shear_note}、{bond_note}。\
                     余裕度<1.0（赤）はせん断・付着が曲げ降伏に先行することを示す。\
                     対象は RC梁・RC矩形柱・RC円形柱。"
                ),
            );
        }
    }

    ui.add_space(12.0);
    ui.strong("CFT柱の軸終局耐力（CFT指針）");
    ui.add_space(4.0);
    match app.compute_cft_ultimate_checks() {
        Err(msg) => {
            ui.colored_label(crate::theme::GRAY_600, &msg);
        }
        Ok(checks) => {
            let ng = checks.iter().filter(|c| !c.ok).count();
            ui.horizontal(|ui| {
                ui.label(format!("対象 CFT 柱 {} 本", checks.len()));
                if ng > 0 {
                    ui.colored_label(crate::theme::ERROR_RED, format!("NG {} 本", ng));
                } else {
                    ui.colored_label(crate::theme::GOOD_GREEN, "全柱 OK");
                }
            });
            ui.add_space(4.0);
            crate::table_util::standard_table(
                ui,
                "cft_ultimate_checks",
                &[
                    Col::id_named("部材"),
                    Col::label("分類"),
                    Col::num("Ncu[kN]"),
                    Col::num("Ntu[kN]"),
                    Col::num("Mu(N-M)[kN·m]"),
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
                crate::theme::GRAY_600,
                format!("Ncu=軸圧縮終局耐力（短柱=cNc+(1+ξ)sNc、長柱=座屈耐力、中柱=線形補間）、\
                 Ntu=軸引張終局耐力（sA·Fy）。N は採用需要の軸力（圧縮正）。{source}。座屈長さは幾何長（K=1）。"),
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
mod tests {
    use super::*;
    use sepika_core::ids::LoadCaseId;
    use sepika_core::model::{LoadCase, LoadCaseKind, LoadCombination};
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    use sepika_solver::statics::analysis::SeismicDir;

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
