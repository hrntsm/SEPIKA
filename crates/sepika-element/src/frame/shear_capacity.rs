//! 増分解析で用いる線材のせん断降伏条件。

mod box_section;
mod rc;

use box_section::BoxSection;
use rc::RcSection;
use sepika_core::material_grade::resolve_steel_f;
use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::section_shape::SectionShape;
use sepika_core::structure_kind::{member_structure_kind, StructureKind};

/// 線材のせん断降伏面。局所力の順は [Qy, Qz]、対応する曲げは [Mz, My]。
#[derive(Clone, Debug)]
pub struct ShearCapacity(Capacity);

#[derive(Clone, Debug)]
enum Capacity {
    Web(f64),
    Box(BoxSection),
    Pipe(f64),
    Rc(Box<RcSection>),
}

/// 無次元の降伏利用率とその局所せん断力 [N] に関する勾配。
#[derive(Clone, Copy, Debug)]
pub struct ShearLimit {
    pub utilization: f64,
    pub normal_per_n: [f64; 2],
}

/// 同じ試行状態の降伏面への射影で共用する局所条件。
pub struct PreparedShearCapacity<'a>(Projection<'a>);

enum Projection<'a> {
    Steel(&'a ShearCapacity),
    Rc([f64; 2]),
}

impl PreparedShearCapacity<'_> {
    /// 硬化分を差し引いた局所せん断力 [N] に対する利用率と勾配を返す。
    pub fn limit(&self, shifted_shear_n: [f64; 2]) -> Result<ShearLimit, String> {
        match self.0 {
            Projection::Steel(steel) => steel.limit(shifted_shear_n, 0.0, [0.0; 2]),
            Projection::Rc(axes) => {
                rc::ellipse_limit(shifted_shear_n, axes.map(|capacity| (capacity, 1.0))).checked()
            }
        }
    }
}

impl ShearLimit {
    fn zero() -> Self {
        Self {
            utilization: 0.0,
            normal_per_n: [0.0; 2],
        }
    }

    fn direction(axis: usize, force_n: f64, capacity_n: f64) -> Self {
        let mut normal_per_n = [0.0; 2];
        normal_per_n[axis] = (1.0 / capacity_n).copysign(force_n);
        Self {
            utilization: force_n.abs() / capacity_n,
            normal_per_n,
        }
    }

    fn checked(self) -> Result<Self, String> {
        if !self.utilization.is_finite() || self.normal_per_n.into_iter().any(|v| !v.is_finite()) {
            return Err("線材のせん断降伏条件を有限値として評価できません".into());
        }
        Ok(self)
    }
}

impl ShearCapacity {
    /// 増分解析のS造・RC造線材に使う断面の条件。今回の対象外は `None`。
    pub fn from_model(model: &Model, elem: &ElementData) -> Result<Option<Self>, String> {
        if !matches!(elem.kind, ElementKind::Beam | ElementKind::Fiber)
            || super::beam::is_elastic_wall_girder(model, elem)
        {
            return Ok(None);
        }
        let kind = member_structure_kind(model, elem);
        if !matches!(kind, StructureKind::Rc | StructureKind::S) {
            return Ok(None);
        }
        let section = model.element_section(elem).ok_or("断面が未割当です")?;
        let shape = section
            .shape
            .as_ref()
            .ok_or("せん断降伏を確認する断面形状がありません")?;
        let material = model.element_material(elem).ok_or("材料が未割当です")?;
        if kind == StructureKind::Rc {
            return RcSection::from_model(model, elem, shape, material)
                .map(|section| Some(Self(Capacity::Rc(Box::new(section)))));
        }
        if model.steel_design_attrs.iter().any(|attr| {
            attr.elem == elem.id
                && (attr.joint_web_loss != 0.0
                    || attr.joint_flange_loss != 0.0
                    || attr.scallop_web_loss != 0.0)
        }) {
            return Err("断面欠損のあるS造線材のせん断降伏は適用範囲外です".into());
        }
        let strength = |thickness_mm| resolve_steel_f(Some(material), Some(thickness_mm));
        let capacity = match *shape {
            SectionShape::SteelH {
                height,
                web_thick,
                flange_thick,
                ..
            } => Self::web(height - 2.0 * flange_thick, web_thick, strength(web_thick)?)?,
            SectionShape::SteelBuiltH {
                height,
                web_thick,
                upper_thick,
                lower_thick,
                ..
            } => Self::web(
                height - upper_thick - lower_thick,
                web_thick,
                strength(web_thick)?,
            )?,
            SectionShape::SteelBox {
                height,
                width,
                thick,
                horizontal_thick,
                corner_r,
            } => Capacity::Box(BoxSection::new(
                height,
                width,
                thick,
                horizontal_thick,
                corner_r,
                material,
            )?),
            SectionShape::SteelPipe { outer_dia, thick } => {
                if !thick.is_finite()
                    || !outer_dia.is_finite()
                    || thick <= 0.0
                    || outer_dia <= 2.0 * thick
                {
                    return Err("円形鋼管の断面寸法が不正です".into());
                }
                let area_mm2 = std::f64::consts::PI * thick * (outer_dia - thick);
                let yield_n = area_mm2 / 2.0 * strength(thick)? / 3.0_f64.sqrt();
                Capacity::Pipe(Self::positive_strength(yield_n)?)
            }
            _ => {
                return Err(
                    "このS造線材の断面形状についてせん断降伏の適用条件を確認できません".into(),
                )
            }
        };
        Ok(Some(Self(capacity)))
    }

