//! せん断降伏耐力 Qy の算定と降伏イベントの追跡（SRC 柱・SRC 耐震壁の
//! 部材ランク判定で「破壊モードがせん断破壊か」の判定に用いる）。
//!
//! - [`ShearThreshold`] / [`DirThreshold`] / [`ShearDir`] — 方向別 Qy しきい値
//! - [`build_shear_yield_thresholds`] — 全部材のしきい値を組み立て
//! - [`track_shear_yield`] — 各ステップのせん断降伏を軸力 σ0 を反映して判定

use super::geom::member_local_frame;
use super::geom::{axial_compression, dot3, recording_force};
use super::types::ShearYieldEvent;
use sepika_core::material_grade::{
    material_strength_factor_rebar, material_strength_factor_steel, resolve_steel_f,
    src_h_steel_minimum_thickness_mm, steel_f_value_issue, steel_grade_base,
    validate_src_h_steel_thickness,
};
use sepika_core::model::{ElementData, Material, MaterialCategory, Model, Section};
use sepika_core::rc_capacity::{rc_qsu_simple, RcCapacityInput};
use sepika_core::section_shape::SectionShape;
use sepika_element::behavior::{Ctx, ElementBehavior, LoadedFrameResponse};

/// せん断降伏耐力 Qy の判定しきい値（部材ごと、局所 y・z 方向、独立）。
pub(crate) struct ShearThreshold {
    pub(crate) y: DirThreshold,
    pub(crate) z: DirThreshold,
}

/// せん断降伏耐力 Qy の算定方式（方向別）。
pub(crate) enum DirThreshold {
    Static(f64),
    RcArakawa {
        /// σ0 抜きの入力一式（`sigma_0` は常に 0.0 のプレースホルダ。
        /// [`DirThreshold::qy`] が呼び出しのたびに軸力由来の値へ差し替える）。
        input: RcCapacityInput,
        /// 全断面積 [mm²]（= b・D。方向によらず同一値。σ0 = 圧縮軸力/gross_area
        /// の算定に用いる）。
        gross_area: f64,
        /// 内蔵鉄骨の全塑性せん断耐力 sAw・F/√3 [N]（SRC の累加項。RC は 0）。
        steel_qy: f64,
    },
}

impl DirThreshold {
    /// 圧縮軸力 `n_compress` [N]（0 以上。引張は呼び出し側で 0 として渡す）から Qy [N] を求める。
    pub(crate) fn qy(&self, n_compress: f64) -> f64 {
        match self {
            DirThreshold::Static(v) => *v,
            DirThreshold::RcArakawa {
                input,
                gross_area,
                steel_qy,
            } => {
                let sigma_0 = if *gross_area > 0.0 {
                    n_compress / gross_area
                } else {
                    0.0
                };
                let mut inp = *input;
                inp.sigma_0 = sigma_0;
                rc_qsu_simple(&inp) + steel_qy
            }
        }
    }
}

/// せん断降伏耐力 Qy 算定対象の方向（局所座標系。せい方向＝ローカル y）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShearDir {
    /// 局所 y 方向（強軸曲げ＝Mz 面に伴うせん断、`Section.as_z` 対応）。
    Y,
    /// 局所 z 方向（弱軸曲げ＝My 面に伴うせん断、`Section.as_y` 対応）。
    Z,
}

