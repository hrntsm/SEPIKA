use crate::app::src_holding::{
    case_label, SrcCollapse, SrcCollapseOrigin, SrcColumnBase, SrcFailure, SrcFrameFaceLimits,
    SrcFrameLayout, SrcMemberEnd, SrcOtherCollapseCases, SRC_HOLDING_CASES,
};
use crate::app::App;
use sepika_core::ids::ElemId;
use sepika_solver::nonlinear::pushover::story_response::{
    EvaluationPoint, EvaluationPurpose, SrcFrameFaceReview,
};
use sepika_solver::nonlinear::pushover::{PushoverResult, PushoverSign};

pub(super) fn inputs(ui: &mut egui::Ui, app: &mut App) {
    let mut direction = app.core.scoped.pushover_view_dir;
    let mut sign = app.core.scoped.src_holding_sign;
    ui.horizontal(|ui| {
        ui.label("SRC加力方向");
        let mut case = (direction, sign);
        for (d, s) in SRC_HOLDING_CASES {
            ui.selectable_value(&mut case, (d, s), case_label(d, s));
        }
        (direction, sign) = case;
    });
    app.set_pushover_view_dir(direction);
    app.core.scoped.src_holding_sign = sign;
    let run_label = case_label(direction, sign);
    if ui.button(format!("{run_label} の増分解析を実行")).clicked() {
        app.start_src_pushover_job(direction, sign);
    }
    let mut adopt = None;
    if let Some(bundle) = &mut app.core.scoped.results {
        let run = match sign {
            PushoverSign::Positive => match direction {
                sepika_solver::statics::analysis::SeismicDir::X => bundle.pushover_x.as_mut(),
                sepika_solver::statics::analysis::SeismicDir::Y => bundle.pushover_y.as_mut(),
            },
            PushoverSign::Negative => bundle
                .src_pushover_negative
                .iter_mut()
                .find(|(d, _, _)| *d == direction)
                .map(|(_, _, p)| p),
        };
        if let Some(run) = run {
            let max_step = run.capacity_curve.last().map_or(0, |p| p.step);
            ui.collapsing("確定stepの層力・変形", |ui| {
                for point in &run.capacity_curve {
                    ui.label(format!(
                        "step {}、頂部変位 {:.3} mm、層せん断力 {:?} kN、層間変位 {:?} mm",
                        point.step,
                        point.roof_disp,
                        point
                            .story_shear
                            .iter()
                            .map(|q| q / 1000.0)
                            .collect::<Vec<_>>(),
                        point.story_drift,
                    ));
                }
            });
            if let Some(reason) = run.shear_design_unavailable_at(max_step) {
                ui.colored_label(crate::theme::SECONDARY_AMBER, reason);
            }
            for (purpose, label) in [
                (EvaluationPurpose::Ds, "Ds"),
                (EvaluationPurpose::HoldingCapacity, "Qu"),
            ] {
                if evaluation_slot(run, purpose).is_none()
                    && ui.button(format!("{label}採用点を指定")).clicked()
                {
                    let start = run
                        .capacity_curve
                        .iter()
                        .find(|p| p.step > 0 && run.shear_design_unavailable_at(p.step).is_none())
                        .map(|p| p.step);
                    if let Some(step) = start {
                        *evaluation_slot(run, purpose) = run
                            .evaluation_point(purpose, direction, step, "選定理由を記入".into())
                            .ok();
                        if let Some(point) = evaluation_slot(run, purpose) {
                            point.selection_reason.clear();
                        }
                    }
                }
                if let Some(point) = evaluation_slot(run, purpose) {
                    ui.horizontal(|ui| {
                        ui.label(format!("{label}確定step"));
                        ui.add(egui::DragValue::new(&mut point.step).range(1..=max_step));
                        ui.text_edit_singleline(&mut point.selection_reason)
                            .on_hover_text("この確定stepを選定した根拠");
                        if ui.button(format!("{label}採用点を保存")).clicked() {
                            adopt = Some(purpose);
                        }
                    });
                }
            }
        }
    }
    if let Some(purpose) = adopt {
        if let Err(reason) = app.adopt_holding_evaluation_for(direction, sign, purpose) {
            app.core.scoped.last_error = Some(reason);
        }
    }
    let saved = app
        .core
        .scoped
        .results
        .as_mut()
        .map(|b| &mut b.holding_evaluations);
    let Some(saved) = saved else {
        return;
    };
    let mut confirmation = None;
    let mut ds_columns: &[SrcFrameFaceReview] = &[];
    let mut qu_columns: &[SrcFrameFaceReview] = &[];
    for item in saved.iter_mut().filter(|e| {
        e.point.direction == direction && e.run.wall_run.as_ref().is_some_and(|id| id.sign == sign)
    }) {
        ui.label(format!(
            "保存 {:?} / run {} / step {} / {}",
            item.point.purpose, item.point.run_id, item.point.step, item.point.selection_reason
        ));
        if item.point.purpose == EvaluationPurpose::Ds {
            confirmation = item.src_confirmation.as_mut();
        }
        if let Ok(response) = item.run.confirmed_response(&item.point, item.point.purpose) {
            match item.point.purpose {
                EvaluationPurpose::Ds => ds_columns = &response.src_frame_faces,
                EvaluationPurpose::HoldingCapacity => qu_columns = &response.src_frame_faces,
            }
        }
    }
    let has_confirmation = confirmation.is_some();
    if let Some(form) = confirmation {
        ui.collapsing(format!("{run_label} の設計者確認"), |ui| {
            ui.label(format!(
                "Ds run {} / step {}、Qu run {} / step {}",
                form.ds_run_id, form.ds_step, form.qu_run_id, form.qu_step
            ));
            field(ui, "設計者名", &mut form.reviewer);
            ui.horizontal(|ui| {
                ui.label("採用崩壊形");
                ui.selectable_value(&mut form.collapse, Some(SrcCollapse::Overall), "全体");
                ui.selectable_value(&mut form.collapse, Some(SrcCollapse::Partial), "部分");
                ui.selectable_value(&mut form.collapse, Some(SrcCollapse::Local), "局部");
            });
            field(
                ui,
                "設計者による機構の成立・崩壊範囲・採用stepの確認資料",
                &mut form.collapse_basis,
            );
            if form.collapse == Some(SrcCollapse::Overall) {
                ui.checkbox(&mut form.wall_base_overall_mechanism, "壁脚曲げで全体崩壊形が成立することを設計者が確認");
                ui.label("部材端の耐力到達と、設計者が示す全体機構の成立を確認してください。");
            }
            let wall_ids: Vec<_> = form.walls.keys().copied().collect();
            let column_ids: Vec<_> = form.columns.keys().copied().collect();
            collapse_origins(
                ui,
                "Ds",
                &mut form.ds_origins,
                ds_columns,
                &column_ids,
                &wall_ids,
            );
            if form.ds_run_id != form.qu_run_id || form.ds_step != form.qu_step {
                collapse_origins(
                    ui,
                    "Qu",
                    &mut form.qu_origins,
                    qu_columns,
                    &column_ids,
                    &wall_ids,
                );
            }
            field(
                ui,
                "同方向の他の崩壊形との比較・採用Quが最小となる根拠",
                &mut form.other_cases_basis,
            );
            ui.horizontal(|ui| {
                ui.label("同方向の他の有効崩壊形");
                ui.selectable_value(
                    &mut form.other_cases,
                    Some(SrcOtherCollapseCases::NoOtherCase),
                    "なしを確認",
                );
                ui.selectable_value(
                    &mut form.other_cases,
                    Some(SrcOtherCollapseCases::ReviewedMinimum),
                    "あり・各層最小を確認",
                );
            });
            field(
                ui,
                "鉛直支持・壁脚拘束・直交梁・ピロティ・部材の耐力・先行脆性破壊と除外条件の確認資料",
                &mut form.additional_conditions_basis,
            );
            confirm(ui, "各採用点以前にSRC柱・梁のせん断破壊が先行していないことを確認", &mut form.no_prior_frame_shear);
            confirm(ui, "脆性部材等の除外なしを確認", &mut form.all_physical_members_counted);
            for proof in &mut form.layers {
                ui.collapsing(format!("層 {}", proof.layer + 1), |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut proof.layout,
                            Some(SrcFrameLayout::MomentFrame),
                            "壁なし剛節架構",
                        );
                        ui.selectable_value(
                            &mut proof.layout,
                            Some(SrcFrameLayout::WallStructure),
                            "壁式",
                        );
                        ui.selectable_value(
                            &mut proof.layout,
                            Some(SrcFrameLayout::MomentFrameWithWalls),
                            "壁付き剛節架構",
                        );
                    });
                    field(
                        ui,
                        "SRC用規定・架構形式・崩壊時応力の確認資料",
                        &mut proof.application_basis,
                    );
                    confirm(
                        ui,
                        "この層のDs用応力を採用崩壊stepで確定",
                        &mut proof.uses_collapse_step_for_ds,
                    );
                    choice(ui, "FD柱除去による局部崩壊", &mut proof.fd_removal);
                    field(ui, "FD柱除去の確認資料", &mut proof.fd_removal_basis);
                    if form.other_cases == Some(SrcOtherCollapseCases::ReviewedMinimum) {
                        capacity(
                            ui,
                            "他の有効崩壊形の最小Qu [kN]",
                            &mut proof.other_cases_min_qu_kn,
                        );
                    }
                });
            }
            for (id, proof) in &mut form.columns {
                ui.collapsing(format!("柱 ID {}", id.0), |ui| {
                    failure(ui, &mut proof.failure, false);
                    choice(
                        ui,
                        "この柱の破壊形式が採用点で発現",
                        &mut proof.failure_reached,
                    );
                    field(ui, "破壊形式・柱脚の確認資料", &mut proof.failure_basis);
                    ui.horizontal(|ui| {
                        ui.label("非埋込み柱脚の曲げ降伏");
                        ui.selectable_value(
                            &mut proof.base,
                            Some(SrcColumnBase::NoUnembeddedBaseYield),
                            "該当なし",
                        );
                        ui.selectable_value(
                            &mut proof.base,
                            Some(SrcColumnBase::UnembeddedBaseYield),
                            "発生",
                        );
                    });
                    if proof.base == Some(SrcColumnBase::UnembeddedBaseYield) {
                        choice(ui, "塑性変形による急激な耐力低下", &mut proof.brittle_rc);
                    } else {
                        capacity(ui, "N0 [kN]", &mut proof.n0_kn);
                        capacity(ui, "sM0 [kN·m]", &mut proof.steel_m0_knm);
                        capacity(ui, "M0 [kN·m]", &mut proof.m0_knm);
                    }
                    capacity(
                        ui,
                        "柱群がある層の群判定用水平耐力 [kN]",
                        &mut proof.horizontal_capacity_kn,
                    );
                    face_limits(ui, "Ds", *id, ds_columns, &mut proof.ds_limits);
                    if form.ds_run_id != form.qu_run_id || form.ds_step != form.qu_step {
                        face_limits(ui, "Qu", *id, qu_columns, &mut proof.qu_limits);
                    }
                    field(
                        ui,
                        "材料強度・各端の同時作用下耐力・群判定用耐力の確認資料",
                        &mut proof.strength_basis,
                    );
                });
            }
            for (id, proof) in &mut form.walls {
                ui.collapsing(format!("壁要素 ID {}", id.0), |ui| {
                    failure(ui, &mut proof.failure, true);
                    field(
                        ui,
                        "破壊形式・採用ステップの確認資料",
                        &mut proof.failure_basis,
                    );
                    capacity(
                        ui,
                        "群判定用水平耐力 [kN]",
                        &mut proof.horizontal_capacity_kn,
                    );
                    field(ui, "設計水平耐力の確認資料", &mut proof.strength_basis);
                });
            }
            for (id, proof) in &mut form.girders {
                ui.collapsing(format!("SRC梁 ID {}", id.0), |ui| {
                    face_limits(ui, "Ds", *id, ds_columns, &mut proof.ds_limits);
                    if form.ds_run_id != form.qu_run_id || form.ds_step != form.qu_step {
                        face_limits(ui, "Qu", *id, qu_columns, &mut proof.qu_limits);
                    }
                    field(ui, "各端の材料強度・同時作用下耐力の確認資料", &mut proof.strength_basis);
                });
            }
            for (id, proof) in &mut form.other_girders {
                field(
                    ui,
                    &format!("他種梁 ID {} の崩壊時応力・耐力の確認資料", id.0),
                    proof,
                );
            }
        });
    }
    let button = if has_confirmation {
        "現在の採用点で設計者確認を作り直す"
    } else {
        "保存採用点の設計者確認を入力"
    };
    if ui.button(button).clicked() {
        if let Err(reason) = app.prepare_src_holding_confirmation() {
            app.core.scoped.last_error = Some(reason);
        }
    }
}

