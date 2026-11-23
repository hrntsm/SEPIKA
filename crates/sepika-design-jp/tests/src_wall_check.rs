#[path = "../../sepika-solver/tests/support/rc_wall_side_column_model.rs"]
mod rc_wall_side_column_model;
#[path = "../../sepika-solver/tests/support/src_wall_model.rs"]
mod src_wall_model;

use sepika_core::ids::{ElemId, LoadCaseId, NodeId};
use sepika_core::model::{LoadCaseKind, NodalLoad, SteelDesignAttr};
use sepika_core::section_shape::SectionShape;
use sepika_core::src_wall_capacity::{resolve_src_wall, SrcWallEndForces};
use sepika_core::wall_expand::expand_wall_elements;
use sepika_design_jp::wall_check::WallCheckKind;
use sepika_design_jp::{
    run_member_design_checks, CheckOutcome, LoadTerm, MemberDesignCheckOptions,
};
use sepika_element::frame::beam::MemberForces;
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::superpose_static;

#[test]
fn linear_result_feeds_article_25_and_38_for_both_src_wall_specs() {
    for embedded in [false, true] {
        let mut input = src_wall_model::input_model(embedded);
        let mut seismic = input.load_cases[0].clone();
        let quake_id = LoadCaseId(input.load_cases.len() as u32);
        seismic.id = quake_id;
        seismic.kind = LoadCaseKind::Seismic;
        seismic.name = "EX".into();
        seismic.nodal = (2..=3)
            .map(|i| NodalLoad::manual(NodeId(i), [50_000.0, 0.0, 0.0, 0.0, 300e6, 0.0]))
            .collect();
        input.load_cases.push(seismic);
        let (mut model, index, _) = expand_wall_elements(&input);
        let analysis = Analysis::prepare(&model).unwrap();
        let gravity = analysis.linear_static(LoadCaseId(0)).unwrap();
        let quake = analysis.linear_static(quake_id).unwrap();
        let combined = superpose_static(&[(&gravity, 1.0), (&quake, 1.0)]).unwrap();
        let terms = [(LoadCaseId(0), 1.0), (LoadCaseId(1), 1.0), (quake_id, 1.0)];
        let report = |model: &sepika_core::Model, forces: &[(ElemId, MemberForces)], reviewed| {
            run_member_design_checks(
                model,
                forces,
                &combined.panel_moments,
                &MemberDesignCheckOptions {
                    term: LoadTerm::Short,
                    wall_index: Some(&index),
                    wall_case: "G+EX",
                    load_terms: Some(&terms),
                    src_wall_gravity_forces: Some(&gravity.member_forces),
                    src_holding_reviewed: reviewed,
                    ..Default::default()
                },
            )
            .unwrap()
            .wall_checks
        };
        let unchecked_holding = report(&model, &combined.member_forces, false);
        let checked_holding = report(&model, &combined.member_forces, true);
        assert_eq!(unchecked_holding.len(), 5);
        let ratio = |report: &[sepika_design_jp::wall_check::WallCheck], kind| {
            let row = report.iter().find(|row| row.kind == kind).unwrap();
            let CheckOutcome::Checked(result) = &row.outcome else {
                panic!("{row:?}");
            };
            assert!(result.ok(), "{result:?}");
            result.ratio()
        };
        let qd_before = ratio(&unchecked_holding, WallCheckKind::AllowableShear);
        let qd_after = ratio(&checked_holding, WallCheckKind::AllowableShear);
        assert!(
            qd_after > 0.0 && (qd_before / qd_after - 1.5).abs() < 1e-12,
            "Qd ratio before={qd_before}, after={qd_after}; {unchecked_holding:?}; {checked_holding:?}"
        );
        assert!(ratio(&checked_holding, WallCheckKind::AllowableFlexure) > 0.0);
        assert!(ratio(&checked_holding, WallCheckKind::UltimateShear) > 0.0);
        assert!(ratio(&checked_holding, WallCheckKind::UltimateFlexure) > 0.0);

        let mut low_shear = combined.member_forces.clone();
        let wall = low_shear
            .iter_mut()
            .find(|(id, _)| *id == ElemId(4))
            .unwrap();
        for (_, f) in &mut wall.1.at {
            f[1] *= 0.01;
        }
        let partial = report(&model, &low_shear, true);
        let shear = partial
            .iter()
            .find(|check| check.kind == WallCheckKind::UltimateShear)
            .unwrap();
        assert!(
            matches!(&shear.outcome, CheckOutcome::Skipped { reason } if reason.contains("適用範囲1〜3外"))
        );
        assert_eq!(
            partial
                .iter()
                .filter(|check| check.kind == WallCheckKind::UltimateFlexure
                    && matches!(check.outcome, CheckOutcome::Checked(_)))
                .count(),
            2
        );

        let mut high_axial = combined.member_forces.clone();
        let wall = high_axial
            .iter_mut()
            .find(|(id, _)| *id == ElemId(4))
            .unwrap();
        wall.1.at.iter_mut().find(|(x, _)| *x == 0.0).unwrap().1[0] = -1e9;
        let partial = report(&model, &high_axial, true);
        let flexure: Vec<_> = partial
            .iter()
            .filter(|check| check.kind == WallCheckKind::UltimateFlexure)
            .collect();
        assert!(
            matches!(&flexure[0].outcome, CheckOutcome::Skipped { reason } if reason.contains("壁脚") && reason.contains("全塑性断面の範囲外"))
        );
        assert!(matches!(flexure[1].outcome, CheckOutcome::Checked(_)));
        assert!(ratio(&partial, WallCheckKind::UltimateShear) > 0.0);

        if embedded {
            model.wall_plates[0]
                .src_steel_plate
                .as_mut()
                .unwrap()
                .force_transfer_confirmed = false;
        } else {
            model.wall_plates[0].src_wall_detailing_confirmed = false;
        }
        let unconfirmed = report(&model, &combined.member_forces, true);
        assert_eq!(unconfirmed.len(), 5);
        assert!(unconfirmed.iter().all(|row| matches!(&row.outcome, CheckOutcome::Skipped { reason } if reason.contains("未確認"))));
    }
}

