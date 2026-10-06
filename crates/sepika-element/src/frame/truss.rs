use crate::behavior::{Ctx, ElementBehavior, LocalMat, MassOption};
use crate::frame::section_lookup::{get_material, get_section, sec_material};
use crate::transform::LocalFrame;
use sepika_core::dof::DofMap;
use sepika_core::ids::{ElemId, NodeId};
use sepika_core::model::{ElementData, Model};
use smallvec::SmallVec;

/// 一般ブレース要素。軸剛性のみを持ち、曲げ・せん断・ねじりはゼロ。
#[derive(Clone)]
pub struct TrussElement {
    pub id: ElemId,
    pub e: f64,
    /// 軸剛性用断面積（降伏部の断面積）。
    pub a: f64,
    /// 共通の材料領域算定による単位長さ当たり質量 [t/mm]。
    pub mass_per_length: f64,
    pub length: f64,
    pub nodes: [NodeId; 2],
    pub axis: LocalFrame,
    /// 確定変位（グローバル座標系）。
    pub committed_disp: [f64; 12],
    /// トライアル変位（グローバル座標系）。
    pub trial_disp: [f64; 12],
}

impl TrussElement {
    /// 算定不能なら panic。入力値の検証には `try_new` を用いる。
    pub fn new(data: &ElementData, model: &Model) -> Self {
        Self::try_new(data, model)
            .unwrap_or_else(|error| panic!("ブレースの質量特性を解決できません: {error}"))
    }

    /// 入力 A は軸剛性に用い、物理質量は共通算定を用いる。算定不能なら理由付きエラー。
    pub fn try_new(data: &ElementData, model: &Model) -> Result<Self, String> {
        let geom = crate::transform::EndGeometry::of_element(data, model);
        let [n0, n1] = geom.nodes;
        let len = geom.length;

        let axis = geom.local_frame(data.local_axis.ref_vector);
        let sec = get_section(model, data.section);
        let mat = get_material(model, sec_material(model, data));
        let a = sec.resolved_area()?;
        let mass_per_length = model.element_mass_properties(data)?.mass_per_length;
        if !len.is_finite()
            || len < 0.0
            || !mass_per_length.is_finite()
            || mass_per_length < 0.0
            || !(mass_per_length * len).is_finite()
        {
            return Err("ブレースの長さ・総質量が有限な非負値として算定できません".into());
        }

        Ok(Self {
            id: data.id,
            e: mat.young,
            a,
            mass_per_length,
            length: len,
            nodes: [n0, n1],
            axis,
            committed_disp: [0.0; 12],
            trial_disp: [0.0; 12],
        })
    }

    /// 局所座標系での 12×12 剛性行列。軸方向（ux, ux_j）成分のみ非ゼロ。
    pub fn local_stiffness(&self) -> LocalMat {
        let mut k = LocalMat::zeros(12);
        if self.length < 1e-12 {
            return k;
        }
        let ka = self.e * self.a / self.length;
        k.set(0, 0, ka);
        k.set(6, 6, ka);
        k.set(0, 6, -ka);
        k.set(6, 0, -ka);
        k
    }
}

impl ElementBehavior for TrussElement {
    fn n_dof(&self) -> usize {
        12
    }

    fn global_dofs(&self, dof: &DofMap) -> SmallVec<[usize; 24]> {
        crate::behavior::node_global_dofs(&self.nodes, dof)
    }

    fn tangent_stiffness(&self, _ctx: &Ctx) -> LocalMat {
        self.axis.to_global(&self.local_stiffness())
    }

    crate::behavior::elastic_disp_behavior!(TrussElement, 12);

    fn mass_matrix(&self, opt: MassOption) -> LocalMat {
        let m = self.mass_per_length * self.length;
        let mut mm = LocalMat::zeros(12);
        match opt {
            MassOption::Lumped => {
                for d in [0, 1, 2, 6, 7, 8] {
                    mm.set(d, d, m / 2.0);
                }
                mm
            }
            MassOption::Consistent => {
                let c1 = m / 6.0;
                mm.set(0, 0, 2.0 * c1);
                mm.set(0, 6, 1.0 * c1);
                mm.set(6, 0, 1.0 * c1);
                mm.set(6, 6, 2.0 * c1);
                for d in [1, 2, 7, 8] {
                    mm.set(d, d, m / 2.0);
                }
                self.axis.to_global(&mm)
            }
        }
    }

    fn recover_forces(&self, u_elem: &[f64]) -> Option<crate::frame::beam::MemberForces> {
        let f_local = self
            .axis
            .local_end_forces(&self.local_stiffness(), u_elem)?;
        let n = -f_local[0];
        Some(crate::frame::beam::MemberForces {
            at: vec![
                (0.0, [n, 0.0, 0.0, 0.0, 0.0, 0.0]),
                (1.0, [n, 0.0, 0.0, 0.0, 0.0, 0.0]),
            ],
        })
    }

