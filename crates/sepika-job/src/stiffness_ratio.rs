//! 自立壁の付加剛性を反映した剛性率評価用再解析。

use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, NodeId, StoryId};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Model, Node,
    EX_CASE_NAME, EY_CASE_NAME,
};
use sepika_design_jp::secondary::eccentricity::{has_misc_wall, misc_wall_stiffnesses};
use sepika_design_jp::secondary::eccentricity_analysis::column_stiffnesses_from_analysis;
use sepika_math::solver::SolveError;
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::{linear_static_once, StaticOnce};

fn standard_cases(model: &Model) -> Result<[LoadCaseId; 2], String> {
    let find = |name| {
        model
            .load_cases
            .iter()
            .find(|case| {
                case.name == name && case.kind == sepika_core::model::LoadCaseKind::Seismic
            })
            .map(|case| case.id)
            .ok_or_else(|| format!("Rs評価に必要な{name}荷重ケースがありません"))
    };
    Ok([find(EX_CASE_NAME)?, find(EY_CASE_NAME)?])
}

fn standard_axis(model: &Model, id: LoadCaseId) -> Option<usize> {
    model
        .load_cases
        .iter()
        .find(|case| case.id == id)
        .and_then(|case| match (case.name.as_str(), case.kind) {
            (EX_CASE_NAME, sepika_core::model::LoadCaseKind::Seismic) => Some(0),
            (EY_CASE_NAME, sepika_core::model::LoadCaseKind::Seismic) => Some(1),
            _ => None,
        })
}

