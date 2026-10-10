use super::super::*;

impl App {
    /// 終局検定（靭性保証型耐震設計指針）: RC 矩形部材の終局せん断強度（塑性
    /// 理論式）・付着割裂耐力・軸終局耐力に対する余裕度を算定する。
    ///
    /// 柱の曲げ終局強度 Mu・軸余裕度に用いる設計軸力は、長期（G+P 相当）静的
    /// 解析結果（先頭重力ケースを優先、なければ最後に実行した静的解析）の軸力
    /// （圧縮正）を用いる。静的解析結果がない場合は軸力 0（安全側）で評価する。
    ///
    /// 対象 RC 矩形部材が 1 つもない場合は `Err` を返す（UI 側で案内表示）。
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

    /// 終局検定用の部材需要（軸力 [N]圧縮正・強軸/弱軸の設計用曲げ [N·mm]）。
    ///
    /// `ultimate_use_pushover` が真で増分解析応答（部材別応答）が得られる場合は、
    /// 終局時の部材別 Qmu（設計用せん断）・需要曲げ・軸力・Rp を直接反映する
    /// （[`Self::ultimate_demand_from_pushover`]）。それ以外は先頭重力ケース（G+P 相当）の
    /// 静的解析結果を優先し、なければ最後に実行した静的解析結果を用いる（軸力は始端値、
    /// 曲げは部材内の最大絶対値、Qmu は両端ヒンジ 2·Mu/内法、Rp は UI 一律指定）。
    /// いずれの応答もなければ空（＝需要 0）。
    fn ultimate_demand_by_elem(
        &self,
    ) -> Result<Vec<(ElemId, sepika_design_jp::ultimate::MemberDemand)>, String> {
        if self.core.ultimate_use_pushover {
            if let Some(demand) = self.ultimate_demand_from_pushover() {
                self.require_result_input(ResultInputKey::Pushover(
                    self.core.scoped.pushover_view_dir,
                ))?;
                self.require_ultimate_gravity_inputs()?;
                return Ok(demand);
            }
        }
        let gravity_long = self.core.scoped.results.as_ref().and_then(|r| {
            sepika_job::sum_analyzed_gravity_member_forces(&self.core.model, |lc| {
                r.statics
                    .iter()
                    .find(|(id, _)| *id == StaticCaseKey::User(lc))
                    .map(|(_, s)| s.member_forces.clone())
            })
        });
        if gravity_long.is_some() {
            self.require_ultimate_gravity_inputs()?;
        } else if let Some(bundle) = &self.core.scoped.results {
            let gravity = gravity_cases_for_seismic_weight(&self.core.model)
                .first()
                .copied()
                .filter(|lc| {
                    bundle
                        .statics
                        .iter()
                        .any(|(key, _)| *key == StaticCaseKey::User(*lc))
                });
            if let Some(lc) = gravity {
                self.require_result_input(ResultInputKey::Static(StaticCaseKey::User(lc)))?;
            } else if !bundle.member_forces.is_empty() {
                self.require_selected_static_input()?;
            }
        }
        Ok(self
            .core
            .scoped
            .results
            .as_ref()
            .map(|r| {
                let fallback = gravity_cases_for_seismic_weight(&self.core.model)
                    .first()
                    .and_then(|lc| {
                        r.statics
                            .iter()
                            .find(|(id, _)| *id == StaticCaseKey::User(*lc))
                    })
                    .map(|(_, s)| s.member_forces.as_slice())
                    .unwrap_or(r.member_forces.as_slice());
                let member_forces: &[(ElemId, sepika_element::frame::beam::MemberForces)] =
                    gravity_long.as_deref().unwrap_or(fallback);
                let ql_map = sepika_job::q_long_map_from_member_forces(member_forces);
                sepika_job::member_demand_from_static_forces(member_forces, Some(&ql_map))
            })
            .unwrap_or_default())
    }

    fn require_ultimate_gravity_inputs(&self) -> Result<(), String> {
        if let Some(bundle) = &self.core.scoped.results {
            for lc in gravity_cases_for_seismic_weight(&self.core.model) {
                if bundle
                    .statics
                    .iter()
                    .any(|(key, _)| *key == StaticCaseKey::User(lc))
                {
                    self.require_result_input(ResultInputKey::Static(StaticCaseKey::User(lc)))?;
                }
            }
        }
        Ok(())
    }

