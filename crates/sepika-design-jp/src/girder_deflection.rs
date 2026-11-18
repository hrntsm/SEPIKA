use sepika_core::dof::Dof;
use sepika_core::geom::vec3;
use sepika_core::ids::LoadCaseId;
use sepika_core::model::{
    ElementData, ElementKind, LoadCaseKind, Material, MemberLoadKind, Model, Section,
};
use sepika_core::section_shape::SectionShape;
use sepika_element::frame::beam::{BeamElement, MemberForces};
use sepika_load::floor::response::Polynomial;

use crate::{CheckComponent, CheckKind};

pub(crate) struct Deflection {
    downward_mm: f64,
    position_mm: f64,
    span_mm: f64,
}

impl Deflection {
    pub(crate) fn rc_component(&self) -> Option<CheckComponent> {
        let alpha = 8.0;
        let relative = alpha * (self.downward_mm.abs() / self.span_mm);
        let ratio = 250.0 * relative;
        if !ratio.is_finite() || (ratio == 0.0 && self.downward_mm != 0.0) {
            return None;
        }
        Some(CheckComponent {
            kind: CheckKind::Deflection,
            ratio,
            detail: format!(
                "δ={:.6} mm, v={:+.6} mm（下向き正）, x={:.3} mm（i端から）, L={:.3} mm, α={alpha:.0}, α·δ/L={relative:.6}, 制限 1/250, 検定比={ratio:.6}",
                self.downward_mm.abs(), self.downward_mm, self.position_mm, self.span_mm,
            ),
        })
    }

    pub(crate) fn steel_detail(&self) -> String {
        format!(
            "たわみ参考 δ={:.6} mm, v={:+.6} mm（下向き正）, x={:.3} mm（i端から）, L={:.3} mm, δ/L={:.6}",
            self.downward_mm.abs(),
            self.downward_mm,
            self.position_mm,
            self.span_mm,
            self.downward_mm.abs() / self.span_mm,
        )
    }
}

pub(crate) fn maximum(
    model: &Model,
    elem: &ElementData,
    sec: &Section,
    mat: &Material,
    forces: &MemberForces,
    terms: Option<&[(LoadCaseId, f64)]>,
) -> Result<Deflection, String> {
    if elem.kind != ElementKind::Beam || elem.nodes.len() != 2 {
        return Err("単一の弾性梁要素が必要です".into());
    }
    if elem.end_offsets_mm != [[0.0; 3]; 2] {
        return Err("材端に偏心がある梁は対象外です".into());
    }
    let [i, j] = [elem.nodes[0], elem.nodes[1]];
    let i = model.node(i).ok_or("i端の節点が不足しています")?;
    let j = model.node(j).ok_or("j端の節点が不足しています")?;
    if !(i.restraint.is_fixed(Dof::Uz) && j.restraint.is_fixed(Dof::Uz)) {
        return Err(
            "両端の直接鉛直拘束を確認できません。片持ち梁や架構で支持される梁は対象外です".into(),
        );
    }
    let rigid = model.analysis_rigid_zone(elem);
    if !rigid.faces_computed() {
        return Err("柱フェース距離が未算定です".into());
    }
    if rigid.face_i != Some(0.0)
        || rigid.face_j != Some(0.0)
        || [
            rigid.length_i,
            rigid.length_j,
            rigid.panel_offset_i,
            rigid.panel_offset_j,
        ] != [0.0; 4]
    {
        return Err(
            "柱フェース距離・剛域・仕口オフセットがある梁の有効長さと弦を確認できません".into(),
        );
    }
    let [_, ey, _] = vec3::local_frame_axes(i.coord, j.coord, elem.local_axis.ref_vector)?;
    if i.coord[2] != j.coord[2] || ey[0].abs() > 1e-12 || ey[1].abs() > 1e-12 {
        return Err("水平で強軸曲げが鉛直変形となる梁が必要です".into());
    }

    let span_mm = model.member_length(elem);
    if !span_mm.is_finite() || span_mm <= 1e-9 {
        return Err("スパンが有限な正の材長として解決できません".into());
    }
    let w_n_per_mm = uniform_load(model, elem, terms, span_mm)?;
    let beam = BeamElement::try_new(elem, model)?;
    let inertia_mm4 = match sec.shape.as_ref() {
        Some(SectionShape::RcBeamRect { b, d, .. }) => b * d.powi(3) / 12.0,
        _ => sec.iy,
    };
    let ei_n_mm2 = mat.young * inertia_mm4;
    if !ei_n_mm2.is_finite() || ei_n_mm2 <= 0.0 {
        return Err("強軸曲げ剛性 EI は正の有限値が必要です".into());
    }
    let analysis_ei_n_mm2 = beam.e * beam.iz;
    if !analysis_ei_n_mm2.is_finite() || (analysis_ei_n_mm2 / ei_n_mm2 - 1.0).abs() > 1e-12 {
        return Err("解析の強軸曲げ剛性が梁単体の全断面曲げ剛性と異なります".into());
    }
    maximum_from_forces(forces, w_n_per_mm, ey[2], span_mm, ei_n_mm2)
}

