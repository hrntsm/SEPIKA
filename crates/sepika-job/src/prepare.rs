//! 解析前処理（モデルを解ける状態へ整える）。
//!
//! **解析の直前に必ず通すこと。** 前処理を省くと、仕口パネルのない剛性で解いたり、
//! 地震力ゼロで増分解析したりすることになる。

use sepika_core::model::Model;
use sepika_core::region_rebuild::rebuild_floor_regions;
use sepika_core::wall_region_rebuild::rebuild_wall_regions;

use crate::auto_loads::{
    apply_auto_load_cases, apply_tip_loads, clear_failed_tip_seismic_cases,
    compute_gravity_auto_load_cases, compute_seismic_auto_load_cases, compute_tip_loads,
};
use crate::error::JobError;
use crate::settings::AnalysisSettings;

/// 解析前処理（剛域・仕口パネル・荷重自動同期）の報告。
pub struct PrepareReport {
    /// 生成した仕口パネル（GUI の準備計算表が表示する）。
    pub panels: Vec<sepika_element::springs::panel_gen::GeneratedPanel>,
    /// 荷重同期で発生した注意事項（SemiPrecise で固有周期未算定など）。
    pub notices: Vec<String>,
    /// 領域再生成で節点が削除され、節点 ID の付け直しが発生した。
    pub nodes_renumbered: bool,
}

/// 剛域と仕口パネルを自動算定してモデルへ反映する。
/// 剛域算定は壁展開モデルに対して行い、既存要素へ書き戻す。
/// 仕口パネルは壁に依存しないため非展開モデルに対して算定する。
/// いずれも冪等で、呼び出し順にも依存しない。
pub fn apply_rigid_zones_and_panels(
    model: &mut Model,
) -> Vec<sepika_element::springs::panel_gen::GeneratedPanel> {
    let rule = sepika_element::frame::beam::RigidZoneRule {
        consider_walls: model.stress_cfg.rigid_zone_consider_walls,
    };
    if sepika_load::wall_expand::model_has_wall_plates_to_expand(model) {
        let (mut expanded, _wall_index, _wall_report) =
            sepika_load::wall_expand::expand_wall_elements(model);
        sepika_element::frame::beam::apply_auto_rigid_zones(&mut expanded, &rule);
        debug_assert!(
            model.elements.len() <= expanded.elements.len()
                && model
                    .elements
                    .iter()
                    .zip(expanded.elements.iter())
                    .all(|(dst, src)| dst.id == src.id),
            "壁展開後の先頭要素列の ElemId が非展開モデルと一致しない。\
             expand_wall_elements が既存要素の途中へ挿入していないか確認すること。"
        );
        for (dst, src) in model.elements.iter_mut().zip(expanded.elements.iter()) {
            dst.rigid_zone = src.rigid_zone;
        }
    } else {
        sepika_element::frame::beam::apply_auto_rigid_zones(model, &rule);
    }
    sepika_element::springs::panel_gen::apply_auto_panel_zones(model)
}

/// 解析前処理を一括で行う（剛域・仕口パネル・荷重ケースの自動同期）。
pub fn prepare_model_for_analysis(
    model: &mut Model,
    settings: &AnalysisSettings,
    design_period: Option<f64>,
) -> Result<PrepareReport, JobError> {
    prepare_model(model, settings, design_period, false)
}

/// 明示準備では階未定義時に床レベルを初期化し、解析直前では既存階を保持する。
/// 地震力算定不能は旧 Auto EX/EY を両方除去し、独立した重力解析を許す。
pub fn prepare_model(
    model: &mut Model,
    settings: &AnalysisSettings,
    design_period: Option<f64>,
    initialize_stories: bool,
) -> Result<PrepareReport, JobError> {
    let mut work = model.clone();
    let _ = work.anchorize_secondary_members();
    work.rebuild_assignment_regions()
        .map_err(JobError::InvalidInput)?;
    let report = prepare_work_model(&mut work, settings, design_period, initialize_stories);
    match report {
        Ok(report) => {
            *model = work;
            Ok(report)
        }
        Err(error) => {
            let seismic = compute_seismic_auto_load_cases(model, settings, design_period);
            invalidate_design_weights(model);
            if seismic.notices.is_empty() {
                Err(error)
            } else {
                Err(JobError::InvalidInput(format!(
                    "{error}; {}",
                    seismic.notices.join("; ")
                )))
            }
        }
    }
}