    fn state_member_forces(&self, _ctx: &Ctx) -> Option<crate::frame::beam::MemberForces> {
        self.recover_forces(&self.trial_disp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::LocalVec;
    use sepika_core::ids::{ElemId, MaterialId, NodeId, SectionId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Material, MaterialCategory,
        Node, RigidZone, Section,
    };

    fn make_model(p0: [f64; 3], p1: [f64; 3]) -> (Model, ElementData) {
        let model = Model {
            nodes: vec![
                Node {
                    id: NodeId(0),
                    coord: p0,
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                },
                Node {
                    id: NodeId(1),
                    coord: p1,
                    restraint: Default::default(),
                    mass: None,
                    story: None,
                    support_spring: None,
                },
            ],
            sections: vec![Section {
                frame_use: None,
                id: SectionId(0),
                name: "brace".to_string(),
                area: 2000.0,
                iy: 0.0,
                iz: 0.0,
                j: 0.0,
                depth: 100.0,
                width: 100.0,
                as_y: 0.0,
                as_z: 0.0,
                floor: None,
                panel_thickness: None,
                thickness: None,
                shape: None,
                material: Some(MaterialId(0)),
                rebar_material: None,
                shear_rebar_material: None,
                steel_material: None,
                property_basis: Default::default(),
            }],
            materials: vec![Material {
                strength_factor: None,
                concrete_class: Default::default(),
                id: MaterialId(0),
                name: "steel".to_string(),
                category: MaterialCategory::Steel,
                young: 205000.0,
                poisson: 0.3,
                density: 7.85e-9,
                shear: None,
                fc: None,
                fy: Some(235.0),
            }],
            ..Default::default()
        };
        let data = ElementData {
            id: ElemId(0),
            kind: ElementKind::Brace {
                tension_only: false,
            },
            nodes: smallvec::smallvec![NodeId(0), NodeId(1)],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Pinned, EndCondition::Pinned],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            plastic_zone: None,
            spring: None,
        };
        (model, data)
    }

    fn assert_total_mass(behavior: &dyn ElementBehavior, expected: f64) {
        for mode in [MassOption::Lumped, MassOption::Consistent] {
            let matrix = behavior.mass_matrix(mode);
            for direction in 0..3 {
                let actual = matrix.get(direction, direction)
                    + matrix.get(direction, direction + 6)
                    + matrix.get(direction + 6, direction)
                    + matrix.get(direction + 6, direction + 6);
                approx::assert_relative_eq!(actual, expected, max_relative = 1.0e-12);
            }
        }
    }

    #[test]
    fn supplied_area_stays_in_stiffness_while_rounded_brace_mass_uses_common_geometry() {
        use sepika_core::model::{FrameSectionUse, PropertyBasis};
        use sepika_core::section_shape::SectionShape;
        for end in [[3000.0, 0.0, 0.0], [1000.0, 2000.0, 2000.0]] {
            for (shape, radius, expected_area) in [
                (
                    SectionShape::SteelH {
                        height: 400.0,
                        width: 200.0,
                        web_thick: 9.0,
                        flange_thick: 12.0,
                        root_r: Some(0.0),
                    },
                    13.0,
                    8184.0 + (4.0 - std::f64::consts::PI) * 13.0_f64.powi(2),
                ),
                (
                    SectionShape::SteelBox {
                        height: 500.0,
                        width: 300.0,
                        thick: 10.0,
                        corner_r: Some(0.0),
                    },
                    30.0,
                    15600.0 - (4.0 - std::f64::consts::PI) * (30.0_f64.powi(2) - 20.0_f64.powi(2)),
                ),
            ] {
                let (mut model, data) = make_model([0.0; 3], end);
                let mut section = shape.to_section(SectionId(0), "直接入力保護".into());
                let supplied_area = section.area;
                section.property_basis.area = PropertyBasis::Supplied;
                section.material = Some(MaterialId(0));
                section.frame_use = Some(FrameSectionUse::Brace);
                model.sections[0] = section.with_surface_radius(Some(radius)).unwrap();
                assert_eq!(model.sections[0].area, supplied_area);
                let truss = TrussElement::try_new(&data, &model).unwrap();
                let common = model.element_mass_properties(&data).unwrap();
                let expected = 7.85e-9 * expected_area * 3000.0;
                assert_eq!(truss.a, supplied_area);
                assert_eq!(truss.mass_per_length, common.mass_per_length);
                approx::assert_relative_eq!(
                    truss.local_stiffness().get(0, 0),
                    205000.0 * supplied_area / 3000.0,
                    max_relative = 1.0e-12
                );
                assert_total_mass(&truss, expected);
                let linear = crate::factory::build_behavior_with_axial_factor(&data, &model, 0.75);
                assert_total_mass(linear.as_ref(), expected);
                let nonlinear = crate::factory::build_nonlinear_behavior(
                    &data,
                    &model,
                    crate::factory::StrengthBasis::Nominal,
                    crate::factory::AnalysisKind::Incremental,
                );
                assert_total_mass(nonlinear.as_ref(), expected);
                eprintln!("{shape:?}, 編集寸法={radius}, 保護A={supplied_area}, 共通質量={expected:.15e} t");
            }
        }
    }

    #[test]
    fn catalog_properties_are_protected_but_brace_mass_follows_fillet_and_corner_r() {
        use sepika_core::model::FrameSectionUse;
        use sepika_core::section_shape::SectionShape;
        use sepika_section::catalog::{entries, to_section, CatalogShape};
        for kind in [CatalogShape::H, CatalogShape::Box] {
            let entry = entries().iter().find(|entry| entry.shape == kind).unwrap();
            let mut section = to_section(entry, SectionId(0));
            section.material = Some(MaterialId(0));
            section.frame_use = Some(FrameSectionUse::Brace);
            let radius = match section.shape.as_ref().unwrap() {
                SectionShape::SteelH {
                    height,
                    width,
                    web_thick,
                    flange_thick,
                    ..
                } => ((height - 2.0 * flange_thick) / 4.0).min((width - web_thick) / 4.0),
                SectionShape::SteelBox {
                    height,
                    width,
                    thick,
                    ..
                } => thick.min(height.min(*width) / 4.0),
                _ => panic!("H または Box のカタログ形状が必要"),
            };
            let section = section.with_surface_radius(Some(radius)).unwrap();
            assert_eq!(
                (section.area, section.iy, section.iz, section.j),
                (entry.area, entry.iy, entry.iz, entry.j)
            );
            let expected_area = match section.shape.as_ref().unwrap() {
                SectionShape::SteelH {
                    height,
                    width,
                    web_thick,
                    flange_thick,
                    ..
                } => {
                    2.0 * width * flange_thick
                        + (height - 2.0 * flange_thick) * web_thick
                        + (4.0 - std::f64::consts::PI) * radius * radius
                }
                SectionShape::SteelBox {
                    height,
                    width,
                    thick,
                    ..
                } => {
                    height * width
                        - (height - 2.0 * thick) * (width - 2.0 * thick)
                        - (4.0 - std::f64::consts::PI)
                            * (radius.powi(2) - (radius - thick).max(0.0).powi(2))
                }
                _ => unreachable!(),
            };
            let (mut model, data) = make_model([0.0; 3], [3000.0, 0.0, 0.0]);
            model.sections[0] = section;
            let truss = TrussElement::try_new(&data, &model).unwrap();
            assert_eq!(truss.a, entry.area);
            assert_total_mass(&truss, 7.85e-9 * expected_area * 3000.0);
        }
    }

    #[test]
    fn unknown_and_invalid_geometry_is_a_checked_constructor_error() {
        use sepika_core::model::PropertyBasis;
        use sepika_core::section_shape::SectionShape;
        for shape in [
            SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 9.0,
                flange_thick: 12.0,
                root_r: None,
            },
            SectionShape::SteelBox {
                height: 500.0,
                width: 300.0,
                thick: 10.0,
                corner_r: None,
            },
        ] {
            let (mut model, data) = make_model([0.0; 3], [3000.0, 0.0, 0.0]);
            let mut section = shape
                .input_section(SectionId(0), "未知寸法".into())
                .unwrap();
            section.material = Some(MaterialId(0));
            model.sections[0] = section;
            assert!(TrussElement::try_new(&data, &model)
                .err()
                .unwrap()
                .contains("未算定"));
            model.sections[0].area = 8184.0;
            model.sections[0].property_basis.area = PropertyBasis::Supplied;
            assert!(TrussElement::try_new(&data, &model)
                .err()
                .unwrap()
                .contains("未知"));
            model.sections[0].property_basis = Default::default();
            let table_only = TrussElement::try_new(&data, &model).unwrap();
            assert_total_mass(&table_only, 7.85e-9 * 8184.0 * 3000.0);
        }
        for r in [Some(-1.0), Some(f64::NAN), Some(f64::INFINITY), Some(151.0)] {
            let (mut model, data) = make_model([0.0; 3], [3000.0, 0.0, 0.0]);
            model.sections[0].shape = Some(SectionShape::SteelBox {
                height: 500.0,
                width: 300.0,
                thick: 10.0,
                corner_r: r,
            });
            assert!(TrussElement::try_new(&data, &model)
                .err()
                .unwrap()
                .contains("不正"));
        }
    }

