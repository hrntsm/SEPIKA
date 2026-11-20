use approx::assert_relative_eq;
use sepika_core::dof::{Dof, Dof6Mask};
use sepika_core::ids::{ElemId, LoadCaseId, MaterialId, NodeId, SectionId, StoryId};
use sepika_core::model::{
    Constraint, ElementData, ElementKind, EndCondition, ForceRegime, FrameSectionUse, LoadCase,
    LoadCaseKind, LocalAxis, Material, MaterialCategory, Model, NodalLoad, Node, RigidZone,
    Section, Story,
};
use sepika_design_jp::secondary::eccentricity::{
    center_of_rigidity, eccentricity, ColumnStiffness,
};
use sepika_design_jp::secondary::eccentricity_analysis::{
    column_horizontal_stiffnesses, column_stiffnesses_from_analysis,
    story_eccentricity_from_analysis,
};
use sepika_design_jp::secondary::holding_capacity::fes;
use sepika_element::behavior::ElementBehavior;
use sepika_element::frame::beam::BeamElement;
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::{linear_static_once, StaticOnce};
use smallvec::smallvec;

const TOP: StoryId = StoryId(1);

fn column_model(heads_mm: &[[f64; 2]], first_dx_mm: f64, first_dy_mm: f64) -> Model {
    let mut model = Model {
        stories: vec![
            Story::new(StoryId(0), "基礎".into(), Some(0.0), Some(0.0)),
            Story::new(TOP, "上階".into(), Some(3000.0), Some(0.0)),
        ],
        materials: vec![Material {
            id: MaterialId(0),
            name: "鋼".into(),
            category: MaterialCategory::Steel,
            young: 200_000.0,
            poisson: 0.25,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
            strength_factor: None,
            concrete_class: Default::default(),
        }],
        ..Default::default()
    };
    let mut section = Section::zero(SectionId(0), "柱断面".into());
    section.frame_use = Some(FrameSectionUse::Column);
    section.material = Some(MaterialId(0));
    section.area = 2_000.0;
    section.as_y = 1_600.0;
    section.as_z = 1_600.0;
    section.iy = 8_000_000.0;
    section.iz = 8_000_000.0;
    section.j = 4_000_000.0;
    model.sections.push(section);
    for (index, &[x, y]) in heads_mm.iter().enumerate() {
        let bottom = NodeId((2 * index) as u32);
        let top = NodeId(bottom.0 + 1);
        let base_xy = if index == 0 {
            [x - first_dx_mm, y - first_dy_mm]
        } else {
            [x, y]
        };
        for (id, coord, restraint, story) in [
            (
                bottom,
                [base_xy[0], base_xy[1], 0.0],
                Dof6Mask::FIXED,
                StoryId(0),
            ),
            (top, [x, y, 3000.0], Dof6Mask::FREE, TOP),
        ] {
            model.nodes.push(Node {
                id,
                coord,
                restraint,
                mass: None,
                story: Some(story),
                support_spring: None,
            });
            model.stories[story.index()].node_ids.push(id);
        }
        model.elements.push(ElementData {
            id: ElemId(index as u32),
            kind: ElementKind::Beam,
            nodes: smallvec![bottom, top],
            section: Some(SectionId(0)),
            local_axis: LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: RigidZone::default(),
            end_offsets_mm: [[0.0; 3]; 2],
            plastic_zone: None,
            spring: None,
        });
    }
    model
}

fn add_case(model: &mut Model, name: &str, values: [f64; 6]) -> LoadCaseId {
    let id = LoadCaseId(model.load_cases.len() as u32);
    model.load_cases.push(LoadCase {
        origin: Default::default(),
        id,
        name: name.into(),
        kind: LoadCaseKind::Seismic,
        nodal: vec![NodalLoad::manual(NodeId(1), values)],
        member: Vec::new(),
    });
    id
}

fn element_nodal_residual(model: &Model, element: &ElementData, result: &StaticOnce) -> [f64; 12] {
    let mut disp = [0.0; 12];
    for (dofs, node) in disp.as_chunks_mut::<6>().0.iter_mut().zip(&element.nodes) {
        dofs.copy_from_slice(&result.disp[node.index()]);
    }
    BeamElement::try_new(element, model)
        .unwrap()
        .frame_load_response(&disp, &[])
        .unwrap()
        .unwrap()
        .nodal_residual
        .data
        .as_slice()
        .try_into()
        .unwrap()
}

