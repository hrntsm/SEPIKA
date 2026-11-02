use super::*;
use sepika_design_jp::secondary::story_columns::checked_story_columns;
use sepika_solver::dynamic::lumped_mass::{bending_drifts, StoryBending, StoryIdentification};

fn bad(layer: &Layer, reason: impl std::fmt::Display) -> JobError {
    JobError::InvalidInput(format!("階 {}: {reason}", layer.name))
}

fn axial_at(
    model: &Model,
    res: &StaticOnce,
    id: sepika_core::ids::ElemId,
    upper: bool,
) -> JobResult<f64> {
    let elem = model
        .element(id)
        .ok_or_else(|| JobError::InvalidInput(format!("柱 {} がありません", id.0)))?;
    let p0 = model.nodes[elem.nodes[0].index()].coord;
    let p1 = model.nodes[elem.nodes[1].index()].coord;
    let frame = LocalFrame::from_nodes(p0, p1, elem.local_axis.ref_vector)
        .map_err(|e| JobError::InvalidInput(e.into()))?;
    let forward = p1[2] > p0[2];
    let force = res
        .member_forces
        .iter()
        .find(|(key, _)| *key == id)
        .and_then(|(_, f)| {
            if upper == forward {
                f.at.last()
            } else {
                f.at.first()
            }
        })
        .ok_or_else(|| JobError::InvalidInput(format!("柱 {} の軸力がありません", id.0)))?
        .1[0];
    let n = force * frame.rot[0][2].abs();
    if !n.is_finite() {
        return Err(JobError::InvalidInput(format!(
            "柱 {} の鉛直軸力が不正です: {}",
            id.0, n
        )));
    }
    Ok(n)
}

fn ei_values(model: &Model, res: &StaticOnce, dir: SeismicDir) -> JobResult<Vec<f64>> {
    let mut result = Vec::new();
    for layer in model.layers() {
        let cols = checked_story_columns(model, layer.top).map_err(|e| bad(&layer, e))?;
        let covered: std::collections::HashSet<_> = cols
            .iter()
            .flat_map(|c| c.segments.iter().copied())
            .collect();
        for element in layer_elements(model, &layer)? {
            if !model
                .element_section(element)
                .is_some_and(|s| s.frame_use == Some(sepika_core::model::FrameSectionUse::Column))
            {
                continue;
            }
            let a = model.nodes[element.nodes[0].index()].coord;
            let b = model.nodes[element.nodes[1].index()].coord;
            if a[0] != b[0] || a[1] != b[1] {
                return Err(bad(
                    &layer,
                    format!(
                        "傾斜柱 {} は鉛直軸変形を柱軸伸縮として代表できないため対象外です",
                        element.id.0
                    ),
                ));
            }
            if !covered.contains(&element.id) {
                return Err(bad(&layer,format!("柱 {} は上下階SLを結ぶ非分岐の柱として追跡できません（下端階未到達・中間分岐・複数階横断を確認してください）",element.id.0)));
            }
        }
        if cols.is_empty() {
            return Err(bad(&layer, "曲げ剛性を算定する柱がありません"));
        }
        let axis = if dir == SeismicDir::X { 0 } else { 1 };
        let sign = if dir == SeismicDir::X { -1.0 } else { 1.0 };
        let origin = cols
            .iter()
            .map(|c| model.nodes[c.top.index()].coord[axis])
            .sum::<f64>()
            / cols.len() as f64;
        let mut mb = 0.0;
        let mut mt = 0.0;
        let mut work = 0.0;
        for c in cols {
            for id in &c.segments {
                let e = model.element(*id).unwrap();
                let a = model.nodes[e.nodes[0].index()].coord;
                let b = model.nodes[e.nodes[1].index()].coord;
                if a[0] != b[0] || a[1] != b[1] {
                    return Err(bad(&layer, format!("傾斜柱 {} は鉛直軸変形を柱軸伸縮として代表できないため曲げせん断型の対象外です", id.0)));
                }
            }
            let ub = res
                .disp
                .get(c.bottom.index())
                .ok_or_else(|| bad(&layer, format!("柱 {} 下端変位がありません", c.top_elem.0)))?;
            let ut = res
                .disp
                .get(c.top.index())
                .ok_or_else(|| bad(&layer, format!("柱 {} 上端変位がありません", c.top_elem.0)))?;
            let nt = axial_at(model, res, c.segments[0], true).map_err(|e| bad(&layer, e))?;
            let nb = axial_at(model, res, *c.segments.last().unwrap(), false)
                .map_err(|e| bad(&layer, e))?;
            let dz = ut[2] - ub[2];
            if !dz.is_finite() {
                return Err(bad(
                    &layer,
                    format!("柱 {} の鉛直軸変形が不正です", c.top_elem.0),
                ));
            }
            mt += sign * nt * (model.nodes[c.top.index()].coord[axis] - origin);
            mb += sign * nb * (model.nodes[c.bottom.index()].coord[axis] - origin);
            work += 0.5 * (nb + nt) * dz;
        }
        let moment = 0.5 * (mb + mt);
        let theta = work / moment;
        let h = model.stories[layer.top.index()]
            .sl_mm()
            .zip(model.stories[layer.bottom.index()].sl_mm())
            .map(|(t, b)| t - b)
            .ok_or_else(|| bad(&layer, "構造階高が未設定です"))?;
        let ei = h * moment / theta;
        if !h.is_finite()
            || h <= 0.0
            || !moment.is_finite()
            || moment == 0.0
            || !work.is_finite()
            || work <= 0.0
            || !ei.is_finite()
            || ei <= 0.0
        {
            return Err(bad(&layer,format!("{:?}方向の曲げ剛性を算定できません: h={} mm, Mb={} Nmm, Mt={} Nmm, ΣNΔuz={} Nmm, Δθ={} rad, EI={} Nmm²",dir,h,mb,mt,work,theta,ei)));
        }
        result.push(ei);
    }
    Ok(result)
}

fn scope_location(model: &Model, node: sepika_core::ids::NodeId) -> String {
    let node = &model.nodes[node.index()];
    let name = node
        .story
        .and_then(|id| model.stories.get(id.index()))
        .map(|s| s.name.clone())
        .or_else(|| {
            model
                .layers()
                .iter()
                .find(|layer| {
                    layer.bottom_sl_mm <= node.coord[2] && node.coord[2] <= layer.top_sl_mm
                })
                .map(|layer| layer.name.clone())
        })
        .unwrap_or_else(|| "未所属".into());
    format!("階 {}（高さ {} mm）", name, node.coord[2])
}