fn uniform_load(
    model: &Model,
    elem: &ElementData,
    terms: Option<&[(LoadCaseId, f64)]>,
    span_mm: f64,
) -> Result<f64, String> {
    let terms = terms
        .filter(|terms| terms.iter().any(|&(_, factor)| factor != 0.0))
        .ok_or("解析に使った固定・積載の荷重ケースを確認できません")?;
    let mut w_n_per_mm = 0.0;
    for &(id, factor) in terms {
        if factor == 0.0 {
            continue;
        }
        let case = model
            .load_cases
            .iter()
            .find(|case| case.id == id)
            .ok_or_else(|| format!("解析に使った荷重ケース {} が見つかりません", id.0))?;
        if !factor.is_finite()
            || factor < 0.0
            || !matches!(case.kind, LoadCaseKind::Dead | LoadCaseKind::Live)
        {
            return Err("長期たわみには固定・積載荷重と非負の有限な組合せ係数が必要です".into());
        }
        for load in case.member.iter().filter(|load| load.elem == elem.id) {
            let intensity = match load.kind {
                MemberLoadKind::Point { .. } => return Err("部材集中荷重は対象外です".into()),
                MemberLoadKind::Distributed { a, b, w1, w2 }
                    if a == 0.0 && b == span_mm && w1 == w2 =>
                {
                    w1
                }
                _ => return Err("部材の分布荷重は全長の等分布に限ります".into()),
            };
            load.validate_extent(span_mm)?;
            load.validate_direction()?;
            if load.dir[..2] != [0.0; 2] {
                return Err("部材荷重の作用方向は鉛直に限ります".into());
            }
            w_n_per_mm -= load.dir[2].signum() * factor * intensity;
        }
    }
    if !w_n_per_mm.is_finite() {
        return Err("組合せの等分布荷重強度が有限値ではありません".into());
    }
    Ok(w_n_per_mm)
}

fn maximum_from_forces(
    forces: &MemberForces,
    w_n_per_mm: f64,
    local_y_up: f64,
    span_mm: f64,
    ei_n_mm2: f64,
) -> Result<Deflection, String> {
    let end_moment = |t| {
        forces
            .at
            .iter()
            .find(|(x, _)| *x == t)
            .map(|(_, f)| f[5] * local_y_up)
    };
    let m_i = end_moment(0.0).ok_or("i端の解析曲げが不足しています")?;
    let m_j = end_moment(1.0).ok_or("j端の解析曲げが不足しています")?;
    let m_c = end_moment(0.5).ok_or("中央の解析曲げが不足しています")?;
    let w_l2 = (w_n_per_mm * span_mm) * span_mm;
    if ![m_i, m_j, m_c, w_l2].iter().all(|x| x.is_finite()) || (w_l2 == 0.0 && w_n_per_mm != 0.0) {
        return Err("解析曲げまたは荷重による曲げを有限な数値で表現できません".into());
    }
    let moment_scale = m_i.abs().max(m_j.abs()).max(w_l2.abs());
    let moment_tolerance = 1e-8 * moment_scale;
    if forces.at.iter().any(|&(t, f)| {
        let expected = (1.0 - t) * m_i + t * m_j + (w_l2 / 2.0) * t * (1.0 - t);
        !(0.0..=1.0).contains(&t)
            || !f[5].is_finite()
            || (f[5] * local_y_up - expected).abs() > moment_tolerance
    }) {
        return Err("解析曲げ図と荷重ケースの全長等分布が一致しません".into());
    }
    if moment_scale == 0.0 {
        return Ok(Deflection {
            downward_mm: 0.0,
            position_mm: 0.0,
            span_mm,
        });
    }
    let m_i = m_i / moment_scale;
    let m_j = m_j / moment_scale;
    let w_l2 = w_l2 / moment_scale;
    let mut v = Polynomial(vec![m_i, m_j - m_i + w_l2 / 2.0, -w_l2 / 2.0])
        .integral()
        .integral()
        .scale(-1.0);
    v.0[1] -= v.value(1.0);
    let mut at_max = 0.0;
    let mut signed_max = 0.0_f64;
    for t in v
        .derivative()
        .roots(0.0, 1.0)
        .into_iter()
        .filter(|t| *t > 0.0 && *t < 1.0)
    {
        let value = v.value(t);
        if value.abs() > signed_max.abs() {
            at_max = t;
            signed_max = value;
        }
    }
    let downward_mm = ((signed_max * moment_scale / ei_n_mm2) * span_mm) * span_mm;
    if !downward_mm.is_finite() || downward_mm == 0.0 || !(downward_mm / span_mm).is_finite() {
        return Err("最大たわみを有限な数値で算定できません".into());
    }
    Ok(Deflection {
        downward_mm,
        position_mm: at_max * span_mm,
        span_mm,
    })
}
