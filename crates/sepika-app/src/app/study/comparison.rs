use super::*;

pub(super) fn require_static_result(app: &App, key: &ResultInputKey) -> Result<(), String> {
    app.require_result_input(key.clone())?;
    if app
        .core
        .scoped
        .results
        .as_ref()
        .is_some_and(|bundle| match key {
            ResultInputKey::Static(key) => bundle.statics.iter().any(|(k, _)| k == key),
            ResultInputKey::Combo(name) => bundle.combos.iter().any(|(n, _)| n == name),
            _ => false,
        })
    {
        Ok(())
    } else {
        Err("静的結果の欠落・未計算です。再計算してください".into())
    }
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExpectedPosition {
    xi: u64,
    components: Option<Vec<sepika_design_jp::CheckKind>>,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExpectedMember {
    elem: ElemId,
    positions: Vec<ExpectedPosition>,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExpectedJoint {
    node: NodeId,
    label: String,
    components: Option<Vec<sepika_design_jp::CheckKind>>,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExpectedWall {
    label: String,
    node: Option<NodeId>,
    seismic_target: bool,
    components: Option<Vec<sepika_design_jp::CheckKind>>,
}
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct ExpectedChecks {
    source: Option<ResultInputKey>,
    run: Option<String>,
    members: Vec<ExpectedMember>,
    joints: Vec<ExpectedJoint>,
    walls: Vec<ExpectedWall>,
}
impl ExpectedChecks {
    pub(super) fn capture(bundle: &ResultsBundle) -> Self {
        fn components(
            outcome: &sepika_design_jp::CheckOutcome,
        ) -> Option<Vec<sepika_design_jp::CheckKind>> {
            match outcome {
                sepika_design_jp::CheckOutcome::Checked(check) => {
                    let mut kinds: Vec<_> = check.components.iter().map(|c| c.kind).collect();
                    kinds.sort_by_key(|k| k.label());
                    Some(kinds)
                }
                sepika_design_jp::CheckOutcome::Skipped { .. } => None,
            }
        }
        let mut members: Vec<_> = bundle
            .member_checks
            .iter()
            .map(|member| {
                let mut positions: Vec<_> = member
                    .positions
                    .iter()
                    .map(|p| ExpectedPosition {
                        xi: p.xi.to_bits(),
                        components: components(&p.outcome),
                    })
                    .collect();
                positions.sort_by_key(|p| p.xi);
                ExpectedMember {
                    elem: member.elem,
                    positions,
                }
            })
            .collect();
        members.sort_by_key(|m| m.elem.0);
        let mut joints: Vec<_> = bundle
            .joint_checks
            .iter()
            .map(|j| ExpectedJoint {
                node: j.node,
                label: j.label.clone(),
                components: components(&j.outcome),
            })
            .collect();
        joints.sort_by(|a, b| (a.node.0, &a.label).cmp(&(b.node.0, &b.label)));
        let mut walls: Vec<_> = bundle
            .wall_checks
            .iter()
            .map(|w| ExpectedWall {
                label: w.label(),
                node: w.node,
                seismic_target: w.seismic_target,
                components: components(&w.outcome),
            })
            .collect();
        walls.sort_by(|a, b| {
            (&a.label, a.node.map(|id| id.0)).cmp(&(&b.label, b.node.map(|id| id.0)))
        });
        Self {
            source: bundle.design_source.clone(),
            run: bundle
                .reference_runs
                .iter()
                .find(|(key, _)| *key == ResultInputKey::Design)
                .map(|(_, run)| run.clone()),
            members,
            joints,
            walls,
        }
    }
}

pub(super) fn validate_expected_checks(evidence: &ReferencedResult) -> Result<(), String> {
    validate_expected_bundle(evidence.expected_checks.as_ref(), &evidence.bundle)
}

pub(super) fn validate_expected_bundle(
    expected: Option<&ExpectedChecks>,
    bundle: &ResultsBundle,
) -> Result<(), String> {
    let expected =
        expected.ok_or("計算時に保持した検定対象・位置・項目の範囲情報が欠落しています")?;
    let actual = ExpectedChecks::capture(bundle);
    if expected.source.is_none()
        || expected.run.is_none()
        || expected.source != actual.source
        || expected.run != actual.run
    {
        return Err("検定の実行・ケースと保持した算定範囲が一致しません".into());
    }
    if expected.members != actual.members || actual.members.iter().any(|m| m.positions.is_empty()) {
        return Err(
            "部材の検定対象・位置・項目が欠落・空、または保持した算定範囲と一致しません".into(),
        );
    }
    if expected.joints != actual.joints {
        return Err("接合部の検定対象・項目が保持した算定範囲と一致しません".into());
    }
    if expected.walls != actual.walls {
        return Err("壁の検定対象・項目が保持した算定範囲と一致しません".into());
    }
    Ok(())
}

pub(super) fn capture_checks(worker: &App) -> Vec<ReferencedResult> {
    let Some(bundle) = &worker.core.scoped.results else {
        return Vec::new();
    };
    let mut view = App {
        core: StudyInput::capture(&worker.core).core(),
        ..Default::default()
    };
    view.core.scoped.results = Some(bundle.clone());
    view.core.scoped.last_static = worker.core.scoped.last_static;
    view.validate_restored_results();
    let keys: Vec<_> = bundle
        .statics
        .iter()
        .map(|(key, _)| StaticKey::Case(*key))
        .chain((0..bundle.combos.len()).map(StaticKey::Combo))
        .collect();
    let mut saved = Vec::new();
    for key in keys {
        view.core.scoped.last_static = Some(key);
        if view.require_selected_static_input().is_err() {
            continue;
        }
        view.select_displayed_result(key);
        if view.require_design_input().is_ok() {
            saved.push(ReferencedResult {
                expected_checks: Some(ExpectedChecks::capture(
                    view.core.scoped.results.as_ref().unwrap(),
                )),
                input: StudyInput::capture(&view.core),
                bundle: view.core.scoped.results.as_ref().unwrap().clone(),
                last_static: Some(key),
            });
        }
    }
    saved
}

#[cfg(any(feature = "gui", test))]
mod projection {
    use super::*;
    use references::{DisplayConditions, EvidenceReference, ReferenceTarget};

    pub(super) fn fixed_members(study: &Study) -> Vec<ElemId> {
        let mut members: Vec<_> = study
            .fixed
            .iter()
            .filter_map(|r| {
                if let ReferenceTarget::Member(id) = r.target {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();
        members.sort_by_key(|id| id.0);
        members.dedup();
        members
    }

    pub(super) fn same_conditions(
        baseline: &StudyInput,
        input: &StudyInput,
        fixed: &[ElemId],
    ) -> bool {
        if baseline.references.project != input.references.project
            || !baseline.settings.same_input_conditions(&input.settings)
        {
            return false;
        }
        let mut candidate = input.core();
        for id in fixed {
            let Some(before) = baseline.model.element(*id) else {
                return false;
            };
            let Some(after) = candidate.model.elements.iter_mut().find(|e| e.id == *id) else {
                return false;
            };
            after.section = before.section;
        }
        let mut baseline_model = baseline.model.clone();
        if baseline_model.assign_stb_node_ids().is_err()
            || candidate.model.assign_stb_node_ids().is_err()
        {
            return false;
        }
        bincode::serialize(&baseline_model).unwrap()
            == bincode::serialize(&candidate.model).unwrap()
    }

    pub(super) fn condition_differences(
        baseline: &StudyInput,
        input: &StudyInput,
        fixed: &[ElemId],
    ) -> Vec<String> {
        let mut differences = Vec::new();
        let a = &baseline.model;
        let b = &input.model;
        let supports = |model: &sepika_core::model::Model| {
            bincode::serialize(&(
                model
                    .nodes
                    .iter()
                    .map(|n| (n.id, n.restraint, n.support_spring))
                    .collect::<Vec<_>>(),
                &model.constraints,
            ))
            .unwrap()
        };
        if supports(a) != supports(b) {
            differences.push("支点・拘束条件が不一致".into());
        }
        let manual_loads = |model: &sepika_core::model::Model| {
            bincode::serialize(
                &model
                    .load_cases
                    .iter()
                    .map(|c| {
                        (
                            c.id,
                            c.nodal
                                .iter()
                                .filter(|l| !l.source.is_auto())
                                .collect::<Vec<_>>(),
                            c.member
                                .iter()
                                .filter(|l| !l.source.is_auto())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap()
        };
        if manual_loads(a) != manual_loads(b) {
            differences.push("ユーザー入力荷重が不一致".into());
        }
        let definitions = |model: &sepika_core::model::Model| {
            bincode::serialize(&(
                model
                    .load_cases
                    .iter()
                    .map(|c| (c.id, &c.name, c.explicit_input_kind(), c.origin))
                    .collect::<Vec<_>>(),
                &model.combinations,
            ))
            .unwrap()
        };
        if definitions(a) != definitions(b) {
            differences.push("ケース／組合せの定義・集合が不一致".into());
        }
        if bincode::serialize(&baseline.settings.analysis_cfg).unwrap()
            != bincode::serialize(&input.settings.analysis_cfg).unwrap()
        {
            differences.push("解析・検定設定が不一致".into());
        }
        if !baseline.settings.same_input_conditions(&input.settings) {
            differences.push("検定条件・設定が不一致".into());
        }
        if !same_conditions(baseline, input, fixed) {
            differences.push("断面割当以外の入力・対象・範囲に差があります（上記以外の形状・材料・数量・検定属性を含む）".into());
        }
        differences
    }

    pub(super) fn result_app(
        input: &StudyInput,
        bundle: &ResultsBundle,
        last: Option<StaticKey>,
    ) -> App {
        let mut app = App {
            core: input.core(),
            ..Default::default()
        };
        app.core.scoped.results = Some(bundle.clone());
        app.core.scoped.last_static = last;
        app.validate_restored_results();
        app.core.scoped.undo.restrict_sections(Vec::new());
        app.studies.active = Location::Proposal(0, 0);
        app
    }

    #[derive(Clone, Debug, PartialEq)]
    pub(super) struct CheckRow {
        pub key: String,
        pub case: String,
        pub target: Option<ReferenceTarget>,
        pub position: Option<f64>,
        pub ratio: Option<f64>,
        pub complete: bool,
        pub invalid_components: bool,
        pub reason: String,
        pub items: Vec<sepika_design_jp::CheckKind>,
        pub governing_items: Vec<sepika_design_jp::CheckKind>,
        pub evidence: Option<references::ResultReference>,
    }
    fn ratio(outcome: &sepika_design_jp::CheckOutcome) -> (Option<f64>, String) {
        match outcome {
            sepika_design_jp::CheckOutcome::Checked(check) => {
                let ratio = result_validity::checked_ratio(check);
                let reason = if result_validity::evaluate_check(check).complete {
                    check.basis.clone()
                } else {
                    result_validity::INVALID_CHECK_COMPONENTS.into()
                };
                (ratio, reason)
            }
            sepika_design_jp::CheckOutcome::Skipped { reason } => (None, reason.clone()),
        }
    }
    pub(super) fn fixed_coverage(
        evidence: &ReferencedResult,
        fixed: &[ElemId],
    ) -> Result<(), String> {
        for id in fixed {
            if !evidence
                .bundle
                .member_checks
                .iter()
                .any(|m| m.elem == *id && !m.positions.is_empty())
            {
                return Err(format!("固定対象部材{}の検定結果・位置が欠落または空です。範囲を取得できないため未検定行数へ0として補完しません",id.0));
            }
        }
        Ok(())
    }

    pub(super) fn fixed_rows<'a>(
        rows: impl IntoIterator<Item = &'a CheckRow>,
        fixed: &[ElemId],
    ) -> Vec<&'a CheckRow> {
        rows.into_iter().filter(|row| matches!(row.target, Some(ReferenceTarget::Member(id)) if fixed.contains(&id))).collect()
    }

    pub(super) fn case_label(key: &ResultInputKey, model: &sepika_core::model::Model) -> String {
        match key {
            ResultInputKey::Static(StaticCaseKey::User(id)) => {
                model.load_cases.iter().find(|c| c.id == *id).map_or_else(
                    || format!("荷重ケース{}（定義欠落）", id.0),
                    |c| format!("荷重ケース{} {}", id.0, c.name),
                )
            }
            ResultInputKey::Static(StaticCaseKey::Seismic(direction)) => {
                format!("地震{direction:?}")
            }
            ResultInputKey::Combo(name) => format!("組合せ {name}"),
            _ => "静的応力源の欠落・未対応".into(),
        }
    }

    pub(super) fn check_rows(evidence: &ReferencedResult) -> Result<Vec<CheckRow>, String> {
        validate_expected_checks(evidence)?;
        let app = result_app(&evidence.input, &evidence.bundle, evidence.last_static);
        app.require_design_input()?;
        let case_key = evidence
            .bundle
            .design_source
            .as_ref()
            .ok_or("検定の静的応力源が欠落しています")?;
        let case = case_label(case_key, &evidence.input.model);
        let mut rows = Vec::new();
        let mut add = |target: Option<ReferenceTarget>,
                       position: Option<f64>,
                       key: String,
                       outcome: &sepika_design_jp::CheckOutcome| {
            let (mut ratio, mut reason) = ratio(outcome);
            if position.is_some_and(|p: f64| !p.is_finite() || !(0.0..=1.0).contains(&p)) {
                ratio = None;
                reason = "検定位置の欠落・不正".into();
            }
            let items = match outcome {
                sepika_design_jp::CheckOutcome::Checked(c) => {
                    c.components.iter().map(|c| c.kind).collect()
                }
                _ => Vec::new(),
            };
            let governing_items = match outcome {
                sepika_design_jp::CheckOutcome::Checked(check) => check
                    .components
                    .iter()
                    .filter(|c| Some(c.ratio) == ratio)
                    .map(|c| c.kind)
                    .collect(),
                _ => Vec::new(),
            };
            let reference = target.and_then(|target| {
                app.recorded_target_result_reference(
                    ResultInputKey::Design,
                    target,
                    DisplayConditions {
                        position,
                        component: Some(key.clone()),
                        unit: Some("無次元".into()),
                        ..Default::default()
                    },
                )
                .ok()
            });
            rows.push(CheckRow {
                key: format!("{case}/{key}"),
                case: case.clone(),
                target,
                position,
                ratio,
                complete: !result_validity::check_is_unrated(outcome) && ratio.is_some(),
                invalid_components: matches!(outcome, sepika_design_jp::CheckOutcome::Checked(check) if !result_validity::evaluate_check(check).complete),
                reason,
                items,
                governing_items,
                evidence: reference,
            });
        };
        for member in &evidence.bundle.member_checks {
            for p in &member.positions {
                add(
                    Some(ReferenceTarget::Member(member.elem)),
                    Some(p.xi),
                    format!("部材{}/位置{:016x}", member.elem.0, p.xi.to_bits()),
                    &p.outcome,
                );
            }
        }
        for joint in &evidence.bundle.joint_checks {
            add(
                Some(ReferenceTarget::Node(joint.node)),
                None,
                format!("節点{}/{}", joint.node.0, joint.label),
                &joint.outcome,
            );
        }
        for wall in &evidence.bundle.wall_checks {
            if !wall.seismic_target {
                continue;
            }
            let target = wall
                .elem
                .map(ReferenceTarget::Member)
                .or_else(|| wall.node.map(ReferenceTarget::Node))
                .or_else(|| {
                    wall.plate
                        .and_then(|id| evidence.input.model.wall_plates.iter().find(|p| p.id == id))
                        .and_then(|plate| plate.boundary_nodes(&evidence.input.model))
                        .and_then(|nodes| nodes.first().copied())
                        .map(ReferenceTarget::Node)
                });
            add(target, None, wall.label(), &wall.outcome);
        }
        Ok(rows)
    }

    pub(super) fn displacement(
        app: &App,
        key: StaticKey,
        node: NodeId,
        component: usize,
    ) -> Result<(f64, EvidenceReference), String> {
        if component > 2 {
            return Err("全体座標X/Y/Zの並進成分を指定してください".into());
        }
        let kind = match key {
            StaticKey::Case(key) => ResultInputKey::Static(key),
            StaticKey::Combo(i) => ResultInputKey::Combo(
                app.core
                    .scoped
                    .results
                    .as_ref()
                    .and_then(|b| b.combos.get(i))
                    .ok_or("組合せ結果が欠落")?
                    .0
                    .clone(),
            ),
        };
        app.require_result_input(kind.clone())?;
        let bundle = app.core.scoped.results.as_ref().ok_or("結果なし")?;
        let result = match key {
            StaticKey::Case(key) => bundle
                .statics
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, r)| r),
            StaticKey::Combo(i) => bundle.combos.get(i).map(|(_, r)| r),
        }
        .ok_or("指定ケース結果が欠落")?;
        let index = app
            .core
            .model
            .nodes
            .iter()
            .position(|n| n.id == node)
            .ok_or("指定節点が欠落")?;
        let value = result
            .disp
            .get(index)
            .and_then(|v| v.get(component))
            .copied()
            .filter(|v| v.is_finite())
            .ok_or("指定節点変位が欠落・不正")?;
        let reference = app.recorded_target_result_reference(
            kind,
            ReferenceTarget::Node(node),
            DisplayConditions {
                direction: Some(["X", "Y", "Z"][component].into()),
                component: Some("節点変位".into()),
                unit: Some("mm".into()),
                ..Default::default()
            },
        )?;
        Ok((value, (&reference).into()))
    }

    #[derive(Default)]
    pub(super) struct Summary {
        pub maximum: Option<CheckRow>,
        pub checked: usize,
        pub skipped: usize,
        pub ng: usize,
    }
    pub(super) fn summarize<'a>(rows: impl IntoIterator<Item = &'a CheckRow>) -> Summary {
        let mut summary = Summary::default();
        let mut maximum: Option<&CheckRow> = None;
        for row in rows {
            if let Some(ratio) = row.ratio {
                summary.checked += usize::from(row.complete);
                if ratio > 1.0 {
                    summary.ng += 1;
                }
                if maximum.is_none_or(|r| r.ratio.unwrap() < ratio) {
                    maximum = Some(row);
                }
            }
            if !row.complete {
                summary.skipped += 1;
            }
        }
        summary.maximum = maximum.cloned();
        summary
    }
    pub(super) fn same_evaluation_set<'a>(
        baseline: impl IntoIterator<Item = &'a CheckRow>,
        rows: impl IntoIterator<Item = &'a CheckRow>,
    ) -> bool {
        let baseline: Vec<_> = baseline.into_iter().collect();
        let rows: Vec<_> = rows.into_iter().collect();
        let before: std::collections::BTreeMap<_, _> = baseline
            .iter()
            .map(|r| (r.key.as_str(), (r.target, &r.items)))
            .collect();
        let after: std::collections::BTreeMap<_, _> = rows
            .iter()
            .map(|r| (r.key.as_str(), (r.target, &r.items)))
            .collect();
        baseline.len() == before.len() && rows.len() == after.len() && before == after
    }

    pub(super) fn same_check_scope<'a>(
        baseline: impl IntoIterator<Item = &'a CheckRow>,
        rows: impl IntoIterator<Item = &'a CheckRow>,
    ) -> bool {
        fn scopes<'a>(
            rows: impl IntoIterator<Item = &'a CheckRow>,
        ) -> std::collections::BTreeMap<String, Vec<&'static str>> {
            let mut scopes: std::collections::BTreeMap<String, Vec<&'static str>> =
                Default::default();
            for row in rows {
                let key = match (row.target, row.position) {
                    (Some(ReferenceTarget::Member(id)), Some(_)) => {
                        format!("{}/部材{}", row.case, id.0)
                    }
                    _ => row.key.clone(),
                };
                let items = scopes.entry(key).or_default();
                for item in &row.items {
                    if !items.contains(&item.label()) {
                        items.push(item.label());
                    }
                }
                items.sort();
            }
            scopes
        }
        scopes(baseline) == scopes(rows)
    }

    pub(super) fn same_evaluation_range<'a>(
        baseline: impl IntoIterator<Item = &'a CheckRow>,
        rows: impl IntoIterator<Item = &'a CheckRow>,
    ) -> bool {
        let baseline: Vec<_> = baseline.into_iter().collect();
        let rows: Vec<_> = rows.into_iter().collect();
        if baseline
            .iter()
            .chain(rows.iter())
            .any(|r| r.invalid_components)
        {
            return false;
        }
        if !same_check_scope(baseline.iter().copied(), rows.iter().copied()) {
            return false;
        }
        fn groups<'a>(
            rows: &[&'a CheckRow],
        ) -> std::collections::BTreeMap<String, Vec<&'a CheckRow>> {
            let mut groups: std::collections::BTreeMap<_, Vec<_>> = Default::default();
            for row in rows {
                let key = match (row.target, row.position) {
                    (Some(ReferenceTarget::Member(id)), Some(_)) => {
                        format!("{}/部材{}", row.case, id.0)
                    }
                    _ => row.key.clone(),
                };
                groups.entry(key).or_default().push(*row);
            }
            groups
        }
        let before = groups(&baseline);
        let after = groups(&rows);
        before.iter().all(|(key, base)| {
            let Some(candidate) = after.get(key) else {
                return false;
            };
            if base.iter().all(|r| r.complete) && candidate.iter().all(|r| r.complete) {
                return true;
            }
            if !same_evaluation_set(base.iter().copied(), candidate.iter().copied()) {
                return false;
            }
            let states: std::collections::HashMap<_, _> =
                base.iter().map(|r| (r.key.as_str(), r.complete)).collect();
            candidate
                .iter()
                .all(|r| states.get(r.key.as_str()) == Some(&r.complete))
        })
    }

    #[derive(Clone)]
    pub(super) struct MemberImpact {
        pub before: Option<CheckRow>,
        pub after: CheckRow,
        pub difference: Option<f64>,
        pub skipped_before: usize,
        pub skipped_after: usize,
        pub positions_changed: bool,
        pub items_changed: bool,
        pub new_ng: bool,
    }
    pub(super) fn member_impacts(
        baseline: &[CheckRow],
        rows: &[CheckRow],
        fixed: &[ElemId],
    ) -> Vec<MemberImpact> {
        let mut groups: std::collections::BTreeMap<_, Vec<&CheckRow>> = Default::default();
        for row in rows {
            if let (Some(ReferenceTarget::Member(id)), Some(_)) = (row.target, row.position) {
                if !fixed.contains(&id) {
                    groups
                        .entry((row.case.as_str(), id.0))
                        .or_default()
                        .push(row);
                }
            }
        }
        let mut impacts = Vec::new();
        for ((case, id), values) in groups {
            let before: Vec<_> = baseline
                .iter()
                .filter(|r| {
                    r.case == case
                        && r.target == Some(ReferenceTarget::Member(ElemId(id)))
                        && r.position.is_some()
                })
                .collect();
            let base = summarize(before.iter().copied());
            let candidate = summarize(values.iter().copied());
            let Some(after) = candidate.maximum else {
                continue;
            };
            let before_keys: std::collections::BTreeSet<_> =
                before.iter().map(|r| r.key.as_str()).collect();
            let after_keys: std::collections::BTreeSet<_> =
                values.iter().map(|r| r.key.as_str()).collect();
            let positions_changed = before_keys != after_keys;
            let items_changed = !same_check_scope(before.iter().copied(), values.iter().copied());
            let comparable_range = !items_changed
                && base.skipped == 0
                && candidate.skipped == 0
                && base.maximum.is_some();
            let difference = comparable_range
                .then(|| after.ratio.unwrap() - base.maximum.as_ref().unwrap().ratio.unwrap());
            let comparable_ng = comparable_range
                && !positions_changed
                && same_evaluation_set(before.iter().copied(), values.iter().copied());
            let new_ng = comparable_ng
                && base.maximum.as_ref().unwrap().ratio.unwrap() <= 1.0
                && after.ratio.unwrap() > 1.0;
            if difference.is_some_and(|d| d > 0.0)
                || !comparable_range
                || after.ratio.unwrap() > 1.0 && !comparable_ng
            {
                impacts.push(MemberImpact {
                    before: base.maximum,
                    after,
                    difference,
                    skipped_before: base.skipped,
                    skipped_after: candidate.skipped,
                    positions_changed,
                    items_changed,
                    new_ng,
                });
            }
        }
        impacts
    }

    pub(super) fn unmatched_positions<'a>(
        baseline: impl IntoIterator<Item = &'a CheckRow>,
        rows: impl IntoIterator<Item = &'a CheckRow>,
    ) -> (usize, usize) {
        let before: std::collections::HashMap<_, _> = baseline
            .into_iter()
            .filter(|r| r.position.is_some())
            .map(|r| (r.key.as_str(), &r.items))
            .collect();
        let after: std::collections::HashMap<_, _> = rows
            .into_iter()
            .filter(|r| r.position.is_some())
            .map(|r| (r.key.as_str(), &r.items))
            .collect();
        let missing_before = before
            .iter()
            .filter(|(key, items)| after.get(**key) != Some(*items))
            .count();
        let missing_after = after
            .iter()
            .filter(|(key, items)| before.get(**key) != Some(*items))
            .count();
        (missing_before, missing_after)
    }

    pub(super) fn worsening<'a>(
        baseline: &[CheckRow],
        rows: impl IntoIterator<Item = &'a CheckRow>,
        fixed: &[ElemId],
    ) -> Vec<(&'a CheckRow, f64, bool)> {
        let baseline: std::collections::HashMap<_, _> =
            baseline.iter().map(|r| (r.key.as_str(), r)).collect();
        rows.into_iter().filter(|row| !matches!(row.target, Some(ReferenceTarget::Member(id)) if fixed.contains(&id)))
            .filter_map(|row| {
                let before = baseline.get(row.key.as_str())?;
                if !before.complete || !row.complete || before.target != row.target || before.items != row.items { return None; }
                let before = before.ratio?;
                let after = row.ratio?;
                (after > before).then_some((row, after - before, before <= 1.0 && after > 1.0))
            }).collect()
    }
}

#[cfg(feature = "gui")]
mod gui;
#[cfg(feature = "gui")]
pub(super) use gui::ComparisonView;
#[cfg(test)]
pub(super) mod tests;

#[cfg(feature = "gui")]
pub(super) fn decision_case_label(confirmation: &super::adoption::Confirmation) -> String {
    confirmation.case.as_ref().map_or_else(
        || "指定なし".into(),
        |case| projection::case_label(case, &confirmation.run.prepared.model),
    )
}
