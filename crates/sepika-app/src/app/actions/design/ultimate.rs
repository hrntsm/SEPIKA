use super::super::*;

impl App {
    /// RC部材の終局検定。必要な重力需要・確定応答が不足すればエラーを返す。
    pub fn compute_ultimate_checks(
        &mut self,
    ) -> Result<Vec<sepika_design_jp::ultimate::UltimateCheck>, String> {
        use sepika_core::section_shape::SectionShape;

        self.apply_rigid_zones_for_analysis();

        let demand = self.ultimate_demand_by_elem()?;

        let opts = sepika_design_jp::ultimate::UltimateShearOptions {
            rp: self.core.ultimate_rp.max(0.0),
            lightweight: self.core.ultimate_lightweight,
            upper_strength_factor: self.core.ultimate_upper_factor.max(0.0),
            sigma_wy: 295.0,
            include_bond: self.core.ultimate_include_bond,
            shear_method: if self.core.ultimate_shear_ductility {
                sepika_design_jp::ultimate::ShearMethod::Ductility
            } else {
                sepika_design_jp::ultimate::ShearMethod::Plastic
            },
            biaxial_shear: self.core.ultimate_biaxial_shear,
            biaxial_bending: self.core.ultimate_biaxial_bending,
        };
        let checks = sepika_design_jp::ultimate::collect_rc_ultimate_checks(
            &self.core.model,
            &demand,
            &opts,
        )?;

        let has_rc_rect = self.core.model.elements.iter().any(|e| {
            e.section
                .and_then(|sid| self.core.model.sections.get(sid.index()))
                .and_then(|s| s.shape.as_ref())
                .map(|sh| {
                    matches!(
                        sh,
                        SectionShape::RcBeamRect { .. }
                            | SectionShape::RcColumnRect { .. }
                            | SectionShape::RcColumnCircle { .. }
                    )
                })
                .unwrap_or(false)
        });
        if checks.is_empty() {
            if has_rc_rect {
                return Err(
                    "RC 部材の終局検定を算定できませんでした（コンクリート強度 Fc の設定・\
                     有効せいを確認してください）。"
                        .to_string(),
                );
            }
            return Err(
                "終局検定の対象となる RC 部材がありません。RC 断面を割り当ててください。"
                    .to_string(),
            );
        }
        Ok(checks)
    }

    /// 保存長期組合せまたは指定方向の確定総応答から需要を採用する。不足時は拒否する。
    pub(crate) fn ultimate_demand_by_elem(
        &self,
    ) -> Result<Vec<(ElemId, sepika_design_jp::ultimate::MemberDemand)>, String> {
        if self.core.ultimate_use_pushover {
            return self.ultimate_demand_from_pushover();
        }
        sepika_job::complete_ultimate_static_demand(&self.core.model, |lc| {
            self.require_result_input(ResultInputKey::Static(StaticCaseKey::User(lc)))
                .map_err(|reason| format!("ultimate_static_input_stale: {reason}"))?;
            self.core
                .scoped
                .results
                .as_ref()
                .and_then(|r| {
                    r.statics
                        .iter()
                        .find(|(id, _)| *id == StaticCaseKey::User(lc))
                })
                .map(|(_, result)| result.member_forces.clone())
                .ok_or_else(|| format!("静的ケース {} が未解析です", lc.0))
        })
    }