fn validate_scope(inp: &LumpedMassBuildInput<'_>) -> JobResult<()> {
    use sepika_core::dof::Dof;
    use sepika_core::model::ElementKind;
    let model = inp.model;
    let mut structural = sepika_core::dof::structural_nodes(model);
    for constraint in &model.constraints {
        use sepika_core::model::Constraint;
        match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => {
                structural[master.index()] = true;
                for node in slaves {
                    structural[node.index()] = true;
                }
            }
            Constraint::Mpc { master, terms } => {
                structural[master.index()] = true;
                for (node, _, _) in terms {
                    structural[node.index()] = true;
                }
            }
        }
    }
    let base_z = model.layers().first().map(|layer| layer.bottom_sl_mm);
    let horizontal = if inp.dim == StickDim::Spatial {
        &[Dof::Ux, Dof::Uy][..]
    } else if inp.dir == SeismicDir::X {
        &[Dof::Ux][..]
    } else {
        &[Dof::Uy][..]
    };
    for node in &model.nodes {
        if !structural[node.id.index()] {
            continue;
        }
        if let Some(spring) = node.support_spring {
            let dofs = [Dof::Ux, Dof::Uy, Dof::Uz, Dof::Rx, Dof::Ry, Dof::Rz];
            if let Some((dof, value)) = dofs
                .into_iter()
                .zip(spring)
                .find(|(dof, value)| !node.restraint.is_fixed(*dof) && *value != 0.0)
            {
                return Err(JobError::InvalidInput(format!(
                    "{}: 節点 {} の活動する支持ばね（{:?}={}）は基部固定の曲げせん断型の対象外です",
                    scope_location(model, node.id),
                    node.id.0,
                    dof,
                    value
                )));
            }
        }
        if base_z.is_some_and(|z| node.coord[2] > z)
            && horizontal.iter().any(|dof| node.restraint.is_fixed(*dof))
        {
            return Err(JobError::InvalidInput(format!("{}: 節点 {} は基部以外で解析対象の水平方向が固定されているため、基部固定の曲げせん断型の対象外です",scope_location(model,node.id),node.id.0)));
        }
    }
    for element in &model.elements {
        if element.kind == ElementKind::PanelZone {
            continue;
        }
        if matches!(element.kind, ElementKind::Beam | ElementKind::Brace { .. })
            && element.nodes.len() == 2
        {
            continue;
        }
        let kind = match element.kind {
            ElementKind::NodalSpring => "節点ばね",
            ElementKind::Isolator => "免震部材",
            ElementKind::Damper => "制振ダンパー",
            ElementKind::Wall => "壁要素",
            ElementKind::Shell => "シェル要素",
            ElementKind::Fiber => "ファイバー要素",
            ElementKind::MultiSpring => "マルチスプリング要素",
            _ => "接続節点数が不正な骨組要素",
        };
        let location = element
            .nodes
            .first()
            .map(|node| scope_location(model, *node))
            .unwrap_or_else(|| "階未所属".into());
        return Err(JobError::InvalidInput(format!(
            "{}: 部材 {} の{}は柱・梁・ブレース骨組の曲げせん断型の対象外です",
            location, element.id.0, kind
        )));
    }
    Ok(())
}

fn layer_elements<'a>(
    model: &'a Model,
    layer: &Layer,
) -> JobResult<Vec<&'a sepika_core::model::ElementData>> {
    let bottom = model.stories[layer.bottom.index()]
        .sl_mm()
        .ok_or_else(|| bad(layer, "下端SLが未設定です"))?;
    let top = model.stories[layer.top.index()]
        .sl_mm()
        .ok_or_else(|| bad(layer, "上端SLが未設定です"))?;
    let cut = 0.5 * (bottom + top);
    let mut elements = Vec::new();
    for e in &model.elements {
        if e.kind == sepika_core::model::ElementKind::PanelZone {
            continue;
        }
        let (zmin, zmax) =
            e.nodes
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), id| {
                    let z = model.nodes[id.index()].coord[2];
                    (lo.min(z), hi.max(z))
                });
        if !(zmin <= cut && cut < zmax) {
            continue;
        }
        if e.nodes.len() != 2
            || !matches!(
                e.kind,
                sepika_core::model::ElementKind::Beam
                    | sepika_core::model::ElementKind::Brace { .. }
            )
        {
            continue;
        }
        elements.push(e);
    }
    Ok(elements)
}

pub(super) fn signed_shears(
    model: &Model,
    res: &StaticOnce,
    dir: SeismicDir,
) -> JobResult<Vec<f64>> {
    super::story_shears(model, res, dir)
}

pub(super) fn step_drifts(
    inp: &LumpedMassBuildInput<'_>,
    dir: SeismicDir,
    step: &sepika_solver::nonlinear::pushover::PushoverStep,
    point: &sepika_solver::nonlinear::pushover::CapacityPoint,
    base: &LumpedMassModel,
) -> JobResult<Vec<f64>> {
    if inp.dim == StickDim::Planar {
        return Ok(point.story_drift.clone());
    }
    if step.floor_disp.len() != inp.model.stories.len() {
        return Err(JobError::InvalidInput("3次元曲げせん断型に必要な増分解析の各階変位・ねじれ回転が不足しています。増分解析を再実行してください".into()));
    }
    let axis = usize::from(dir == SeismicDir::Y);
    inp.model
        .layers()
        .iter()
        .enumerate()
        .map(|(i, layer)| {
            let xy = base.spatial[i].rigidity_xy;
            let displacement = |sid: sepika_core::ids::StoryId| -> JobResult<f64> {
                let story = &inp.model.stories[sid.index()];
                let nid = story_reference_node(inp.model, story).ok_or_else(|| {
                    bad(
                        layer,
                        format!("階 {} の増分応答の代表節点がありません", story.name),
                    )
                })?;
                let p = inp.model.nodes[nid.index()].coord;
                let u = step.floor_disp[sid.index()];
                if u.iter().any(|v| !v.is_finite()) {
                    return Err(bad(
                        layer,
                        format!(
                            "階 {} の増分変位・ねじれ回転が非有限です: {:?}",
                            story.name, u
                        ),
                    ));
                }
                Ok(if axis == 0 {
                    u[0] - (xy[1] - p[1]) * u[2]
                } else {
                    u[1] + (xy[0] - p[0]) * u[2]
                })
            };
            Ok(displacement(layer.top)? - displacement(layer.bottom)?)
        })
        .collect()
}

