use super::super::*;

#[derive(Debug)]
struct RcWallDsGeometry {
    plate: sepika_core::ids::WallPlateId,
    clear_length_mm: f64,
    clear_height_mm: f64,
    columns: [(ElemId, f64, f64); 2],
}

fn rc_wall_ds_geometry(
    wall: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    plate_id: Option<sepika_core::ids::WallPlateId>,
) -> Result<RcWallDsGeometry, String> {
    use sepika_core::model::{MaterialCategory, SupportMemberId};
    use sepika_core::section_shape::SectionShape;
    let plate_id = plate_id.ok_or("壁版の対応が不明です")?;
    let plate = model.wall_plate(plate_id).ok_or("壁版が未解決です")?;
    if !plate.openings.is_empty() || plate.opening_area != 0.0 {
        return Err(format!(
            "壁版 ID {}: 開口付きのDs判定は未対応です",
            plate_id.0
        ));
    }
    let parent = model
        .wall_regions
        .iter()
        .find(|r| r.wall_plate_ids.contains(&plate_id))
        .ok_or("壁領域が未解決です")?;
    if parent.wall_plate_ids.as_slice() != [plate_id] || !model.wall_plate_covers_region(plate) {
        return Err(format!(
            "壁版 ID {}: 単独壁と壁単体Qの負担範囲を確認できません",
            plate_id.0
        ));
    }
    let assignment = model
        .wall_plate_assignment_region(plate_id)
        .ok_or("壁版割当領域が未解決です")?;
    let g = sepika_core::model::wall_element_geometry(wall, model).ok_or("壁幾何が不正です")?;
    let points =
        [g.bottom[0], g.bottom[1], g.top[1], g.top[0]].map(|id| model.nodes[id.index()].coord);
    if points.iter().flatten().any(|v| !v.is_finite()) {
        return Err("壁の座標が非有限です".into());
    }
    if points[0][2] != points[1][2]
        || points[2][2] != points[3][2]
        || !(0..2)
            .all(|axis| points[0][axis] == points[3][axis] && points[1][axis] == points[2][axis])
    {
        return Err("矩形・同一鉛直面以外の壁は未対応です".into());
    }
    let nodes = [g.bottom[0], g.bottom[1], g.top[1], g.top[0]];
    let mut supports = Vec::new();
    for i in 0..4 {
        let candidates: Vec<_> = assignment
            .boundary
            .iter()
            .filter_map(|edge| {
                let SupportMemberId::Primary(id) = edge.support else {
                    return None;
                };
                let e = model.element(id)?;
                (e.nodes.len() == 2
                    && e.nodes.contains(&nodes[i])
                    && e.nodes.contains(&nodes[(i + 1) % 4]))
                .then_some(e)
            })
            .collect();
        if candidates.len() != 1 {
            return Err("周辺の主架構支持材を一意に解決できません".into());
        }
        supports.push(candidates[0]);
    }
    let mut columns = [(ElemId(0), 0.0, 0.0); 2];
    for (side, edge_index) in [3, 1].into_iter().enumerate() {
        let column = supports[edge_index];
        if sepika_design_jp::MemberKind::of_element(column, model)
            != sepika_design_jp::MemberKind::Column
        {
            return Err(format!("側柱 ID {}: 柱用途を確認できません", column.id.0));
        }
        for other in model
            .wall_assignment_regions
            .regions
            .iter()
            .filter(|r| r.id != assignment.id)
        {
            let Some(other_plate) = other.assignment.plate() else {
                continue;
            };
            if parent.wall_plate_ids.contains(&other_plate) {
                continue;
            }
            if other
                .boundary
                .iter()
                .any(|e| e.support == SupportMemberId::Primary(column.id))
            {
                return Err(format!(
                    "壁版 ID {}・側柱 ID {}: 壁版 ID {}と共有する側柱は未対応です",
                    plate_id.0, column.id.0, other_plate.0
                ));
            }
        }
        let section = model
            .element_section(column)
            .ok_or("側柱の断面が未設定です")?;
        let material = model
            .element_material(column)
            .ok_or("側柱の材料が未設定です")?;
        if material.category != MaterialCategory::Concrete
            || material.fc.filter(|v| v.is_finite() && *v > 0.0).is_none()
        {
            return Err(format!("側柱 ID {}: RC材料またはFcが不正です", column.id.0));
        }
        let (area, diameter) = match section.shape.as_ref() {
            Some(SectionShape::RcColumnRect { b, d, .. })
                if [*b, *d].iter().all(|v| v.is_finite() && *v > 0.0) =>
            {
                (b * d, b.min(*d))
            }
            Some(SectionShape::RcColumnCircle { d, .. }) if d.is_finite() && *d > 0.0 => {
                (std::f64::consts::PI * d * d / 4.0, *d)
            }
            _ => {
                return Err(format!(
                    "側柱 ID {}: RC柱の実断面寸法が不明または未対応です",
                    column.id.0
                ))
            }
        };
        columns[side] = (column.id, area, diameter);
    }
    let mut faces = [0.0; 4];
    for i in 0..4 {
        let section = model
            .element_section(supports[i])
            .ok_or("周辺材断面が未設定です")?;
        if ![section.width, section.depth]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
        {
            return Err(format!(
                "周辺材 ID {}: 実断面寸法が不正です",
                supports[i].id.0
            ));
        }
        let material = model
            .element_material(supports[i])
            .ok_or("周辺材材料が未設定です")?;
        if material.category != MaterialCategory::Concrete
            || material.fc.filter(|v| v.is_finite() && *v > 0.0).is_none()
        {
            return Err(format!(
                "周辺材 ID {}: RC材料またはFcが不正です",
                supports[i].id.0
            ));
        }
        let direction = match i {
            0 => [0.0, 0.0, 1.0],
            1 => g.ex_bottom.map(|v| -v),
            2 => [0.0, 0.0, -1.0],
            _ => g.ex_bottom,
        };
        faces[i] = sepika_core::face_distance::wall_support_face(
            model,
            SupportMemberId::Primary(supports[i].id),
            direction,
        )?;
        if !faces[i].is_finite() || faces[i] <= 0.0 {
            return Err(format!(
                "周辺材 ID {}: 実フェース距離が不正です",
                supports[i].id.0
            ));
        }
    }
    let clear_length_mm = g.lw - faces[1] - faces[3];
    let clear_height_mm = points[3][2] - points[0][2] - faces[0] - faces[2];
    if ![clear_length_mm, clear_height_mm]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0)
    {
        return Err("壁板の内法寸法が非正または非有限です".into());
    }
    Ok(RcWallDsGeometry {
        plate: plate_id,
        clear_length_mm,
        clear_height_mm,
        columns,
    })
}