    fn ultimate_demand_from_pushover(
        &self,
    ) -> Result<Vec<(ElemId, sepika_design_jp::ultimate::MemberDemand)>, String> {
        self.ultimate_demand_from_confirmed_run().map_err(|reason| {
            let dir = self.core.scoped.pushover_view_dir;
            let po = self.pushover_for(dir);
            let run = po.and_then(|po| po.wall_run.as_ref());
            let point = po.and_then(|po| po.capacity_evaluation.as_ref());
            let generation = |bytes: Option<&Vec<u8>>| {
                bytes.map_or_else(|| "unresolved".into(), |value| crate::summary::input_generation_hash(value))
            };
            let terms = run.map_or_else(
                || "unresolved".into(),
                |run| {
                    run.gravity_terms
                        .iter()
                        .map(|(case, factor)| format!("case={} coefficient={factor}", case.0))
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            );
            let context = format!(
                "mode=confirmed_total purpose=frame_long evaluation_purpose=HoldingCapacity direction={dir:?} run={} input_generation={} selected_run={} selected_generation={} selected_direction={} step={} gravity_terms=[{terms}]",
                run.map_or("unresolved", |run| run.run_id.as_str()),
                generation(run.and_then(|run| run.input_generation.as_ref())),
                point.map_or("unresolved", |point| point.run_id.as_str()),
                generation(point.map(|point| &point.input_generation)),
                point.map_or_else(
                    || "unresolved".into(),
                    |point| format!("{:?}", point.direction)
                ),
                point.map_or_else(|| "unresolved".into(), |point| point.step.to_string()),
            );
            let member = if reason.contains("member=") {
                ""
            } else {
                " member=unresolved"
            };
            format!("{reason}; {context}{member}")
        })
    }

    fn ultimate_demand_from_confirmed_run(
        &self,
    ) -> Result<Vec<(ElemId, sepika_design_jp::ultimate::MemberDemand)>, String> {
        use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
        let dir = self.core.scoped.pushover_view_dir;
        self.require_result_input(ResultInputKey::Pushover(dir))
            .map_err(|reason| format!("ultimate_input_stale: {reason}"))?;
        let po = self
            .pushover_for(dir)
            .ok_or("ultimate_run_missing: 選択方向の増分解析結果がありません")?;
        let run = po
            .wall_run
            .as_ref()
            .ok_or("ultimate_run_missing: run識別がありません")?;
        if !run.gravity_applied || run.gravity_terms.is_empty() {
            return Err(
                "ultimate_gravity_missing: 初期重力なしrunは一般建物の終局判定へ採用できません"
                    .into(),
            );
        }
        if run.gravity_terms != sepika_core::load_combo::ultimate_gravity_terms(&self.core.model)? {
            return Err(
                "ultimate_gravity_stale: runの初期重力termsと現在入力が一致しません".into(),
            );
        }
        if self.core.ultimate_upper_factor != 1.0 {
            return Err(
                "ultimate_total_unsupported: 確定総応答の上限強度倍率は1のみ対応しています".into(),
            );
        }
        let point = po
            .capacity_evaluation
            .as_ref()
            .ok_or("ultimate_step_missing: 確定応答の採用stepを明示してください")?;
        if point.direction != dir {
            return Err("ultimate_direction_mismatch: 採用方向が不一致です".into());
        }
        let response = po.confirmed_response(point, EvaluationPurpose::HoldingCapacity)?;
        if point.input_generation
            != self
                .result_input_with_settings(&ResultInputKey::Pushover(dir), self.core.analysis_cfg)
        {
            return Err("ultimate_generation_stale: 確定応答の生成入力が現在の計算入力と一致しません。増分解析を再実行し、採用stepを明示してください".into());
        }
        let line_response: Vec<_> = response
            .members
            .iter()
            .filter(|r| r.wall_shear_signed.is_none())
            .cloned()
            .collect();
        if let Some(member) = self.core.model.elements.iter().find(|member| {
            matches!(
                self.core
                    .model
                    .element_section(member)
                    .and_then(|s| s.shape.as_ref()),
                Some(
                    sepika_core::section_shape::SectionShape::RcBeamRect { .. }
                        | sepika_core::section_shape::SectionShape::RcColumnRect { .. }
                        | sepika_core::section_shape::SectionShape::RcColumnCircle { .. }
                        | sepika_core::section_shape::SectionShape::CftBox { .. }
                        | sepika_core::section_shape::SectionShape::CftPipe { .. }
                )
            ) && !line_response
                .iter()
                .any(|response| response.elem == member.id)
        }) {
            return Err(format!(
                "ultimate_response_missing: member={} 必要部材の確定総応答がありません",
                member.id.0
            ));
        }
        sepika_job::member_demand_from_pushover(&line_response, None)
            .ok_or_else(|| "ultimate_response_missing: 有限な確定総応答がありません".into())
    }

    /// CFT 柱の軸終局検定（CFT指針）: CftBox/CftPipe 柱の
    /// 軸圧縮終局耐力 Ncu・軸引張終局耐力 Ntu に対する軸余裕度を算定する。
    ///
    /// 対象 CFT 柱が 1 つもない場合は `Err` を返す（UI 側で案内表示）。
    pub fn compute_cft_ultimate_checks(
        &mut self,
    ) -> Result<Vec<sepika_design_jp::ultimate::CftUltimateCheck>, String> {
        self.apply_rigid_zones_for_analysis();
        let axial: Vec<(ElemId, f64)> = self
            .ultimate_demand_by_elem()?
            .into_iter()
            .map(|(id, d)| (id, d.n_axial))
            .collect();
        let checks =
            sepika_design_jp::ultimate::collect_cft_ultimate_checks(&self.core.model, &axial)?;
        if checks.is_empty() {
            return Err(
                "終局検定の対象（CftBox/CftPipe の CFT 柱）がありません。CFT 断面と\
                 コンクリート強度 Fc を設定してください。"
                    .to_string(),
            );
        }
        Ok(checks)
    }
}

#[cfg(test)]
#[path = "ultimate565_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "ultimate565_rc_cft_tests.rs"]
mod rc_cft_tests;
