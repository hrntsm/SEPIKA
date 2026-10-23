//! 耐震壁（壁エレメントモデル）要素。

use crate::behavior::{Ctx, ElementBehavior, LocalMat, LocalVec, MassOption};
use crate::frame::beam::BeamElement;
use crate::transform::LocalFrame;
use sepika_core::dof::DofMap;
use sepika_core::geom::vec3::{sub, unit};
use sepika_core::ids::NodeId;
use sepika_core::model::{ElementData, HysteresisModel, Model};
use sepika_core::section_shape::{SectionShape, E_STEEL, KAPPA_RC};
use smallvec::SmallVec;

/// 耐震壁（壁エレメントモデル）。
pub struct WallElement {
    /// [下辺a, 下辺b, 上辺a, 上辺b]（a→b が剛梁の軸方向。上下で対応付け済み）
    nodes: [NodeId; 4],
    /// 壁柱（仮想中央柱。上下剛梁の中点を結ぶ）
    column: BeamElement,
    /// 壁柱端 12 自由度 ← 四隅 24 自由度 の変換行列 A（row-major 12×24）。
    /// 四隅の回転自由度に対応する列は常に 0（ピン）。
    a_mat: Vec<f64>,
    /// 質量算定用の壁板総質量 [質量単位]
    mass_total: f64,
    /// 確定変位（四隅 24 自由度、グローバル系）。commit_state で trial から確定。
    committed_disp: [f64; 24],
    /// トライアル変位（四隅 24 自由度、グローバル系）。Newton 反復中も蓄積され、
    /// internal_force はこちらを参照する。
    trial_disp: [f64; 24],
    /// 面内せん断終局強度 [N]。正・負の順の正値。いずれか0以下なら線形弾性。
    qu_shear: [f64; 2],
    /// 面内せん断モードベクトル p（24 自由度）。上辺 2 節点の並進を壁面内水平方向
    /// `ex_bottom` へ 1.0 ずつ与えたもの。`pᵀ·f` は上辺が伝達する面内水平力に等しく、
    /// `u − γp·p` で塑性すべりを差し引く（下記 [`WallElement::shear_return_map`]）。
    shear_mode: [f64; 24],
    /// 確定塑性せん断すべり γp [mm]。
    committed_slip: f64,
    /// トライアル塑性せん断すべり γp [mm]。
    trial_slip: f64,
    /// 面内せん断の復元力ばね。`None` は弾完全塑性リターンマッピング。
    shear_spring: Option<Box<dyn sepika_material::UniaxialMaterial>>,
    /// せん断ばねの変形測度 D = γp + Q/k_s0 に用いる弾性モード剛性
    /// k_s0 = pᵀ·K_elastic·p [N/mm]（ばね骨格の初期剛性と共有）。
    shear_k0: f64,
    /// 壁柱の軸・曲げの弾塑性評価（ファイバー断面＋塑性増分ヒンジ）。
    /// `Some` のとき軸・曲げの応答（剛性・内力）はこのファイバー壁柱から得る。
    fiber_column: Option<crate::frame::fiber::FiberBeam>,
    /// ファイバー壁柱へ与え済みの壁柱端変位（グローバル系 12）。トライアル/確定。
    fiber_u12_trial: [f64; 12],
    fiber_u12_committed: [f64; 12],
}

pub use sepika_core::model::{wall_element_geometry, WallElementGeometry};

/// 増分解析で壁柱がファイバー化されるときの塑性化域長 Lp `[mm]`。
/// ファイバー化されない壁（耐震壁不成立・Qu を算定できない・Fc 未設定など）は `None`。
pub fn wall_column_fiber_lp(data: &ElementData, model: &Model) -> Option<f64> {
    if !crate::wall::misc_wall::wall_is_seismic(data, model) {
        return None;
    }
    WallElement::shear_inputs(data, model, crate::factory::StrengthBasis::Nominal).ok()?;
    let geom = wall_element_geometry(data, model)?;
    model.element_material(data)?.fc.filter(|fc| *fc > 0.0)?;
    let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
    let t = match sec.and_then(|s| s.shape.as_ref()) {
        Some(SectionShape::RcWall { thickness, .. }) => *thickness,
        _ => sec.map(|s| s.thickness.unwrap_or(s.width))?,
    };
    if t <= 0.0 || geom.lw <= 0.0 || geom.h <= 0.0 {
        return None;
    }
    Some(crate::frame::fiber::clamp_plastic_zone(
        0.5 * geom.lw,
        geom.h,
    ))
}

/// 壁要素の面積等価開口寸法 `(l0, h0)` [mm]。
/// 複数開口は面積等価の 1 開口へまとめる（lo = Σli、ho = Σ(li·hi)/lo）。
/// 個別寸法が取れない・開口なしのときは `None`。
fn wall_opening_equiv_dims(data: &ElementData, model: &Model) -> Option<(f64, f64)> {
    model
        .wall_attrs
        .iter()
        .find(|w| w.elem == data.id)
        .and_then(|a| a.opening_dims_for(model.multi_opening_mode))
        .and_then(|dims| {
            let lo: f64 = dims.iter().map(|(l, _)| *l).sum();
            let area: f64 = dims.iter().map(|(l, h)| l * h).sum();
            (lo > 0.0 && area > 0.0).then_some((lo, area / lo))
        })
}

/// 側柱断面の主筋総断面積 at [mm²]。RC 耐力式を適用できない形状は `None`。
///
/// 検証済みの総主筋量を返す。未入力は 0、検証失敗は `None`。
fn side_column_main_area(shape: &SectionShape) -> Option<f64> {
    match shape {
        SectionShape::RcBeamRect { b, d, rebar } => {
            rebar.validate(*b, *d).ok()?;
            Some(rebar.total_main_area())
        }
        SectionShape::RcColumnRect { b, d, rebar } => {
            rebar.validate(*b, *d).ok()?;
            Some(rebar.total_main_area())
        }
        SectionShape::RcColumnCircle { d, rebar } => {
            rebar.validate(*d).ok()?;
            Some(rebar.total_main_area())
        }
        _ => None,
    }
}

/// 耐震壁の幾何・配筋・材料。
struct WallShearGeometry {
    method: sepika_core::model::RcWallShearMethod,
    /// コンクリート設計基準強度 Fc [N/mm²]（未設定は `None`）
    fc: Option<f64>,
    /// 壁厚 t [mm]
    t: f64,
    te: f64,
    sigma_0: f64,
    shear_span_ratio: f64,
    /// 付帯柱中心間距離 lw [mm]
    lw: f64,
    /// 壁の上下梁中心間高さ h [mm]
    h: f64,
    /// 実壁厚に対する横筋比（小数）。
    pwh_ratio: f64,
    pwv_ratio: f64,
    /// 側柱外面間の全長 [mm]。
    d_wall: f64,
    /// 圧縮側柱の壁長方向せい [mm]。
    dc_compression: f64,
    /// 引張側柱の主筋断面積 at [mm²]
    col_main_at: f64,
    /// 側柱（付帯柱）があるか
    has_side_column: bool,
    /// 壁横筋の降伏点 σwh [N/mm²]
    sigma_wh: f64,
    /// 開口寸法 `(l0, h0)` [mm]（無開口は `None`）
    opening: Option<(f64, f64)>,
}

fn wall_input_error(data: &ElementData, role: &str, reason: impl std::fmt::Display) -> String {
    format!(
        "RC壁 要素ID {}・断面ID {:?} の{role}: {reason}",
        data.id.0,
        data.section.map(|s| s.0)
    )
}

fn wall_direction_ratio(data: &ElementData, model: &Model, vertical: bool) -> Result<f64, String> {
    let role = if vertical { "縦筋比" } else { "横筋比" };
    let value = match model.element_section(data).and_then(|s| s.shape.as_ref()) {
        Some(SectionShape::RcWall {
            pwv_ratio,
            pwh_ratio,
            ..
        }) => {
            if vertical {
                *pwv_ratio
            } else {
                *pwh_ratio
            }
        }
        _ => None,
    };
    value.filter(|v| v.is_finite() && *v >= 0.0).ok_or_else(|| {
        wall_input_error(
            data,
            role,
            "明示した有限な非負値が必要です（psから補完できません）",
        )
    })
}

fn wall_rebar_strength(
    data: &ElementData,
    model: &Model,
    vertical: bool,
    basis: crate::factory::StrengthBasis,
) -> Result<f64, String> {
    let role = if vertical {
        "縦筋材料"
    } else {
        "横筋材料"
    };
    let mat = if vertical {
        model.element_rebar_material(data)
    } else {
        model.element_shear_rebar_material(data)
    }
    .ok_or_else(|| {
        wall_input_error(
            data,
            role,
            "材料参照が未設定・破損、または元部位の方向を解決できません",
        )
    })?;
    if model
        .materials
        .get(mat.id.index())
        .is_none_or(|stored| !std::ptr::eq(stored, mat))
    {
        return Err(wall_input_error(
            data,
            role,
            format!("材料ID {}の参照が破損しています", mat.id.0),
        ));
    }
    let supported = if vertical {
        sepika_core::standard_material::rebar_grade_strength(&mat.name).is_some()
    } else {
        sepika_core::material_grade::is_supported_shear_rebar_grade(&mat.name)
    };
    if mat.category != sepika_core::model::MaterialCategory::Rebar || !supported {
        return Err(wall_input_error(
            data,
            role,
            format!(
                "材料ID {}「{}」は対応する鉄筋材種ではありません",
                mat.id.0, mat.name
            ),
        ));
    }
    let fy = mat
        .fy
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| {
            wall_input_error(
                data,
                role,
                format!(
                    "材料ID {}「{}」のfyは明示した有限正値が必要です",
                    mat.id.0, mat.name
                ),
            )
        })?;
    let factor = if vertical {
        basis.rebar_factor(Some(mat))
    } else {
        1.0
    };
    let value = fy * factor;
    if !factor.is_finite() || factor <= 0.0 || !value.is_finite() || value <= 0.0 {
        return Err(wall_input_error(
            data,
            role,
            format!("材料ID {}の強度倍率は有限正値が必要です", mat.id.0),
        ));
    }
    Ok(value)
}

impl WallElement {
    /// 生成。4 節点未満・寸法/断面が不定の場合は None。
    pub fn try_new(data: &ElementData, model: &Model) -> Option<Self> {
        Self::try_new_scaled(data, model, 1.0)
    }

    /// 剛性スケール付き生成。`stiffness_scale` で壁要素の剛性をスケールする。
    pub(crate) fn try_new_scaled(
        data: &ElementData,
        model: &Model,
        stiffness_scale: f64,
    ) -> Option<Self> {
        let geom = wall_element_geometry(data, model)?;
        let (ids_b0, ids_b1) = (geom.bottom[0], geom.bottom[1]);
        let (ids_ta, ids_tb) = (geom.top[0], geom.top[1]);
        let coord_of =
            |nid: NodeId| -> Option<[f64; 3]> { model.nodes.get(nid.index()).map(|n| n.coord) };
        let ex_bot = geom.ex_bottom;
        let ex_top = unit(sub(coord_of(ids_tb)?, coord_of(ids_ta)?))?;
        let (bc, tc) = (geom.bottom_center, geom.top_center);
        let h = geom.h;
        let lw = geom.lw;

        let sec = data
            .section
            .and_then(|sid| model.sections.get(sid.index()))?;
        let t = match &sec.shape {
            Some(SectionShape::RcWall { thickness, .. }) => *thickness,
            _ => sec.thickness.unwrap_or(sec.width),
        };
        if t <= 0.0 {
            return None;
        }
        let mat = model.element_material(data)?;
        if !mat.density.is_finite() || mat.density < 0.0 {
            return None;
        }
        let is_rc_wall = matches!(&sec.shape, Some(SectionShape::RcWall { .. }));
        if is_rc_wall && !mat.fc.is_some_and(|fc| fc.is_finite() && fc > 0.0) {
            return None;
        }
        let young = mat.young;
        let r = crate::factory::wall_opening_reduction(data, model).max(1e-6);

        let ps = match &sec.shape {
            Some(SectionShape::RcWall { ps, .. }) => (*ps).max(0.0),
            _ => 0.0,
        };
        let rebar_factor = if mat.fc.is_some() && young > 0.0 && ps > 0.0 {
            1.0 + (E_STEEL / young - 1.0) * ps
        } else {
            1.0
        };

        let shear_rigidity = super::shear_section::wall_shear_rigidity(data, model).ok()?;
        let area = t * lw;
        let rc_density = mat
            .fc
            .filter(|fc| fc.is_finite() && *fc > 0.0)
            .map(|fc| {
                sepika_core::units::to_internal::mass_density_from_unit_weight_kn_m3(
                    sepika_core::units::concrete_unit_weight_kn_m3(
                        fc,
                        mat.concrete_class,
                        sepika_core::units::ConcreteComposition::Rc,
                    ),
                )
            })
            .unwrap_or(mat.density);
        let rc_mass_properties_valid = !is_rc_wall
            || sepika_core::model::validate_section_materials(sec, Some(mat), None, None, None)
                .is_ok();
        let section_mass_properties = if is_rc_wall && rc_mass_properties_valid {
            sepika_core::model::SectionMassProperties::uniform(
                rc_density,
                area,
                lw * t.powi(3) / 12.0,
                t * lw.powi(3) / 12.0,
            )
        } else {
            sepika_core::model::SectionMassProperties::uniform(
                mat.density,
                area,
                lw * t.powi(3) / 12.0,
                t * lw.powi(3) / 12.0,
            )
        };
        let column = BeamElement {
            id: data.id,
            e: young * stiffness_scale,
            g: mat
                .shear
                .filter(|shear| shear.is_finite() && *shear > 0.0)
                .unwrap_or_else(|| young / (2.0 * (1.0 + mat.poisson)))
                * stiffness_scale,
            a: area * rebar_factor,
            iy: lw * t.powi(3) / 12.0,
            iz: t * lw.powi(3) / 12.0 * rebar_factor,
            j: lw * t.powi(3) / 3.0,
            as_y: r * shear_rigidity
                / mat
                    .shear
                    .filter(|shear| shear.is_finite() && *shear > 0.0)
                    .unwrap_or_else(|| young / (2.0 * (1.0 + mat.poisson))),
            as_z: r * area / KAPPA_RC,
            length: h,
            mass_properties: section_mass_properties,
            mass_properties_error: (!rc_mass_properties_valid)
                .then(|| "RC壁の整合質量には正の Fc が必要です".to_string()),
            nodes: [ids_b0, ids_ta],
            axis: LocalFrame::from_nodes(bc, tc, ex_bot)
                .expect("壁幾何から検証済みの仮想柱局所基底"),
            rigid: Default::default(),
            end_cond: [
                sepika_core::model::EndCondition::Fixed,
                sepika_core::model::EndCondition::Fixed,
            ],
            torsion_release: [false, false],
            eval_sections: vec![0.0, 0.5, 1.0],
            section: data.section,
            material: model.element_section(data).and_then(|s| s.material),
            committed_disp: [0.0; 12],
            trial_disp: [0.0; 12],
            committed_member_loads: Vec::new(),
            trial_member_loads: Vec::new(),
            local_stiffness_cache: std::sync::OnceLock::new(),
        };

        let mut a_mat = vec![0.0; 12 * 24];
        let corner_slot = |idx: usize| -> usize { idx * 6 };
        let node_order = [ids_b0, ids_b1, ids_ta, ids_tb];
        let slot_of = |orig: NodeId| -> usize {
            node_order
                .iter()
                .position(|&x| x == orig)
                .expect("node_order は 4 節点の並べ替え")
        };
        let mut fill_end = |col_base: usize, ca: NodeId, cb: NodeId, ex: [f64; 3], lw_e: f64| {
            let (sa, sb) = (corner_slot(slot_of(ca)), corner_slot(slot_of(cb)));
            for tdof in 0..3 {
                a_mat[(col_base + tdof) * 24 + sa + tdof] += 0.5;
                a_mat[(col_base + tdof) * 24 + sb + tdof] += 0.5;
            }
            for i in 0..3 {
                for j in 0..3 {
                    for k in 0..3 {
                        let e = levi_civita(i, j, k);
                        if e == 0.0 {
                            continue;
                        }
                        let c = e * ex[j] / lw_e;
                        a_mat[(col_base + 3 + i) * 24 + sb + k] += c;
                        a_mat[(col_base + 3 + i) * 24 + sa + k] -= c;
                    }
                }
            }
        };
        fill_end(0, ids_b0, ids_b1, ex_bot, geom.lw_bottom);
        fill_end(6, ids_ta, ids_tb, ex_top, geom.lw_top);

        let mut shear_mode = [0.0; 24];
        for slot in [2usize, 3usize] {
            for k in 0..3 {
                shear_mode[slot * 6 + k] = ex_bot[k];
            }
        }

        Some(Self {
            nodes: [ids_b0, ids_b1, ids_ta, ids_tb],
            column,
            a_mat,
            mass_total: {
                let origin = model.generated_wall_origins.get(&data.id);
                let plate = origin.and_then(|id| model.wall_plate(*id));
                if origin.is_some() && plate.is_none() {
                    return None;
                }
                if let Some(plate) = plate {
                    model.wall_weight(plate).ok()?.totals.matrix_n
                        / sepika_core::units::GRAVITY_MM_S2
                } else {
                    let attr = model.wall_attrs.iter().find(|a| a.elem == data.id);
                    let opening_area = attr.map(|a| a.total_opening_area()).unwrap_or(0.0);
                    let opening_weight = attr.map(|a| a.opening_weight).unwrap_or(0.0);
                    let boundary = [geom.bottom[0], geom.bottom[1], geom.top[1], geom.top[0]];
                    let points: Vec<_> = boundary
                        .iter()
                        .map(|n| model.nodes[n.index()].coord)
                        .collect();
                    let dimensions = std::array::from_fn(|i| {
                        let a = boundary[i];
                        let b = boundary[(i + 1) % 4];
                        model
                            .elements
                            .iter()
                            .find(|e| {
                                e.kind.is_primary_frame_support()
                                    && e.nodes.len() >= 2
                                    && ((e.nodes[0] == a && e.nodes[e.nodes.len() - 1] == b)
                                        || (e.nodes[0] == b && e.nodes[e.nodes.len() - 1] == a))
                            })
                            .and_then(|e| model.element_section(e))
                            .map(|s| [s.width, s.depth])
                    });
                    let area = sepika_core::geom::polygon::area_3d(&points)
                        * sepika_core::model::wall_clear_area_factor(&points, &dimensions);
                    let net_area = (area - opening_area).max(0.0);
                    let mass_per_area = if is_rc_wall {
                        rc_density * t
                    } else {
                        section_mass_properties.mass_per_length / lw
                    };
                    (mass_per_area * net_area + opening_weight / sepika_core::units::GRAVITY_MM_S2)
                        .max(0.0)
                }
            },
            committed_disp: [0.0; 24],
            trial_disp: [0.0; 24],
            qu_shear: [0.0; 2],
            shear_mode,
            committed_slip: 0.0,
            trial_slip: 0.0,
            shear_spring: None,
            shear_k0: 0.0,
            fiber_column: None,
            fiber_u12_trial: [0.0; 12],
            fiber_u12_committed: [0.0; 12],
        })
    }