fn skeletons(
    inp: &LumpedMassBuildInput<'_>,
    dir: SeismicDir,
    ei: &[f64],
    height: &[f64],
    base: &LumpedMassModel,
) -> JobResult<(Vec<StoryTrilinear>, Vec<StoryIdentification>)> {
    let layers = inp.model.layers();
    let res = static_of(inp, dir)?;
    if !inp.nonlinear {
        let q = signed_shears(inp.model, res, dir)?;
        let db =
            bending_drifts(height, ei, &q).map_err(|e| JobError::InvalidInput(e.to_string()))?;
        let mut sk = Vec::new();
        for (i, layer) in layers.iter().enumerate() {
            let d = if inp.dim == StickDim::Spatial {
                story_drift_at(inp.model, res, dir, layer, base.spatial[i].rigidity_xy)
            } else {
                story_drifts(inp.model, res, dir)[i]
            };
            let ds = d - db[i];
            let k = q[i] / ds;
            if !k.is_finite() || k <= 0.0 || q[i] * ds <= 0.0 {
                return Err(bad(layer,format!("正のせん断剛性を算定できません: Q={} N, δ={} mm, δb={} mm, δs={} mm, Ks={} N/mm",q[i],d,db[i],ds,k)));
            }
            sk.push(StoryTrilinear::elastic(k));
        }
        return Ok((sk, Vec::new()));
    }
    let fits = super::identification::fit(inp, dir, base, Some((ei, height)))?;
    Ok((fits.iter().map(|f| f.fit.skeleton).collect(), fits))
}

