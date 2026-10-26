use crate::behavior::{Ctx, DuctilityProbe, ElementBehavior, LocalMat, LocalVec, MassOption};
use crate::frame::member_load::FrameDeadLoadMap;
use sepika_core::dof::DofMap;
use sepika_core::error::RebarGeometryError;
use sepika_core::ids::NodeId;
use sepika_core::model::{AnalysisKind, HysteresisModel, MemberLoad, SectionMassProperties};
use sepika_core::section_shape::SectionShape;
use sepika_material::uniaxial::{MenegottoPinto, UniaxialMaterial};
use sepika_section::fiber::{Fiber, FiberSection};
use sepika_section::mn_surface::StrengthParams;
use smallvec::SmallVec;
use std::any::Any;

/// 塑性化域長 Lp `[mm]` を部材長 `l` `[mm]` に対して有効な範囲へクランプする。
/// 各端 45% を上限、1e-6·L を下限とする。
pub fn clamp_plastic_zone(lp: f64, l: f64) -> f64 {
    lp.clamp(1.0e-6 * l, 0.45 * l)
}

/// ファイバー梁の端部断面分割（幅方向・せい方向）。表示 API も同じ格子を使う。
pub(crate) const FIBER_NW: usize = 12;
pub(crate) const FIBER_ND: usize = 20;

/// 鋼材・鉄筋のファイバ材料を生成する。
///
/// # Panics
///
/// fy 未設定・0 以下で panic する。
pub(crate) fn steel_fiber_material(e: f64, fy: Option<f64>) -> Box<dyn UniaxialMaterial> {
    let Some(fy) = fy.filter(|fy| *fy > 0.0) else {
        panic!(
            "ファイバー断面の鋼材・主筋に降伏強度 fy が未設定です。\
             解析前に factory::ensure_nonlinear_input で入力チェックを行ってください"
        );
    };
    Box::new(MenegottoPinto::new(e, fy))
}

/// コンクリートのファイバ材料を生成する。
///
/// `rule` は逆行型・原点指向型・Karsan–Jirsa 型のいずれかを渡す。
///
/// # Panics
///
/// 曲げバネ用履歴則の混入で panic する。
pub(crate) fn concrete_fiber_material(
    fc: Option<f64>,
    young: f64,
    rule: HysteresisModel,
) -> Box<dyn UniaxialMaterial> {
    let Some(fc) = fc.filter(|fc| *fc > 0.0) else {
        panic!(
            "ファイバー断面のコンクリートに設計基準強度 Fc が未設定です。\
             解析前に factory::ensure_nonlinear_input で入力チェックを行ってください"
        );
    };
    match rule {
        HysteresisModel::KarsanJirsa => {
            if fc <= 60.0 {
                Box::new(sepika_material::ConcreteCyclic::newrc_with_initial_tangent(
                    fc,
                    0.01,
                    2.0,
                    young / 10.0,
                    young,
                ))
            } else {
                let ec = sepika_material::newrc::NewRcEnvelope::new(fc).ec;
                Box::new(sepika_material::ConcreteCyclic::newrc(
                    fc,
                    0.01,
                    2.0,
                    ec / 10.0,
                ))
            }
        }
        HysteresisModel::Retrograde | HysteresisModel::OriginOriented => {
            let mut m = sepika_material::ConcreteNewRc::new(fc, 2.0);
            if fc <= 60.0 {
                m.set_initial_tangent(young);
            }
            m.set_concrete_hysteresis(rule == HysteresisModel::OriginOriented);
            Box::new(m)
        }
        other => panic!(
            "コンクリートのファイバ材料の除荷則として解釈できません: {other:?}\
            （逆行型・原点指向型・Karsan-Jirsa型のみ。\
             factory::resolve_fiber_concrete_hysteresis で解決した値を渡してください）"
        ),
    }
}

/// ファイバ断面の各領域の降伏点 [N/mm²]。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FiberYield {
    /// 主材料の fy（形状を持たない断面の格子に用いる）。
    pub main: Option<f64>,
    /// 主筋の σy（RC・SRC 断面）。
    pub rebar: Option<f64>,
    /// 鋼材領域の fy（SRC は内蔵鉄骨、それ以外は主材料）。
    pub steel: Option<f64>,
}

pub(crate) fn resolve_fiber_yield(
    model: &sepika_core::model::Model,
    data: &sepika_core::model::ElementData,
) -> FiberYield {
    let main = model.element_material(data).and_then(|m| m.fy);
    let rebar =
        sepika_core::material_grade::rebar_yield_strength(model.element_rebar_material(data));
    let steel = match model.element_section(data).and_then(|s| s.shape.as_ref()) {
        Some(
            SectionShape::SrcBeamRect {
                steel_flange_thick, ..
            }
            | SectionShape::SrcColumnRect {
                steel_flange_thick, ..
            },
        ) => {
            let thick = *steel_flange_thick;
            model.element_steel_material(data).and_then(|m| {
                if sepika_core::material_grade::steel_f_value_issue(&m.name, thick).is_some() {
                    return None;
                }
                sepika_core::material_grade::steel_f_value_prefix(&m.name, thick).or(m.fy)
            })
        }
        Some(
            box_shape @ SectionShape::CftBox { thick, .. }
            | box_shape @ SectionShape::CftPipe { thick, .. },
        ) => {
            let thick = box_shape
                .box_thicknesses()
                .map_or(*thick, |(t1, t2)| t1.max(t2));
            model.element_steel_material(data).and_then(|m| {
                if sepika_core::material_grade::steel_f_value_issue(&m.name, thick).is_some() {
                    return None;
                }
                sepika_core::material_grade::steel_f_value_prefix(&m.name, thick).or(m.fy)
            })
        }
        _ => main,
    };
    FiberYield { main, rebar, steel }
}

/// 断面形状が生成するファイバ領域の降伏点が [`FiberYield`] から解決できるか。
///
/// `false` は形状ファイバの材料生成が panic する条件を表す。コンクリート領域の
/// Fc は [`FiberYield`] の対象外であり入力チェックで確認する。
pub(crate) fn fiber_yield_covers_shape(shape: Option<&SectionShape>, yield_: &FiberYield) -> bool {
    let rebar = yield_.rebar.or(yield_.main).is_some_and(|fy| fy > 0.0);
    let steel = yield_.steel.is_some_and(|fy| fy > 0.0);
    match shape {
        Some(SectionShape::RcBeamRect { .. })
        | Some(SectionShape::RcColumnRect { .. })
        | Some(SectionShape::RcColumnCircle { .. }) => rebar,
        Some(SectionShape::SrcBeamRect { .. }) | Some(SectionShape::SrcColumnRect { .. }) => {
            rebar && steel
        }
        Some(SectionShape::CftBox { .. }) | Some(SectionShape::CftPipe { .. }) => steel,
        Some(SectionShape::RcWall { .. }) | None => true,
        Some(SectionShape::RcSlab { .. }) => true,
        Some(_) => steel,
    }
}

/// ファイバ断面の実効材料強度。解析用 [`build_gauss_fibers`] と表示 API が共有する。
pub(crate) fn fiber_strength_params(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    basis: crate::factory::StrengthBasis,
) -> StrengthParams {
    fiber_strength_params_from_yield(data, model, basis, resolve_fiber_yield(model, data))
}

fn fiber_steel_material<'a>(
    data: &sepika_core::model::ElementData,
    model: &'a sepika_core::model::Model,
) -> Option<&'a sepika_core::model::Material> {
    let main = model.element_material(data);
    match model.element_section(data).and_then(|s| s.shape.as_ref()) {
        Some(SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. }) => {
            model.element_steel_material(data).or(main)
        }
        Some(SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }) => {
            model.element_steel_material(data)
        }
        _ => main,
    }
}

pub(crate) fn fiber_young_moduli(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
) -> (f64, f64) {
    let mat_ref = model.element_material(data);
    let shape = model.element_section(data).and_then(|s| s.shape.as_ref());
    let concrete_e = match (shape, mat_ref) {
        (Some(SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }), Some(m)) => {
            m.fc.filter(|fc| *fc > 0.0)
                .map(|fc| {
                    let gamma = sepika_core::units::concrete_unit_weight_kn_m3(
                        fc,
                        m.concrete_class,
                        sepika_core::units::ConcreteComposition::Plain,
                    );
                    sepika_core::section_shape::concrete_young_modulus_gamma(fc, gamma)
                })
                .unwrap_or(0.0)
        }
        (_, Some(m)) => m.young,
        _ => 0.0,
    };
    let steel_e = match shape {
        Some(SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. }) => model
            .element_steel_material(data)
            .map(|m| m.young)
            .unwrap_or(0.0),
        Some(SectionShape::CftBox { .. } | SectionShape::CftPipe { .. }) => model
            .element_steel_material(data)
            .map(|m| m.young)
            .unwrap_or(0.0),
        _ => mat_ref.map(|m| m.young).unwrap_or(0.0),
    };
    (concrete_e, steel_e)
}

/// 解決済みの [`FiberYield`] を渡す内部版。呼び出し元が [`resolve_fiber_yield`] を
/// 再実行せずに [`fiber_strength_params`] と同じ値を得るために用いる。
fn fiber_strength_params_from_yield(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    basis: crate::factory::StrengthBasis,
    yield_: FiberYield,
) -> StrengthParams {
    let mat_ref = model.element_material(data);
    let steel_mat_ref = fiber_steel_material(data, model);
    let (_, steel_e) = fiber_young_moduli(data, model);
    let fc = mat_ref.and_then(|m| m.fc);
    let rebar_fy = yield_.rebar.or(yield_.main);
    let steel_fy = yield_.steel;
    StrengthParams {
        steel_fy: steel_fy.unwrap_or(235.0) * basis.steel_factor(steel_mat_ref),
        rebar_fy: rebar_fy.unwrap_or(345.0) * basis.rebar_factor(mat_ref),
        concrete_fc: fc.unwrap_or(24.0),
        steel_e,
    }
}