    /// 壁柱の軸・曲げを、明示縦筋の等価分散配置によるファイバー断面へ切り替える。
    /// 必要な縦筋比・縦筋材料・コンクリート材料が不足する場合は理由を返す。
    pub(crate) fn with_fiber_flexure(
        mut self,
        data: &ElementData,
        model: &Model,
        basis: crate::factory::StrengthBasis,
        kind: sepika_core::model::AnalysisKind,
    ) -> Result<Self, String> {
        let geom = wall_element_geometry(data, model)
            .ok_or_else(|| wall_input_error(data, "幾何", "壁寸法を解決できません"))?;
        let mat = model.element_material(data).ok_or_else(|| {
            wall_input_error(data, "コンクリート材料", "材料参照が未設定・破損です")
        })?;
        if mat.category != sepika_core::model::MaterialCategory::Concrete {
            return Err(wall_input_error(
                data,
                "コンクリート材料",
                "コンクリート材種が必要です",
            ));
        }
        let fc = mat
            .fc
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| wall_input_error(data, "コンクリート材料", "有限正Fcが必要です"))?;
        let t = match model.element_section(data).and_then(|s| s.shape.as_ref()) {
            Some(SectionShape::RcWall { thickness, .. }) => *thickness,
            _ => return Err(wall_input_error(data, "断面", "明示RC壁形状が必要です")),
        };
        let pwv_ratio = wall_direction_ratio(data, model, true)?;
        let rebar_fy = wall_rebar_strength(data, model, true, basis)?;
        let lw = 0.5 * (geom.lw_bottom + geom.lw_top);
        let h = self.column.length;
        if !t.is_finite() || t <= 0.0 || lw <= 0.0 || h <= 0.0 {
            return Err(wall_input_error(
                data,
                "幾何",
                "有限正の壁厚・壁長・高さが必要です",
            ));
        }

        let rebar_factor = 1.0 + (E_STEEL / mat.young - 1.0) * pwv_ratio;
        self.column.a = t * lw * rebar_factor;
        self.column.iz = t * lw.powi(3) / 12.0 * rebar_factor;
        self.column.local_stiffness_cache = std::sync::OnceLock::new();
        let nw = 4;
        let nd = 20;
        let concrete_rule = crate::factory::resolve_wall_concrete_hysteresis(data, model, kind);
        let make_section = || {
            let (mut section, mut mats) = crate::frame::fiber::build_gauss_fibers(
                t,
                lw,
                nw,
                nd,
                None,
                Some(fc),
                self.column.e,
                self.column.e,
                crate::frame::fiber::FiberYield::default(),
                1.0,
                1.0,
                sepika_section::mn_surface::StrengthParams::default(),
                concrete_rule,
            )
            .expect("形状なしの壁ファイバは実配筋を要求しない");
            if pwv_ratio > 0.0 {
                let a_each = pwv_ratio * t * lw / nd as f64;
                for i in 0..nd {
                    let y = ((i as f64 + 0.5) / nd as f64 - 0.5) * lw;
                    section.fibers.push(sepika_section::fiber::Fiber {
                        y,
                        z: 0.0,
                        area: a_each,
                        material: 1,
                    });
                    mats.push(crate::frame::fiber::steel_fiber_material(
                        E_STEEL,
                        Some(rebar_fy),
                    ));
                }
            }
            (section, mats)
        };