    fn web(depth_mm: f64, thickness_mm: f64, fy_mpa: f64) -> Result<Capacity, String> {
        if !depth_mm.is_finite()
            || depth_mm <= 0.0
            || !thickness_mm.is_finite()
            || thickness_mm <= 0.0
        {
            return Err("H形断面のウェブ内法せいと板厚は正の有限値が必要です".into());
        }
        let yield_n = depth_mm * thickness_mm * fy_mpa / 3.0_f64.sqrt();
        Ok(Capacity::Web(Self::positive_strength(yield_n)?))
    }

    fn positive_strength(yield_n: f64) -> Result<f64, String> {
        if !yield_n.is_finite() || yield_n <= 0.0 {
            return Err("線材のせん断降伏耐力を正の有限値として算定できません".into());
        }
        Ok(yield_n)
    }

    /// 終局せん断耐力への到達を設計用に制限するRC線材かを返す。
    pub fn is_rc(&self) -> bool {
        matches!(self.0, Capacity::Rc(_))
    }

    pub(crate) fn is_web(&self) -> bool {
        matches!(self.0, Capacity::Web(_))
    }

    pub(crate) fn uses_axial_force(&self) -> bool {
        matches!(&self.0, Capacity::Rc(section) if section.uses_axial_force())
    }

    /// 実際の断面力での適用範囲を確認する。軸力は圧縮正 [N]。
    pub fn validate_response(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
    ) -> Result<(), String> {
        if shear_n
            .into_iter()
            .chain(moments_nmm)
            .chain([axial_compression_n])
            .any(|f| !f.is_finite())
        {
            return Err("線材の断面力は有限値が必要です".into());
        }
        match &self.0 {
            Capacity::Web(yield_n) if shear_n[1].abs() > 64.0 * f64::EPSILON * yield_n => {
                Err("H形断面の弱軸せん断に対する非線形せん断の適用条件を確認できません".into())
            }
            Capacity::Rc(section) => section.validate_response(shear_n, axial_compression_n),
            _ => Ok(()),
        }
    }

    /// 試行状態の降伏面を評価する。軸力は圧縮正 [N]、曲げは [N·mm]。
    pub fn limit(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
    ) -> Result<ShearLimit, String> {
        if shear_n
            .into_iter()
            .chain(moments_nmm)
            .chain([axial_compression_n])
            .any(|f| !f.is_finite())
        {
            return Err("線材の断面力は有限値が必要です".into());
        }
        let result = match &self.0 {
            Capacity::Web(yield_n) => ShearLimit::direction(0, shear_n[0], *yield_n),
            Capacity::Box(section) => section.limit(shear_n),
            Capacity::Pipe(yield_n) => {
                let norm_n = shear_n[0].hypot(shear_n[1]);
                if norm_n == 0.0 {
                    ShearLimit::zero()
                } else {
                    ShearLimit {
                        utilization: norm_n / yield_n,
                        normal_per_n: shear_n.map(|q| q / norm_n / yield_n),
                    }
                }
            }
            Capacity::Rc(section) => section.limit(shear_n, axial_compression_n, moments_nmm)?,
        };
        result.checked()
    }

