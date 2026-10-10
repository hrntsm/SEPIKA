use super::super::*;

impl App {
    /// 選択対象の保存係数・種別による状態。単独ケースと組合せを区別する。
    pub fn selected_design_load_state(
        &self,
    ) -> Result<sepika_core::load_combo::DesignLoadState, String> {
        use sepika_core::load_combo::{case_design_state, combination_design_state};
        match self.core.scoped.last_static {
            Some(StaticKey::Combo(index)) => {
                let name = self
                    .core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|r| r.combos.get(index))
                    .map(|(name, _)| name)
                    .ok_or("選択組合せの解析結果がありません")?;
                let mut combos = self
                    .core
                    .model
                    .combinations
                    .iter()
                    .filter(|c| &c.name == name);
                let combo = combos.next().ok_or("保存組合せがありません")?;
                if combos.next().is_some() {
                    return Err("同名組合せが複数あり対象が未判定です".into());
                }
                combination_design_state(combo, &self.core.model.load_cases)
            }
            Some(StaticKey::Case(key)) => {
                let id = match key {
                    StaticCaseKey::User(id) => Some(id),
                    StaticCaseKey::Seismic(dir) => self.seismic_case_id(dir),
                }
                .ok_or("荷重ケースの検定用途が未判定です")?;
                let case = self
                    .core
                    .model
                    .load_cases
                    .iter()
                    .find(|c| c.id == id)
                    .ok_or("荷重ケースがありません")?;
                case_design_state(case.kind)
            }
            None => Err("検定対象が選択されていません".into()),
        }
    }

    /// 解析結果の member_forces から検定結果を生成する。
    /// 危険断面位置（既定は柱フェイスと中央）の内力に対し、
    /// 材種・部材種別に応じた検定を適用する（令82条・各構造設計規準準拠）。
    /// 節点芯は剛域が有る場合は検定対象外。
    ///
    /// - 部材種別は断面用途と解析結果に基づいて扱う。
    /// - せん断スパン比 M/(Q·d) 用の代表値は、モーメントが最大となる
    ///   検定位置の値を採用する方針で部材単位に求める。
    /// - 柱は軸力＋二軸曲げ（n, my, mz）を検定に渡す。
    /// - 検定器は構造種別（`sepika_core::structure_kind`）で選択する。
    /// - 床板の設計自重が計算不可の場合は理由を通知し、既存の検定結果を消去する。
    pub fn run_design_check(&mut self) {
        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.member_checks.clear();
            bundle.joint_checks.clear();
            bundle.wall_checks.clear();
            bundle.beam_checks.clear();
            bundle.slab_checks.clear();
        }
        self.apply_rigid_zones_for_analysis();
        if let Err(error) = sepika_load::floor::validate_one_way_directions(&self.core.model) {
            self.report_error(error.to_string());
            return;
        }
        for slab in &self.core.model.slabs {
            if let Err(error) = sepika_load::floor::distribute_slab(&self.core.model, slab) {
                self.report_error(format!("床板・小梁検定: {error}"));
                return;
            }
        }
        let state = self.selected_design_load_state();
        if let Ok(state) = state {
            self.core.design_term = match state.duration {
                sepika_core::load_combo::LoadDuration::Long => LoadTerm::Long,
                sepika_core::load_combo::LoadDuration::Short => LoadTerm::Short,
            };
        }
        let mut load_error = state.as_ref().err().cloned();
        if state.as_ref().is_ok_and(|s| {
            !s.combination && s.duration == sepika_core::load_combo::LoadDuration::Short
        }) {
            load_error =
                Some("単独の雪・風・地震応力です。G+Pを含む保存組合せを選択してください".into());
        }
        if state.as_ref().is_ok_and(|s| s.combination)
            && self.core.model.stress_cfg.tension_only_iteration
            && self.core.model.elements.iter().any(|e| {
                matches!(
                    e.kind,
                    sepika_core::model::ElementKind::Brace { tension_only: true }
                )
            })
        {
            load_error =
                Some("引張専用ブレースの別ケース合成は同一解を保証できないため未検定です".into());
        }
        let Some(results) = &self.core.scoped.results else {
            return;
        };
        let expanded_storage;
        let wall_index;
        let design_model: &sepika_core::model::Model =
            if sepika_load::wall_expand::model_has_wall_plates_to_expand(&self.core.model) {
                let (expanded, index, _wall_report) =
                    sepika_load::wall_expand::expand_wall_elements(&self.core.model);
                wall_index = Some(index);
                expanded_storage = expanded;
                &expanded_storage
            } else {
                wall_index = None;
                &self.core.model
            };
        let is_seismic_combo = state.as_ref().is_ok_and(|s| {
            s.combination && s.action == sepika_core::load_combo::LoadAction::Seismic
        });
        let gravity_terms = match self.core.scoped.last_static {
            Some(StaticKey::Combo(index)) => results
                .combos
                .get(index)
                .and_then(|(name, _)| {
                    self.core
                        .model
                        .combinations
                        .iter()
                        .find(|c| &c.name == name)
                })
                .map(|combo| sepika_job::design_gravity_terms(&self.core.model, &combo.terms))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let gravity_long_owned = if is_seismic_combo {
            match sepika_job::complete_design_gravity_forces(&gravity_terms, |lc| {
                sepika_job::compute::compute_linear_static(self.core.model.clone(), lc)
                    .ok()
                    .map(|r| r.member_forces)
            }) {
                Ok(forces) => Some(forces),
                Err(missing) => {
                    load_error = Some(format!("検定用G+Pの重力応力が不足しています: {missing:?}"));
                    None
                }
            }
        } else {
            None
        };
        let long_member_forces = gravity_long_owned.as_deref();
        let group_overrides =
            sepika_design_jp::girder_group_overrides(&self.core.model, &results.member_forces);
        let q0_by_elem = if long_member_forces.is_some() {
            sepika_job::simple_beam_q0_by_terms(&self.core.model, &gravity_terms)
        } else {
            Default::default()
        };
        let wall_case = match self.core.scoped.last_static {
            Some(StaticKey::Case(StaticCaseKey::User(id))) => format!("case:{}", id.0),
            Some(StaticKey::Case(key)) => format!("case:{key:?}"),
            Some(StaticKey::Combo(idx)) => results
                .combos
                .get(idx)
                .map(|(name, _)| format!("combo:{idx}:{name}"))
                .unwrap_or_else(|| format!("combo:{idx}")),
            None => "未選択".into(),
        };
        let mut report = sepika_design_jp::run_member_design_checks(
            design_model,
            &results.member_forces,
            &results.panel_moments,
            &sepika_design_jp::MemberDesignCheckOptions {
                term: self.core.design_term,
                wall_index: wall_index.as_ref(),
                wall_case: &wall_case,
                rc_damage_control: self.core.analysis_cfg.rc_damage_control,
                bond_method: self.core.analysis_cfg.bond_method,
                qd_method: self.core.analysis_cfg.qd_method,
                long_member_forces,
                q_simple_by_elem: Some(&q0_by_elem),
                girder_group_overrides: Some(&group_overrides),
                steel_fb_basis: sepika_design_jp::SteelFbBasis::default(),
            },
        );
        if let Some(reason) = &load_error {
            report.skip_for_load_state(reason);
        }
        let joint_checks = report
            .joint_checks
            .into_iter()
            .map(|(node, label, outcome)| JointCheck {
                node,
                label,
                outcome,
            })
            .collect();
        let (beam_checks, slab_checks) = self.floor_design_checks();

        let member_checks = group_member_checks(report.member_checks);

        if let Some(bundle) = self.core.scoped.results.as_mut() {
            bundle.member_checks = member_checks;
            bundle.joint_checks = joint_checks;
            bundle.wall_checks = report.wall_checks;
            bundle.beam_checks = beam_checks;
            bundle.slab_checks = slab_checks;
        }
    }

    /// 床の中での小梁・スラブ設計を算定する（`run_design_check` から呼ぶ）。
    ///
    /// - 二次部材小梁: 二次部材の反力の逐次伝達（[`sepika_load::cascade`]）が求めた荷重
    ///   （床板分配の辺荷重・自重・架け側から渡された集中荷重）を単純梁または片持ち梁と
    ///   して検定する。
    /// - 実部材化された小梁（支持間に実 Beam がある）は全体 FEM で検定するため対象外。
    /// - 断面未割当・鋼以外の材料・分配荷重が無い・期待床板の欠落・カバー不足の二次部材は表に「未」として残す。
    /// - スラブ: 矩形スラブの短辺を設計スパンとし、一方向版として設計曲げモーメントと
    ///   必要鉄筋量を算定する（鋼小梁・SD295 鉄筋の既定値を用いる）。
    pub(crate) fn floor_design_checks(
        &self,
    ) -> (Vec<crate::app::BeamCheck>, Vec<crate::app::SlabCheck>) {
        use sepika_core::model::LoadPurpose;
        use sepika_design_jp::floor as fd;

        let mut beam_checks = Vec::new();
        let mut slab_checks = Vec::new();

        for slab in &self.core.model.slabs {
            let Some(thickness) = self.core.model.slab_plate_thickness(slab) else {
                continue;
            };
            let w = self.core.model.slab_intensity(slab, LoadPurpose::Floor);
            if slab.is_attached() {
                if let Some(span) = slab.attached_design_span() {
                    let r = fd::design_slab_oneway(
                        span,
                        w,
                        2.0,
                        thickness,
                        fd::SLAB_DEFAULT_COVER,
                        fd::REBAR_FT_LONG_SD295,
                        fd::SLAB_J_RATIO,
                    );
                    slab_checks.push((slab.id, r));
                }
            } else if let Some((lx, ly)) =
                sepika_load::floor::slab_dimensions(&self.core.model, slab)
            {
                use sepika_core::model::OneWayDir;
                let span = match slab.one_way() {
                    Some(OneWayDir::X) => lx,
                    Some(OneWayDir::Y) => ly,
                    Some(OneWayDir::Short) if (lx - ly).abs() >= 1e-6 => lx.min(ly),
                    Some(OneWayDir::Short) => continue,
                    None => lx.min(ly),
                };
                if span > 1e-9 {
                    let r = fd::design_slab_oneway(
                        span,
                        w,
                        8.0,
                        thickness,
                        fd::SLAB_DEFAULT_COVER,
                        fd::REBAR_FT_LONG_SD295,
                        fd::SLAB_J_RATIO,
                    );
                    slab_checks.push((slab.id, r));
                }
            }
        }

        self.design_secondary_beam_checks(&mut beam_checks);

        (beam_checks, slab_checks)
    }

    /// 領域内小梁および未割当の片持ち小梁を、二次部材の反力の逐次伝達
    /// （[`sepika_load::cascade`]）が求めた荷重で検定する。
    ///
    /// 荷重は床板分配の辺荷重・自重・**架け側の二次部材から渡された集中荷重**の
    /// 重ね合わせである。**荷重同期（`sepika-job::auto_loads`）と同じ経路を使う**
    /// （判定が 2 か所に分かれると解析と検定で荷重が食い違うため）。
    ///
    /// ただし**荷重の値そのものは一致しない**。共有するのは支持関係の判定と伝達の
    /// 手順であって、面荷重強度は用途ごとに違う。小梁検定は小梁用（`LoadPurpose::Beam`。
    /// 固定＋小梁用積載）、荷重同期は固定荷重ケースと積載荷重ケースへ分けて解く。
    ///
    /// 断面未割当・鋼以外の材料・分配が足りないものは表に「未」として残す。
    /// 所属先がない小梁のうち、片持ち小梁以外は検定の対象にしない（表に「未」として
    /// 残す。片持ち小梁は取付き線の支持辺として荷重を受けるため検定する）。
    ///
    /// # 検定できない二次部材（表には「未」の行として残す）
    ///
    /// - **間柱**。壁版から受ける荷重は材軸方向の軸力であり、地震時には壁の面外
    ///   地震力による弱軸曲げも受ける。軸力・面外曲げのいずれも本検定は扱わない。
    /// - **剛床でない床の小梁**。分配 Span 検定は曲げ・せん断・たわみだけを見て
    ///   軸力を見ない。これは「その小梁が載る床面が 1 枚の剛体で、面内力を剛床が
    ///   処理している」ことを前提にしている。剛床でない床の小梁は面内力を負担する
    ///   ため前提が成り立たない（`Model::floor_region_on_single_diaphragm`）。
    ///
    /// 表から消すと検定されていないことに気づけないため、行は残す。
    fn design_secondary_beam_checks(&self, beam_checks: &mut Vec<crate::app::BeamCheck>) {
        use sepika_core::model::{LoadPurpose, SecondaryMemberKind};
        use sepika_design_jp::floor as fd;
        use sepika_load::floor::{cantilever_extremes, simple_beam_extremes};

        let w_of =
            |s: &sepika_core::model::Slab| self.core.model.slab_intensity(s, LoadPurpose::Beam);
        let transfer = match sepika_load::cascade::solve(&self.core.model, w_of, true) {
            Ok(transfer) => transfer,
            Err(_) => return,
        };

        for sm in self.core.model.posts() {
            let Some((_, _, span)) = self.core.model.secondary_member_axis(sm) else {
                continue;
            };
            beam_checks.push((
                None,
                crate::app::BeamCheckTarget::SecondaryPost { member: sm.id },
                fd::beam_unchecked(span),
            ));
        }

        for sm in self.core.model.beams() {
            if sm.kind != SecondaryMemberKind::Beam {
                continue;
            }
            if self.core.model.secondary_member_materialized(sm) {
                continue;
            }
            let Some((_, _, span)) = self.core.model.secondary_member_axis(sm) else {
                continue;
            };

            let target = crate::app::BeamCheckTarget::SecondaryBeam { member: sm.id };
            let entry = transfer.members.get(&sm.id);
            let region = self.core.model.floor_region_of_beam(sm.id);
            let region_slab = region.and_then(|r| r.slab_ids.first().copied());
            let slab_id = region_slab.or_else(|| entry.and_then(|e| e.rep_slab_id));

            let on_single_diaphragm = match region {
                Some(r) => self.core.model.floor_region_on_single_diaphragm(r),
                None => sm.is_cantilever(),
            };
            if !on_single_diaphragm {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            }

            let Some(sid) = sm.section else {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            };
            let Some(sec) = self.core.model.sections.get(sid.index()) else {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            };
            let z = if sec.depth > 0.0 {
                sec.iy / (sec.depth / 2.0)
            } else {
                0.0
            };
            let mat = self.core.model.secondary_material(sm);
            let Some((e, ft)) = fd::beam_steel_e_and_ft(mat) else {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            };

            let Some(entry) = entry.filter(|e| e.distribution_ready) else {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            };
            let ex = if sm.is_cantilever() {
                cantilever_extremes(&entry.member_loads, span, e, sec.iy)
            } else {
                simple_beam_extremes(&entry.member_loads, span, e, sec.iy)
            };
            if ex.w_equiv <= 1e-9 && ex.m_max <= 1e-9 {
                beam_checks.push((slab_id, target, fd::beam_unchecked(span)));
                continue;
            }

            let r = fd::design_beam_from_forces(
                span,
                ex.w_equiv,
                ex.m_max,
                ex.q_max,
                ex.deflection,
                z,
                ft,
                fd::DEFLECTION_LIMIT_DENOM,
            );
            beam_checks.push((slab_id, target, r));
        }
    }
}