pub(crate) fn resolve_steel_fiber_fy(
    shape: Option<&SectionShape>,
    steel_mat: Option<&sepika_core::model::Material>,
    mat_fy: Option<f64>,
) -> Option<f64> {
    let thickness = match shape {
        Some(
            SectionShape::SrcBeamRect {
                steel_flange_thick, ..
            }
            | SectionShape::SrcColumnRect {
                steel_flange_thick, ..
            },
        ) => Some(*steel_flange_thick),
        Some(SectionShape::CftBox {
            thick,
            horizontal_thick,
            ..
        }) => Some(thick.max(*horizontal_thick)),
        Some(SectionShape::CftPipe { thick, .. }) => Some(*thick),
        _ => None,
    };
    if let (Some(m), Some(t)) = (steel_mat, thickness) {
        if sepika_core::material_grade::steel_f_value_issue(&m.name, t).is_some() {
            return None;
        }
    }
    match shape {
        Some(
            SectionShape::SrcBeamRect {
                steel_flange_thick, ..
            }
            | SectionShape::SrcColumnRect {
                steel_flange_thick, ..
            },
        ) => steel_mat
            .and_then(|m| {
                sepika_core::material_grade::steel_f_value_prefix(&m.name, *steel_flange_thick)
                    .or(m.fy)
            })
            .or(mat_fy),
        Some(
            box_shape @ SectionShape::CftBox { thick, .. }
            | box_shape @ SectionShape::CftPipe { thick, .. },
        ) => steel_mat
            .and_then(|m| {
                let thick = box_shape
                    .box_thicknesses()
                    .map_or(*thick, |(t1, t2)| t1.max(t2));
                if sepika_core::material_grade::steel_f_value_issue(&m.name, thick).is_some() {
                    return None;
                }
                sepika_core::material_grade::steel_f_value_prefix(&m.name, thick).or(m.fy)
            })
            .or(mat_fy),
        _ => mat_fy,
    }
}

/// 同じ諸元のファイバー断面をガウス点 2 点分つくる。
/// 各ガウス点は独立した断面と材料インスタンスを持つ。
/// 断面・材料が未割当のときはゼロ剛性。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn build_gauss_fiber_pair(
    data: &sepika_core::model::ElementData,
    model: &sepika_core::model::Model,
    basis: crate::factory::StrengthBasis,
    kind: AnalysisKind,
    width: f64,
    depth: f64,
    nw: usize,
    nd: usize,
) -> Result<[(FiberSection, Vec<Box<dyn UniaxialMaterial>>); 2], RebarGeometryError> {
    let mat_ref = model.element_material(data);
    let shape = model.element_section(data).and_then(|s| s.shape.as_ref());
    let fc = mat_ref.and_then(|m| m.fc);
    let (concrete_e, steel_e) = fiber_young_moduli(data, model);
    let yield_ = resolve_fiber_yield(model, data);
    let strength = fiber_strength_params_from_yield(data, model, basis, yield_);
    let steel_factor = basis.steel_factor(fiber_steel_material(data, model));
    let rebar_factor = basis.rebar_factor(mat_ref);
    let concrete_rule = crate::factory::resolve_fiber_concrete_hysteresis(data, model, kind);
    let build = || {
        build_gauss_fibers(
            width,
            depth,
            nw,
            nd,
            shape,
            fc,
            concrete_e,
            steel_e,
            yield_,
            steel_factor,
            rebar_factor,
            strength,
            concrete_rule,
        )
    };
    Ok([build()?, build()?])
}

/// ガウス点のファイバー断面と材料を構築する。
/// 形状がない場合は width×depth の中実格子とする。
///
/// ファイバの材料区分タグ（`Fiber::material`）:
/// 0=コンクリート、1=主筋、2=鋼材。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn build_gauss_fibers(
    width: f64,
    depth: f64,
    nw: usize,
    nd: usize,
    shape: Option<&SectionShape>,
    fc: Option<f64>,
    concrete_e: f64,
    steel_e: f64,
    yield_: FiberYield,
    steel_factor: f64,
    rebar_factor: f64,
    strength: StrengthParams,
    concrete_rule: HysteresisModel,
) -> Result<(FiberSection, Vec<Box<dyn UniaxialMaterial>>), RebarGeometryError> {
    let result = shape
        .filter(|s| !matches!(s, SectionShape::RcWall { .. }))
        .map(|s| {
            build_shape_fibers(
                s,
                fc,
                concrete_e,
                steel_e,
                yield_,
                steel_factor,
                rebar_factor,
                strength,
                concrete_rule,
            )
        })
        .transpose()?;

    let (mut fibers, mats) = match result {
        Some(r) => r,
        None => {
            let base: Box<dyn UniaxialMaterial> = if fc.is_some() {
                concrete_fiber_material(fc, concrete_e, concrete_rule)
            } else {
                steel_fiber_material(steel_e, yield_.main.map(|fy| fy * steel_factor))
            };
            let tag = if fc.is_some() { 0 } else { 2 };
            let grid = sepika_section::fiber::rect_fiber_section(width, depth, nw, nd, tag);
            let fibers = grid.fibers;
            let mats: Vec<Box<dyn UniaxialMaterial>> =
                (0..fibers.len()).map(|_| base.clone_box()).collect();
            (fibers, mats)
        }
    };

    for f in &mut fibers {
        let (y, z) = (f.y, f.z);
        f.y = z;
        f.z = -y;
    }
    Ok((FiberSection { fibers }, mats))
}

/// 断面形状に応じたファイバ配置と材料の構築。
/// 主筋の降伏点は断面の主筋材質から解決し、なければ部材材料の fy を用いる。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn build_shape_fibers(
    shape: &SectionShape,
    fc: Option<f64>,
    concrete_e: f64,
    steel_e: f64,
    yield_: FiberYield,
    steel_factor: f64,
    rebar_factor: f64,
    strength: StrengthParams,
    concrete_rule: HysteresisModel,
) -> Result<(Vec<Fiber>, Vec<Box<dyn UniaxialMaterial>>), RebarGeometryError> {
    use sepika_section::mn_surface::{max_dimension, plastic_fibers_at, AnnulusRes, FiberRegion};

    let rebar_fy = yield_.rebar.or(yield_.main);
    let steel_fy = yield_.steel;
    let target = max_dimension(shape) / 16.0;
    let ring = AnnulusRes {
        n_theta: 24,
        n_r_thin: 2,
        n_r_solid: 8,
    };
    let placed = plastic_fibers_at(shape, &strength, target, ring)?;

    let mut concrete: Option<Box<dyn UniaxialMaterial>> = None;
    let mut steel: Option<Box<dyn UniaxialMaterial>> = None;
    let mut rebar: Option<Box<dyn UniaxialMaterial>> = None;

    let mut fibers = Vec::with_capacity(placed.len());
    let mut mats: Vec<Box<dyn UniaxialMaterial>> = Vec::with_capacity(placed.len());
    for f in &placed {
        let (tag, mat) = match f.region {
            FiberRegion::Concrete => {
                let template = concrete
                    .get_or_insert_with(|| concrete_fiber_material(fc, concrete_e, concrete_rule));
                (0usize, template.clone_box())
            }
            FiberRegion::Rebar => {
                let template = rebar.get_or_insert_with(|| {
                    steel_fiber_material(205000.0, rebar_fy.map(|fy| fy * rebar_factor))
                });
                (1usize, template.clone_box())
            }
            FiberRegion::Steel => {
                let template = steel.get_or_insert_with(|| {
                    steel_fiber_material(steel_e, steel_fy.map(|fy| fy * steel_factor))
                });
                (2usize, template.clone_box())
            }
        };
        fibers.push(Fiber {
            y: f.y,
            z: f.z,
            area: f.area,
            material: tag,
        });
        mats.push(mat);
    }
    Ok((fibers, mats))
}

/// 材端解放で内部自由度へ分離した要素端回転。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EndRelease {
    /// 可撓端系ローカル自由度 index（3,4,5 = i 端、9,10,11 = j 端）。
    pub dof: usize,
    /// 回転ばね剛性 [N·mm/rad]（ピンは 0）。
    pub spring: f64,
}

/// 端条件とねじれ解放から、解放する回転自由度を決める。
/// ねじり剛性がない部材（J≤0）の rx は解放しない。
fn resolve_end_releases(
    end_cond: &[sepika_core::model::EndCondition; 2],
    torsion_release: [bool; 2],
    has_torsion: bool,
) -> SmallVec<[EndRelease; 6]> {
    use sepika_core::model::EndCondition;
    const ROT_DOFS: [(usize, usize); 6] = [(3, 0), (4, 0), (5, 0), (9, 1), (10, 1), (11, 1)];
    let mut out = SmallVec::new();
    for &(dof, end) in ROT_DOFS.iter() {
        let is_torsion = dof == 3 || dof == 9;
        let spring = match end_cond[end] {
            EndCondition::Fixed if is_torsion && torsion_release[end] => 0.0,
            EndCondition::Fixed => continue,
            EndCondition::Pinned => 0.0,
            EndCondition::SemiRigid { k_theta } => k_theta,
        };
        if is_torsion && !has_torsion {
            continue;
        }
        out.push(EndRelease { dof, spring });
    }
    out
}