pub(super) fn build(inp: LumpedMassBuildInput<'_>) -> JobResult<LumpedMassModel> {
    validate_scope(&inp)?;
    let selected = static_of(&inp, inp.dir)?;
    let ex = if inp.dim == StickDim::Spatial {
        static_of(&inp, SeismicDir::X)?
    } else {
        selected
    };
    let ey = if inp.dim == StickDim::Spatial {
        static_of(&inp, SeismicDir::Y)?
    } else {
        selected
    };
    let xi = ei_values(
        inp.model,
        ex,
        if inp.dim == StickDim::Spatial {
            SeismicDir::X
        } else {
            inp.dir
        },
    )?;
    let yi = if inp.dim == StickDim::Spatial {
        ei_values(inp.model, ey, SeismicDir::Y)?
    } else {
        xi.clone()
    };
    let mut lm = match inp.dim {
        StickDim::Planar => {
            let stories = inp
                .model
                .layers()
                .iter()
                .map(|layer| {
                    Ok(StoryStick {
                        story: layer.bottom,
                        mass: layer_mass(layer)?,
                        height: layer.height,
                        skeleton: StoryTrilinear::elastic(1.0),
                    })
                })
                .collect::<JobResult<Vec<_>>>()?;
            let mut model =
                LumpedMassModel::from_stories(LumpedMassType::EquivalentBendingShear, stories);
            model.dir = inp.dir;
            model
        }
        StickDim::Spatial => build_spatial(LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentShear,
            nonlinear: false,
            source: LumpedStiffnessSource::StoryQd,
            ..inp
        })?,
    };
    let layers = inp.model.layers();
    let mut heights = Vec::new();
    for (i, layer) in layers.iter().enumerate() {
        let h = inp.model.stories[layer.top.index()].sl_mm().unwrap()
            - inp.model.stories[layer.bottom.index()].sl_mm().unwrap();
        lm.stories[i].height = h;
        heights.push(h);
        let dm = layer
            .dynamic_mass
            .ok_or_else(|| bad(layer, "動的物理質量がありません"))?;
        let (ei_x, ei_y) = if inp.dim == StickDim::Spatial {
            (yi[i], xi[i])
        } else if inp.dir == SeismicDir::X {
            (0.0, xi[i])
        } else {
            (xi[i], 0.0)
        };
        lm.bending.push(StoryBending {
            ei_x,
            ei_y,
            inertia: dm.horizontal_inertia_t_mm2,
        });
    }
    let (skx, fitx) = skeletons(
        &inp,
        if inp.dim == StickDim::Spatial {
            SeismicDir::X
        } else {
            inp.dir
        },
        &xi,
        &heights,
        &lm,
    )?;
    let (sky, fity) = if inp.dim == StickDim::Spatial {
        skeletons(&inp, SeismicDir::Y, &yi, &heights, &lm)?
    } else {
        (skx.clone(), Vec::new())
    };
    lm.identifications.extend(fitx);
    lm.identifications.extend(fity);
    for i in 0..layers.len() {
        lm.stories[i].skeleton = if inp.dir == SeismicDir::X || inp.dim == StickDim::Planar {
            skx[i]
        } else {
            sky[i]
        };
        if inp.dim == StickDim::Spatial {
            lm.spatial[i].skeleton_x = skx[i];
            lm.spatial[i].skeleton_y = sky[i];
            lm.spatial[i].k1_x = skx[i].k1;
            lm.spatial[i].k1_y = sky[i].k1;
        }
    }
    lm.model_type = LumpedMassType::EquivalentBendingShear;
    lm.nonlinear = inp.nonlinear;
    Ok(lm)
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::{ElemId, NodeId, StoryId};
    use sepika_core::model::Constraint;
    use sepika_element::frame::beam::MemberForces;

    fn fixture(dir: SeismicDir, reverse: bool, split: bool) -> (Model, StaticOnce) {
        let mut model = super::super::tests::two_story_column_model();
        let node = model.nodes[0].clone();
        let element = model.elements[0].clone();
        model.nodes.clear();
        model.elements.clear();
        model.constraints.clear();
        let xy = [
            [-1000.0, -2000.0],
            [1000.0, -2000.0],
            [-1000.0, 2000.0],
            [1000.0, 2000.0],
        ];
        for level in 0..3 {
            let ids: Vec<_> = (0..4).map(|i| NodeId((level * 4 + i) as u32)).collect();
            for (k, &[x, y]) in xy.iter().enumerate() {
                let mut n = node.clone();
                n.id = ids[k];
                n.coord = [x, y, level as f64 * 3000.0];
                n.story = Some(StoryId(level as u32));
                n.restraint = if level == 0 {
                    sepika_core::dof::Dof6Mask::FIXED
                } else {
                    sepika_core::dof::Dof6Mask::FREE
                };
                model.nodes.push(n);
            }
            model.stories[level].node_ids = ids.clone();
            let dm = model.stories[level].dynamic_mass.as_mut().unwrap();
            dm.center_xy_mm = [0.0, 0.0];
            dm.horizontal_inertia_t_mm2 = [4e7, 1e7, 0.0];
            dm.inertia_t_mm2 = 5e7;
            model.constraints.push(Constraint::RigidDiaphragm {
                story: StoryId(level as u32),
                master: ids[0],
                slaves: ids[1..].to_vec(),
                weight: None,
                ci_override: None,
            });
        }
        for layer in 0..2 {
            for (k, point) in xy.iter().enumerate() {
                let bottom = NodeId((layer * 4 + k) as u32);
                let top = NodeId(((layer + 1) * 4 + k) as u32);
                let segments = if split {
                    let mut n = node.clone();
                    n.id = NodeId(model.nodes.len() as u32);
                    n.coord = [point[0], point[1], layer as f64 * 3000.0 + 1500.0];
                    n.story = Some(StoryId(layer as u32));
                    n.restraint = sepika_core::dof::Dof6Mask::FREE;
                    let mid = n.id;
                    model.nodes.push(n);
                    vec![[bottom, mid], [mid, top]]
                } else {
                    vec![[bottom, top]]
                };
                for mut pair in segments {
                    if reverse {
                        pair.reverse();
                    }
                    let mut e = element.clone();
                    e.id = ElemId(model.elements.len() as u32);
                    e.nodes = pair.into_iter().collect();
                    model.elements.push(e);
                }
            }
        }
        let axis = usize::from(dir == SeismicDir::Y);
        let ei = if axis == 0 { 1.2e14 } else { 4.8e14 };
        let db = bending_drifts(&[3000.0; 2], &[ei; 2], &[2000.0, 1000.0]).unwrap();
        let mut disp = vec![[0.0; 6]; model.nodes.len()];
        for n in &model.nodes {
            let level = n.coord[2] / 3000.0;
            let moments = [6e6, 3e6];
            let mut theta = 0.0;
            let full = level.floor() as usize;
            for &m in moments.iter().take(full) {
                theta += 3000.0 * m / ei;
            }
            if full < 2 {
                theta += (level - full as f64) * 3000.0 * moments[full] / ei;
            }
            disp[n.id.index()][2] = -theta * n.coord[axis];
            if level == 1.0 {
                disp[n.id.index()][axis] = db[0] + 0.2;
            }
            if level == 2.0 {
                disp[n.id.index()][axis] = db[0] + db[1] + 0.3;
            }
        }
        let mut member_forces = Vec::new();
        for e in &model.elements {
            let p0 = model.nodes[e.nodes[0].index()].coord;
            let p1 = model.nodes[e.nodes[1].index()].coord;
            let layer = ((p0[2] + p1[2]) / 2.0 / 3000.0) as usize;
            let n = -[6e6, 3e6][layer] * p0[axis] / if axis == 0 { 4e6 } else { 16e6 };
            let frame = LocalFrame::from_nodes(p0, p1, e.local_axis.ref_vector).unwrap();
            let q = [2000.0, 1000.0][layer] / 4.0;
            let local = [
                n,
                -q * frame.rot[0][2].signum() * frame.rot[1][axis],
                -q * frame.rot[0][2].signum() * frame.rot[2][axis],
                0.0,
                0.0,
                0.0,
            ];
            member_forces.push((
                e.id,
                MemberForces {
                    at: vec![(0.0, local), (1.0, local)],
                },
            ));
        }
        (
            model,
            StaticOnce {
                story_shears: Some([vec![2000.0, 1000.0], vec![2000.0, 1000.0]]),
                stiffness_ratio_disp: None,
                disp,
                member_forces,
                panel_moments: Vec::new(),
            },
        )
    }
    fn input<'a>(m: &'a Model, r: &'a StaticOnce, dir: SeismicDir) -> LumpedMassBuildInput<'a> {
        LumpedMassBuildInput {
            model_type: LumpedMassType::EquivalentBendingShear,
            model: m,
            dim: StickDim::Planar,
            source: LumpedStiffnessSource::StoryQd,
            dir,
            nonlinear: false,
            secant_ratio: 0.75,
            target_drift_angle: 1.0 / 200.0,
            res_x: Some(r),
            res_y: Some(r),
            po_x: None,
            po_y: None,
        }
    }
    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9 * b.abs().max(1.0), "{a} != {b}");
    }

    #[test]
    fn solved_frame_static_modes_and_plastic_response_comparison() {
        use sepika_core::ids::{LoadCaseId, MaterialId, SectionId};
        use sepika_core::model::{
            ElementKind, FrameSectionUse, MassMethod, Material, MaterialCategory,
        };
        use sepika_solver::dynamic::lumped_mass::{bending_shear_element, lumped_mass_eigen};
        let (mut model, _) = fixture(SeismicDir::X, false, false);
        model.mass_method = MassMethod::LumpedOnly;
        model.materials.push(Material {
            id: MaterialId(0),
            name: "steel".into(),
            category: MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fy: Some(50.0),
            fc: None,
            strength_factor: None,
            concrete_class: Default::default(),
        });
        let section = &mut model.sections[0];
        section.material = Some(MaterialId(0));
        section.area = 40000.0;
        section.iy = 1.0e9;
        section.iz = 1.0e9;
        section.j = 5.0e8;
        section.depth = 500.0;
        section.width = 400.0;
        section.as_y = 30000.0;
        section.as_z = 30000.0;
        let mut girder = section.clone();
        girder.id = SectionId(1);
        girder.frame_use = Some(FrameSectionUse::Girder);
        model.sections.push(girder);
        for level in 1..=2 {
            for (a, b) in [(0, 1), (1, 3), (3, 2), (2, 0)] {
                let mut e = model.elements[0].clone();
                e.id = ElemId(model.elements.len() as u32);
                e.section = Some(SectionId(1));
                e.nodes = [NodeId(level * 4 + a), NodeId(level * 4 + b)]
                    .into_iter()
                    .collect();
                e.local_axis.ref_vector = [0.0, 0.0, 1.0];
                model.elements.push(e);
            }
        }
        for e in &mut model.elements {
            e.kind = ElementKind::Beam;
        }
        let mut upper_material = model.materials[0].clone();
        upper_material.id = MaterialId(1);
        upper_material.fy = Some(20.0);
        model.materials.push(upper_material);
        for id in 0..2 {
            let mut s = model.sections[id].clone();
            s.id = SectionId((id + 2) as u32);
            s.material = Some(MaterialId(1));
            model.sections.push(s);
        }
        for e in &mut model.elements {
            if e.nodes
                .iter()
                .all(|id| model.nodes[id.index()].coord[2] >= 3000.0)
            {
                e.section = Some(SectionId(e.section.unwrap().0 + 2));
            }
        }
        for n in model.nodes.iter_mut().filter(|n| n.coord[2] > 0.0) {
            let m = if n.coord[2] < 4000.0 { 5.0 } else { 2.5 };
            n.mass = Some([m, m, m, 0.0, 0.0, 0.0]);
        }
        model.load_cases = sepika_core::model::default_load_cases();
        model.combinations = sepika_core::model::default_combinations();
        model.ultimate_gravity_combination = Some(0);
        let mut cfg = crate::AnalysisSettings {
            push_apply_long_term: false,
            push_steps: 100,
            push_drift_denom: 75.0,
            th_apply_long_term: false,
            ..Default::default()
        };
        let generated = crate::auto_loads::compute_seismic_auto_load_cases(&model, &cfg, None);
        crate::auto_loads::apply_auto_load_cases(&mut model, &generated.cases);
        let ex_id = model
            .load_cases
            .iter()
            .find(|c| c.name == "EX")
            .map(|c| c.id)
            .unwrap_or(LoadCaseId(4));
        let res = sepika_solver::statics::linear::linear_static_once(&model, ex_id).unwrap();
        let mut inp = input(&model, &res, SeismicDir::X);
        let bending_model = build(inp).unwrap();
        inp.model_type = LumpedMassType::EquivalentShear;
        let shear_model = super::super::build_lumped_mass(inp).unwrap();
        let q = signed_shears(&model, &res, SeismicDir::X).unwrap();
        let d = story_drifts(&model, &res, SeismicDir::X);
        let mut matrix = vec![0.0; 16];
        for (i, story) in bending_model.stories.iter().enumerate() {
            let local = bending_shear_element(
                story.height,
                bending_model.bending[i].ei_y,
                story.skeleton.k1,
            )
            .unwrap();
            let map = if i == 0 {
                [None, None, Some(0), Some(1)]
            } else {
                [Some(0), Some(1), Some(2), Some(3)]
            };
            for a in 0..4 {
                for b in 0..4 {
                    if let (Some(a_global), Some(b_global)) = (map[a], map[b]) {
                        matrix[a_global * 4 + b_global] += local[a][b];
                    }
                }
            }
            near(q[i] / shear_model.stories[i].skeleton.k1, d[i]);
        }
        let u = sepika_math::dense_lu::CheckedDenseLu::factorize(&matrix, 4)
            .unwrap()
            .solve(&[q[0] - q[1], 0.0, q[1], 0.0], 1)
            .unwrap();
        near(u[0], d[0]);
        near(u[2] - u[0], d[1]);
        let fem = crate::compute::compute_eigen(model.clone(), 6).unwrap();
        let bs = lumped_mass_eigen(&bending_model, 4).unwrap();
        let ss = lumped_mass_eigen(&shear_model, 2).unwrap();
        println!("FRAME static Q={q:?} d={d:?}; FEM T={:?}; shear T={:?} modes={:?}; bending T={:?} modes={:?}",fem.period,ss.period,ss.shapes,bs.period,bs.shapes);
        for (i, shape) in fem.node_shapes.iter().enumerate() {
            println!(
                "FRAME mode {} T={} effective mass {:?} X-floor-ratio={} Y-floor-ratio={}",
                i + 1,
                fem.period[i],
                fem.effective_mass[i],
                shape[4][0] / shape[8][0],
                shape[4][1] / shape[8][1]
            );
        }
        let ey_id = model.load_cases.iter().find(|c| c.name == "EY").unwrap().id;
        let ey = sepika_solver::statics::linear::linear_static_once(&model, ey_id).unwrap();
        for dir in [SeismicDir::X, SeismicDir::Y] {
            let mut spatial_input = input(&model, &res, dir);
            spatial_input.res_y = Some(&ey);
            spatial_input.dim = StickDim::Spatial;
            let spatial = build(spatial_input).unwrap();
            let reference = if dir == SeismicDir::X { &res } else { &ey };
            let qref = signed_shears(&model, reference, dir).unwrap();
            let ei = ei_values(&model, reference, dir).unwrap();
            let db = bending_drifts(&[3000.0; 2], &ei, &qref).unwrap();
            for (i, layer) in model.layers().iter().enumerate() {
                let total = story_drift_at(
                    &model,
                    reference,
                    dir,
                    layer,
                    spatial.spatial[i].rigidity_xy,
                );
                near(qref[i] / spatial.stories[i].skeleton.k1 + db[i], total);
            }
        }
        let po = crate::compute::compute_pushover(model.clone(), cfg).unwrap();
        println!(
            "FRAME pushover points={} end={:?}",
            po.capacity_curve.len(),
            po.termination
        );
        inp.model_type = LumpedMassType::EquivalentBendingShear;
        inp.nonlinear = true;
        inp.po_x = Some(&po);
        let failure = build(inp).unwrap_err().to_string();
        assert!(failure.contains("第1折点の割線交点が未同定"), "{failure}");
        let mut plastic_frame = model.clone();
        plastic_frame.elements.retain(|e| {
            e.nodes
                .iter()
                .all(|id| model.nodes[id.index()].coord[2] <= 3000.0)
        });
        for (i, e) in plastic_frame.elements.iter_mut().enumerate() {
            e.id = ElemId(i as u32);
        }
        plastic_frame.nodes.truncate(8);
        plastic_frame.stories.truncate(2);
        plastic_frame.constraints.truncate(2);
        plastic_frame.load_cases = sepika_core::model::default_load_cases();
        let generated =
            crate::auto_loads::compute_seismic_auto_load_cases(&plastic_frame, &cfg, None);
        crate::auto_loads::apply_auto_load_cases(&mut plastic_frame, &generated.cases);
        let plastic_res =
            sepika_solver::statics::linear::linear_static_once(&plastic_frame, ex_id).unwrap();
        let plastic_po = crate::compute::compute_pushover(plastic_frame.clone(), cfg).unwrap();
        let mut plastic_input = input(&plastic_frame, &plastic_res, SeismicDir::X);
        plastic_input.nonlinear = true;
        plastic_input.po_x = Some(&plastic_po);
        let nonlinear = build(plastic_input).unwrap();
        for steps in [25, 50, 100] {
            let mut sensitivity_cfg = cfg;
            sensitivity_cfg.push_steps = steps;
            let sensitivity_po =
                crate::compute::compute_pushover(plastic_frame.clone(), sensitivity_cfg).unwrap();
            for target in [1.0 / 250.0, 1.0 / 200.0, 1.0 / 150.0] {
                let sensitivity_input = LumpedMassBuildInput {
                    po_x: Some(&sensitivity_po),
                    target_drift_angle: target,
                    ..plastic_input
                };
                let identified = build(sensitivity_input).unwrap();
                let fit = identified.identifications[0].fit;
                near(fit.skeleton.k1, nonlinear.stories[0].skeleton.k1);
                println!("SENSITIVITY steps={steps} R={target} dy={} Kp={} interval={:?} Darea={} Dfit={} error={}",fit.skeleton.d2,fit.skeleton.k3(),fit.interval,fit.options.area_limit,fit.options.identification_limit,fit.normalized_rmse);
            }
        }

        println!("FRAME identified {:?}", nonlinear.identifications);
        let wave: Vec<_> = (0..1001)
            .map(|i| 35000.0 * (i as f64 * 0.002 * std::f64::consts::TAU / 0.11).sin())
            .collect();
        let response = sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
            &nonlinear, &wave, 0.002, 0.02,
        )
        .unwrap();
        println!(
            "FRAME nonlinear μ={:?}, total μ={:?}, peak drift={:?}",
            response.story_ductility, response.story_total_ductility, response.story_peak_drift
        );
        assert!(response.story_ductility.iter().any(|x| *x > 1.0));
        plastic_input.model_type = LumpedMassType::EquivalentShear;
        let nonlinear_shear = super::super::build_lumped_mass(plastic_input).unwrap();
        let shear_response = sepika_solver::dynamic::lumped_mass::lumped_mass_time_history(
            &nonlinear_shear,
            &wave,
            0.002,
            0.02,
        )
        .unwrap();
        cfg.th_nonlinear = true;
        let fem_response = crate::compute::compute_time_history(
            plastic_frame.clone(),
            cfg,
            sepika_solver::dynamic::timehistory::GroundMotion {
                dt: 0.002,
                accel_x: wave,
                accel_y: None,
                accel_theta: None,
            },
        )
        .unwrap();
        println!(
            "FRAME dynamic FEM peak={} drift_angle={:?}; shear peak={:?} μ={:?}",
            fem_response.peak_disp[4][0],
            fem_response.story_drift_angle,
            shear_response.story_peak_drift,
            shear_response.story_ductility
        );
        assert!(fem_response.peak_disp[4][0] > 15.0);
    }

    #[test]
    fn column_axial_work_recovers_ei_axes_units_and_split_columns() {
        for dir in [SeismicDir::X, SeismicDir::Y] {
            for reverse in [false, true] {
                for split in [false, true] {
                    let (model, res) = fixture(dir, reverse, split);
                    let expected = if dir == SeismicDir::X { 1.2e14 } else { 4.8e14 };
                    for ei in ei_values(&model, &res, dir).unwrap() {
                        near(ei, expected);
                    }
                    assert_eq!(
                        signed_shears(&model, &res, dir).unwrap(),
                        vec![2000.0, 1000.0]
                    );
                    let lm = build(input(&model, &res, dir)).unwrap();
                    near(lm.stories[0].skeleton.k1, 10000.0);
                    near(lm.stories[1].skeleton.k1, 10000.0);
                    let mut opposite = res.clone();
                    for q in opposite.story_shears.as_mut().unwrap().iter_mut().flatten() {
                        *q = -*q;
                    }
                    for u in &mut opposite.disp {
                        for v in u {
                            *v = -*v;
                        }
                    }
                    for (_, f) in &mut opposite.member_forces {
                        for (_, v) in &mut f.at {
                            for x in v {
                                *x = -*x;
                            }
                        }
                    }
                    for ei in ei_values(&model, &opposite, dir).unwrap() {
                        near(ei, expected);
                    }
                    let lm2 = build(input(&model, &opposite, dir)).unwrap();
                    near(lm2.stories[0].skeleton.k1, 10000.0);
                }
            }
        }
    }

    fn synthetic_pushover(
        model: &Model,
        res: &StaticOnce,
        dir: SeismicDir,
        spatial: bool,
    ) -> (PushoverResult, f64) {
        use sepika_solver::nonlinear::pushover::{CapacityPoint, PushoverStep, PushoverTarget};
        let qel = signed_shears(model, res, dir).unwrap();
        let ei = ei_values(model, res, dir).unwrap();
        let db = bending_drifts(&[3000.0; 2], &ei, &qel).unwrap();
        let del = story_drifts(model, res, dir);
        let ds: Vec<_> = del.iter().zip(&db).map(|(d, b)| d - b).collect();
        let axis = usize::from(dir == SeismicDir::Y);
        let mut po = super::super::tests::empty_pushover();
        for (index, (x, y)) in [
            (0.0, 0.0),
            (1.0, 1.0),
            (3.0, 1.4),
            (6.0, 1.55),
            (10.0, 1.75),
        ]
        .into_iter()
        .enumerate()
        {
            let drift: Vec<_> = (0..2)
                .map(|i| [2.0, 3.0][i] + x * ds[i] + y * db[i])
                .collect();
            let floors = [0.0, drift[0], drift.iter().sum()];
            let mut floor_disp = Vec::new();
            for (j, story) in model.stories.iter().enumerate() {
                let nid = story_reference_node(model, story).unwrap();
                let xy = model.nodes[nid.index()].coord;
                let theta = if j == 0 {
                    0.0
                } else {
                    0.0001 * j as f64 + 0.0002 * y
                };
                let mut u = [0.0; 3];
                u[axis] = floors[j];
                u[0] -= xy[1] * theta;
                u[1] += xy[0] * theta;
                u[2] = theta;
                floor_disp.push(u);
            }
            let recorded = if spatial {
                vec![
                    floor_disp[1][axis] - floor_disp[0][axis],
                    floor_disp[2][axis] - floor_disp[1][axis],
                ]
            } else {
                drift.clone()
            };
            let q: Vec<_> = (0..2).map(|i| [100.0, 200.0][i] + y * qel[i]).collect();
            po.capacity_curve.push(CapacityPoint {
                step: index as u32,
                roof_disp: floors[2],
                base_shear: q[0],
                story_shear: q,
                story_drift: recorded.clone(),
            });
            po.steps.push(PushoverStep {
                load_factor: y,
                top_disp: floors[2],
                base_shear: po.capacity_curve.last().unwrap().base_shear,
                story_drifts: recorded,
                floor_disp: if spatial { floor_disp } else { Vec::new() },
            });
        }
        po.target = Some(PushoverTarget {
            max_disp: Some(po.capacity_curve.last().unwrap().roof_disp),
            max_drift_angle: None,
        });
        let target = (0..2)
            .map(|i| 10.0 * ds[i] + 1.75 * db[i])
            .fold(f64::INFINITY, f64::min)
            * 0.8
            / 3000.0;
        (po, target)
    }

    #[test]
    fn nonlinear_backbone_preserves_elastic_stiffness_and_subtracts_baseline() {
        let (model, res) = fixture(SeismicDir::X, false, false);
        let (po, target) = synthetic_pushover(&model, &res, SeismicDir::X, false);
        let mut inp = input(&model, &res, SeismicDir::X);
        inp.nonlinear = true;
        inp.po_x = Some(&po);
        inp.target_drift_angle = target;
        let lm = build(inp).unwrap();
        for story in &lm.stories {
            near(story.skeleton.k1, 10000.0);
            assert!(story.skeleton.d2 > story.skeleton.d1);
        }
        near(lm.stories[0].skeleton.d3, 2.0);
        near(lm.stories[1].skeleton.d3, 1.0);
        let r = sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
            &lm,
            &[0.0, 1.0, -1.0, 0.0],
            0.01,
            0.02,
        )
        .unwrap();
        assert_eq!(r.time.len(), 4);
        assert!(r.story_total_ductility.is_some());
    }

    #[test]
    fn invalid_axial_results_and_nonpositive_shear_deformation_report_story() {
        let (model, mut res) = fixture(SeismicDir::X, false, false);
        res.member_forces.clear();
        let err = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(err.contains("柱") && err.contains("軸力"), "{err}");
        let (model, mut res) = fixture(SeismicDir::X, false, false);
        for u in &mut res.disp {
            u[0] = 0.0;
        }
        let err = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(err.contains("階") && err.contains("δs"), "{err}");
    }
    #[test]
    fn inclined_columns_are_rejected_before_axial_work_is_used() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        model.nodes[4].coord[0] += 1.0;
        let error = ei_values(&model, &res, SeismicDir::X)
            .unwrap_err()
            .to_string();
        assert!(error.contains("傾斜柱") && error.contains("階"), "{error}");
    }
    #[test]
    fn unsupported_wall_is_rejected_in_every_dimension_and_linearity() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        let mut wall = model.elements[0].clone();
        wall.id = ElemId(model.elements.len() as u32);
        wall.kind = sepika_core::model::ElementKind::Wall;
        wall.nodes = [NodeId(0), NodeId(1), NodeId(4), NodeId(5)]
            .into_iter()
            .collect();
        model.elements.push(wall);
        for dim in [StickDim::Planar, StickDim::Spatial] {
            for nonlinear in [false, true] {
                let mut inp = input(&model, &res, SeismicDir::X);
                inp.dim = dim;
                inp.nonlinear = nonlinear;
                let error = build(inp).unwrap_err().to_string();
                assert!(
                    error.contains("階") && error.contains("対象外"),
                    "{dim:?}/{nonlinear}: {error}"
                );
            }
        }
    }
    #[test]
    fn nonlinear_spatial_skeleton_is_independent_of_rigid_floor_master_with_twist() {
        let (mut model, ex) = fixture(SeismicDir::X, false, false);
        let (_, ey) = fixture(SeismicDir::Y, false, false);
        let mut originals = None;
        for master_slot in [0, 3] {
            for (level, constraint) in model.constraints.iter_mut().enumerate() {
                if let Constraint::RigidDiaphragm { master, slaves, .. } = constraint {
                    *master = NodeId((level * 4 + master_slot) as u32);
                    *slaves = (0..4)
                        .filter(|k| *k != master_slot)
                        .map(|k| NodeId((level * 4 + k) as u32))
                        .collect();
                }
            }
            let (px, target_x) = synthetic_pushover(&model, &ex, SeismicDir::X, true);
            let (py, target_y) = synthetic_pushover(&model, &ey, SeismicDir::Y, true);
            let mut inp = input(&model, &ex, SeismicDir::X);
            inp.dim = StickDim::Spatial;
            inp.nonlinear = true;
            inp.res_y = Some(&ey);
            inp.po_x = Some(&px);
            inp.po_y = Some(&py);
            inp.target_drift_angle = target_x.min(target_y);
            let lm = build(inp).unwrap();
            near(lm.spatial[0].skeleton_x.k1, 10000.0);
            near(lm.spatial[0].skeleton_y.k1, 10000.0);
            near(lm.spatial[1].skeleton_x.k1, 10000.0);
            near(lm.spatial[1].skeleton_y.k1, 10000.0);
            if let Some(ref original) = originals {
                let original: &LumpedMassModel = original;
                for (left, right) in lm.spatial.iter().zip(&original.spatial) {
                    for (a, b) in [
                        (left.skeleton_x, right.skeleton_x),
                        (left.skeleton_y, right.skeleton_y),
                    ] {
                        for (x, y) in [a.k1, a.d1, a.q1, a.d2, a.q2, a.d3, a.q3]
                            .into_iter()
                            .zip([b.k1, b.d1, b.q1, b.d2, b.q2, b.d3, b.q3])
                        {
                            near(x, y);
                        }
                    }
                }
            } else {
                originals = Some(lm.clone());
            }
            assert!(sepika_solver::dynamic::lumped_mass::lumped_mass_eigen(&lm, 10).is_ok());
            let response = sepika_solver::dynamic::lumped_mass::bending_shear_time_history(
                &lm,
                &[0.0, 1.0, -1.0, 0.0],
                0.01,
                0.02,
            )
            .unwrap();
            assert_eq!(response.time.len(), 4);
        }
    }

    #[test]
    fn columns_missing_intermediate_floor_or_branching_cannot_drop_out_of_ei() {
        for inclined in [false, true] {
            let (mut model, res) = fixture(SeismicDir::X, false, false);
            model
                .elements
                .retain(|e| e.id != ElemId(0) && e.id != ElemId(4));
            let mut spanning = model.elements[0].clone();
            spanning.id = ElemId(8);
            spanning.nodes = [NodeId(0), NodeId(if inclined { 9 } else { 8 })]
                .into_iter()
                .collect();
            model.elements.push(spanning);
            let error = ei_values(&model, &res, SeismicDir::X)
                .unwrap_err()
                .to_string();
            assert!(error.contains("階") && error.contains("柱 8"), "{error}");
            assert!(
                error.contains(if inclined {
                    "傾斜柱"
                } else {
                    "追跡できません"
                }),
                "{error}"
            );
        }
        let (mut model, res) = fixture(SeismicDir::X, false, true);
        let mut branch = model.elements[0].clone();
        branch.id = ElemId(model.elements.len() as u32);
        branch.nodes = [NodeId(12), NodeId(1)].into_iter().collect();
        model.elements.push(branch);
        let error = ei_values(&model, &res, SeismicDir::X)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("階") && error.contains("中間分岐"),
            "{error}"
        );
    }

    #[test]
    fn generated_panel_zone_connectivity_is_not_a_physical_story_crossing() {
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        let mut panel = model.elements[0].clone();
        panel.id = ElemId(model.elements.len() as u32);
        panel.kind = sepika_core::model::ElementKind::PanelZone;
        panel.section = None;
        panel.nodes = [NodeId(4), NodeId(0), NodeId(5)].into_iter().collect();
        model.elements.push(panel);
        assert_eq!(
            signed_shears(&model, &res, SeismicDir::X).unwrap(),
            vec![2000.0, 1000.0]
        );
        let lm = build(input(&model, &res, SeismicDir::X)).unwrap();
        near(lm.stories[0].skeleton.k1, 10000.0);
    }
    #[test]
    fn elastic_shear_curve_does_not_generate_a_yield_point() {
        let (model, res) = fixture(SeismicDir::X, false, false);
        let (mut po, target) = synthetic_pushover(&model, &res, SeismicDir::X, false);
        for (cp, step) in po.capacity_curve.iter_mut().zip(&mut po.steps) {
            let x = step.load_factor;
            let q = vec![2000.0 * x, 1000.0 * x];
            let db = bending_drifts(&[3000.0; 2], &[1.2e14; 2], &q).unwrap();
            cp.story_drift = vec![2.0 + 0.2 * x + db[0], 3.0 + 0.1 * x + db[1]];
            cp.story_shear = vec![100.0 + q[0], 200.0 + q[1]];
            step.story_drifts = cp.story_drift.clone();
        }
        po.target = Some(sepika_solver::nonlinear::pushover::PushoverTarget {
            max_disp: None,
            max_drift_angle: None,
        });
        let mut inp = input(&model, &res, SeismicDir::X);
        inp.nonlinear = true;
        inp.po_x = Some(&po);
        inp.target_drift_angle = target * 0.1;
        let error = build(inp).unwrap_err().to_string();
        assert!(error.contains("等価降伏未同定"), "{error}");
    }

    #[test]
    fn out_of_scope_elements_are_rejected_without_crossing_story_midpoint() {
        use sepika_core::model::ElementKind;
        for kind in [
            ElementKind::NodalSpring,
            ElementKind::Isolator,
            ElementKind::Damper,
            ElementKind::Wall,
            ElementKind::Shell,
        ] {
            for geometry in 0..3 {
                let (mut model, res) = fixture(SeismicDir::X, false, false);
                let mut added = model.elements[0].clone();
                added.id = ElemId(model.elements.len() as u32);
                added.kind = kind;
                added.section = None;
                let mut node = model.nodes[4].clone();
                node.id = NodeId(model.nodes.len() as u32);
                node.coord[2] = if geometry < 2 { 3000.0 } else { 2400.0 };
                if geometry == 1 {
                    node.coord[0] += 100.0;
                }
                let other = node.id;
                model.nodes.push(node);
                added.nodes = [NodeId(4), other].into_iter().collect();
                if matches!(kind, ElementKind::Wall | ElementKind::Shell) {
                    added.nodes.extend([NodeId(5), NodeId(6)]);
                }
                let id = added.id;
                model.elements.push(added);
                for dim in [StickDim::Planar, StickDim::Spatial] {
                    for nonlinear in [false, true] {
                        let mut inp = input(&model, &res, SeismicDir::X);
                        inp.dim = dim;
                        inp.nonlinear = nonlinear;
                        let error = build(inp).unwrap_err().to_string();
                        assert!(
                            error.contains("階")
                                && error.contains(&format!("部材 {}", id.0))
                                && error.contains("対象外"),
                            "{kind:?}/{geometry}/{dim:?}/{nonlinear}: {error}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn active_node_support_springs_are_rejected_but_zero_or_fixed_components_are_inactive() {
        use sepika_core::dof::Dof6Mask;
        let (mut model, res) = fixture(SeismicDir::X, false, false);
        for component in 0..6 {
            model.nodes[4].restraint = Dof6Mask::FREE;
            let mut stiffness = [0.0; 6];
            stiffness[component] = 1000.0;
            model.nodes[4].support_spring = Some(stiffness);
            let error = build(input(&model, &res, SeismicDir::X))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("階") && error.contains("節点 4") && error.contains("支持ばね"),
                "{error}"
            );
        }
        model.nodes[4].support_spring = Some([0.0; 6]);
        model.nodes[0].support_spring = Some([1000.0; 6]);
        assert!(build(input(&model, &res, SeismicDir::X)).is_ok());
        model.nodes[0].restraint = Dof6Mask::FREE;
        let error = build(input(&model, &res, SeismicDir::X))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("節点 0") && error.contains("支持ばね"),
            "{error}"
        );
    }

    #[test]
    fn horizontal_ground_restraints_above_base_are_rejected_in_the_modelled_directions() {
        use sepika_core::dof::{Dof, Dof6Mask};
        let (mut model, res) = fixture(SeismicDir::X, false, true);
        for node in [4_usize, 12] {
            model.nodes[node].restraint.set_fixed(Dof::Ux);
            let error = build(input(&model, &res, SeismicDir::X))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("階")
                    && error.contains(&format!("節点 {}", node))
                    && error.contains("基部以外"),
                "{error}"
            );
            model.nodes[node].restraint = Dof6Mask::FREE;
        }
        model.nodes[4].restraint.set_fixed(Dof::Uy);
        assert!(validate_scope(&input(&model, &res, SeismicDir::X)).is_ok());
        let mut spatial = input(&model, &res, SeismicDir::X);
        spatial.dim = StickDim::Spatial;
        let error = validate_scope(&spatial).unwrap_err().to_string();
        assert!(
            error.contains("節点 4") && error.contains("基部以外"),
            "{error}"
        );
        assert!(validate_scope(&input(&model, &res, SeismicDir::Y)).is_err());
    }
    #[test]
    fn support_at_virtual_floor_master_and_other_constraint_nodes_is_rejected() {
        use sepika_core::dof::{Dof, Dof6Mask};
        for connection in 0..4 {
            for spring in [false, true] {
                let (mut model, res) = fixture(SeismicDir::X, false, false);
                let mut node = model.nodes[4].clone();
                node.id = NodeId(model.nodes.len() as u32);
                node.coord = [0.0, 0.0, 3000.0];
                node.restraint = Dof6Mask::FREE;
                if spring {
                    node.support_spring = Some([1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
                } else {
                    node.restraint.set_fixed(Dof::Ux);
                }
                let id = node.id;
                model.nodes.push(node);
                match connection {
                    0 => {
                        model.constraints[1] = Constraint::rigid_diaphragm(
                            StoryId(1),
                            id,
                            vec![NodeId(4), NodeId(5), NodeId(6), NodeId(7)],
                        )
                    }
                    1 => model.constraints.push(Constraint::RigidLink {
                        master: id,
                        slaves: vec![NodeId(4)],
                        dofs: Dof6Mask::FIXED,
                    }),
                    2 => model.constraints.push(Constraint::Mpc {
                        master: NodeId(4),
                        terms: vec![(NodeId(4), Dof::Ux, 1.0), (id, Dof::Ux, -1.0)],
                    }),
                    3 => {
                        if let Constraint::RigidDiaphragm { slaves, .. } = &mut model.constraints[1]
                        {
                            slaves.push(id);
                        }
                    }
                    _ => unreachable!(),
                }
                let error = build(input(&model, &res, SeismicDir::X))
                    .unwrap_err()
                    .to_string();
                assert!(
                    error.contains("階")
                        && error.contains(&format!("節点 {}", id.0))
                        && error.contains(if spring {
                            "支持ばね"
                        } else {
                            "基部以外"
                        }),
                    "{connection}/{spring}: {error}"
                );
            }
        }
    }
}
