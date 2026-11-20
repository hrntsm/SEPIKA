//! 鋼製間柱の独立した許容応力度検定。力・長さは N-mm 系。

use crate::{CheckKind, CheckOutcome, DesignCtx, LoadTerm, MemberForcesAt, MemberKind};
use sepika_core::load_combo::effective_combination_terms;
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
pub struct PostDisplacement {
    /// 強軸・弱軸方向の絶対最大変位 `[mm]`。
    pub axes_mm: [Option<f64>; 2],
    /// 二方向の同位置合成による絶対最大変位 `[mm]`。
    pub resultant_mm: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PostCombinationResult {
    pub name: String,
    pub short_term: bool,
    pub positions: Vec<PostPositionResult>,
    /// 鉛直端部反力 `[N]`。始端、終端の順。
    pub gravity_reactions: Option<[f64; 2]>,
    pub deflection_ratio: Option<f64>,
    pub reasons: Vec<String>,
    /// 全体X/Y方向の横反力 `[N]`。始端、終端の順。
    pub lateral_reactions: Option<[[f64; 2]; 2]>,
    /// 強軸・弱軸の固定端曲げ `[N mm]`。始端、終端の順。
    pub support_moments: Option<[[f64; 2]; 2]>,
    pub displacement: Option<PostDisplacement>,
    /// 検定比の数値が未算定でも、たわみの限界超過を確定できたか。
    #[serde(default)]
    pub deflection_limit_exceeded_without_ratio: bool,
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
fn squared_norm(first: &Polynomial, second: &Polynomial, second_weight: f64) -> Polynomial {
    let scale = first
        .0
        .iter()
        .chain(&second.0)
        .map(|v| v.abs())
        .fold(1.0, f64::max);
    // 極値位置だけを求めるため、二乗があふれない共通尺度を使える。
    let first = first.scale(1.0 / scale);
    let second = second.scale(1.0 / scale);
    first
        .multiply(&first)
        .add(&second.multiply(&second).scale(second_weight))
}

fn finite_roots(p: &Polynomial, a: f64, b: f64) -> Result<Vec<f64>, String> {
    if p.0.iter().all(|v| v.is_finite()) {
        Ok(p.roots(a, b))
    } else {
        Err("応答・検定の評価位置を有限値で算定できません".into())
    }
}

fn candidates(polys: &[Polynomial], a: f64, b: f64) -> Result<Vec<f64>, String> {
    let mut result = vec![a, b];
    for p in polys {
        result.extend(finite_roots(p, a, b)?);
    }
    result.sort_by(f64::total_cmp);
    result.dedup_by(|x, y| (*x - *y).abs() < 1e-10);
    Ok(result)
}

fn axis_displacement(response: &BeamResponse) -> Result<f64, String> {
    let mut max_mm = 0.0_f64;
    for i in &response.intervals {
        for t in candidates(&[i.displacement.derivative()], i.range[0], i.range[1])? {
            let delta = i.displacement.value(t).abs();
            if !delta.is_finite() {
                return Err("方向別変位を有限値で算定できません".into());
            }
            max_mm = max_mm.max(delta);
        }
    }
    Ok(max_mm)
}

fn resultant_displacement(y: &BeamResponse, z: &BeamResponse) -> Result<f64, String> {
    let mut knots = vec![0.0, 1.0];
    for response in [y, z] {
        knots.extend(response.intervals.iter().flat_map(|i| i.range));
    }
    knots.sort_by(f64::total_cmp);
    knots.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
    let mut resultant_mm = 0.0_f64;
    for w in knots.windows(2) {
        let t = (w[0] + w[1]) / 2.0;
        let dy = &interval(y, t).displacement;
        let dz = &interval(z, t).displacement;
        let squared = squared_norm(dy, dz, 1.0);
        for t in candidates(&[squared.derivative()], w[0], w[1])? {
            let delta = dy.value(t).hypot(dz.value(t));
            if !delta.is_finite() {
                return Err("同位置合成変位を有限値で算定できません".into());
            }
            resultant_mm = resultant_mm.max(delta);
        }
    }
    Ok(resultant_mm)
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
            gravity_reactions: None,
            lateral_reactions: None,
            support_moments: None,
            displacement: None,
            deflection_ratio: None,
            deflection_limit_exceeded_without_ratio: false,
            reasons: Vec::new(),
        };
        let terms = match effective_combination_terms(combo, &model.load_cases) {
            Ok(terms) => terms,
            Err(reason) => {
                c.reasons.push(reason);
                result.combinations.push(c);
                continue;
            }
        };
        let mut gravity = Vec::new();
        let mut gravity_reactions = [0.0; 2];
        let mut gravity_valid = true;
        let mut lateral = [Vec::new(), Vec::new()];
        let mut lateral_valid = true;
        let mut has_lateral = false;
        for (case_id, factor) in terms {
            if factor == 0.0 {
                continue;
            }
            let case = model
                .load_cases
                .iter()
                .find(|c| c.id == case_id)
                .expect("組合せで参照するケースは検証済み");
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
                    gravity_valid = false;
                } else {
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
                                    "鉛直荷重伝達に循環・負担率未設定・支持先未解決があります"
                                        .into(),
                                );
                                if t.has_incomplete_dependency(sm.id) {
                                    gravity_valid = false;
                                    c.reasons.push("間柱への鉛直荷重伝達が未確定です".into());
                                }
                            }
                            if let Some(entry) = t.members.get(&sm.id) {
                                gravity.extend(
                                    entry.member_loads.iter().map(|load| scaled(load, factor)),
                                );
                                for (combined, reaction) in
                                    gravity_reactions.iter_mut().zip(entry.reactions)
                                {
                                    *combined += reaction * factor;
                                }
                            } else {
                                c.reasons.push("間柱の鉛直荷重伝達が未解決です".into());
                                gravity_valid = false;
                            }
                        }
                        Err(e) => {
                            c.reasons.push(format!("鉛直荷重伝達: {e}"));
                            gravity_valid = false;
                        }
                    }
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
                    lateral_valid = false;
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
        if gravity_valid {
            if gravity_reactions.iter().all(|v| v.is_finite()) {
                c.gravity_reactions = Some(gravity_reactions);
            } else {
                c.reasons
                    .push("鉛直端部反力を有限値で算定できません".into());
            }
        }
        let evaluated = (|| -> Result<(), String> {
            if !lateral_valid {
                return Ok(());
            }
            let [(y, strong_mm), (z, weak_mm)] =
                [("強軸", &lateral[0], sec.iy), ("弱軸", &lateral[1], sec.iz)].map(
                    |(axis, loads, inertia)| {
                        let response = match beam_response(
                            loads,
                            l,
                            mat.young * inertia,
                            input.lateral_support,
                        ) {
                            Ok(response) => response,
                            Err(reason) => {
                                c.reasons
                                    .push(format!("{axis}方向の局部横荷重応答: {reason}"));
                                return (None, None);
                            }
                        };
                        let delta = match axis_displacement(&response) {
                            Ok(delta) => Some(delta),
                            Err(reason) => {
                                c.reasons.push(format!(
                                    "{axis}方向の局部横荷重の変位が未算定です（{reason}）"
                                ));
                                None
                            }
                        };
                        (Some(response), delta)
                    },
                );
            let axes_mm = [strong_mm, weak_mm];
            let resultant_mm = match (&y, &z, axes_mm) {
                (Some(y), Some(z), [Some(_), Some(_)]) => match resultant_displacement(y, z) {
                    Ok(delta) => Some(delta),
                    Err(reason) => {
                        c.reasons.push(format!("合成変位が未算定です（{reason}）"));
                        None
                    }
                },
                _ => None,
            };
            if axes_mm.iter().any(Option::is_some) {
                c.displacement = Some(PostDisplacement {
                    axes_mm,
                    resultant_mm,
                });
            }
            if let Some(delta) = resultant_mm {
                let relative_displacement = delta / l;
                match input
                    .deflection_limit_denom
                    .filter(|d| d.is_finite() && *d > 0.0)
                {
                    Some(d) => {
                        let ratio = relative_displacement * d;
                        if ratio.is_finite() {
                            c.deflection_ratio = Some(ratio);
                        } else {
                            c.deflection_limit_exceeded_without_ratio = delta > l / d;
                            let reason = if relative_displacement.is_finite() {
                                "たわみ検定比の乗算で非有限値が発生しました"
                            } else {
                                "たわみの材長比（δ/L）の除算で非有限値が発生しました"
                            };
                            c.reasons.push(reason.into());
                        }
                    }
                    None => c
                        .reasons
                        .push("たわみ限界が未設定または不正のため、変位のみ算定しています".into()),
                }
            }

            let (Some(y), Some(z)) = (y, z) else {
                return Ok(());
            };
            let mut lateral_reactions = [[0.0; 2]; 2];
            let mut support_moments = [[0.0; 2]; 2];
            for (k, (reaction, moment)) in lateral_reactions
                .iter_mut()
                .zip(&mut support_moments)
                .enumerate()
            {
                *reaction = [
                    y.reactions[k] * strong[0] + z.reactions[k] * weak[0],
                    y.reactions[k] * strong[1] + z.reactions[k] * weak[1],
                ];
                *moment = [y.support_moments[k], z.support_moments[k]];
            }
            if lateral_reactions.iter().flatten().all(|v| v.is_finite()) {
                c.lateral_reactions = Some(lateral_reactions);
            } else {
                c.reasons
                    .push("局部横荷重の横反力の計算で非有限値が発生しました".into());
            }
            if support_moments.iter().flatten().all(|v| v.is_finite()) {
                c.support_moments = Some(support_moments);
            } else {
                c.reasons
                    .push("局部横荷重の固定端曲げの計算で非有限値が発生しました".into());
            }
            let Some(gravity_reactions) = c.gravity_reactions else {
                return Ok(());
            };
            let g = beam_response(&gravity, l, 1.0, PostLateralSupport::Pinned)
                .map_err(|e| format!("鉛直荷重応答: {e}"))?;
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
                    mx: 0.0,
                    my: 0.0,
                    qy: 0.0,
                    qz: 0.0,
                };
                match crate::steel::SteelDesign.check_secondary_post(&zero, sec, mat, &ctx) {
                    CheckOutcome::Skipped { reason } => Err(reason),
                    CheckOutcome::Checked(_) => {
                        f.and_then(|f| crate::steel::column_allowables(sec, mat, &ctx, f, None))
                    }
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
                    .add(&Polynomial::constant(-gravity_reactions[0]))
                    .scale((b[2] - a[2]).signum());
                let force_polys = [
                    n.clone(),
                    yi.moment.clone(),
                    zi.moment.clone(),
                    yi.shear.clone(),
                    zi.shear.clone(),
                ];
                let sign_knots = candidates(&force_polys, w[0], w[1])?;
                for sw in sign_knots.windows(2) {
                    let m = (sw[0] + sw[1]) / 2.0;
                    let mut positions = vec![sw[0], sw[1]];
                    for p in &force_polys {
                        positions.extend(finite_roots(&p.derivative(), sw[0], sw[1])?);
                    }
                    if let Ok(a) = &allow {
                        let abs: Vec<_> = force_polys.iter().map(|p| signed(p, m)).collect();
                        let axial = abs[0]
                            .scale(1.0 / (a.area * if n.value(m) < 0.0 { a.fc } else { a.ft }))
                            .add(&abs[1].scale(1.0 / (a.z_strong * a.fb_strong)))
                            .add(&abs[2].scale(1.0 / (a.z_weak * a.fb_weak)));
                        let sigma = abs[0]
                            .scale(1.0 / a.area)
                            .add(&abs[1].scale(a.h_web_factor / a.z_strong))
                            .add(&abs[2].scale(
                                if matches!(
                                    sec.shape,
                                    Some(sepika_core::section_shape::SectionShape::SteelH { .. })
                                ) {
                                    0.0
                                } else {
                                    1.0 / a.z_weak
                                },
                            ));
                        let tau = abs[3]
                            .scale(1.0 / a.shear_areas[0])
                            .add(&abs[4].scale(1.0 / a.shear_areas[1]));
                        let vm = squared_norm(&sigma, &tau, 3.0);
                        for p in [&axial, &vm, &tau] {
                            match finite_roots(&p.derivative(), sw[0], sw[1]) {
                                Ok(points) => positions.extend(points),
                                Err(_) => {
                                    let detail = "断面検定の評価位置の計算で非有限値が発生しました";
                                    if !c.reasons.iter().any(|r| r == detail) {
                                        c.reasons.push(detail.into());
                                    }
                                }
                            }
                        }
                    }
                    positions.sort_by(f64::total_cmp);
                    positions.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
                    for t in positions {
                        let forces = MemberForcesAt {
                            pos: t,
                            n: n.value(t),
                            mz: yi.moment.value(t),
                            mx: 0.0,
                            my: zi.moment.value(t),
                            qy: yi.shear.value(t),
                            qz: zi.shear.value(t),
                        };
                        if ![forces.n, forces.mz, forces.my, forces.qy, forces.qz]
                            .iter()
                            .all(|v| v.is_finite())
                        {
                            return Err("間柱の断面力を有限値で算定できません".into());
                        }
                        let mut check = if let Err(reason) = &allow {
                            CheckOutcome::Skipped {
                                reason: reason.clone(),
                            }
                        } else {
                            let mut check = crate::steel::SteelDesign
                                .check_secondary_post(&forces, sec, mat, &ctx);
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
                        if let CheckOutcome::Checked(r) = &mut check {
                            r.components.retain(|component| {
                                if component.ratio.is_finite() {
                                    return true;
                                }
                                let reason = format!(
                                    "{}の検定比の計算で非有限値が発生しました",
                                    component.kind.label()
                                );
                                if !c.reasons.contains(&reason) {
                                    c.reasons.push(reason);
                                }
                                false
                            });
                            if r.components.is_empty() {
                                check = CheckOutcome::Skipped {
                                    reason: "断面検定比の計算で非有限値が発生しました".into(),
                                };
                            }
                        }
                        c.positions.push(PostPositionResult {
                            position: t * l,
                            forces,
                            check,
                        });
                    }
                }
            }
            Ok(())
        })();
        if let Err(reason) = evaluated {
            c.reasons.push(reason);
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
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].terms = vec![(dead, 1.0)];
        let r = run(&m);
        assert!(r.complete(), "{r:?}");
        assert!(r.ratio().is_finite());
        let c = &r.combinations[0];
        let gravity = c.gravity_reactions.unwrap();
        assert!((gravity[0] - 1070.112).abs() < 1e-6);
        assert_eq!(gravity[1], 0.0);
        let reactions = c.lateral_reactions.unwrap();
        assert!((reactions[0][0] - 3000.0).abs() < 1e-9);
        assert!((reactions[1][0] - 3000.0).abs() < 1e-9);
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
            (c.displacement.as_ref().unwrap().resultant_mm.unwrap() - 0.4462086848252602).abs()
                < 1e-5
        );
        for factors in [[f64::MAX, -f64::MAX, 1.0], [f64::MAX, 1.0, -f64::MAX]] {
            m.combinations[0].terms = factors.into_iter().map(|f| (dead, f)).collect();
            assert_eq!(run(&m), r);
        }
    }

    #[test]
    fn invalid_combination_coefficient_has_named_reason_and_no_response() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].name = "係数異常の組合せ".into();
        for (factors, reason) in [
            (vec![1.0, f64::INFINITY], "荷重組合せ係数が非有限"),
            (vec![f64::MAX, f64::MAX], "荷重組合せ係数の合計が非有限"),
        ] {
            m.combinations[0].terms = factors.into_iter().map(|f| (dead, f)).collect();
            let r = run(&m);
            let c = &r.combinations[0];
            assert_eq!(c.name, "係数異常の組合せ");
            assert!(c.reasons.iter().any(|r| r.contains(reason)), "{c:?}");
            assert!(c.gravity_reactions.is_none());
            assert!(c.lateral_reactions.is_none());
            assert!(c.support_moments.is_none());
            assert!(c.displacement.is_none());
            assert!(c.positions.is_empty());
            assert!(c.deflection_ratio.is_none());
            assert!(!r.complete());
        }
    }

    #[test]
    fn nonfinite_check_extremum_keeps_known_slenderness_failure_and_shear() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].terms = vec![(dead, 1.0)];
        m.post_design_inputs[0].buckling_lengths_mm = Some([1e160; 2]);
        let r = run(&m);
        let c = &r.combinations[0];
        assert!(c
            .reasons
            .iter()
            .any(|reason| reason.contains("断面検定の評価位置の計算で非有限値")));
        assert!(!r.complete());

        let start = c.positions.iter().find(|p| p.position == 0.0).unwrap();
        assert!((start.forces.n + 1070.112).abs() < 1e-6);
        let CheckOutcome::Checked(start_check) = &start.check else {
            panic!("{:?}", start.check);
        };
        let slenderness = start_check
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Provision)
            .unwrap()
            .ratio;
        assert!((slenderness / 1.2243241899999748e156 - 1.0).abs() < 1e-12);
        assert_eq!(r.ratio(), slenderness);

        let end = c.positions.iter().find(|p| p.position == 3000.0).unwrap();
        assert_eq!(end.forces.n, 0.0);
        assert!((end.forces.qy + 3000.0).abs() < 1e-9);
        let CheckOutcome::Checked(end_check) = &end.check else {
            panic!("{:?}", end.check);
        };
        let shear = end_check
            .components
            .iter()
            .find(|c| c.kind == CheckKind::Shear)
            .unwrap()
            .ratio;
        assert!((shear - 0.011265941018241138).abs() < 1e-12);
    }

    #[test]
    fn nonfinite_lateral_response_keeps_gravity_reactions() {
        let mut m = fixture::steel_post_model();
        m.combinations[0].name = "横荷重積分の組合せ".into();
        m.post_design_inputs[0].lateral_loads[0].load = MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 1e305,
            w2: 1e305,
        };
        let r = run(&m);
        let c = &r.combinations[0];
        assert_eq!(c.name, "横荷重積分の組合せ");
        assert!(
            c.reasons
                .iter()
                .any(|r| r.contains("局部横荷重の横反力の計算で非有限値")),
            "{c:?}"
        );
        let gravity = c.gravity_reactions.unwrap();
        assert!((gravity[0] - 1070.112).abs() < 1e-6);
        assert_eq!(gravity[1], 0.0);
        assert!(c.lateral_reactions.is_none());
        assert!(c.support_moments.is_none());
        let displacement = c.displacement.as_ref().unwrap();
        assert_eq!(displacement.axes_mm, [None, Some(0.0)]);
        assert!(displacement.resultant_mm.is_none());
        assert!(c.positions.is_empty());
        assert!(c.deflection_ratio.is_none());
        assert!(!r.complete());
    }

    #[test]
    fn nonfinite_gravity_reaction_keeps_lateral_response_and_other_combination() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        let lateral_case = m
            .load_cases
            .iter()
            .find(|case| case.kind == LoadCaseKind::Seismic)
            .unwrap()
            .id;
        m.post_design_inputs[0].lateral_loads[0].case = lateral_case;
        m.combinations[0].terms = vec![(dead, 1.0), (lateral_case, 1.0)];
        let expected = run(&m);
        m.combinations.push(sepika_core::model::LoadCombination {
            name: "鉛直端部反力の組合せ".into(),
            terms: vec![(dead, 1e307), (lateral_case, 1.0)],
        });
        m.post_design_inputs[0]
            .combinations
            .push((m.combinations.len() - 1, false));
        let r = run(&m);
        let c = &r.combinations[1];
        assert_eq!(c.name, "鉛直端部反力の組合せ");
        assert!(
            c.reasons
                .iter()
                .any(|r| r.contains("鉛直端部反力を有限値で算定できません")),
            "{c:?}"
        );
        assert!(c.gravity_reactions.is_none());
        assert_eq!(
            c.lateral_reactions,
            expected.combinations[0].lateral_reactions
        );
        assert_eq!(c.support_moments, expected.combinations[0].support_moments);
        assert_eq!(c.displacement, expected.combinations[0].displacement);
        assert!(c.positions.is_empty());
        assert_eq!(
            c.deflection_ratio,
            expected.combinations[0].deflection_ratio
        );
        assert!(!r.complete());
        assert_eq!(r.combinations[0], expected.combinations[0]);
        assert_eq!(r.ratio(), expected.ratio());
    }

    #[test]
    fn nonfinite_fixed_end_moment_keeps_lateral_reactions() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].name = "固定端曲げの組合せ".into();
        m.combinations[0].terms = vec![(dead, 1.0)];
        let input = &mut m.post_design_inputs[0];
        input.lateral_support = PostLateralSupport::FixedStart;
        input.lateral_loads[0].load = MemberLoadKind::Point {
            a: 3000.0,
            p: 1e306,
        };
        let r = run(&m);
        let c = &r.combinations[0];
        assert_eq!(c.name, "固定端曲げの組合せ");
        assert_eq!(c.lateral_reactions, Some([[1e306, 0.0], [0.0, 0.0]]));
        assert!(c.support_moments.is_none());
        assert!(c
            .reasons
            .iter()
            .any(|r| r.contains("固定端曲げの計算で非有限値")));
        assert!(!r.complete());
    }

    #[test]
    fn unrelated_invalid_gravity_shares_keep_post_reactions_and_forces() {
        let mut m = fixture::steel_post_model();
        let expected = run(&m);
        let mut separate_post = m.wall_regions[0].posts[0].clone();
        separate_post.id = m.alloc_secondary_member_id();
        separate_post.gravity_end_shares = None;
        let sepika_core::model::SecondaryMemberEnds::Supported(anchors) = &mut separate_post.ends
        else {
            panic!();
        };
        for anchor in anchors {
            anchor.position = 0.75;
        }
        m.wall_regions[0].posts.push(separate_post);
        let r = run(&m);
        let c = &r.combinations[0];
        assert!(c.reasons.iter().any(|r| r.contains("負担率未設定")));
        assert!(!r.complete());
        assert_eq!(
            c.gravity_reactions,
            expected.combinations[0].gravity_reactions
        );
        assert_eq!(c.positions, expected.combinations[0].positions);
    }

    #[test]
    fn invalid_gravity_shares_on_geometric_support_path_leave_gravity_unavailable() {
        use sepika_core::model::SecondaryMemberEnds;

        let mut m = fixture::steel_post_model();
        let mut supported_post = m.wall_regions[0].posts[0].clone();
        supported_post.id = m.alloc_secondary_member_id();
        supported_post.ends =
            SecondaryMemberEnds::Detached([[5.0, 0.0, 1500.0], [5.0, 0.0, 3000.0]]);
        m.wall_regions[0].posts.push(supported_post);

        let baseline = run(&m);
        let c = &baseline.combinations[0];
        let gravity = c.gravity_reactions.unwrap();
        assert!((gravity[0] - 1605.168).abs() < 1e-6);
        assert_eq!(gravity[1], 0.0);
        assert!(!c.positions.is_empty());

        m.wall_regions[0].posts[1].gravity_end_shares = None;
        let r = run(&m);
        let c = &r.combinations[0];
        assert!(c.gravity_reactions.is_none());
        assert!(c.positions.is_empty());
        assert!(c
            .reasons
            .iter()
            .any(|reason| reason.contains("間柱への鉛直荷重伝達が未確定")));
        assert_eq!(
            c.lateral_reactions,
            baseline.combinations[0].lateral_reactions
        );
    }

    #[test]
    fn displacement_overflow_keeps_finite_reactions_and_moment() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].terms = vec![(dead, 1.0)];
        m.materials[0].young = 2.05e-303;
        let r = run(&m);
        let c = &r.combinations[0];
        let gravity = c.gravity_reactions.unwrap();
        assert!((gravity[0] - 1070.112).abs() < 1e-6);
        assert_eq!(gravity[1], 0.0);
        assert_eq!(c.lateral_reactions, Some([[3000.0, 0.0]; 2]));
        assert_eq!(c.support_moments, Some([[0.0; 2]; 2]));
        let displacement = c.displacement.as_ref().unwrap();
        assert_eq!(displacement.axes_mm, [None, Some(0.0)]);
        assert!(displacement.resultant_mm.is_none());
        assert!(c.deflection_ratio.is_none());
        assert!(c
            .reasons
            .iter()
            .any(|reason| reason.contains("変位が未算定")));
        let center = c
            .positions
            .iter()
            .find(|p| (p.position - 1500.0).abs() < 1e-7)
            .unwrap();
        assert!((center.forces.mz - 2_250_000.0).abs() < 1e-7);
        assert!(!r.complete());
    }

    #[test]
    fn nonfinite_weak_axis_displacement_keeps_finite_strong_axis_displacement() {
        let mut m = fixture::steel_post_model();
        m.materials[0].young = 2.05e-303;
        m.post_design_inputs[0].lateral_loads[0].direction = [0.01, 0.9999_f64.sqrt()];
        let r = run(&m);
        let c = &r.combinations[0];
        let displacement = c.displacement.as_ref().unwrap();
        let strong_mm = displacement.axes_mm[0].unwrap();
        assert!((strong_mm / 4.462086848252602e305 - 1.0).abs() < 1e-8);
        assert!(displacement.axes_mm[1].is_none());
        assert!(displacement.resultant_mm.is_none());
        assert!(c.deflection_ratio.is_none());
        assert!(c
            .reasons
            .iter()
            .any(|reason| reason.contains("弱軸方向の局部横荷重の変位が未算定")));
        assert!(!r.complete());
    }

    #[test]
    fn invalid_weak_axis_stiffness_keeps_strong_axis_displacement() {
        let mut m = fixture::steel_post_model();
        m.sections[0].iz = 0.0;
        let r = run(&m);
        let c = &r.combinations[0];
        let displacement = c.displacement.as_ref().unwrap();
        assert!((displacement.axes_mm[0].unwrap() - 0.4462086848252602).abs() < 1e-5);
        assert!(displacement.axes_mm[1].is_none());
        assert!(displacement.resultant_mm.is_none());
        assert!(c.lateral_reactions.is_none());
        assert!(c.support_moments.is_none());
        assert!(c.positions.is_empty());
        assert!(c
            .reasons
            .iter()
            .any(|reason| reason.contains("弱軸方向の局部横荷重応答")));
        assert!(!r.complete());
    }

    #[test]
    fn large_finite_responses_keep_available_section_checks() {
        let mut m = fixture::steel_post_model();
        let dead = m.post_design_inputs[0].lateral_loads[0].case;
        m.combinations[0].terms = vec![(dead, 1.0)];
        for (w, complete) in [(1e153, true), (1e155, false)] {
            m.post_design_inputs[0].lateral_loads[0].load = MemberLoadKind::Distributed {
                a: 0.0,
                b: 3000.0,
                w1: w,
                w2: w,
            };
            let r = run(&m);
            let c = &r.combinations[0];
            assert_eq!(r.complete(), complete, "{:?}", c.reasons);
            assert!(r.ratio().is_finite());
            let reactions = c.lateral_reactions.unwrap();
            assert!((reactions[0][0] / w - 1500.0).abs() < 1e-8);
            assert!((reactions[1][0] / w - 1500.0).abs() < 1e-8);
            assert!(
                (c.displacement.as_ref().unwrap().resultant_mm.unwrap() / w - 0.2231043424126301)
                    .abs()
                    < 1e-10
            );
            assert!((c.deflection_ratio.unwrap() / w - 0.01859202853438584).abs() < 1e-10);
            let center = c
                .positions
                .iter()
                .find(|p| (p.position - 1500.0).abs() < 1e-7)
                .unwrap();
            assert!((center.forces.mz / w - 1_125_000.0).abs() < 1e-8);
            let CheckOutcome::Checked(check) = &center.check else {
                panic!("{:?}", center.check);
            };
            assert!(check
                .components
                .iter()
                .any(|c| c.kind == CheckKind::AxialBending));
            assert!(check.components.iter().all(|c| c.ratio.is_finite()));
            assert_eq!(
                check.components.iter().any(|c| c.kind == CheckKind::Shear),
                complete
            );
            if !complete {
                assert!(c
                    .reasons
                    .iter()
                    .any(|r| r.contains("せん断の検定比の計算で非有限値が発生しました")));
            }
        }
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
            rotated.combinations[0]
                .displacement
                .as_ref()
                .unwrap()
                .resultant_mm
                .unwrap()
                > base.combinations[0]
                    .displacement
                    .as_ref()
                    .unwrap()
                    .resultant_mm
                    .unwrap()
        );
        m.post_design_inputs[0].lateral_support = PostLateralSupport::FixedStart;
        let cant = run(&m);
        assert!((cant.combinations[0].support_moments.unwrap()[0][1].abs() - 9e6).abs() < 1e-6);
        m.post_design_inputs[0].deflection_limit_denom = None;
        let missing = run(&m);
        assert!(!missing.complete());
        assert!(
            missing.combinations[0]
                .displacement
                .as_ref()
                .unwrap()
                .resultant_mm
                .unwrap()
                > 0.0
        );
        assert!(missing.combinations[0].deflection_ratio.is_none());
        m.post_design_inputs[0].buckling_lengths_mm = None;
        let missing = run(&m);
        assert!(!missing.complete());
        assert!(missing.combinations[0].positions.iter().any(|p|matches!(&p.check,CheckOutcome::Checked(r) if r.components.iter().all(|c|c.kind==CheckKind::Shear))));
    }
    #[test]
    fn simultaneous_two_direction_displacement_uses_vector_at_same_position() {
        let mut m = fixture::steel_post_model();
        let one = run(&m).combinations[0]
            .displacement
            .as_ref()
            .unwrap()
            .axes_mm[0]
            .unwrap();
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
        assert!(
            (r.combinations[0]
                .displacement
                .as_ref()
                .unwrap()
                .resultant_mm
                .unwrap()
                - one * 2.0_f64.sqrt())
            .abs()
                < 1e-8
        );
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
    fn deflection_ratio_overflow_preserves_confirmed_limit_exceedance() {
        let mut m = fixture::steel_post_model();
        let input = &mut m.post_design_inputs[0];
        input.lateral_loads[0].load = MemberLoadKind::Distributed {
            a: 0.0,
            b: 3000.0,
            w1: 2e5,
            w2: 2e5,
        };
        input.deflection_limit_denom = Some(f64::MAX);
        let r = run(&m);
        let c = &r.combinations[0];
        let delta = c.displacement.as_ref().unwrap().resultant_mm.unwrap();
        assert!((delta / 3000.0 - 14.87362282750867).abs() < 1e-8);
        assert!(c.deflection_ratio.is_none());
        assert!(c.deflection_limit_exceeded_without_ratio);
        assert!(c
            .reasons
            .iter()
            .any(|r| r.contains("たわみ検定比の乗算で非有限値")));
        assert!(r.ratio().is_finite());
        assert!(!r.complete());

        m.post_design_inputs[0].deflection_limit_denom = Some(1.0);
        let baseline = run(&m);
        let c = &baseline.combinations[0];
        assert_eq!(c.deflection_ratio, Some(delta / 3000.0));
        assert!(!c.deflection_limit_exceeded_without_ratio);
        assert!(baseline.complete());
    }

    #[test]
    fn deflection_span_division_overflow_preserves_confirmed_limit_exceedance() {
        let mut m = fixture::steel_post_model();
        m.nodes[2].coord[2] = 0.001;
        m.nodes[3].coord[2] = 0.001;
        m.materials[0].young = 1e-190;
        let input = &mut m.post_design_inputs[0];
        let dead = input.lateral_loads[0].case;
        input.lateral_support = PostLateralSupport::FixedStart;
        input.lateral_loads[0].load = MemberLoadKind::Point { a: 0.001, p: 1e134 };
        input.deflection_limit_denom = Some(250.0);
        m.combinations[0].terms = vec![(dead, 1.0)];

        let r = run(&m);
        let c = &r.combinations[0];
        let delta = c.displacement.as_ref().unwrap().resultant_mm.unwrap();
        assert!((delta / 1.4454957888660281e307 - 1.0).abs() < 1e-12);
        assert!(c.deflection_ratio.is_none());
        assert!(c.deflection_limit_exceeded_without_ratio);
        assert!(c
            .reasons
            .iter()
            .any(|r| r.contains("材長比（δ/L）の除算で非有限値")));
        assert!(!r.complete());
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
            base.combinations[0].displacement,
            lateral.combinations[0].displacement
        );
        m.post_design_inputs[0].lateral_buckling_length_mm = Some(3000.0);
        m.post_design_inputs[0].buckling_lengths_mm = Some([3000.0, 6000.0]);
        let axial = run(&m);
        assert!(axial.ratio() > base.ratio());
        assert_eq!(
            base.combinations[0].displacement,
            axial.combinations[0].displacement
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
        assert!((c.gravity_reactions.unwrap().iter().sum::<f64>() - 1212.7936).abs() < 1e-6);
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
                .unwrap()
                .iter()
                .sum::<f64>()
                - 1229.29116)
                .abs()
                < 1e-6
        );
    }
}
