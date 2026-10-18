//! 鋼構造の許容応力度と断面検定（許容応力度検定。
//! 根拠規準は鋼構造設計規準 1973・構造規定）。
//! 形状は `Section.shape` 優先、なければ `Section.name` から推定し、合わなければ `Other` とする。

use crate::{CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, MemberKind};
use sepika_core::model::{Material, Section};
use sepika_core::section_shape::SectionShape;

pub use crate::material_strength::{
    big_lambda, plate_thickness, steel_f_value, steel_f_value_prefix, steel_fc, steel_fs, steel_ft,
};

mod beam;
/// 鉄骨造梁の非線形復元力特性（全塑性 Mp・横座屈 Mcr・軸 Nu）。
/// 非線形解析の材端バネ骨格に用いる（鋼構造塑性設計指針）。
pub mod beam_nonlinear;
mod brace;
/// 鉄骨造柱の座屈長さ係数 K（鋼構造塑性設計指針、水平移動非拘束）。
pub mod buckling;
pub mod cold_formed;
mod column;
pub mod lateral_bracing;
mod lip_channel;
pub mod panel_zone;
mod section;
#[cfg(test)]
mod shear_contract_tests;
#[cfg(test)]
mod torsion_contract_tests;

pub use section::{resolve_lb, steel_fb_h, steel_fb_h_asd2019, steel_h_z_with_loss, steel_i_t};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeCategory {
    H,
    Box,
    Pipe,
    Other,
}

/// `Section.name` の先頭アルファベットトークンから形状カテゴリを推定する。
/// 例: "H-300x300x10x15"→H、"BOX-200x200x12"→Box、"PIPE-216.3x8.2"→Pipe。
/// 該当しない場合は `Other`（一般断面フォールバック）。
fn classify_shape(name: &str) -> ShapeCategory {
    let token: String = name
        .trim()
        .to_uppercase()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    match token.as_str() {
        "H" => ShapeCategory::H,
        "BOX" => ShapeCategory::Box,
        "PIPE" | "P" => ShapeCategory::Pipe,
        _ => ShapeCategory::Other,
    }
}

/// 形状カテゴリと板厚 `(カテゴリ, tf, tw)` を解決する。
/// `Section.shape` があれば実寸、なければ `Section.thickness` を `tf ≈ tw` とする。
fn shape_of(sec: &Section) -> (ShapeCategory, f64, f64) {
    if let Some(shape) = &sec.shape {
        match *shape {
            SectionShape::SteelH {
                web_thick,
                flange_thick,
                ..
            } => return (ShapeCategory::H, flange_thick, web_thick),
            SectionShape::SteelBuiltH {
                web_thick,
                upper_thick,
                lower_thick,
                ..
            } => return (ShapeCategory::H, upper_thick.min(lower_thick), web_thick),
            SectionShape::SteelBox { thick, .. } => return (ShapeCategory::Box, thick, thick),
            SectionShape::SteelPipe { thick, .. } => return (ShapeCategory::Pipe, thick, thick),
            SectionShape::SteelChannel {
                web_thick,
                flange_thick,
                ..
            }
            | SectionShape::SteelTee {
                web_thick,
                flange_thick,
                ..
            } => return (ShapeCategory::Other, flange_thick, web_thick),
            SectionShape::SteelAngle { thick, .. } => return (ShapeCategory::Other, thick, thick),
            SectionShape::SteelFlatBar { thick, .. } => {
                return (ShapeCategory::Other, thick, thick)
            }
            SectionShape::SteelRoundBar { dia } => return (ShapeCategory::Other, dia, dia),
            SectionShape::SteelLipChannel { thick, .. } => {
                return (ShapeCategory::Other, thick, thick)
            }
            SectionShape::CftBox { thick, .. } => return (ShapeCategory::Box, thick, thick),
            SectionShape::CftPipe { thick, .. } => return (ShapeCategory::Pipe, thick, thick),
            SectionShape::RcBeamRect { .. }
            | SectionShape::RcColumnRect { .. }
            | SectionShape::RcColumnCircle { .. }
            | SectionShape::SrcBeamRect { .. }
            | SectionShape::SrcColumnRect { .. }
            | SectionShape::RcWall { .. }
            | SectionShape::RcSlab { .. } => return (ShapeCategory::Other, 0.0, 0.0),
        }
    }
    let t = sec.thickness.unwrap_or(0.0);
    (classify_shape(&sec.name), t, t)
}