#[test]
fn wall_materials_reject_incompatible_steel_products_and_round_wall_rebar() {
    use sepika_core::ids::MaterialId;
    let capacity = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
    };

    let mut column_model = src_wall_model::input_model(false);
    let mut beam_steel = column_model.materials[3].clone();
    beam_steel.id = MaterialId(4);
    column_model.materials.push(beam_steel);
    column_model.sections[1].steel_material = Some(MaterialId(4));
    column_model.materials[3].name = "STK400".into();
    let error = capacity(&column_model).unwrap_err();
    assert!(error.contains("側柱") && error.contains("鋼管用"));
    column_model.materials[3].name = "SN400C".into();
    assert!(capacity(&column_model).unwrap_err().contains("16 mm 以上"));
    column_model.materials[3].name = "SN400B".into();
    let Some(SectionShape::SrcColumnRect { steel_width, .. }) = &mut column_model.sections[0].shape
    else {
        unreachable!()
    };
    *steel_width = 560.0;
    assert!(capacity(&column_model)
        .unwrap_err()
        .contains("幅厚比の適用条件"));

    let mut plate_model = src_wall_model::input_model(true);
    let mut material = plate_model.materials[3].clone();
    material.id = MaterialId(4);
    material.name = "STKR400".into();
    plate_model.materials.push(material);
    plate_model.wall_plates[0]
        .src_steel_plate
        .as_mut()
        .unwrap()
        .material = MaterialId(4);
    assert!(capacity(&plate_model)
        .unwrap_err()
        .contains("内蔵鋼板に適用できません"));
    plate_model.materials[4].name = "SNR400B".into();
    assert!(capacity(&plate_model)
        .unwrap_err()
        .contains("内蔵鋼板に適用できません"));
    plate_model.materials[4].name = "壁用直接入力鋼材".into();
    assert!(capacity(&plate_model).unwrap().is_some());

    let mut rebar_model = src_wall_model::input_model(false);
    let mut material = rebar_model.materials[2].clone();
    material.id = MaterialId(4);
    material.name = "SR295".into();
    rebar_model.materials.push(material);
    rebar_model.sections[2].shear_rebar_material = Some(MaterialId(4));
    assert!(capacity(&rebar_model)
        .unwrap_err()
        .contains("壁横筋の材料「SR295」は丸鋼"));
    rebar_model.sections[2].shear_rebar_material = Some(MaterialId(2));
    rebar_model.sections[2].rebar_material = Some(MaterialId(4));
    assert!(capacity(&rebar_model)
        .unwrap_err()
        .contains("壁縦筋の材料「SR295」は丸鋼"));
    rebar_model.sections[2].rebar_material = None;
    assert!(capacity(&rebar_model)
        .unwrap_err()
        .contains("壁縦筋材料が未割当"));
}