impl App {
    /// 保有水平耐力の層別判定を行う。前提データが不足していれば Err(案内文)。
    ///
    /// 戻り値の第 2 要素は層ごとに採用された部材ランク（`design_rank_auto` が
    /// true の場合は自動判定。確定応力・耐力入力・ランクの欠損は Err。
    /// false の場合は全層の明示入力 `design_rank`）。
    #[allow(clippy::type_complexity)]
    pub fn compute_holding_capacity(
        &mut self,
    ) -> Result<
        (
            sepika_design_jp::secondary::holding_capacity::HoldingCapacityResult,
            Vec<sepika_design_jp::secondary::holding_capacity::MemberRank>,
        ),
        String,
    > {
        use sepika_core::section_shape::SectionShape;
        use sepika_design_jp::secondary::ds_group::{
            ds_rc, ds_steel, member_group, rank_index_for_group, rc_wall_shear_brittle,
            rc_wall_tau_over_fc, rc_wall_type, steel_brace_type, GroupType,
        };
        use sepika_design_jp::secondary::holding_capacity::{
            check_holding_capacity_at_state, MemberRank,
        };
        use sepika_design_jp::secondary::member_rank::worst_rank;
        use sepika_solver::nonlinear::pushover::story_response::{EvaluationPurpose, ForceGroup};
        use sepika_solver::nonlinear::pushover::MechanismType;

        self.core.scoped.holding_capacity_source = None;
        self.core.scoped.ds_beta_u_by_story.clear();
        self.core.scoped.ds_beta_u_unavailable = false;
        self.core.scoped.ds_rank_fallback_stories.clear();
        self.core.scoped.ds_wall_details.clear();
        self.apply_rigid_zones_for_analysis()?;
        for elem in &self.core.model.elements {
            if !matches!(
                elem.kind,
                sepika_core::model::ElementKind::Beam
                    | sepika_core::model::ElementKind::Fiber
                    | sepika_core::model::ElementKind::MultiSpring
                    | sepika_core::model::ElementKind::Brace { .. }
            ) {
                continue;
            }
            let Some(sec) = self.core.model.element_section(elem) else {
                continue;
            };
            let material = if sec.shape.as_ref().is_some_and(|s| {
                matches!(
                    s,
                    SectionShape::SrcBeamRect { .. }
                        | SectionShape::SrcColumnRect { .. }
                        | SectionShape::CftBox { .. }
                        | SectionShape::CftPipe { .. }
                )
            }) {
                self.core.model.element_steel_material(elem)
            } else if elem_is_steel(elem, &self.core.model) {
                self.core.model.element_material(elem)
            } else {
                None
            };
            let is_steel_brace = elem_is_steel(elem, &self.core.model)
                && (matches!(elem.kind, sepika_core::model::ElementKind::Brace { .. })
                    || sepika_design_jp::MemberKind::of_element(elem, &self.core.model)
                        == sepika_design_jp::MemberKind::Brace);
            if is_steel_brace {
                sepika_design_jp::material_strength::section_steel_f(sec, material)
                    .map_err(|reason| format!("部材 {:?}・断面 {:?}: {reason}", elem.id, sec.id))?;
                continue;
            }
            if let Some(reason) = material.and_then(|m| {
                sepika_core::material_grade::steel_f_value_issue(
                    &m.name,
                    sepika_design_jp::material_strength::plate_thickness(sec),
                )
            }) {
                return Err(format!("部材 {:?}・断面 {:?}: {reason}", elem.id, sec.id));
            }
        }

        if self.core.model.stories.is_empty() {
            return Err(
                "階が未定義です。解析タブの「準備計算 実行」を行ってください。".to_string(),
            );
        }
        let view_dir = self.core.scoped.pushover_view_dir;
        if self.core.model.constraints.iter().any(|constraint| {
            matches!(
                constraint,
                sepika_core::model::Constraint::RigidDiaphragm {
                    ci_override: Some(_),
                    ..
                }
            )
        }) {
            return Err("指定 Ci の副剛床を含む必要耐力用地震力 Qud は未対応です。副系統の C0=1.0 に対する適用条件を確認していません。".to_string());
        }
        if self.core.analysis_cfg.ai_mode == AiMode::SemiPrecise {
            return Err("精算周期による必要耐力用地震力 Qud は未対応です。Rt ただし書の適用証拠と採用 Rt の検証を保存していません。".to_string());
        }
        let layers = self.core.model.layers();
        let t = self.design_seismic_period()?;
        let qud = sepika_solver::statics::analysis::seismic_distribution_for_model(
            &self.core.model,
            sepika_solver::statics::analysis::SeismicCfg {
                dir: view_dir,
                mode: self.core.analysis_cfg.ai_mode,
                z: self.core.analysis_cfg.z,
                soil: self.core.analysis_cfg.soil,
                c0: 1.0,
            },
            t,
        )
        .map_err(|e| format!("必要耐力用地震力 Qud を算定できません: {e}"))?
        .qi;
        let n_stories = layers.len();

        let saved_evaluations = self
            .core
            .scoped
            .results
            .as_ref()
            .map(|b| &b.holding_evaluations);
        let saved_ds = saved_evaluations.and_then(|entries| {
            entries
                .iter()
                .find(|e| e.point.direction == view_dir && e.point.purpose == EvaluationPurpose::Ds)
        });
        let saved_capacity = saved_evaluations.and_then(|entries| {
            entries.iter().find(|e| {
                e.point.direction == view_dir
                    && e.point.purpose == EvaluationPurpose::HoldingCapacity
            })
        });
        let mut dependencies = vec![
            ResultInputKey::Pushover(view_dir),
            ResultInputKey::Static(StaticCaseKey::Seismic(view_dir)),
        ];
        if saved_ds.is_some() && saved_capacity.is_some() {
            dependencies.retain(|key| !matches!(key, ResultInputKey::Pushover(_)));
        }
        let ctx = crate::summary::metrics_ctx_from_results(
            &self.core.model,
            self.core.scoped.results.as_ref(),
        );
        if ctx.seismic_x.is_some() && ctx.seismic_y.is_some() {
            for dir in [SeismicDir::X, SeismicDir::Y] {
                let key = ResultInputKey::Static(StaticCaseKey::Seismic(dir));
                if !dependencies.contains(&key) {
                    dependencies.push(key);
                }
            }
            if let Some((name, _)) = self.core.scoped.results.as_ref().and_then(|r| {
                r.combos.iter().find(|(name, _)| {
                    sepika_core::load_combo::is_gravity_combination(name, &self.core.model)
                })
            }) {
                dependencies.push(ResultInputKey::Combo(name.clone()));
            }
        }
        let reasons: Vec<_> = dependencies
            .into_iter()
            .filter_map(|key| self.require_result_input(key).err())
            .collect();
        if !reasons.is_empty() {
            return Err(reasons.join("\n"));
        }
        let latest = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.pushover_for_dir(view_dir));
        let po = saved_ds
            .map(|e| &e.run)
            .or(latest)
            .ok_or("増分解析未実行です。増分解析を実行してください")?;
        let capacity_po = saved_capacity
            .map(|e| &e.run)
            .or(latest)
            .ok_or("保有耐力比較用の増分解析結果がありません")?;
        let ds_conditions = saved_ds.map_or(self.core.analysis_cfg, |e| e.conditions);
        let capacity_conditions = saved_capacity.map_or(self.core.analysis_cfg, |e| e.conditions);
        let st = self
            .core
            .scoped
            .results
            .as_ref()
            .and_then(|r| r.seismic(view_dir))
            .ok_or_else(|| {
                "静的解析結果がありません。地震静的(Ai)を実行してください。".to_string()
            })?;

        let metrics =
            crate::summary::compute_story_metrics_with(&self.core.model, &st.disp, view_dir, &ctx);

        let fes: Vec<f64> = metrics
            .iter()
            .map(|m| m.fes.clone())
            .collect::<Result<_, _>>()?;
        let rs: Vec<f64> = metrics
            .iter()
            .map(|m| m.rs.clone())
            .collect::<Result<_, _>>()?;
        let re: Vec<f64> = metrics
            .iter()
            .map(|m| m.re.clone())
            .collect::<Result<_, _>>()?;

        if sepika_load::wall_expand::model_has_wall_plates_to_expand(&self.core.model) {
            let generation = po
                .wall_run
                .as_ref()
                .and_then(|run| run.input_generation.as_ref())
                .ok_or_else(|| {
                    "壁応答の入力識別情報がありません。増分解析を再実行してください".to_string()
                })?;
            if *generation
                != self
                    .result_input_with_settings(&ResultInputKey::Pushover(view_dir), ds_conditions)
            {
                return Err("壁応答の生成入力が現在の計算入力と一致しません".into());
            }
        }
        let ds_point = saved_ds
            .map(|e| &e.point)
            .or(po.ds_evaluation.as_ref())
            .ok_or("Ds判定用の評価点を明示してください")?;
        let capacity_point = saved_capacity
            .map(|e| &e.point)
            .or(capacity_po.capacity_evaluation.as_ref())
            .ok_or("保有耐力比較用の評価点を明示してください")?;
        for (point, conditions) in [
            (ds_point, ds_conditions),
            (capacity_point, capacity_conditions),
        ] {
            if point.direction != view_dir
                || point.input_generation
                    != self
                        .result_input_with_settings(&ResultInputKey::Pushover(view_dir), conditions)
            {
                return Err("評価点の方向・生成入力が現在の計算入力と一致しません".into());
            }
        }
        let ds_response = po.confirmed_response(ds_point, EvaluationPurpose::Ds)?;
        let ds_forces = po.evaluate_stories(ds_point, EvaluationPurpose::Ds)?;
        let capacity_forces =
            capacity_po.evaluate_stories(capacity_point, EvaluationPurpose::HoldingCapacity)?;
        if ds_forces.len() != n_stories || capacity_forces.len() != n_stories {
            return Err("評価点の層数が現在モデルと一致しません".into());
        }
        let expanded_storage;
        let evaluation_model =
            if sepika_load::wall_expand::model_has_wall_plates_to_expand(&self.core.model) {
                expanded_storage =
                    sepika_load::wall_expand::expand_wall_elements(&self.core.model).0;
                &expanded_storage
            } else {
                &self.core.model
            };
        for (point, run) in [(ds_point, po), (capacity_point, capacity_po)] {
            let record = run.confirmed_response(point, point.purpose)?;
            for cut in &record.cuts {
                let expected: std::collections::HashSet<_> = evaluation_model
                    .elements
                    .iter()
                    .filter(|e| !matches!(e.kind, sepika_core::model::ElementKind::PanelZone))
                    .filter(|e| {
                        e.nodes
                            .iter()
                            .filter_map(|n| evaluation_model.nodes.get(n.index()))
                            .any(|n| n.coord[2] > cut.elevation_mm)
                            && e.nodes
                                .iter()
                                .filter_map(|n| evaluation_model.nodes.get(n.index()))
                                .any(|n| n.coord[2] < cut.elevation_mm)
                    })
                    .map(|e| e.id)
                    .collect();
                let recorded: std::collections::HashSet<_> =
                    cut.forces.iter().map(|f| f.elem).collect();
                if expected != recorded {
                    return Err("層切断面の物理要素履歴がモデルと一致しません".into());
                }
            }
        }
        for elem in evaluation_model
            .elements
            .iter()
            .filter(|e| matches!(e.kind, sepika_core::model::ElementKind::Wall))
        {
            for (point, run) in [(ds_point, po), (capacity_point, capacity_po)] {
                run.wall_response_at(elem.id, point.step)
                    .map_err(|reason| format!("壁要素 {:?}: {}", elem.id, reason.description()))?;
            }
        }
        if matches!(
            self.core.design_frame,
            sepika_design_jp::secondary::holding_capacity::FrameType::RcWall
                | sepika_design_jp::secondary::holding_capacity::FrameType::SteelBrace
        ) && ds_forces
            .iter()
            .any(|f| f.wall_n == 0.0 && f.brace_n == 0.0)
        {
            return Err(
                "宣言した壁・筋かい架構の負担力がありません。架構・方向・評価点を確認してください"
                    .into(),
            );
        }
        let ds_step = ds_point.step;
        let resp_by_elem: std::collections::HashMap<_, _> =
            ds_response.members.iter().map(|r| (r.elem, *r)).collect();
        let shear_yield_elems: std::collections::HashSet<ElemId> = po
            .shear_yields
            .iter()
            .filter(|s| s.step <= ds_step)
            .map(|s| s.elem)
            .collect();
        let mut capacity_seen = std::collections::HashSet::new();
        if self.core.design_rank_auto
            && ds_point
                .member_capacities_n
                .iter()
                .any(|(elem, q)| !capacity_seen.insert(*elem) || !q.is_finite() || *q <= 0.0)
        {
            return Err("部材群の耐力入力が重複・非有限・非正です".into());
        }
        let mut cb_members: Vec<Vec<(u8, f64)>> = vec![Vec::new(); n_stories];
        let mut wall_members: Vec<Vec<(u8, f64)>> = vec![Vec::new(); n_stories];