/// せん断有効断面積 `(Ay, Az)` [mm²]（強軸 Qy・弱軸 Qz。梁・柱で共用の単一定義）。
/// Box の寸法と外角半径は検定入口で適用可能と確認済みであること。
fn shear_area_2d(shape: ShapeCategory, sec: &Section, tf: f64, tw: f64) -> (f64, f64) {
    let h = sec.depth;
    let b = sec.width;
    match shape {
        ShapeCategory::H => {
            let ay = (tw * (h - 2.0 * tf).max(0.0)).max(0.0);
            let az = (2.0 * b * tf / 1.5).max(0.0);
            (ay, az)
        }
        ShapeCategory::Box => {
            let t = tw;
            let r = match &sec.shape {
                Some(SectionShape::SteelBox { corner_r, .. }) => {
                    corner_r.expect("角形鋼管のせん断検定入口で外角半径を確認済み")
                }
                _ => 0.0,
            };
            let end = if r == 0.0 { t } else { r };
            (2.0 * t * (h - 2.0 * end), 2.0 * t * (b - 2.0 * end))
        }
        ShapeCategory::Pipe => {
            let t = tw;
            let d = sec.depth;
            let a = (std::f64::consts::PI * t * (d - t) / 2.0).max(0.0);
            (a, a)
        }
        ShapeCategory::Other => {
            let ay = if sec.as_z > 0.0 { sec.as_z } else { sec.area };
            let az = if sec.as_y > 0.0 { sec.as_y } else { sec.area };
            (ay, az)
        }
    }
}

/// 分母が極小の場合に安全側デフォルトへ逃がすヘルパー。
fn safe_denom(x: f64) -> f64 {
    if x.abs() > 1e-9 {
        x
    } else {
        1e-9
    }
}

/// 断面係数 Z = I / (半せい)。半せいが極小なら 0（呼び出し側で 1.0 にフォールバック）。
fn section_modulus(i: f64, half_dim: f64) -> f64 {
    if half_dim > 1e-9 {
        i / half_dim
    } else {
        0.0
    }
}

fn nonzero(z: f64) -> f64 {
    if z.abs() > 1e-9 {
        z
    } else {
        1.0
    }
}

pub struct SteelDesign;

impl DesignCheck for SteelDesign {
    fn check(
        &self,
        forces: &MemberForcesAt,
        sec: &Section,
        mat: &Material,
        ctx: &DesignCtx,
    ) -> CheckOutcome {
        if matches!(sec.shape, Some(SectionShape::SteelLipChannel { .. })) {
            return CheckOutcome::Skipped {
                reason: lip_channel::unsupported_reason(forces, sec, ctx),
            };
        }
        if matches!(ctx.kind, MemberKind::Girder | MemberKind::Column) {
            if sec.shape.is_none() && shape_of(sec).0 == ShapeCategory::Box {
                return CheckOutcome::Skipped {
                    reason: format!(
                        "鋼せん断検定: 断面 {} の角形鋼管寸法と外角半径が未確定です。断面形状を入力してください",
                        sec.name
                    ),
                };
            }
            if let Some(shape @ SectionShape::SteelBox { corner_r, .. }) = sec.shape.as_ref() {
                if let Err(error) = shape.validate_surface_radius() {
                    return CheckOutcome::Skipped {
                        reason: format!(
                            "鋼せん断検定: 断面 {} の角形鋼管外角半径または適用寸法が不正です。{error}",
                            sec.name
                        ),
                    };
                }
                let Some(radius) = corner_r else {
                    return CheckOutcome::Skipped {
                        reason: format!(
                            "鋼せん断検定: 断面 {} の角形鋼管外角半径が未知です。半径を入力してください（0は溶接箱）",
                            sec.name
                        ),
                    };
                };
                let (_, _, thick) = shape_of(sec);
                let end = if *radius == 0.0 { thick } else { *radius };
                if ![sec.depth, sec.width, thick]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.0)
                    || (*radius > 0.0 && *radius < thick)
                    || sec.depth - 2.0 * end <= 0.0
                    || sec.width - 2.0 * end <= 0.0
                    || [
                        2.0 * thick * (sec.depth - 2.0 * end),
                        2.0 * thick * (sec.width - 2.0 * end),
                    ]
                    .iter()
                    .any(|area_mm2| !area_mm2.is_finite() || *area_mm2 <= 0.0)
                {
                    return CheckOutcome::Skipped {
                        reason: format!(
                            "鋼せん断検定: 断面 {} の外角半径または寸法が適用範囲外です。溶接箱は外角半径0、角形鋼管は外角半径が板厚以上、両方向の直線長が正で面積が有限正である必要があります",
                            sec.name
                        ),
                    };
                }
                let (ay, az) = shear_area_2d(ShapeCategory::Box, sec, thick, thick);
                if ![forces.qy / ay, forces.qz / az]
                    .iter()
                    .all(|stress_mpa| stress_mpa.is_finite())
                {
                    return CheckOutcome::Skipped {
                        reason: format!(
                            "鋼せん断検定: 断面 {} のせん断応力度が非有限です。せん断力・断面寸法の桁と単位を確認してください",
                            sec.name
                        ),
                    };
                }
            }
        }
        let t = plate_thickness(sec);
        if let Some(reason) = sepika_core::material_grade::steel_f_value_issue(&mat.name, t) {
            return CheckOutcome::Skipped { reason };
        }
        let f = steel_f_value_prefix(&mat.name, t)
            .or(mat.fy)
            .unwrap_or(235.0);
        let term = ctx.term;