        let col = &self.column;
        let mut fiber = crate::frame::fiber::FiberBeam::from_raw_parts(
            col.nodes,
            col.length,
            col.axis,
            col.mass_properties,
            col.e,
            col.g,
            col.a,
            col.iy,
            col.iz,
            col.as_y,
            col.as_z,
            col.j,
            0.5 * lw,
            [make_section(), make_section()],
        );
        fiber.releases = [4, 10]
            .into_iter()
            .map(|dof| crate::frame::fiber::EndRelease { dof, spring: 0.0 })
            .collect();
        fiber.trial_int = smallvec::smallvec![0.0; 2];
        fiber.committed_int = smallvec::smallvec![0.0; 2];
        self.fiber_column = Some(fiber);
        Ok(self)
    }

    /// 選択方式による耐震壁の面内せん断終局強度 Qu [N]。
    /// 開口低減は耐力用 r2 = 1−max(r0, l0/lw, h0/h)（剛性用 r1 = 1−1.25·r0 とは別式）。
    /// 算定できない場合（Fc 未設定など）は 0.0 を返す。
    fn shear_capacity(inp: &WallShearGeometry) -> f64 {
        let &WallShearGeometry {
            method,
            fc,
            t,
            te,
            sigma_0,
            shear_span_ratio,
            lw,
            h,
            pwh_ratio,
            pwv_ratio,
            d_wall,
            dc_compression,
            col_main_at,
            has_side_column,
            sigma_wh,
            opening,
        } = inp;
        let Some(fc) = fc else {
            return 0.0;
        };
        if fc <= 0.0 || t <= 0.0 || lw <= 0.0 || h <= 0.0 {
            return 0.0;
        }
        let d_eff = d_wall - dc_compression / 2.0;
        if d_eff <= 0.0 {
            return 0.0;
        }
        let at = if has_side_column {
            col_main_at
        } else {
            pwv_ratio * t * lw
        };
        if at <= 0.0 {
            return 0.0;
        }
        sepika_core::rc_wall_capacity::wall_shear_ultimate(
            &sepika_core::rc_wall_capacity::RcWallShearInput {
                method,
                fc,
                te,
                t,
                d_wall,
                dc_compression,
                tension_column_at: at,
                sigma_wh,
                pwh_ratio,
                sigma_0,
                shear_span_ratio,
                opening: opening.map(|(l0, h0)| (l0, h0, h, lw)),
            },
        )
    }

    /// 耐力用開口低減率 r2（無開口は 1.0）。
    pub fn opening_strength_reduction(data: &ElementData, model: &Model) -> f64 {
        let Some(geom) = wall_element_geometry(data, model) else {
            return 1.0;
        };
        let opening =
            wall_opening_equiv_dims(data, model).map(|(l0, h0)| (l0, h0, geom.h, geom.lw));
        sepika_core::rc_wall_capacity::wall_opening_reduction_strength(opening)
    }

    /// 静的一次参照の圧縮正N `[N]`と局所正負のM/(QD)からQu `[N]`を求める。
    /// 参照の欠落・非有限・引張N・0.4Fcを超える圧縮は理由付きで拒否する。
    pub fn directional_shear_capacity_at_reference(
        data: &ElementData,
        model: &Model,
        basis: crate::factory::StrengthBasis,
        compression_n: f64,
        span_ratios: [f64; 2],
    ) -> Result<[f64; 2], String> {
        if model.element_section(data).is_none() || wall_element_geometry(data, model).is_none() {
            return Err(wall_input_error(
                data,
                "断面・幾何",
                "必要入力が不足しています",
            ));
        }
        if !crate::wall::misc_wall::is_rc_wall(data, model) {
            return Ok([Self::steel_shear_capacity_of(data, model); 2]);
        }
        let mut inputs = Self::shear_inputs(data, model, basis)?;
        let properties = super::shear_section::WallSection::new(data, model)?.properties()?;
        let fc = inputs[0].fc.ok_or("Fcが不足しています")?;
        let sigma_0 = compression_n / properties.area_mm2;
        if !sigma_0.is_finite()
            || !(0.0..=0.4 * fc).contains(&sigma_0)
            || span_ratios.iter().any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(wall_input_error(
                data,
                "静的一次参照",
                "NまたはM/(QD)が適用範囲外・非有限です",
            ));
        }
        let qu = std::array::from_fn(|branch| {
            inputs[branch].sigma_0 = sigma_0;
            inputs[branch].shear_span_ratio = span_ratios[branch];
            Self::shear_capacity(&inputs[branch])
        });
        if qu.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(wall_input_error(
                data,
                "せん断耐力",
                "全局所分岐のQuは有限正値が必要です",
            ));
        }
        Ok(qu)
    }

    fn shear_inputs(
        data: &ElementData,
        model: &Model,
        basis: crate::factory::StrengthBasis,
    ) -> Result<[WallShearGeometry; 2], String> {
        let Some(geom) = wall_element_geometry(data, model) else {
            return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
        };
        if model.element_section(data).is_none() {
            return Err(wall_input_error(data, "断面", "断面が設定されていません"));
        }
        if !crate::wall::misc_wall::is_rc_wall(data, model) {
            return Err(wall_input_error(data, "断面", "RC壁以外は対象外です"));
        }
        let Some(sec) = model.element_section(data) else {
            return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
        };
        if Some(sec.id) != data.section {
            return Err(wall_input_error(data, "断面", "参照IDが破損しています"));
        }
        let t = match sec.shape.as_ref() {
            Some(SectionShape::RcWall { thickness, .. }) => *thickness,
            _ => return Err(wall_input_error(data, "断面", "明示RC壁形状が必要です")),
        };
        let pwh_ratio = wall_direction_ratio(data, model, false)?;
        let fc = model
            .element_material(data)
            .filter(|m| m.category == sepika_core::model::MaterialCategory::Concrete)
            .and_then(|m| m.fc)
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| wall_input_error(data, "コンクリート材料", "有限正Fcが必要です"))?;

        let Ok(section) = super::shear_section::WallSection::new(data, model) else {
            return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
        };
        let mut at = [0.0; 2];
        let mut depth = [0.0; 2];
        for (side, col) in section.columns.iter().enumerate() {
            if let Some(col) = col {
                let Some(shape) = model
                    .element_section(&model.elements[col.element_index])
                    .and_then(|sec| sec.shape.as_ref())
                else {
                    return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
                };
                let Some(area) = side_column_main_area(shape) else {
                    return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
                };
                at[side] = area;
                if at[side] <= 0.0 {
                    return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
                }
                depth[side] = col.extent[1] - col.extent[0];
            }
        }
        let d_wall = geom.lw + depth.iter().sum::<f64>() / 2.0;
        let Ok(properties) = section.properties() else {
            return Err(wall_input_error(data, "せん断耐力", "幾何・断面・側柱配筋を解決できません（主筋を明示したRC矩形・円形側柱が必要です。SRC・CFT・鋼材側柱は適用外です）"));
        };
        let te = (properties.area_mm2 / d_wall).min(1.5 * t);
        let sigma_wh = wall_rebar_strength(data, model, false, basis)?;
        let pwv_ratio = if section.columns.iter().any(Option::is_none) {
            wall_direction_ratio(data, model, true)?
        } else {
            0.0
        };
        let inputs = std::array::from_fn(|tension| WallShearGeometry {
            method: model.stress_cfg.rc_wall_shear_method,
            fc: Some(fc),
            t,
            te,
            sigma_0: 0.0,
            shear_span_ratio: 0.0,
            lw: geom.lw,
            h: geom.h,
            pwh_ratio,
            pwv_ratio,
            d_wall,
            dc_compression: depth[1 - tension],
            col_main_at: at[tension],
            has_side_column: section.columns[tension].is_some(),
            sigma_wh,
            opening: wall_opening_equiv_dims(data, model),
        });
        if inputs.iter().any(|input| {
            let at = if input.has_side_column {
                input.col_main_at
            } else {
                input.pwv_ratio * input.t * input.lw
            };
            !at.is_finite() || at <= 0.0 || input.d_wall - input.dc_compression / 2.0 <= 0.0
        }) {
            return Err(wall_input_error(
                data,
                "せん断耐力",
                "主筋量または有効断面の必要入力が不成立です",
            ));
        }
        Ok(inputs)
    }

    /// 鋼板耐震壁の面内せん断終局強度 Qy `[N]`。
    ///
    /// 鋼板のせん断降伏で決まるものとし、von Mises の降伏条件による純せん断の
    /// 降伏せん断応力度 τy = F/√3 を全断面 t·lw に乗じる。
    ///
    /// ```text
    /// Qy = t · lw · F / √3
    /// ```
    ///
    /// F は材料の降伏強度 `Material.fy` を用いる。
    /// せん断座屈は考慮していない。
    pub fn steel_shear_capacity_of(data: &ElementData, model: &Model) -> f64 {
        let Some(geom) = wall_element_geometry(data, model) else {
            return 0.0;
        };
        let Some(sec) = data.section.and_then(|sid| model.sections.get(sid.index())) else {
            return 0.0;
        };
        let t = sec.thickness.unwrap_or(sec.width);
        let f = model
            .element_material(data)
            .and_then(|m| m.fy)
            .unwrap_or(0.0);
        if t <= 0.0 || geom.lw <= 0.0 || f <= 0.0 {
            return 0.0;
        }
        t * geom.lw * f / 3.0_f64.sqrt()
    }

    /// RC壁の耐力式を適用できない側柱・配筋・材料・幾何の不備を返す。
    pub fn wall_shear_capacity_issue(
        data: &ElementData,
        model: &Model,
        basis: crate::factory::StrengthBasis,
    ) -> Option<String> {
        if !matches!(data.kind, sepika_core::model::ElementKind::Wall) {
            return None;
        }
        match wall_element_geometry(data, model) {
            Some(_) => (),
            None if data.nodes.len() >= 4 => {
                return Some(format!(
                    "耐震壁 ID {} を壁エレメントとして構築できません（4 節点の指定と節点座標を確認してください）。解析を開始できません。",
                    data.id.0
                ));
            }
            None => return None,
        };
        if !crate::wall::misc_wall::wall_is_seismic(data, model) {
            return None;
        }
        if crate::wall::misc_wall::is_rc_wall(data, model) {
            return Self::shear_inputs(data, model, basis)
                .and_then(|_| {
                    wall_direction_ratio(data, model, true)?;
                    wall_rebar_strength(data, model, true, basis)
                })
                .err();
        }
        let Some(sec) = data.section.and_then(|sid| model.sections.get(sid.index())) else {
            return Some(format!(
                "耐震壁 ID {} に断面が設定されていません。\
                 断面タブで壁厚・壁筋比を設定してください。\
                 保有水平耐力計算では耐震壁の終局せん断強度が必要です。",
                data.id.0
            ));
        };
        let t = sec.thickness.unwrap_or(sec.width);
        if t <= 0.0 {
            return Some(format!(
                "耐震壁 ID {} の断面「{}」の壁厚が 0 以下です。\
                 断面タブで壁厚を設定してください。\
                 保有水平耐力計算では耐震壁の終局せん断強度が必要です。",
                data.id.0, sec.name
            ));
        }
        let Some(mat) = model.element_material(data) else {
            return Some(format!(
                "耐震壁 ID {} に材料が設定されていません。\
                 材料タブで材料を割り当ててください。\
                 保有水平耐力計算では耐震壁の終局せん断強度が必要です。",
                data.id.0
            ));
        };
        if !crate::wall::misc_wall::is_rc_wall(data, model) {
            if !mat.fy.is_some_and(|fy| fy > 0.0) {
                return Some(format!(
                    "鋼板耐震壁 ID {} の材料「{}」に降伏強度 fy が設定されていません。\
                     材料タブで fy を設定してください。\
                     保有水平耐力計算では鋼板のせん断降伏 Qy=t·lw·F/√3 で面内せん断を頭打ちにします。",
                    data.id.0, mat.name
                ));
            }
            if Self::steel_shear_capacity_of(data, model) <= 0.0 {
                return Some(format!(
                    "鋼板耐震壁 ID {} の終局せん断強度 Qy を算定できません（算定結果が 0 以下）。\
                     壁の板厚・壁長さの入力を確認してください。\
                     保有水平耐力計算では Qy が定まらない壁を弾性として扱えません。",
                    data.id.0
                ));
            }
            return None;
        }
        None
    }

    /// 面内せん断の終局強度 Qu [N] を与えて弾完全塑性化する（保有水平耐力用）。
    /// `qu <= 0` は弾性のまま（降伏しない）。
    pub(crate) fn with_shear_capacity(mut self, qu: [f64; 2]) -> Self {
        self.qu_shear = qu.map(|q| q.max(0.0));
        self
    }

    /// 面内せん断の復元力ばねを構築する。
    /// 骨格は弾完全塑性（初期剛性 k_s0、耐力 Qu で頭打ち）とし、
    /// 除荷・再載荷則のみ `rule` に従う。
    /// `qu_shear <= 0` や k_s0 が取れない場合は何もしない。
    pub(crate) fn with_shear_hysteresis(mut self, rule: HysteresisModel) -> Self {
        use sepika_material::{HysteresisMaterial, HysteresisRule};
        if self.qu_shear.iter().any(|q| *q <= 0.0) {
            return self;
        }
        let k12 = self.column.axis.to_global(&self.column_stiffness_pinned());
        let mut v = [0.0_f64; 12];
        for (i, vi) in v.iter_mut().enumerate() {
            let mut acc = 0.0;
            for q in 0..24 {
                acc += self.a_mat[i * 24 + q] * self.shear_mode[q];
            }
            *vi = acc;
        }
        let mut w = [0.0_f64; 12];
        for (i, wi) in w.iter_mut().enumerate() {
            let mut acc = 0.0;
            for (j, vj) in v.iter().enumerate() {
                acc += k12.get(i, j) * vj;
            }
            *wi = acc;
        }
        let k_s0: f64 = v.iter().zip(w.iter()).map(|(a, b)| a * b).sum();
        if k_s0 <= 0.0 {
            return self;
        }
        let make_rule = |qu: f64| {
            let dy = qu / k_s0;
            let crack = (qu / 3.0, dy / 3.0);
            let yield_point = (qu, dy);
            let ultimate = (qu, dy * 1.0e4);
            match rule {
                HysteresisModel::Retrograde => HysteresisRule::Retrograde {
                    crack,
                    yield_point,
                    ultimate,
                },
                HysteresisModel::Standard => HysteresisRule::Standard {
                    crack,
                    yield_point,
                    ultimate,
                },
                HysteresisModel::OriginOriented => HysteresisRule::OriginOriented {
                    yield_point,
                    ultimate,
                },
                HysteresisModel::Takeda => HysteresisRule::Takeda {
                    crack,
                    yield_point,
                    ultimate,
                    alpha: 0.4,
                },
                _ => HysteresisRule::MaxPointOriented {
                    crack,
                    yield_point,
                    ultimate,
                },
            }
        };
        self.shear_spring = Some(Box::new(
            HysteresisMaterial::new(make_rule(self.qu_shear[0]))
                .with_negative_rule(make_rule(self.qu_shear[1])),
        ));
        self.shear_k0 = k_s0;
        self
    }

    fn shear_limit(&self, q: f64) -> f64 {
        self.qu_shear[usize::from(q < 0.0)]
    }

    /// 面内せん断の弾完全塑性リターンマッピング。
    ///
    /// せん断モード p（[`Self::shear_mode`]）に沿う塑性すべり γp を導入し、
    /// 有効変位を `u_eff = u − γp·p` とする。弾性内力 `f = K·u_eff` に対し
    /// 壁が伝達する面内水平力は `Q = pᵀ·f` であり、`|Q| > Qu` のとき
    /// `Δγp = (|Q| − Qu)·sign(Q)/k_s`（`k_s = pᵀ·K·p`）だけ γp を増やすと
    /// `|Q| = Qu` に戻る（Q は γp に線形なため 1 回の補正で厳密に満たす）。
    ///
    /// 戻り値は `(γp, 降伏しているか)`。`qu_shear <= 0` は常に弾性。
    fn shear_return_map(&self, k: &LocalMat, u24: &[f64; 24]) -> (f64, bool) {
        if self.qu_shear.iter().any(|q| *q <= 0.0) {
            return (0.0, false);
        }
        let kp = Self::mat_vec(k, &self.shear_mode);
        let k_s: f64 = self
            .shear_mode
            .iter()
            .zip(kp.iter())
            .map(|(p, v)| p * v)
            .sum();
        if k_s <= 0.0 {
            return (self.committed_slip, false);
        }
        let mut u_eff = *u24;
        for (ue, p) in u_eff.iter_mut().zip(self.shear_mode.iter()) {
            *ue -= self.committed_slip * p;
        }
        let f = Self::mat_vec(k, &u_eff);
        let q_trial: f64 = self
            .shear_mode
            .iter()
            .zip(f.iter())
            .map(|(p, v)| p * v)
            .sum();
        if let Some(sp) = &self.shear_spring {
            if self.shear_k0 > 0.0 {
                let d = self.committed_slip + q_trial / self.shear_k0;
                let (q_target, _) = sp.probe(d);
                let yielded = (q_trial - q_target).abs() > self.shear_limit(q_trial) * 1e-9;
                return (self.committed_slip + (q_trial - q_target) / k_s, yielded);
            }
        }
        if q_trial.abs() <= self.shear_limit(q_trial) {
            return (self.committed_slip, false);
        }
        let d_gamma = (q_trial.abs() - self.shear_limit(q_trial)) * q_trial.signum() / k_s;
        (self.committed_slip + d_gamma, true)
    }

    /// K·v（24 次）。
    fn mat_vec(k: &LocalMat, v: &[f64; 24]) -> [f64; 24] {
        let mut out = [0.0; 24];
        for (i, o) in out.iter_mut().enumerate() {
            let mut s = 0.0;
            for (j, vj) in v.iter().enumerate() {
                if *vj != 0.0 {
                    s += k.get(i, j) * vj;
                }
            }
            *o = s;
        }
        out
    }

    /// 壁柱の局所 y 軸まわり（面外曲げ）の両端回転を静縮約した弾性剛性。
    fn column_stiffness_pinned(&self) -> LocalMat {
        crate::frame::prismatic::condense_end_releases(
            &self.column.local_stiffness(),
            &[(4, 0.0), (10, 0.0)],
        )
        .unwrap_or_else(|| {
            panic!(
                "壁柱の端部解放剛性を縮約できません: Kbb が特異です（解放条件を確認してください）"
            )
        })
    }

    /// 壁柱の全体系 12×12 接線剛性（ファイバー壁柱があればその整合接線、
    /// なければ弾性壁柱）。
    fn k12_global(&self, ctx: &Ctx) -> LocalMat {
        match &self.fiber_column {
            Some(f) => f.tangent_stiffness(ctx),
            None => self.column.axis.to_global(&self.column_stiffness_pinned()),
        }
    }

    /// 壁柱の現在トライアル状態の全体系内力（24 自由度）。ファイバー壁柱専用
    /// （履歴に整合した復元力 f24 = Aᵀ·f12）。
    fn f24_fiber(&self, ctx: &Ctx) -> Option<[f64; 24]> {
        let f = self.fiber_column.as_ref()?;
        let f12 = f.internal_force(ctx);
        let mut f24 = [0.0_f64; 24];
        for (p, fp) in f24.iter_mut().enumerate() {
            let mut s = 0.0;
            for i in 0..12 {
                s += self.a_mat[i * 24 + p] * f12.data[i];
            }
            *fp = s;
        }
        Some(f24)
    }

    /// ファイバー壁柱が現在伝達している面内水平力 Q = pᵀ·f24。
    fn inplane_shear_fiber(&self, ctx: &Ctx) -> Option<f64> {
        let f24 = self.f24_fiber(ctx)?;
        Some(
            self.shear_mode
                .iter()
                .zip(f24.iter())
                .map(|(p, v)| p * v)
                .sum(),
        )
    }

    /// 現在のトライアル状態で壁が伝達している面内水平力 Q = pᵀ·f。
    /// ファイバー壁柱はその内力から、弾性壁柱は f = K·(u − γp·p) から評価する。
    fn inplane_shear_trial(&self, ctx: &Ctx) -> f64 {
        if let Some(q) = self.inplane_shear_fiber(ctx) {
            return q;
        }
        let k = self.stiffness_24(ctx);
        let mut u_eff = self.trial_disp;
        for (ue, p) in u_eff.iter_mut().zip(self.shear_mode.iter()) {
            *ue -= self.trial_slip * p;
        }
        let f = Self::mat_vec(&k, &u_eff);
        self.shear_mode
            .iter()
            .zip(f.iter())
            .map(|(p, v)| p * v)
            .sum()
    }

    /// 全体系 24×24 剛性 K = Aᵀ·K_col·A。
    fn stiffness_24(&self, ctx: &Ctx) -> LocalMat {
        let k12 = self.k12_global(ctx);
        let mut k = LocalMat::zeros(24);
        for p in 0..24 {
            for q in 0..24 {
                let mut s = 0.0;
                for i in 0..12 {
                    let aip = self.a_mat[i * 24 + p];
                    if aip == 0.0 {
                        continue;
                    }
                    for j in 0..12 {
                        let ajq = self.a_mat[j * 24 + q];
                        if ajq != 0.0 {
                            s += aip * k12.get(i, j) * ajq;
                        }
                    }
                }
                if s != 0.0 {
                    k.set(p, q, s);
                }
            }
        }
        k
    }

    /// 四隅変位 24 → 壁柱端変位 12（全体系）。
    fn to_column_disp(&self, u24: &[f64]) -> [f64; 12] {
        let mut u12 = [0.0; 12];
        for (i, ui) in u12.iter_mut().enumerate() {
            let mut s = 0.0;
            for p in 0..24 {
                s += self.a_mat[i * 24 + p] * u24[p];
            }
            *ui = s;
        }
        u12
    }
}

impl ElementBehavior for WallElement {
    fn n_dof(&self) -> usize {
        24
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        crate::behavior::node_global_dofs(&self.nodes, dof)
    }

    fn tangent_stiffness(&self, ctx: &Ctx) -> LocalMat {
        let k = self.stiffness_24(ctx);
        if self.qu_shear.iter().any(|q| *q <= 0.0) {
            return k;
        }
        let kp = Self::mat_vec(&k, &self.shear_mode);
        let k_s: f64 = self
            .shear_mode
            .iter()
            .zip(kp.iter())
            .map(|(p, v)| p * v)
            .sum();
        if k_s <= 0.0 {
            return k;
        }
        let factor = if self.shear_spring.is_some() && self.shear_k0 > 0.0 {
            0.0
        } else {
            let yielded = match self.inplane_shear_fiber(ctx) {
                Some(q) => q.abs() >= self.shear_limit(q) * (1.0 - 1e-9),
                None => self.shear_return_map(&k, &self.trial_disp).1,
            };
            if yielded {
                1.0
            } else {
                0.0
            }
        };
        if factor <= 0.0 {
            return k;
        }
        let mut kt = LocalMat::zeros(24);
        for i in 0..24 {
            for j in 0..24 {
                let v = k.get(i, j) - factor * kp[i] * kp[j] / k_s;
                if v != 0.0 {
                    kt.set(i, j, v);
                }
            }
        }
        kt
    }

    fn internal_force(&self, ctx: &Ctx) -> LocalVec {
        if let Some(f24) = self.f24_fiber(ctx) {
            return LocalVec {
                data: smallvec::SmallVec::from_slice(&f24),
            };
        }
        let k = self.stiffness_24(ctx);
        let (slip, _) = self.shear_return_map(&k, &self.trial_disp);
        let mut u_eff = self.trial_disp;
        for (ue, p) in u_eff.iter_mut().zip(self.shear_mode.iter()) {
            *ue -= slip * p;
        }
        let fv = Self::mat_vec(&k, &u_eff);
        LocalVec {
            data: smallvec::SmallVec::from_slice(&fv),
        }
    }