#[test]
fn embedded_plate_checks_cover_and_product_thickness_at_the_boundaries() {
    use sepika_core::ids::MaterialId;
    let mut model = src_wall_model::input_model(true);
    let mut plate_material = model.materials[3].clone();
    plate_material.id = MaterialId(4);
    model.materials.push(plate_material);
    model.wall_plates[0]
        .src_steel_plate
        .as_mut()
        .unwrap()
        .material = MaterialId(4);
    let mut check = |grade: &str, thickness_mm, wall_thickness_mm| {
        model.materials[4].name = grade.into();
        let Some(SectionShape::RcWall { thickness, .. }) = &mut model.sections[2].shape else {
            panic!("壁の断面形状がありません");
        };
        *thickness = wall_thickness_mm;
        model.wall_plates[0]
            .src_steel_plate
            .as_mut()
            .unwrap()
            .thickness_mm = thickness_mm;
        let (expanded, _, _) = expand_wall_elements(&model);
        resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
    };
    assert!(check("SN400B", 6.0, 180.0).is_ok());
    assert!(check("SN400B", 5.9, 180.0)
        .unwrap_err()
        .contains("6 mm 以上"));
    assert!(check("SN400C", 16.0, 180.0).is_ok());
    assert!(check("SN400C", 15.9, 180.0)
        .unwrap_err()
        .contains("16 mm 以上"));
    assert!(check("SN400B", 80.0, 180.0).is_ok());
    assert!(check("SN400B", 80.1, 180.0)
        .unwrap_err()
        .contains("50 mm以上"));
    assert!(check("SS490", 12.0, 180.0).unwrap_err().contains("溶接"));
    assert!(check("SS540", 12.0, 180.0).unwrap_err().contains("溶接"));
    for grade in ["SSC400", "SWH400"] {
        assert!(check(grade, 12.0, 180.0)
            .unwrap_err()
            .contains("内蔵鋼板に適用できません"));
    }
    for grade in ["SM400", "SM490", "SM490Y", "SM520", "SMA490B"] {
        assert!(check(grade, 12.0, 180.0)
            .unwrap_err()
            .contains("種類記号が不完全"));
    }
    for grade in ["SMA400", "SMA400AW", "SMA490", "SMA490BP"] {
        assert!(check(grade, 40.0, 180.0).is_ok());
        assert!(check(grade, 100.0, 220.0).is_ok());
        assert!(check(grade, 100.001, 220.0)
            .unwrap_err()
            .contains("適用範囲"));
    }
    for grade in ["SCW410-CF", "SCW480-CF", "SCW490-CF", "SCW520-CF"] {
        assert!(check(grade, 12.0, 180.0)
            .unwrap_err()
            .contains("内蔵鋼板に適用できません"));
    }
    assert!(check("SM490A", 12.0, 180.0).is_ok());
    assert!(check("SM490YA", 12.0, 180.0).is_ok());
    assert!(check("壁用直接入力鋼材", 5.9, 180.0).is_ok());
}

#[test]
fn higher_fy_for_a_known_jis_main_bar_does_not_increase_wall_resistance() {
    use sepika_core::ids::MaterialId;
    let mut model = src_wall_model::input_model(false);
    let end = SrcWallEndForces {
        moment_nmm: 4e9,
        ..Default::default()
    };
    let results = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        let cap = resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
            .unwrap()
            .unwrap();
        (
            cap.allowable_bending_ratio(end, [0.0; 2], false, false)
                .unwrap(),
            cap.ultimate_moment_nmm(end),
        )
    };
    let (ratio, mu) = results(&model);
    assert!(mu.is_ok());
    let mut column_rebar = model.materials[1].clone();
    column_rebar.id = MaterialId(4);
    column_rebar.fy = Some(685.0);
    model.materials.push(column_rebar);
    model.sections[0].rebar_material = Some(MaterialId(4));
    let (higher_fy_ratio, higher_fy_mu) = results(&model);
    assert!(ratio > 0.0 && ratio < 1.0);
    assert_eq!(higher_fy_ratio, ratio);
    let error = higher_fy_mu.unwrap_err();
    assert!(error.contains("側柱") && error.contains("Fy = 345"));
    model.sections[0].rebar_material = Some(MaterialId(1));
    model.sections[2].rebar_material = Some(MaterialId(4));
    let (wall_bar_ratio, wall_bar_mu) = results(&model);
    assert_eq!(wall_bar_ratio, ratio);
    let error = wall_bar_mu.unwrap_err();
    assert!(error.contains("壁縦筋") && error.contains("Fy = 345"));

    model.materials[4].name = "USD685".into();
    model.materials[4].strength_factor = Some(1.0);
    assert!(results(&model).1.is_ok());
}