    /// 試行状態 [N, N·mm] に対応するせん断降伏面を、局所射影で再利用できる状態にする。
    pub fn prepare_projection(
        &self,
        physical_shear_n: [f64; 2],
        axial_compression_n: f64,
        moments_nmm: [f64; 2],
    ) -> Result<PreparedShearCapacity<'_>, String> {
        if physical_shear_n
            .into_iter()
            .chain(moments_nmm)
            .chain([axial_compression_n])
            .any(|v| !v.is_finite())
        {
            return Err("線材の断面力は有限値が必要です".into());
        }
        match &self.0 {
            Capacity::Rc(rc) => rc
                .projection_axes(physical_shear_n, axial_compression_n, moments_nmm)
                .map(|axes| PreparedShearCapacity(Projection::Rc(axes))),
            _ => Ok(PreparedShearCapacity(Projection::Steel(self))),
        }
    }

    /// 絶対せん断上界 [N]・圧縮軸力下界 [N]・軸別の曲げの [下界, 上界] [N·mm] から利用率上界を求める。
    pub fn bound_utilization(
        &self,
        shear_n: [f64; 2],
        axial_compression_n: f64,
        moment_bounds_nmm: [[f64; 2]; 2],
    ) -> Result<f64, String> {
        if moment_bounds_nmm
            .iter()
            .any(|&[lo, hi]| !lo.is_finite() || !hi.is_finite() || lo > hi)
        {
            return Err("材端間の曲げの範囲を確認できません".into());
        }
        let moments_nmm =
            moment_bounds_nmm.map(|[lo, hi]| if lo.abs() > hi.abs() { lo } else { hi });
        let unknown_sign = moment_bounds_nmm.map(|[lo, hi]| lo <= 0.0 && hi >= 0.0);
        self.validate_response(shear_n, axial_compression_n, moments_nmm)?;
        let utilization = match &self.0 {
            Capacity::Rc(rc) => {
                rc.bound_utilization(shear_n, axial_compression_n, moments_nmm, unknown_sign)?
            }
            _ => {
                self.limit(shear_n, axial_compression_n, moments_nmm)?
                    .utilization
            }
        };
        utilization
            .is_finite()
            .then_some(utilization)
            .ok_or_else(|| "せん断利用率の上界を有限値として評価できません".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
    use sepika_core::model::{
        EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory, Node,
    };

    fn steel() -> Material {
        Material {
            id: MaterialId(0),
            name: "SS400".into(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: Some(235.0),
            concrete_class: Default::default(),
            strength_factor: None,
        }
    }

    fn model_with_shape(shape: SectionShape) -> Model {
        let mut section = shape.to_section(SectionId(0), "S".into());
        section.material = Some(MaterialId(0));
        Model {
            nodes: [0.0, 3_000.0]
                .into_iter()
                .enumerate()
                .map(|(i, x)| Node {
                    id: NodeId(i as u32),
                    coord: [x, 0.0, 0.0],
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                })
                .collect(),
            elements: vec![ElementData {
                id: ElemId(0),
                kind: ElementKind::Fiber,
                nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
                section: Some(SectionId(0)),
                local_axis: LocalAxis {
                    ref_vector: [0.0, 1.0, 0.0],
                },
                end_cond: [EndCondition::Fixed; 2],
                force_regime: ForceRegime::Auto,
                rigid_zone: Default::default(),
                end_offsets_mm: [[0.0; 3]; 2],
                plastic_zone: None,
                spring: None,
            }],
            sections: vec![section],
            materials: vec![steel()],
            ..Default::default()
        }
    }

    fn from_shape(shape: SectionShape) -> ShearCapacity {
        let model = model_with_shape(shape);
        ShearCapacity::from_model(&model, &model.elements[0])
            .unwrap()
            .unwrap()
    }

    #[test]
    fn rc_beam_moment_reversal_selects_the_actual_tension_side() {
        use sepika_core::section_shape::{BeamStirrup, RcBeamRebar, RebarSize, StandardRebar};
        let mut model = model_with_shape(SectionShape::RcBeamRect {
            b: 450.0,
            d: 600.0,
            rebar: RcBeamRebar {
                main_dia: RebarSize::Standard(StandardRebar::D25),
                top: vec![3],
                bottom: vec![6],
                cover: 40.0,
                stirrup: BeamStirrup {
                    dia: RebarSize::Standard(StandardRebar::D10),
                    pitch: 100.0,
                    legs: 2,
                },
            },
        });
        model.materials[0].category = MaterialCategory::Concrete;
        model.materials[0].name = "Fc24".into();
        model.materials[0].fc = Some(24.0);
        model.materials[0].fy = None;
        let reinforcement = Material {
            id: MaterialId(1),
            name: "SD295".into(),
            category: MaterialCategory::Rebar,
            fy: Some(295.0),
            ..steel()
        };
        model.materials.push(reinforcement);
        model.sections[0].rebar_material = Some(MaterialId(1));
        model.sections[0].shear_rebar_material = Some(MaterialId(1));
        let capacity = ShearCapacity::from_model(&model, &model.elements[0])
            .unwrap()
            .unwrap();

        let trial_shear_n = [250_000.0, 0.0];
        for (moment_nmm, actual_utilization, contact_n) in [
            (260.0e6, 0.516_523_187_090_322_4, 742_797.546_679_799_2),
            (-260.0e6, 0.570_363_525_657_061_9, 658_972.181_736_813_9),
            (
                0.0,
                250_000.0 / 658_972.181_736_813_9,
                658_972.181_736_813_9,
            ),
        ] {
            let response = capacity
                .limit(trial_shear_n, 0.0, [moment_nmm, 0.0])
                .unwrap();
            assert!((response.utilization - actual_utilization).abs() < 1e-12);
            let projection = capacity
                .prepare_projection(trial_shear_n, 0.0, [moment_nmm, 0.0])
                .unwrap();
            assert!((projection.limit([contact_n, 0.0]).unwrap().utilization - 1.0).abs() < 1e-12);
        }
        for (moment_bounds, expected) in [
            ([240.0e6, 260.0e6], 0.516_523_187_090_322_4),
            ([-260.0e6, -240.0e6], 0.570_363_525_657_061_9),
            ([-260.0e6, 260.0e6], 0.570_363_525_657_061_9),
        ] {
            let span_bound = capacity
                .bound_utilization(trial_shear_n, 0.0, [moment_bounds, [0.0; 2]])
                .unwrap();
            assert!((span_bound - expected).abs() < 1e-12);
        }
    }

    #[test]
    fn h_web_uses_clear_web_area_and_ignores_axial_force_in_the_yield_model() {
        let section = from_shape(SectionShape::SteelH {
            height: 300.0,
            width: 150.0,
            web_thick: 8.0,
            flange_thick: 12.0,
            root_r: Some(0.0),
        });
        let yield_n = 299_575.507_677_113_06;
        let result = section.limit([yield_n, 0.0], 500_000.0, [0.0; 2]).unwrap();
        assert!((result.utilization - 1.0).abs() < 1e-12);
        assert_eq!(result.normal_per_n[1], 0.0);
        assert!(section
            .validate_response([yield_n, 1.0], 500_000.0, [0.0; 2])
            .is_err());
    }

    #[test]
    fn square_box_counts_both_directions_at_the_same_plate_location() {
        let section = from_shape(SectionShape::SteelBox {
            height: 210.0,
            width: 210.0,
            thick: 10.0,
            horizontal_thick: 10.0,
            corner_r: Some(0.0),
        });
        let public_limit_n = 542_709.253_038_248_3;
        let one_direction = section.limit([public_limit_n, 0.0], 0.0, [0.0; 2]).unwrap();
        assert!((one_direction.utilization - 1.125).abs() < 1e-12);
        let two_directions = section
            .limit([0.8 * public_limit_n, -0.8 * public_limit_n], 0.0, [0.0; 2])
            .unwrap();
        assert!((two_directions.utilization - 1.2).abs() < 1e-12);
        assert!(two_directions.normal_per_n[0] > 0.0);
        assert!(two_directions.normal_per_n[1] < 0.0);
    }

    #[test]
    fn rounded_rectangular_box_finds_the_current_critical_stress_inside_the_corner() {
        let section = from_shape(SectionShape::SteelBox {
            height: 300.0,
            width: 200.0,
            thick: 12.0,
            horizontal_thick: 12.0,
            corner_r: Some(28.0),
        });
        let result = section
            .limit([250_000.0, 220_000.0], 0.0, [0.0; 2])
            .unwrap();
        // 板厚中心線を各区間20万分割したせん断流積分では角丸の約61%で最大。
        assert!((result.utilization - 0.481_955_068_15).abs() < 1e-8);
        assert!((result.normal_per_n[0] - 0.689_846e-6).abs() < 1e-11);
        assert!((result.normal_per_n[1] - 1.406_789e-6).abs() < 1e-11);
    }

    #[test]
    fn round_pipe_yield_is_invariant_under_rotation_of_shear_force() {
        let section = from_shape(SectionShape::SteelPipe {
            outer_dia: 300.0,
            thick: 10.0,
        });
        let yield_n = 618_052.133_362_809_8;
        let limit = section
            .limit([0.6 * yield_n, 0.8 * yield_n], 0.0, [0.0; 2])
            .unwrap();
        assert!((limit.utilization - 1.0).abs() < 1e-12);
        assert!((limit.normal_per_n[0] / limit.normal_per_n[1] - 0.75).abs() < 1e-12);
    }
}
