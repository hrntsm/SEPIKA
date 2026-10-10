//! バネ / 履歴則パラメータ算定。
//!
//! 材端曲げバネの構築は [`build_flexural_springs`]。

use sepika_core::model::{
    default_fiber_concrete_hysteresis, default_member_hysteresis, AnalysisKind, ElementData,
    HysteresisModel, Model,
};
use sepika_material::uniaxial::{Bilinear, UniaxialMaterial};
use sepika_material::{HysteresisMaterial, HysteresisRule, SteelBuckling, TsujiYamada};

use super::regime::is_column_member;
use super::StrengthBasis;
use crate::frame::concentrated::MnInteraction;

/// 端部塑性化域モデルの塑性化域長 Lp [mm]。
pub fn plastic_zone_length(data: &ElementData, model: &Model) -> f64 {
    let depth = data
        .section
        .and_then(|sid| model.sections.get(sid.index()))
        .map(|s| s.depth)
        .filter(|d| *d > 0.0)
        .unwrap_or(200.0);
    data.plastic_zone.unwrap_or(0.5 * depth)
}

/// ファイバー梁の生成。
pub(super) fn build_fiber(
    data: &ElementData,
    model: &Model,
    basis: StrengthBasis,
    kind: AnalysisKind,
) -> crate::frame::fiber::FiberBeam {
    let lp = plastic_zone_length(data, model);
    crate::frame::fiber::FiberBeam::with_plastic_zone(data, model, lp, basis, kind)
}

/// 部材の曲げ終局（降伏）モーメント My [N·mm]。
/// RC=0.9·at·σy·j、鉄骨=Zp·σy（全塑性 Mp）、
/// それ以外（複合断面・形状不明）は σy·Z弾性でフォールバックする。
fn flexural_yield_moment(data: &ElementData, model: &Model, basis: StrengthBasis) -> f64 {
    let mat = model.element_material(data);
    let rebar_mat = model.element_rebar_material(data);
    sepika_core::flexural_strength::member_flexural_yield_moment(
        data,
        model,
        sepika_core::flexural_strength::FlexuralStrengthFactors {
            steel: basis.steel_factor(mat),
            rebar: basis.rebar_factor(rebar_mat),
        },
    )
}

/// 集中バネの降伏モーメント My0 と軸許容耐力 N許容 = σy·A（MN 相関用）。
pub(super) fn yield_moment_and_axial(
    data: &ElementData,
    model: &Model,
    basis: StrengthBasis,
) -> (f64, f64) {
    let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
    let mat = model.element_material(data);
    let fy_sigma = mat.and_then(|m| m.fy).unwrap_or(235.0) * basis.steel_factor(mat);
    let area = sec.map(|s| s.area).unwrap_or(1.0e4);
    (flexural_yield_moment(data, model, basis), fy_sigma * area)
}

/// 部材の可撓長さ [mm]（= 節点間長 − 両端剛域長。剛域控除後が非正なら全長）。
fn flexible_length(data: &ElementData, model: &Model) -> f64 {
    data.rigid_zone
        .flexible_length_from(model.member_length(data))
}

/// 材端曲げバネの初期回転剛性 k_rot [N·mm/rad] と降伏モーメント My [N·mm]。
/// k_rot は可とう長 L'（= L − 剛域長）基準で評価する。
fn rotational_spring_params(data: &ElementData, model: &Model, basis: StrengthBasis) -> (f64, f64) {
    let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
    let mat = model.element_material(data);
    let e = mat.map(|m| m.young).unwrap_or(205000.0);
    let iz = sec.map(|s| s.iz.max(s.iy)).unwrap_or(1.0e6);
    let (e, iz) = if sec
        .and_then(|s| s.shape.as_ref())
        .and_then(sepika_core::structure_kind::shape_composite_kind)
        == Some(sepika_core::structure_kind::StructureKind::Src)
    {
        let beam = crate::frame::beam::BeamElement::new(data, model);
        (beam.e, beam.iy.max(beam.iz))
    } else {
        (e, iz)
    };
    let my = flexural_yield_moment(data, model, basis);

    let l_eff = flexible_length(data, model);
    let k_rot = if l_eff > 0.0 {
        6.0 * e * iz / l_eff
    } else {
        1.0e12
    };
    (k_rot, my)
}