    /// 増分解析応答（部材別応答）から終局検定用の部材需要を組み立てる。
    ///
    /// 増分解析最終ステップの部材別応答（[`sepika_solver::nonlinear::pushover::PushoverMemberResponse`]）
    /// から、軸力（圧縮正）・強軸/弱軸の設計用曲げ・強軸設計用せん断・部材別 Rp を
    /// 反映する。増分解析未実行、または部材別応答が空（ステップ未確定）の場合は
    /// `None`（呼び出し側が静的応答へフォールバック）。
    fn ultimate_demand_from_pushover(
        &self,
    ) -> Option<Vec<(ElemId, sepika_design_jp::ultimate::MemberDemand)>> {
        let po = self.displayed_pushover()?;
        let gravity_long = self.core.scoped.results.as_ref().and_then(|res| {
            sepika_job::sum_analyzed_gravity_member_forces(&self.core.model, |lc| {
                res.statics
                    .iter()
                    .find(|(id, _)| *id == StaticCaseKey::User(lc))
                    .map(|(_, s)| s.member_forces.clone())
            })
        });
        let ql_by_elem: Option<std::collections::HashMap<ElemId, f64>> =
            gravity_long.as_ref().map(|gl| {
                gl.iter()
                    .map(|(id, mf)| {
                        (
                            *id,
                            mf.at.iter().map(|(_, f)| f[1].abs()).fold(0.0, f64::max),
                        )
                    })
                    .collect()
            });
        sepika_job::member_demand_from_pushover(&po.member_response, ql_by_elem.as_ref())
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
mod tests {
    use super::*;

    fn analyzed() -> App {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.core.analysis_cfg.threads = 1;
        app.run_linear_static(LoadCaseId(0));
        assert!(app.core.scoped.last_error.is_none());
        app
    }

    #[test]
    fn ultimate_gravity_sources_reject_changed_missing_and_old_contract_inputs() {
        for missing in [false, true] {
            let mut app = analyzed();
            assert!(!app.ultimate_demand_by_elem().unwrap().is_empty());
            if missing {
                app.core
                    .scoped
                    .results
                    .as_mut()
                    .unwrap()
                    .input_records
                    .clear();
            } else {
                app.core.model.sections[0].area *= 1.1;
            }
            let expected = if missing {
                "入力識別情報"
            } else {
                "一致しません"
            };
            assert!(app
                .compute_ultimate_checks()
                .unwrap_err()
                .contains(expected));
            assert!(app
                .compute_cft_ultimate_checks()
                .unwrap_err()
                .contains(expected));
            app.run_linear_static(LoadCaseId(0));
            assert!(!app.ultimate_demand_by_elem().unwrap().is_empty());
            app.core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .analysis_contract_version = Some(0);
            assert!(app
                .compute_ultimate_checks()
                .unwrap_err()
                .contains("旧契約"));
        }
    }

    #[test]
    fn ultimate_fallback_checks_selected_combo_name_without_requiring_unused_static_keys() {
        let mut app = analyzed();
        app.core
            .model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "終局用選択".into(),
                terms: vec![(LoadCaseId(0), 1.0)],
            });
        app.run_combination(0);
        app.core.scoped.results.as_mut().unwrap().statics.clear();
        assert!(!app.ultimate_demand_by_elem().unwrap().is_empty());
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .input_records
            .retain(|(key, _)| !matches!(key, ResultInputKey::Combo(_)));
        let reason = app.ultimate_demand_by_elem().unwrap_err();
        assert!(reason.contains("終局用選択") && reason.contains("入力識別情報"));
    }

    #[test]
    fn ultimate_pushover_requires_its_actual_direction_input_and_recovers_explicitly() {
        let mut app = analyzed();
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_linear_static(LoadCaseId(0));
        app.core.analysis_cfg.push_steps = 3;
        app.run_pushover();
        app.core.ultimate_use_pushover = true;
        assert!(app.ultimate_demand_from_pushover().is_some());
        assert!(!app.ultimate_demand_by_elem().unwrap().is_empty());
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .input_records
            .retain(|(key, _)| !matches!(key, ResultInputKey::Pushover(SeismicDir::X)));
        let reason = app.compute_ultimate_checks().unwrap_err();
        assert!(reason.contains("増分解析 X") && reason.contains("入力識別情報"));
        app.run_pushover();
        assert!(!app.ultimate_demand_by_elem().unwrap().is_empty());
    }
}
