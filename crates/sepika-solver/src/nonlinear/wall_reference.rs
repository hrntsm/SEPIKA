//! 同じ展開済みrun入力からRC壁の初期Quを解決する静的一次参照。

use crate::statics::analysis::{
    build_seismic_load_case_from_model, building_height_mm, steel_height_ratio, Analysis,
    SeismicCfg, SeismicDir,
};
use sepika_core::ids::{ElemId, LoadCaseId};
use sepika_core::model::{ElementKind, LoadCase, LoadCaseKind, Model};
use sepika_element::factory::StrengthBasis;
use sepika_element::wall::static_face::StaticWallFace;
use sepika_element::wall::wall_element::WallElement;

/// 対象壁がなければ静的求解・Ai生成を行わない。全壁の正負が成功した場合だけ値を返す。
pub(crate) fn resolve(
    model: &Model,
    apply_long_term: bool,
    basis: StrengthBasis,
) -> Result<Vec<(ElemId, [f64; 2])>, String> {
    resolve_inputs(model, apply_long_term, basis).map_err(|reason| {
        format!(
            "RC壁 {}: {reason}",
            model.stress_cfg.rc_wall_shear_method.label()
        )
    })
}

pub(crate) fn resolve_with_gravity_terms(
    model: &Model,
    terms: &[(LoadCaseId, f64)],
    basis: StrengthBasis,
) -> Result<Vec<(ElemId, [f64; 2])>, String> {
    use sepika_core::model::MemberLoadKind;
    let mut work = model.clone();
    work.load_cases
        .retain(|case| terms.iter().any(|(id, _)| *id == case.id) || !case.kind.is_long_term());
    for case in &mut work.load_cases {
        let Some((_, factor)) = terms.iter().find(|(id, _)| *id == case.id) else {
            continue;
        };
        for load in &mut case.nodal {
            load.values = load.values.map(|value| value * factor);
        }
        for load in &mut case.member {
            match &mut load.kind {
                MemberLoadKind::Point { p, .. } => *p *= factor,
                MemberLoadKind::Distributed { w1, w2, .. } => {
                    *w1 *= factor;
                    *w2 *= factor;
                }
            }
        }
    }
    resolve(&work, !terms.is_empty(), basis)
}

