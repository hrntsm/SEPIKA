//! [`SectionShape`] から [`Section`] を生成するビルダ（[`SectionShape::to_section`]）。

use super::constants::{KAPPA_RC, N_S_EQ};
use super::geometry::h_web_shear_area;
use super::types::SectionShape;
use crate::ids::SectionId;
use crate::model::Section;

impl SectionShape {
    /// 形状算定の断面を生成する。材料・用途・階は未設定。
    /// 算定不能なら panic。入力値の検証には `try_to_section` を用いる。
    pub fn to_section(&self, id: SectionId, name: String) -> Section {
        self.try_to_section(id, name)
            .unwrap_or_else(|error| panic!("断面性能を算定できません: {error}"))
    }

    /// 形状から断面を生成する。必要なフィレット半径・角Rが未知・不正ならエラー。
    pub fn try_to_section(&self, id: SectionId, name: String) -> Result<Section, String> {
        Ok(self.build_section(
            id,
            name,
            self.try_calc_area()?,
            self.try_calc_iy()?,
            self.try_calc_iz()?,
        ))
    }

    /// 未知のフィレット半径・角Rを保持して入力する。A・Iy・Iz は明示的な未算定区分となる。
    /// 不正な寸法はエラー。未算定値を用いる計算は `Section` の解決検証で拒否する。
    pub fn input_section(&self, id: SectionId, name: String) -> Result<Section, String> {
        self.validate_surface_radius()?;
        if matches!(
            self,
            Self::SteelH { root_r: None, .. }
                | Self::SteelBox { corner_r: None, .. }
                | Self::CftBox { corner_r: None, .. }
        ) {
            let mut section = self.build_section(id, name, 0.0, 0.0, 0.0);
            section.property_basis.area = crate::model::PropertyBasis::PendingShape;
            section.property_basis.iy = crate::model::PropertyBasis::PendingShape;
            section.property_basis.iz = crate::model::PropertyBasis::PendingShape;
            Ok(section)
        } else {
            self.try_to_section(id, name)
        }
    }

    fn build_section(&self, id: SectionId, name: String, area: f64, iy: f64, iz: f64) -> Section {
        let j = self.calc_j();
        let (depth, width, as_y, as_z) = match *self {
            SectionShape::SteelH {
                height,
                width,
                web_thick,
                flange_thick,
                ..
            } => (
                height,
                width,
                2.0 * width * flange_thick,
                h_web_shear_area(height, web_thick),
            ),
            SectionShape::SteelBox {
                height,
                width,
                thick,
                ..
            }
            | SectionShape::CftBox {
                height,
                width,
                thick,
                ..
            } => (
                height,
                width,
                2.0 * thick * (width - 2.0 * thick).max(0.0),
                2.0 * thick * (height - 2.0 * thick).max(0.0),
            ),
            SectionShape::SteelAngle {
                leg_a,
                leg_b,
                thick,
            } => (
                leg_a.max(leg_b),
                leg_a.min(leg_b),
                leg_b * thick,
                leg_a * thick,
            ),
            SectionShape::SteelChannel {
                height,
                width,
                web_thick,
                flange_thick,
            } => (
                height,
                width,
                2.0 * width * flange_thick,
                h_web_shear_area(height, web_thick),
            ),
            SectionShape::SteelTee {
                height,
                width,
                web_thick,
                flange_thick,
            } => (
                height,
                width,
                width * flange_thick,
                h_web_shear_area(height, web_thick),
            ),
            SectionShape::SteelPipe { outer_dia, .. } | SectionShape::CftPipe { outer_dia, .. } => {
                (outer_dia, outer_dia, area / 2.0, area / 2.0)
            }
            SectionShape::SteelFlatBar { width, thick } => (
                thick,
                width,
                width * thick / KAPPA_RC,
                width * thick / KAPPA_RC,
            ),
            SectionShape::SteelRoundBar { dia } => (dia, dia, area * 0.9, area * 0.9),
            SectionShape::SteelLipChannel {
                height,
                width,
                thick,
                ..
            } => (
                height,
                width,
                2.0 * (width - thick) * thick,
                h_web_shear_area(height, thick),
            ),
            SectionShape::SteelBuiltH {
                height,
                upper_width,
                upper_thick,
                lower_width,
                lower_thick,
                web_thick,
            } => (
                height,
                upper_width.max(lower_width),
                upper_width * upper_thick + lower_width * lower_thick,
                h_web_shear_area(height, web_thick),
            ),
            SectionShape::SrcBeamRect {
                b,
                d,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
                ..
            }
            | SectionShape::SrcColumnRect {
                b,
                d,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
                ..
            } => {
                let rc_as = b * d / KAPPA_RC;
                let s_web = h_web_shear_area(steel_height, steel_web_thick);
                let s_flange = 2.0 * steel_width * steel_flange_thick;
                (
                    d,
                    b,
                    rc_as + (N_S_EQ - 1.0) * s_flange,
                    rc_as + (N_S_EQ - 1.0) * s_web,
                )
            }
            SectionShape::RcBeamRect { b, d, .. } | SectionShape::RcColumnRect { b, d, .. } => {
                (d, b, b * d / KAPPA_RC, b * d / KAPPA_RC)
            }
            SectionShape::RcColumnCircle { d, .. } => (d, d, area / KAPPA_RC, area / KAPPA_RC),
            SectionShape::RcWall { thickness, .. } | SectionShape::RcSlab { thickness } => (
                1000.0,
                thickness,
                1000.0 * thickness / KAPPA_RC,
                1000.0 * thickness / KAPPA_RC,
            ),
        };
        let thickness = match *self {
            SectionShape::CftBox { thick, .. } | SectionShape::CftPipe { thick, .. } => Some(thick),
            SectionShape::RcWall { thickness, .. } | SectionShape::RcSlab { thickness } => {
                Some(thickness)
            }
            _ => None,
        };
        Section {
            id,
            name,
            frame_use: None,
            floor: None,
            area,
            iy,
            iz,
            j,
            depth,
            width,
            as_y,
            as_z,
            panel_thickness: None,
            thickness,
            shape: Some(self.clone()),
            material: None,
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: crate::model::SectionPropertyBasis::SHAPE,
        }
    }
}