#[test]
fn inclined_ordinary_beam_reactions_and_signed_section_forces_balance_the_applied_load() {
    let component_45 = 1000.0 * std::f64::consts::FRAC_1_SQRT_2;
    for (dx_mm, py_n, pz_n, expected_section) in [
        (0.0, 700.0, -500.0, [-500.0, -700.0, 1000.0]),
        (4000.0, 700.0, -500.0, [500.0, -700.0, 1000.0]),
        (-4000.0, 700.0, -500.0, [-1100.0, -700.0, 200.0]),
        (3000.0, 0.0, 0.0, [component_45, 0.0, component_45]),
    ] {
        for reversed in [false, true] {
            let mut model = column_model(&[[0.0, 0.0]], dx_mm, 0.0);
            if reversed {
                model.elements[0].nodes.swap(0, 1);
            }
            let case = add_case(&mut model, "先端荷重", [1000.0, py_n, pz_n, 0.0, 0.0, 0.0]);
            let response = linear_static_once(&model, case).unwrap();
            let residual = element_nodal_residual(&model, &model.elements[0], &response);
            let fixed = if reversed { 6 } else { 0 };
            for (actual, expected) in residual[fixed..fixed + 6].iter().zip([
                -1000.0,
                -py_n,
                -pz_n,
                3000.0 * py_n,
                -3_000_000.0 + pz_n * dx_mm,
                -py_n * dx_mm,
            ]) {
                assert_relative_eq!(*actual, expected, epsilon = 1e-6);
            }
            let story_shears = response.story_shears.as_ref().unwrap();
            assert_relative_eq!(story_shears[0][0], 1000.0, epsilon = 1e-8);
            assert_relative_eq!(story_shears[1][0], py_n, epsilon = 1e-8);

            let [n, qy, qz] = expected_section;
            let expected = if reversed { [n, -qy, qz] } else { [n, qy, qz] };
            for (_, section) in &response.member_forces[0].1.at {
                for (actual, expected) in section.iter().zip(expected) {
                    assert_relative_eq!(*actual, expected, epsilon = 1e-8);
                }
            }
            let kx =
                column_horizontal_stiffnesses(&model, TOP, &response, SeismicDir::X).unwrap()[0];
            let horizontal_n = kx * response.disp[1][0].abs();
            if dx_mm == 0.0 {
                assert_relative_eq!(horizontal_n, 1000.0, epsilon = 1e-8);
            }
            eprintln!("柱頭のXずれ={dx_mm} mm、i/j反転={reversed}、偏心率に使う水平力の大きさ={horizontal_n:.9} N");
        }
    }
}