#[test]
fn article_38_caps_horizontal_wall_rebar_strength_at_grade_value() {
    use sepika_core::ids::MaterialId;
    let mut model = src_wall_model::input_model(false);
    let shear = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        let cap = resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
            .unwrap()
            .unwrap();
        cap.ultimate_shear_n(SrcWallEndForces {
            moment_nmm: 600e6,
            shear_n: 100e3,
            ..Default::default()
        })
        .unwrap()
    };
    let nominal = shear(&model);
    let mut rebar = model.materials[2].clone();
    rebar.id = MaterialId(4);
    rebar.fy = Some(685.0);
    model.materials.push(rebar);
    model.sections[2].shear_rebar_material = Some(MaterialId(4));
    assert_eq!(shear(&model), nominal);
}

#[test]
fn wall_strength_validates_src_frame_steel_and_rebar_geometry() {
    use sepika_core::ids::MaterialId;
    use sepika_core::section_shape::RebarSize;
    let reason = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap()).unwrap_err()
    };
    let mut model = src_wall_model::input_model(false);
    let Some(SectionShape::SrcBeamRect { steel_height, .. }) = &mut model.sections[1].shape else {
        unreachable!()
    };
    *steel_height = 620.0;
    let beam_err = reason(&model);
    assert!(beam_err.contains("周辺梁") && beam_err.contains("SRC断面・配筋"));

    let mut model = src_wall_model::input_model(false);
    let Some(SectionShape::SrcColumnRect { steel_height, .. }) = &mut model.sections[0].shape
    else {
        unreachable!()
    };
    *steel_height = 510.0;
    let column_err = reason(&model);
    assert!(column_err.contains("側柱") && column_err.contains("SRC断面・配筋"));

    let mut model = src_wall_model::input_model(false);
    let Some(SectionShape::SrcColumnRect { rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!()
    };
    rebar.main_dia = RebarSize::DiameterMm(12.0);
    assert!(reason(&model).contains("D13〜D41"));
    let mut material = model.materials[1].clone();
    material.id = MaterialId(4);
    material.name = "SR295".into();
    model.materials.push(material);
    model.sections[0].rebar_material = Some(MaterialId(4));
    assert!(reason(&model).contains("異形主筋に対応する材種"));
}

#[test]
fn src_wall_rejects_incompatible_surrounding_beam_steel_and_frame_section_losses() {
    use sepika_core::ids::{MaterialId, SectionId};
    let capacity = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
    };
    for beam_id in [ElemId(2), ElemId(3)] {
        let mut model = src_wall_model::input_model(false);
        let mut material = model.materials[3].clone();
        material.id = MaterialId(4);
        material.name = "STK400".into();
        model.materials.push(material);
        let mut beam = model.sections[1].clone();
        beam.id = SectionId(3);
        beam.steel_material = Some(MaterialId(4));
        model.sections.push(beam);
        model.elements[beam_id.index()].section = Some(SectionId(3));
        let location = format!("周辺梁 ID {}", beam_id.0);
        let error = capacity(&model).unwrap_err();
        assert!(error.contains(&location) && error.contains("鋼管用"));
        model.materials[4].name = "SN400B".into();
        let Some(SectionShape::SrcBeamRect {
            steel_web_thick, ..
        }) = &mut model.sections[3].shape
        else {
            unreachable!()
        };
        *steel_web_thick = 5.0;
        let error = capacity(&model).unwrap_err();
        assert!(error.contains(&location) && error.contains("6 mm 以上"));
    }
    for (member, location) in [
        (ElemId(0), "側柱"),
        (ElemId(2), "周辺梁"),
        (ElemId(3), "周辺梁"),
    ] {
        for loss in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] {
            let mut model = src_wall_model::input_model(false);
            model.steel_design_attrs.push(SteelDesignAttr {
                elem: member,
                joint_flange_loss: loss[0],
                joint_web_loss: loss[1],
                scallop_web_loss: loss[2],
                lb_direct: None,
                lateral_brace_count: None,
                lk_y_direct: None,
                lk_z_direct: None,
                c_direct: None,
                column_lateral_buckling_end_supports: None,
            });
            let error = capacity(&model).unwrap_err();
            assert!(error.contains(location) && error.contains("欠損"));
        }
    }
}