/// スナップショットに保存する trial / committed 部材荷重。
#[derive(Clone, Default)]
pub struct FiberBeamLoadState {
    trial_member_loads: Vec<MemberLoad>,
    committed_member_loads: Vec<MemberLoad>,
    trial_load_map: Option<FrameDeadLoadMap>,
    committed_load_map: Option<FrameDeadLoadMap>,
}

/// スナップショットの型。ヒンジ無しモデルはヒンジ状態が空。
pub type FiberBeamSnapshot = (
    [f64; 12],
    [f64; 12],
    Vec<Vec<Box<dyn UniaxialMaterial>>>,
    Vec<f64>,
    Vec<f64>,
    Vec<f64>,
    Vec<f64>,
    Vec<(GaussResponse, GaussResponse)>,
    Option<crate::wall::side_column::release_connection::ReleaseConnection>,
    bool,
    Vec<EndRelease>,
    FiberBeamLoadState,
);

#[derive(serde::Serialize, serde::Deserialize)]
struct FiberBeamCheckpoint {
    trial_member_loads: Vec<MemberLoad>,
    committed_member_loads: Vec<MemberLoad>,
    releases: Vec<EndRelease>,
    wall_connection: Option<crate::wall::side_column::release_connection::ReleaseConnection>,
    trial_disp: [f64; 12],
    committed_disp: [f64; 12],
    gauss_points: Vec<Vec<Vec<u8>>>,
    responses: Vec<(GaussResponse, GaussResponse)>,
    /// 材端解放の内部自由度（要素端回転）。
    trial_int: Vec<f64>,
    committed_int: Vec<f64>,
    /// 塑性増分ヒンジ状態（η6 + 塑性増分6。ヒンジ無しモデルは空）。
    trial_hinge: Vec<f64>,
    committed_hinge: Vec<f64>,
}

/// 塑性増分ヒンジの定義と状態。
#[derive(Clone)]
pub struct HingeState {
    /// 塑性化域長 Lp `[mm]`。
    pub lp: f64,
    pub k_el: LocalMat,
    /// 初期断面剛性 [端][行][列]。成分順は ε, κy, κz。
    d0: [[[f64; 3]; 3]; 2],
    /// 端部断面のひずみ・曲率 [i_ε, i_κy, i_κz, j_ε, j_κy, j_κz]。
    pub trial_eta: [f64; 6],
    pub committed_eta: [f64; 6],
    /// 塑性増分変形 [mm, rad, rad]（i 端、j 端の順）。
    trial_plastic: [f64; 6],
    committed_plastic: [f64; 6],
}

mod constitutive;

const HINGE_SLOTS: [usize; 6] = [0, 4, 5, 6, 10, 11];
const HINGE_SIGN: [f64; 6] = [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0];

/// 積分点の保存応答。材料の再trialを行わず復元する。
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct GaussResponse {
    stress: Vec<f64>,
    tangent: Vec<f64>,
    force: [f64; 3],
    stiffness: [[f64; 3]; 3],
}

pub struct GaussPoint {
    pub xi: f64,
    pub weight: f64,
    pub section: FiberSection,
    pub mats: Vec<Box<dyn UniaxialMaterial>>,
    pub trial_stress: Vec<f64>,
    pub trial_et: Vec<f64>,
    b: [[f64; 12]; 3],
    /// 断面応答のキャッシュ。`trial_stress`/`trial_et` を書き換えた直後に更新すること。
    cached_force: [f64; 3],
    cached_stiff: [[f64; 3]; 3],
    committed_response: GaussResponse,
}

impl GaussPoint {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        xi: f64,
        weight: f64,
        section: FiberSection,
        mut mats: Vec<Box<dyn UniaxialMaterial>>,
        l: f64,
        phi_y: f64,
        phi_z: f64,
    ) -> Self {
        let n = section.fibers.len();
        let trial_et: Vec<f64> = mats.iter_mut().map(|m| m.trial(0.0).1).collect();
        let b = FiberBeam::compute_b_matrix(xi, l, phi_y, phi_z);
        let mut gp = GaussPoint {
            xi,
            weight,
            section,
            mats,
            trial_stress: vec![0.0; n],
            trial_et,
            b,
            cached_force: [0.0; 3],
            cached_stiff: [[0.0; 3]; 3],
            committed_response: GaussResponse {
                stress: Vec::new(),
                tangent: Vec::new(),
                force: [0.0; 3],
                stiffness: [[0.0; 3]; 3],
            },
        };
        gp.refresh_response();
        gp.committed_response = gp.response();
        gp
    }

    fn response(&self) -> GaussResponse {
        GaussResponse {
            stress: self.trial_stress.clone(),
            tangent: self.trial_et.clone(),
            force: self.cached_force,
            stiffness: self.cached_stiff,
        }
    }

    fn restore_response(&mut self, response: &GaussResponse) {
        self.trial_stress.clone_from(&response.stress);
        self.trial_et.clone_from(&response.tangent);
        self.cached_force = response.force;
        self.cached_stiff = response.stiffness;
    }

    /// `trial_stress`/`trial_et` を書き換えた直後に呼ぶこと。
    fn refresh_response(&mut self) {
        let (stress, et) = (&self.trial_stress, &self.trial_et);
        let (f, s) =
            sepika_section::fiber::integrate_fibers(&self.section, |i, _| (stress[i], et[i]));
        self.cached_force = [f.n, f.my, f.mz];
        self.cached_stiff = s.d;
    }
}

/// ファイバー梁要素。
pub struct FiberBeam {
    element_id: Option<sepika_core::ids::ElemId>,
    /// 節点間長 L `[mm]`。
    pub length: f64,
    /// 材端の剛域長 λi, λj `[mm]`。
    pub rigid_i: f64,
    pub rigid_j: f64,
    /// 可撓長 L' = `length` − λi − λj `[mm]`。
    pub flex_length: f64,
    pub nodes: [NodeId; 2],
    pub gauss_points: Vec<GaussPoint>,
    pub mass_properties: SectionMassProperties,
    pub mass_properties_error: Option<String>,
    /// ねじり定数 J `[mm⁴]`。
    pub torsion_j: f64,
    /// せん断弾性係数 G [N/mm²]。
    pub g: f64,
    /// せん断変形係数 φy。GAs ≤ 0 なら 0。
    pub phi_y: f64,
    /// せん断変形係数 φz。
    pub phi_z: f64,
    pub k_shear: LocalMat,
    /// 要素ローカル系→グローバル系の回転。内部状態はローカル系で保持する。
    pub axis: crate::transform::LocalFrame,
    /// 塑性増分ヒンジ。None = 全長ファイバー積分モデル。
    pub hinge: Option<HingeState>,
    /// 材端解放で分離した要素端回転。空なら全端剛接。
    pub releases: SmallVec<[EndRelease; 6]>,
    connection_started: bool,
    member_load_state: FiberBeamLoadState,
    wall_connection: Option<crate::wall::side_column::release_connection::ReleaseConnection>,
    initial_elastic_stiffness: LocalMat,
    /// 内部回転 `[rad]`。壁接続設定時はそのB列、それ以外は `releases` と同順。
    pub trial_int: SmallVec<[f64; 6]>,
    pub committed_int: SmallVec<[f64; 6]>,
    /// 内力を評価する危険断面位置（正規化座標 \[0,1\]）。
    pub eval_sections: Vec<f64>,
    pub committed_disp: [f64; 12],
    pub trial_disp: [f64; 12],
}

impl FiberBeam {
    pub fn new(
        data: &sepika_core::model::ElementData,
        model: &sepika_core::model::Model,
        basis: crate::factory::StrengthBasis,
        kind: AnalysisKind,
    ) -> Self {
        Self::try_new(data, model, basis, kind)
            .unwrap_or_else(|error| panic!("ファイバー梁要素を生成できません: {error}"))
    }