/// 断面形状がコンクリート系か否か。
pub(super) fn is_rc_like_section(data: &ElementData, model: &Model) -> bool {
    data.section
        .and_then(|sid| model.sections.get(sid.index()))
        .and_then(|s| s.shape.as_ref())
        .is_some_and(|s| s.is_concrete_like())
}

/// 解析種別に応じた部材個別指定のスロットを返す（増分用／時刻歴用）。
/// 時刻歴用スロットが未指定の部材は増分用の指定に従う（`member_hysteresis_th`）。
fn specified_hysteresis(
    data: &ElementData,
    model: &Model,
    kind: AnalysisKind,
) -> Option<HysteresisModel> {
    match kind {
        AnalysisKind::Incremental => model.member_hysteresis(data.id),
        AnalysisKind::TimeHistory => model.member_hysteresis_th(data.id),
    }
}

/// 部材の曲げ履歴則（材端集中バネ用）を解決する。
pub fn resolve_member_hysteresis(
    data: &ElementData,
    model: &Model,
    kind: AnalysisKind,
) -> HysteresisModel {
    match specified_hysteresis(data, model, kind) {
        Some(r) if r != HysteresisModel::Auto => r,
        _ => default_member_hysteresis(is_rc_like_section(data, model)),
    }
}

/// ファイバー断面・MS 要素のコンクリート除荷則を解決する。
pub fn resolve_fiber_concrete_hysteresis(
    data: &ElementData,
    model: &Model,
    kind: AnalysisKind,
) -> HysteresisModel {
    match specified_hysteresis(data, model, kind) {
        Some(
            r @ (HysteresisModel::Retrograde
            | HysteresisModel::OriginOriented
            | HysteresisModel::KarsanJirsa),
        ) => r,
        _ => default_fiber_concrete_hysteresis(kind),
    }
}

/// 耐震壁の壁柱ファイバのコンクリート除荷則を解決する。
pub fn resolve_wall_concrete_hysteresis(
    data: &ElementData,
    model: &Model,
    kind: AnalysisKind,
) -> HysteresisModel {
    match specified_hysteresis(data, model, kind) {
        Some(
            r @ (HysteresisModel::Retrograde
            | HysteresisModel::OriginOriented
            | HysteresisModel::KarsanJirsa),
        ) => r,
        _ => match kind {
            AnalysisKind::Incremental => HysteresisModel::OriginOriented,
            AnalysisKind::TimeHistory => HysteresisModel::KarsanJirsa,
        },
    }
}

/// 耐震壁の面内せん断ばねの履歴則を解決する。
pub fn resolve_wall_shear_hysteresis(
    data: &ElementData,
    model: &Model,
    kind: AnalysisKind,
) -> HysteresisModel {
    match specified_hysteresis(data, model, kind) {
        Some(
            r @ (HysteresisModel::Retrograde
            | HysteresisModel::Standard
            | HysteresisModel::OriginOriented
            | HysteresisModel::MaxPointOriented
            | HysteresisModel::Takeda),
        ) => r,
        _ => HysteresisModel::MaxPointOriented,
    }
}

/// 材端曲げバネのひび割れモーメント Mc [N·mm]。RC 系は Mc=0.56·√Fc·Ze、
/// それ以外は My/3 とする。
fn crack_moment(data: &ElementData, model: &Model, my: f64) -> f64 {
    let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
    let mat = model.element_material(data);
    let depth = sec.map(|s| s.depth.max(s.width)).unwrap_or(100.0);
    let iz = sec.map(|s| s.iz.max(s.iy)).unwrap_or(1.0e6);
    let ze = if depth > 0.0 { iz / (depth / 2.0) } else { 0.0 };
    match (is_rc_like_section(data, model), mat.and_then(|m| m.fc)) {
        (true, Some(fc)) if fc > 0.0 && ze > 0.0 => {
            sepika_core::rc_capacity::rc_crack_moment(fc, ze).clamp(my * 0.1, my * 0.9)
        }
        _ => my / 3.0,
    }
}

/// 材端曲げバネの降伏時剛性低下率 αy。
/// RC 矩形断面の梁（水平材）は菅野式で算定する。それ以外は 0.3 を用いる。
pub(super) fn flexural_alpha_y(data: &ElementData, model: &Model) -> f64 {
    flexural_alpha_y_checked(data, model).expect("RC梁のαy入力診断が必要です")
}