fn prepare_work_model(
    model: &mut Model,
    settings: &AnalysisSettings,
    design_period: Option<f64>,
    initialize_stories: bool,
) -> Result<PrepareReport, JobError> {
    model
        .validate_attached_slabs()
        .map_err(|e| JobError::InvalidInput(e.to_string()))?;
    rebuild_floor_regions(model);
    let wall_report = rebuild_wall_regions(model);
    let panels = apply_rigid_zones_and_panels(model);
    let gravity = compute_gravity_auto_load_cases(model)?;
    let tip_loads = compute_tip_loads(model)?;
    apply_auto_load_cases(model, &gravity.cases);
    apply_tip_loads(model, tip_loads);
    let mut notices = gravity.notices;
    let mut weights_valid = true;
    if (!model.stories.is_empty() || initialize_stories)
        && !crate::weight_preparation::weights_are_current(model, settings.mass_method)
    {
        let cases = crate::gravity_case_ids_for_seismic_weight(model);
        let generated = if model.load_cases.iter().any(|case| {
            case.kind == sepika_core::model::LoadCaseKind::Dead
                && case.name == sepika_core::model::DL_CASE_NAME
        }) {
            sepika_load::story_gen::generate_stories_with_synced_self_weight(
                model,
                &cases,
                settings.mass_method,
            )
        } else {
            sepika_load::story_gen::generate_stories_with_opts(
                model,
                &cases,
                true,
                settings.mass_method,
            )
        };
        match generated {
            Ok(generated) => crate::weight_preparation::apply_generated_weights(
                model,
                generated,
                settings.mass_method,
            ),
            Err(error) => {
                weights_valid = false;
                invalidate_generated_weights(model);
                for name in [
                    sepika_core::model::EX_CASE_NAME,
                    sepika_core::model::EY_CASE_NAME,
                ] {
                    notices.push(format!("{name} の Ai 地震力を再生成できません: 地震用重量を再生成できません: {error}"));
                }
            }
        }
    }
    let mut seismic = if weights_valid {
        compute_seismic_auto_load_cases(model, settings, design_period)
    } else {
        crate::auto_loads::AutoLoadComputeResult {
            cases: Vec::new(),
            notices: Vec::new(),
        }
    };
    clear_failed_tip_seismic_cases(model, &mut seismic);
    apply_auto_load_cases(model, &seismic.cases);
    notices.extend(seismic.notices);
    if let Some(warning) = model.unset_plate_assignment_warning() {
        notices.push(warning);
    }
    Ok(PrepareReport {
        panels,
        notices,
        nodes_renumbered: wall_report.deleted_nodes > 0,
    })
}

/// 設計重量の準備失敗で旧階重量と標準Auto DL/EX/EYを無効化する。手入力は保持する。
pub fn invalidate_design_weights(model: &mut Model) {
    invalidate_generated_weights(model);
    clear_standard_seismic_auto(model);
    for case in &mut model.load_cases {
        if case.kind == sepika_core::model::LoadCaseKind::Dead
            && case.name == sepika_core::model::DL_CASE_NAME
        {
            case.replace_auto_loads(Vec::new(), Vec::new());
        }
    }
}

fn invalidate_generated_weights(model: &mut Model) {
    if model.generated_masters.is_empty() && model.seismic_weight_generation.is_none() {
        return;
    }
    let record = model.seismic_weight_generation.get_or_insert_with(|| {
        sepika_core::model::SeismicWeightGeneration {
            input_key: Vec::new(),
            output_key: Vec::new(),
            calculated_weights: Vec::new(),
            automatic_diaphragms: Vec::new(),
            automatic_master_restraints: Vec::new(),
        }
    });
    record.input_key.clear();
    record.output_key.clear();
    record.calculated_weights.clear();
}