fn real_rebar_capacity_input(
    shape: &SectionShape,
    mat: &Material,
    rebar_mat: Option<(&Material, Option<&str>)>,
    shear_mat: Option<&Material>,
    dir: ShearDir,
    clear_span: f64,
) -> Option<RcCapacityInput> {
    let (b, d, steel_candidates, pw) = match shape {
        SectionShape::RcBeamRect { b, d, rebar }
        | SectionShape::SrcBeamRect { b, d, rebar, .. } => {
            if dir != ShearDir::Y {
                return None;
            }
            let bottom = rebar.bending_steel(*d, false).tension;
            let top = rebar.bending_steel(*d, true).tension;
            (
                *b,
                *d,
                vec![
                    (bottom.area_mm2, bottom.effective_depth_mm),
                    (top.area_mm2, top.effective_depth_mm),
                ],
                rebar.pw(*b),
            )
        }
        SectionShape::RcColumnRect { b, d, rebar }
        | SectionShape::SrcColumnRect { b, d, rebar, .. } => {
            let edge = rebar.edge_steel(
                if dir == ShearDir::Y {
                    sepika_core::rc_rebar_geom::RectEdge::Top
                } else {
                    sepika_core::rc_rebar_geom::RectEdge::Left
                },
                *b,
                *d,
            );
            let (width, depth, pw) = if dir == ShearDir::Y {
                (
                    *b,
                    *d,
                    if rebar.hoop.pitch > 0.0 {
                        rebar.aw_x_mm2() / (*b * rebar.hoop.pitch)
                    } else {
                        0.0
                    },
                )
            } else {
                (
                    *d,
                    *b,
                    if rebar.hoop.pitch > 0.0 {
                        rebar.aw_y_mm2() / (*d * rebar.hoop.pitch)
                    } else {
                        0.0
                    },
                )
            };
            (
                width,
                depth,
                vec![(edge.area_mm2, edge.effective_depth_mm)],
                pw,
            )
        }
        SectionShape::RcColumnCircle { d, rebar } => {
            let side = rebar.equivalent_square_side_mm(*d);
            (
                side,
                side,
                vec![(
                    rebar.equivalent_tension_area_mm2(),
                    rebar.equivalent_effective_depth_mm(*d),
                )],
                rebar.pw(side),
            )
        }
        _ => return None,
    };
    let fc = mat.fc?;
    if b <= 0.0 || d <= 0.0 {
        return None;
    }
    let input_for = |(at, d_eff)| RcCapacityInput {
        b,
        d,
        at,
        d_eff,
        sigma_y: sepika_core::material_grade::rebar_yield_strength(rebar_mat.map(|(mat, _)| mat))
            .or(mat.fy)
            .unwrap_or(345.0)
            * rebar_mat
                .map(|(mat, grade)| material_strength_factor_rebar(mat, grade))
                .unwrap_or(1.1),
        fc,
        pw,
        sigma_wy: sepika_core::material_grade::shear_rebar_yield_strength(shear_mat)
            .unwrap_or(sepika_core::material_grade::SHEAR_REBAR_DEFAULT_FY),
        clear_span,
        sigma_0: 0.0,
    };
    steel_candidates
        .into_iter()
        .filter(|(at, d_eff)| *at > 0.0 && *d_eff > 0.0)
        .map(input_for)
        .min_by(|a, b| {
            rc_qsu_simple(a)
                .partial_cmp(&rc_qsu_simple(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn src_steel_shear_capacities(
    shape: Option<&SectionShape>,
    steel: Option<&Material>,
) -> Result<[f64; 2], String> {
    let Some(
        SectionShape::SrcBeamRect {
            steel_height: height_mm,
            steel_width: width_mm,
            steel_web_thick: tw_mm,
            steel_flange_thick: tf_mm,
            ..
        }
        | SectionShape::SrcColumnRect {
            steel_height: height_mm,
            steel_width: width_mm,
            steel_web_thick: tw_mm,
            steel_flange_thick: tf_mm,
            ..
        },
    ) = shape
    else {
        return Ok([0.0; 2]);
    };

    let steel = steel.ok_or("内蔵鉄骨の材料が未割当、または単一の参照先を確定できません")?;
    let tw_mm = *tw_mm;
    let tf_mm = *tf_mm;
    // 不完全な SM520 の種類記号でも、既知の板厚範囲外を先に診断する。
    if steel.category == MaterialCategory::Steel {
        for thickness_mm in [tw_mm, tf_mm] {
            if let Some(reason) = steel_f_value_issue(&steel.name, thickness_mm) {
                return Err(reason);
            }
        }
    }
    let minimum_mm = src_h_steel_minimum_thickness_mm(steel).map_err(|reason| {
        if steel.category != MaterialCategory::Steel || steel_grade_base(&steel.name).is_some() {
            return reason;
        }
        if steel.fy.is_some() {
            format!("{reason}。未登録の材種名に明示 fy が入力されています")
        } else {
            format!("{reason}。未登録の材種名で、明示 fy も未入力です")
        }
    })?;
    validate_src_h_steel_thickness(&steel.name, tw_mm, tf_mm, minimum_mm)?;
    let web_f_n_mm2 = resolve_steel_f(Some(steel), Some(tw_mm))?;
    let flange_f_n_mm2 = resolve_steel_f(Some(steel), Some(tf_mm))?;

    let factor = material_strength_factor_steel(steel);
    let divisor = 3.0_f64.sqrt();
    let web_area_mm2 = (tw_mm * (height_mm - 2.0 * tf_mm)).max(0.0);
    let flange_area_mm2 = (2.0 * width_mm * tf_mm).max(0.0);
    Ok([
        web_area_mm2 * web_f_n_mm2 * factor / divisor,
        flange_area_mm2 * flange_f_n_mm2 * factor / divisor,
    ])
}

/// 方向別のせん断降伏耐力しきい値（[`DirThreshold`]）を組み立てる。
#[derive(Clone, Copy)]
pub(crate) struct SecMaterials<'a> {
    pub material: Option<&'a Material>,
    pub rebar_mat: Option<(&'a Material, Option<&'a str>)>,
    pub shear_mat: Option<&'a Material>,
    #[cfg(test)]
    pub steel_mat: Option<&'a Material>,
}

/// 断面形状から精算できる場合は [`DirThreshold::RcArakawa`]、他は [`DirThreshold::Static`] とする。
fn build_dir_threshold(
    as_area: f64,
    mats: SecMaterials<'_>,
    section: Option<&Section>,
    dir: ShearDir,
    clear_span: f64,
    steel_qy: f64,
) -> DirThreshold {
    let SecMaterials {
        material,
        rebar_mat,
        shear_mat,
        ..
    } = mats;
    if as_area <= 0.0 {
        return DirThreshold::Static(f64::INFINITY);
    }
    let Some(mat) = material else {
        return DirThreshold::Static(f64::INFINITY);
    };
    if let Some(shape) = section.and_then(|s| s.shape.as_ref()) {
        if let Some(input) =
            real_rebar_capacity_input(shape, mat, rebar_mat, shear_mat, dir, clear_span)
        {
            if rc_qsu_simple(&input) + steel_qy > 0.0 {
                return DirThreshold::RcArakawa {
                    gross_area: input.b * input.d,
                    input,
                    steel_qy,
                };
            }
        }
    }
    if let Some(fy) = mat.fy {
        return DirThreshold::Static(
            as_area * fy * material_strength_factor_steel(mat) / 3.0_f64.sqrt(),
        );
    }
    let Some(fc) = mat.fc else {
        return DirThreshold::Static(f64::INFINITY);
    };
    DirThreshold::Static(as_area * 0.7 * fc.sqrt())
}

/// せん断降伏耐力 Qy [N] を算定する。
///
/// 軸力なし（σ0=0）の静的評価。
#[cfg(test)]
pub(crate) fn compute_shear_yield_qy(
    as_area: f64,
    mats: SecMaterials<'_>,
    section: Option<&Section>,
    dir: ShearDir,
    clear_span: f64,
) -> f64 {
    let [steel_y, steel_z] =
        src_steel_shear_capacities(section.and_then(|s| s.shape.as_ref()), mats.steel_mat).unwrap();
    let steel_qy = match dir {
        ShearDir::Y => steel_y,
        ShearDir::Z => steel_z,
    };
    build_dir_threshold(as_area, mats, section, dir, clear_span, steel_qy).qy(0.0)
}

/// 部材長（節点間距離）[mm]。節点参照が欠落・退化の場合は None。
fn elem_length(model: &Model, elem: &ElementData) -> Option<f64> {
    let len = model.member_length(elem);
    (len > 0.0).then_some(len)
}

pub(super) fn resolve_src_steel_shear_capacities(model: &Model) -> Result<Vec<[f64; 2]>, String> {
    model
        .elements
        .iter()
        .map(|elem| {
            if sepika_element::frame::beam::is_elastic_wall_girder(model, elem) {
                return Ok([0.0; 2]);
            }
            let section = model.element_section(elem);
            let shape = section.and_then(|section| section.shape.as_ref());
            let steel = model.element_steel_material(elem);
            src_steel_shear_capacities(shape, steel).map_err(|reason| {
                let references =
                    section
                        .and_then(|section| {
                            model.stb_strengths.sections.iter().find(|input| {
                                input.section == section.id && !input.steel.is_empty()
                            })
                        })
                        .filter(|_| steel.is_none())
                        .map(|input| {
                            let grades: Vec<_> = input
                                .steel
                                .iter()
                                .map(|steel| steel.strength.as_str())
                                .collect();
                            format!("（外部入力の材種: {}）", grades.join("、"))
                        })
                        .unwrap_or_default();
                format!(
                    "部材 ID {} のせん断降伏耐力: {reason}{references}",
                    elem.id.0
                )
            })
        })
        .collect()
}

pub(super) fn build_shear_yield_thresholds(
    model: &Model,
    src_steel_capacities: Vec<[f64; 2]>,
) -> Vec<ShearThreshold> {
    model
        .elements
        .iter()
        .zip(src_steel_capacities)
        .map(|(elem, [steel_y, steel_z])| {
            if sepika_element::frame::beam::is_elastic_wall_girder(model, elem) {
                return ShearThreshold {
                    y: DirThreshold::Static(f64::INFINITY),
                    z: DirThreshold::Static(f64::INFINITY),
                };
            }
            let sec = model.element_section(elem);
            let mats = SecMaterials {
                material: model.element_material(elem),
                rebar_mat: model.element_rebar_material_with_grade(elem),
                shear_mat: model.element_shear_rebar_material(elem),
                #[cfg(test)]
                steel_mat: None,
            };
            let (as_y, as_z) = sec.map(|s| (s.as_y, s.as_z)).unwrap_or((0.0, 0.0));
            let raw_length = elem_length(model, elem).unwrap_or(0.0);
            let clear_span = model
                .analysis_rigid_zone(elem)
                .flexible_length_from(raw_length);
            ShearThreshold {
                y: build_dir_threshold(as_z, mats, sec, ShearDir::Y, clear_span, steel_y),
                z: build_dir_threshold(as_y, mats, sec, ShearDir::Z, clear_span, steel_z),
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn compute_shear_yield_thresholds(model: &Model) -> Result<Vec<ShearThreshold>, String> {
    Ok(build_shear_yield_thresholds(
        model,
        resolve_src_steel_shear_capacities(model)?,
    ))
}

/// せん断降伏イベントの追跡（曲げとは独立の判定）。
pub(crate) fn track_shear_yield(
    model: &Model,
    behaviors: &[Box<dyn ElementBehavior>],
    responses: &[Option<LoadedFrameResponse>],
    thresholds: &[ShearThreshold],
    step: u32,
    events: &mut Vec<ShearYieldEvent>,
) {
    let ctx = Ctx { model };
    for (i, ((elem, b), response)) in model
        .elements
        .iter()
        .zip(behaviors)
        .zip(responses)
        .enumerate()
    {
        if sepika_element::frame::beam::is_elastic_wall_girder(model, elem) || elem.nodes.len() != 2
        {
            continue;
        }
        let (Some(pi), Some(pj)) = (
            model.nodes.get(elem.nodes[0].index()),
            model.nodes.get(elem.nodes[1].index()),
        ) else {
            continue;
        };
        if elem_length(model, elem).is_none() {
            continue;
        }
        let frame = member_local_frame(elem, pi.coord, pj.coord)
            .unwrap_or_else(|reason| panic!("部材 ID {} の局所座標: {reason}", elem.id.0));
        let ex = frame.rot[0];
        let ey = frame.rot[1];
        let ez = frame.rot[2];

        let f = recording_force(b.as_ref(), response.as_ref(), &ctx);
        let f_i = [f.data[0], f.data[1], f.data[2]];
        let f_j = [f.data[6], f.data[7], f.data[8]];
        let vy = dot3(f_i, ey).abs().max(dot3(f_j, ey).abs());
        let vz = dot3(f_i, ez).abs().max(dot3(f_j, ez).abs());
        let n_compress = axial_compression(f_i, f_j, ex);

        let th = &thresholds[i];
        let qy_y = th.y.qy(n_compress);
        let qy_z = th.z.qy(n_compress);
        if vy >= qy_y || vz >= qy_z {
            events.push(ShearYieldEvent {
                step,
                elem: elem.id,
            });
        }
    }
}

#[cfg(test)]
mod ratio_contract_tests {
    use super::*;

    #[test]
    fn shear_factory_uses_web_width_effective_depth_and_decimal_hoop_ratio() {
        use sepika_core::ids::MaterialId;
        use sepika_core::model::MaterialCategory;
        use sepika_core::section_shape::{BeamStirrup, RcBeamRebar};
        let main_dia = (2400.0 / std::f64::consts::PI).sqrt();
        let hoop_dia = (120.0 / std::f64::consts::PI).sqrt();
        let shape = SectionShape::RcBeamRect {
            b: 300.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: sepika_core::section_shape::RebarSize::DiameterMm(main_dia),
                top: vec![3],
                bottom: vec![3],
                cover: 60.0 - hoop_dia - main_dia / 2.0,
                stirrup: BeamStirrup {
                    dia: sepika_core::section_shape::RebarSize::DiameterMm(hoop_dia),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        };
        let mat = Material {
            id: MaterialId(0),
            name: "FC24".into(),
            category: MaterialCategory::Concrete,
            young: 20_000.0,
            poisson: 0.2,
            density: 0.0,
            shear: None,
            fc: Some(24.0),
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        };
        let input =
            real_rebar_capacity_input(&shape, &mat, None, None, ShearDir::Y, 3000.0).unwrap();
        assert!((input.at - 1800.0).abs() < 1e-9);
        assert_eq!(input.b, 300.0);
        assert_eq!(input.d, 600.0);
        assert!((input.d_eff - 540.0).abs() < 1e-12);
        assert!((input.pw - 0.002).abs() < 1e-15);
        assert!((rc_qsu_simple(&input) - 235_681.416_383_415_87).abs() < 1e-8);
    }
}