        let cr = match ctx.kind {
            MemberKind::Girder => beam::check_beam(forces, sec, mat, ctx, f, term),
            MemberKind::Column => column::check_column(forces, sec, mat, ctx, f, term),
            MemberKind::Brace => Ok(brace::check_brace(forces, sec, mat, ctx, f, term)),
        };
        match cr {
            Ok(result) => CheckOutcome::Checked(result),
            Err(reason) => CheckOutcome::Skipped {
                reason: format!("鋼横座屈検定: {reason}"),
            },
        }
    }
}

#[cfg(test)]
mod surface_radius_tests {
    use super::*;
    use crate::LoadTerm;
    use sepika_core::ids::SectionId;

    fn supplied_box_section(radius: Option<f64>, name: &str) -> Section {
        Section {
            area: 400.0 * 300.0 - 376.0 * 276.0,
            iy: (300.0 * 400.0_f64.powi(3) - 276.0 * 376.0_f64.powi(3)) / 12.0,
            iz: (400.0 * 300.0_f64.powi(3) - 376.0 * 276.0_f64.powi(3)) / 12.0,
            depth: 400.0,
            width: 300.0,
            shape: Some(SectionShape::SteelBox {
                height: 400.0,
                width: 300.0,
                thick: 12.0,
                corner_r: radius,
            }),
            ..Section::zero(SectionId(0), name.into())
        }
    }