fn resolve_inputs(
    model: &Model,
    apply_long_term: bool,
    basis: StrengthBasis,
) -> Result<Vec<(ElemId, [f64; 2])>, String> {
    let walls: Vec<_> = model
        .elements
        .iter()
        .filter(|w| {
            w.kind == ElementKind::Wall
                && sepika_element::wall::misc_wall::is_rc_wall(w, model)
                && sepika_element::wall::misc_wall::wall_is_seismic(w, model)
        })
        .collect();
    if walls.is_empty() {
        return Ok(Vec::new());
    }
    let faces: Vec<_> = walls
        .iter()
        .map(|w| StaticWallFace::new(w, model))
        .collect::<Result<_, _>>()?;
    let mut gravity = LoadCase {
        id: LoadCaseId(0),
        name: "RC壁一次参照G".into(),
        kind: LoadCaseKind::Dead,
        nodal: Vec::new(),
        member: Vec::new(),
    };
    if apply_long_term {
        for case in model.load_cases.iter().filter(|c| c.kind.is_long_term()) {
            gravity.nodal.extend(case.nodal.clone());
            gravity.member.extend(case.member.clone());
        }
    }
    for face in &faces {
        face.ensure_loads(std::slice::from_ref(&gravity), true)?;
    }
    // Wallへの直接荷重は既存組立の対象外なので、後続run荷重でも読み飛ばしを許さない。
    for w in &walls {
        if model
            .load_cases
            .iter()
            .flat_map(|c| &c.member)
            .any(|m| m.elem == w.id)
        {
            return Err(format!(
                "壁 ID {}: 生成Wallへの直接部材荷重は未対応です",
                w.id.0
            ));
        }
    }
    let t = sepika_load::ai::approx_t(
        building_height_mm(model) / 1000.0,
        steel_height_ratio(model),
    );
    let ex = build_seismic_load_case_from_model(
        model,
        SeismicCfg {
            dir: SeismicDir::X,
            ..Default::default()
        },
        t,
    )
    .map_err(|e| format!("RC壁一次参照EX: {e}"))?;
    let ey = build_seismic_load_case_from_model(
        model,
        SeismicCfg {
            dir: SeismicDir::Y,
            ..Default::default()
        },
        t,
    )
    .map_err(|e| format!("RC壁一次参照EY: {e}"))?;
    let mut reference_model = model.clone();
    reference_model.combinations.clear();
    reference_model.load_cases = vec![gravity];
    for w in &walls {
        let g = sepika_core::model::wall_element_geometry(w, model).ok_or("壁方向が不明です")?;
        for sign in [1.0, -1.0] {
            let mut case = LoadCase {
                id: LoadCaseId(reference_model.load_cases.len() as u32),
                name: format!("壁{}一次参照E{sign}", w.id.0),
                kind: LoadCaseKind::Seismic,
                nodal: Vec::new(),
                member: Vec::new(),
            };
            for (base, factor) in [(&ex, sign * g.ex_bottom[0]), (&ey, sign * g.ex_bottom[1])] {
                for load in &base.nodal {
                    let mut load = load.clone();
                    load.values = load.values.map(|v| v * factor);
                    case.nodal.push(load);
                }
            }
            reference_model.load_cases.push(case);
        }
    }
    let analysis = Analysis::prepare(&reference_model).map_err(|e| format!("RC壁一次参照: {e}"))?;
    let has_gravity = !reference_model.load_cases[0].nodal.is_empty()
        || !reference_model.load_cases[0].member.is_empty();
    let gravity_result = has_gravity
        .then(|| analysis.linear_static(LoadCaseId(0)))
        .transpose()
        .map_err(|e| format!("RC壁一次参照G: {e}"))?;
    let gravity_batch: Vec<_> = gravity_result
        .as_ref()
        .map(|r| {
            r.member_forces
                .iter()
                .map(|(id, f)| (*id, f.at.as_slice()))
                .collect()
        })
        .unwrap_or_default();
    let mut capacities = Vec::new();
    for (i, (w, face)) in walls.iter().zip(&faces).enumerate() {
        let ng = if has_gravity {
            face.recover(&gravity_batch)?.compression_n
        } else {
            0.0
        };
        let mut ratios = [None; 2];
        for sign in 0..2 {
            let result = analysis
                .linear_static(LoadCaseId((1 + 2 * i + sign) as u32))
                .map_err(|e| format!("壁 ID {} 参照分岐 {sign}: {e}", w.id.0))?;
            let batch: Vec<_> = result
                .member_forces
                .iter()
                .map(|(id, f)| (*id, f.at.as_slice()))
                .collect();
            let f = face.recover(&batch)?;
            if f.shear_n == 0.0 {
                return Err(format!(
                    "壁 ID {} 参照分岐 {sign}: Q=0でM/(QD)が未解決です",
                    w.id.0
                ));
            }
            // 回収Qyは下側節点力。ばねの上辺伝達力Qwとは逆符号である。
            let branch = usize::from(f.shear_n > 0.0);
            let ratio = f.moment_nmm.abs() / (f.shear_n.abs() * f.depth_mm);
            if !ratio.is_finite() || ratios[branch].replace(ratio).is_some() {
                return Err(format!(
                    "壁 ID {} 局所分岐 {branch}: 参照方向が未解決・重複・非有限です",
                    w.id.0
                ));
            }
        }
        let ratios = [
            ratios[0].ok_or_else(|| format!("壁 ID {} 正局所分岐が不足しています", w.id.0))?,
            ratios[1].ok_or_else(|| format!("壁 ID {} 負局所分岐が不足しています", w.id.0))?,
        ];
        let qu = WallElement::directional_shear_capacity_at_reference(w, model, basis, ng, ratios)?;
        if qu.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(format!(
                "壁 ID {}: 全局所分岐のQuは有限正値が必要です",
                w.id.0
            ));
        }
        capacities.push((w.id, qu));
    }
    Ok(capacities)
}