/// 元のX/Y解析結果で自立壁の剛性を確定し、Rs評価専用のモデルを作る。
/// 支持床の剛床帰属を一意に解決できなければErr。入力モデルは変更しない。
pub fn evaluation_model(model: &Model, x: &StaticOnce, y: &StaticOnce) -> Result<Model, String> {
    model.stress_cfg.validate_misc_wall_n()?;
    let mut evaluated = model.clone();
    if model.stress_cfg.misc_wall_n.is_none() {
        return Ok(evaluated);
    }
    for layer in model.layers() {
        if !has_misc_wall(model, layer.top)? {
            continue;
        }
        let columns = column_stiffnesses_from_analysis(model, layer.top, x, y)?;
        for wall in misc_wall_stiffnesses(model, layer.top, &columns)? {
            let stiffness = wall.in_plane_stiffness();
            if !stiffness.is_finite() || stiffness < 0.0 {
                return Err(format!("Rs評価: 壁{}の付加剛性が不正です", wall.wall.0));
            }
            let lower = add_floor_point(&mut evaluated, layer.bottom, wall.pos)
                .map_err(|reason| format!("Rs評価: 壁{}下端: {reason}", wall.wall.0))?;
            let upper = add_floor_point(&mut evaluated, layer.top, wall.pos)
                .map_err(|reason| format!("Rs評価: 壁{}上端: {reason}", wall.wall.0))?;
            evaluated.elements.push(ElementData {
                id: ElemId(evaluated.elements.len() as u32),
                kind: ElementKind::NodalSpring,
                nodes: [lower, upper].into_iter().collect(),
                section: None,
                local_axis: LocalAxis {
                    ref_vector: [wall.dir[0], wall.dir[1], 0.0],
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                plastic_zone: None,
                spring: Some([0.0, stiffness, 0.0, 0.0, 0.0, 0.0]),
                end_offsets_mm: [[0.0; 3]; 2],
            });
        }
    }
    Ok(evaluated)
}

fn add_floor_point(model: &mut Model, story: StoryId, pos: [f64; 2]) -> Result<NodeId, String> {
    let z = model
        .stories
        .get(story.index())
        .and_then(|s| s.sl_mm())
        .ok_or_else(|| format!("階{}のSLが不明です", story.0))?;
    let candidates: Vec<_> = model.diaphragms_of(story).map(|d| d.master).collect();
    let master = if candidates.len() == 1 {
        Some(candidates[0])
    } else if candidates.is_empty() {
        let structural = sepika_core::dof::structural_nodes(model);
        let floor_nodes: Vec<_> = model
            .nodes
            .iter()
            .filter(|n| model.on_diaphragm_level(story, n.coord[2]) && structural[n.id.index()])
            .collect();
        if floor_nodes.is_empty()
            || floor_nodes.iter().any(|n| {
                [Dof::Ux, Dof::Uy, Dof::Rz]
                    .iter()
                    .any(|d| !n.restraint.is_fixed(*d))
            })
        {
            return Err(format!(
                "階{}の接続可能な剛床又は固定支持がありません",
                story.0
            ));
        }
        None
    } else {
        let mut containing = Vec::new();
        for d in model.diaphragms_of(story) {
            if model.floor_regions.iter().any(|region| {
                model.floor_region_contains_point_including_boundary(region.id, [pos[0], pos[1], z])
                    && region
                        .boundary
                        .iter()
                        .all(|id| *id == d.master || d.slaves.contains(id))
            }) {
                containing.push(d.master);
            }
        }
        if containing.len() != 1 {
            return Err(format!("階{}の壁位置の剛床を一意に特定できません", story.0));
        }
        Some(containing[0])
    };
    let id = NodeId(model.nodes.len() as u32);
    let restraint = if master.is_some() {
        let mut mask = Dof6Mask::FIXED;
        for dof in [Dof::Ux, Dof::Uy, Dof::Rz] {
            mask.set_free(dof);
        }
        mask
    } else {
        Dof6Mask::FIXED
    };
    model.nodes.push(Node {
        id,
        coord: [pos[0], pos[1], z],
        restraint,
        mass: None,
        story: Some(story),
        support_spring: None,
    });
    if let Some(master) = master {
        for constraint in &mut model.constraints {
            if let Constraint::RigidDiaphragm {
                master: m, slaves, ..
            } = constraint
            {
                if *m == master {
                    slaves.push(id);
                }
            }
        }
    }
    Ok(id)
}

fn requires_reanalysis(model: &Model) -> Result<bool, String> {
    model.stress_cfg.validate_misc_wall_n()?;
    for layer in model.layers() {
        if has_misc_wall(model, layer.top)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 一括解析の標準EX/EY結果へRs評価専用変位を付加する。n未指定なら処理しない。
/// 評価失敗は通常の応力結果を維持し、専用変位のErrへ格納する。
pub fn attach_evaluations(
    model: &Model,
    ids: &[LoadCaseId],
    results: &mut [Result<StaticOnce, SolveError>],
) {
    if model.stress_cfg.misc_wall_n.is_none() {
        return;
    }
    let needed = requires_reanalysis(model);
    if matches!(needed, Ok(false)) {
        for (id, result) in ids.iter().zip(results) {
            if standard_axis(model, *id).is_some() {
                if let Ok(result) = result {
                    result.stiffness_ratio_disp = Some(Ok(result.disp.clone()));
                }
            }
        }
        return;
    }
    let computed = (|| {
        needed?;
        let cases = standard_cases(model)?;
        let result = |id| {
            ids.iter()
                .position(|candidate| *candidate == id)
                .and_then(|i| results.get(i))
                .and_then(|r| r.as_ref().ok())
                .ok_or_else(|| {
                    format!("Rs評価に必要な荷重ケース{}の一次解析結果がありません", id.0)
                })
        };
        let evaluated = evaluation_model(model, result(cases[0])?, result(cases[1])?)?;
        Ok::<_, String>(cases.map(|id| {
            linear_static_once(&evaluated, id)
                .map(|mut r| {
                    r.disp.truncate(model.nodes.len());
                    r.disp
                })
                .map_err(|e| e.to_string())
        }))
    })();
    for (id, result) in ids.iter().zip(results) {
        let Some(axis) = standard_axis(model, *id) else {
            continue;
        };
        if let Ok(result) = result {
            result.stiffness_ratio_disp = Some(match &computed {
                Ok(values) => values[axis].clone(),
                Err(reason) => Err(reason.clone()),
            });
        }
    }
}

/// 単体解析の標準EX/EYへRs評価変位を付加する。他方向の同じ同期荷重ケースを一次解析する。
pub fn attach_single_evaluation(
    model: &Model,
    analysis: &Analysis<'_>,
    lc: LoadCaseId,
    result: &mut StaticOnce,
) {
    if model.stress_cfg.misc_wall_n.is_none() || standard_axis(model, lc).is_none() {
        return;
    }
    match requires_reanalysis(model) {
        Ok(false) => {
            result.stiffness_ratio_disp = Some(Ok(result.disp.clone()));
            return;
        }
        Err(reason) => {
            result.stiffness_ratio_disp = Some(Err(reason));
            return;
        }
        Ok(true) => {}
    }
    let cases = match standard_cases(model) {
        Ok(cases) if cases.contains(&lc) => cases,
        Ok(_) => return,
        Err(reason) => {
            result.stiffness_ratio_disp = Some(Err(reason));
            return;
        }
    };
    let other = if lc == cases[0] { cases[1] } else { cases[0] };
    if model
        .load_cases
        .iter()
        .find(|case| case.id == other)
        .is_some_and(crate::compute::missing_seismic_horizontal_load)
    {
        result.stiffness_ratio_disp =
            Some(Err("Rs評価に必要な他方向のAi地震水平力がありません".into()));
        return;
    }
    let mut results = [Ok(result.clone()), analysis.linear_static(other)];
    attach_evaluations(model, &[lc, other], &mut results);
    if let Ok(evaluated) = &results[0] {
        result.stiffness_ratio_disp = evaluated.stiffness_ratio_disp.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::frame_gen::{frame_model, BaseSupport, FrameSpec};
    use sepika_core::ids::{MaterialId, SectionId, WallPlateId};
    use sepika_core::model::*;

    fn fixture(stories: usize) -> Model {
        let mut model = frame_model(&FrameSpec {
            x_spans: vec![],
            y_spans: vec![],
            story_heights: vec![1000.0; stories],
            sl_offsets_mm: vec![Some(0.0); stories + 1],
            with_girders: false,
            with_slabs: false,
            base_support: BaseSupport::Fixed,
            ..Default::default()
        })
        .unwrap();
        model.panel_zone = PanelZoneMode::None;
        model.stress_cfg.consider_rigid_zones = false;
        model.stress_cfg.misc_wall_n = Some(1.0);
        model.materials.push(Material {
            id: MaterialId(0),
            name: "弾性".into(),
            category: MaterialCategory::Steel,
            young: 1.0,
            poisson: 0.0,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        });
        model.materials.push(Material {
            id: MaterialId(1),
            category: MaterialCategory::Concrete,
            fc: Some(24.0),
            ..model.materials[0].clone()
        });
        model.sections.push(Section {
            id: SectionId(0),
            name: "柱".into(),
            frame_use: Some(FrameSectionUse::Column),
            floor: None,
            area: 1.0,
            iy: 1e9 / 12.0,
            iz: 1e9 / 12.0,
            j: 2000.0,
            depth: 0.0,
            width: 0.0,
            as_y: 1e18,
            as_z: 1e18,
            panel_thickness: None,
            thickness: None,
            shape: None,
            material: Some(MaterialId(0)),
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        });
        model.sections.push(Section {
            id: SectionId(1),
            name: "壁".into(),
            frame_use: None,
            thickness: Some(0.001),
            material: Some(MaterialId(1)),
            ..model.sections[0].clone()
        });
        for element in &mut model.elements {
            element.section = Some(SectionId(0));
            element.end_offsets_mm = [[0.0, 0.0, 0.0]; 2];
        }
        for node in &mut model.nodes {
            node.story = Some(StoryId(node.id.0));
            model.stories[node.id.index()].node_ids = vec![node.id];
            node.mass = Some([1.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            if node.id.0 > 0 {
                node.restraint = Dof6Mask::FIXED;
                for d in [Dof::Ux, Dof::Uy, Dof::Rz] {
                    node.restraint.set_free(d);
                }
                model.constraints.push(Constraint::rigid_diaphragm(
                    node.story.unwrap(),
                    node.id,
                    vec![],
                ));
            }
        }
        model.load_cases.clear();
        for (axis, name) in [EX_CASE_NAME, EY_CASE_NAME].into_iter().enumerate() {
            model.load_cases.push(LoadCase {
                origin: Default::default(),
                id: LoadCaseId(axis as u32),
                name: name.into(),
                kind: LoadCaseKind::Seismic,
                nodal: (1..=stories)
                    .map(|i| {
                        let mut values = [0.0; 6];
                        values[axis] = 2.0;
                        NodalLoad::auto(NodeId(i as u32), values)
                    })
                    .collect(),
                member: vec![],
            });
        }
        model
    }

    fn wall(model: &mut Model, story: usize, pos: [f64; 2], dir: [f64; 2]) {
        let z = (story - 1) as f64 * 1000.0;
        let nodes = [-0.5, 0.5].map(|t| {
            let id = NodeId(model.nodes.len() as u32);
            model.nodes.push(Node {
                id,
                coord: [
                    pos[0] + t * 1000.0 * dir[0],
                    pos[1] + t * 1000.0 * dir[1],
                    z,
                ],
                restraint: Dof6Mask::FIXED,
                mass: None,
                story: Some(StoryId((story - 1) as u32)),
                support_spring: None,
            });
            id
        });
        model.wall_plates.push(WallPlate {
            id: WallPlateId(model.wall_plates.len() as u32),
            shape: WallPlateShape::Attached {
                anchor: RegionAnchor::FloorRegion { nodes },
                extent: None,
            },
            section: Some(SectionId(1)),
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: vec![],
            loads: vec![],
            slit: Default::default(),
            dl_support: None,
            self_weight_shares: vec![],
        });
    }

    fn base_results(model: &Model) -> [Result<StaticOnce, SolveError>; 2] {
        [0, 1].map(|id| linear_static_once(model, LoadCaseId(id)))
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
    }

    #[test]
    fn 壁位置による回転を再解析し通常応力結果は変えない() {
        let mut model = fixture(1);
        wall(&mut model, 1, [0.0, 1.0], [1.0, 0.0]);
        let mut results = base_results(&model);
        let before = results[0].as_ref().unwrap().clone();
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        let result = results[0].as_ref().unwrap();
        let evaluated = result
            .stiffness_ratio_disp
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        near(evaluated[1][0], 4.0 / 3.0);
        near(evaluated[1][5], 2.0 / 3.0);
        assert_eq!(result.disp, before.disp);
        assert_eq!(
            bincode::serialize(&result.member_forces).unwrap(),
            bincode::serialize(&before.member_forces).unwrap()
        );
        assert_eq!(evaluated.len(), model.nodes.len());
    }

    #[test]
    fn torsional_cancellation_holds_misc_wall_rs_reanalysis() {
        let mut model = fixture(1);
        for i in 0..2 {
            let mut node = model.nodes[i].clone();
            node.id = NodeId(i as u32 + 2);
            node.coord[0] = 6000.0;
            model.stories[i].node_ids.push(node.id);
            model.nodes.push(node);
        }
        model.constraints[0] = Constraint::rigid_diaphragm(StoryId(1), NodeId(1), vec![NodeId(3)]);
        let mut column = model.elements[0].clone();
        column.id = ElemId(1);
        column.nodes = [NodeId(2), NodeId(3)].into_iter().collect();
        model.elements.push(column);
        wall(&mut model, 1, [3000.0, 0.0], [1.0, 0.0]);

        let mut results = base_results(&model);
        let y = results[1].as_mut().unwrap();
        for (head, uy_mm) in [(1, 0.01), (3, 1.99)] {
            y.disp[head][Dof::Uy as usize] = uy_mm;
            y.disp[head][Dof::Rz as usize] = 0.00033;
        }
        let original_displacements = results.each_ref().map(|r| r.as_ref().unwrap().disp.clone());
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        for (result, original) in results.into_iter().zip(original_displacements) {
            let result = result.unwrap();
            assert_eq!(result.disp, original);
            let reason = result.stiffness_ratio_disp.unwrap().unwrap_err();
            for expected in ["階1", "Y方向", "柱0", "ねじれ", "相殺"] {
                assert!(reason.contains(expected), "{reason}");
            }
        }
    }

    #[test]
    fn inclined_column_force_determines_misc_wall_reference_or_explains_hold() {
        for method in [
            MiscWallStiffnessMethod::StoryColumns,
            MiscWallStiffnessMethod::RepresentativeColumn,
        ] {
            for reversed in [false, true] {
                let mut model = fixture(1);
                model.nodes[0].coord[0] = -1000.0;
                if reversed {
                    model.elements[0].nodes.swap(0, 1);
                }
                model.stress_cfg.misc_wall_method = method;
                model.stress_cfg.misc_wall_representatives = vec![MiscWallRepresentativeColumn {
                    story: StoryId(1),
                    column: ElemId(0),
                }];
                wall(&mut model, 1, [0.0, 0.0], [1.0, 0.0]);
                let [x, y] = base_results(&model).map(Result::unwrap);
                let expected_column_x = 2.0 / x.disp[1][0];
                let evaluated = evaluation_model(&model, &x, &y).unwrap();
                near(
                    evaluated.elements.last().unwrap().spring.unwrap()[1],
                    expected_column_x,
                );
                let mut results = [Ok(x), Ok(y)];
                attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
                let result = results[0].as_ref().unwrap();
                let drift = result
                    .stiffness_ratio_disp
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap()[1][0];
                near(drift, result.disp[1][0] / 2.0);

                model.elements[0].end_offsets_mm[usize::from(!reversed)][2] = 100.0;
                let mut results = base_results(&model);
                attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
                for result in results {
                    let reason = result.unwrap().stiffness_ratio_disp.unwrap().unwrap_err();
                    assert!(
                        reason.contains("柱0") && reason.contains("切断位置"),
                        "{reason}"
                    );
                }
            }
        }
    }

    #[test]
    fn inclined_column_force_reaches_planar_and_spatial_lumped_models() {
        use sepika_core::units::GRAVITY_MM_S2;
        use sepika_solver::dynamic::lumped_mass::{
            LumpedMassType, LumpedStiffnessSource, StickDim,
        };
        use sepika_solver::statics::analysis::SeismicDir;

        let mut model = fixture(1);
        model.nodes[0].coord[0] = -1000.0;
        model.elements[0].nodes.swap(0, 1);
        model.stories[1].dynamic_mass = Some(StoryDynamicMass {
            horizontal_inertia_t_mm2: [0.0; 3],
            mass_equiv_weight_n: GRAVITY_MM_S2,
            lumped_mass: None,
            center_xy_mm: [0.0, 0.0],
            inertia_t_mm2: 1e6,
        });
        wall(&mut model, 1, [0.0, 1000.0], [1.0, 0.0]);
        model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
        let [x, y] = base_results(&model).map(Result::unwrap);
        let expected = 2.0 / x.disp[1][0];
        let input = crate::LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentShear,
            model: &model,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::ColumnKi,
            dir: SeismicDir::X,
            nonlinear: false,
            secant_ratio: 0.75,
            target_drift_angle: 1.0 / 200.0,
            res_x: Some(&x),
            res_y: Some(&y),
            po_x: None,
            po_y: None,
        };
        let planar = crate::build_lumped_mass(input).unwrap();
        near(planar.stories[0].skeleton.k1, expected);
        for source in [
            LumpedStiffnessSource::ColumnKi,
            LumpedStiffnessSource::StoryQd,
        ] {
            let spatial = crate::build_lumped_mass(crate::LumpedMassBuildInput {
                dim: StickDim::Spatial,
                source,
                ..input
            })
            .unwrap();
            let layer = &spatial.spatial[0];
            near(layer.rigidity_xy[0], 0.0);
            near(layer.rigidity_xy[1], 500.0);
            near(
                layer.k1_x,
                if source == LumpedStiffnessSource::ColumnKi {
                    2.0 * expected
                } else {
                    expected
                },
            );
            near(layer.kr / 500_000.0, expected);
        }
    }

    #[test]
    fn shared_sl_columns_and_interior_branches_reach_wall_and_lumped_model_evaluation() {
        use sepika_core::units::GRAVITY_MM_S2;
        use sepika_solver::dynamic::lumped_mass::{
            LumpedMassType, LumpedStiffnessSource, StickDim,
        };
        use sepika_solver::statics::analysis::SeismicDir;

        let mut model = fixture(1);
        model.nodes[0].coord[0] = -500.0;
        let mut mirror_base = model.nodes[0].clone();
        mirror_base.id = NodeId(2);
        mirror_base.coord[0] = 500.0;
        model.nodes.push(mirror_base);
        model.stories[0].node_ids.push(NodeId(2));
        let mut mirror_column = model.elements[0].clone();
        mirror_column.id = ElemId(1);
        mirror_column.nodes[0] = NodeId(2);
        model.elements.push(mirror_column);
        model.stories[1].dynamic_mass = Some(StoryDynamicMass {
            horizontal_inertia_t_mm2: [0.0; 3],
            mass_equiv_weight_n: GRAVITY_MM_S2,
            lumped_mass: None,
            center_xy_mm: [0.0, 0.0],
            inertia_t_mm2: 1e6,
        });
        wall(&mut model, 1, [0.0, 1000.0], [1.0, 0.0]);
        model.wall_weight_generation = Some(WallWeightGenerationMode::GravityCasesOnly);
        model.stress_cfg.misc_wall_representatives = vec![MiscWallRepresentativeColumn {
            story: StoryId(1),
            column: ElemId(1),
        }];
        let [x, y] = base_results(&model).map(Result::unwrap);
        let expected_columns = 2.0 / x.disp[1][0].abs();

        let build = |model: &Model, x: &StaticOnce, y: &StaticOnce, dim, source| {
            crate::build_lumped_mass(crate::LumpedMassBuildInput {
                model_type: LumpedMassType::EquivalentShear,
                model,
                dim,
                source,
                dir: SeismicDir::X,
                nonlinear: false,
                secant_ratio: 0.75,
                target_drift_angle: 1.0 / 200.0,
                res_x: Some(x),
                res_y: Some(y),
                po_x: None,
                po_y: None,
            })
        };
        for method in [
            MiscWallStiffnessMethod::StoryColumns,
            MiscWallStiffnessMethod::RepresentativeColumn,
        ] {
            model.stress_cfg.misc_wall_method = method;
            let evaluated = evaluation_model(&model, &x, &y).unwrap();
            near(
                evaluated.elements.last().unwrap().spring.unwrap()[1],
                expected_columns / 2.0,
            );
            let spatial = build(
                &model,
                &x,
                &y,
                StickDim::Spatial,
                LumpedStiffnessSource::ColumnKi,
            )
            .unwrap();
            near(spatial.spatial[0].k1_x, expected_columns * 1.5);
        }
        let planar = build(
            &model,
            &x,
            &y,
            StickDim::Planar,
            LumpedStiffnessSource::ColumnKi,
        )
        .unwrap();
        near(planar.stories[0].skeleton.k1, expected_columns);

        let mut middle = model.nodes[1].clone();
        let middle_id = NodeId(model.nodes.len() as u32);
        middle.id = middle_id;
        middle.story = None;
        middle.mass = None;
        middle.restraint = Dof6Mask::FREE;
        middle.coord[2] = 500.0;
        model.nodes.push(middle);
        for lower in &mut model.elements {
            lower.nodes[1] = middle_id;
        }
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(2);
        upper.nodes = [middle_id, NodeId(1)].into_iter().collect();
        model.elements.push(upper);

        let mut results = base_results(&model);
        let reason = evaluation_model(
            &model,
            results[0].as_ref().unwrap(),
            results[1].as_ref().unwrap(),
        )
        .unwrap_err();
        assert!(reason.contains("階1の柱2は中間節点で分岐"), "{reason}");
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        for response in &results {
            assert_eq!(
                response.as_ref().unwrap().stiffness_ratio_disp.as_ref(),
                Some(&Err(reason.clone()))
            );
        }
        let [x, y] = results.map(Result::unwrap);
        for (dim, source) in [
            (StickDim::Planar, LumpedStiffnessSource::ColumnKi),
            (StickDim::Spatial, LumpedStiffnessSource::ColumnKi),
            (StickDim::Spatial, LumpedStiffnessSource::StoryQd),
        ] {
            let failure = build(&model, &x, &y, dim, source).unwrap_err();
            assert!(failure.to_string().contains(&reason), "{failure}");
        }
    }

    #[test]
    fn re_1640_wall_stiffness_evaluation_keeps_the_sl_interval_error() {
        let mut model = fixture(1);
        let mut split = model.nodes[1].clone();
        split.id = NodeId(2);
        split.story = None;
        split.mass = None;
        split.restraint = Dof6Mask::FREE;
        split.coord[2] = 999.5;
        model.nodes.push(split);
        let mut upper = model.elements[0].clone();
        upper.id = ElemId(1);
        upper.nodes[0] = NodeId(2);
        model.elements[0].nodes[1] = NodeId(2);
        model.elements.push(upper);
        wall(&mut model, 1, [0.0, 1.0], [1.0, 0.0]);

        let mut results = base_results(&model);
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        for result in results {
            let result = result.unwrap();
            let reason = result.stiffness_ratio_disp.unwrap().unwrap_err();
            assert!(reason.contains("上階SLの柱1"), "{reason}");
            assert!(reason.contains("一意に特定できません"), "{reason}");
        }
    }

    #[test]
    fn 斜め壁のxy回転連成を同時に解く() {
        let mut model = fixture(1);
        model.sections[0].iz *= 4.0;
        let c = 0.5_f64.sqrt();
        wall(&mut model, 1, [0.0, 1.0], [c, c]);
        let mut results = base_results(&model);
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        let evaluated = results[0]
            .as_ref()
            .unwrap()
            .stiffness_ratio_disp
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        near(evaluated[1][0], 82.0 / 61.0);
        near(evaluated[1][1], -10.0 / 61.0);
        near(evaluated[1][5], 40.0 / 61.0);
    }

    #[test]
    fn 上層の壁は直下階へ接続し地盤へ直結しない() {
        let mut model = fixture(2);
        wall(&mut model, 2, [0.0, 0.0], [1.0, 0.0]);
        let mut results = base_results(&model);
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        let evaluated = results[0]
            .as_ref()
            .unwrap()
            .stiffness_ratio_disp
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap();
        near(evaluated[1][0], 4.0);
        near(evaluated[2][0], 5.0);
        let drifts =
            sepika_design_jp::secondary::stiffness_ratio::cog_story_drifts(&model, evaluated, 0)
                .unwrap();
        let rs = sepika_design_jp::secondary::holding_capacity::stiffness_ratios(
            &[1000.0, 1000.0],
            &drifts,
        )
        .unwrap();
        near(rs[0], 0.4);
        near(rs[1], 1.6);
    }

    #[test]
    fn misc740_微小な同比例荷重でも両方式の柱壁剛性と二層rsは不変() {
        for method in [
            MiscWallStiffnessMethod::StoryColumns,
            MiscWallStiffnessMethod::RepresentativeColumn,
        ] {
            for scale in [1.0, 1e-10] {
                let mut model = fixture(2);
                model.stress_cfg.misc_wall_method = method;
                model.stress_cfg.misc_wall_representatives = vec![MiscWallRepresentativeColumn {
                    story: StoryId(2),
                    column: ElemId(1),
                }];
                wall(&mut model, 2, [0.0, 0.0], [1.0, 0.0]);
                for case in &mut model.load_cases {
                    for load in &mut case.nodal {
                        for value in &mut load.values {
                            *value *= scale;
                        }
                    }
                }
                let mut results = base_results(&model);
                let columns = column_stiffnesses_from_analysis(
                    &model,
                    StoryId(2),
                    results[0].as_ref().unwrap(),
                    results[1].as_ref().unwrap(),
                )
                .unwrap();
                near(columns[0].dx, 1.0);
                near(columns[0].dy, 1.0);
                let walls = misc_wall_stiffnesses(&model, StoryId(2), &columns).unwrap();
                near(walls[0].in_plane_stiffness(), 1.0);
                attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
                let evaluated = results[0]
                    .as_ref()
                    .unwrap()
                    .stiffness_ratio_disp
                    .as_ref()
                    .unwrap()
                    .as_ref()
                    .unwrap();
                let drifts = sepika_design_jp::secondary::stiffness_ratio::cog_story_drifts(
                    &model, evaluated, 0,
                )
                .unwrap();
                let rs = sepika_design_jp::secondary::holding_capacity::stiffness_ratios(
                    &[1000.0, 1000.0],
                    &drifts,
                )
                .unwrap();
                near(rs[0], 0.4);
                near(rs[1], 1.6);
            }
        }
    }

    #[test]
    fn mixed_column_model_supplies_both_misc_wall_reference_methods() {
        for method in [
            MiscWallStiffnessMethod::StoryColumns,
            MiscWallStiffnessMethod::RepresentativeColumn,
        ] {
            let mut model = fixture(2);
            model.elements[1].kind = ElementKind::MultiSpring;
            model.stress_cfg.misc_wall_method = method;
            model.stress_cfg.misc_wall_representatives = vec![MiscWallRepresentativeColumn {
                story: StoryId(2),
                column: ElemId(1),
            }];
            wall(&mut model, 2, [0.0, 0.0], [1.0, 0.0]);
            let mut results = base_results(&model);
            attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
            let evaluated = results[0]
                .as_ref()
                .unwrap()
                .stiffness_ratio_disp
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            near(evaluated[1][0], 4.0);
            near(evaluated[2][0], 5.0);
        }
    }

    #[test]
    fn 対象壁がなければ他方向や柱内力を要求しない() {
        let model = fixture(1);
        let mut results = [linear_static_once(&model, LoadCaseId(0))];
        results[0].as_mut().unwrap().member_forces.clear();
        attach_evaluations(&model, &[LoadCaseId(0)], &mut results);
        let result = results[0].as_ref().unwrap();
        assert_eq!(
            result
                .stiffness_ratio_disp
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap(),
            &result.disp
        );
    }

    #[test]
    fn 床帰属が不明なら通常変位へ補完しない() {
        let mut model = fixture(1);
        wall(&mut model, 1, [0.0, 1.0], [1.0, 0.0]);
        let mut results = base_results(&model);
        model.constraints.clear();
        attach_evaluations(&model, &[LoadCaseId(0), LoadCaseId(1)], &mut results);
        assert!(results[0]
            .as_ref()
            .unwrap()
            .stiffness_ratio_disp
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap_err()
            .contains("上端"));
    }
}