    #[test]
    fn box_shear_requires_known_radius_for_girders_and_columns() {
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 150_000.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let material = test_support::mat("SN400");
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let ctx = DesignCtx {
                kind,
                term: LoadTerm::Long,
                length: 0.0,
                ..Default::default()
            };
            for (radius, area_mm2) in [
                (0.0, 9024.0),
                (12.0, 9024.0),
                (30.0, 8160.0),
                (60.0, 6720.0),
                (149.0, 2448.0),
            ] {
                let section = supplied_box_section(Some(radius), "BOX半径検証");
                let outcome = SteelDesign.check(&forces, &section, &material, &ctx);
                let expected = forces.qy / area_mm2 / steel_fs(235.0, LoadTerm::Long);
                assert!((outcome.unwrap_checked().ratio() / expected - 1.0).abs() < 1e-10);
            }
            let section = supplied_box_section(None, "BOX未知半径");
            assert!(
                matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("外角半径が未知"))
            );
        }
    }

    #[test]
    fn box_shear_rejects_invalid_radius_for_girders_and_columns() {
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 150_000.0,
            qz: 150_000.0,
            my: 0.0,
            mz: 0.0,
        };
        let material = test_support::mat("SN400");
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let ctx = DesignCtx {
                kind,
                ..Default::default()
            };
            for r in [
                -1.0,
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                0.001,
                11.999,
                150.0,
                150.1,
                300.0,
            ] {
                let section = supplied_box_section(Some(r), "BOX不正半径検証");
                match SteelDesign.check(&forces, &section, &material, &ctx) {
                    CheckOutcome::Skipped { reason } => {
                        assert!(reason.contains("BOX不正半径検証"));
                        assert!(reason.contains("外角半径"));
                        assert!(reason.contains("不正") || reason.contains("適用範囲外"));
                    }
                    CheckOutcome::Checked(_) => panic!("不正半径で検定済みを返してはならない: {r}"),
                }
            }
        }
    }

    #[test]
    fn box_shear_rejects_unknown_geometry_and_invalid_section_dimensions() {
        let material = test_support::mat("SN400");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 0.0,
            qy: 1.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let ctx = DesignCtx {
                kind,
                ..Default::default()
            };
            let mut section = supplied_box_section(Some(0.0), "BOX寸法検証");
            section.shape = None;
            assert!(
                matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("未確定"))
            );
            for invalid in [0.0, -1.0, 24.0, f64::NAN, f64::INFINITY, f64::MAX] {
                let mut section = supplied_box_section(Some(0.0), "BOX寸法検証");
                section.width = invalid;
                assert!(
                    matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                    CheckOutcome::Skipped { reason } if reason.contains("寸法"))
                );
            }
            for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
                let mut section = supplied_box_section(Some(0.0), "BOX不正板厚");
                section.shape = Some(SectionShape::SteelBox {
                    height: 400.0,
                    width: 300.0,
                    thick: invalid,
                    corner_r: Some(0.0),
                });
                assert!(
                    matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                    CheckOutcome::Skipped { reason } if reason.contains("寸法"))
                );
            }
            let mut section = supplied_box_section(Some(0.0), "BOX面積下限");
            section.depth = 4e-199;
            section.width = 3e-199;
            section.shape = Some(SectionShape::SteelBox {
                height: section.depth,
                width: section.width,
                thick: 1e-200,
                corner_r: Some(0.0),
            });
            assert!(
                matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                CheckOutcome::Skipped { reason } if reason.contains("面積が有限正"))
            );
        }
    }

    #[test]
    fn box_shear_keeps_small_positive_areas_and_rejects_nonfinite_stress() {
        let mut material = test_support::mat("相似比較用鋼材");
        material.fy = Some(235.0);
        for kind in [MemberKind::Girder, MemberKind::Column] {
            let ctx = DesignCtx {
                kind,
                term: LoadTerm::Long,
                length: 0.0,
                ..Default::default()
            };
            for scale in [1e-8_f64, 1.0, 1e8] {
                let section = SectionShape::SteelBox {
                    height: 400.0 * scale,
                    width: 300.0 * scale,
                    thick: 12.0 * scale,
                    corner_r: Some(30.0 * scale),
                }
                .to_section(SectionId(0), "相似Boxせん断".into());
                let mut forces = MemberForcesAt {
                    pos: 0.5,
                    n: 0.0,
                    qy: 100_000.0,
                    qz: 0.0,
                    my: 0.0,
                    mz: 0.0,
                };
                let result = SteelDesign
                    .check(&forces, &section, &material, &ctx)
                    .unwrap_checked();
                let expected =
                    100_000.0 / (8160.0 * scale.powi(2)) / steel_fs(235.0, LoadTerm::Long);
                assert!(
                    (result.ratio() / expected - 1.0).abs() < 1e-10,
                    "{kind:?} scale={scale}: {} / {expected}",
                    result.ratio()
                );
                if scale == 1e-8 {
                    for invalid in [f64::MAX, f64::INFINITY, f64::NAN] {
                        for direction in [0, 1] {
                            forces.qy = if direction == 0 { invalid } else { 0.0 };
                            forces.qz = if direction == 1 { invalid } else { 0.0 };
                            assert!(
                                matches!(SteelDesign.check(&forces, &section, &material, &ctx),
                                CheckOutcome::Skipped { reason } if reason.contains("せん断応力度が非有限"))
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn independent_directional_areas_preserve_purpose_axes_scaling_and_rotation() {
        for root_r in [0.0, 13.0] {
            let section = SectionShape::SteelH {
                height: 400.0,
                width: 200.0,
                web_thick: 8.0,
                flange_thick: 13.0,
                root_r: Some(root_r),
            }
            .to_section(SectionId(0), "H".into());
            assert_eq!((section.as_y, section.as_z), (5200.0, 3200.0));
            let (ay, az) = shear_area_2d(ShapeCategory::H, &section, 13.0, 8.0);
            assert_eq!(ay, 2992.0);
            assert!((az / 3466.6666666666665 - 1.0).abs() < 1e-10);
            assert!((100_000.0 / ay / 100.0 - 0.3342245989304813).abs() < 1e-10);
            assert_eq!(100_000.0 / section.as_z / 100.0, 0.3125);
            for scale in [0.1_f64, 10.0] {
                let scaled = SectionShape::SteelH {
                    height: 400.0 * scale,
                    width: 200.0 * scale,
                    web_thick: 8.0 * scale,
                    flange_thick: 13.0 * scale,
                    root_r: Some(root_r * scale),
                }
                .to_section(SectionId(0), "相似H".into());
                let (scaled_ay, scaled_az) =
                    shear_area_2d(ShapeCategory::H, &scaled, 13.0 * scale, 8.0 * scale);
                for (actual, expected) in [
                    (scaled_ay, ay),
                    (scaled_az, az),
                    (scaled.as_y, 5200.0),
                    (scaled.as_z, 3200.0),
                ] {
                    assert!((actual / (expected * scale.powi(2)) - 1.0).abs() < 1e-10);
                }
            }
        }
        for (radius, expected) in [
            (0.0, (9024.0, 6624.0)),
            (12.0, (9024.0, 6624.0)),
            (30.0, (8160.0, 5760.0)),
        ] {
            for scale in [0.1_f64, 1.0, 10.0] {
                for rotated in [false, true] {
                    let (height, width) = if rotated {
                        (300.0, 400.0)
                    } else {
                        (400.0, 300.0)
                    };
                    let section = SectionShape::SteelBox {
                        height: height * scale,
                        width: width * scale,
                        thick: 12.0 * scale,
                        corner_r: Some(radius * scale),
                    }
                    .to_section(SectionId(0), "BOX".into());
                    let (ay, az) =
                        shear_area_2d(ShapeCategory::Box, &section, 12.0 * scale, 12.0 * scale);
                    let expected = if rotated {
                        (expected.1, expected.0)
                    } else {
                        expected
                    };
                    assert!((ay / (expected.0 * scale.powi(2)) - 1.0).abs() < 1e-10);
                    assert!((az / (expected.1 * scale.powi(2)) - 1.0).abs() < 1e-10);
                    let stiffness = if rotated {
                        (9024.0, 6624.0)
                    } else {
                        (6624.0, 9024.0)
                    };
                    assert!((section.as_y / (stiffness.0 * scale.powi(2)) - 1.0).abs() < 1e-10);
                    assert!((section.as_z / (stiffness.1 * scale.powi(2)) - 1.0).abs() < 1e-10);
                }
            }
        }
        let mut section = Section::zero(SectionId(0), "任意断面".into());
        section.as_y = 123.0;
        section.as_z = 456.0;
        assert_eq!(
            shear_area_2d(ShapeCategory::Other, &section, 0.0, 0.0),
            (456.0, 123.0)
        );
    }

    #[test]
    fn axial_only_brace_does_not_require_surface_radius() {
        let section = supplied_box_section(None, "BOXブレース");
        let forces = MemberForcesAt {
            pos: 0.5,
            n: 100_000.0,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        };
        let ctx = DesignCtx {
            kind: MemberKind::Brace,
            ..Default::default()
        };
        assert!(matches!(
            SteelDesign.check(&forces, &section, &test_support::mat("SN400"), &ctx),
            CheckOutcome::Checked(_)
        ));
    }
}

/// テスト共通ヘルパー（`mat`/`rect_section`/`h_section`）。各サブモジュールの
/// テストから `super::super::test_support::*` として共有する。
#[cfg(test)]
pub(crate) mod test_support {
    use sepika_core::ids::{MaterialId, SectionId};
    use sepika_core::model::{Material, MaterialCategory, Section};
    use sepika_core::section_shape::SectionShape;

    pub(crate) fn mat(name: &str) -> Material {
        Material {
            strength_factor: None,
            concrete_class: Default::default(),
            id: MaterialId(0),
            name: name.to_string(),
            category: MaterialCategory::Steel,
            young: 205_000.0,
            poisson: 0.3,
            density: 7.85e-9,
            shear: None,
            fc: None,
            fy: None,
        }
    }

    pub(crate) fn rect_section(b: f64, d: f64, name: &str) -> Section {
        Section {
            frame_use: Some(sepika_core::model::FrameSectionUse::Girder),
            id: SectionId(0),
            name: name.to_string(),
            area: b * d,
            iy: b * d.powi(3) / 12.0,
            iz: d * b.powi(3) / 12.0,
            j: 0.0,
            depth: d,
            width: b,
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
        }
    }

    /// `SectionShape::SteelH` 付きの断面（実寸 tf/tw を持つ正規経路の検証用）。
    pub(crate) fn h_section(h: f64, b: f64, tw: f64, tf: f64) -> Section {
        let shape = SectionShape::SteelH {
            root_r: Some(0.0),
            height: h,
            width: b,
            web_thick: tw,
            flange_thick: tf,
        };
        shape.to_section(SectionId(0), format!("H-{}x{}x{}x{}", h, b, tw, tf))
    }
}

#[cfg(test)]
mod sm520_tests {
    use super::*;
    use crate::LoadTerm;
    use test_support::{h_section, mat, rect_section};
    fn forces() -> MemberForcesAt {
        MemberForcesAt {
            pos: 0.5,
            n: 2_200_000.0,
            qy: 0.0,
            qz: 0.0,
            my: 0.0,
            mz: 0.0,
        }
    }
    #[test]
    fn sm520_out_of_range_is_skipped_for_every_steel_member_and_explicit_fy() {
        let sec = h_section(1000.0, 400.0, 100.001, 80.0);
        for name in ["SM520", "SM520B", "SM520C"] {
            for fy in [None, Some(177.0), Some(235.0), Some(335.0), Some(999.0)] {
                let mut material = mat(name);
                material.fy = fy;
                for kind in [MemberKind::Brace, MemberKind::Girder, MemberKind::Column] {
                    let ctx = DesignCtx {
                        kind,
                        ..Default::default()
                    };
                    assert!(
                        matches!(SteelDesign.check(&forces(), &sec, &material, &ctx), CheckOutcome::Skipped { reason } if reason.contains("適用範囲") && reason.contains("100.001"))
                    );
                }
                if fy.is_some() {
                    material.name = "任意鋼材".into();
                    assert!(matches!(
                        SteelDesign.check(
                            &forces(),
                            &sec,
                            &material,
                            &DesignCtx {
                                kind: MemberKind::Brace,
                                ..Default::default()
                            }
                        ),
                        CheckOutcome::Checked(_)
                    ));
                }
            }
        }
    }
    #[test]
    fn sm520_325_fails_fixed_tension_demand_which_335_passes() {
        let mut sec = rect_section(100.0, 100.0, "固定面積ブレース");
        sec.thickness = Some(80.0);
        let ctx = DesignCtx {
            kind: MemberKind::Brace,
            term: LoadTerm::Long,
            ..Default::default()
        };
        let checked = SteelDesign
            .check(&forces(), &sec, &mat("SM520B"), &ctx)
            .unwrap_checked();
        assert!((checked.ratio() - 1.0153846153846153).abs() < 1e-12);
        assert!(!checked.ok());
        let mut old = mat("任意鋼材");
        old.fy = Some(335.0);
        let old_checked = SteelDesign
            .check(&forces(), &sec, &old, &ctx)
            .unwrap_checked();
        assert!((old_checked.ratio() - 0.9850746268656716).abs() < 1e-12);
        assert!(old_checked.ok());
        assert!((2_200_000.0 / checked.ratio() - 2_166_666.6666666665).abs() < 1e-8);
        assert!((2_200_000.0 / old_checked.ratio() - 2_233_333.3333333335).abs() < 1e-8);
    }
}