    fn update_state(
        &mut self,
        du: &LocalVec,
        commit: bool,
        ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, self.n_dof())?;
        let previous = self.snapshot_state();
        for i in 0..24.min(du.data.len()) {
            self.trial_disp[i] += du.data[i];
        }
        if self.fiber_column.is_some() {
            let mut slip = self.committed_slip;
            for _ in 0..8 {
                let mut u_eff = self.trial_disp;
                for (ue, p) in u_eff.iter_mut().zip(self.shear_mode.iter()) {
                    *ue -= slip * p;
                }
                let u12 = self.to_column_disp(&u_eff);
                let du12: [f64; 12] = std::array::from_fn(|i| u12[i] - self.fiber_u12_trial[i]);
                let dv = LocalVec {
                    data: smallvec::SmallVec::from_slice(&du12),
                };
                let Some(fiber) = self.fiber_column.as_mut() else {
                    break;
                };
                if let Err(error) = fiber.update_state(&dv, false, ctx) {
                    self.restore_state(previous.as_ref());
                    return Err(crate::behavior::ElementUpdateError(format!(
                        "WallElement 内部柱: {error}"
                    )));
                }
                self.fiber_u12_trial = u12;
                if self.qu_shear.iter().any(|q| *q <= 0.0) {
                    break;
                }
                let Some(q) = self.inplane_shear_fiber(ctx) else {
                    break;
                };
                let residual = if self.shear_spring.is_some() && self.shear_k0 > 0.0 {
                    let d = slip + q / self.shear_k0;
                    let q_target = self
                        .shear_spring
                        .as_ref()
                        .map(|sp| sp.probe(d).0)
                        .unwrap_or(q);
                    q - q_target
                } else if q.abs() > self.shear_limit(q) {
                    (q.abs() - self.shear_limit(q)) * q.signum()
                } else {
                    0.0
                };
                if residual.abs() <= self.shear_limit(q) * 1e-9 {
                    break;
                }
                let k = self.stiffness_24(ctx);
                let kp = Self::mat_vec(&k, &self.shear_mode);
                let k_s: f64 = self
                    .shear_mode
                    .iter()
                    .zip(kp.iter())
                    .map(|(p, v)| p * v)
                    .sum();
                if k_s <= 0.0 {
                    break;
                }
                slip += residual / k_s;
            }
            self.trial_slip = slip;
        } else {
            let k = self.stiffness_24(ctx);
            let (slip, _) = self.shear_return_map(&k, &self.trial_disp);
            self.trial_slip = slip;
        }
        if self.shear_spring.is_some() && self.shear_k0 > 0.0 {
            let q = self.inplane_shear_trial(ctx);
            let d = self.trial_slip + q / self.shear_k0;
            if let Some(sp) = &mut self.shear_spring {
                sp.trial(d);
            }
        }
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn commit_state(&mut self) {
        self.committed_disp = self.trial_disp;
        self.committed_slip = self.trial_slip;
        if let Some(f) = &mut self.fiber_column {
            f.commit_state();
        }
        if let Some(sp) = &mut self.shear_spring {
            sp.commit();
        }
        self.fiber_u12_committed = self.fiber_u12_trial;
    }

    fn revert_state(&mut self) {
        self.trial_disp = self.committed_disp;
        self.trial_slip = self.committed_slip;
        if let Some(f) = &mut self.fiber_column {
            f.revert_state();
        }
        if let Some(sp) = &mut self.shear_spring {
            sp.revert();
        }
        self.fiber_u12_trial = self.fiber_u12_committed;
    }

    fn snapshot_state(&self) -> Box<dyn std::any::Any> {
        Box::new((
            self.committed_disp,
            self.trial_disp,
            self.committed_slip,
            self.trial_slip,
            self.fiber_column.as_ref().map(|f| f.snapshot_state()),
            self.fiber_u12_trial,
            self.fiber_u12_committed,
            self.shear_spring.as_ref().map(|sp| sp.serialize_state()),
        ))
    }

    fn restore_state(&mut self, state: &dyn std::any::Any) {
        type Snapshot = (
            [f64; 24],
            [f64; 24],
            f64,
            f64,
            Option<Box<dyn std::any::Any>>,
            [f64; 12],
            [f64; 12],
            Option<Vec<u8>>,
        );
        let (committed, trial, cslip, tslip, fsnap, u12t, u12c, spring) =
            crate::behavior::downcast_snapshot::<Snapshot>("WallElement", state);
        self.committed_disp = *committed;
        self.trial_disp = *trial;
        self.committed_slip = *cslip;
        self.trial_slip = *tslip;
        if let (Some(f), Some(snap)) = (&mut self.fiber_column, fsnap.as_ref()) {
            f.restore_state(snap.as_ref());
        }
        self.fiber_u12_trial = *u12t;
        self.fiber_u12_committed = *u12c;
        if let (Some(sp), Some(bytes)) = (&mut self.shear_spring, spring.as_ref()) {
            sp.deserialize_state(bytes)
                .expect("壁せん断ばねのスナップショット復元");
        }
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        let cp = WallElementCheckpoint {
            committed_disp: self.committed_disp,
            trial_disp: self.trial_disp,
            committed_slip: self.committed_slip,
            trial_slip: self.trial_slip,
            fiber: self.fiber_column.as_ref().map(|f| f.serialize_checkpoint()),
            fiber_u12_trial: self.fiber_u12_trial,
            fiber_u12_committed: self.fiber_u12_committed,
            shear_spring: self.shear_spring.as_ref().map(|sp| sp.serialize_state()),
        };
        bincode::serialize(&cp).expect("serialize checkpoint")
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        if data.is_empty() {
            return Ok(());
        }
        if let Ok(cp) = bincode::deserialize::<WallElementCheckpoint>(data) {
            self.committed_disp = cp.committed_disp;
            self.trial_disp = cp.trial_disp;
            self.committed_slip = cp.committed_slip;
            self.trial_slip = cp.trial_slip;
            if let (Some(f), Some(bytes)) = (&mut self.fiber_column, cp.fiber.as_ref()) {
                f.deserialize_checkpoint(bytes)?;
            }
            self.fiber_u12_trial = cp.fiber_u12_trial;
            self.fiber_u12_committed = cp.fiber_u12_committed;
            if let (Some(sp), Some(bytes)) = (&mut self.shear_spring, cp.shear_spring.as_ref()) {
                sp.deserialize_state(bytes)
                    .map_err(|e| crate::behavior::CheckpointError::Decode(e.to_string()))?;
            }
            return Ok(());
        }
        let (committed, trial): ([f64; 24], [f64; 24]) = bincode::deserialize(data)
            .map_err(|e| crate::behavior::CheckpointError::Decode(e.to_string()))?;
        self.committed_disp = committed;
        self.trial_disp = trial;
        Ok(())
    }

    /// 塑性率評価はファイバー壁柱の危険断面プローブへ委譲する（弾性壁柱は None）。
    fn ductility_probe(&self) -> Option<crate::behavior::DuctilityProbe> {
        self.fiber_column.as_ref().and_then(|f| f.ductility_probe())
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        if matches!(opt, MassOption::Consistent) {
            let column_mass_properties = self
                .column
                .mass_properties_error
                .as_ref()
                .map_or(self.column.mass_properties, |error| {
                    panic!("質量特性を解決できません: {error}")
                });
            let column_mass = crate::frame::prismatic::condense_end_releases_with_mass(
                &self.column.local_stiffness(),
                &self
                    .column
                    .axis
                    .to_local(&self.column.mass_matrix(MassOption::Consistent)),
                column_mass_properties,
                0.0,
                0.0,
                &[(4, 0.0), (10, 0.0)],
            )
            .unwrap_or_else(|| {
                panic!(
                    "壁柱の整合質量を縮約できません: Kbb が特異です（解放条件を確認してください）"
                )
            });
            let column_mass = self.column.axis.to_global(&column_mass);
            let reference_mass = column_mass_properties.total_mass(self.column.length);
            let scale = if reference_mass > 0.0 {
                self.mass_total / reference_mass
            } else {
                0.0
            };
            let mut mass = LocalMat::zeros(24);
            for p in 0..24 {
                for q in 0..24 {
                    let mut value = 0.0;
                    for i in 0..12 {
                        let aip = self.a_mat[i * 24 + p];
                        if aip == 0.0 {
                            continue;
                        }
                        for j in 0..12 {
                            let ajq = self.a_mat[j * 24 + q];
                            if ajq != 0.0 {
                                value += aip * column_mass.get(i, j) * ajq;
                            }
                        }
                    }
                    mass.set(p, q, value * scale);
                }
            }
            return mass;
        }
        let mut mm = LocalMat::zeros(24);
        let m_node = self.mass_total / 4.0;
        for i in 0..4 {
            let bo = i * 6;
            for d in 0..3 {
                mm.set(bo + d, bo + d, m_node);
            }
        }
        mm
    }

    fn geometric_stiffness(&self, _n: f64) -> LocalMat {
        LocalMat::zeros(24)
    }

    fn state_member_forces(&self, ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        if let Some(fiber) = &self.fiber_column {
            return fiber.state_member_forces(ctx);
        }
        let k = self.stiffness_24(ctx);
        let (slip, _) = self.shear_return_map(&k, &self.trial_disp);
        let mut u_eff = self.trial_disp;
        for (u, p) in u_eff.iter_mut().zip(&self.shear_mode) {
            *u -= slip * p;
        }
        self.recover_forces(&u_eff)
    }

    fn recover_forces(&self, u_elem: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        if u_elem.len() < 24 {
            return None;
        }
        let u12 = self.to_column_disp(&u_elem[..24]);
        let u_local = self.column.axis.rotate_to_local(&u12);
        let k = self.column_stiffness_pinned();
        let f_local = std::array::from_fn(|i| (0..12).map(|j| k.get(i, j) * u_local[j]).sum());
        Some(crate::frame::beam::member_forces_from_end_forces(
            &f_local,
            self.column.length,
            &self.column.eval_sections,
        ))
    }
}

/// [`WallElement`] のチェックポイント形式。
#[derive(serde::Serialize, serde::Deserialize)]
struct WallElementCheckpoint {
    committed_disp: [f64; 24],
    trial_disp: [f64; 24],
    committed_slip: f64,
    trial_slip: f64,
    /// ファイバー壁柱のチェックポイント（弾性壁柱は None）。
    fiber: Option<Vec<u8>>,
    fiber_u12_trial: [f64; 12],
    fiber_u12_committed: [f64; 12],
    /// 面内せん断ばねの材料状態（ばね未構築は None）。
    #[serde(default)]
    shear_spring: Option<Vec<u8>>,
}

fn levi_civita(i: usize, j: usize, k: usize) -> f64 {
    match (i, j, k) {
        (0, 1, 2) | (1, 2, 0) | (2, 0, 1) => 1.0,
        (0, 2, 1) | (2, 1, 0) | (1, 0, 2) => -1.0,
        _ => 0.0,
    }
}

#[cfg(test)]
fn explicit_wall_test_rebar(model: &mut Model) {
    let id = sepika_core::ids::MaterialId(model.materials.len() as u32);
    model.materials.push(sepika_core::model::Material {
        id,
        name: "SD345".into(),
        category: sepika_core::model::MaterialCategory::Rebar,
        young: E_STEEL,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: Some(345.0),
        strength_factor: Some(1.0),
        concrete_class: Default::default(),
    });
    model.sections[0].rebar_material = Some(id);
    model.sections[0].shear_rebar_material = Some(id);
}