#[cfg(test)]
#[path = "../../../../test_fixtures/rc_wall617.rs"]
mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::model::{MemberLoad, MemberLoadKind, NodalLoad};
    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1e-8 * expected.abs().max(1.0),
            "actual={actual} expected={expected}"
        );
    }
    fn direct_state(
        model: &Model,
        case: LoadCase,
    ) -> sepika_element::wall::static_face::WallFaceForces {
        let mut m = model.clone();
        m.load_cases = vec![LoadCase {
            id: LoadCaseId(0),
            ..case
        }];
        m.combinations.clear();
        let r = Analysis::prepare(&m)
            .unwrap()
            .linear_static(LoadCaseId(0))
            .unwrap();
        let batch: Vec<_> = r
            .member_forces
            .iter()
            .map(|(id, f)| (*id, f.at.as_slice()))
            .collect();
        StaticWallFace::new(&m.elements[4], &m)
            .unwrap()
            .recover(&batch)
            .unwrap()
    }
    #[test]
    fn actual_static_cut_equilibrium_and_reference_capacity_match_independent_values() {
        let m = fixture::expanded_model();
        let g = direct_state(&m, m.load_cases[0].clone());
        close(g.compression_n, 800_000.0);
        let lateral = LoadCase {
            id: LoadCaseId(0),
            name: "E".into(),
            kind: LoadCaseKind::Seismic,
            nodal: vec![
                NodalLoad::manual(
                    sepika_core::ids::NodeId(2),
                    [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
                NodalLoad::manual(
                    sepika_core::ids::NodeId(3),
                    [100_000.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ),
            ],
            member: Vec::new(),
        };
        let e = direct_state(&m, lateral);
        close(e.shear_n.abs(), 200_000.0);
        close(e.moment_nmm.abs(), 540_000_000.0);
        close(e.compression_n, 0.0);
        let q = resolve(&m, true, StrengthBasis::Nominal).unwrap();
        let explicit = WallElement::directional_shear_capacity_at_reference(
            &m.elements[4],
            &m,
            StrengthBasis::Nominal,
            800_000.0,
            [2700.0 / 4600.0; 2],
        )
        .unwrap();
        close(q[0].1[0], explicit[0]);
        close(q[0].1[1], explicit[1]);
    }
    #[test]
    fn off_empty_gravity_and_distributed_axial_gravity_keep_face_values_and_input_unchanged() {
        let mut m = fixture::expanded_model();
        let before = serde_json::to_vec(&m).unwrap();
        let off = resolve(&m, false, StrengthBasis::Nominal).unwrap();
        assert_eq!(serde_json::to_vec(&m).unwrap(), before);
        m.load_cases.clear();
        assert_eq!(resolve(&m, true, StrengthBasis::Nominal).unwrap(), off);
        m.load_cases = vec![LoadCase {
            id: LoadCaseId(0),
            name: "axial".into(),
            kind: LoadCaseKind::Dead,
            nodal: Vec::new(),
            member: vec![MemberLoad::manual(
                ElemId(0),
                [0.0, 0.0, -1.0],
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 3000.0,
                    w1: 100.0,
                    w2: 100.0,
                },
            )],
        }];
        let g = direct_state(&m, m.load_cases[0].clone());
        close(g.compression_n, 270_000.0);
        let q = resolve(&m, true, StrengthBasis::Nominal).unwrap();
        assert!(q[0].1[0] > off[0].1[0]);
    }
    #[test]
    fn rotated_reference_does_not_follow_push_direction_and_failure_is_atomic() {
        let original = fixture::expanded_model();
        let expected = resolve(&original, true, StrengthBasis::Nominal).unwrap();
        for angle in [std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_4] {
            let mut m = original.clone();
            let rotate = |[x, y, z]: [f64; 3]| {
                [
                    x * angle.cos() - y * angle.sin(),
                    x * angle.sin() + y * angle.cos(),
                    z,
                ]
            };
            for n in &mut m.nodes {
                n.coord = rotate(n.coord);
            }
            for e in &mut m.elements {
                e.local_axis.ref_vector = rotate(e.local_axis.ref_vector);
            }
            let actual = resolve(&m, true, StrengthBasis::Nominal).unwrap();
            close(actual[0].1[0], expected[0].1[0]);
            close(actual[0].1[1], expected[0].1[1]);
        }
        let mut bad = original.clone();
        bad.elements[1].kind = ElementKind::Fiber;
        assert!(resolve(&bad, true, StrengthBasis::Nominal)
            .unwrap_err()
            .contains("側柱 ID 1"));
        bad = original.clone();
        bad.stories[1].seismic_weight = None;
        assert!(resolve(&bad, true, StrengthBasis::Nominal).is_err());
        bad.elements.pop();
        assert!(resolve(&bad, true, StrengthBasis::Nominal)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn active_set_gravity_sum_and_both_lateral_signs_match_direct_load_solutions() {
        use sepika_core::ids::{MaterialId, NodeId, SectionId};
        use sepika_core::model::{FrameSectionUse, MaterialCategory, Node};
        let mut m = fixture::expanded_model();
        m.stress_cfg.tension_only_iteration = true;
        for node in &mut m.nodes[2..4] {
            node.coord[2] = 6000.0;
        }
        m.stories[1].elevation = 6000.0;
        let mut steel = m.materials[1].clone();
        steel.id = MaterialId(3);
        steel.category = MaterialCategory::Steel;
        m.materials.push(steel);
        let mut section = sepika_core::section_shape::SectionShape::SteelH {
            height: 400.0,
            width: 200.0,
            web_thick: 9.0,
            flange_thick: 12.0,
            root_r: Some(0.0),
        }
        .input_section(SectionId(3), "引張専用".into())
        .unwrap();
        section.material = Some(MaterialId(3));
        section.frame_use = Some(FrameSectionUse::Brace);
        m.sections.push(section);
        m.nodes.push(Node {
            id: NodeId(4),
            coord: [8000.0, 0.0, 0.0],
            restraint: sepika_core::dof::Dof6Mask::FIXED,
            mass: None,
            story: Some(sepika_core::ids::StoryId(0)),
            support_spring: None,
        });
        let mut brace = m.elements[0].clone();
        brace.id = ElemId(5);
        brace.nodes = [NodeId(4), NodeId(3)].into_iter().collect();
        brace.kind = ElementKind::Brace { tension_only: true };
        brace.section = Some(SectionId(3));
        brace.rigid_zone = Default::default();
        m.elements.push(brace);
        let mut second = m.load_cases[0].clone();
        second.id = LoadCaseId(1);
        second.kind = LoadCaseKind::Live;
        second.nodal[0].values[2] = 200_000.0;
        second.nodal[1].values[2] = -100_000.0;
        m.load_cases.push(second);
        let mut g = m.load_cases[0].clone();
        g.nodal.extend(m.load_cases[1].nodal.clone());
        let ng = direct_state(&m, g).compression_n;
        let t = sepika_load::ai::approx_t(building_height_mm(&m) / 1000.0, steel_height_ratio(&m));
        let e = build_seismic_load_case_from_model(&m, SeismicCfg::default(), t).unwrap();
        let positive = direct_state(&m, e.clone());
        let mut negative = e;
        for load in &mut negative.nodal {
            load.values = load.values.map(|v| -v);
        }
        let negative = direct_state(&m, negative);
        assert!(positive.shear_n * negative.shear_n < 0.0);
        let plus_ratio = positive.moment_nmm.abs() / (positive.shear_n.abs() * positive.depth_mm);
        let minus_ratio = negative.moment_nmm.abs() / (negative.shear_n.abs() * negative.depth_mm);
        assert!(
            (plus_ratio - minus_ratio).abs() > 1e-6,
            "正負の直接解が同一になっています"
        );
        assert!(positive.shear_n < 0.0 && negative.shear_n > 0.0);
        let ratios = [plus_ratio, minus_ratio];
        let expected = WallElement::directional_shear_capacity_at_reference(
            &m.elements[4],
            &m,
            StrengthBasis::Nominal,
            ng,
            ratios,
        )
        .unwrap();
        assert!((expected[0] - expected[1]).abs() > 1.0);
        let actual = resolve(&m, true, StrengthBasis::Nominal).unwrap();
        close(actual[0].1[0], expected[0]);
        close(actual[0].1[1], expected[1]);
    }

    #[test]
    fn direct_wall_and_horizontal_column_member_loads_are_rejected_with_ids() {
        for (elem, dir) in [(ElemId(4), [0.0, 0.0, -1.0]), (ElemId(0), [1.0, 0.0, 0.0])] {
            let mut m = fixture::expanded_model();
            m.load_cases[0].member.push(MemberLoad::manual(
                elem,
                dir,
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: 3000.0,
                    w1: 10.0,
                    w2: 10.0,
                },
            ));
            let error = resolve(&m, true, StrengthBasis::Nominal).unwrap_err();
            assert!(error.contains("壁 ID 4"), "{error}");
            assert!(error.contains(&format!("ID {}", elem.0)), "{error}");
        }
    }

    #[test]
    fn unresolved_second_wall_does_not_return_first_wall_capacity_or_mutate_model() {
        let mut m = fixture::expanded_model();
        let nodes = m.nodes.clone();
        for mut node in nodes {
            node.id.0 += 4;
            node.coord[0] += 10000.0;
            m.nodes.push(node);
        }
        let elements = m.elements.clone();
        for mut element in elements {
            element.id.0 += 5;
            for node in &mut element.nodes {
                node.0 += 4;
            }
            m.elements.push(element);
        }
        m.elements[5].rigid_zone.face_i = None;
        let before = serde_json::to_vec(&m).unwrap();
        let error = resolve(&m, true, StrengthBasis::Nominal).unwrap_err();
        assert!(error.contains("壁 ID 9"), "{error}");
        assert_eq!(serde_json::to_vec(&m).unwrap(), before);
    }
}