    #[test]
    fn shapeless_direct_properties_keep_both_brace_mass_modes() {
        let (model, data) = make_model([0.0; 3], [1000.0, 2000.0, 2000.0]);
        let truss = TrussElement::try_new(&data, &model).unwrap();
        assert_eq!(truss.a, 2000.0);
        assert_total_mass(&truss, 0.0471);
    }

    #[test]
    fn test_axial_local_stiffness_matches_ea_over_l() {
        let (model, data) = make_model([0.0, 0.0, 0.0], [4000.0, 0.0, 0.0]);
        let truss = TrussElement::new(&data, &model);
        let k = truss.local_stiffness();
        let ea_l = truss.e * truss.a / truss.length;
        assert!((k.get(0, 0) - ea_l).abs() < 1e-9);
        assert!((k.get(6, 6) - ea_l).abs() < 1e-9);
        assert!((k.get(0, 6) + ea_l).abs() < 1e-9);
        assert!((k.get(6, 0) + ea_l).abs() < 1e-9);
    }

    /// 斜め配置でも全体系剛性が軸方向ベクトル t による t·tᵀ 展開に一致すること
    /// （K_global = k·(t·tᵀ) をブロックごとに検証。t = 部材軸方向単位ベクトル）。
    #[test]
    fn test_global_stiffness_matches_t_tt_projection() {
        let (model, data) = make_model([0.0, 0.0, 0.0], [3000.0, 0.0, 4000.0]);
        let truss = TrussElement::new(&data, &model);
        let ctx = Ctx { model: &model };
        let k_global = truss.tangent_stiffness(&ctx);

        let l = truss.length;
        let t = [3000.0 / l, 0.0, 4000.0 / l];
        let k = truss.e * truss.a / l;

        for i in 0..3 {
            for j in 0..3 {
                let expected = k * t[i] * t[j];
                assert!(
                    (k_global.get(i, j) - expected).abs() < 1e-6,
                    "K[{i}][{j}]: {} vs {}",
                    k_global.get(i, j),
                    expected
                );
                assert!((k_global.get(i + 6, j + 6) - expected).abs() < 1e-6);
                assert!((k_global.get(i, j + 6) + expected).abs() < 1e-6);
            }
        }
        for i in 3..6 {
            for j in 0..12 {
                assert_eq!(k_global.get(i, j), 0.0);
                assert_eq!(k_global.get(j, i), 0.0);
            }
        }
    }

