use super::*;
use crate::{CheckKind, LoadTerm, SteelFbBasis};
use sepika_core::ids::SectionId;

fn supplied_section(category: ShapeCategory) -> Section {
    let (shape, width_mm) = match category {
        ShapeCategory::H => (
            Some(SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(13.0),
            }),
            200.0,
        ),
        ShapeCategory::Box => (
            Some(SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                horizontal_thick: 12.0,
                corner_r: Some(30.0),
            }),
            300.0,
        ),
        ShapeCategory::Other => (None, 250.0),
        ShapeCategory::Pipe => unreachable!(),
    };
    Section {
        area: 10_000.0,
        iy: 200_000_000.0,
        iz: width_mm * 500_000.0,
        depth: 400.0,
        width: width_mm,
        as_y: 4000.0,
        as_z: 6000.0,
        shape,
        ..Section::zero(SectionId(0), "供給性能せん断受入".into())
    }
}

#[test]
fn directional_shear_sum_and_mises_use_independent_ratios_without_repeating_normal_stress() {
    // 供給 A=10000、両軸 Z=1000000。N100kN・Mz20kNm・My30kNmで応力度10/20/30。
    // H柱の既存法線応力度は28.7、梁とBox/Other柱は60。fs=235/(1.5√3)。
    let oracle = [
        (
            ShapeCategory::H,
            100_000.0,
            0.0,
            0.36950679988527074,
            0.5321728833206429,
            0.41242501977451873,
        ),
        (
            ShapeCategory::H,
            0.0,
            80_000.0,
            0.25513023352078384,
            0.46017837697649683,
            0.3140868635121952,
        ),
        (
            ShapeCategory::H,
            100_000.0,
            80_000.0,
            0.6246370334060547,
            0.732696476095436,
            0.6509458850602524,
        ),
        (
            ShapeCategory::H,
            0.0,
            0.0,
            0.0,
            0.3829787234042553,
            0.18319148936170213,
        ),
        (
            ShapeCategory::Box,
            100_000.0,
            0.0,
            0.1354858266245993,
            0.40623775279570473,
            0.40623775279570473,
        ),
        (
            ShapeCategory::Box,
            0.0,
            80_000.0,
            0.1535506035078792,
            0.41261421499748047,
            0.41261421499748047,
        ),
        (
            ShapeCategory::Box,
            100_000.0,
            80_000.0,
            0.2890364301324785,
            0.47980700341291416,
            0.47980700341291416,
        ),
        (
            ShapeCategory::Box,
            0.0,
            0.0,
            0.0,
            0.3829787234042553,
            0.3829787234042553,
        ),
        (
            ShapeCategory::Other,
            100_000.0,
            0.0,
            0.18426072420945502,
            0.42499966713698256,
            0.42499966713698256,
        ),
        (
            ShapeCategory::Other,
            0.0,
            80_000.0,
            0.22111286905134603,
            0.44222573810269206,
            0.44222573810269206,
        ),
        (
            ShapeCategory::Other,
            100_000.0,
            80_000.0,
            0.4053735932608011,
            0.5576741456204748,
            0.5576741456204748,
        ),
        (
            ShapeCategory::Other,
            0.0,
            0.0,
            0.0,
            0.3829787234042553,
            0.3829787234042553,
        ),
    ];
    let mut material = test_support::mat("独立比較用鋼材");
    material.fy = Some(235.0);
    for (category, qy_n, qz_n, pure_shear, beam_nm, column_nm) in oracle {
        let section = supplied_section(category);
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let ctx = DesignCtx {
                kind,
                term: LoadTerm::Long,
                length: 0.0,
                steel_fb_basis: SteelFbBasis::Standard1973,
                ..Default::default()
            };
            for normal_stress in [false, true] {
                for (sign_y, sign_z) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    let forces = MemberForcesAt {
                        pos: 0.5,
                        n: if normal_stress {
                            sign_y * 100_000.0
                        } else {
                            0.0
                        },
                        qy: sign_y * qy_n,
                        qz: sign_z * qz_n,
                        my: if normal_stress {
                            sign_z * 30_000_000.0
                        } else {
                            0.0
                        },
                        mz: if normal_stress {
                            sign_y * 20_000_000.0
                        } else {
                            0.0
                        },
                    };
                    let result = SteelDesign
                        .check(&forces, &section, &material, &ctx)
                        .unwrap_checked();
                    let component = |kind| {
                        result
                            .components
                            .iter()
                            .find(|c| c.kind == kind)
                            .unwrap()
                            .ratio
                    };
                    let expected_shear = if normal_stress && kind == MemberKind::Column {
                        column_nm
                    } else {
                        pure_shear
                    };
                    assert!(
                        (component(CheckKind::Shear) - expected_shear).abs() < 1e-10,
                        "{category:?} {kind:?} Qy={qy_n} Qz={qz_n} NM={normal_stress}: {result:?}"
                    );
                    let (normal_kind, expected_normal) = if kind == MemberKind::Girder {
                        (
                            CheckKind::Bending,
                            if normal_stress { beam_nm } else { pure_shear },
                        )
                    } else {
                        (
                            CheckKind::AxialBending,
                            if normal_stress {
                                0.3829787234042553
                            } else {
                                0.0
                            },
                        )
                    };
                    assert!(
                        (component(normal_kind) - expected_normal).abs() < 1e-10,
                        "{category:?} {kind:?} Qy={qy_n} Qz={qz_n} NM={normal_stress}: {result:?}"
                    );
                    assert!((result.ratio() - expected_normal.max(expected_shear)).abs() < 1e-10);
                }
            }
        }
    }
}