    pub fn try_new(
        data: &sepika_core::model::ElementData,
        model: &sepika_core::model::Model,
        basis: crate::factory::StrengthBasis,
        kind: AnalysisKind,
    ) -> Result<Self, String> {
        let geometry = crate::transform::EndGeometry::try_of_element(data, model)?;
        let length = geometry.length;
        let (rigid_i, rigid_j) = crate::frame::rigid_arm::resolve_lengths(
            model.analysis_rigid_zone(data).rigid_length_i(),
            model.analysis_rigid_zone(data).rigid_length_j(),
            length,
        );
        let flex_length = length - rigid_i - rigid_j;

        let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
        let width = sec.map(|s| s.width).unwrap_or(0.0);
        let depth = sec.map(|s| s.depth).unwrap_or(0.0);
        let beam_props = crate::frame::beam::BeamElement::try_new(data, model)?;
        let composite_kind = sec
            .and_then(|s| s.shape.as_ref())
            .and_then(sepika_core::structure_kind::shape_composite_kind);
        let torsion_j = if matches!(
            composite_kind,
            Some(
                sepika_core::structure_kind::StructureKind::Src
                    | sepika_core::structure_kind::StructureKind::Cft
            )
        ) {
            beam_props.j
        } else {
            sec.map(|s| s.resolved_j()).transpose()?.unwrap_or(0.0)
        };
        let e = beam_props.e;
        let g = beam_props.g;
        let sec_iy = beam_props.iz;
        let sec_iz = beam_props.iy;
        let sec_as_y = beam_props.as_z;
        let sec_as_z = beam_props.as_y;
        let phi_of = |ei: f64, gas: f64| {
            if gas > 0.0 && ei > 0.0 && flex_length > 0.0 {
                12.0 * ei / (gas * flex_length * flex_length)
            } else {
                0.0
            }
        };
        let phi_y = phi_of(e * sec_iy, g * sec_as_z);
        let phi_z = phi_of(e * sec_iz, g * sec_as_y);
        let k_shear =
            Self::compute_shear_stiffness(flex_length, phi_y, phi_z, g * sec_as_z, g * sec_as_y);

        let nw = FIBER_NW;
        let nd = FIBER_ND;
        let [(sec_a, mats_a), (sec_b, mats_b)] =
            build_gauss_fiber_pair(data, model, basis, kind, width, depth, nw, nd)
                .map_err(|error| format!("ファイバー断面を生成できません: {error}"))?;
        let gauss_points = vec![
            GaussPoint::new(
                -0.5773502691896257,
                1.0,
                sec_a,
                mats_a,
                flex_length,
                phi_y,
                phi_z,
            ),
            GaussPoint::new(
                0.5773502691896257,
                1.0,
                sec_b,
                mats_b,
                flex_length,
                phi_y,
                phi_z,
            ),
        ];
        let initial_elastic_stiffness = beam_props.local_stiffness_flex_raw();

        let axis = crate::transform::LocalFrame::from_nodes(
            geometry.coords[0],
            geometry.coords[1],
            data.local_axis.ref_vector,
        )
        .map_err(|reason| format!("部材 ID {} の局所座標: {reason}", data.id.0))?;

        let releases = resolve_end_releases(
            &data.end_cond,
            [
                crate::frame::beam::i_end_torsion_release(data, model),
                false,
            ],
            torsion_j > 0.0 && g > 0.0,
        );
        let trial_int = SmallVec::from_elem(0.0, releases.len());

        Ok(FiberBeam {
            element_id: Some(data.id),
            length,
            rigid_i,
            rigid_j,
            flex_length,
            releases,
            connection_started: false,
            member_load_state: FiberBeamLoadState::default(),
            wall_connection: None,
            initial_elastic_stiffness,
            committed_int: trial_int.clone(),
            trial_int,
            nodes: [data.nodes[0], data.nodes[1]],
            gauss_points,
            mass_properties: beam_props.mass_properties,
            mass_properties_error: beam_props.mass_properties_error,
            torsion_j,
            g,
            phi_y,
            phi_z,
            k_shear,
            axis,
            hinge: None,
            eval_sections: crate::frame::beam::eval_sections_of(data, model, length),
            committed_disp: [0.0; 12],
            trial_disp: [0.0; 12],
        })
    }

    fn compute_shear_stiffness(l: f64, phi_y: f64, phi_z: f64, gas_y: f64, gas_z: f64) -> LocalMat {
        let mut k = LocalMat::zeros(12);
        if l <= 0.0 {
            return k;
        }
        let planes: [([(usize, f64); 4], f64); 2] = [
            (
                [
                    (1, 2.0 * phi_y / (2.0 * (1.0 + phi_y) * l)),
                    (7, -2.0 * phi_y / (2.0 * (1.0 + phi_y) * l)),
                    (5, phi_y / (2.0 * (1.0 + phi_y))),
                    (11, phi_y / (2.0 * (1.0 + phi_y))),
                ],
                gas_y,
            ),
            (
                [
                    (2, -2.0 * phi_z / (2.0 * (1.0 + phi_z) * l)),
                    (8, 2.0 * phi_z / (2.0 * (1.0 + phi_z) * l)),
                    (4, phi_z / (2.0 * (1.0 + phi_z))),
                    (10, phi_z / (2.0 * (1.0 + phi_z))),
                ],
                gas_z,
            ),
        ];
        for (bg, gas) in planes {
            if gas <= 0.0 {
                continue;
            }
            for &(i, bi) in &bg {
                for &(j, bj) in &bg {
                    let v = gas * l * bi * bj;
                    if v != 0.0 {
                        k.set(i, j, k.get(i, j) + v);
                    }
                }
            }
        }
        k
    }

