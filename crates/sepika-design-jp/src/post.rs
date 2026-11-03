//! 鋼製間柱の独立した許容応力度検定。力・長さは N-mm 系。

use crate::{
    CheckKind, CheckOutcome, DesignCheck, DesignCtx, LoadTerm, MemberForcesAt, MemberKind,
};
use sepika_core::model::{
    LoadCaseKind, LoadPurpose, MemberLoadKind, Model, PostLateralSupport, SecondaryMember,
};
use sepika_load::floor::response::{beam_response, BeamResponse, Polynomial};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PostPositionResult {
    pub position: f64,
    pub forces: MemberForcesAt,
    pub check: CheckOutcome,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PostCombinationResult {
    pub name: String,
    pub short_term: bool,
    pub positions: Vec<PostPositionResult>,
    /// 鉛直反力 `[N]`、全体X/Y方向横反力 `[N]`、固定端曲げ反力 `[N mm]`。
    pub gravity_reactions: [f64; 2],
    pub lateral_reactions: [[f64; 2]; 2],
    pub support_moments: [[f64; 2]; 2],
    pub displacement_mm: [f64; 2],
    pub resultant_displacement_mm: f64,
    pub deflection_ratio: Option<f64>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PostDesignResult {
    pub support: PostLateralSupport,
    pub combinations: Vec<PostCombinationResult>,
    pub reasons: Vec<String>,
}
impl PostDesignResult {
    pub fn ratio(&self) -> f64 {
        self.combinations
            .iter()
            .flat_map(|c| {
                c.positions
                    .iter()
                    .filter_map(|p| match &p.check {
                        CheckOutcome::Checked(r) => Some(r.ratio()),
                        _ => None,
                    })
                    .chain(c.deflection_ratio)
            })
            .fold(0.0, f64::max)
    }
    pub fn complete(&self) -> bool {
        self.reasons.is_empty()&&!self.combinations.is_empty()&&self.combinations.iter().all(|c|c.reasons.is_empty()&&c.deflection_ratio.is_some()&&!c.positions.is_empty()&&c.positions.iter().all(|p|matches!(&p.check,CheckOutcome::Checked(r) if r.components.iter().any(|c|c.kind==CheckKind::AxialBending)&&r.components.iter().any(|c|c.kind==CheckKind::Shear))))
    }
}

fn scaled(load: &MemberLoadKind, s: f64) -> MemberLoadKind {
    match *load {
        MemberLoadKind::Point { a, p } => MemberLoadKind::Point { a, p: p * s },
        MemberLoadKind::Distributed { a, b, w1, w2 } => MemberLoadKind::Distributed {
            a,
            b,
            w1: w1 * s,
            w2: w2 * s,
        },
    }
}
fn interval(response: &BeamResponse, t: f64) -> &sepika_load::floor::response::ResponseInterval {
    response
        .intervals
        .iter()
        .find(|i| t >= i.range[0] - 1e-12 && t <= i.range[1] + 1e-12)
        .expect("応答区間")
}
fn signed(p: &Polynomial, t: f64) -> Polynomial {
    p.scale(if p.value(t) < 0.0 { -1.0 } else { 1.0 })
}
fn candidates(polys: &[Polynomial], a: f64, b: f64) -> Vec<f64> {
    let mut result = vec![a, b];
    for p in polys {
        result.extend(p.roots(a, b));
    }
    result.sort_by(f64::total_cmp);
    result.dedup_by(|x, y| (*x - *y).abs() < 1e-10);
    result
}

/// 保存組合せごとに、既存の鉛直荷重伝達を一度だけ取り込んで検定する。
pub fn design_post(model: &Model, sm: &SecondaryMember) -> PostDesignResult {
    let input = model.post_design_inputs.iter().find(|i| i.member == sm.id);
    let mut result = PostDesignResult {
        support: input.map(|i| i.lateral_support).unwrap_or_default(),
        combinations: Vec::new(),
        reasons: Vec::new(),
    };
    let Some(input) = input else {
        result.reasons.push("間柱検定条件が未設定です".into());
        return result;
    };
    if !model
        .wall_regions
        .iter()
        .any(|r| r.posts.iter().any(|p| p.id == sm.id))
    {
        result.reasons.push("壁領域に所属していません".into());
    }
    if sm.is_detached() || model.secondary_member_materialized(sm) {
        result
            .reasons
            .push("支持先未解決または全体解析の実部材であるため独立検定対象外です".into());
    }
    if sm.is_cantilever() && input.lateral_support != PostLateralSupport::FixedStart {
        result
            .reasons
            .push("片持ち配置の横支持は、支持アンカーがある始端固定のみ設定できます".into());
    }
    let Some((a, b, l)) = model.secondary_member_axis(sm) else {
        result.reasons.push("材軸長さが未確定です".into());
        return result;
    };
    if (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-6 {
        result.reasons.push("鉛直な間柱のみ対応しています".into());
    }
    let Some(sec) = sm.section.and_then(|id| model.section(id)) else {
        result.reasons.push("断面が未設定です".into());
        return result;
    };
    let Some(mat) = model.secondary_material(sm) else {
        result.reasons.push("材料が未設定です".into());
        return result;
    };
    let valid_shape = mat.category == sepika_core::model::MaterialCategory::Steel
        && match &sec.shape {
            Some(sepika_core::section_shape::SectionShape::SteelH { .. }) => true,
            Some(sepika_core::section_shape::SectionShape::SteelBox {
                thick,
                horizontal_thick,
                ..
            }) => thick == horizontal_thick,
            _ => false,
        };
    if !valid_shape {
        result
            .reasons
            .push("断面検定の対象は寸法が確定したH形鋼・等厚角形鋼管です".into());
    }
    let angle = input.strong_direction_deg.filter(|v| v.is_finite());
    if angle.is_none() {
        result
            .reasons
            .push("断面の強軸方向が未設定または不正です".into());
        return result;
    }
    let theta = angle.unwrap().to_radians();
    let strong = [theta.cos(), theta.sin()];
    let weak = [-strong[1], strong[0]];
    let lk = input
        .buckling_lengths_mm
        .filter(|v| v.iter().all(|v| v.is_finite() && *v > 0.0));
    let lb = input
        .lateral_buckling_length_mm
        .filter(|v| v.is_finite() && *v > 0.0);
    if lk.is_none() {
        result
            .reasons
            .push("強軸・弱軸の軸圧縮座屈長さが未設定または不正です".into());
    }
    if lb.is_none() {
        result
            .reasons
            .push("横補剛間隔が未設定または不正です".into());
    }
    if input.combinations.is_empty() {
        result
            .reasons
            .push("検定対象の荷重組合せが未設定です".into());
    }
    for &(idx, short) in &input.combinations {
        let Some(combo) = model.combinations.get(idx) else {
            result
                .reasons
                .push("検定対象の荷重組合せが存在しません".into());
            continue;
        };
        let mut c = PostCombinationResult {
            name: combo.name.clone(),
            short_term: short,
            positions: Vec::new(),
            gravity_reactions: [0.0; 2],
            lateral_reactions: [[0.0; 2]; 2],
            support_moments: [[0.0; 2]; 2],
            displacement_mm: [0.0; 2],
            resultant_displacement_mm: 0.0,
            deflection_ratio: None,
            reasons: Vec::new(),
        };
        let mut gravity = Vec::new();
        let mut lateral = [Vec::new(), Vec::new()];
        let mut has_lateral = false;
        for &(case_id, factor) in &combo.terms {
            let Some(case) = model.load_cases.iter().find(|c| c.id == case_id) else {
                c.reasons.push("組合せの荷重ケースが存在しません".into());
                continue;
            };
            if !factor.is_finite() {
                c.reasons.push("組合せ係数が不正です".into());
                continue;
            }
            if factor == 0.0 {
                continue;
            }
            if matches!(
                case.kind,
                LoadCaseKind::Dead | LoadCaseKind::Live | LoadCaseKind::LiveSeismic
            ) {
                if model
                    .load_cases
                    .iter()
                    .filter(|c| c.kind == case.kind)
                    .count()
                    != 1
                {
                    c.reasons.push(
                        "同種の鉛直荷重ケースが複数あり既存自動伝達の所属を確定できません".into(),
                    );
                    continue;
                }
                let transfer = sepika_load::cascade::solve(
                    model,
                    |s| match case.kind {
                        LoadCaseKind::Dead => model.slab_dead_intensity(s),
                        LoadCaseKind::Live => {
                            model.slab_intensity(s, LoadPurpose::Frame)
                                - model.slab_dead_intensity(s)
                        }
                        LoadCaseKind::LiveSeismic => {
                            model.slab_intensity(s, LoadPurpose::Seismic)
                                - model.slab_dead_intensity(s)
                        }
                        _ => 0.0,
                    },
                    case.kind == LoadCaseKind::Dead,
                );
                match transfer {
                    Ok(t) => {
                        if !t.cyclic.is_empty()
                            || !t.invalid_end_shares.is_empty()
                            || !t.unresolved.is_empty()
                        {
                            c.reasons.push(
                                "鉛直荷重伝達に循環・負担率未設定・支持先未解決があります".into(),
                            );
                        }
                        if let Some(entry) = t.members.get(&sm.id) {
                            gravity
                                .extend(entry.member_loads.iter().map(|load| scaled(load, factor)));
                            for k in 0..2 {
                                c.gravity_reactions[k] += entry.reactions[k] * factor;
                            }
                        } else {
                            c.reasons.push("間柱の鉛直荷重伝達が未解決です".into());
                        }
                    }
                    Err(e) => c.reasons.push(format!("鉛直荷重伝達: {e}")),
                }
            }
            for load in input
                .lateral_loads
                .iter()
                .filter(|load| load.case == case_id)
            {
                if !load.direction.iter().all(|v| v.is_finite())
                    || (load.direction[0].hypot(load.direction[1]) - 1.0).abs() > 1e-8
                {
                    c.reasons
                        .push("局部横荷重の方向は単位ベクトルが必要です".into());
                    continue;
                }
                has_lateral = true;
                for (axis, d) in [strong, weak].iter().enumerate() {
                    lateral[axis].push(scaled(
                        &load.load,
                        factor * (load.direction[0] * d[0] + load.direction[1] * d[1]),
                    ));
                }
            }
        }
        if !has_lateral {
            c.reasons
                .push("局部横荷重が未設定です。風・地震の横荷重に対して未検定です".into());
        }
        let response = (|| {
            let g = beam_response(&gravity, l, 1.0, PostLateralSupport::Pinned)?;
            let y = beam_response(&lateral[0], l, mat.young * sec.iy, input.lateral_support)?;
            let z = beam_response(&lateral[1], l, mat.young * sec.iz, input.lateral_support)?;
            Ok::<_, String>((g, y, z))
        })();
        match response {
            Err(e) => c.reasons.push(e),
            Ok((g, y, z)) => {
                for k in 0..2 {
                    c.lateral_reactions[k] = [
                        y.reactions[k] * strong[0] + z.reactions[k] * weak[0],
                        y.reactions[k] * strong[1] + z.reactions[k] * weak[1],
                    ];
                    c.support_moments[k] = [y.support_moments[k], z.support_moments[k]];
                }
                for (axis, r) in [&y, &z].iter().enumerate() {
                    for i in &r.intervals {
                        for t in candidates(&[i.displacement.derivative()], i.range[0], i.range[1])
                        {
                            c.displacement_mm[axis] =
                                c.displacement_mm[axis].max(i.displacement.value(t).abs());
                        }
                    }
                }
                let mut displacement_knots = vec![0.0, 1.0];
                for r in [&y, &z] {
                    for i in &r.intervals {
                        displacement_knots.extend(i.range);
                    }
                }
                displacement_knots.sort_by(f64::total_cmp);
                displacement_knots.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
                for w in displacement_knots.windows(2) {
                    let t = (w[0] + w[1]) / 2.0;
                    let dy = &interval(&y, t).displacement;
                    let dz = &interval(&z, t).displacement;
                    let squared = dy.multiply(dy).add(&dz.multiply(dz));
                    for t in candidates(&[squared.derivative()], w[0], w[1]) {
                        c.resultant_displacement_mm = c
                            .resultant_displacement_mm
                            .max(dy.value(t).hypot(dz.value(t)));
                    }
                }
                let max_delta = c.resultant_displacement_mm;
                match input
                    .deflection_limit_denom
                    .filter(|d| d.is_finite() && *d > 0.0)
                {
                    Some(d) => c.deflection_ratio = Some(max_delta / l * d),
                    None => c
                        .reasons
                        .push("たわみ限界が未設定または不正のため、変位のみ算定しています".into()),
                }
                let ctx = DesignCtx {
                    kind: MemberKind::Column,
                    term: if short {
                        LoadTerm::Short
                    } else {
                        LoadTerm::Long
                    },
                    length: lb.unwrap_or(l),
                    lk_y: lk.map(|v| v[0]),
                    lk_z: lk.map(|v| v[1]),
                    ..Default::default()
                };
                let f = crate::material_strength::section_steel_f(sec, Some(mat));
                let allow = if valid_shape {
                    let zero = MemberForcesAt {
                        pos: 0.0,
                        n: 0.0,
                        mz: 0.0,
                        my: 0.0,
                        qy: 0.0,
                        qz: 0.0,
                    };
                    match crate::steel::SteelDesign.check(&zero, sec, mat, &ctx) {
                        CheckOutcome::Skipped { reason } => Err(reason),
                        CheckOutcome::Checked(_) => f.and_then(|f| {
                            crate::steel::column_allowables(sec, mat, &ctx, f, ctx.term)
                        }),
                    }
                } else {
                    Err("断面形式・座屈条件が未確定です".into())
                };
                let mut knots = vec![0.0, 1.0];
                for r in [&g, &y, &z] {
                    for i in &r.intervals {
                        knots.extend(i.range);
                    }
                }
                knots.sort_by(f64::total_cmp);
                knots.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
                for w in knots.windows(2) {
                    let mid = (w[0] + w[1]) / 2.0;
                    let (gi, yi, zi) = (interval(&g, mid), interval(&y, mid), interval(&z, mid));
                    let n = gi
                        .cumulative_load
                        .add(&Polynomial::constant(-c.gravity_reactions[0]))
                        .scale((b[2] - a[2]).signum());
                    let force_polys = [
                        n.clone(),
                        yi.moment.clone(),
                        zi.moment.clone(),
                        yi.shear.clone(),
                        zi.shear.clone(),
                    ];
                    let sign_knots = candidates(&force_polys, w[0], w[1]);
                    for sw in sign_knots.windows(2) {
                        let m = (sw[0] + sw[1]) / 2.0;
                        let (mut positions, mut reason) = (vec![sw[0], sw[1]], None);
                        match &allow {
                            Ok(a) => {
                                let abs: Vec<_> =
                                    force_polys.iter().map(|p| signed(p, m)).collect();
                                let axial = abs[0]
                                    .scale(
                                        1.0 / (a.area * if n.value(m) < 0.0 { a.fc } else { a.ft }),
                                    )
                                    .add(&abs[1].scale(1.0 / (a.z_strong * a.fb_strong)))
                                    .add(&abs[2].scale(1.0 / (a.z_weak * a.fb_weak)));
                                let sigma=abs[0].scale(1.0/a.area).add(&abs[1].scale(a.h_web_factor/a.z_strong)).add(&abs[2].scale(if matches!(sec.shape,Some(sepika_core::section_shape::SectionShape::SteelH{..})){0.0}else{1.0/a.z_weak}));
                                let tau = abs[3]
                                    .scale(1.0 / a.shear_areas[0])
                                    .add(&abs[4].scale(1.0 / a.shear_areas[1]));
                                let vm = sigma.multiply(&sigma).add(&tau.multiply(&tau).scale(3.0));
                                for p in &force_polys {
                                    positions.extend(p.derivative().roots(sw[0], sw[1]));
                                }
                                positions.extend(axial.derivative().roots(sw[0], sw[1]));
                                positions.extend(vm.derivative().roots(sw[0], sw[1]));
                                positions.extend(tau.derivative().roots(sw[0], sw[1]));
                            }
                            Err(e) => reason = Some(e.clone()),
                        }
                        positions.sort_by(f64::total_cmp);
                        positions.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
                        for t in positions {
                            let forces = MemberForcesAt {
                                pos: t,
                                n: n.value(t),
                                mz: yi.moment.value(t),
                                my: zi.moment.value(t),
                                qy: yi.shear.value(t),
                                qz: zi.shear.value(t),
                            };
                            let check = if let Some(reason) = reason.clone() {
                                CheckOutcome::Skipped { reason }
                            } else {
                                let mut check =
                                    crate::steel::SteelDesign.check(&forces, sec, mat, &ctx);
                                if lk.is_none() || lb.is_none() {
                                    if let CheckOutcome::Checked(r) = &mut check {
                                        r.components.retain(|c| c.kind == CheckKind::Shear);
                                        if r.components.is_empty() {
                                            check = CheckOutcome::Skipped {
                                                reason: "座屈条件が不足しています".into(),
                                            };
                                        }
                                    }
                                }
                                check
                            };
                            c.positions.push(PostPositionResult {
                                position: t * l,
                                forces,
                                check,
                            });
                        }
                    }
                }
            }
        }
        result.combinations.push(c);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    mod fixture {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sepika-core/tests/support/steel_secondary_post_model.rs"
        ));
    }
    fn run(m: &Model) -> PostDesignResult {
        design_post(m, m.posts().next().unwrap())
    }
    #[test]
    fn three_metre_box_combines_gravity_and_lateral_at_same_position() {
        let m = fixture::steel_post_model();
        let r = run(&m);
        assert!(r.complete(), "{r:?}");
        let c = &r.combinations[0];
        assert!((c.lateral_reactions[0][0] - 3000.0).abs() < 1e-9);
        assert!((c.lateral_reactions[1][0] - 3000.0).abs() < 1e-9);
        // A=4544 mm²、Iy=23,060,138.6667 mm⁴、Z=230,601.386667 mm³。
        // 自重=78.5e-6×4544=0.356704 N/mm、中央N=-535.056 N。
        let center = c
            .positions
            .iter()
            .find(|p| (p.position - 1500.0).abs() < 1e-7)
            .unwrap();
        assert!((center.forces.mz - 2.25e6).abs() < 1e-7);
        assert!((center.forces.n + 535.056).abs() < 1e-6);
        let CheckOutcome::Checked(check) = &center.check else {
            panic!()
        };
        assert!(
            (check
                .components
                .iter()
                .find(|c| c.kind == CheckKind::AxialBending)
                .unwrap()
                .ratio
                - 0.06331187630168549)
                .abs()
                < 1e-12
        );
        assert!(
            (c.resultant_displacement_mm - 0.4462086848252602).abs() < 1e-5,
            "{}",
            c.resultant_displacement_mm
        );
    }
    #[test]
    fn gravity_shares_reverse_compression_and_tension_without_changing_transverse_support() {
        let mut m = fixture::steel_post_model();
        let before = run(&m);
        m.wall_regions[0].posts[0].gravity_end_shares = Some([0.0, 1.0]);
        let after = run(&m);
        assert!(after.complete(), "{after:?}");
        assert!(before.combinations[0]
            .positions
            .iter()
            .any(|p| p.forces.n < -1000.0));
        assert!(after.combinations[0]
            .positions
            .iter()
            .any(|p| p.forces.n > 1000.0));
        assert_eq!(before.support, after.support);
        assert_eq!(
            before.combinations[0].lateral_reactions,
            after.combinations[0].lateral_reactions
        );
    }
    #[test]
    fn orientation_cantilever_limit_and_missing_conditions() {
        let mut m = fixture::steel_post_model();
        let base = run(&m);
        m.post_design_inputs[0].strong_direction_deg = Some(90.0);
        let rotated = run(&m);
        assert!(rotated.combinations[0]
            .positions
            .iter()
            .any(|p| p.forces.my.abs() > 2e6));
        assert!(
            rotated.combinations[0].resultant_displacement_mm
                > base.combinations[0].resultant_displacement_mm
        );
        m.post_design_inputs[0].lateral_support = PostLateralSupport::FixedStart;
        let cant = run(&m);
        assert!((cant.combinations[0].support_moments[0][1].abs() - 9e6).abs() < 1e-6);
        m.post_design_inputs[0].deflection_limit_denom = None;
        let missing = run(&m);
        assert!(!missing.complete());
        assert!(missing.combinations[0].resultant_displacement_mm > 0.0);
        assert!(missing.combinations[0].deflection_ratio.is_none());
        m.post_design_inputs[0].buckling_lengths_mm = None;
        let missing = run(&m);
        assert!(!missing.complete());
        assert!(missing.combinations[0].positions.iter().any(|p|matches!(&p.check,CheckOutcome::Checked(r) if r.components.iter().all(|c|c.kind==CheckKind::Shear))));
    }
    #[test]
    fn simultaneous_two_direction_displacement_uses_vector_at_same_position() {
        let mut m = fixture::steel_post_model();
        let one = run(&m).combinations[0].displacement_mm[0];
        let iy = m.sections[0].iy;
        let iz = m.sections[0].iz;
        let mut load = m.post_design_inputs[0].lateral_loads[0].clone();
        load.direction = [0.0, 1.0];
        load.load = MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 2.0 * iz / iy,
            w2: 2.0 * iz / iy,
        };
        m.post_design_inputs[0].lateral_loads.push(load);
        let r = run(&m);
        assert!((r.combinations[0].resultant_displacement_mm - one * 2.0_f64.sqrt()).abs() < 1e-8);
    }
    #[test]
    fn absent_and_invalid_box_radius_are_reasoned_unchecked_not_panics() {
        for radius in [None, Some(1.0)] {
            let mut m = fixture::steel_post_model();
            if let Some(sepika_core::section_shape::SectionShape::SteelBox { corner_r, .. }) =
                &mut m.sections[0].shape
            {
                *corner_r = radius;
            }
            let r = run(&m);
            assert!(!r.complete());
            assert!(r.combinations[0].positions.iter().all(
                |p| matches!(&p.check,CheckOutcome::Skipped{reason} if reason.contains("半径"))
            ));
        }
    }
    #[test]
    fn short_term_and_ratio_crossing_one_use_selected_combination() {
        let mut m = fixture::steel_post_model();
        m.combinations[0].terms = vec![(sepika_core::ids::LoadCaseId(1), 1.0)];
        m.post_design_inputs[0].lateral_loads[0].case = sepika_core::ids::LoadCaseId(1);
        m.post_design_inputs[0].deflection_limit_denom = Some(1.0);
        // BOXの長期ft=156.6666667、Z=230601.386667。w=8Zft/L²で曲げ比1。
        let w = 32.112785698765435;
        for (factor, ok) in [(0.999, true), (1.001, false)] {
            m.post_design_inputs[0].lateral_loads[0].load = MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: w * factor,
                w2: w * factor,
            };
            let r = run(&m);
            assert!(r.complete(), "{r:?}");
            assert_eq!(r.ratio() <= 1.0, ok, "{}", r.ratio());
        }
        let long = run(&m);
        m.post_design_inputs[0].combinations[0].1 = true;
        let short = run(&m);
        assert!((long.ratio() / short.ratio() - 1.5).abs() < 1e-8);
    }
    #[test]
    fn h_section_axial_and_lateral_buckling_lengths_are_independent_of_span() {
        let mut m = fixture::steel_post_model();
        let mut sec = crate::steel::test_support::h_section(200.0, 100.0, 6.0, 8.0);
        sec.material = Some(sepika_core::ids::MaterialId(0));
        sec.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
        m.sections[0] = sec;
        let base = run(&m);
        m.post_design_inputs[0].lateral_buckling_length_mm = Some(10000.0);
        let lateral = run(&m);
        assert!(lateral.ratio() > base.ratio());
        assert_eq!(
            base.combinations[0].resultant_displacement_mm,
            lateral.combinations[0].resultant_displacement_mm
        );
        m.post_design_inputs[0].lateral_buckling_length_mm = Some(3000.0);
        m.post_design_inputs[0].buckling_lengths_mm = Some([3000.0, 6000.0]);
        let axial = run(&m);
        assert!(axial.ratio() > base.ratio());
        assert_eq!(
            base.combinations[0].resultant_displacement_mm,
            axial.combinations[0].resultant_displacement_mm
        );
    }
    #[test]
    fn incoming_secondary_reaction_is_counted_once_and_changes_axial_force_at_its_position() {
        use sepika_core::{ids::*, model::*};
        let mut m = fixture::steel_post_model();
        m.unassigned_beams.push(SecondaryMember {
            id: SecondaryMemberId(1),
            kind: SecondaryMemberKind::Beam,
            section: Some(SectionId(1)),
            ends: SecondaryMemberEnds::Supported([
                SecondaryMemberAnchor {
                    support: SupportMemberId::Secondary(SecondaryMemberId(0)),
                    position: 0.5,
                },
                SecondaryMemberAnchor {
                    support: SupportMemberId::Primary(ElemId(3)),
                    position: 0.5,
                },
            ]),
            name: "架け側".into(),
            ..Default::default()
        });
        m.next_secondary_member_id = 2;
        let r = run(&m);
        assert!(r.complete(), "{r:?}");
        let c = &r.combinations[0];
        // 鉄骨線重量0.356704 N/mm、架け側の面間自重区間800 mmの半分142.6816 Nを受ける。
        assert!((c.gravity_reactions.iter().sum::<f64>() - 1212.7936).abs() < 1e-6);
        let forces: Vec<_> = c
            .positions
            .iter()
            .filter(|p| (p.position - 1500.0).abs() < 1e-7)
            .map(|p| p.forces.n)
            .collect();
        let low = forces.iter().copied().fold(f64::INFINITY, f64::min);
        let high = forces.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!((high - low - 142.6816).abs() < 1e-6);
        let support = SupportMemberId::Secondary(SecondaryMemberId(0));
        assert_eq!(
            sepika_core::face_distance::wall_support_face(&m, support, [1.0, 0.0, 0.0]),
            Ok(100.0)
        );
        m.post_design_inputs[0].strong_direction_deg = Some(90.0);
        assert!(
            (sepika_core::face_distance::wall_support_face(&m, support, [1.0, 0.0, 0.0]).unwrap()
                - 50.0)
                .abs()
                < 1e-8
        );
        let rotated = run(&m);
        assert!(rotated.complete(), "{rotated:?}");
        assert!(
            (rotated.combinations[0]
                .gravity_reactions
                .iter()
                .sum::<f64>()
                - 1229.29116)
                .abs()
                < 1e-6
        );
    }
}