pub(super) fn flexural_alpha_y_checked(
    data: &ElementData,
    model: &Model,
) -> Result<f64, sepika_core::error::CoreError> {
    use sepika_core::section_shape::SectionShape;
    const DEFAULT_ALPHA_Y: f64 = 0.3;
    if is_column_member(data, model) {
        return Ok(DEFAULT_ALPHA_Y);
    }
    let sec = data.section.and_then(|sid| model.sections.get(sid.index()));
    let Some(shape) = sec.and_then(|s| s.shape.as_ref()) else {
        return Ok(DEFAULT_ALPHA_Y);
    };
    let (b, d, at, d_eff) = match shape {
        SectionShape::RcBeamRect { b, d, rebar } => {
            let bottom = rebar.bending_steel(*d, false).tension;
            let top = rebar.bending_steel(*d, true).tension;
            for steel in [bottom, top] {
                sepika_core::rc_capacity::rc_rebar_ratios(
                    *b,
                    *d,
                    steel.effective_depth_mm,
                    steel.area_mm2,
                    sepika_core::rc_capacity::RcAlphaSection::Rectangular,
                )?;
            }
            let steel = if bottom.area_mm2 <= top.area_mm2 {
                bottom
            } else {
                top
            };
            (*b, *d, steel.area_mm2, steel.effective_depth_mm)
        }
        _ => return Ok(DEFAULT_ALPHA_Y),
    };
    let pt_alpha_ratio = sepika_core::rc_capacity::rc_rebar_ratios(
        b,
        d,
        d_eff,
        at,
        sepika_core::rc_capacity::RcAlphaSection::Rectangular,
    )?
    .pt_alpha_ratio;
    let ec = model
        .element_material(data)
        .ok_or_else(|| {
            sepika_core::error::CoreError::InvalidInput("RC梁のEc材料が未指定です".into())
        })?
        .young;
    let es = model
        .element_rebar_material(data)
        .ok_or_else(|| {
            sepika_core::error::CoreError::InvalidInput("RC梁のEs材料が未指定です".into())
        })?
        .young;
    let n = es / ec;
    let l = model.member_length(data)
        - data.rigid_zone.rigid_length_i()
        - data.rigid_zone.rigid_length_j();
    sepika_core::rc_capacity::rc_alpha_y_sugano_checked(pt_alpha_ratio, l / (2.0 * d), d_eff / d, n)
}

pub(super) fn uses_rc_alpha_reference(data: &ElementData, model: &Model) -> bool {
    !is_column_member(data, model)
        && matches!(
            model.element_section(data).and_then(|s| s.shape.as_ref()),
            Some(sepika_core::section_shape::SectionShape::RcBeamRect { .. })
        )
}

pub(super) fn rc_reference_stiffness(data: &ElementData, model: &Model) -> Result<f64, String> {
    use sepika_core::model::{EndCondition, RcBeamReference};
    use sepika_core::section_shape::SectionShape;
    if model.member_rc_beam_reference(data.id) != Some(RcBeamReference::AntisymmetricHalfMember) {
        return Err(
            "RC梁の逆対称・同一半部材の基準分布が未指定です。実応力確認とは別の明示選択が必要です"
                .into(),
        );
    }
    let Some(SectionShape::RcBeamRect { b, d, rebar }) =
        model.element_section(data).and_then(|s| s.shape.as_ref())
    else {
        return Err("矩形RC梁の一様断面が必要です".into());
    };
    if rebar.top != rebar.bottom {
        return Err("逆対称基準には上下引張側が同じ配筋の両半部材が必要です".into());
    }
    if data
        .end_cond
        .iter()
        .any(|end| !matches!(end, EndCondition::Fixed))
    {
        return Err("逆対称RC基準の対象曲げ面に端部解放を設定できません".into());
    }
    if model
        .member_detail(data.id)
        .is_some_and(|detail| detail.haunch_i.is_some() || detail.haunch_j.is_some())
    {
        return Err("逆対称RC基準はハンチのない同一一様半部材に限定します".into());
    }
    if [
        data.rigid_zone.length_i,
        data.rigid_zone.length_j,
        data.rigid_zone.panel_offset_i,
        data.rigid_zone.panel_offset_j,
    ]
    .iter()
    .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("剛域長・パネルオフセットは非負の有限値が必要です".into());
    }
    let l = model.member_length(data)
        - data.rigid_zone.rigid_length_i()
        - data.rigid_zone.rigid_length_j();
    if !l.is_finite() || l <= 0.0 {
        return Err("剛域控除後の柔部材長Lは正の有限値が必要です。全長へ代用しません".into());
    }
    let beam = crate::frame::beam::BeamElement::try_new(data, model)?;
    let i0 = b * d.powi(3) / 12.0;
    if !beam.iz.is_finite() || (beam.iz - i0).abs() > i0.abs() * 1e-10 {
        return Err(
            "対象曲げ軸の弾性梁Iと矩形コンクリート幾何I0が一致しません。付帯断面の代用は行いません"
                .into(),
        );
    }
    sepika_core::rc_beam_backbone::RcBeamMomentDiagram::Antisymmetric {
        flexible_length_mm: l,
        identical_halves: true,
    }
    .initial_stiffness(beam.e * beam.iz)
    .map_err(|error| error.to_string())
}