    /// 諸元は要素座標系で与える。両端剛接としてヒンジを両端有効にする。剛域なし。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_raw_parts(
        nodes: [NodeId; 2],
        length: f64,
        axis: crate::transform::LocalFrame,
        mass_properties: SectionMassProperties,
        e: f64,
        g: f64,
        area: f64,
        iy_elem: f64,
        iz_elem: f64,
        as_y_elem: f64,
        as_z_elem: f64,
        torsion_j: f64,
        lp: f64,
        sections: [(FiberSection, Vec<Box<dyn UniaxialMaterial>>); 2],
    ) -> Self {
        let flex_length = length;
        let phi_of = |ei: f64, gas: f64| {
            if gas > 0.0 && ei > 0.0 && flex_length > 0.0 {
                12.0 * ei / (gas * flex_length * flex_length)
            } else {
                0.0
            }
        };
        let phi_y = phi_of(e * iz_elem, g * as_y_elem);
        let phi_z = phi_of(e * iy_elem, g * as_z_elem);
        let k_shear =
            Self::compute_shear_stiffness(flex_length, phi_y, phi_z, g * as_y_elem, g * as_z_elem);
        let lp = clamp_plastic_zone(lp, flex_length);
        let [(sec_a, mats_a), (sec_b, mats_b)] = sections;
        let w_end = 2.0 * lp / flex_length;
        let gauss_points = vec![
            GaussPoint::new(-1.0, w_end, sec_a, mats_a, flex_length, phi_y, phi_z),
            GaussPoint::new(1.0, w_end, sec_b, mats_b, flex_length, phi_y, phi_z),
        ];
        let mut fb = FiberBeam {
            element_id: None,
            length,
            rigid_i: 0.0,
            rigid_j: 0.0,
            flex_length,
            releases: SmallVec::new(),
            connection_started: false,
            member_load_state: FiberBeamLoadState::default(),
            wall_connection: None,
            initial_elastic_stiffness: LocalMat::zeros(12),
            committed_int: SmallVec::new(),
            trial_int: SmallVec::new(),
            nodes,
            gauss_points,
            mass_properties,
            mass_properties_error: None,
            torsion_j,
            g,
            phi_y,
            phi_z,
            k_shear,
            axis,
            hinge: None,
            eval_sections: vec![0.0, 0.5, 1.0],
            committed_disp: [0.0; 12],
            trial_disp: [0.0; 12],
        };
        let d_nom = [e * area, e * iy_elem, e * iz_elem];
        let mut k_el = LocalMat::zeros(12);
        for sgn in [-1.0_f64, 1.0] {
            let xi = sgn / 3.0_f64.sqrt();
            let w_phys = flex_length / 2.0;
            let b = Self::compute_b_matrix(xi, flex_length, fb.phi_y, fb.phi_z);
            for i in 0..12 {
                for j in 0..12 {
                    let mut val = 0.0;
                    for (p, dp) in d_nom.iter().enumerate() {
                        val += b[p][i] * dp * b[p][j];
                    }
                    if val != 0.0 {
                        k_el.set(i, j, k_el.get(i, j) + val * w_phys);
                    }
                }
            }
        }
        for i in 0..12 {
            for j in 0..12 {
                let v = fb.k_shear.get(i, j);
                if v != 0.0 {
                    k_el.set(i, j, k_el.get(i, j) + v);
                }
            }
        }
        if let Some(kt) = fb.torsion_stiffness() {
            k_el.set(3, 3, k_el.get(3, 3) + kt);
            k_el.set(9, 9, k_el.get(9, 9) + kt);
            k_el.set(3, 9, k_el.get(3, 9) - kt);
            k_el.set(9, 3, k_el.get(9, 3) - kt);
        }
        fb.initial_elastic_stiffness = k_el.clone();
        fb.hinge = Some(HingeState::new(lp, k_el, &fb.gauss_points));
        fb
    }

    pub fn with_plastic_zone(
        data: &sepika_core::model::ElementData,
        model: &sepika_core::model::Model,
        lp: f64,
        basis: crate::factory::StrengthBasis,
        kind: AnalysisKind,
    ) -> Self {
        Self::build_plastic_zone(data, model, lp, FIBER_NW, FIBER_ND, basis, kind)
    }

    /// `nw × nd` は端部断面のファイバ分割数。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_plastic_zone(
        data: &sepika_core::model::ElementData,
        model: &sepika_core::model::Model,
        lp: f64,
        nw: usize,
        nd: usize,
        basis: crate::factory::StrengthBasis,
        kind: AnalysisKind,
    ) -> Self {
        let mut fb = Self::new(data, model, basis, kind);
        let l = fb.flex_length;
        if l <= 0.0 {
            return fb;
        }
        let lp = clamp_plastic_zone(lp, l);

        let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
        let width = sec.map(|s| s.width).unwrap_or(0.0);
        let depth = sec.map(|s| s.depth).unwrap_or(0.0);
        let beam = crate::frame::beam::BeamElement::try_new(data, model)
            .expect("Self::new で公称梁の断面性能を検証済み");
        fb.torsion_j = beam.j;

        let w_end = 2.0 * lp / l;
        let [(sec_a, mats_a), (sec_b, mats_b)] =
            build_gauss_fiber_pair(data, model, basis, kind, width, depth, nw, nd)
                .expect("Self::new で断面ファイバーの生成を検証済み");
        fb.gauss_points = vec![
            GaussPoint::new(-1.0, w_end, sec_a, mats_a, l, fb.phi_y, fb.phi_z),
            GaussPoint::new(1.0, w_end, sec_b, mats_b, l, fb.phi_y, fb.phi_z),
        ];

        let d_nom = [beam.e * beam.a, beam.e * beam.iy, beam.e * beam.iz];
        let mut k_el = LocalMat::zeros(12);
        for sgn in [-1.0_f64, 1.0] {
            let xi = sgn / 3.0_f64.sqrt();
            let w_phys = l / 2.0;
            let b = Self::compute_b_matrix(xi, l, fb.phi_y, fb.phi_z);
            for i in 0..12 {
                for j in 0..12 {
                    let mut val = 0.0;
                    for (p, dp) in d_nom.iter().enumerate() {
                        val += b[p][i] * dp * b[p][j];
                    }
                    if val != 0.0 {
                        k_el.set(i, j, k_el.get(i, j) + val * w_phys);
                    }
                }
            }
        }
        for i in 0..12 {
            for j in 0..12 {
                let v = fb.k_shear.get(i, j);
                if v != 0.0 {
                    k_el.set(i, j, k_el.get(i, j) + v);
                }
            }
        }
        if let Some(kt) = fb.torsion_stiffness() {
            k_el.set(3, 3, k_el.get(3, 3) + kt);
            k_el.set(9, 9, k_el.get(9, 9) + kt);
            k_el.set(3, 9, k_el.get(3, 9) - kt);
            k_el.set(9, 3, k_el.get(9, 3) - kt);
        }
        let axial_stiffness = d_nom[0] / fb.length;
        for (i, j, sign) in [(0, 0, 1.0), (0, 6, -1.0), (6, 0, -1.0), (6, 6, 1.0)] {
            k_el.set(i, j, sign * axial_stiffness);
        }
        fb.initial_elastic_stiffness = k_el.clone();

        fb.hinge = Some(HingeState::new(lp, k_el, &fb.gauss_points));
        fb
    }

    pub fn flex_disp(&self) -> [f64; 12] {
        crate::frame::rigid_arm::to_flex_disp(&self.trial_disp, self.rigid_i, self.rigid_j)
    }

    fn elem_disp(&self, u_flex: &[f64; 12]) -> [f64; 12] {
        if let Some(connection) = &self.wall_connection {
            return connection
                .flexible_displacement(u_flex, &self.trial_int)
                .unwrap_or_else(|error| self.constitutive_failure(&error.to_string()));
        }
        let mut u = *u_flex;
        for (k, rel) in self.releases.iter().enumerate() {
            u[rel.dof] = self.trial_int[k];
        }
        u
    }

    fn update_section_trial(&mut self, u_elem: &[f64; 12]) {
        let l = self.flex_length;
        if l <= 0.0 {
            return;
        }
        for gp in &mut self.gauss_points {
            let b = gp.b;
            let eps0 = b[0][0] * u_elem[0] + b[0][6] * u_elem[6];
            let ky = b[1][2] * u_elem[2]
                + b[1][4] * u_elem[4]
                + b[1][8] * u_elem[8]
                + b[1][10] * u_elem[10];
            let kz = b[2][1] * u_elem[1]
                + b[2][5] * u_elem[5]
                + b[2][7] * u_elem[7]
                + b[2][11] * u_elem[11];
            for (i, fiber) in gp.section.fibers.iter().enumerate() {
                let eps = eps0 - kz * fiber.y + ky * fiber.z;
                gp.mats[i].revert();
                let (sigma, et) = gp.mats[i].trial(eps);
                gp.trial_stress[i] = sigma;
                gp.trial_et[i] = et;
            }
            gp.refresh_response();
        }
    }

    fn elem_tangent(&self) -> LocalMat {
        let mut k = LocalMat::zeros(12);
        let l = self.flex_length;
        if l <= 0.0 {
            return k;
        }
        if let Some(h) = &self.hinge {
            return self
                .hinge_tangent(h)
                .unwrap_or_else(|error| self.constitutive_failure(&error));
        }
        let half = l / 2.0;

        for gp in &self.gauss_points {
            let (_, d) = Self::section_response_from_cache(gp);
            let w = gp.weight * half;
            let b = gp.b;

            for i in 0..12 {
                for p in 0..3 {
                    let bpi = b[p][i];
                    if bpi == 0.0 {
                        continue;
                    }
                    for j in 0..12 {
                        let mut val = 0.0;
                        for q in 0..3 {
                            val += d[p][q] * b[q][j];
                        }
                        if val != 0.0 {
                            let old = k.get(i, j);
                            k.set(i, j, old + bpi * val * w);
                        }
                    }
                }
            }
        }

        for i in 0..12 {
            for j in 0..12 {
                let v = self.k_shear.get(i, j);
                if v != 0.0 {
                    k.set(i, j, k.get(i, j) + v);
                }
            }
        }

        if let Some(kt) = self.torsion_stiffness() {
            k.set(3, 3, k.get(3, 3) + kt);
            k.set(9, 9, k.get(9, 9) + kt);
            k.set(3, 9, k.get(3, 9) - kt);
            k.set(9, 3, k.get(9, 3) - kt);
        }
        k
    }

    /// ねじり剛性 GJ/L。J≤0 では None。
    fn torsion_stiffness(&self) -> Option<f64> {
        (self.torsion_j > 0.0 && self.length > 0.0).then(|| self.g * self.torsion_j / self.length)
    }

    /// `u_elem` と整合させるには先に [`Self::update_section_trial`] を呼ぶこと。
    fn elem_internal_force(&self, u_elem: &[f64; 12]) -> [f64; 12] {
        let mut f = [0.0_f64; 12];
        let l = self.flex_length;
        if l <= 0.0 {
            return f;
        }
        if let Some(h) = &self.hinge {
            return Self::hinge_internal_force(h, u_elem);
        }
        let half = l / 2.0;

        for gp in &self.gauss_points {
            let (force, _) = Self::section_response_from_cache(gp);
            let w = gp.weight * half;
            let b = gp.b;
            for (i, fi) in f.iter_mut().enumerate() {
                *fi += (b[0][i] * force[0] + b[1][i] * force[1] + b[2][i] * force[2]) * w;
            }
        }

        for i in 0..12 {
            let mut si = 0.0;
            for j in 0..12 {
                si += self.k_shear.get(i, j) * u_elem[j];
            }
            f[i] += si;
        }

        if let Some(kt) = self.torsion_stiffness() {
            let drx = u_elem[3] - u_elem[9];
            f[3] += kt * drx;
            f[9] -= kt * drx;
        }
        f
    }

    fn solve_distributed_internal_dofs(&mut self) -> Result<(), String> {
        /// 内部釣合いの最大反復数（弾性域は 1 回、降伏を跨いでも数回で収まる）。
        const MAX_ITER: usize = 20;
        let u_flex = self.flex_disp();
        let qf = self.flex_member_load();
        if self.releases.is_empty() {
            let u_elem = self.elem_disp(&u_flex);
            self.update_section_trial(&u_elem);
            return Ok(());
        }
        let indices = self.active_release_indices();
        let n = indices.len();
        for _ in 0..MAX_ITER {
            let u_elem = self.elem_disp(&u_flex);
            self.update_section_trial(&u_elem);
            let f_elem = self.elem_internal_force(&u_elem);

            let mut r = [0.0_f64; 6];
            for (k, &index) in indices.iter().enumerate() {
                let rel = &self.releases[index];
                r[k] = f_elem[rel.dof] + rel.spring * (u_elem[rel.dof] - u_flex[rel.dof])
                    - qf[rel.dof];
            }
            let k_elem = self.elem_tangent();
            let force_scale = [3usize, 4, 5, 9, 10, 11]
                .iter()
                .map(|&i| f_elem[i].abs().max(qf[i].abs()))
                .fold(1.0_f64, f64::max);
            let mut converged = true;
            for (a, &index) in indices.iter().enumerate() {
                let release = &self.releases[index];
                let uncancelled: f64 = (0..12)
                    .map(|j| (k_elem.get(release.dof, j) * u_elem[j]).abs())
                    .sum();
                let scale = force_scale.max(uncancelled);
                if !r[a].is_finite() || !uncancelled.is_finite() {
                    return Err("分布断面の外側端部接続残差または尺度が非有限です".into());
                }
                converged &= r[a].abs() <= 1e-10 * scale;
            }
            let mut kbb = [0.0_f64; 36];
            for (a, &index) in indices.iter().enumerate() {
                let ra = &self.releases[index];
                for (b, &other) in indices.iter().enumerate() {
                    kbb[a * n + b] = k_elem.get(ra.dof, self.releases[other].dof);
                }
                kbb[a * n + a] += ra.spring;
            }
            let lu = sepika_math::dense_lu::CheckedDenseLu::factorize(&kbb[..n * n], n)
                .map_err(|error| format!("分布断面の外側端部接続釣合い: {error}"))?;
            if converged {
                return Ok(());
            }
            let du = lu
                .solve(&r[..n], 1)
                .map_err(|error| format!("分布断面の外側端部接続修正量: {error}"))?;
            for (a, d) in du[..n].iter().enumerate() {
                self.trial_int[indices[a]] -= d;
            }
        }
        Err("分布断面の外側端部接続釣合いが20反復で収束しません".into())
    }

    fn solve_internal_dofs(&mut self) -> Result<(), String> {
        if self.wall_connection.is_some() {
            return self.solve_wall_connection();
        }
        if self.hinge.is_none() {
            return self.solve_distributed_internal_dofs();
        }
        /// 内部釣合いの最大反復数（弾性域は 1 回、降伏を跨いでも数回で収まる）。
        const MAX_ITER: usize = 20;
        let u_flex = self.flex_disp();
        let qf = self.flex_member_load();
        if self.releases.is_empty() {
            let u_elem = self.elem_disp(&u_flex);
            self.update_trial_state(&u_elem)?;
            return Ok(());
        }
        let indices = self.active_release_indices();
        let n = indices.len();
        let initial_u = self.elem_disp(&u_flex);
        let mut initial_kbb = vec![0.0; n * n];
        let mut initial_residual = vec![0.0; n];
        for (a, &index) in indices.iter().enumerate() {
            let release = &self.releases[index];
            initial_residual[a] = (0..12)
                .map(|j| self.initial_elastic_stiffness.get(release.dof, j) * initial_u[j])
                .sum::<f64>()
                + release.spring * (initial_u[release.dof] - u_flex[release.dof])
                - qf[release.dof];
            for (b, &other) in indices.iter().enumerate() {
                initial_kbb[a * n + b] = self
                    .initial_elastic_stiffness
                    .get(release.dof, self.releases[other].dof);
            }
            initial_kbb[a * n + a] += release.spring;
        }
        let initial_lu = sepika_math::dense_lu::CheckedDenseLu::factorize(&initial_kbb, n)
            .map_err(|error| format!("外側端部接続の初期予測: {error}"))?;
        let predictor = initial_lu
            .solve(&initial_residual, 1)
            .map_err(|error| format!("外側端部接続の初期予測: {error}"))?;
        for (a, &index) in indices.iter().enumerate() {
            self.trial_int[index] -= predictor[a];
        }
        for _ in 0..MAX_ITER {
            let u_elem = self.elem_disp(&u_flex);
            self.update_trial_state(&u_elem)?;
            let f_elem = self.elem_internal_force(&u_elem);

            let mut r = [0.0_f64; 6];
            for (k, &index) in indices.iter().enumerate() {
                let rel = &self.releases[index];
                r[k] = f_elem[rel.dof] + rel.spring * (u_elem[rel.dof] - u_flex[rel.dof])
                    - qf[rel.dof];
            }
            let converged =
                indices
                    .iter()
                    .enumerate()
                    .try_fold(true, |converged, (k, &index)| {
                        let rel = &self.releases[index];
                        let h = self.hinge.as_ref().unwrap();
                        let uhat = Self::hinge_uhat(h, &u_elem);
                        let uncancelled: f64 = (0..12)
                            .map(|j| (h.k_el.get(rel.dof, j) * uhat[j]).abs())
                            .sum();
                        let mut scale = 1.0_f64
                            .max(f_elem[rel.dof].abs())
                            .max(uncancelled)
                            .max(qf[rel.dof].abs());
                        if self.hinge.is_some() && rel.dof % 6 != 3 {
                            let gp = &self.gauss_points[rel.dof / 6];
                            let fiber_moment: f64 = gp
                                .section
                                .fibers
                                .iter()
                                .zip(&gp.trial_stress)
                                .map(|(fiber, stress)| {
                                    (stress
                                        * fiber.area
                                        * if rel.dof % 6 == 4 { fiber.z } else { fiber.y })
                                    .abs()
                                })
                                .sum();
                            scale = scale.max(fiber_moment);
                        }
                        if !scale.is_finite() || !r[k].is_finite() {
                            return Err("外側端部接続の残差または尺度が非有限です".to_string());
                        }
                        Ok(converged && r[k].abs() <= 1e-10 * scale)
                    })?;
            if converged {
                return Ok(());
            }

            let k_elem = if let Some(h) = &self.hinge {
                self.hinge_tangent(h)?
            } else {
                self.elem_tangent()
            };
            let mut kbb = [0.0_f64; 36];
            for (a, &index) in indices.iter().enumerate() {
                let ra = &self.releases[index];
                for (b, &other) in indices.iter().enumerate() {
                    let rb = &self.releases[other];
                    kbb[a * n + b] = k_elem.get(ra.dof, rb.dof);
                }
                kbb[a * n + a] += ra.spring;
            }
            let lu = sepika_math::dense_lu::CheckedDenseLu::factorize(&kbb[..n * n], n)
                .map_err(|error| format!("端部接続の外側釣合い: {error}"))?;
            let du = lu
                .solve(&r[..n], 1)
                .map_err(|error| format!("端部接続の外側釣合い: {error}"))?;
            for (k, d) in du[..n].iter().enumerate() {
                self.trial_int[indices[k]] -= d;
            }
        }
        Err("端部接続の外側釣合いが20反復で収束しません".into())
    }

    fn update_trial_state(&mut self, u_elem: &[f64; 12]) -> Result<(), String> {
        if self.hinge.is_some() {
            self.solve_hinges(u_elem)?;
        } else {
            self.update_section_trial(u_elem);
        }
        Ok(())
    }

    fn condense_releases(&self, k_elem: &LocalMat) -> LocalMat {
        self.condensed_release_tangent(k_elem)
            .unwrap_or_else(|error| self.constitutive_failure(&error))
    }

    fn section_response_from_cache(gp: &GaussPoint) -> ([f64; 3], [[f64; 3]; 3]) {
        (gp.cached_force, gp.cached_stiff)
    }

    /// ひずみ－変位行列（行 0: 軸ひずみ、行 1: κy、行 2: κz）。
    fn compute_b_matrix(xi: f64, l: f64, phi_y: f64, phi_z: f64) -> [[f64; 12]; 3] {
        let inv_l = 1.0 / l;
        let inv_l2 = 1.0 / (l * l);
        let mut b = [[0.0; 12]; 3];
        b[0][0] = -inv_l;
        b[0][6] = inv_l;
        let cz = 1.0 / (1.0 + phi_z);
        b[1][2] = -6.0 * xi * inv_l2 * cz;
        b[1][4] = -(1.0 - 3.0 * xi + phi_z) * inv_l * cz;
        b[1][8] = 6.0 * xi * inv_l2 * cz;
        b[1][10] = (1.0 + 3.0 * xi + phi_z) * inv_l * cz;
        let cy = 1.0 / (1.0 + phi_y);
        b[2][1] = 6.0 * xi * inv_l2 * cy;
        b[2][5] = -(1.0 - 3.0 * xi + phi_y) * inv_l * cy;
        b[2][7] = -6.0 * xi * inv_l2 * cy;
        b[2][11] = (1.0 + 3.0 * xi + phi_y) * inv_l * cy;
        b
    }
}