#[test]
fn src_wall_requires_compatible_concrete_and_rebar_in_each_surrounding_beam() {
    use sepika_core::ids::{MaterialId, SectionId};
    for beam_id in [ElemId(2), ElemId(3)] {
        let mut model = src_wall_model::input_model(false);
        let mut section = model.sections[1].clone();
        section.id = SectionId(3);
        model.sections.push(section.clone());
        model.elements[beam_id.index()].section = Some(SectionId(3));
        let mut no_fc = model.materials[0].clone();
        no_fc.id = MaterialId(4);
        no_fc.fc = None;
        model.materials.push(no_fc);
        let mut round_bar = model.materials[1].clone();
        round_bar.id = MaterialId(5);
        round_bar.name = "SR295".into();
        round_bar.fy = Some(295.0);
        model.materials.push(round_bar);
        let mut high_strength = model.materials[1].clone();
        high_strength.id = MaterialId(6);
        high_strength.name = "USD685".into();
        high_strength.fy = Some(685.0);
        model.materials.push(high_strength);
        for (mat, main, hoop, message) in [
            (
                Some(MaterialId(3)),
                section.rebar_material,
                section.shear_rebar_material,
                "コンクリート材料",
            ),
            (
                Some(MaterialId(4)),
                section.rebar_material,
                section.shear_rebar_material,
                "コンクリートのFc",
            ),
            (
                section.material,
                None,
                section.shear_rebar_material,
                "主筋材料が未割当",
            ),
            (
                section.material,
                Some(MaterialId(5)),
                section.shear_rebar_material,
                "異形主筋に対応する材種",
            ),
            (
                section.material,
                section.rebar_material,
                None,
                "せん断補強筋材料が未割当",
            ),
            (
                section.material,
                section.rebar_material,
                Some(MaterialId(6)),
                "せん断補強筋グレード",
            ),
        ] {
            model.sections[3].material = mat;
            model.sections[3].rebar_material = main;
            model.sections[3].shear_rebar_material = hoop;
            let (expanded, _, _) = expand_wall_elements(&model);
            let error =
                resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap()).unwrap_err();
            assert!(
                error.contains(&format!("周辺梁 ID {}", beam_id.0)) && error.contains(message),
                "{error}"
            );
        }
    }
}