        let (story_ranks, member_ranks): (Vec<MemberRank>, Vec<(ElemId, MemberRank)>) = if self
            .core
            .design_rank_auto
        {
            let mut per_story: Vec<Vec<MemberRank>> = vec![Vec::new(); n_stories];
            let mut computed: Vec<(ElemId, MemberRank)> = Vec::new();
            let expanded_storage;
            let wall_index;
            let model: &sepika_core::model::Model =
                if sepika_load::wall_expand::model_has_wall_plates_to_expand(&self.core.model) {
                    let (expanded, index, _wall_report) =
                        sepika_load::wall_expand::expand_wall_elements(&self.core.model);
                    wall_index = index;
                    expanded_storage = expanded;
                    &expanded_storage
                } else {
                    wall_index = Default::default();
                    &self.core.model
                };
            for elem in &model.elements {
                if matches!(elem.kind, sepika_core::model::ElementKind::Wall) {
                    po.wall_response_at(elem.id, ds_step).map_err(|reason| {
                        format!("壁要素 {:?}: {}", elem.id, reason.description())
                    })?;
                }
                if !matches!(
                    elem.kind,
                    sepika_core::model::ElementKind::Wall
                        | sepika_core::model::ElementKind::Beam
                        | sepika_core::model::ElementKind::Fiber
                        | sepika_core::model::ElementKind::MultiSpring
                        | sepika_core::model::ElementKind::Brace { .. }
                ) {
                    continue;
                }
                let Some(sec) = elem.section.and_then(|sid| model.sections.get(sid.index())) else {
                    if matches!(elem.kind, sepika_core::model::ElementKind::Wall) {
                        return Err(format!("壁要素 {:?}: 断面が未設定です", elem.id));
                    }
                    continue;
                };
                let Some(mat) = model.element_material(elem) else {
                    if matches!(elem.kind, sepika_core::model::ElementKind::Wall) {
                        return Err(format!("壁要素 {:?}: 材料が未設定です", elem.id));
                    }
                    if elem
                        .section
                        .and_then(|sid| model.sections.get(sid.index()))
                        .is_some_and(|section| {
                            matches!(
                                section.shape,
                                Some(
                                    SectionShape::RcBeamRect { .. }
                                        | SectionShape::RcColumnRect { .. }
                                        | SectionShape::RcColumnCircle { .. }
                                        | SectionShape::SrcBeamRect { .. }
                                        | SectionShape::SrcColumnRect { .. }
                                )
                            )
                        })
                    {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 材料が未設定です",
                            elem.id,
                            elem.section.unwrap()
                        ));
                    }
                    continue;
                };
                let rebar_mat = model.element_rebar_material(elem);
                let shear_mat = model.element_shear_rebar_material(elem);
                let steel_grade = model
                    .element_steel_material(elem)
                    .map(|m| m.name.clone())
                    .unwrap_or_default();
                let is_brace_elem =
                    matches!(elem.kind, sepika_core::model::ElementKind::Brace { .. })
                        || (!matches!(elem.kind, sepika_core::model::ElementKind::Wall)
                            && sepika_design_jp::MemberKind::of_element(elem, model)
                                == sepika_design_jp::MemberKind::Brace);
                let elem_steel = elem_is_steel(elem, model);
                if elem_steel && matches!(elem.kind, sepika_core::model::ElementKind::Wall) {
                    return Err(format!(
                        "壁要素 {:?}: 鋼板壁のランク判定は未対応です",
                        elem.id
                    ));
                }
                let thickness = sepika_design_jp::material_strength::plate_thickness(sec);
                let grade = if elem_steel {
                    mat.name.as_str()
                } else {
                    steel_grade.as_str()
                };
                if let Some(reason) =
                    sepika_core::material_grade::steel_f_value_issue(grade, thickness)
                {
                    return Err(format!("部材 {:?}・断面 {:?}: {reason}", elem.id, sec.id));
                }
                let rank = if is_brace_elem && elem_steel {
                    let len = model.member_length(elem);
                    let i_min = sec.iy.min(sec.iz);
                    if sec.area <= 0.0 || i_min <= 0.0 || len <= 0.0 {
                        continue;
                    }
                    let radius = (i_min / sec.area).sqrt();
                    if radius <= 0.0 {
                        continue;
                    }
                    let f_value =
                        sepika_design_jp::material_strength::section_steel_f(sec, Some(mat))
                            .map_err(|reason| {
                                format!("部材 {:?}・断面 {:?}: {reason}", elem.id, sec.id)
                            })?;
                    steel_brace_type(len / radius, f_value)
                } else if elem_steel {
                    let Some(shape) = sec.shape.as_ref() else {
                        continue;
                    };
                    let member_use = steel_member_use_of(elem, model);
                    let Some(rank) = steel_width_thickness_rank(shape, member_use, &mat.name)
                    else {
                        continue;
                    };
                    rank
                } else if matches!(
                    sec.shape.as_ref(),
                    Some(SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. },)
                ) {
                    use sepika_design_jp::secondary::src_rank::{
                        src_column_rank, src_column_rank_ratios,
                    };
                    if sepika_design_jp::MemberKind::of_element(elem, model)
                        != sepika_design_jp::MemberKind::Column
                    {
                        continue;
                    }
                    let Some(fc) = mat.fc.filter(|f| *f > 0.0) else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: Fc が未設定または 0 以下です",
                            elem.id, sec.id
                        ));
                    };
                    let Some(resp) = resp_by_elem.get(&elem.id) else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 増分解析応答がありません",
                            elem.id, sec.id
                        ));
                    };
                    let shape = sec.shape.as_ref().expect("SRC 矩形柱と判定済み");
                    if shape.beam_rebar().is_none() && shape.rect_column_rebar().is_none() {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 配筋が未設定です",
                            elem.id, sec.id
                        ));
                    }
                    let Some(rebar_sy) =
                        sepika_core::material_grade::rebar_yield_strength(rebar_mat).or(mat.fy)
                    else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 主筋の降伏強度を解決できません",
                            elem.id, sec.id
                        ));
                    };
                    let Some((n_n0, smo_m0)) =
                        src_column_rank_ratios(shape, &steel_grade, fc, rebar_sy, resp.axial)
                    else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: SRC 柱のランク判定比を算定できません",
                            elem.id, sec.id
                        ));
                    };
                    src_column_rank(n_n0, smo_m0, shear_yield_elems.contains(&elem.id))
                } else if let Some(SectionShape::RcWall { thickness, .. }) = sec.shape.as_ref() {
                    if wall_has_src_boundary_column(elem, model) {
                        let capacities = po
                            .wall_capacities
                            .iter()
                            .find(|(id, _)| *id == elem.id)
                            .map(|(_, qu)| *qu)
                            .ok_or_else(|| {
                                format!("壁 ID {}: 採用runの初期Quが未解決です", elem.id.0)
                            })?;
                        let wall = po
                            .wall_response_at(elem.id, ds_step)
                            .map_err(|reason| reason.description().to_string())?;
                        let qu = capacities[usize::from(wall.qw_n < 0.0)];
                        if !qu.is_finite() || qu <= 0.0 {
                            return Err(format!(
                                "壁要素 {:?}: 正負別せん断耐力を算定できません",
                                elem.id
                            ));
                        }
                        let shear_failure = wall.qw_n.abs() >= 0.99 * qu;
                        sepika_design_jp::secondary::src_rank::src_wall_type(shear_failure)
                    } else {
                        if mat.category != sepika_core::model::MaterialCategory::Concrete {
                            return Err(format!("壁要素 {:?}: RC材料を確認できません", elem.id));
                        }
                        let fc = mat
                            .fc
                            .filter(|v| v.is_finite() && *v > 0.0)
                            .ok_or_else(|| {
                                format!("壁要素 {:?}: Fc が未設定または不正です", elem.id)
                            })?;
                        let wall = po
                            .wall_response_at(elem.id, ds_step)
                            .map_err(|reason| reason.description().to_string())?;
                        let wgeom =
                            sepika_element::wall::wall_element::wall_element_geometry(elem, model)
                                .ok_or_else(|| format!("壁要素 {:?}: 壁幾何が不正です", elem.id))?;
                        let geometry =
                            rc_wall_ds_geometry(elem, model, wall_index.plate_of(elem.id))
                                .map_err(|reason| format!("壁要素 {:?}: {reason}", elem.id))?;
                        let Some(tau_over_fc) =
                            rc_wall_tau_over_fc(wall.qw_n, *thickness, wgeom.lw, fc)
                        else {
                            return Err(format!("壁要素 {:?}: τu/Fc の入力が不正です", elem.id));
                        };
                        let capacities = po
                            .wall_capacities
                            .iter()
                            .find(|(id, _)| *id == elem.id)
                            .map(|(_, qu)| *qu)
                            .ok_or_else(|| {
                                format!("壁 ID {}: 採用runの初期Quが未解決です", elem.id.0)
                            })?;
                        let wall = po
                            .wall_response_at(elem.id, ds_step)
                            .map_err(|reason| reason.description().to_string())?;
                        let qu = capacities[usize::from(wall.qw_n < 0.0)];
                        if !qu.is_finite() || qu <= 0.0 {
                            return Err(format!(
                                "壁要素 {:?}: 正負別せん断耐力を算定できません",
                                elem.id
                            ));
                        }
                        let brittle = rc_wall_shear_brittle(wall.qw_n.abs(), qu);
                        let eligible = geometry.columns.map(|(_, area, diameter)| {
                            sepika_design_jp::secondary::ds_group::rc_wall_side_column_eligible(
                                geometry.clear_length_mm,
                                geometry.clear_height_mm,
                                *thickness,
                                area,
                                diameter,
                            )
                            .ok_or_else(|| {
                                format!("壁要素 {:?}: 側柱寸法条件の入力が不正です", elem.id)
                            })
                        });
                        let [left, right] = eligible;
                        let eligible = [left?, right?];
                        let wall_structure = self.core.wall_structure || eligible.contains(&false);
                        self.core.scoped.ds_wall_details.push(format!(
                            "壁 ID {} / 壁版 ID {}: Q_Ds={:.3} N, t={:.3} mm, l_cc={:.3} mm, Fc={:.3} N/mm², τu/Fc={:.9}; 内法 {:.3}×{:.3} mm; 側柱 ID {} (Ac={:.3} mm², 最小径={:.3} mm): {} / ID {} (Ac={:.3} mm², 最小径={:.3} mm): {}; 適用表: {}。2015年版引用（日本建築防災協会 2024年3月報告書 本文44頁）",
                            elem.id.0, geometry.plate.0, wall.qw_n, thickness, wgeom.lw, fc, tau_over_fc,
                            geometry.clear_length_mm, geometry.clear_height_mm,
                            geometry.columns[0].0.0, geometry.columns[0].1, geometry.columns[0].2,
                            if eligible[0] { "適格" } else { "寸法不足" },
                            geometry.columns[1].0.0, geometry.columns[1].1, geometry.columns[1].2,
                            if eligible[1] { "適格" } else { "寸法不足" },
                            if wall_structure { "壁式構造用" } else { "通常" },
                        ));
                        rc_wall_type(tau_over_fc, wall_structure, brittle)
                    }
                } else if let Some(SectionShape::RcBeamRect { b, d, rebar }) = sec.shape.as_ref() {
                    if sepika_design_jp::MemberKind::of_element(elem, model)
                        != sepika_design_jp::MemberKind::Girder
                    {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 新型RC梁断面の用途が梁ではありません",
                            elem.id, sec.id
                        ));
                    }
                    let Some(resp) = resp_by_elem.get(&elem.id) else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 増分解析応答がありません",
                            elem.id, sec.id
                        ));
                    };
                    let geom_len = model.member_length(elem);
                    let face_sum =
                        elem.rigid_zone.face_i_or_zero() + elem.rigid_zone.face_j_or_zero();
                    let clear_span = if geom_len - face_sum > 0.0 {
                        geom_len - face_sum
                    } else {
                        geom_len
                    };
                    rank_new_rc_beam(
                        elem.id, sec.id, *b, *d, rebar, mat, rebar_mat, shear_mat, resp, clear_span,
                    )?
                } else if let Some(
                    shape @ (SectionShape::RcColumnRect { .. }
                    | SectionShape::RcColumnCircle { .. }),
                ) = sec.shape.as_ref()
                {
                    if sepika_design_jp::MemberKind::of_element(elem, model)
                        != sepika_design_jp::MemberKind::Column
                    {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 新型RC柱断面の用途が柱ではありません",
                            elem.id, sec.id
                        ));
                    }
                    let Some(resp) = resp_by_elem.get(&elem.id) else {
                        return Err(format!(
                            "部材 {:?}・断面 {:?}: 増分解析応答がありません",
                            elem.id, sec.id
                        ));
                    };
                    let geom_len = model.member_length(elem);
                    let face_sum =
                        elem.rigid_zone.face_i_or_zero() + elem.rigid_zone.face_j_or_zero();
                    let clear_span = if geom_len - face_sum > 0.0 {
                        geom_len - face_sum
                    } else {
                        geom_len
                    };
                    match shape {
                        SectionShape::RcColumnRect { b, d, rebar } => rank_new_rc_column_rect(
                            elem.id, sec.id, *b, *d, rebar, mat, rebar_mat, shear_mat, resp,
                            clear_span,
                        )?,
                        SectionShape::RcColumnCircle { d, rebar } => rank_new_rc_circle_column(
                            elem.id, sec.id, *d, rebar, mat, rebar_mat, shear_mat, resp, clear_span,
                        )?,
                        _ => unreachable!(),
                    }
                } else {
                    continue;
                };
                let Some(story_idx) = elem
                    .nodes
                    .iter()
                    .filter_map(|nid| model.nodes.get(nid.index()))
                    .filter_map(|n| n.story)
                    .max()
                else {
                    continue;
                };
                let Some(idx) = story_idx.index().checked_sub(1) else {
                    continue;
                };
                if idx >= n_stories {
                    continue;
                }
                per_story[idx].push(rank);
                computed.push((elem.id, rank));

                let capacity_n = ds_point
                    .member_capacities_n
                    .iter()
                    .find(|(id, _)| *id == elem.id)
                    .map(|(_, q)| *q)
                    .ok_or_else(|| {
                        format!(
                            "部材 {:?}: 群判定用の耐力入力がありません。負担力では代用できません",
                            elem.id
                        )
                    })?;
                let gi = rank_index_for_group(rank);
                let group = ds_response
                    .cuts
                    .iter()
                    .flat_map(|c| &c.forces)
                    .find(|f| f.elem == elem.id)
                    .map(|f| f.group);
                if matches!(group, Some(ForceGroup::Wall | ForceGroup::Brace)) {
                    wall_members[idx].push((gi, capacity_n));
                } else {
                    cb_members[idx].push((gi, capacity_n));
                }
            }
            if per_story.iter().any(|ranks| ranks.is_empty()) {
                return Err("自動部材ランクの確定評価データが不足しています".into());
            }
            for cut in &ds_response.cuts {
                for force in &cut.forces {
                    if !computed.iter().any(|(id, _)| *id == force.elem) {
                        return Err(format!(
                            "部材 {:?}: 自動部材ランクの確定評価データが不足しています",
                            force.elem
                        ));
                    }
                }
            }
            let ranks: Vec<MemberRank> = per_story
                .into_iter()
                .map(|rs| worst_rank(&rs).expect("非空の部材ランク集合"))
                .collect();
            (ranks, computed)
        } else {
            self.core.scoped.ds_rank_fallback_stories = Vec::new();
            (vec![self.core.design_rank; n_stories], Vec::new())
        };

        let mechanism = &po.mechanism_at(ds_point, evaluation_model)?;
        let is_rc_frame = matches!(
            self.core.design_frame,
            sepika_design_jp::secondary::holding_capacity::FrameType::RcFrame
                | sepika_design_jp::secondary::holding_capacity::FrameType::RcWall
        );
        let beta_u_by_story: Vec<_> = ds_forces.iter().map(|f| f.beta_u).collect();
        let ds_vec: Vec<f64> = (0..n_stories)
            .map(|i| {
                let explicit_group = |rank: MemberRank| match rank {
                    MemberRank::FA => GroupType::A,
                    MemberRank::FB => GroupType::B,
                    MemberRank::FC => GroupType::C,
                    MemberRank::FD => GroupType::D,
                };
                let rep_rank = story_ranks[i];
                let mut cb_group = if self.core.design_rank_auto {
                    member_group(&cb_members[i]).ok_or_else(|| {
                        if ds_response.cuts[i].forces.iter().any(|f| f.group == ForceGroup::Frame) {
                            format!("層{}: 存在する柱梁群の耐力入力が欠損しています", i+1)
                        } else {
                            format!("層{}: 柱梁群を持たない構造の自動Ds判定は未対応です。明示設計入力が必要です", i+1)
                        }
                    })?
                } else {
                    explicit_group(rep_rank)
                };
                if let MechanismType::StoryCollapse { layer } = mechanism {
                    if *layer == i {
                        cb_group = match cb_group {
                            GroupType::A => GroupType::B,
                            GroupType::B => GroupType::C,
                            GroupType::C | GroupType::D => GroupType::D,
                        };
                    }
                }
                if beta_u_by_story[i] == 0.0 {
                    let rank = match cb_group {
                        GroupType::A => MemberRank::FA,
                        GroupType::B => MemberRank::FB,
                        GroupType::C => MemberRank::FC,
                        GroupType::D => MemberRank::FD,
                    };
                    let frame = if is_rc_frame {
                        sepika_design_jp::secondary::holding_capacity::FrameType::RcFrame
                    } else {
                        sepika_design_jp::secondary::holding_capacity::FrameType::SteelFrame
                    };
                    return Ok(sepika_design_jp::secondary::holding_capacity::ds_value(
                        frame, rank,
                    ));
                }
                let wall_group = if self.core.design_rank_auto {
                    member_group(&wall_members[i]).ok_or_else(|| {
                        format!("層{}: 壁・筋かい群の耐力入力が欠損しています", i + 1)
                    })?
                } else {
                    explicit_group(rep_rank)
                };
                if is_rc_frame {
                    Ok(ds_rc(wall_group, beta_u_by_story[i], cb_group))
                } else {
                    Ok(ds_steel(wall_group, beta_u_by_story[i], cb_group))
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.core.scoped.ds_beta_u_by_story = beta_u_by_story;
        let heights: Vec<f64> = metrics.iter().map(|m| m.height).collect();

        let capacity_curve = capacity_po
            .capacity_curve
            .iter()
            .find(|p| p.step == capacity_point.step)
            .ok_or("比較評価点の変形履歴がありません")?;
        let qu_n: Vec<_> = capacity_forces.iter().map(|f| f.qu_n).collect();
        let result = check_holding_capacity_at_state(
            &qu_n,
            &capacity_curve.story_drift,
            &qud,
            &ds_vec,
            &fes,
            &rs,
            &re,
            &heights,
            member_ranks,
        )?;
        self.core.scoped.holding_capacity_source = Some(HoldingCapacitySource {
            direction: view_dir,
            qu_steps: vec![Some(capacity_point.step); n_stories],
            response_step: Some(ds_step),
            ds_point: ds_point.clone(),
            capacity_point: capacity_point.clone(),
            ds_forces,
            capacity_forces: capacity_forces.clone(),
            termination: po.termination.clone(),
            capacity_termination: capacity_po.termination.clone(),
            ds_mechanism: mechanism.clone(),
            ds_conditions,
            capacity_conditions,
        });
        Ok((result, story_ranks))
    }
}

#[allow(clippy::too_many_arguments)]
fn rank_new_rc_column_rect(
    elem_id: ElemId,
    section_id: sepika_core::ids::SectionId,
    b: f64,
    d: f64,
    rebar: &sepika_core::section_shape::RcRectColumnRebar,
    mat: &sepika_core::model::Material,
    rebar_mat: Option<&sepika_core::model::Material>,
    shear_mat: Option<&sepika_core::model::Material>,
    response: &sepika_solver::nonlinear::pushover::PushoverMemberResponse,
    clear_span: f64,
) -> Result<sepika_design_jp::secondary::holding_capacity::MemberRank, String> {
    use sepika_core::rc_capacity::{rc_column_mu_simple, rc_qsu_simple, RcCapacityInput};
    use sepika_core::rc_rebar_geom::RectEdge;
    use sepika_design_jp::secondary::ds_group::rc_column_type;
    let error = |s: &str| format!("部材 {:?}・断面 {:?}: {}", elem_id, section_id, s);
    if rebar.is_unset() {
        return Err(error("配筋が未設定です"));
    }
    rebar
        .validate(b, d)
        .map_err(|_| error("配筋形状が不正です"))?;
    validate_new_column_materials(mat, rebar_mat, shear_mat, clear_span).map_err(error)?;
    if !b.is_finite() || !d.is_finite() || b <= 0.0 || d <= 0.0 {
        return Err(error("断面寸法が不正です"));
    }
    let fc = mat.fc.unwrap();
    let sigma_y = sepika_core::material_grade::rebar_yield_strength(rebar_mat)
        .or(mat.fy)
        .ok_or_else(|| error("主筋の降伏強度を解決できません"))?;
    let sigma_wy = sepika_core::material_grade::shear_rebar_yield_strength(shear_mat)
        .ok_or_else(|| error("せん断補強筋の降伏強度を解決できません"))?;
    let ag = rebar.total_main_area();
    let eval = |strong: bool, shear: f64| {
        let (bd, dd, edge, aw, legs) = if strong {
            (b, d, RectEdge::Top, rebar.aw_x_mm2(), rebar.hoop.legs_x)
        } else {
            (d, b, RectEdge::Left, rebar.aw_y_mm2(), rebar.hoop.legs_y)
        };
        let steel = rebar.edge_steel(edge, b, d);
        let pitch = rebar.hoop.pitch;
        let pw = if bd <= 0.0 || pitch <= 0.0 {
            0.0
        } else {
            aw / (bd * pitch)
        };
        let input = RcCapacityInput {
            b: bd,
            d: dd,
            at: steel.area_mm2,
            d_eff: steel.effective_depth_mm,
            sigma_y,
            fc,
            pw,
            sigma_wy,
            clear_span,
            sigma_0: response.axial / (b * d),
        };
        let qmu = 2.0 * rc_column_mu_simple(&input, ag, response.axial) / clear_span;
        let qsu = rc_qsu_simple(&input);
        let rank = rc_column_type(
            clear_span / dd,
            response.axial / (b * d * fc),
            100.0 * input.at / (bd * input.d_eff),
            (shear / (bd)) / fc,
            qsu < qmu,
        );
        let _ = legs;
        rank
    };
    Ok(sepika_design_jp::secondary::member_rank::worst_rank(&[
        eval(true, response.shear_strong.abs()),
        eval(false, response.shear_weak.abs()),
    ])
    .expect("二軸の候補は空ではありません"))
}

#[allow(clippy::too_many_arguments)]
fn rank_new_rc_circle_column(
    elem_id: ElemId,
    section_id: sepika_core::ids::SectionId,
    d: f64,
    rebar: &sepika_core::section_shape::RcCircleColumnRebar,
    mat: &sepika_core::model::Material,
    rebar_mat: Option<&sepika_core::model::Material>,
    shear_mat: Option<&sepika_core::model::Material>,
    response: &sepika_solver::nonlinear::pushover::PushoverMemberResponse,
    clear_span: f64,
) -> Result<sepika_design_jp::secondary::holding_capacity::MemberRank, String> {
    use sepika_core::rc_capacity::{rc_column_mu_simple, rc_qsu_simple, RcCapacityInput};
    use sepika_design_jp::secondary::ds_group::rc_column_type;
    let error = |s: &str| format!("部材 {:?}・断面 {:?}: {}", elem_id, section_id, s);
    if rebar.is_unset() {
        return Err(error("配筋が未設定です"));
    }
    rebar.validate(d).map_err(|_| error("配筋形状が不正です"))?;
    validate_new_column_materials(mat, rebar_mat, shear_mat, clear_span).map_err(error)?;
    if !d.is_finite() || d <= 0.0 {
        return Err(error("断面寸法が不正です"));
    }
    let fc = mat.fc.unwrap();
    let side = rebar.equivalent_square_side_mm(d);
    let sigma_y = sepika_core::material_grade::rebar_yield_strength(rebar_mat)
        .or(mat.fy)
        .ok_or_else(|| error("主筋の降伏強度を解決できません"))?;
    let sigma_wy = sepika_core::material_grade::shear_rebar_yield_strength(shear_mat)
        .ok_or_else(|| error("せん断補強筋の降伏強度を解決できません"))?;
    let input = RcCapacityInput {
        b: side,
        d: side,
        at: rebar.equivalent_tension_area_mm2(),
        d_eff: rebar.equivalent_effective_depth_mm(d),
        sigma_y,
        fc,
        pw: rebar.pw(side),
        sigma_wy,
        clear_span,
        sigma_0: response.axial / (side * side),
    };
    let qmu =
        2.0 * rc_column_mu_simple(&input, rebar.total_main_area(), response.axial) / clear_span;
    let qsu = rc_qsu_simple(&input);
    let rank = |shear: f64| {
        rc_column_type(
            clear_span / d,
            response.axial / (side * side * fc),
            100.0 * input.at / (side * input.d_eff),
            (shear / (side * side)) / fc,
            qsu < qmu,
        )
    };
    Ok(sepika_design_jp::secondary::member_rank::worst_rank(&[
        rank(response.shear_strong.abs()),
        rank(response.shear_weak.abs()),
    ])
    .expect("二軸の候補は空ではありません"))
}

fn validate_new_column_materials(
    mat: &sepika_core::model::Material,
    rebar_mat: Option<&sepika_core::model::Material>,
    shear_mat: Option<&sepika_core::model::Material>,
    clear_span: f64,
) -> Result<(), &'static str> {
    if mat.fc.filter(|v| v.is_finite() && *v > 0.0).is_none() {
        return Err("Fc が未設定または 0 以下です");
    }
    if !clear_span.is_finite() || clear_span <= 0.0 {
        return Err("内法スパンが 0 以下です");
    }
    if sepika_core::material_grade::rebar_yield_strength(rebar_mat)
        .or(mat.fy)
        .filter(|v| v.is_finite() && *v > 0.0)
        .is_none()
    {
        return Err("主筋の降伏強度を解決できません");
    }
    if sepika_core::material_grade::shear_rebar_yield_strength(shear_mat)
        .filter(|v| v.is_finite() && *v > 0.0)
        .is_none()
    {
        return Err("せん断補強筋の降伏強度を解決できません");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rank_new_rc_beam(
    elem_id: ElemId,
    section_id: sepika_core::ids::SectionId,
    b: f64,
    d: f64,
    rebar: &sepika_core::section_shape::RcBeamRebar,
    mat: &sepika_core::model::Material,
    rebar_mat: Option<&sepika_core::model::Material>,
    shear_mat: Option<&sepika_core::model::Material>,
    response: &sepika_solver::nonlinear::pushover::PushoverMemberResponse,
    clear_span: f64,
) -> Result<sepika_design_jp::secondary::holding_capacity::MemberRank, String> {
    let error = |message: &str| format!("部材 {:?}・断面 {:?}: {}", elem_id, section_id, message);
    rebar
        .validate(b, d)
        .map_err(|_| error("配筋形状が不正です"))?;
    let fc = mat
        .fc
        .filter(|fc| *fc > 0.0)
        .ok_or_else(|| error("Fc が未設定または 0 以下です"))?;
    if clear_span <= 0.0 {
        return Err(error("内法スパンが 0 以下です"));
    }
    let sigma_y = sepika_core::material_grade::rebar_yield_strength(rebar_mat)
        .or(mat.fy)
        .ok_or_else(|| error("主筋の降伏強度を解決できません"))?;
    let sigma_wy = sepika_core::material_grade::shear_rebar_yield_strength(shear_mat)
        .ok_or_else(|| error("せん断補強筋の降伏強度を解決できません"))?;
    if rebar.is_unset() {
        return Err(error("配筋が未設定です"));
    }
    let pw = rebar.pw(b);
    let (top_rank, _, _) = rank_new_rc_beam_side(
        b,
        d,
        rebar,
        true,
        sigma_y,
        fc,
        pw,
        sigma_wy,
        clear_span,
        response.axial,
        response.shear_strong.abs(),
    );
    let (bottom_rank, _, _) = rank_new_rc_beam_side(
        b,
        d,
        rebar,
        false,
        sigma_y,
        fc,
        pw,
        sigma_wy,
        clear_span,
        response.axial,
        response.shear_strong.abs(),
    );
    Ok(worst_rc_beam_rank(top_rank, bottom_rank))
}

fn worst_rc_beam_rank(
    top: sepika_design_jp::secondary::holding_capacity::MemberRank,
    bottom: sepika_design_jp::secondary::holding_capacity::MemberRank,
) -> sepika_design_jp::secondary::holding_capacity::MemberRank {
    sepika_design_jp::secondary::member_rank::worst_rank(&[top, bottom])
        .expect("上下側のランクは空ではありません")
}

#[allow(clippy::too_many_arguments)]
fn rank_new_rc_beam_side(
    b: f64,
    d: f64,
    rebar: &sepika_core::section_shape::RcBeamRebar,
    tension_is_top: bool,
    sigma_y: f64,
    fc: f64,
    pw: f64,
    sigma_wy: f64,
    clear_span: f64,
    axial: f64,
    shear: f64,
) -> (
    sepika_design_jp::secondary::holding_capacity::MemberRank,
    f64,
    f64,
) {
    use sepika_core::rc_capacity::{rc_qmu_simple, rc_qsu_simple, RcCapacityInput};
    use sepika_design_jp::secondary::ds_group::rc_beam_type;

    let bending = rebar.bending_steel(d, tension_is_top);
    let input = RcCapacityInput {
        b,
        d,
        at: bending.tension.area_mm2,
        d_eff: bending.tension.effective_depth_mm,
        sigma_y,
        fc,
        pw,
        sigma_wy,
        clear_span,
        sigma_0: axial / (b * d),
    };
    let qmu = rc_qmu_simple(&input);
    let qsu = rc_qsu_simple(&input);
    let tau_over_fc = (shear / (b * d)) / fc;
    let brittle = qmu > 0.0 && qsu < qmu;
    (rc_beam_type(tau_over_fc, brittle), qmu, qsu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::model::Material;
    use sepika_core::section_shape::{BeamStirrup, RcBeamRebar};
    use sepika_design_jp::secondary::holding_capacity::MemberRank;
    use sepika_design_jp::secondary::member_rank::worst_rank;

    fn single_wall_619_model(clear_height_mm: f64) -> sepika_core::model::Model {
        use sepika_core::section_shape::SectionShape;
        let mut model = crate::app::rc_wall617_tests::fixture::input_model();
        for node in &mut model.nodes {
            if node.coord[0] == 4000.0 {
                node.coord[0] = 3000.0;
            }
            if node.coord[2] == 3000.0 {
                node.coord[2] = clear_height_mm + 400.0;
            }
        }
        model.stories[1].elevation = clear_height_mm + 400.0;
        let column_rebar = match model.sections[0].shape.clone().unwrap() {
            SectionShape::RcColumnRect { rebar, .. } => rebar,
            _ => unreachable!(),
        };
        let shapes = [
            SectionShape::RcColumnRect {
                b: 400.0,
                d: 400.0,
                rebar: column_rebar,
            },
            SectionShape::RcBeamRect {
                b: 400.0,
                d: 400.0,
                rebar: RcBeamRebar {
                    main_dia: 22.0,
                    top: vec![4],
                    bottom: vec![4],
                    cover: 40.0,
                    stirrup: BeamStirrup {
                        dia: 10.0,
                        pitch: 100.0,
                        legs: 2,
                    },
                },
            },
            SectionShape::RcWall {
                thickness: 200.0,
                ps: 0.002,
                pwh_ratio: Some(0.004),
                pwv_ratio: Some(0.002),
            },
        ];
        for (section, shape) in model.sections.iter_mut().zip(shapes) {
            let old = section.clone();
            *section = shape.to_section(old.id, old.name);
            section.frame_use = old.frame_use;
            section.material = old.material;
            section.rebar_material = old.rebar_material;
            section.shear_rebar_material = old.shear_rebar_material;
        }
        model
    }

    fn wall_619_geometry(model: &sepika_core::model::Model) -> Result<RcWallDsGeometry, String> {
        let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(model);
        let id = index.generated_elem_ids().next().unwrap();
        rc_wall_ds_geometry(expanded.element(id).unwrap(), &expanded, index.plate_of(id))
    }

    #[test]
    fn rc_wall_619_real_support_faces_and_side_columns_select_table_without_changing_stress() {
        use sepika_design_jp::secondary::ds_group::{
            rc_wall_side_column_eligible, rc_wall_tau_over_fc, rc_wall_type,
        };
        for (height, eligible, rank) in [
            (1500.0, true, MemberRank::FB),
            (1600.0, true, MemberRank::FB),
            (2400.0, false, MemberRank::FD),
        ] {
            let model = single_wall_619_model(height);
            let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
            let id = index.generated_elem_ids().next().unwrap();
            let wall = expanded.element(id).unwrap();
            let center = sepika_core::model::wall_element_geometry(wall, &expanded)
                .unwrap()
                .lw;
            let geometry = rc_wall_ds_geometry(wall, &expanded, index.plate_of(id)).unwrap();
            assert_eq!(center, 3000.0);
            assert_eq!(
                (geometry.clear_length_mm, geometry.clear_height_mm),
                (2600.0, height)
            );
            assert_eq!(
                geometry.columns,
                [(ElemId(0), 160000.0, 400.0), (ElemId(1), 160000.0, 400.0)]
            );
            let eligibility = geometry.columns.map(|(_, area, diameter)| {
                rc_wall_side_column_eligible(2600.0, height, 200.0, area, diameter).unwrap()
            });
            assert_eq!(eligibility, [eligible; 2]);
            let ratio = rc_wall_tau_over_fc(3e6, 200.0, center, 24.0).unwrap();
            assert_eq!(ratio, 5.0 / 24.0);
            assert_eq!(
                rc_wall_type(ratio, eligibility.contains(&false), false),
                rank
            );
        }
        let mut model = single_wall_619_model(1500.0);
        let mut small = model.sections[0].clone();
        small.id = sepika_core::ids::SectionId(3);
        small.shape = Some(sepika_core::section_shape::SectionShape::RcColumnRect {
            b: 390.0,
            d: 400.0,
            rebar: match small.shape.unwrap() {
                sepika_core::section_shape::SectionShape::RcColumnRect { rebar, .. } => rebar,
                _ => unreachable!(),
            },
        });
        small.width = 390.0;
        model.sections.push(small);
        model.elements[1].section = Some(sepika_core::ids::SectionId(3));
        let geometry = wall_619_geometry(&model).unwrap();
        let eligibility = geometry.columns.map(|(_, area, diameter)| {
            rc_wall_side_column_eligible(
                geometry.clear_length_mm,
                geometry.clear_height_mm,
                200.0,
                area,
                diameter,
            )
            .unwrap()
        });
        assert_eq!(eligibility, [true, false]);
        assert_eq!(
            rc_wall_type(5.0 / 24.0, eligibility.contains(&false), false),
            MemberRank::FD
        );
    }

    #[test]
    fn rc_wall_619_rejects_unknown_opening_shared_and_invalid_geometry_with_reason() {
        let mut model = single_wall_619_model(1500.0);
        let (expanded, index, _) = sepika_load::wall_expand::expand_wall_elements(&model);
        let id = index.generated_elem_ids().next().unwrap();
        assert!(
            rc_wall_ds_geometry(expanded.element(id).unwrap(), &expanded, None)
                .unwrap_err()
                .contains("壁版の対応が不明")
        );
        model.wall_plates[0].opening_area = 1.0;
        assert!(wall_619_geometry(&model)
            .unwrap_err()
            .contains("壁版 ID 0: 開口付き"));
        model.wall_plates[0].opening_area = 0.0;
        model.sections[0].width = f64::NAN;
        assert!(wall_619_geometry(&model).unwrap_err().contains("不正"));
        let mut model = single_wall_619_model(1500.0);
        model.nodes[3].coord[1] = 1.0;
        assert!(wall_619_geometry(&model)
            .unwrap_err()
            .contains("同一鉛直面"));
        for endpoint in [[6000.0, 0.0], [3000.0, 3000.0]] {
            let mut model = single_wall_619_model(1500.0);
            for (id, z) in [(4, 0.0), (5, 1900.0)] {
                let mut node = model.nodes[if z == 0.0 { 1 } else { 3 }].clone();
                node.id = sepika_core::ids::NodeId(id);
                node.coord = [endpoint[0], endpoint[1], z];
                model.nodes.push(node);
            }
            for (id, source, nodes) in [(4, 1, [4, 5]), (5, 3, [1, 4]), (6, 2, [3, 5])] {
                let mut member = model.elements[source].clone();
                member.id = ElemId(id);
                member.nodes = nodes.map(sepika_core::ids::NodeId).into_iter().collect();
                model.elements.push(member);
            }
            let boundary = [1, 4, 5, 3].map(sepika_core::ids::NodeId).to_vec();
            let plate = model.wall_plates[0].clone();
            let other_plate = model.add_enclosed_wall_plate_from_nodes(&boundary, plate);
            model.wall_regions.push(sepika_core::model::WallRegion {
                id: sepika_core::ids::WallRegionId(1),
                name: "共有側柱を持つ壁".into(),
                boundary,
                wall_plate_ids: vec![other_plate],
                posts: Vec::new(),
            });
            let error = wall_619_geometry(&model).unwrap_err();
            assert!(
                error.contains("側柱 ID 1") && error.contains("共有する側柱"),
                "{error}"
            );
        }
        let mut model = single_wall_619_model(1500.0);
        model.wall_regions[0]
            .wall_plate_ids
            .push(sepika_core::ids::WallPlateId(1));
        assert!(wall_619_geometry(&model)
            .unwrap_err()
            .contains("Qの負担範囲を確認できません"));
    }

    #[test]
    fn rc_wall_619_existing_ds_entry_uses_adopted_run_wall_response_and_reports_inputs() {
        let mut app = App::default();
        app.load_model(crate::app::tests::with_explicit_pushover_gravity(
            single_wall_619_model(1500.0),
        ));
        app.core.analysis_cfg.push_steps = 2;
        app.core.analysis_cfg.push_use_max_disp = true;
        app.core.analysis_cfg.push_use_drift_angle = false;
        app.core.analysis_cfg.push_max_disp = 0.2;
        app.run_preparation();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        app.run_pushover();
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        crate::app::tests::select_holding_points(&mut app);
        app.core.design_rank_auto = true;
        let po = app.displayed_pushover().unwrap();
        let point = po.ds_evaluation.as_ref().unwrap();
        let wall_id = po.wall_capacities[0].0;
        let qw_n = po.wall_response_at(wall_id, point.step).unwrap().qw_n;
        assert!(qw_n.is_finite() && qw_n.abs() > 0.0);
        let result = app.compute_holding_capacity().unwrap().0;
        assert!(result.member_ranks.iter().any(|(id, _)| *id == wall_id));
        let detail = &app.core.scoped.ds_wall_details[0];
        assert!(detail.contains(&format!("Q_Ds={qw_n:.3}")), "{detail}");
        assert!(
            detail.contains("t=200.000 mm, l_cc=3000.000 mm, Fc=24.000 N/mm²"),
            "{detail}"
        );
        assert!(detail.contains("内法 2600.000×1500.000 mm"), "{detail}");
        assert!(
            detail.contains("適用表: 通常") && detail.contains("2015年版引用"),
            "{detail}"
        );
        assert!(
            detail.contains(&format!("τu/Fc={:.9}", qw_n.abs() / 14_400_000.0)),
            "{detail}"
        );
        let valid_results = app.core.scoped.results.clone();
        for mismatch in 0..4 {
            app.core.scoped.results = valid_results.clone();
            let bundle = app.core.scoped.results.as_mut().unwrap();
            for po in [&mut bundle.pushover, &mut bundle.pushover_x]
                .into_iter()
                .flatten()
            {
                let point = po.ds_evaluation.as_mut().unwrap();
                match mismatch {
                    0 => point.run_id.push_str("other"),
                    1 => point.input_generation.push(0),
                    2 => point.direction = SeismicDir::Y,
                    _ => point.step = u32::MAX,
                }
            }
            assert!(
                app.compute_holding_capacity().is_err(),
                "mismatch={mismatch}"
            );
            assert!(app.core.scoped.ds_wall_details.is_empty());
        }
        app.core.scoped.results = valid_results;
        app.core.model.materials[0].fc = Some(25.0);
        assert!(app.compute_holding_capacity().is_err());
        assert!(app.core.scoped.ds_wall_details.is_empty());
    }

    fn rebar() -> RcBeamRebar {
        RcBeamRebar {
            main_dia: 22.0,
            top: vec![4, 2],
            bottom: vec![3, 2],
            cover: 40.0,
            stirrup: BeamStirrup {
                dia: 10.0,
                pitch: 100.0,
                legs: 2,
            },
        }
    }

    fn side(top: bool, shear: f64) -> (MemberRank, f64, f64, f64, f64) {
        let rebar = rebar();
        let bending = rebar.bending_steel(600.0, top);
        let (rank, qmu, qsu) = rank_new_rc_beam_side(
            300.0,
            600.0,
            &rebar,
            top,
            345.0,
            24.0,
            rebar.pw(300.0),
            295.0,
            3000.0,
            0.0,
            shear,
        );
        (
            rank,
            qmu,
            qsu,
            bending.tension.area_mm2,
            bending.tension.effective_depth_mm,
        )
    }

    #[test]
    fn 新型_rc梁の上下引張側を個別評価する() {
        use sepika_core::rc_capacity::{rc_qmu_simple, RcCapacityInput};

        let rebar = rebar();
        let expected = |top| {
            let bending = rebar.bending_steel(600.0, top);
            rc_qmu_simple(&RcCapacityInput {
                b: 300.0,
                d: 600.0,
                at: bending.tension.area_mm2,
                d_eff: bending.tension.effective_depth_mm,
                sigma_y: 345.0,
                fc: 24.0,
                pw: rebar.pw(300.0),
                sigma_wy: 295.0,
                clear_span: 3000.0,
                sigma_0: 0.0,
            })
        };
        let top = side(true, 0.0);
        let bottom = side(false, 0.0);
        assert_eq!(top.1, expected(true));
        assert_eq!(bottom.1, expected(false));
        assert_ne!((top.3, top.4, top.1), (bottom.3, bottom.4, bottom.1));

        assert_eq!(
            worst_rc_beam_rank(MemberRank::FA, MemberRank::FD),
            MemberRank::FD
        );
        assert_eq!(
            worst_rc_beam_rank(MemberRank::FC, MemberRank::FB),
            MemberRank::FC
        );
    }

    fn rank_input() -> (
        Material,
        Material,
        Material,
        sepika_solver::nonlinear::pushover::PushoverMemberResponse,
    ) {
        let steel = |id, name: &str, fy| Material {
            id: sepika_core::ids::MaterialId(id),
            name: name.into(),
            category: sepika_core::model::MaterialCategory::Steel,
            young: 200_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(fy),
            concrete_class: Default::default(),
            strength_factor: None,
        };
        (
            Material {
                id: sepika_core::ids::MaterialId(1),
                name: "concrete".into(),
                category: sepika_core::model::MaterialCategory::Concrete,
                young: 25_000.0,
                poisson: 0.2,
                density: 0.0,
                shear: None,
                fc: Some(24.0),
                fy: None,
                concrete_class: Default::default(),
                strength_factor: None,
            },
            steel(2, "SD345", 345.0),
            steel(3, "SD295", 295.0),
            sepika_solver::nonlinear::pushover::PushoverMemberResponse {
                elem: ElemId(3),
                m_strong: 0.0,
                m_weak: 0.0,
                shear_strong: 0.0,
                shear_weak: 0.0,
                axial: 0.0,
                rp: 0.0,
                horizontal_force: 0.0,
                wall_shear_signed: None,
            },
        )
    }

    #[test]
    fn 新型_rc梁の不正入力は識別情報付きで失敗する() {
        let rebar = rebar();
        let (mat, main_steel, shear_steel, response) = rank_input();
        let elem = ElemId(3);
        let section = sepika_core::ids::SectionId(4);
        let unset = RcBeamRebar {
            main_dia: 0.0,
            top: vec![],
            bottom: vec![],
            cover: 0.0,
            stirrup: BeamStirrup {
                dia: 0.0,
                pitch: 0.0,
                legs: 0,
            },
        };
        let rank = |rebar: &RcBeamRebar, mat: &Material, rebar_mat, shear_mat, span| {
            rank_new_rc_beam(
                elem, section, 300.0, 600.0, rebar, mat, rebar_mat, shear_mat, &response, span,
            )
        };
        let valid = rank(&rebar, &mat, Some(&main_steel), Some(&shear_steel), 3000.0);
        assert!(valid.is_ok(), "正常入力で失敗: {valid:?}");
        let assert_error = |result: Result<MemberRank, String>, reason: &str| {
            let error = result.unwrap_err();
            assert!(error.contains("部材 ElemId(3)"), "{error}");
            assert!(error.contains("断面 SectionId(4)"), "{error}");
            assert!(error.contains(reason), "{error}");
        };
        let mut invalid = rebar.clone();
        invalid.top[0] = 0;
        assert_error(
            rank(
                &invalid,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                3000.0,
            ),
            "配筋形状が不正",
        );
        assert_error(
            rank(&unset, &mat, Some(&main_steel), Some(&shear_steel), 3000.0),
            "配筋が未設定",
        );
        let mut no_fc = mat.clone();
        no_fc.fc = None;
        assert_error(
            rank(
                &rebar,
                &no_fc,
                Some(&main_steel),
                Some(&shear_steel),
                3000.0,
            ),
            "Fc が未設定",
        );
        let mut zero_fc = mat.clone();
        zero_fc.fc = Some(0.0);
        assert_error(
            rank(
                &rebar,
                &zero_fc,
                Some(&main_steel),
                Some(&shear_steel),
                3000.0,
            ),
            "Fc が未設定",
        );
        assert_error(
            rank(&rebar, &mat, Some(&main_steel), Some(&shear_steel), 0.0),
            "内法スパン",
        );
        let mut no_fy = mat.clone();
        no_fy.fy = None;
        assert_error(
            rank(&rebar, &no_fy, None, Some(&shear_steel), 3000.0),
            "主筋の降伏強度",
        );
        assert_error(
            rank(&rebar, &mat, Some(&main_steel), None, 3000.0),
            "せん断補強筋の降伏強度",
        );
    }

    #[test]
    fn 新型_rc柱の未設定配筋は識別情報付きで失敗する() {
        use sepika_core::section_shape::{
            CircleColumnHoop, RcCircleColumnRebar, RcRectColumnRebar, RectColumnHoop,
        };

        let (mat, main_steel, shear_steel, response) = rank_input();
        let elem = ElemId(3);
        let section = sepika_core::ids::SectionId(4);
        let rect = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![],
            y: vec![],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        };
        let circle = RcCircleColumnRebar {
            main_dia: 22.0,
            count: 0,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 100.0,
            },
        };
        for error in [
            rank_new_rc_column_rect(
                elem,
                section,
                400.0,
                600.0,
                &rect,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap_err(),
            rank_new_rc_circle_column(
                elem,
                section,
                600.0,
                &circle,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap_err(),
        ] {
            assert!(error.contains("部材 ElemId(3)"), "{error}");
            assert!(error.contains("断面 SectionId(4)"), "{error}");
            assert!(error.contains("配筋が未設定"), "{error}");
        }
    }

    #[test]
    fn 新型_rc矩形柱は二方向の断面算定ランクの最悪値を採用する() {
        use sepika_core::rc_capacity::{rc_column_mu_simple, rc_qsu_simple, RcCapacityInput};
        use sepika_core::rc_rebar_geom::RectEdge;
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
        use sepika_design_jp::secondary::ds_group::rc_column_type;

        let (mat, main_steel, shear_steel, response) = rank_input();
        let rebar = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 3,
            },
        };
        let a1 = std::f64::consts::PI * 22.0_f64.powi(2) / 4.0;
        let ag = rebar.total_main_area();
        assert!((ag - 8.0 * a1).abs() < 1e-10);
        let expected = |strong: bool| {
            let (bd, dd, edge, aw, shear) = if strong {
                (
                    400.0,
                    600.0,
                    RectEdge::Top,
                    rebar.aw_x_mm2(),
                    response.shear_strong,
                )
            } else {
                (
                    600.0,
                    400.0,
                    RectEdge::Left,
                    rebar.aw_y_mm2(),
                    response.shear_weak,
                )
            };
            let steel = rebar.edge_steel(edge, 400.0, 600.0);
            assert!((steel.area_mm2 - 3.0 * a1).abs() < 1e-10);
            let input = RcCapacityInput {
                b: bd,
                d: dd,
                at: steel.area_mm2,
                d_eff: steel.effective_depth_mm,
                sigma_y: 345.0,
                fc: 24.0,
                pw: aw / (bd * 100.0),
                sigma_wy: 295.0,
                clear_span: 3000.0,
                sigma_0: response.axial / (400.0 * 600.0),
            };
            let qmu = 2.0 * rc_column_mu_simple(&input, ag, response.axial) / 3000.0;
            let qsu = rc_qsu_simple(&input);
            rc_column_type(
                3000.0 / dd,
                response.axial / (400.0 * 600.0 * 24.0),
                100.0 * input.at / (bd * input.d_eff),
                (shear.abs() / (bd)) / 24.0,
                qsu < qmu,
            )
        };
        assert_ne!(rebar.aw_x_mm2(), rebar.aw_y_mm2());
        assert_eq!(
            rebar.aw_x_mm2() / (400.0 * 100.0),
            rebar.aw_y_mm2() / (600.0 * 100.0)
        );
        let expected_rank = worst_rank(&[expected(true), expected(false)]).unwrap();
        assert_eq!(
            rank_new_rc_column_rect(
                ElemId(3),
                sepika_core::ids::SectionId(4),
                400.0,
                600.0,
                &rebar,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0
            )
            .unwrap(),
            expected_rank,
        );
    }

    #[test]
    fn 新型_rc矩形柱は帯筋ピッチ非正でpwゼロとしてランク付けする() {
        use sepika_core::rc_capacity::{rc_column_mu_simple, rc_qsu_simple, RcCapacityInput};
        use sepika_core::rc_rebar_geom::RectEdge;
        use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop};
        use sepika_design_jp::secondary::ds_group::rc_column_type;

        let (mat, main_steel, shear_steel, response) = rank_input();
        let base = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![3],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 3,
            },
        };
        let ag = base.total_main_area();
        let expected_zero_pw = |strong: bool| {
            let (bd, dd, edge, shear) = if strong {
                (400.0, 600.0, RectEdge::Top, response.shear_strong)
            } else {
                (600.0, 400.0, RectEdge::Left, response.shear_weak)
            };
            let steel = base.edge_steel(edge, 400.0, 600.0);
            let input = RcCapacityInput {
                b: bd,
                d: dd,
                at: steel.area_mm2,
                d_eff: steel.effective_depth_mm,
                sigma_y: 345.0,
                fc: 24.0,
                pw: 0.0,
                sigma_wy: 295.0,
                clear_span: 3000.0,
                sigma_0: response.axial / (400.0 * 600.0),
            };
            let qmu = 2.0 * rc_column_mu_simple(&input, ag, response.axial) / 3000.0;
            let qsu = rc_qsu_simple(&input);
            assert!(qmu.is_finite() && qsu.is_finite());
            rc_column_type(
                3000.0 / dd,
                response.axial / (400.0 * 600.0 * 24.0),
                100.0 * input.at / (bd * input.d_eff),
                (shear.abs() / bd) / 24.0,
                qsu < qmu,
            )
        };
        let expected = worst_rank(&[expected_zero_pw(true), expected_zero_pw(false)]).unwrap();
        for pitch in [0.0, -50.0] {
            let mut rebar = base.clone();
            rebar.hoop.pitch = pitch;
            let rank = rank_new_rc_column_rect(
                ElemId(3),
                sepika_core::ids::SectionId(4),
                400.0,
                600.0,
                &rebar,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap();
            assert_eq!(rank, expected, "pitch={pitch}");
        }
    }

    #[test]
    fn 新型_rc円形柱は実径と二方向せん断応答の最悪ランクを採用する() {
        use sepika_core::rc_capacity::{rc_column_mu_simple, rc_qsu_simple, RcCapacityInput};
        use sepika_core::section_shape::{CircleColumnHoop, RcCircleColumnRebar};
        use sepika_design_jp::secondary::ds_group::rc_column_type;

        let (mat, main_steel, shear_steel, mut response) = rank_input();
        response.shear_strong = 100_000.0;
        response.shear_weak = 500_000.0;
        let rebar = RcCircleColumnRebar {
            main_dia: 22.0,
            count: 8,
            cover: 40.0,
            hoop: CircleColumnHoop {
                dia: 10.0,
                pitch: 100.0,
            },
        };
        let d: f64 = 600.0;
        let side = (std::f64::consts::PI * d.powi(2) / 4.0).sqrt();
        let a1 = std::f64::consts::PI * 22.0_f64.powi(2) / 4.0;
        let ag = rebar.total_main_area();
        let at = rebar.equivalent_tension_area_mm2();
        let pw = 2.0 * (std::f64::consts::PI * 10.0_f64.powi(2) / 4.0) / (side * 100.0);
        assert!((ag - 8.0 * a1).abs() < 1e-10);
        assert!((at - 2.0 * a1).abs() < 1e-10);
        let input = RcCapacityInput {
            b: side,
            d: side,
            at,
            d_eff: rebar.equivalent_effective_depth_mm(d),
            sigma_y: 345.0,
            fc: 24.0,
            pw,
            sigma_wy: 295.0,
            clear_span: 3000.0,
            sigma_0: response.axial / side.powi(2),
        };
        let qmu = 2.0 * rc_column_mu_simple(&input, ag, response.axial) / 3000.0;
        let qsu = rc_qsu_simple(&input);
        let qgross = side.powi(2);
        let expected = |shear: f64| {
            rc_column_type(
                3000.0 / d,
                response.axial / (qgross * 24.0),
                100.0 * at / (side * input.d_eff),
                (shear.abs() / qgross) / 24.0,
                qsu < qmu,
            )
        };
        let expected_rank = worst_rank(&[
            expected(response.shear_strong),
            expected(response.shear_weak),
        ])
        .unwrap();
        assert_ne!(response.shear_strong, response.shear_weak);
        assert_eq!(
            rank_new_rc_circle_column(
                ElemId(3),
                sepika_core::ids::SectionId(4),
                d,
                &rebar,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0
            )
            .unwrap(),
            expected_rank,
        );
    }

    #[test]
    fn 新型_rc柱の不正形状と未設定配筋は識別情報付きで失敗する() {
        use sepika_core::section_shape::{
            CircleColumnHoop, RcCircleColumnRebar, RcRectColumnRebar, RectColumnHoop,
        };

        let (mat, main_steel, shear_steel, response) = rank_input();
        let elem = ElemId(3);
        let section = sepika_core::ids::SectionId(4);
        let rect = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![4, 2],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 3,
            },
        };
        let unset_rect = RcRectColumnRebar {
            x: vec![],
            y: vec![],
            ..rect.clone()
        };
        let unset_circle = RcCircleColumnRebar {
            main_dia: 0.0,
            count: 0,
            cover: 0.0,
            hoop: CircleColumnHoop {
                dia: 0.0,
                pitch: 0.0,
            },
        };
        for error in [
            rank_new_rc_column_rect(
                elem,
                section,
                400.0,
                600.0,
                &rect,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap_err(),
            rank_new_rc_column_rect(
                elem,
                section,
                400.0,
                600.0,
                &unset_rect,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap_err(),
            rank_new_rc_circle_column(
                elem,
                section,
                600.0,
                &unset_circle,
                &mat,
                Some(&main_steel),
                Some(&shear_steel),
                &response,
                3000.0,
            )
            .unwrap_err(),
        ] {
            assert!(error.contains("部材 ElemId(3)"), "{error}");
            assert!(error.contains("断面 SectionId(4)"), "{error}");
        }
    }
}