impl FiberBeam {
    fn member_load_map(&self, loads: &[MemberLoad]) -> Result<Option<FrameDeadLoadMap>, String> {
        if loads.is_empty() {
            return Ok(None);
        }
        let id = self
            .element_id
            .ok_or("物理部材IDがない要素は部材荷重を所有できません")?;
        if loads.iter().any(|load| load.elem != id) {
            return Err("荷重の部材IDが要素と一致しません".into());
        }
        FrameDeadLoadMap::new(loads, &self.axis, self.length, self.rigid_i, self.rigid_j).map(Some)
    }

    fn try_update_state_with_member_loads(
        &mut self,
        du: &LocalVec,
        absolute_loads: Option<&[MemberLoad]>,
        commit: bool,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        crate::behavior::validate_trial_increment(du, self.n_dof()).map_err(|error| {
            crate::behavior::ElementUpdateError(format!(
                "Fiber/MS 要素 {:?}（節点 {:?}）: {error}",
                self.element_id, self.nodes
            ))
        })?;
        let replacement = absolute_loads
            .map(|loads| self.member_load_map(loads))
            .transpose()
            .map_err(|error| {
                crate::behavior::ElementUpdateError(format!(
                    "Fiber/MS 要素 {:?}（節点 {:?}）の部材荷重: {error}",
                    self.element_id, self.nodes
                ))
            })?;
        let previous = self.snapshot_state();
        if let Some(map) = replacement {
            self.member_load_state.trial_member_loads = absolute_loads.unwrap().to_vec();
            self.member_load_state.trial_load_map = map;
        }
        let du_global: [f64; 12] = std::array::from_fn(|i| du.data[i]);
        let du_local = self.axis.rotate_to_local(&du_global);
        for i in 0..12 {
            self.trial_disp[i] += du_local[i];
        }
        if self.flex_length <= 0.0 {
            return Ok(());
        }
        if let Err(error) = self
            .solve_internal_dofs()
            .and_then(|()| self.trial_global_force().map(|_| ()))
            .and_then(|()| self.current_loaded_response().map(|_| ()))
            .and_then(|()| {
                let k = match &self.hinge {
                    Some(h) => self.hinge_tangent(h)?,
                    None => self.elem_tangent(),
                };
                let condensed = self.condensed_release_tangent(&k)?;
                if condensed.data.iter().any(|v| !v.is_finite()) {
                    return Err("端部接続接線が非有限です".into());
                }
                Ok(())
            })
        {
            self.restore_state(previous.as_ref());
            return Err(crate::behavior::ElementUpdateError(format!(
                "Fiber/MS 要素 {:?}（節点 {:?}）の構成則を解けません: {error}",
                self.element_id, self.nodes
            )));
        }
        self.connection_started = true;
        if commit {
            self.commit_state();
        }
        Ok(())
    }
}

impl ElementBehavior for FiberBeam {
    fn n_dof(&self) -> usize {
        12
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        crate::behavior::node_global_dofs(&self.nodes, dof)
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        if self.flex_length <= 0.0 {
            return LocalMat::zeros(12);
        }
        let k_elem = self.elem_tangent();
        let k_end = self.condense_releases(&k_elem);
        let k_node =
            crate::frame::rigid_arm::transform_stiffness(&k_end, self.rigid_i, self.rigid_j);
        self.axis.to_global(&k_node)
    }

    fn internal_force(&self, _ctx: &Ctx) -> LocalVec {
        if self.flex_length <= 0.0 {
            return LocalVec {
                data: SmallVec::from_elem(0.0, 12),
            };
        }
        let force = self
            .trial_global_force()
            .unwrap_or_else(|error| self.constitutive_failure(&error));
        LocalVec {
            data: SmallVec::from_slice(&force),
        }
    }

