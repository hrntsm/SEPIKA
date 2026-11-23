use crate::behavior::{ElementUpdateError, PanelColumnSection};
use sepika_core::geom::vec3::dot;
use sepika_core::ids::NodeId;
use sepika_core::model::Model;
use sepika_core::panel_zone::{
    panel_joint_planes, steel_panel_column_sections, PanelAnalysisJoint, PanelShapeKind,
    PANEL_DIRECTION_TOL,
};
use sepika_core::section_shape::SectionShape;
use sepika_material::uniaxial::{Bilinear, UniaxialMaterial};

const HARDENING_RATIO: f64 = 0.01;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct PanelResponse {
    pub(super) moment: [f64; 2],
    pub(super) tangent: [f64; 2],
    pub(super) axial_slope: [f64; 2],
    pub(super) axial_gradient: Vec<(usize, f64)>,
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub(super) struct NonlinearPanelState {
    spring: Bilinear,
    pub(super) trial: PanelResponse,
    committed: PanelResponse,
}

impl NonlinearPanelState {
    pub(super) fn commit(&mut self) {
        self.spring.commit();
        self.committed.clone_from(&self.trial);
    }

    pub(super) fn revert(&mut self) {
        self.spring.revert();
        self.trial.clone_from(&self.committed);
    }
}

pub(super) struct NonlinearPanel {
    pub(super) columns: [PanelColumnSection; 2],
    pub(super) state: NonlinearPanelState,
    joint: PanelAnalysisJoint,
    pub(super) axes: [[f64; 3]; 2],
    diagonal_outer_radius_mm: Option<f64>,
    area_mm2: f64,
}

impl NonlinearPanel {
    pub(super) fn new(
        model: &Model,
        node: NodeId,
        joint: PanelAnalysisJoint,
    ) -> Result<Self, String> {
        let invalid = |reason: &str| format!("接合部 ID {} の非線形仕口パネル: {reason}", node.0);
        let adopted = model
            .element(joint.column)
            .and_then(|element| model.element_section(element))
            .ok_or_else(|| invalid("採用柱断面がありません"))?;
        let planes = panel_joint_planes(model, node, &model.elements);
        let [plane] = planes.as_slice() else {
            return Err(invalid(
                "標準形式の非線形復元力には単一の載荷構面が必要です",
            ));
        };
        plane
            .standard_applicability(node)
            .map_err(|reason| invalid(&reason))?;
        let positions = *plane
            .beams
            .first()
            .ok_or_else(|| invalid("梁フランジ位置がありません"))?
            .flanges
            .as_ref()
            .map_err(|reason| invalid(reason))?;
        let normal = [-plane.axis[1], plane.axis[0], 0.0];
        let axes = [normal, plane.axis];
        let [strong, weak] = joint.moment_axes.map(|axis| dot(axis, normal).abs());
        let diagonal_outer_radius_mm = match joint.panels[0].geometry.kind {
            PanelShapeKind::H { .. } if weak <= PANEL_DIRECTION_TOL => None,
            PanelShapeKind::H { .. } => {
                return Err(invalid("H形の載荷構面が強軸と整列していません"))
            }
            PanelShapeKind::Pipe => None,
            PanelShapeKind::SquareBox if strong.min(weak) <= PANEL_DIRECTION_TOL => None,
            PanelShapeKind::SquareBox if (strong - weak).abs() <= PANEL_DIRECTION_TOL => {
                let Some(SectionShape::SteelBox {
                    corner_r: Some(radius),
                    ..
                }) = adopted.shape
                else {
                    return Err(invalid(
                        "正方形等厚鋼管の45度全塑性耐力には外側角半径が必要です",
                    ));
                };
                Some(radius)
            }
            _ => {
                return Err(invalid(
                    "正方形等厚鋼管の載荷構面は柱主軸または45度方向に限ります",
                ))
            }
        };
        let columns =
            steel_panel_column_sections(model, node, joint.column, positions, &model.elements)
                .map_err(|reason| invalid(&reason))?;
        let area_mm2 = adopted.resolved_area().map_err(|reason| invalid(&reason))?;
        let direction = joint.panels[0];
        let perpendicular_stiffness = joint.panels[1].stiffness_n_mm;
        let trial = PanelResponse {
            moment: [0.0; 2],
            tangent: [direction.stiffness_n_mm, perpendicular_stiffness],
            axial_slope: [0.0; 2],
            axial_gradient: Vec::new(),
        };
        let moment = panel_plastic_moment(joint, area_mm2, diagonal_outer_radius_mm, 0.0)
            .ok_or_else(|| invalid("柱断面・寸法が当該載荷方向の全塑性耐力式の適用範囲外です"))?
            .0;
        Ok(Self {
            columns,
            state: NonlinearPanelState {
                spring: Bilinear::new(direction.stiffness_n_mm, moment, HARDENING_RATIO),
                committed: trial.clone(),
                trial,
            },
            joint,
            axes,
            diagonal_outer_radius_mm,
            area_mm2,
        })
    }

    pub(super) fn update(
        &mut self,
        node: NodeId,
        gamma: [f64; 2],
        axial_n: f64,
        axial_gradient: Vec<(usize, f64)>,
    ) -> Result<(), ElementUpdateError> {
        let invalid = |reason: &str| {
            ElementUpdateError(format!("接合部 ID {} の非線形仕口パネル: {reason}", node.0))
        };
        let perpendicular = [self.axes[1][0] * gamma[0], self.axes[1][1] * gamma[1]];
        let perpendicular_strain = perpendicular[0] + perpendicular[1];
        if !perpendicular_strain.is_finite()
            || perpendicular_strain.abs()
                > 32.0 * f64::EPSILON * (perpendicular[0].abs() + perpendicular[1].abs())
        {
            return Err(invalid(
                "固定した載荷構面と直交するせん断変形は単一方向の適用範囲外です",
            ));
        }
        let strain = self.axes[0][0] * gamma[0] + self.axes[0][1] * gamma[1];
        let stiffness = self.joint.panels[1].stiffness_n_mm;
        let axial_capacity_n = self.joint.panels[0].fy * self.area_mm2;
        let ratio = axial_n.abs() / axial_capacity_n;
        if !axial_capacity_n.is_finite() || (axial_n != 0.0 && ratio == 0.0) {
            return Err(invalid("上下柱の実軸力比を有限値として算定できません"));
        }
        let (moment, Some(slope)) = panel_plastic_moment(
            self.joint,
            self.area_mm2,
            self.diagonal_outer_radius_mm,
            ratio,
        )
        .ok_or_else(|| invalid("上下柱実軸力による全塑性耐力の適用範囲外です"))?
        else {
            return Err(invalid("軸耐力到達により有効な全塑性耐力がありません"));
        };
        let mut spring = self.state.spring.clone();
        spring.fy = moment;
        let (shear_moment, tangent) = spring.trial(strain);
        let dm_d_axial_n = if tangent == spring.e {
            0.0
        } else {
            (1.0 - HARDENING_RATIO) * spring.trial_loading_sign(strain) * slope * axial_n.signum()
                / axial_capacity_n
        };
        let perpendicular_moment = stiffness * perpendicular_strain;
        if [shear_moment, tangent, dm_d_axial_n, perpendicular_moment]
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(invalid("パネルモーメントまたは連成接線が非有限です"));
        }
        self.state.spring = spring;
        self.state.trial = PanelResponse {
            moment: [shear_moment, perpendicular_moment],
            tangent: [tangent, stiffness],
            axial_slope: [dm_d_axial_n, 0.0],
            axial_gradient,
        };
        Ok(())
    }
}

fn panel_plastic_moment(
    joint: PanelAnalysisJoint,
    area_mm2: f64,
    diagonal_outer_radius_mm: Option<f64>,
    axial_ratio: f64,
) -> Option<(f64, Option<f64>)> {
    let direction = joint.panels[0];
    if let Some(radius) = diagonal_outer_radius_mm {
        direction
            .geometry
            .steel_diagonal_plastic_moment(
                joint.db,
                direction.fy,
                axial_ratio,
                direction.product_thickness_mm,
                radius,
            )
            .map(|(moment, slope)| (moment, Some(slope)))
    } else {
        direction.geometry.steel_plastic_moment(
            joint.db,
            direction.fy,
            area_mm2,
            axial_ratio,
            direction.product_thickness_mm,
        )
    }
}