fn evaluation_slot(
    run: &mut PushoverResult,
    purpose: EvaluationPurpose,
) -> &mut Option<EvaluationPoint> {
    match purpose {
        EvaluationPurpose::Ds => &mut run.ds_evaluation,
        EvaluationPurpose::HoldingCapacity => &mut run.capacity_evaluation,
    }
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String) {
    ui.label(label);
    ui.text_edit_singleline(text);
}

fn capacity(ui: &mut egui::Ui, label: &str, value: &mut f64) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).max_decimals(6));
    });
}

fn choice(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.selectable_value(value, Some(false), "なし");
        ui.selectable_value(value, Some(true), "あり");
    });
}

fn confirm(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.selectable_value(value, Some(true), "確認済み");
        ui.selectable_value(value, Some(false), "未確認");
    });
}

fn face_limits(
    ui: &mut egui::Ui,
    purpose: &str,
    id: ElemId,
    records: &[SrcFrameFaceReview],
    limits: &mut [SrcFrameFaceLimits; 2],
) {
    ui.collapsing(format!("{purpose} 確定点の部材端耐力"), |ui| {
        let response = records.iter().find(|r| r.elem == id);
        for (i, limit) in limits.iter_mut().enumerate() {
            ui.strong(if i == 0 { "i端" } else { "j端" });
            match response.map(|r| &r.faces[i]) {
                Some(Ok([n, qy, qz, _, my, mz])) => {
                    ui.label(format!(
                    "確定応力 N={:.3} kN（引張正）、Q強/弱={:.3}/{:.3} kN、M強/弱={:.3}/{:.3} kN·m",
                    n / 1000.0, qy / 1000.0, qz / 1000.0, mz / 1e6, my / 1e6
                ))
                }
                Some(Err(reason)) => ui.label(reason),
                None => ui.label("確定断面力がありません"),
            };
            capacity(ui, "作用符号の軸耐力 [kN]", &mut limit.axial_kn);
            capacity(
                ui,
                "同時作用下の曲げ耐力・強軸 [kN·m]",
                &mut limit.bending_strong_knm,
            );
            capacity(
                ui,
                "同時作用下の曲げ耐力・弱軸 [kN·m]",
                &mut limit.bending_weak_knm,
            );
            capacity(
                ui,
                "同時作用下のせん断耐力・強軸 [kN]",
                &mut limit.shear_strong_kn,
            );
            capacity(
                ui,
                "同時作用下のせん断耐力・弱軸 [kN]",
                &mut limit.shear_weak_kn,
            );
        }
    });
}