    fn state_member_forces(&self, ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        if let Some(response) = self
            .state_frame_load_response()
            .unwrap_or_else(|error| self.constitutive_failure(&error))
        {
            return Some(
                response
                    .section
                    .recover(&self.eval_sections)
                    .unwrap_or_else(|error| self.constitutive_failure(&error)),
            );
        }
        let f_global = self.internal_force(ctx);
        let arr: [f64; 12] = std::array::from_fn(|i| f_global.data[i]);
        let f_local = self.axis.rotate_to_local(&arr);
        Some(crate::frame::beam::member_forces_from_end_forces(
            &f_local,
            self.length,
            &self.eval_sections,
        ))
    }

    fn update_state(
        &mut self,
        du: &LocalVec,
        commit: bool,
        _ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        self.try_update_state_with_member_loads(du, None, commit)
    }

    fn owns_frame_member_loads(&self) -> bool {
        true
    }

    fn frame_load_response(
        &self,
        _u_elem: &[f64],
        _absolute_loads: &[MemberLoad],
    ) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        if !self.owns_frame_member_loads() {
            return Ok(None);
        }
        Err(format!("Fiber/MS 要素 {:?}: 非線形要素の線形荷重応答は未対応です。状態更新後の試行応答を使用してください", self.element_id))
    }

    fn state_frame_load_response(
        &self,
    ) -> Result<Option<crate::behavior::LoadedFrameResponse>, String> {
        if !self.owns_frame_member_loads() {
            return Ok(None);
        }
        let map = FrameDeadLoadMap::new(
            &self.member_load_state.trial_member_loads,
            &self.axis,
            self.length,
            self.rigid_i,
            self.rigid_j,
        )
        .map_err(|error| format!("Fiber/MS 要素 {:?}の部材荷重応答: {error}", self.element_id))?;
        self.current_response_from_map(&map)
            .map(Some)
            .map_err(|error| format!("Fiber/MS 要素 {:?}の部材荷重応答: {error}", self.element_id))
    }

    fn update_state_with_member_loads(
        &mut self,
        du: &LocalVec,
        absolute_loads: &[MemberLoad],
        commit: bool,
        _ctx: &Ctx,
    ) -> Result<(), crate::behavior::ElementUpdateError> {
        if !self.flex_length.is_finite() || self.flex_length <= 0.0 {
            return Err(crate::behavior::ElementUpdateError(format!(
                "Fiber/MS 要素 {:?}: 可撓長が不正です",
                self.element_id
            )));
        }
        let previous = self.snapshot_state();
        let result = self
            .try_update_state_with_member_loads(du, Some(absolute_loads), false)
            .and_then(|()| {
                self.state_frame_load_response()
                    .map(|_| ())
                    .map_err(crate::behavior::ElementUpdateError)
            });
        if let Err(error) = result {
            self.restore_state(previous.as_ref());
            return Err(error);
        }
        if commit {
            self.commit_state();
        }
        Ok(())
    }

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        let mass_properties = self
            .mass_properties_error
            .as_ref()
            .map_or(self.mass_properties, |error| {
                panic!("質量特性を解決できません: {error}")
            });
        match opt {
            MassOption::Lumped => {
                crate::frame::prismatic::lumped_mass(mass_properties.total_mass(self.length))
            }
            MassOption::Consistent => {
                let flex = crate::frame::prismatic::consistent_mass_timoshenko(
                    mass_properties,
                    self.flex_length,
                    self.phi_y,
                    self.phi_z,
                );
                if let Some(connection) = &self.wall_connection {
                    let mass = connection
                        .initial_consistent_mass(&self.initial_elastic_stiffness, &flex)
                        .unwrap_or_else(|error| {
                            panic!("FiberBeam の壁接続質量を縮約できません: {error}")
                        });
                    let mass = crate::frame::prismatic::mass_without_end_releases(
                        &mass,
                        mass_properties,
                        self.rigid_i,
                        self.rigid_j,
                    );
                    return self.axis.to_global(&mass);
                }
                let releases: SmallVec<[(usize, f64); 6]> =
                    self.releases.iter().map(|r| (r.dof, r.spring)).collect();
                let mm = crate::frame::prismatic::condense_end_releases_with_mass(
                    &self.initial_elastic_stiffness,
                    &flex,
                    mass_properties,
                    self.rigid_i,
                    self.rigid_j,
                    &releases,
                )
                .unwrap_or_else(|| {
                    panic!(
                        "FiberBeam の端部解放質量を縮約できません: Kbb が特異です（解放条件を確認してください）"
                    )
                });
                self.axis.to_global(&mm)
            }
        }
    }

    fn geometric_stiffness(&self, n: f64) -> LocalMat {
        let kg_node = crate::frame::prismatic::geometric_stiffness(
            n,
            self.flex_length,
            self.rigid_i,
            self.rigid_j,
        );
        self.axis.to_global(&kg_node)
    }

    fn snapshot_state(&self) -> Box<dyn Any> {
        let gauss_data: Vec<Vec<Box<dyn UniaxialMaterial>>> = self
            .gauss_points
            .iter()
            .map(|gp| gp.mats.iter().map(|m| m.clone_box()).collect())
            .collect();
        let (trial_hinge, committed_hinge) = match &self.hinge {
            Some(h) => (
                h.trial_eta
                    .iter()
                    .chain(h.trial_plastic.iter())
                    .copied()
                    .collect(),
                h.committed_eta
                    .iter()
                    .chain(h.committed_plastic.iter())
                    .copied()
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        Box::new((
            self.trial_disp,
            self.committed_disp,
            gauss_data,
            self.trial_int.to_vec(),
            self.committed_int.to_vec(),
            trial_hinge,
            committed_hinge,
            self.gauss_points
                .iter()
                .map(|gp| (gp.response(), gp.committed_response.clone()))
                .collect::<Vec<_>>(),
            self.wall_connection.clone(),
            self.connection_started,
            self.releases.to_vec(),
            self.member_load_state.clone(),
        ))
    }

    fn restore_state(&mut self, state: &dyn Any) {
        let (
            trial,
            committed,
            mats_data,
            trial_int,
            committed_int,
            th,
            ch,
            responses,
            wall_connection,
            connection_started,
            releases,
            member_load_state,
        ) = crate::behavior::downcast_snapshot::<FiberBeamSnapshot>("FiberBeam", state);
        self.member_load_state = member_load_state.clone();
        self.wall_connection = wall_connection.clone();
        self.connection_started = *connection_started;
        self.releases = SmallVec::from_slice(releases);
        self.trial_disp = *trial;
        self.committed_disp = *committed;
        for (gp, gp_mats) in self.gauss_points.iter_mut().zip(mats_data) {
            for (mat, new_mat) in gp.mats.iter_mut().zip(gp_mats) {
                *mat = new_mat.clone_box();
            }
        }
        self.trial_int = SmallVec::from_slice(trial_int);
        self.committed_int = SmallVec::from_slice(committed_int);
        if let Some(h) = &mut self.hinge {
            if th.len() == 12 && ch.len() == 12 {
                h.trial_eta.copy_from_slice(&th[..6]);
                h.trial_plastic.copy_from_slice(&th[6..]);
                h.committed_eta.copy_from_slice(&ch[..6]);
                h.committed_plastic.copy_from_slice(&ch[6..]);
            }
        }
        for (gp, (trial, committed)) in self.gauss_points.iter_mut().zip(responses) {
            gp.restore_response(trial);
            gp.committed_response = committed.clone();
        }
    }

    fn commit_state(&mut self) {
        for gp in &mut self.gauss_points {
            for mat in &mut gp.mats {
                mat.commit();
            }
            gp.committed_response = gp.response();
        }
        self.member_load_state.committed_member_loads =
            self.member_load_state.trial_member_loads.clone();
        self.member_load_state.committed_load_map = self.member_load_state.trial_load_map.clone();
        self.committed_disp = self.trial_disp;
        self.committed_int = self.trial_int.clone();
        if let Some(h) = &mut self.hinge {
            h.committed_eta = h.trial_eta;
            h.committed_plastic = h.trial_plastic;
        }
    }

    fn revert_state(&mut self) {
        for gp in &mut self.gauss_points {
            for mat in &mut gp.mats {
                mat.revert();
            }
            let response = gp.committed_response.clone();
            gp.restore_response(&response);
        }
        self.member_load_state.trial_member_loads =
            self.member_load_state.committed_member_loads.clone();
        self.member_load_state.trial_load_map = self.member_load_state.committed_load_map.clone();
        self.trial_disp = self.committed_disp;
        self.trial_int = self.committed_int.clone();
        if let Some(h) = &mut self.hinge {
            h.trial_eta = h.committed_eta;
            h.trial_plastic = h.committed_plastic;
        }
    }

    fn serialize_checkpoint(&self) -> Vec<u8> {
        let gauss_points: Vec<Vec<Vec<u8>>> = self
            .gauss_points
            .iter()
            .map(|gp| {
                gp.mats
                    .iter()
                    .map(|m| m.serialize_state())
                    .collect::<Vec<_>>()
            })
            .collect();
        let (trial_hinge, committed_hinge) = match &self.hinge {
            Some(h) => (
                h.trial_eta
                    .iter()
                    .chain(h.trial_plastic.iter())
                    .copied()
                    .collect(),
                h.committed_eta
                    .iter()
                    .chain(h.committed_plastic.iter())
                    .copied()
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let cp = FiberBeamCheckpoint {
            trial_member_loads: self.member_load_state.trial_member_loads.clone(),
            committed_member_loads: self.member_load_state.committed_member_loads.clone(),
            releases: self.releases.to_vec(),
            wall_connection: self.wall_connection.clone(),
            trial_disp: self.trial_disp,
            committed_disp: self.committed_disp,
            gauss_points,
            responses: self
                .gauss_points
                .iter()
                .map(|gp| (gp.response(), gp.committed_response.clone()))
                .collect(),
            trial_int: self.trial_int.to_vec(),
            committed_int: self.committed_int.to_vec(),
            trial_hinge,
            committed_hinge,
        };
        bincode::serialize(&cp).expect("serialize checkpoint")
    }

    fn deserialize_checkpoint(
        &mut self,
        data: &[u8],
    ) -> Result<(), crate::behavior::CheckpointError> {
        let previous = self.snapshot_state();
        let result =
            (|| {
                let cp: FiberBeamCheckpoint = bincode::deserialize(data)
                    .map_err(|error| crate::behavior::CheckpointError::Decode(error.to_string()))?;
                if cp.releases.as_slice() != self.releases.as_slice()
                    || cp.wall_connection != self.wall_connection
                    || cp.trial_int.len() != self.internal_rotation_count()
                    || cp.committed_int.len() != self.internal_rotation_count()
                    || cp.gauss_points.len() != self.gauss_points.len()
                    || cp.responses.len() != self.gauss_points.len()
                    || cp.responses.iter().zip(&self.gauss_points).any(
                        |((trial, committed), gp)| {
                            [trial, committed].into_iter().any(|response| {
                                response.stress.len() != gp.mats.len()
                                    || response.tangent.len() != gp.mats.len()
                                    || response
                                        .stress
                                        .iter()
                                        .chain(&response.tangent)
                                        .chain(&response.force)
                                        .chain(response.stiffness.iter().flatten())
                                        .any(|value| !value.is_finite())
                            })
                        },
                    )
                    || cp
                        .gauss_points
                        .iter()
                        .zip(&self.gauss_points)
                        .any(|(m, gp)| m.len() != gp.mats.len())
                    || (self.hinge.is_some()
                        && (cp.trial_hinge.len() != 12 || cp.committed_hinge.len() != 12))
                    || (self.hinge.is_none()
                        && (!cp.trial_hinge.is_empty() || !cp.committed_hinge.is_empty()))
                    || cp
                        .trial_disp
                        .iter()
                        .chain(&cp.committed_disp)
                        .chain(&cp.trial_int)
                        .chain(&cp.committed_int)
                        .chain(&cp.trial_hinge)
                        .chain(&cp.committed_hinge)
                        .any(|value| !value.is_finite())
                {
                    return Err(crate::behavior::CheckpointError::Decode(
                        "Fiber/MS状態の寸法が一致しません".into(),
                    ));
                }
                let trial_load_map = self
                    .member_load_map(&cp.trial_member_loads)
                    .map_err(crate::behavior::CheckpointError::Decode)?;
                let committed_load_map = self
                    .member_load_map(&cp.committed_member_loads)
                    .map_err(crate::behavior::CheckpointError::Decode)?;
                self.member_load_state = FiberBeamLoadState {
                    trial_member_loads: cp.trial_member_loads,
                    committed_member_loads: cp.committed_member_loads,
                    trial_load_map,
                    committed_load_map,
                };
                self.trial_disp = cp.trial_disp;
                self.committed_disp = cp.committed_disp;
                for (gp, gp_mats) in self.gauss_points.iter_mut().zip(cp.gauss_points) {
                    for (mat, mat_bytes) in gp.mats.iter_mut().zip(gp_mats) {
                        mat.deserialize_state(&mat_bytes)?;
                    }
                }
                self.trial_int = SmallVec::from_slice(&cp.trial_int);
                self.committed_int = SmallVec::from_slice(&cp.committed_int);
                if let Some(h) = &mut self.hinge {
                    h.trial_eta.copy_from_slice(&cp.trial_hinge[..6]);
                    h.trial_plastic.copy_from_slice(&cp.trial_hinge[6..]);
                    h.committed_eta.copy_from_slice(&cp.committed_hinge[..6]);
                    h.committed_plastic
                        .copy_from_slice(&cp.committed_hinge[6..]);
                }
                for (gp, (trial, committed)) in self.gauss_points.iter_mut().zip(cp.responses) {
                    gp.restore_response(&trial);
                    gp.committed_response = committed;
                }
                Ok(())
            })();
        if result.is_err() {
            self.restore_state(previous.as_ref());
        } else {
            self.connection_started = true;
        }
        result
    }

    fn ductility_probe(&self) -> Option<DuctilityProbe> {
        let l = self.flex_length;
        if l <= 0.0 || self.gauss_points.is_empty() {
            return None;
        }
        let td = self.elem_disp(&self.flex_disp());
        let mut best: Option<(f64, usize, f64, f64, f64)> = None;
        if let Some(h) = &self.hinge {
            for gi in 0..self.gauss_points.len().min(2) {
                let [eps0, ky, kz] = std::array::from_fn(|a| h.trial_eta[gi * 3 + a]);
                let kappa = (ky * ky + kz * kz).sqrt();
                if best.is_none_or(|(bk, ..)| kappa > bk) {
                    best = Some((kappa, gi, eps0, ky, kz));
                }
            }
        } else {
            for (gi, gp) in self.gauss_points.iter().enumerate() {
                let b = gp.b;
                let eps0 = b[0][0] * td[0] + b[0][6] * td[6];
                let ky = b[1][2] * td[2] + b[1][4] * td[4] + b[1][8] * td[8] + b[1][10] * td[10];
                let kz = b[2][1] * td[1] + b[2][5] * td[5] + b[2][7] * td[7] + b[2][11] * td[11];
                let kappa = (ky * ky + kz * kz).sqrt();
                if best.is_none_or(|(bk, ..)| kappa > bk) {
                    best = Some((kappa, gi, eps0, ky, kz));
                }
            }
        }
        let (kappa, gi, eps0, ky, kz) = best?;
        let gp = &self.gauss_points[gi];
        let mut max_t = 0.0_f64;
        let mut max_c = 0.0_f64;
        let mut max_yr = 0.0_f64;
        let mut jm_num = 0.0_f64;
        let mut jm_den = 0.0_f64;
        for (i, fiber) in gp.section.fibers.iter().enumerate() {
            let eps = eps0 - kz * fiber.y + ky * fiber.z;
            max_t = max_t.max(eps);
            max_c = max_c.max(-eps);
            let sref = gp.mats[i].reference_stress();
            let eref = gp.mats[i].reference_strain();
            if sref > 0.0 && eref > 0.0 {
                let mu_i = eps.abs() / eref;
                max_yr = max_yr.max(mu_i);
                let w = sref * fiber.area * eps.abs();
                jm_num += w * mu_i;
                jm_den += w;
            }
        }
        let jm = if jm_den > 0.0 { jm_num / jm_den } else { 0.0 };
        Some(DuctilityProbe {
            curvature: kappa,
            max_tension_strain: max_t,
            max_compression_strain: max_c,
            max_yield_ratio: max_yr,
            jm,
        })
    }

    fn fiber_section_states(&self) -> Option<Vec<crate::behavior::FiberSectionState>> {
        use crate::behavior::{FiberSectionState, FiberStateSample};
        let l = self.flex_length;
        if l <= 0.0 || self.gauss_points.is_empty() {
            return None;
        }
        let td = self.elem_disp(&self.flex_disp());
        let n_sections = if self.hinge.is_some() {
            self.gauss_points.len().min(2)
        } else {
            self.gauss_points.len()
        };
        let mut out = Vec::with_capacity(n_sections);
        for (gi, gp) in self.gauss_points.iter().take(n_sections).enumerate() {
            let (eps0, ky, kz) = if let Some(h) = &self.hinge {
                (
                    h.trial_eta[gi * 3],
                    h.trial_eta[gi * 3 + 1],
                    h.trial_eta[gi * 3 + 2],
                )
            } else {
                let b = gp.b;
                let eps0 = b[0][0] * td[0] + b[0][6] * td[6];
                let ky = b[1][2] * td[2] + b[1][4] * td[4] + b[1][8] * td[8] + b[1][10] * td[10];
                let kz = b[2][1] * td[1] + b[2][5] * td[5] + b[2][7] * td[7] + b[2][11] * td[11];
                (eps0, ky, kz)
            };
            let fibers = gp
                .section
                .fibers
                .iter()
                .enumerate()
                .map(|(i, fiber)| {
                    let eps = eps0 - kz * fiber.y + ky * fiber.z;
                    let eref = gp.mats[i].reference_strain();
                    FiberStateSample {
                        y: fiber.y,
                        z: fiber.z,
                        area: fiber.area,
                        strain: eps,
                        yield_ratio: if eref > 0.0 { eps.abs() / eref } else { 0.0 },
                        material: fiber.material,
                    }
                })
                .collect();
            out.push(FiberSectionState { xi: gp.xi, fibers });
        }
        (!out.is_empty()).then_some(out)
    }
}

#[cfg(test)]
mod tests;

mod wall_connection;

#[cfg(test)]
mod wall_connection_tests;

#[cfg(test)]
mod wall_mass_tests;

#[cfg(test)]
mod constitutive_tests;

#[cfg(test)]
mod member_load_tests;

#[cfg(test)]
mod independent_plastic_reference;

#[cfg(test)]
mod physical_curvature_tests;

#[cfg(test)]
mod distributed_curvature_tests;

#[cfg(test)]
mod rigid_axial_tests;