#[test]
fn src_wall_checks_surrounding_stirrups_alignment_and_steel_cover() {
    use sepika_core::section_shape::RebarSize;
    let capacity = |model: &sepika_core::Model| {
        let (expanded, _, _) = expand_wall_elements(model);
        resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
    };
    for (legs, pitch, diameter) in [(0, 100.0, 10.0), (2, 0.0, 10.0), (2, 100.0, 0.0)] {
        let mut model = src_wall_model::input_model(false);
        let Some(SectionShape::SrcBeamRect { rebar, .. }) = &mut model.sections[1].shape else {
            unreachable!()
        };
        rebar.stirrup.legs = legs;
        rebar.stirrup.pitch = pitch;
        rebar.stirrup.dia = RebarSize::DiameterMm(diameter);
        assert!(capacity(&model)
            .unwrap_err()
            .contains("あばら筋の脚数・断面積・間隔"));
    }
    let mut model = src_wall_model::input_model(false);
    model.elements[2].end_offsets_mm = [[0.0, 500.0, 0.0]; 2];
    assert!(capacity(&model)
        .unwrap_err()
        .contains("周辺梁 ID 2: 壁と整合する材軸"));

    for (section, location) in [(0, "側柱"), (1, "周辺梁")] {
        for (height, is_valid) in [(500.0, true), (500.1, false)] {
            let mut model = src_wall_model::input_model(false);
            let (steel_height, steel_width) = match model.sections[section].shape.as_mut().unwrap()
            {
                SectionShape::SrcColumnRect {
                    steel_height,
                    steel_width,
                    ..
                }
                | SectionShape::SrcBeamRect {
                    steel_height,
                    steel_width,
                    ..
                } => (steel_height, steel_width),
                _ => unreachable!(),
            };
            *steel_width = 100.0;
            *steel_height = height;
            let result = capacity(&model);
            if is_valid {
                assert!(result.is_ok(), "{result:?}");
            } else {
                let error = result.unwrap_err();
                assert!(
                    error.contains(location) && error.contains("各面50 mm以上"),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn wall_weight_replaces_clear_panel_concrete_volume_with_plate_steel() {
    let ordinary = src_wall_model::input_model(false);
    let embedded = src_wall_model::input_model(true);
    let a = ordinary.wall_weight(&ordinary.wall_plates[0]).unwrap();
    let b = embedded.wall_weight(&embedded.wall_plates[0]).unwrap();
    let area_mm2 = 3_400.0 * 2_400.0;
    assert_eq!(b.net_area_mm2, area_mm2);
    assert!((b.design_body_n - a.design_body_n - area_mm2 * 12.0 * 54.96404e-6).abs() < 1e-7);
    let physical_plate_n = area_mm2 * 12.0 * 7.85e-9 * sepika_core::units::GRAVITY_MM_S2;
    let concrete_displaced_n = a.physical_body_n / 15.0;
    assert!(
        (b.totals.matrix_n - a.totals.matrix_n - physical_plate_n + concrete_displaced_n).abs()
            < 1e-7
    );
}

#[test]
fn article_25_shear_uses_the_corresponding_tie_legs_for_each_column_axis() {
    let mut model = src_wall_model::input_model(false);
    let Some(SectionShape::SrcColumnRect { b, d, rebar, .. }) = &mut model.sections[0].shape else {
        unreachable!()
    };
    *b = 550.0;
    *d = 650.0;
    rebar.hoop.legs_x = 2;
    rebar.hoop.legs_y = 4;
    for (axis, expected_n) in [
        ([1.0, 0.0, 0.0], 2_480_661.7978961924),
        ([0.0, 1.0, 0.0], 2_694_192.8396090437),
    ] {
        for column in &mut model.elements[..2] {
            column.local_axis.ref_vector = axis;
        }
        let (expanded, _, _) = expand_wall_elements(&model);
        let section = resolve_src_wall(&expanded, expanded.element(ElemId(4)).unwrap())
            .unwrap()
            .unwrap();
        assert!((section.allowable_shear_n(false) - expected_n).abs() < 1e-6);
    }
}

#[test]
fn article_25_column_flexure_defaults_each_buckling_length_to_one_point_two_times_node_span() {
    let (mut model, _, _) = expand_wall_elements(&src_wall_model::analysis_input_model(false));
    let ratio = |model: &sepika_core::Model| {
        resolve_src_wall(model, model.element(ElemId(4)).unwrap())
            .unwrap()
            .unwrap()
            .allowable_bending_ratio(
                SrcWallEndForces {
                    axial_n: 800_000.0,
                    ..Default::default()
                },
                [400_000.0; 2],
                true,
                false,
            )
    };
    assert!(ratio(&model).unwrap_err().contains("短柱条件"));
    model.steel_design_attrs = [ElemId(0), ElemId(1)]
        .map(|elem| SteelDesignAttr {
            elem,
            joint_flange_loss: 0.0,
            joint_web_loss: 0.0,
            scallop_web_loss: 0.0,
            lb_direct: None,
            lateral_brace_count: None,
            lk_y_direct: Some(7_000.0),
            lk_z_direct: None,
            c_direct: None,
            column_lateral_buckling_end_supports: None,
        })
        .to_vec();
    assert!(ratio(&model).unwrap_err().contains("短柱条件"));
    for attr in &mut model.steel_design_attrs {
        attr.lk_z_direct = Some(7_200.0);
    }
    assert!(ratio(&model).is_ok_and(|r| r > 0.0 && r < 1.0));
}