    #[test]
    fn test_stiffness_matrix_symmetric() {
        let (model, data) = make_model([1000.0, 500.0, 0.0], [5000.0, 2500.0, 3000.0]);
        let truss = TrussElement::new(&data, &model);
        let ctx = Ctx { model: &model };
        let k = truss.tangent_stiffness(&ctx);
        for i in 0..12 {
            for j in 0..12 {
                assert!(
                    (k.get(i, j) - k.get(j, i)).abs() < 1e-9,
                    "K[{i}][{j}] != K[{j}][{i}]"
                );
            }
        }
    }

    /// 剛体移動（両節点を同一量だけ並進）を与えると内力がゼロになること。
    #[test]
    fn test_rigid_body_translation_gives_zero_force() {
        let (model, data) = make_model([0.0, 0.0, 0.0], [3000.0, 4000.0, 0.0]);
        let mut truss = TrussElement::new(&data, &model);
        let du = LocalVec {
            data: SmallVec::from_vec(vec![
                1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.0, 0.0, 0.0,
            ]),
        };
        let ctx = Ctx { model: &model };
        truss.update_state(&du, true, &ctx);
        let f = truss.internal_force(&ctx);
        for i in 0..12 {
            assert!(f.data[i].abs() < 1e-6, "f[{i}]={}", f.data[i]);
        }
    }

    /// j端の軸方向変位を与えると軸力が EA/L×変位 となること（trial_disp 経路）。
    #[test]
    fn test_axial_force_matches_ea_over_l() {
        let (model, data) = make_model([0.0, 0.0, 0.0], [4000.0, 0.0, 0.0]);
        let mut truss = TrussElement::new(&data, &model);
        let ctx = Ctx { model: &model };
        let ea_l = truss.e * truss.a / truss.length;

        let du = LocalVec {
            data: SmallVec::from_vec(vec![
                0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ]),
        };
        truss.update_state(&du, true, &ctx);
        let f = truss.internal_force(&ctx);
        assert!((f.data[6] - ea_l).abs() < 1e-6, "f[6]={}", f.data[6]);
        assert!((f.data[0] + ea_l).abs() < 1e-6, "f[0]={}", f.data[0]);
    }
}