/// 失敗した準備で旧自動地震力を使用させない。手入力荷重は保持する。
pub fn clear_standard_seismic_auto(model: &mut Model) {
    use sepika_core::model::{LoadCaseKind, EX_CASE_NAME, EY_CASE_NAME};
    for case in &mut model.load_cases {
        if case.kind == LoadCaseKind::Seismic
            && matches!(case.name.as_str(), EX_CASE_NAME | EY_CASE_NAME)
        {
            case.replace_auto_loads(Vec::new(), Vec::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, NodeId, SectionId};
    use sepika_core::model::{
        DistributionMethod, ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Node,
        SlabPlate,
    };
    use sepika_core::section_shape::SectionShape;

    fn node(id: u32, x: f64, y: f64) -> Node {
        Node {
            id: NodeId(id),
            coord: [x, y, 0.0],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    fn beam(id: u32, i: u32, j: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        }
    }

    /// 閉路 1 + 床板 2 枚。荷重同期の前に、大梁の区画（床領域）1 つへ帰属し直す
    /// （床板そのものは畳まない。申し送り「床領域・壁領域の再設計」参照）。
    #[test]
    fn prepare_groups_two_slabs_into_one_region_before_loads() {
        let mut model = Model::default();
        for (i, (x, y)) in [
            (0.0, 0.0),
            (2000.0, 0.0),
            (4000.0, 0.0),
            (4000.0, 4000.0),
            (2000.0, 4000.0),
            (0.0, 4000.0),
        ]
        .into_iter()
        .enumerate()
        {
            model.nodes.push(node(i as u32, x, y));
        }
        model.elements.extend([
            beam(0, 0, 1),
            beam(1, 1, 2),
            beam(2, 2, 3),
            beam(3, 3, 4),
            beam(4, 4, 5),
            beam(5, 5, 0),
        ]);
        let sid = SectionId(0);
        model
            .sections
            .push(SectionShape::RcSlab { thickness: 150.0 }.to_section(sid, "S150".into()));
        model
            .unassigned_beams
            .push(sepika_core::model::SecondaryMember {
                gravity_end_shares: None,
                id: sepika_core::ids::SecondaryMemberId(0),
                kind: sepika_core::model::SecondaryMemberKind::Beam,
                ends: sepika_core::model::SecondaryMemberEnds::Supported([
                    sepika_core::model::SecondaryMemberAnchor {
                        support: sepika_core::model::SupportMemberId::Primary(ElemId(0)),
                        position: 1.0,
                    },
                    sepika_core::model::SecondaryMemberAnchor {
                        support: sepika_core::model::SupportMemberId::Primary(ElemId(4)),
                        position: 0.0,
                    },
                ]),
                section: None,
                name: "J".into(),
            });
        model.rebuild_floor_assignment_regions();
        let plate = SlabPlate {
            section: Some(sid),
            loads: Vec::new(),
            usage: None,
            method: DistributionMethod::TriTrapezoid,
            one_way: None,
        };
        model
            .assign_enclosed_slab_to_matching_region(
                &[NodeId(0), NodeId(1), NodeId(4), NodeId(5)],
                plate.clone(),
            )
            .expect("左半分");
        model
            .assign_enclosed_slab_to_matching_region(
                &[NodeId(1), NodeId(2), NodeId(3), NodeId(4)],
                plate,
            )
            .expect("右半分");
        assert_eq!(model.slabs.len(), 2);
        let _ = prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None);
        assert_eq!(model.slabs.len(), 2, "床板は畳まずそのまま残る");
        assert_eq!(
            model.floor_regions.len(),
            1,
            "大梁が囲む区画は 1 つなので床領域も 1 つ"
        );
        assert_eq!(
            model.floor_regions[0].slab_ids.len(),
            2,
            "2 枚とも同じ床領域へ帰属"
        );

        // 2 回連続で実行しても結果が変わらない（割当領域を先に再構築してから
        // 旧床領域・壁領域を再構築する順序が、実行回数に依存しないことの回帰）。
        let after_first = model.clone();
        let _ = prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None);
        assert!(
            after_first.eq_ignoring_dofmap(&model),
            "前処理が 2 回目で結果を変えている"
        );
    }

    #[test]
    fn prepare_rejects_square_short_direction() {
        let mut model = Model {
            nodes: vec![
                node(0, 0.0, 0.0),
                node(1, 4000.0, 0.0),
                node(2, 4000.0, 4000.0),
                node(3, 0.0, 4000.0),
            ],
            ..Default::default()
        };
        model.add_enclosed_slab_from_nodes(
            &[NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            SlabPlate {
                method: DistributionMethod::OneWay,
                one_way: Some(sepika_core::model::OneWayDir::Short),
                ..SlabPlate::default()
            },
        );
        let error = match prepare_model_for_analysis(&mut model, &AnalysisSettings::default(), None)
        {
            Ok(_) => panic!("正方形の短辺方向を受け入れた"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("正方形"));
    }

    /// 剛域算定（`apply_rigid_zones_and_panels`）が壁展開モデルを見ていることの
    /// 回帰テスト。壁展開しないまま算定すると `model.elements` に壁要素が
    /// 0 件のため `rigid_zone_consider_walls`（既定 true。柱の袖壁・梁の腰壁/垂壁
    /// の張り出しを剛域長へ反映する技術基準の規定）が壁の有無に関わらず常に
    /// 無効化されてしまう（`dev_docs/handoff/床領域・壁領域の再設計_申し送り.md`
    /// §5.15 参照）。
    ///
    /// 柱・梁を全周 RC にそろえる（`all_rc_src_at` が全節点で成立する構成に
    /// する）必要がある点に注意。RC/S 混在フレームでは「1 本でも S 系が
    /// 集まる仕口には剛域を設けない」規則が優先し、壁の有無による差が
    /// 現れない（この規則自体が §5.14 の `wall_bay_model` 回帰網を偶然素通り
    /// させていた原因）。
    #[test]
    fn apply_rigid_zones_considers_wall_plates() {
        use sepika_core::ids::{MaterialId, WallPlateId, WallRegionId};
        use sepika_core::model::{
            Material, MaterialCategory, WallPlate, WallPlateShape, WallRegion,
        };
        use sepika_core::section_shape::{RcBeamRebar, RcRectColumnRebar, SectionShape};

        // 主筋 3-D22・せん断補強筋 D10@100（`剛域`の算定自体は鉄筋量を見ないが、
        // 実配筋の検証を通すため名目値を与える）。
        fn column_rebar() -> RcRectColumnRebar {
            use sepika_core::section_shape::RectColumnHoop;
            RcRectColumnRebar {
                main_dia: 22.0,
                x: vec![3],
                y: vec![3],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            }
        }

        fn beam_rebar() -> RcBeamRebar {
            use sepika_core::section_shape::BeamStirrup;
            RcBeamRebar {
                main_dia: 22.0,
                top: vec![3],
                bottom: vec![3],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: 10.0,
                    pitch: 100.0,
                    legs: 2,
                },
            }
        }

        fn node3(id: u32, x: f64, y: f64, z: f64) -> Node {
            Node {
                id: NodeId(id),
                coord: [x, y, z],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            }
        }

        // 1 バイの全周 RC 架構（柱 4 本・頂部梁 4 本・柱脚間梁 4 本）。
        // Y=0 面（節点 0,1,5,4）に耐震壁 1 枚を想定する。
        fn base_frame() -> Model {
            let mut model = Model::default();
            let base = [
                (0.0, 0.0, 0.0),
                (4000.0, 0.0, 0.0),
                (4000.0, 3000.0, 0.0),
                (0.0, 3000.0, 0.0),
            ];
            let top = [
                (0.0, 0.0, 3000.0),
                (4000.0, 0.0, 3000.0),
                (4000.0, 3000.0, 3000.0),
                (0.0, 3000.0, 3000.0),
            ];
            for (i, &(x, y, z)) in base.iter().enumerate() {
                model.nodes.push(node3(i as u32, x, y, z));
            }
            for (i, &(x, y, z)) in top.iter().enumerate() {
                model.nodes.push(node3(4 + i as u32, x, y, z));
            }
            model.materials.push(Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "Fc24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            });
            let mut col_sec = SectionShape::RcColumnRect {
                b: 300.0,
                d: 300.0,
                rebar: column_rebar(),
            }
            .to_section(SectionId(0), "柱 RC 300x300".into());
            col_sec.material = Some(MaterialId(0));
            model.sections.push(col_sec);
            let mut beam_sec = SectionShape::RcBeamRect {
                b: 300.0,
                d: 400.0,
                rebar: beam_rebar(),
            }
            .to_section(SectionId(1), "梁 RC 300x400".into());
            beam_sec.material = Some(MaterialId(0));
            model.sections.push(beam_sec);

            let member = |id: u32, i: u32, j: u32, sec: u32| {
                let mut e = beam(id, i, j);
                e.section = Some(SectionId(sec));
                e
            };
            for i in 0..4u32 {
                model.elements.push(member(i, i, 4 + i, 0));
            }
            let top_pairs = [(4u32, 5u32), (5, 6), (6, 7), (7, 4)];
            for (k, (i, j)) in top_pairs.iter().enumerate() {
                model.elements.push(member(4 + k as u32, *i, *j, 1));
            }
            let base_pairs = [(0u32, 1u32), (1, 2), (2, 3), (3, 0)];
            for (k, (i, j)) in base_pairs.iter().enumerate() {
                model.elements.push(member(8 + k as u32, *i, *j, 1));
            }
            model
        }

        fn wall_section_id() -> SectionId {
            SectionId(2)
        }

        let mut with_wall = base_frame();
        let mut wall_sec = SectionShape::RcWall {
            thickness: 150.0,
            pwh_ratio: None,
            ps: 0.0025,
        }
        .to_section(wall_section_id(), "耐震壁 t150".into());
        wall_sec.material = Some(MaterialId(0));
        with_wall.sections.push(wall_sec);
        with_wall.add_enclosed_wall_plate_from_nodes(
            &[NodeId(0), NodeId(1), NodeId(5), NodeId(4)],
            WallPlate {
                dl_support: None,
                self_weight_shares: Vec::new(),
                id: WallPlateId(0),
                shape: WallPlateShape::Enclosed,
                section: Some(wall_section_id()),
                opening_area: 0.0,
                opening_weight: 0.0,
                openings: Vec::new(),
                loads: vec![],
                slit: Default::default(),
            },
        );
        with_wall.wall_regions.push(WallRegion {
            id: WallRegionId(0),
            name: String::new(),
            boundary: vec![NodeId(0), NodeId(1), NodeId(5), NodeId(4)],
            wall_plate_ids: vec![WallPlateId(0)],
            posts: Vec::new(),
        });

        let mut without_wall = base_frame();

        apply_rigid_zones_and_panels(&mut with_wall);
        apply_rigid_zones_and_panels(&mut without_wall);

        // 壁の側柱（elem 0、Y=0 面）は、壁を考慮すると剛域長が変わる。
        let with_wall_col0 = with_wall.elements[0].rigid_zone;
        let without_wall_col0 = without_wall.elements[0].rigid_zone;
        assert_ne!(
            with_wall_col0.length_i, without_wall_col0.length_i,
            "壁の有無で側柱の剛域長 length_i が変わらない\
             （壁展開モデルを見ずに算定している疑いがある）: with={:?} without={:?}",
            with_wall_col0, without_wall_col0
        );
        assert_ne!(
            with_wall_col0.length_j, without_wall_col0.length_j,
            "壁の有無で側柱の剛域長 length_j が変わらない: with={:?} without={:?}",
            with_wall_col0, without_wall_col0
        );
        // 壁ありのほうが張り出しの分だけ剛域は長くなる（少なくとも一方の端で）。
        assert!(
            with_wall_col0.length_j > without_wall_col0.length_j,
            "壁を考慮すると剛域長は伸びるはず: with={:?} without={:?}",
            with_wall_col0,
            without_wall_col0
        );

        // 壁の頂部大梁（elem 4）も同様。
        let with_wall_beam4 = with_wall.elements[4].rigid_zone;
        let without_wall_beam4 = without_wall.elements[4].rigid_zone;
        assert!(
            with_wall_beam4.length_i > without_wall_beam4.length_i
                && with_wall_beam4.length_j > without_wall_beam4.length_j,
            "壁の頂部大梁は壁を考慮すると両端とも剛域長が伸びるはず: with={:?} without={:?}",
            with_wall_beam4,
            without_wall_beam4
        );

        // `apply_rigid_zones_and_panels` 自身は壁要素をモデルへ残さない（D5）。
        // 壁展開はこの関数の内部だけの一時的な操作であること（書き戻しの実装
        // ミスで壁要素が漏れ出していないか）を確認する。
        assert!(
            with_wall
                .elements
                .iter()
                .all(|e| e.kind != sepika_core::model::ElementKind::Wall),
            "apply_rigid_zones_and_panels は壁要素を model.elements へ残してはならない"
        );
    }
}