fn rc_reference_crack_moment(data: &ElementData, model: &Model) -> Result<f64, String> {
    let Some(sepika_core::section_shape::SectionShape::RcBeamRect { b, d, .. }) = model
        .element_section(data)
        .and_then(|section| section.shape.as_ref())
    else {
        return Err("矩形RC梁の幾何断面が必要です".into());
    };
    let fc = model
        .element_material(data)
        .and_then(|material| material.fc)
        .ok_or_else(|| "RC梁のFcが必要です".to_string())?;
    // αy、My、I0と同じ矩形幾何を使用する。独立のSection.depthへ切り替えない。
    let mc = sepika_core::rc_capacity::rc_crack_moment(fc, b * d.powi(2) / 6.0);
    if !mc.is_finite() || mc <= 0.0 {
        return Err("RC梁の幾何断面によるMcは正の有限値が必要です".into());
    }
    Ok(mc)
}

pub(super) fn rc_reference_issue(
    data: &ElementData,
    model: &Model,
    rule: HysteresisModel,
) -> Option<String> {
    if !uses_rc_alpha_reference(data, model)
        || !super::hinge_view::resolves_to_concentrated_spring(data, model)
    {
        return None;
    }

    let adopted = matches!(
        rule,
        HysteresisModel::Takeda | HysteresisModel::Retrograde | HysteresisModel::MaxPointOriented
    );
    if rule == HysteresisModel::OriginOriented {
        return Some(
            "原点指向型は初期K0とひび割れ点を持たないため採用RC梁αyの総角接続に未対応です".into(),
        );
    }
    if !adopted {
        if model.member_rc_beam_reference(data.id).is_some() {
            return Some(
                "指定履歴則は採用RC梁αy基準接続の対象外です（武田型・逆行型・最大点指向型のみ）"
                    .into(),
            );
        }
        return None;
    }
    let s = match rc_reference_stiffness(data, model) {
        Ok(s) => s,
        Err(error) => return Some(error),
    };
    let alpha = match flexural_alpha_y_checked(data, model) {
        Ok(alpha) => alpha,
        Err(error) => return Some(error.to_string()),
    };
    let my = flexural_yield_moment(data, model, StrengthBasis::Nominal);
    let mc = match rc_reference_crack_moment(data, model) {
        Ok(mc) => mc,
        Err(error) => return Some(error),
    };
    let result = match sepika_core::rc_beam_backbone::rc_beam_yield_evaluation(my, alpha, s) {
        Ok(result) => result,
        Err(error) => return Some(error.to_string()),
    };
    if let Err(error) = result.additional_flexibility {
        return Some(error.to_string());
    }
    if let Err(error) = sepika_core::rc_beam_backbone::rc_beam_second_slope(
        mc,
        my,
        mc / s,
        result.total_rotation_rad,
    ) {
        return Some(error.to_string());
    }
    None
}