#[cfg(test)]
fn fixture_directional_capacity(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    basis: crate::factory::StrengthBasis,
) -> Result<[f64; 2], String> {
    WallElement::directional_shear_capacity_at_reference(data, model, basis, 0.0, [1.0; 2])
}
#[cfg(test)]
fn fixture_capacity(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
) -> Result<f64, String> {
    fixture_directional_capacity(data, model, crate::factory::StrengthBasis::Nominal)
        .map(|q| q[0].min(q[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, SectionId};
    use sepika_core::model::MaterialCategory;
    use sepika_core::model::{ElementKind, EndCondition, ForceRegime, LocalAxis, Material, Node};
    use sepika_core::section_shape::SectionShape;

    /// 4000×3000×t150 の壁（X-Z 面内）を持つモデル。
    fn make_wall_model() -> (Model, ElementData) {
        let make_node = |id: u32, coord: [f64; 3]| Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let shape = SectionShape::RcWall {
            thickness: 150.0,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
            ps: 0.0025,
        };
        let model = Model {
            nodes: vec![
                make_node(0, [0.0, 0.0, 0.0]),
                make_node(1, [4000.0, 0.0, 0.0]),
                make_node(2, [4000.0, 0.0, 3000.0]),
                make_node(3, [0.0, 0.0, 3000.0]),
            ],
            sections: vec![sepika_core::model::Section {
                material: Some(MaterialId(0)),
                ..shape.to_section(SectionId(0), "W150".into())
            }],
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "FC24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            }],
            ..Default::default()
        };
        let data = ElementData {
            id: ElemId(0),
            kind: ElementKind::Wall,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        let mut model = model;
        model.elements.push(data.clone());
        crate::wall::add_surrounding_frame(&mut model, &data);
        let mut model = model;
        explicit_wall_test_rebar(&mut model);
        (model, data)
    }

    fn energy(k: &LocalMat, u: &[f64; 24]) -> f64 {
        let mut s = 0.0;
        for i in 0..24 {
            for j in 0..24 {
                s += u[i] * k.get(i, j) * u[j];
            }
        }
        s
    }

    #[test]
    fn failed_internal_fiber_trial_restores_entire_wall_and_allows_retry() {
        let (model, data) = make_wall_model();
        let ctx = Ctx { model: &model };
        let mut wall = WallElement::try_new(&data, &model)
            .unwrap()
            .with_fiber_flexure(
                &data,
                &model,
                crate::factory::StrengthBasis::MaterialStrength,
                sepika_core::model::AnalysisKind::Incremental,
            )
            .unwrap();
        let mut du = LocalVec {
            data: smallvec::smallvec![0.0;24],
        };
        du.data[12] = 0.001;
        du.data[18] = 0.001;
        wall.update_state(&du, true, &ctx).unwrap();
        let committed = wall.serialize_checkpoint();
        wall.update_state(&du, false, &ctx).unwrap();
        let trial = wall.serialize_checkpoint();
        let force = wall.internal_force(&ctx).data;
        let tangent = wall.tangent_stiffness(&ctx).data;
        let mut bad = LocalVec {
            data: smallvec::smallvec![0.0;24],
        };
        bad.data[14] = 1e300;
        bad.data[20] = -1e300;
        let error = wall.update_state(&bad, true, &ctx).unwrap_err();
        assert!(error.to_string().contains("WallElement 内部柱"), "{error}");
        assert_eq!(wall.serialize_checkpoint(), trial);
        assert_eq!(wall.internal_force(&ctx).data, force);
        assert_eq!(wall.tangent_stiffness(&ctx).data, tangent);
        wall.revert_state();
        assert_eq!(wall.serialize_checkpoint(), committed);
        wall.update_state(&du, false, &ctx).unwrap();
        assert_eq!(wall.serialize_checkpoint(), trial);
    }

    #[test]
    fn zero_increment_asymmetric_wall_shear_keeps_return_map_trial_commit_and_revert() {
        let (model, data) = make_wall_model();
        let ctx = Ctx { model: &model };
        let mut wall = WallElement::try_new_scaled(&data, &model, 1.0)
            .unwrap()
            .with_shear_capacity([1000.0, 2000.0])
            .with_shear_hysteresis(HysteresisModel::MaxPointOriented);
        assert!(wall.shear_k0 > 0.0);
        assert_eq!(
            wall.shear_spring.as_ref().unwrap().probe(0.0),
            (0.0, wall.shear_k0)
        );
        let zero = LocalVec {
            data: smallvec::smallvec![0.0;24],
        };
        let mut previous = 0.0;
        for (target, expected) in [(-20.0, -2000.0), (30.0, 1000.0), (-30.0, -2000.0)] {
            let mut du = LocalVec {
                data: smallvec::smallvec![0.0;24],
            };
            du.data[12] = target - previous;
            du.data[18] = target - previous;
            wall.update_state(&du, false, &ctx).unwrap();
            wall.commit_state();
            previous = target;
            let force = wall.internal_force(&ctx).data;
            assert!((force[12] + force[18] - expected).abs() < 1e-7);
            let state = wall.shear_spring.as_ref().unwrap().serialize_state();
            let k = wall.stiffness_24(&ctx);
            let _ = wall.shear_return_map(&k, &wall.trial_disp);
            assert_eq!(state, wall.shear_spring.as_ref().unwrap().serialize_state());
            wall.update_state(&zero, false, &ctx).unwrap();
            wall.commit_state();
            assert_eq!(state, wall.shear_spring.as_ref().unwrap().serialize_state());
            assert_eq!(force, wall.internal_force(&ctx).data);
            let tangent = wall.tangent_stiffness(&ctx).data;
            wall.update_state(&du, false, &ctx).unwrap();
            wall.revert_state();
            assert_eq!(force, wall.internal_force(&ctx).data);
            assert_eq!(tangent, wall.tangent_stiffness(&ctx).data);
            assert_eq!(state, wall.shear_spring.as_ref().unwrap().serialize_state());
        }
    }

    #[test]
    fn test_wall_mass_uses_clear_area_between_surrounding_members() {
        for (boundary_mm, thickness_mm, expected_mass) in [
            (600.0, 150.0, 29_376.0 / 9806.65),
            (400.0, 200.0, 44_928.0 / 9806.65),
        ] {
            let (mut model, data) = make_wall_model();
            if let Some(SectionShape::RcWall { thickness, .. }) = &mut model.sections[0].shape {
                *thickness = thickness_mm;
            }
            let column_id = SectionId(model.sections.len() as u32);
            let mut section = SectionShape::RcColumnRect {
                b: boundary_mm,
                d: boundary_mm,
                rebar: sepika_core::section_shape::RcRectColumnRebar {
                    main_dia: 22.0,
                    x: vec![8],
                    y: vec![8],
                    cover: 50.0,
                    hoop: sepika_core::section_shape::RectColumnHoop {
                        dia: 10.0,
                        pitch: 100.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
            }
            .to_section(column_id, "C600".into());
            section.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
            section.material = Some(MaterialId(0));
            model.sections.push(section);
            let girder_id = SectionId(model.sections.len() as u32);
            let mut girder = sepika_core::model::Section::zero(girder_id, "G600".into());
            girder.depth = boundary_mm;
            girder.width = boundary_mm;
            girder.material = Some(MaterialId(0));
            girder.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
            model.sections.push(girder);
            model.elements.clear();
            for i in 0..4 {
                let mut beam = data.clone();
                beam.id = ElemId(i as u32 + 1);
                beam.kind = ElementKind::Beam;
                beam.nodes = smallvec::smallvec![data.nodes[i], data.nodes[(i + 1) % 4]];
                beam.section = Some(if i % 2 == 0 { girder_id } else { column_id });
                model.elements.push(beam);
            }
            for kind in [
                ElementKind::Beam,
                ElementKind::Fiber,
                ElementKind::MultiSpring,
            ] {
                model.elements.retain(|e| e.kind != ElementKind::Wall);
                for frame in &mut model.elements {
                    frame.kind = kind;
                }
                for order in [[0, 1, 2, 3], [0, 3, 2, 1], [0, 3, 1, 2], [2, 0, 3, 1]] {
                    let mut reordered = data.clone();
                    reordered.nodes = order.into_iter().map(|i| data.nodes[i]).collect();
                    model.elements.retain(|e| e.kind != ElementKind::Wall);
                    model.elements.push(reordered.clone());
                    let wall = WallElement::try_new(&reordered, &model).unwrap();
                    let expected = expected_mass;
                    for option in [MassOption::Lumped, MassOption::Consistent] {
                        let mass = wall.mass_matrix(option);
                        for dir in 0..3 {
                            let total: f64 = (0..4)
                                .flat_map(|i| (0..4).map(move |j| (i, j)))
                                .map(|(i, j)| mass.get(i * 6 + dir, j * 6 + dir))
                                .sum();
                            assert!(
                                (total - expected).abs() < 1e-10,
                                "total={total} expected={expected}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_wall_mass_preserves_section_mass_with_rebar() {
        let (mut model, mut data) = make_wall_model();
        let mut rebar = model.materials[0].clone();
        rebar.id = MaterialId(1);
        rebar.name = "SD345".into();
        rebar.category = MaterialCategory::Rebar;
        rebar.density = 7.85e-9;
        rebar.fy = Some(345.0);
        model.materials.push(rebar);
        model.sections[0].rebar_material = Some(MaterialId(1));
        data.nodes = smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)];
        let wall = WallElement::try_new(&data, &model).unwrap();
        let rc_density = sepika_core::units::to_internal::mass_density_from_unit_weight_kn_m3(24.0);
        let expected = rc_density * 150.0 * 4000.0 * 3000.0;
        let mass = wall.mass_matrix(MassOption::Lumped);
        let actual: f64 = (0..4).map(|i| mass.get(i * 6, i * 6)).sum();
        assert!(
            (actual - expected).abs() < expected * 1e-12,
            "actual={actual} expected={expected}"
        );
    }

    #[test]
    fn test_rc_wall_mass_uses_fc_standard_unit_weight() {
        let (mut model, data) = make_wall_model();
        model.materials[0].density = 2.4e-9;
        let fc24 = WallElement::try_new(&data, &model)
            .unwrap()
            .mass_matrix(MassOption::Lumped);
        model.materials[0].fc = Some(42.0);
        let fc42 = WallElement::try_new(&data, &model)
            .unwrap()
            .mass_matrix(MassOption::Lumped);
        let total = |mass: &LocalMat| (0..4).map(|i| mass.get(i * 6, i * 6)).sum::<f64>();
        assert!(total(&fc42) > total(&fc24));
        assert!((total(&fc24) / total(&fc42) - 24.0 / 24.5).abs() < 1e-12);
    }

    #[test]
    fn test_rc_wall_column_mass_properties_use_wall_length() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let density = sepika_core::units::to_internal::mass_density_from_unit_weight_kn_m3(24.0);
        let t: f64 = 150.0;
        let lw: f64 = 4000.0;
        let properties = wall.column.mass_properties;
        assert!((properties.mass_per_length - density * lw * t).abs() < 1e-12);
        assert!(
            (properties.rotary_inertia_y_per_length - density * lw * t.powi(3) / 12.0).abs()
                < 1e-12
        );
        assert!(
            (properties.rotary_inertia_z_per_length - density * t * lw.powi(3) / 12.0).abs()
                < 1e-12
        );
        assert!(
            (properties.polar_inertia_per_length()
                - density * (lw * t.powi(3) + t * lw.powi(3)) / 12.0)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn test_rc_wall_rejects_invalid_fc_at_generation() {
        let (mut model, data) = make_wall_model();
        for fc in [None, Some(0.0), Some(f64::NAN)] {
            model.materials[0].fc = fc;
            assert!(WallElement::try_new(&data, &model).is_none());
        }
    }

    #[test]
    fn test_rc_wall_stiffness_uses_material_young() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let expected = model.materials[0].young;
        assert!((wall.column.e - expected).abs() < 1e-9);
    }

    #[test]
    fn test_wall_consistent_mass_preserves_total_mass_and_differs_from_lumped() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let consistent = wall.mass_matrix(MassOption::Consistent);
        let lumped = wall.mass_matrix(MassOption::Lumped);
        let total_consistent: f64 = (0..24)
            .map(|i| (0..24).map(|j| consistent.get(i, j)).sum::<f64>())
            .sum();
        let total_lumped: f64 = (0..24)
            .map(|i| (0..24).map(|j| lumped.get(i, j)).sum::<f64>())
            .sum();
        assert!((total_consistent - total_lumped).abs() < total_lumped * 1e-12);
        assert!((consistent.get(0, 6) - lumped.get(0, 6)).abs() > 1e-12);
    }

    #[test]
    fn test_wall_element_rejects_nonfinite_main_density() {
        let (mut model, data) = make_wall_model();
        model.materials[0].density = f64::NAN;
        assert!(WallElement::try_new(&data, &model).is_none());
        model.materials[0].density = f64::INFINITY;
        assert!(WallElement::try_new(&data, &model).is_none());
        model.materials[0].density = -1.0;
        assert!(WallElement::try_new(&data, &model).is_none());
    }

    #[test]
    fn test_wall_element_rigid_translation_zero_force() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };
        let k = wall.stiffness_24(&ctx);
        for dir in 0..3 {
            let mut u = [0.0; 24];
            for n in 0..4 {
                u[n * 6 + dir] = 1.0;
            }
            for i in 0..24 {
                let f: f64 = (0..24).map(|j| k.get(i, j) * u[j]).sum();
                assert!(
                    f.abs() < 1e-6,
                    "剛体移動で内力が生じた: dir={dir} i={i} f={f}"
                );
            }
        }
    }

    fn assert_rigid_rotation_zero_force(axis: usize, nonlinear: bool) {
        let (model, data) = make_wall_model();
        let mut wall = WallElement::try_new(&data, &model).unwrap();
        if nonlinear {
            wall = wall
                .with_fiber_flexure(
                    &data,
                    &model,
                    crate::factory::StrengthBasis::MaterialStrength,
                    sepika_core::model::AnalysisKind::Incremental,
                )
                .unwrap();
            assert!(wall.fiber_column.is_some());
        }
        let k = wall.stiffness_24(&Ctx { model: &model });
        let mut theta = [0.0; 3];
        theta[axis] = 1e-3;
        let mut u = [0.0; 24];
        for (slot, node) in wall.nodes.iter().enumerate() {
            let x = model.nodes[node.index()].coord;
            let translation = [
                theta[1] * x[2] - theta[2] * x[1],
                theta[2] * x[0] - theta[0] * x[2],
                theta[0] * x[1] - theta[1] * x[0],
            ];
            u[slot * 6..slot * 6 + 3].copy_from_slice(&translation);
            u[slot * 6 + 3..slot * 6 + 6].copy_from_slice(&theta);
        }
        let max_force = (0..24)
            .map(|i| (0..24).map(|j| k.get(i, j) * u[j]).sum::<f64>().abs())
            .fold(0.0_f64, f64::max);
        assert!(
            max_force < 1e-6,
            "剛体回転で内力: axis={axis}, max={max_force}"
        );
        let ctx = Ctx { model: &model };
        wall.update_state(
            &LocalVec {
                data: u.into_iter().collect(),
            },
            false,
            &ctx,
        )
        .unwrap();
        let force = wall.internal_force(&ctx);
        assert!(
            force.data.iter().all(|v| v.abs() < 1e-6),
            "剛体回転の状態内力: {:?}",
            force.data
        );
        let recovered = wall.recover_forces(&u).unwrap();
        let max_recovered = recovered
            .at
            .iter()
            .flat_map(|(_, f)| f.iter())
            .map(|v| v.abs())
            .fold(0.0, f64::max);
        assert!(max_recovered < 1e-5, "max recovered force={max_recovered}");
    }

    #[test]
    fn test_wall_element_rigid_rotation_horizontal_axis_zero_force() {
        assert_rigid_rotation_zero_force(0, false);
    }

    #[test]
    fn test_wall_element_rigid_rotation_normal_axis_zero_force() {
        assert_rigid_rotation_zero_force(1, false);
    }

    #[test]
    fn test_wall_element_rigid_rotation_vertical_axis_zero_force() {
        assert_rigid_rotation_zero_force(2, false);
    }

    #[test]
    fn test_fiber_wall_rigid_rotations_zero_force() {
        for axis in 0..3 {
            assert_rigid_rotation_zero_force(axis, true);
        }
    }

    #[test]
    fn test_wall_element_inplane_shear_matches_column() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };
        let k = wall.stiffness_24(&ctx);
        let mut u = [0.0; 24];
        u[2 * 6] = 1.0;
        u[3 * 6] = 1.0;
        let uku = energy(&k, &u);

        let col = &wall.column;
        let phi = 12.0 * col.e * col.iz / (col.g * col.as_y * col.length * col.length);
        let expected = 12.0 * col.e * col.iz / ((1.0 + phi) * col.length.powi(3));
        assert!(
            (uku - expected).abs() / expected < 1e-6,
            "uKu={uku} expected={expected}"
        );
    }

    #[test]
    fn test_wall_element_vertical_matches_axial() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };
        let k = wall.stiffness_24(&ctx);
        let mut u = [0.0; 24];
        u[2 * 6 + 2] = 1.0;
        u[3 * 6 + 2] = 1.0;
        let uku = energy(&k, &u);
        let col = &wall.column;
        let expected = col.e * col.a / col.length;
        assert!(
            (uku - expected).abs() / expected < 1e-6,
            "uKu={uku} expected={expected}"
        );
    }

    #[test]
    fn test_wall_element_corner_rotations_are_pinned() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };
        let k = wall.stiffness_24(&ctx);
        for n in 0..4 {
            for d in 3..6 {
                let idx = n * 6 + d;
                for j in 0..24 {
                    assert!(
                        k.get(idx, j).abs() < 1e-9 && k.get(j, idx).abs() < 1e-9,
                        "回転自由度に剛性: node={n} dof={d}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_opening_strength_reduction_no_opening_is_one() {
        let (model, data) = make_wall_model();
        assert!((WallElement::opening_strength_reduction(&data, &model) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn test_opening_strength_reduction_with_opening() {
        use sepika_core::model::{WallAttr, WallOpening};

        let (mut model, data) = make_wall_model();
        model.wall_attrs.push(WallAttr {
            elem: ElemId(0),
            opening_area: 0.0,
            opening_weight: 0.0,
            slit: Default::default(),
            finish_intensity: 0.0,
            openings: vec![WallOpening {
                width: 2000.0,
                height: 1500.0,
                offset: None,
            }],
        });
        let r2 = WallElement::opening_strength_reduction(&data, &model);
        let expected = sepika_core::rc_wall_capacity::wall_opening_reduction_strength(Some((
            2000.0, 1500.0, 3000.0, 4000.0,
        )));
        assert!((r2 - expected).abs() < 1e-12);
        assert!(r2 < 1.0);
        assert!((r2 - 0.5).abs() < 1e-12);
    }

    #[test]
    fn test_wall_element_opening_reduces_inplane_shear() {
        let (mut model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let k_no = {
            let ctx = Ctx { model: &model };
            wall.stiffness_24(&ctx)
        };
        model.wall_attrs.push(sepika_core::model::WallAttr {
            elem: ElemId(0),
            opening_area: 3.0e6,
            opening_weight: 0.0,
            slit: Default::default(),
            openings: vec![],
            finish_intensity: 0.0,
        });
        let wall_o = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };
        let k_o = wall_o.stiffness_24(&ctx);
        let mut u = [0.0; 24];
        u[2 * 6] = 1.0;
        u[3 * 6] = 1.0;
        assert!(
            energy(&k_o, &u) < energy(&k_no, &u) * 0.999,
            "開口低減が面内せん断剛性に効いていない"
        );
    }

    /// 鉄筋剛性の考慮: a = t·lw·(1+(n−1)·ps)、n=Es/Ec。
    #[test]
    fn test_wall_element_rebar_factor() {
        let (model, data) = make_wall_model();
        let wall = WallElement::try_new(&data, &model).unwrap();
        let ec = model.materials[0].young;
        let n = sepika_core::section_shape::E_STEEL / ec;
        let expected = 150.0 * 4000.0 * (1.0 + (n - 1.0) * 0.0025);
        assert!((wall.column.a - expected).abs() < 1e-6);
        assert!(
            (wall.column.mass_properties.mass_per_length - 24.0 / 9.80665e9 * 150.0 * 4000.0).abs()
                < 1e-12
        );
    }

    /// 側柱があるとせん断断面に算入され、I 形の形状係数 κ が用いられる。
    #[test]
    fn test_wall_element_side_columns_increase_shear_area() {
        let (mut model, data) = make_wall_model();
        let wall_plain = WallElement::try_new(&data, &model).unwrap();

        let col_shape = SectionShape::RcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: sepika_core::section_shape::RcRectColumnRebar {
                main_dia: 22.0,
                x: vec![8],
                y: vec![8],
                cover: 50.0,
                hoop: sepika_core::section_shape::RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
        };
        let column_id = SectionId(model.sections.len() as u32);
        let mut col_section = col_shape.to_section(column_id, "C600".into());
        col_section.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
        col_section.material = Some(MaterialId(0));
        model.sections.push(col_section);
        let base = model.elements.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
        for (i, (a, b)) in [(NodeId(0), NodeId(3)), (NodeId(1), NodeId(2))]
            .into_iter()
            .enumerate()
        {
            model.elements.push(ElementData {
                id: ElemId(base + i as u32),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![a, b],
                section: Some(column_id),
                local_axis: LocalAxis {
                    ref_vector: [1.0, 0.0, 0.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                plastic_zone: None,
                spring: None,
            });
        }
        let wall_cols = WallElement::try_new(&data, &model).unwrap();
        assert!(
            wall_cols.column.as_y > wall_plain.column.as_y,
            "側柱算入で as_y が増えない: {} vs {}",
            wall_cols.column.as_y,
            wall_plain.column.as_y
        );
        let area = 1_230_000.0;
        let kappa = sepika_core::section_shape::wall_shear_shape_factor_isection(
            4600.0, 600.0, 600.0, 150.0,
        );
        assert!(
            (wall_cols.column.as_y / (area / kappa) - 1.0).abs() < 1e-12,
            "as_y={}, expected={}",
            wall_cols.column.as_y,
            area / kappa
        );
        let column_material_id = MaterialId(model.materials.len() as u32);
        let mut column_material = model.materials[0].clone();
        column_material.id = column_material_id;
        column_material.shear = Some(model.materials[0].shear_modulus() * 2.0);
        model.materials.push(column_material);
        model.sections[column_id.index()].material = Some(column_material_id);
        let stiffer_columns = WallElement::try_new(&data, &model).unwrap();
        assert!(stiffer_columns.column.as_y > wall_cols.column.as_y);
        assert!(stiffer_columns.column.as_y < 2.0 * wall_cols.column.as_y);
    }

    #[test]
    fn test_wall_element_try_new_fallbacks() {
        let (model, mut data) = make_wall_model();
        data.nodes = smallvec::smallvec![NodeId(0), NodeId(2)];
        assert!(WallElement::try_new(&data, &model).is_none());
    }

    /// トライアル追従の回帰テスト: update_state(du, commit=false) が internal_force に
    /// 反映され（内力 = K24·u と厳密に一致）、commit / revert / snapshot / restore が
    /// beam/behavior.rs と同じ規律で機能すること。
    ///
    /// 本テストの K·u 比較は「internal_force と tangent_stiffness が将来ズレない」
    /// ことの回帰ガードであり、K24 の値そのものの正しさは独立の解析解と照合する
    /// `test_wall_element_inplane_shear_matches_column`（12EI/((1+φ)h³)）が担保する
    /// （両者を合わせて非循環な検証となる）。
    #[test]
    fn test_wall_element_trial_displacement_tracking() {
        use crate::behavior::{Ctx, ElementBehavior, LocalVec};
        let (model, data) = make_wall_model();
        let mut wall = WallElement::try_new(&data, &model).unwrap();
        let ctx = Ctx { model: &model };

        let mut du = LocalVec {
            data: smallvec::smallvec![0.0; 24],
        };
        du.data[2 * 6] = 1.0;
        du.data[3 * 6] = 1.0;
        let snap = wall.snapshot_state();
        wall.update_state(&du, false, &ctx).unwrap();

        let f = wall.internal_force(&ctx);
        let k = wall.stiffness_24(&ctx);
        for i in 0..24 {
            let expected: f64 = (0..24).map(|j| k.get(i, j) * du.data[j]).sum();
            assert!(
                (f.data[i] - expected).abs() <= 1e-9 * expected.abs().max(1.0),
                "内力が K·u と不一致: i={i} f={} expected={expected}",
                f.data[i]
            );
        }
        assert!(f.data[2 * 6].abs() > 1.0, "壁の復元力が生じていない");

        wall.commit_state();
        wall.update_state(&du, false, &ctx).unwrap();
        wall.revert_state();
        let f2 = wall.internal_force(&ctx);
        for i in 0..24 {
            assert!((f2.data[i] - f.data[i]).abs() < 1e-9);
        }

        wall.restore_state(&*snap);
        let f0 = wall.internal_force(&ctx);
        assert!(f0.data.iter().all(|v| v.abs() < 1e-12));
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, SectionId};
    use sepika_core::model::{ElementKind, EndCondition, ForceRegime, LocalAxis, Node};
    use sepika_core::section_shape::SectionShape;

    /// 任意の 4 隅座標・任意の節点並び順で壁要素データを作る。
    fn wall_with(coords: [[f64; 3]; 4], order: [u32; 4]) -> (Model, ElementData) {
        let shape = SectionShape::RcWall {
            thickness: 150.0,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
            ps: 0.0025,
        };
        let model = Model {
            nodes: coords
                .iter()
                .enumerate()
                .map(|(i, c)| Node {
                    id: NodeId(i as u32),
                    coord: *c,
                    restraint: Dof6Mask::FREE,
                    mass: None,
                    story: None,
                    support_spring: None,
                })
                .collect(),
            sections: vec![shape.to_section(SectionId(0), "W150".into())],
            ..Default::default()
        };
        let data = ElementData {
            id: ElemId(0),
            kind: ElementKind::Wall,
            nodes: order.iter().map(|i| NodeId(*i)).collect(),
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        let mut model = model;
        explicit_wall_test_rebar(&mut model);
        (model, data)
    }

    /// 台形壁（下辺 4000・上辺 2000）の壁長は上下辺の平均 3000 になる。
    /// 下辺だけ／上辺だけを採ると 4000／2000 となり代表長さにならない。
    #[test]
    fn test_wall_length_is_average_of_top_and_bottom_for_trapezoid() {
        let coords = [
            [0.0, 0.0, 0.0],
            [4000.0, 0.0, 0.0],
            [3000.0, 0.0, 3000.0],
            [1000.0, 0.0, 3000.0],
        ];
        let (model, data) = wall_with(coords, [0, 1, 2, 3]);
        let g = wall_element_geometry(&data, &model).expect("Some");
        assert!((g.lw_bottom - 4000.0).abs() < 1e-6, "{}", g.lw_bottom);
        assert!((g.lw_top - 2000.0).abs() < 1e-6, "{}", g.lw_top);
        assert!(
            (g.lw - 3000.0).abs() < 1e-6,
            "台形壁の壁長は上下辺の平均 3000 であるべき。got {}",
            g.lw
        );
        assert!((g.h - 3000.0).abs() < 1e-6, "{}", g.h);
    }

    /// 節点の並び順に依存しない（z でソートして下辺・上辺を決める）。
    /// 並び順を変えても壁長・高さは不変であること。特に「先頭 2 節点が鉛直辺」に
    /// なる並びでも壁高さを壁長として拾わないこと。
    #[test]
    fn test_wall_geometry_pairs_shifted_top_in_bottom_axis_order() {
        let coords = [
            [0.0, 0.0, 0.0],
            [4000.0, 0.0, 0.0],
            [-1000.0, 0.0, 3000.0],
            [-5000.0, 0.0, 3000.0],
        ];
        let (model, data) = wall_with(coords, [0, 1, 2, 3]);
        let geom = wall_element_geometry(&data, &model).unwrap();
        assert_eq!(geom.top, [NodeId(3), NodeId(2)]);
        let boundary = [geom.bottom[0], geom.bottom[1], geom.top[1], geom.top[0]];
        let points = boundary.map(|n| model.nodes[n.index()].coord);
        assert!((sepika_core::geom::polygon::area_3d(&points) - 12_000_000.0).abs() < 1e-6);
    }

    #[test]
    fn test_wall_geometry_is_independent_of_node_order() {
        let coords = [
            [0.0, 0.0, 0.0],
            [4000.0, 0.0, 0.0],
            [4000.0, 0.0, 3000.0],
            [0.0, 0.0, 3000.0],
        ];
        let (model, data) = wall_with(coords, [0, 3, 1, 2]);
        let g = wall_element_geometry(&data, &model).expect("Some");
        assert!(
            (g.lw - 4000.0).abs() < 1e-6,
            "節点順に依らず壁長 4000（壁高さ 3000 ではない）。got {}",
            g.lw
        );
        assert!((g.h - 3000.0).abs() < 1e-6, "{}", g.h);
    }
}

#[cfg(test)]
mod shear_yield_tests {
    use super::*;
    use crate::behavior::{Ctx, LocalVec};
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, SectionId};
    use sepika_core::model::MaterialCategory;
    use sepika_core::model::{ElementKind, EndCondition, ForceRegime, LocalAxis, Material, Node};
    use sepika_core::section_shape::SectionShape;

    fn wall_model() -> (Model, ElementData) {
        let shape = SectionShape::RcWall {
            thickness: 200.0,
            pwh_ratio: Some(0.0025),
            pwv_ratio: Some(0.0025),
            ps: 0.0025,
        };
        let mk = |id: u32, c: [f64; 3]| Node {
            id: NodeId(id),
            coord: c,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let model = Model {
            nodes: vec![
                mk(0, [0.0, 0.0, 0.0]),
                mk(1, [4000.0, 0.0, 0.0]),
                mk(2, [4000.0, 0.0, 3000.0]),
                mk(3, [0.0, 0.0, 3000.0]),
            ],
            sections: vec![sepika_core::model::Section {
                material: Some(MaterialId(0)),
                ..shape.to_section(SectionId(0), "W200".into())
            }],
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "FC24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            }],
            ..Default::default()
        };
        let data = ElementData {
            id: ElemId(0),
            kind: ElementKind::Wall,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        let mut model = model;
        model.elements.push(data.clone());
        crate::wall::add_surrounding_frame(&mut model, &data);
        let mut model = model;
        explicit_wall_test_rebar(&mut model);
        (model, data)
    }

    #[test]
    fn wall_shear_material_requires_explicit_supported_rebar_and_finite_positive_fy() {
        let (mut model, data) = wall_model();
        let rebar_id = model.sections[0].shear_rebar_material.unwrap();
        for fy in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::NEG_INFINITY),
        ] {
            model.materials[rebar_id.index()].fy = fy;
            let reason = fixture_capacity(&data, &model).unwrap_err();
            assert!(
                reason.contains("横筋材料")
                    && reason.contains("要素ID 0")
                    && reason.contains("断面ID Some(0)")
            );
            assert_eq!(
                WallElement::wall_shear_capacity_issue(
                    &data,
                    &model,
                    crate::factory::StrengthBasis::Nominal
                )
                .unwrap(),
                reason
            );
        }
        model.materials[rebar_id.index()].fy = Some(295.0);
        model.materials[rebar_id.index()].name = "SD295A".into();
        let qu295 = fixture_capacity(&data, &model).unwrap();
        model.materials[rebar_id.index()].fy = Some(345.0);
        let qu345 = fixture_capacity(&data, &model).unwrap();
        assert!(qu345 > qu295);
        model.materials[rebar_id.index()].name = "KH785".into();
        assert!(fixture_capacity(&data, &model).is_err());
        model.materials[rebar_id.index()].name = "SD295A".into();
        model.materials[rebar_id.index()].category = MaterialCategory::Steel;
        assert!(fixture_capacity(&data, &model).is_err());
        for assignment in [None, Some(MaterialId(999))] {
            model.sections[0].shear_rebar_material = assignment;
            assert!(fixture_capacity(&data, &model).is_err());
        }
    }

    /// 対応グレード（SR235）でも fy 未設定なら Qu は算定不能（0）・入力不備とし、
    /// fy を設定すれば算定できる。
    #[test]
    fn test_wall_qu_requires_fy_for_supported_grade() {
        let (mut model, data) = wall_model();
        model.materials[1] = Material {
            strength_factor: Some(1.0),
            concrete_class: Default::default(),
            id: MaterialId(1),
            name: "SR235".into(),
            category: MaterialCategory::Rebar,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: None,
        };
        model.sections[0].shear_rebar_material = Some(MaterialId(1));

        assert!(fixture_capacity(&data, &model).is_err());
        let issue = WallElement::wall_shear_capacity_issue(
            &data,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("fy 未設定は入力不備");
        assert!(issue.contains("SR235") && issue.contains("fy"), "{issue}");

        model.materials[1].fy = Some(235.0);
        assert!(fixture_capacity(&data, &model).unwrap() > 0.0);
        assert!(
            WallElement::wall_shear_capacity_issue(
                &data,
                &model,
                crate::factory::StrengthBasis::Nominal
            )
            .is_none(),
            "fy 設定時は入力不備なし"
        );
    }

    /// 非線形経路（プッシュオーバー）では耐震壁の面内水平力が終局せん断強度 Qu で
    /// 頭打ちになる。
    #[test]
    fn test_wall_shear_yields_at_ultimate_strength() {
        let (model, data) = wall_model();
        let qu = fixture_capacity(&data, &model).unwrap();
        assert!(qu > 0.0, "Qu が算定できるはず");

        let mut b = crate::factory::build_nonlinear_behavior(
            &data,
            &model,
            crate::factory::StrengthBasis::MaterialStrength,
            crate::factory::AnalysisKind::Incremental,
            Some([qu; 2]),
        );
        let ctx = Ctx { model: &model };
        let mut max_q: f64 = 0.0;
        for _ in 0..300 {
            let mut du = LocalVec {
                data: smallvec::SmallVec::from_elem(0.0, 24),
            };
            du.data[12] = 1.0;
            du.data[18] = 1.0;
            b.update_state(&du, false, &ctx).unwrap();
            b.commit_state();
            let f = b.internal_force(&ctx);
            max_q = max_q.max((f.data[0] + f.data[6]).abs());
            let recorded = b.state_member_forces(&ctx).expect("壁柱の状態断面力");
            let recorded_q = recorded.at[0].1[1].abs();
            assert!((recorded_q - (f.data[0] + f.data[6]).abs()).abs() < qu * 1e-8);
        }
        assert!(
            max_q <= qu * 1.001,
            "壁の水平力 {:.3e} N が終局せん断強度 Qu={:.3e} N を超えている",
            max_q,
            qu
        );
        assert!(
            max_q > qu * 0.99,
            "max_q={:.3e} が Qu={:.3e} に達していない",
            max_q,
            qu
        );
    }

    /// 面内せん断ばねの既定履歴（最大点指向型）: 除荷・再載荷が最大経験点を指向
    /// する割線となり、除荷しても変形が完全には戻らず（残留変形）、再載荷の
    /// 中間点では Qu より明確に小さい力（ピンチング）、最大経験変位まで戻すと
    /// Qu へ復帰する。
    #[test]
    fn test_wall_shear_hysteresis_is_max_point_oriented() {
        let (model, data) = wall_model();
        let qu = fixture_capacity(&data, &model).unwrap();
        assert!(qu > 0.0);
        let mut b = crate::factory::build_nonlinear_behavior(
            &data,
            &model,
            crate::factory::StrengthBasis::MaterialStrength,
            crate::factory::AnalysisKind::TimeHistory,
            Some([qu; 2]),
        );
        let ctx = Ctx { model: &model };
        let push = |b: &mut Box<dyn ElementBehavior>, d: f64| -> f64 {
            let mut du = LocalVec {
                data: smallvec::SmallVec::from_elem(0.0, 24),
            };
            du.data[12] = d;
            du.data[18] = d;
            b.update_state(&du, false, &ctx).unwrap();
            b.commit_state();
            let f = b.internal_force(&ctx);
            let recorded = b.state_member_forces(&ctx).expect("時刻歴の壁柱断面力");
            assert!((recorded.at[0].1[1] - (f.data[0] + f.data[6])).abs() < qu * 1e-8);
            f.data[0] + f.data[6]
        };

        let mut q_peak = 0.0;
        for _ in 0..30 {
            q_peak = push(&mut b, 1.0);
        }
        assert!(
            (q_peak.abs() - qu).abs() <= qu * 0.02,
            "ピークで Qu: {q_peak:.3e} vs {qu:.3e}"
        );
        let sgn = q_peak.signum();

        let mut q = q_peak;
        let mut n_unload = 0;
        while q * sgn > qu * 0.02 && n_unload < 29 {
            q = push(&mut b, -1.0);
            n_unload += 1;
        }
        assert!(
            n_unload < 29,
            "除荷完了までの戻し量 {n_unload}mm が押し量より小さい（残留変形）"
        );

        let mut q_mid = 0.0;
        for i in 0..n_unload {
            q = push(&mut b, 1.0);
            if i == n_unload / 2 {
                q_mid = q;
            }
        }
        assert!(
            q_mid * sgn < qu * 0.9,
            "再載荷中間点はピンチング（割線上）: {:.3e} vs Qu={:.3e}",
            q_mid,
            qu
        );
        assert!(
            (q * sgn - qu).abs() <= qu * 0.05,
            "最大経験変位で Qu へ復帰: {:.3e} vs {:.3e}",
            q,
            qu
        );
    }

    /// 弾性経路（許容応力度計算）では降伏しない（線形）。
    #[test]
    fn test_wall_stays_elastic_in_linear_path() {
        let (model, data) = wall_model();
        let mut b = crate::factory::build_behavior(&data, &model);
        let ctx = Ctx { model: &model };
        let mut q_at = vec![];
        for step in 1..=200 {
            let mut du = LocalVec {
                data: smallvec::SmallVec::from_elem(0.0, 24),
            };
            du.data[12] = 1.0;
            du.data[18] = 1.0;
            b.update_state(&du, false, &ctx).unwrap();
            b.commit_state();
            if step == 100 || step == 200 {
                let f = b.internal_force(&ctx);
                q_at.push((f.data[0] + f.data[6]).abs());
            }
        }
        assert!(
            (q_at[1] - 2.0 * q_at[0]).abs() < q_at[1] * 1e-9,
            "弾性経路は線形であるべき: {:?}",
            q_at
        );
    }
}

#[cfg(test)]
mod capacity_issue_tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, MaterialId, SectionId};
    use sepika_core::model::MaterialCategory;
    use sepika_core::model::{
        ElementKind, EndCondition, ForceRegime, LocalAxis, Material, Node, Section,
    };
    use sepika_core::section_shape::{RcRectColumnRebar, RectColumnHoop, SectionShape};

    /// 側柱あり／なし、側柱断面の指定を切り替えて壁モデルを作る。
    fn model_with(side_col_sec: Option<Section>, ps: f64) -> (Model, ElementData) {
        let shape = SectionShape::RcWall {
            thickness: 200.0,
            ps,
            pwh_ratio: Some(ps),
            pwv_ratio: Some(ps),
        };
        let mk = |id: u32, c: [f64; 3]| Node {
            id: NodeId(id),
            coord: c,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        };
        let mut sections = vec![Section {
            material: Some(MaterialId(0)),
            ..shape.to_section(SectionId(0), "W200".into())
        }];
        let mut elements = vec![];
        let mut edge = |id: u32, n0: u32, n1: u32, sec: Option<SectionId>| {
            elements.push(ElementData {
                id: ElemId(id),
                kind: ElementKind::Beam,
                nodes: smallvec::smallvec![NodeId(n0), NodeId(n1)],
                section: sec,
                local_axis: LocalAxis {
                    ref_vector: [1.0, 0.0, 0.0],
                },
                end_cond: [EndCondition::Fixed, EndCondition::Fixed],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                plastic_zone: None,
                spring: None,
            });
        };
        let side_sec = side_col_sec.map(|mut cs| {
            cs.id = SectionId(1);
            cs.frame_use = Some(sepika_core::model::FrameSectionUse::Column);
            cs.material = Some(MaterialId(0));
            cs.rebar_material = Some(MaterialId(0));
            cs.shear_rebar_material = Some(MaterialId(0));
            sections.push(cs);
            SectionId(1)
        });
        let girder_id = SectionId(sections.len() as u32);
        let mut girder = Section::zero(girder_id, "G600".into());
        girder.frame_use = Some(sepika_core::model::FrameSectionUse::Girder);
        girder.material = Some(MaterialId(0));
        sections.push(girder);
        edge(1, 0, 1, Some(girder_id));
        edge(2, 3, 2, Some(girder_id));
        if let Some(sec) = side_sec {
            edge(3, 0, 3, Some(sec));
            edge(4, 1, 2, Some(sec));
        }
        let wall = ElementData {
            id: ElemId(0),
            kind: ElementKind::Wall,
            nodes: smallvec::smallvec![NodeId(0), NodeId(1), NodeId(2), NodeId(3)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        };
        elements.insert(0, wall.clone());
        let model = Model {
            nodes: vec![
                mk(0, [0.0, 0.0, 0.0]),
                mk(1, [4000.0, 0.0, 0.0]),
                mk(2, [4000.0, 0.0, 3000.0]),
                mk(3, [0.0, 0.0, 3000.0]),
            ],
            elements,
            sections,
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "FC24".into(),
                category: MaterialCategory::Concrete,
                young: 23000.0,
                poisson: 0.2,
                density: 2.4e-9,
                shear: None,
                fc: Some(24.0),
                fy: None,
            }],
            ..Default::default()
        };
        let mut model = model;
        explicit_wall_test_rebar(&mut model);
        (model, wall)
    }

    fn rc_col(with_rebar: bool) -> Section {
        let shape = if with_rebar {
            SectionShape::RcColumnRect {
                b: 600.0,
                d: 600.0,
                rebar: RcRectColumnRebar {
                    main_dia: 22.0,
                    x: vec![4],
                    y: vec![3],
                    cover: 40.0,
                    hoop: RectColumnHoop {
                        dia: 10.0,
                        pitch: 100.0,
                        legs_x: 2,
                        legs_y: 2,
                    },
                },
            }
        } else {
            SectionShape::RcColumnRect {
                b: 600.0,
                d: 600.0,
                rebar: RcRectColumnRebar {
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
                },
            }
        };
        shape.to_section(SectionId(1), "C600".into())
    }

    #[test]
    fn beam_fiber_ms_share_independent_wall_section_and_both_capacity_limits() {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
            for member in model
                .elements
                .iter_mut()
                .filter(|e| e.kind != ElementKind::Wall)
            {
                member.kind = kind;
            }
            let section = crate::wall::shear_section::WallSection::new(&wall, &model).unwrap();
            assert!(section.columns.iter().all(Option::is_some));
            let properties = section.properties().unwrap();
            assert!((properties.area_mm2 - 1_400_000.0).abs() < 1e-6);
            assert!((properties.elastic_centroid_mm - 2000.0).abs() < 1e-8);
            assert!(
                (properties.flexural_rigidity_n_mm2 / 81_803_333_333_333_330.0 - 1.0).abs() < 1e-6
            );
            // D=4600、d=4300、te=300 mm、主筋10-D22、横筋比0.0025・345 MPa、N=0。
            let capacity =
                fixture_directional_capacity(&wall, &model, crate::factory::StrengthBasis::Nominal)
                    .unwrap();
            for limit in capacity {
                assert!(
                    (limit - 3_027_363.174_700_624).abs() < 1e-6,
                    "{kind:?}: {capacity:?}"
                );
            }
        }
    }

    #[test]
    fn pure_steel_wall_section_is_plate_only_for_all_three_boundary_representations() {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
            model.materials[0].category = MaterialCategory::Steel;
            model.materials[0].young = 205000.0;
            model.materials[0].poisson = 0.3;
            model.materials[0].fy = Some(235.0);
            model.sections[0].shape = None;
            model.sections[0].thickness = Some(9.0);
            model.sections[1].shape = Some(SectionShape::SteelH {
                height: 600.0,
                width: 300.0,
                web_thick: 12.0,
                flange_thick: 20.0,
                root_r: Some(0.0),
            });
            for member in model
                .elements
                .iter_mut()
                .filter(|e| e.kind != ElementKind::Wall)
            {
                member.kind = kind;
            }
            let section = crate::wall::shear_section::WallSection::new(&wall, &model).unwrap();
            assert!(section.columns.iter().all(Option::is_none));
            let properties = section.properties().unwrap();
            assert!((properties.area_mm2 - 36_000.0).abs() < 1e-6);
            assert!(
                (properties.flexural_rigidity_n_mm2 / 9_840_000_000_000_000.0 - 1.0).abs() < 1e-6,
                "actual EI={} Nmm², independent 205000*9*4000³/12=9.84e15 Nmm²",
                properties.flexural_rigidity_n_mm2
            );
            assert_eq!(
                fixture_directional_capacity(&wall, &model, crate::factory::StrengthBasis::Nominal)
                    .unwrap(),
                [WallElement::steel_shear_capacity_of(&wall, &model); 2]
            );
        }
    }

    #[test]
    fn cft_wall_section_uses_each_explicit_material_and_keeps_capacity_gate() {
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
        ] {
            let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
            let steel_id = MaterialId(model.materials.len() as u32);
            let mut steel = model.materials[0].clone();
            steel.id = steel_id;
            steel.category = MaterialCategory::Steel;
            steel.young = 205000.0;
            steel.shear = Some(80000.0);
            steel.fy = Some(235.0);
            model.materials.push(steel);
            model.sections[1].shape = Some(SectionShape::CftBox {
                height: 600.0,
                width: 600.0,
                thick: 20.0,
                corner_r: Some(0.0),
            });
            model.sections[1].steel_material = Some(steel_id);
            for member in model
                .elements
                .iter_mut()
                .filter(|e| e.kind != ElementKind::Wall)
            {
                member.kind = kind;
            }
            let section = crate::wall::shear_section::WallSection::new(&wall, &model).unwrap();
            assert!(section.columns.iter().all(Option::is_some));
            let properties = section.properties().unwrap();
            assert!((properties.area_mm2 - 1_400_000.0).abs() < 1e-6);
            // Fc24・23 kN/m3のEc=22668.94590387348 MPa、鋼管600角t20、壁中央3400×200 mm。
            assert!(
                (properties.flexural_rigidity_n_mm2 / 1.4947382811333088e+17 - 1.0).abs() < 1e-6
            );
            let reason = WallElement::wall_shear_capacity_issue(
                &wall,
                &model,
                crate::factory::StrengthBasis::Nominal,
            )
            .unwrap();
            assert!(reason.contains("CFT") && reason.contains("適用外"));
            assert!(fixture_capacity(&wall, &model).is_err());
            model.sections[1].steel_material = None;
            let error = crate::wall::shear_section::WallSection::new(&wall, &model)
                .err()
                .unwrap();
            assert!(
                error.contains("MissingSteelMaterial")
                    && error.contains("0")
                    && error.contains("3")
            );
            model.sections[1].steel_material = Some(MaterialId(0));
            let error = crate::wall::shear_section::WallSection::new(&wall, &model)
                .err()
                .unwrap();
            assert!(error.contains("InvalidSteelMaterial"));
        }
    }

    #[test]
    fn wall_section_rejects_missing_owner_and_conflicting_wall_geometry() {
        let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
        model.elements.retain(|e| e.id != wall.id);
        let error = crate::wall::shear_section::WallSection::new(&wall, &model)
            .err()
            .unwrap();
        assert!(error.contains("壁 ID 0") && error.contains("登録"));
        model.elements.push(wall.clone());
        let mut conflict = wall.clone();
        conflict.nodes.swap(0, 1);
        let error = crate::wall::shear_section::WallSection::new(&conflict, &model)
            .err()
            .unwrap();
        assert!(error.contains("壁 ID 0") && error.contains("一致"));
        let mut construction_copy = wall.clone();
        construction_copy.end_cond = [EndCondition::Pinned; 2];
        assert!(crate::wall::shear_section::WallSection::new(&construction_copy, &model).is_ok());
    }

    #[test]
    fn different_column_reinforcement_produces_directional_wall_limits() {
        let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
        let mut right = model.sections[1].clone();
        let right_id = SectionId(model.sections.len() as u32);
        right.id = right_id;
        if let Some(SectionShape::RcColumnRect { rebar, .. }) = &mut right.shape {
            for c in rebar.x.iter_mut() {
                *c *= 2;
            }
            for c in rebar.y.iter_mut() {
                *c *= 2;
            }
        }
        model.sections.push(right);
        model
            .elements
            .iter_mut()
            .find(|e| e.id == ElemId(4))
            .unwrap()
            .section = Some(right_id);
        let qu =
            fixture_directional_capacity(&wall, &model, crate::factory::StrengthBasis::Nominal)
                .unwrap();
        assert!(qu[1] > qu[0] && qu[0] > 0.0, "{qu:?}");
        for mode in 0..3 {
            let limits = if mode == 2 { qu.map(|q| q * 0.001) } else { qu };
            let mut element = WallElement::try_new_scaled(&wall, &model, 1.0)
                .unwrap()
                .with_shear_capacity(limits);
            if mode > 0 {
                element = element.with_shear_hysteresis(HysteresisModel::MaxPointOriented);
            }
            if mode == 2 {
                element = element
                    .with_fiber_flexure(
                        &wall,
                        &model,
                        crate::factory::StrengthBasis::Nominal,
                        crate::factory::AnalysisKind::Incremental,
                    )
                    .unwrap();
                assert!(element.fiber_column.is_some());
            }
            let ctx = Ctx { model: &model };
            let mut displacement = 0.0;
            for (target, expected) in [(100.0, limits[0]), (-150.0, -limits[1]), (150.0, limits[0])]
            {
                let start = displacement;
                for step in 1..=100 {
                    let next = start + (target - start) * step as f64 / 100.0;
                    let mut delta = LocalVec {
                        data: smallvec::smallvec![0.0; 24],
                    };
                    delta.data[12] = next - displacement;
                    delta.data[18] = next - displacement;
                    element.update_state(&delta, false, &ctx).unwrap();
                    element.commit_state();
                    displacement = next;
                }
                let f = element.internal_force(&ctx);
                let q = f.data[12] + f.data[18];
                assert!(
                    (q / expected - 1.0).abs() < 1e-7,
                    "正負耐力: q={q}, expected={expected}"
                );
            }
        }
        let left = model
            .elements
            .iter()
            .position(|e| e.id == ElemId(3))
            .unwrap();
        let right = model
            .elements
            .iter()
            .position(|e| e.id == ElemId(4))
            .unwrap();
        model.elements[left].section = Some(right_id);
        model.elements[right].section = Some(SectionId(1));
        let reversed =
            fixture_directional_capacity(&wall, &model, crate::factory::StrengthBasis::Nominal)
                .unwrap();
        assert_eq!(qu, [reversed[1], reversed[0]]);
    }

    /// 側柱があり主筋も設定されていれば不備なし・Qu が算定できる。
    #[test]
    fn test_no_issue_when_side_column_rebar_available() {
        let (model, wall) = model_with(Some(rc_col(true)), 0.0025);
        assert_eq!(
            WallElement::wall_shear_capacity_issue(
                &wall,
                &model,
                crate::factory::StrengthBasis::Nominal
            ),
            None
        );
        assert!(fixture_capacity(&wall, &model).unwrap() > 0.0);
    }

    /// 側柱はあるのに主筋が取得できない＝断面設定の不備。壁筋比で代替せずエラーとする。
    #[test]
    fn test_issue_when_side_column_has_no_main_rebar() {
        let (model, wall) = model_with(Some(rc_col(false)), 0.0025);
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("側柱主筋がなければ不備として検出されるべき");
        assert!(issue.contains("側柱"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }

    /// 側柱がない壁（壁のみの耐震壁）は不備ではなく、壁筋比から pte を算定する。
    #[test]
    fn test_no_side_column_uses_wall_rebar_ratio() {
        let (model, wall) = model_with(None, 0.0025);
        assert_eq!(
            WallElement::wall_shear_capacity_issue(
                &wall,
                &model,
                crate::factory::StrengthBasis::Nominal
            ),
            None
        );
        assert!(fixture_capacity(&wall, &model).unwrap() > 0.0);
    }

    /// 側柱も壁筋比もなければ pte を算定できないため不備とする。
    #[test]
    fn test_issue_when_no_side_column_and_no_wall_rebar() {
        let (model, wall) = model_with(None, 0.0);
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("側柱も壁筋もなければ不備");
        assert!(issue.contains("不成立"), "{}", issue);
    }

    /// コンクリート強度 Fc が未設定の壁は不備として検出する。
    /// Qu を算定できず弾性のまま解析すると保有水平耐力を過大評価する（危険側）。
    #[test]
    fn test_issue_when_fc_unset() {
        let (mut model, wall) = model_with(None, 0.0025);
        model.materials[0].fc = None;
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("Fc 未設定は不備");
        assert!(issue.contains("Fc"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }

    /// Fc が 0 以下または非有限値でも Qu を算定できないため不備とする（未設定と同じ扱い）。
    #[test]
    fn test_issue_when_fc_not_positive() {
        let (mut model, wall) = model_with(None, 0.0025);
        model.materials[0].fc = Some(0.0);
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("Fc<=0 は不備");
        assert!(issue.contains("Fc"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }

    /// 断面に材料が割り当てられていない壁も不備とする（Fc を参照できない）。
    #[test]
    fn test_issue_when_material_missing() {
        let (mut model, wall) = model_with(None, 0.0025);
        model.sections[0].material = None;
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("材料未設定は不備");
        assert!(issue.contains("コンクリート材料"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }

    /// 断面が割り当てられていない壁も不備とする（壁厚・壁筋比を参照できない）。
    #[test]
    fn test_issue_when_section_missing() {
        let (model, mut wall) = model_with(None, 0.0025);
        wall.section = None;
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("断面未設定は不備");
        assert!(issue.contains("断面が設定されていません"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }

    /// 4 節点を与えているのに幾何が退化した壁（下辺の 2 節点が同一座標）は、
    /// 壁エレメントも雑壁も組めず剛性・耐力を持たないまま消えるため不備として検出する。
    #[test]
    fn test_issue_when_wall_element_cannot_be_built() {
        let (mut model, wall) = model_with(None, 0.0025);
        model.nodes[1].coord = model.nodes[0].coord;
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("壁エレメントを構築できない壁は不備");
        assert!(
            issue.contains("壁エレメントとして構築できません"),
            "{}",
            issue
        );
    }

    /// 実配筋モデルの矩形柱側柱は総主筋量（`total_main_area`）を at として方向別耐力を与える。
    #[test]
    fn test_new_rect_column_side_column_uses_total_main_area() {
        use sepika_core::section_shape::one_bar_area;
        let new_rebar = RcRectColumnRebar {
            main_dia: 22.0,
            x: vec![5],
            y: vec![3],
            cover: 40.0,
            hoop: RectColumnHoop {
                dia: 10.0,
                pitch: 100.0,
                legs_x: 2,
                legs_y: 2,
            },
        };
        let expected_at = 12.0 * one_bar_area(22.0);
        assert!(
            (new_rebar.total_main_area() - expected_at).abs() < 1e-9,
            "テスト前提: 総主筋量が12本分であること"
        );

        let new_shape = SectionShape::RcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: new_rebar,
        };
        let (model_new, wall_new) = model_with(
            Some(new_shape.to_section(SectionId(1), "C600".into())),
            0.0025,
        );

        assert_eq!(
            WallElement::wall_shear_capacity_issue(
                &wall_new,
                &model_new,
                crate::factory::StrengthBasis::Nominal
            ),
            None
        );
        let new_qu = fixture_directional_capacity(
            &wall_new,
            &model_new,
            crate::factory::StrengthBasis::Nominal,
        )
        .unwrap();
        assert!(new_qu[0] > 0.0 && new_qu[1] > 0.0, "{new_qu:?}");
    }

    /// SRC 側柱を含む壁モデル。弾性断面を構築できるよう内蔵鉄骨材料まで与える。
    fn model_with_src_side_column(shape: SectionShape) -> (Model, ElementData) {
        let (mut model, wall) = model_with(
            Some(shape.to_section(SectionId(1), "SRC600".into())),
            0.0025,
        );
        let steel_id = MaterialId(model.materials.len() as u32);
        model.materials.push(Material {
            id: steel_id,
            name: "SN400".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        });
        model.sections[1].steel_material = Some(steel_id);
        let mut model = model;
        explicit_wall_test_rebar(&mut model);
        (model, wall)
    }

    /// SRC 矩形柱側柱は RC 耐力式の適用外として拒否する。
    #[test]
    fn test_issue_when_new_src_side_column() {
        let shape = SectionShape::SrcColumnRect {
            b: 600.0,
            d: 600.0,
            rebar: RcRectColumnRebar {
                main_dia: 22.0,
                x: vec![5],
                y: vec![3],
                cover: 40.0,
                hoop: RectColumnHoop {
                    dia: 10.0,
                    pitch: 100.0,
                    legs_x: 2,
                    legs_y: 2,
                },
            },
            steel_height: 500.0,
            steel_width: 300.0,
            steel_web_thick: 12.0,
            steel_flange_thick: 20.0,
        };
        let (model, wall) = model_with_src_side_column(shape);
        let issue = WallElement::wall_shear_capacity_issue(
            &wall,
            &model,
            crate::factory::StrengthBasis::Nominal,
        )
        .expect("SRC側柱はRC耐力式の適用外として拒否する");
        assert!(issue.contains("SRC"), "{}", issue);
        assert!(issue.contains("適用外"), "{}", issue);
        assert!(fixture_capacity(&wall, &model).is_err());
    }
    fn directional_fixture() -> (Model, ElementData) {
        let (mut model, wall) = model_with(Some(rc_col(true)), 0.0025);
        if let Some(SectionShape::RcWall {
            pwh_ratio,
            pwv_ratio,
            ..
        }) = model.sections[0].shape.as_mut()
        {
            *pwh_ratio = Some(0.002);
            *pwv_ratio = Some(0.006);
        }
        let mut transverse = model.materials[1].clone();
        transverse.id = MaterialId(2);
        transverse.name = "SD295A".into();
        transverse.fy = Some(295.0);
        model.materials.push(transverse);
        model.sections[0].shear_rebar_material = Some(MaterialId(2));
        (model, wall)
    }

    #[test]
    fn vertical_fibers_use_explicit_area_fy_and_own_strength_factor() {
        let (mut model, wall) = directional_fixture();
        for (factor, expected_fy, expected_yield_force) in [
            (Some(1.0), 345.0, 1_656_000.0),
            (Some(1.2), 414.0, 1_987_200.0),
            (None, 379.5, 1_821_600.0),
        ] {
            model.materials[0].strength_factor = Some(8.0);
            model.materials[1].strength_factor = factor;
            let element = WallElement::try_new(&wall, &model)
                .unwrap()
                .with_fiber_flexure(
                    &wall,
                    &model,
                    crate::factory::StrengthBasis::MaterialStrength,
                    sepika_core::model::AnalysisKind::Incremental,
                )
                .unwrap();
            let fiber = element.fiber_column.unwrap();
            for point in &fiber.gauss_points {
                let bars: Vec<_> = point
                    .section
                    .fibers
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.material == 1)
                    .collect();
                assert_eq!(bars.len(), 20);
                let mut area = 0.0;
                let mut yield_force = 0.0;
                for (index, bar) in bars {
                    assert!((bar.area - 240.0).abs() < 1e-10);
                    let material: sepika_material::MenegottoPinto =
                        bincode::deserialize(&point.mats[index].serialize_state()).unwrap();
                    assert!((material.fy - expected_fy).abs() < 1e-10);
                    area += bar.area;
                    yield_force += bar.area * material.fy;
                }
                assert!((area - 4800.0).abs() < 1e-10);
                assert!((yield_force - expected_yield_force).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn direct_directional_capacity_preserves_unscaled_transverse_strength() {
        let (mut model, wall) = directional_fixture();
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[0].shape {
            *pwh_ratio = Some(0.0);
        }
        let basis = crate::factory::StrengthBasis::MaterialStrength;
        let concrete = fixture_directional_capacity(&wall, &model, basis).unwrap();
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = &mut model.sections[0].shape {
            *pwh_ratio = Some(0.002);
        }
        for factor in [None, Some(1.0), Some(1.2)] {
            model.materials[2].strength_factor = factor;
            let expected = 601723.6279664652;
            let capacity = fixture_directional_capacity(&wall, &model, basis).unwrap();
            for direction in 0..2 {
                assert!((capacity[direction] - concrete[direction] - expected).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn transverse_and_vertical_inputs_are_independent_and_missing_is_not_zero() {
        let (mut model, wall) = directional_fixture();
        let basis = crate::factory::StrengthBasis::Nominal;
        let qu = fixture_directional_capacity(&wall, &model, basis).unwrap();
        let linear_mass = WallElement::try_new(&wall, &model).unwrap().mass_total;
        if let Some(SectionShape::RcWall { pwv_ratio, .. }) = model.sections[0].shape.as_mut() {
            *pwv_ratio = None;
        }
        assert_eq!(
            fixture_directional_capacity(&wall, &model, basis).unwrap(),
            qu
        );
        let fiber = WallElement::try_new(&wall, &model)
            .unwrap()
            .with_fiber_flexure(
                &wall,
                &model,
                basis,
                sepika_core::model::AnalysisKind::Incremental,
            );
        let reason = fiber.err().unwrap();
        assert_eq!(
            WallElement::wall_shear_capacity_issue(&wall, &model, basis).unwrap(),
            reason
        );
        assert!(reason.contains("縦筋比"));
        for value in [Some(0.0), Some(0.002), Some(0.006)] {
            if let Some(SectionShape::RcWall { pwv_ratio, .. }) = model.sections[0].shape.as_mut() {
                *pwv_ratio = value;
            }
            assert_eq!(
                fixture_directional_capacity(&wall, &model, basis).unwrap(),
                qu
            );
            let element = WallElement::try_new(&wall, &model)
                .unwrap()
                .with_fiber_flexure(
                    &wall,
                    &model,
                    basis,
                    sepika_core::model::AnalysisKind::Incremental,
                )
                .unwrap();
            assert_eq!(element.mass_total, linear_mass);
            assert!(element.fiber_column.is_some());
        }
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = model.sections[0].shape.as_mut() {
            *pwh_ratio = Some(0.0);
        }
        assert!(fixture_directional_capacity(&wall, &model, basis).is_ok());
        if let Some(SectionShape::RcWall { pwh_ratio, .. }) = model.sections[0].shape.as_mut() {
            *pwh_ratio = None;
        }
        assert!(fixture_directional_capacity(&wall, &model, basis)
            .unwrap_err()
            .contains("横筋比"));
    }

    #[test]
    fn nonlinear_reference_stiffness_and_shear_spring_use_vertical_ratio_before_cache() {
        let (mut model, wall) = directional_fixture();
        let basis = crate::factory::StrengthBasis::Nominal;
        let qu = fixture_directional_capacity(&wall, &model, basis).unwrap();
        let build = |model: &Model| {
            let element = WallElement::try_new(&wall, model).unwrap();
            element.column.local_stiffness();
            element
                .with_fiber_flexure(
                    &wall,
                    model,
                    basis,
                    sepika_core::model::AnalysisKind::Incremental,
                )
                .unwrap()
                .with_shear_capacity(qu)
                .with_shear_hysteresis(HysteresisModel::MaxPointOriented)
        };
        let first = build(&model);
        if let Some(SectionShape::RcWall { ps, .. }) = model.sections[0].shape.as_mut() {
            *ps = 0.02;
        }
        let changed_quantity_ratio = build(&model);
        assert_eq!(first.column.a, changed_quantity_ratio.column.a);
        assert_eq!(first.column.iz, changed_quantity_ratio.column.iz);
        assert_eq!(first.shear_k0, changed_quantity_ratio.shear_k0);
        if let Some(SectionShape::RcWall { pwv_ratio, .. }) = model.sections[0].shape.as_mut() {
            *pwv_ratio = Some(0.002);
        }
        let changed_vertical = build(&model);
        assert_ne!(first.column.a, changed_vertical.column.a);
        assert_ne!(first.column.iz, changed_vertical.column.iz);
        assert_ne!(first.shear_k0, changed_vertical.shear_k0);
    }
    #[test]
    fn vertical_inputs_reject_invalid_strength_ratio_category_and_factor() {
        let (mut model, wall) = directional_fixture();
        for fy in [
            None,
            Some(0.0),
            Some(-1.0),
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(f64::NEG_INFINITY),
        ] {
            model.materials[1].fy = fy;
            assert!(wall_rebar_strength(
                &wall,
                &model,
                true,
                crate::factory::StrengthBasis::Nominal
            )
            .unwrap_err()
            .contains("縦筋材料"));
        }
        model.materials[1].fy = Some(345.0);
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            model.materials[1].strength_factor = Some(factor);
            assert!(wall_rebar_strength(
                &wall,
                &model,
                true,
                crate::factory::StrengthBasis::MaterialStrength
            )
            .unwrap_err()
            .contains("強度倍率"));
        }
        model.materials[1].strength_factor = Some(1.0);
        model.materials[1].name = "任意345".into();
        assert!(
            wall_rebar_strength(&wall, &model, true, crate::factory::StrengthBasis::Nominal)
                .is_err()
        );
        model.materials[1].name = "SD345".into();
        model.materials[1].category = MaterialCategory::Steel;
        assert!(
            wall_rebar_strength(&wall, &model, true, crate::factory::StrengthBasis::Nominal)
                .is_err()
        );
        for vertical in [false, true] {
            for value in [
                None,
                Some(-0.002),
                Some(f64::NAN),
                Some(f64::INFINITY),
                Some(f64::NEG_INFINITY),
            ] {
                if let Some(SectionShape::RcWall {
                    pwh_ratio,
                    pwv_ratio,
                    ..
                }) = &mut model.sections[0].shape
                {
                    if vertical {
                        *pwv_ratio = value;
                    } else {
                        *pwh_ratio = value;
                    }
                }
                assert!(wall_direction_ratio(&wall, &model, vertical).is_err());
            }
        }
    }
}

#[cfg(test)]
use super::static_face::fixture as rc_wall572_fixture;

#[cfg(test)]
mod wall_method_tests {
    use super::*;
    use crate::factory::StrengthBasis;
    use sepika_core::model::RcWallShearMethod::{ArakawaMean, ArakawaMin};

    #[test]
    fn selected_method_reaches_both_directions_with_real_area_and_limited_te() {
        assert_eq!(
            rc_wall572_fixture::input_model()
                .stress_cfg
                .rc_wall_shear_method,
            ArakawaMean
        );
        let mut model = rc_wall572_fixture::expanded_model();
        let wall = model.elements[4].clone();
        for (method, expected) in [
            (ArakawaMin, 2_006_534.4713659945),
            (ArakawaMean, 2_676_420.1354640694),
        ] {
            model.stress_cfg.rc_wall_shear_method = method;
            let qu = WallElement::directional_shear_capacity_at_reference(
                &wall,
                &model,
                StrengthBasis::Nominal,
                1_200_000.0,
                [6450.0 / 4600.0; 2],
            )
            .unwrap();
            for value in qu {
                assert!((value - expected).abs() < 1e-6, "{method:?}: {value}");
            }
            let zero = WallElement::directional_shear_capacity_at_reference(
                &wall,
                &model,
                StrengthBasis::Nominal,
                0.0,
                [6450.0 / 4600.0; 2],
            )
            .unwrap();
            assert!((qu[0] - zero[0] - 91_520.27027027027).abs() < 1e-6);
            for n in [-1.0, 0.4 * 24.0 * 1_332_000.0 + 1.0, f64::NAN] {
                assert!(WallElement::directional_shear_capacity_at_reference(
                    &wall,
                    &model,
                    StrengthBasis::Nominal,
                    n,
                    [6450.0 / 4600.0; 2]
                )
                .is_err());
            }
            for ratios in [[f64::NAN, 1.0], [1.0, f64::INFINITY], [-1.0, 1.0]] {
                assert!(WallElement::directional_shear_capacity_at_reference(
                    &wall,
                    &model,
                    StrengthBasis::Nominal,
                    0.0,
                    ratios
                )
                .is_err());
            }
            let low = WallElement::directional_shear_capacity_at_reference(
                &wall,
                &model,
                StrengthBasis::Nominal,
                0.0,
                [0.1, 1.0],
            )
            .unwrap();
            assert_eq!(low[0], low[1]);
            let high = WallElement::directional_shear_capacity_at_reference(
                &wall,
                &model,
                StrengthBasis::Nominal,
                0.0,
                [3.0, 9.0],
            )
            .unwrap();
            assert_eq!(high[0], high[1]);
            let mut missing = model.clone();
            missing.sections[2].shear_rebar_material = None;
            assert!(WallElement::directional_shear_capacity_at_reference(
                &wall,
                &missing,
                StrengthBasis::Nominal,
                0.0,
                [1.0; 2]
            )
            .is_err());
        }
    }
}