fn collapse_origins(
    ui: &mut egui::Ui,
    purpose: &str,
    origins: &mut Vec<SrcCollapseOrigin>,
    faces: &[SrcFrameFaceReview],
    columns: &[ElemId],
    walls: &[ElemId],
) {
    ui.collapsing(
        format!("{purpose} 採用点の崩壊形を構成する部材端"),
        |ui| {
            let mut remove = None;
            for (index, origin) in origins.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("部材");
                        egui::ComboBox::from_id_salt(("src_collapse_member", purpose))
                            .selected_text(
                                origin
                                    .elem
                                    .map_or_else(|| "未指定".into(), |id| format!("ID {}", id.0)),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut origin.elem, None, "未指定");
                                for id in faces.iter().map(|r| r.elem).chain(walls.iter().copied())
                                {
                                    let label = if walls.contains(&id) {
                                        "壁"
                                    } else if columns.contains(&id) {
                                        "柱"
                                    } else {
                                        "梁"
                                    };
                                    ui.selectable_value(
                                        &mut origin.elem,
                                        Some(id),
                                        format!("{label} ID {}", id.0),
                                    );
                                }
                            });
                        ui.selectable_value(&mut origin.end, Some(SrcMemberEnd::I), "i端 / 壁下端");
                        ui.selectable_value(&mut origin.end, Some(SrcMemberEnd::J), "j端 / 壁上端");
                        if ui.button("削除").clicked() {
                            remove = Some(index);
                        }
                    });
                    failure(ui, &mut origin.failure, false);
                    field(
                        ui,
                        "選んだ端の破壊と崩壊形への対応を示す確認資料",
                        &mut origin.basis,
                    );
                });
            }
            if let Some(index) = remove {
                origins.remove(index);
            }
            if ui.button("構成部材端を追加").clicked() {
                origins.push(SrcCollapseOrigin::default());
            }
        },
    );
}

fn failure(ui: &mut egui::Ui, value: &mut Option<SrcFailure>, wall: bool) {
    ui.horizontal(|ui| {
        ui.label("破壊形式");
        ui.selectable_value(value, Some(SrcFailure::Bending), "曲げ");
        ui.selectable_value(value, Some(SrcFailure::Shear), "せん断");
        if wall {
            ui.selectable_value(value, Some(SrcFailure::Other), "その他");
        }
    });
}