/// 材端曲げバネの骨格種別。N-M 相関時の折れ点の置換規則を決める。
#[derive(Clone, Debug)]
enum FlexuralBackboneKind {
    /// 標準型（バイリニア）。
    Bilinear,
    /// 辻・山田型。
    TsujiYamada,
    /// 座屈考慮型。構築済みの材料を保持する。
    SteelBuckling(SteelBuckling),
    /// 履歴材料（武田型・逆行型・原点指向型・最大点指向型）。
    Explicit,
}

/// 材端曲げバネの M-θ 骨格と N-M 相関の適用可否。
#[derive(Clone, Debug)]
pub struct FlexuralSpringBackbone {
    /// 初期回転剛性 k_rot [N·mm/rad]。
    pub k_rot: f64,
    /// 材料構築に渡した降伏後接線剛性 [N·mm/rad]（バイリニア系のみ。他は 0）。
    pub post_yield_stiffness: f64,
    /// N-M 相関なしの折れ点 [theta(rad), moment(N·mm)]。
    pub points: Vec<[f64; 2]>,
    /// N-M 相関（`set_yield`）を適用可能か。
    pub use_mn: bool,
    kind: FlexuralBackboneKind,
}

impl FlexuralSpringBackbone {
    /// 軸力に対する N-M 線形相関後の折れ点 [theta(rad), moment(N·mm)]。
    /// `mn` が None、または N-M 相関非対応の履歴則では素の折れ点を返す。
    /// `axial_force` [N] は絶対値のみを用いるため符号は問わない。
    pub(crate) fn points_with_mn(
        &self,
        mn: Option<&MnInteraction>,
        axial_force: f64,
    ) -> Vec<[f64; 2]> {
        let Some(mn) = mn.filter(|_| self.use_mn) else {
            return self.points.clone();
        };
        let m = mn.moment_limit(axial_force);
        match &self.kind {
            FlexuralBackboneKind::Bilinear | FlexuralBackboneKind::TsujiYamada => {
                if self.k_rot > 0.0 {
                    bilinear_backbone_points(self.k_rot, m, self.post_yield_stiffness)
                } else {
                    self.points.clone()
                }
            }
            FlexuralBackboneKind::SteelBuckling(template) => {
                let mut material = template.clone();
                material.set_yield(m);
                material.skeleton_points()
            }
            FlexuralBackboneKind::Explicit => self.points.clone(),
        }
    }
}

/// バイリニア系の折れ点 `[(0,0), (My/k, My), (4θy, My + k2·3θy)]`。
/// `k2` は材料構築に渡した降伏後接線 [N·mm/rad]。`k_rot<=0` は原点のみ。
fn bilinear_backbone_points(k_rot: f64, my: f64, k2: f64) -> Vec<[f64; 2]> {
    if k_rot <= 0.0 {
        return vec![[0.0, 0.0]];
    }
    let theta_y = my / k_rot;
    let theta_end = 4.0 * theta_y;
    vec![
        [0.0, 0.0],
        [theta_y, my],
        [theta_end, my + k2 * (theta_end - theta_y)],
    ]
}