#[test]
fn inclined_ordinary_column_eccentricity_is_compared_with_the_rigid_floor_response() {
    let mut model = column_model(&[[0.0, 0.0], [6000.0, 0.0], [0.0, 6000.0]], 2400.0, 3200.0);
    for head in model.nodes.iter_mut().skip(1).step_by(2) {
        for dof in [Dof::Uz, Dof::Rx, Dof::Ry] {
            head.restraint.set_fixed(dof);
        }
    }
    model.constraints.push(Constraint::rigid_diaphragm(
        TOP,
        NodeId(1),
        vec![NodeId(3), NodeId(5)],
    ));
    let x = add_case(
        &mut model,
        "重心X",
        [1000.0, 0.0, 0.0, 0.0, 0.0, -2_000_000.0],
    );
    let y = add_case(
        &mut model,
        "重心Y",
        [0.0, 1000.0, 0.0, 0.0, 0.0, 2_000_000.0],
    );
    let torsion = add_case(
        &mut model,
        "純ねじり",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1_000_000.0],
    );
    let [res_x, res_y, res_twist] =
        [x, y, torsion].map(|case| linear_static_once(&model, case).unwrap());
    for (result, expected_xy) in [
        (&res_x, [1000.0, 0.0]),
        (&res_y, [0.0, 1000.0]),
        (&res_twist, [0.0, 0.0]),
    ] {
        for (axis, expected) in expected_xy.into_iter().enumerate() {
            assert_relative_eq!(
                result.story_shears.as_ref().unwrap()[axis][0],
                expected,
                epsilon = 1e-8
            );
        }
    }
    let current = story_eccentricity_from_analysis(&model, TOP, &res_x, &res_y, None).unwrap();
    let current_columns = column_stiffnesses_from_analysis(&model, TOP, &res_x, &res_y).unwrap();
    let current_center = center_of_rigidity(&current_columns).unwrap();
    let center_of_mass = [2000.0, 2000.0];
    let with_nodal_forces: Vec<_> = model
        .elements
        .iter()
        .zip(&current_columns)
        .map(|(element, col)| {
            let head = element.nodes[1].index();
            let fx = element_nodal_residual(&model, element, &res_x)[6];
            let fy = element_nodal_residual(&model, element, &res_y)[7];
            ColumnStiffness {
                pos: col.pos,
                dx: fx.abs() / res_x.disp[head][0].abs(),
                dy: fy.abs() / res_y.disp[head][1].abs(),
            }
        })
        .collect();
    let nodal_center = center_of_rigidity(&with_nodal_forces).unwrap();
    let nodal_ecc = eccentricity(&with_nodal_forces, center_of_mass, nodal_center).unwrap();

    let [ux, uy, _, _, _, rotation] = res_twist.disp[1];
    let floor_center = [-uy / rotation, ux / rotation];
    let floor_kr = 1_000_000.0 / rotation;
    let [[cxx, cyx], [cxy, cyy]] = [&res_x, &res_y].map(|result| {
        let [u, v, _, _, _, r] = result.disp[1];
        [
            (u - ux * (r / rotation)) / 1000.0,
            (v - uy * (r / rotation)) / 1000.0,
        ]
    });
    assert_relative_eq!(cxy, cyx, epsilon = 1e-14);
    let determinant = cxx * cyy - cxy * cyx;
    let [kxx, kxy, kyy] = [cyy / determinant, -cxy / determinant, cxx / determinant];
    // 柱先端の力・モーメントによるたわみと回転の柔度を先に求め、剛床へ合成した独立値。
    for (actual, expected) in [kxx, kxy, kyy, floor_center[0], floor_center[1], floor_kr]
        .into_iter()
        .zip([
            19_948.412_630_664_1,
            24_529.095_506_958_25,
            34_257.051_676_389_75,
            2_166.687_027_709_256,
            2_862.348_055_979_125,
            31_403_342_934.896_14,
        ])
    {
        assert_relative_eq!(actual, expected, max_relative = 1e-10);
    }
    let floor_re = [
        (center_of_mass[1] - floor_center[1]).abs() / (floor_kr / kxx).sqrt(),
        (center_of_mass[0] - floor_center[0]).abs() / (floor_kr / kyy).sqrt(),
    ];
    let current_re = [current.re_x.unwrap(), current.re_y.unwrap()];
    let nodal_re = [nodal_ecc.re_x.unwrap(), nodal_ecc.re_y.unwrap()];
    eprintln!(
        "X/Y加力時の節点変位差 [mm] = {:?} / {:?}",
        [1, 3, 5].map(|i| res_x.disp[i][0]),
        [1, 3, 5].map(|i| res_y.disp[i][1])
    );
    eprintln!(
        "方向別柱剛性 [N/mm] = {current_columns:?} / {with_nodal_forces:?}、剛床Kxx/Kxy/Kyy = {kxx}/{kxy}/{kyy}"
    );
    for (name, center, kr, re) in [
        ("現行公開Re", current_center, current.kr, current_re),
        ("物理節点力で置換", nodal_center, nodal_ecc.kr, nodal_re),
        ("剛床応答で比較", floor_center, floor_kr, floor_re),
    ] {
        eprintln!(
            "{name} の剛心={center:?}、KR={kr}、Re={re:?}、Fes={:?}",
            re.map(|r| fes(1.0, r).unwrap())
        );
    }
}