/// 材端曲げバネの復元力材料を履歴則に応じて構築する。
/// 戻り値の [`FlexuralSpringBackbone::use_mn`] は N-M 相関（`set_yield`）を
/// 適用可能か。
pub(super) fn build_flexural_springs(
    data: &ElementData,
    model: &Model,
    rule: HysteresisModel,
    basis: StrengthBasis,
) -> (
    Box<dyn UniaxialMaterial>,
    Box<dyn UniaxialMaterial>,
    FlexuralSpringBackbone,
) {
    let (mut k_rot, my) = rotational_spring_params(data, model, basis);
    let rc_reference = uses_rc_alpha_reference(data, model)
        && matches!(
            rule,
            HysteresisModel::Takeda
                | HysteresisModel::Retrograde
                | HysteresisModel::MaxPointOriented
        );
    if rc_reference {
        if let Some(issue) = rc_reference_issue(data, model, rule) {
            panic!("{issue}");
        }
        k_rot = rc_reference_stiffness(data, model).expect("RC基準剛性の入力診断が必要です");
    }
    if my <= 0.0 || k_rot <= 0.0 || rule == HysteresisModel::Standard {
        let my = my.max(1.0);
        let backbone = FlexuralSpringBackbone {
            k_rot,
            post_yield_stiffness: 0.01 * k_rot,
            points: bilinear_backbone_points(k_rot, my, 0.01 * k_rot),
            use_mn: true,
            kind: FlexuralBackboneKind::Bilinear,
        };
        return (
            Box::new(Bilinear::new(k_rot, my, 0.01)),
            Box::new(Bilinear::new(k_rot, my, 0.01)),
            backbone,
        );
    }
    if rule == HysteresisModel::TsujiYamada {
        let k2 = 0.01 * k_rot;
        let mk = || Box::new(TsujiYamada::new(k_rot, my, k2, 0.5)) as Box<dyn UniaxialMaterial>;
        let backbone = FlexuralSpringBackbone {
            k_rot,
            post_yield_stiffness: k2,
            points: bilinear_backbone_points(k_rot, my, k2),
            use_mn: true,
            kind: FlexuralBackboneKind::TsujiYamada,
        };
        return (mk(), mk(), backbone);
    }
    if rule == HysteresisModel::SteelBuckling {
        let template = SteelBuckling::with_defaults(k_rot, my, 1.1);
        let mk = {
            let template = template.clone();
            move || Box::new(template.clone()) as Box<dyn UniaxialMaterial>
        };
        let backbone = FlexuralSpringBackbone {
            k_rot,
            post_yield_stiffness: 0.0,
            points: template.skeleton_points(),
            use_mn: true,
            kind: FlexuralBackboneKind::SteelBuckling(template),
        };
        return (mk(), mk(), backbone);
    }
    let mc = if rc_reference {
        rc_reference_crack_moment(data, model).expect("RC幾何Mcの入力診断が必要です")
    } else {
        crack_moment(data, model, my)
    };
    let tc = if rc_reference {
        mc / k_rot
    } else {
        (mc / k_rot).max(1e-9)
    };
    let alpha_y = flexural_alpha_y(data, model);
    let ty = if rc_reference {
        let evaluation =
            sepika_core::rc_beam_backbone::rc_beam_yield_evaluation(my, alpha_y, k_rot)
                .expect("RC降伏角の入力診断が必要です");
        evaluation
            .additional_flexibility
            .expect("RC受動接続の入力診断が必要です");
        sepika_core::rc_beam_backbone::rc_beam_second_slope(
            mc,
            my,
            tc,
            evaluation.total_rotation_rad,
        )
        .expect("RC折れ点の入力診断が必要です");
        evaluation.total_rotation_rad
    } else {
        (my / (alpha_y * k_rot)).max(tc * 1.5)
    };
    let mu = 1.1 * my;
    let tu = ty * 4.0;
    let alpha = 0.4;
    let mk =
        |r: HysteresisRule| -> Box<dyn UniaxialMaterial> { Box::new(HysteresisMaterial::new(r)) };
    let make_pair = |r: HysteresisRule| (mk(r.clone()), mk(r));
    let (a, b, points) = match rule {
        HysteresisModel::Retrograde => {
            let (a, b) = make_pair(HysteresisRule::Retrograde {
                crack: (mc, tc),
                yield_point: (my, ty),
                ultimate: (mu, tu),
            });
            (a, b, vec![[0.0, 0.0], [tc, mc], [ty, my], [tu, mu]])
        }
        HysteresisModel::OriginOriented => {
            let (a, b) = make_pair(HysteresisRule::OriginOriented {
                yield_point: (my, ty),
                ultimate: (mu, tu),
            });
            (a, b, vec![[0.0, 0.0], [ty, my], [tu, mu]])
        }
        HysteresisModel::MaxPointOriented => {
            let (a, b) = make_pair(HysteresisRule::MaxPointOriented {
                crack: (mc, tc),
                yield_point: (my, ty),
                ultimate: (mu, tu),
            });
            (a, b, vec![[0.0, 0.0], [tc, mc], [ty, my], [tu, mu]])
        }
        _ => {
            let (a, b) = make_pair(HysteresisRule::Takeda {
                crack: (mc, tc),
                yield_point: (my, ty),
                ultimate: (mu, tu),
                alpha,
            });
            (a, b, vec![[0.0, 0.0], [tc, mc], [ty, my], [tu, mu]])
        }
    };
    let backbone = FlexuralSpringBackbone {
        k_rot,
        post_yield_stiffness: 0.0,
        points,
        use_mn: false,
        kind: FlexuralBackboneKind::Explicit,
    };
    (a, b, backbone)
}
