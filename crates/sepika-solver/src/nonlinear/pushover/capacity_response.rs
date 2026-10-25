use super::physical_response::PhysicalStep;
use crate::statics::analysis::SeismicDir;
use sepika_core::dof::DofMap;
use sepika_core::model::{MemberLoadKind, Model};

pub(super) struct CapacityForces {
    pub(super) base: f64,
    pub(super) story: Vec<f64>,
}

impl PhysicalStep {
    pub(super) fn capacity_forces(
        &self,
        model: &Model,
        dofmap: &DofMap,
        dir: SeismicDir,
    ) -> Result<CapacityForces, String> {
        let d = usize::from(dir == SeismicDir::Y);
        let mut end = vec![0.; model.nodes.len()];
        let mut fixed_nodal = 0.;
        for forces in &self.element_nodal {
            for &(node, values) in forces {
                end[node] += values[d];
            }
        }
        for load in &self.source.nodal {
            if self.fixed_dofs[load.node.index()][d] {
                fixed_nodal += load.values[d];
            }
        }
        let layers = model.layers();
        let highest = layers.last().map(|layer| layer.top_sl_mm);
        let mut member_story = vec![0.; layers.len()];
        for load in &self.source.member {
            let elem = model
                .element(load.elem)
                .ok_or("性能曲線の荷重要素がありません")?;
            let length = model.member_length(elem);
            let projection = load.dir[d] / sepika_core::geom::vec3::norm(load.dir);
            if let MemberLoadKind::Point { a, p } = load.kind {
                let endpoint = if a == 0. {
                    Some(0)
                } else if a == length {
                    Some(1)
                } else {
                    None
                };
                if let Some(endpoint) = endpoint {
                    let node = elem.nodes[endpoint].index();
                    end[node] += p * projection;
                    if self.fixed_dofs[node][d] {
                        fixed_nodal += p * projection;
                    }
                    continue;
                }
            }
            let zi = model.nodes[elem.nodes[0].index()].coord[2];
            let zj = model.nodes[elem.nodes[1].index()].coord[2];
            if let Some(highest) = highest {
                for layer in &layers {
                    member_story[layer.index] +=
                        integrate(&load.kind, length, zi, zj, (layer.bottom_sl_mm, highest))
                            * projection;
                }
            }
        }
        let base = -self.support.iter().map(|force| force[d]).sum::<f64>() - fixed_nodal;
        let mut included = vec![false; model.nodes.len()];
        let mut story = vec![0.; layers.len()];
        for layer in layers.iter().rev() {
            for node in &layer.node_ids {
                if dofmap.active(node.index() * 6 + d).is_some()
                    && !self.fixed_dofs[node.index()][d]
                {
                    included[node.index()] = true;
                }
            }
            story[layer.index] = member_story[layer.index]
                + included
                    .iter()
                    .enumerate()
                    .filter(|(_, include)| **include)
                    .map(|(node, _)| end[node] - self.support[node][d])
                    .sum::<f64>();
        }
        if !base.is_finite() || story.iter().any(|q| !q.is_finite()) {
            return Err("性能曲線の物理応答が非有限です".into());
        }
        Ok(CapacityForces { base, story })
    }
}

fn integrate(
    kind: &MemberLoadKind,
    length: f64,
    zi: f64,
    zj: f64,
    (bottom, top): (f64, f64),
) -> f64 {
    match *kind {
        MemberLoadKind::Point { a, p } => {
            let z = zi + (zj - zi) * (a / length);
            if bottom < z && z <= top {
                p
            } else {
                0.
            }
        }
        MemberLoadKind::Distributed { a, b, w1, w2 } => {
            let (lo, hi) = if zi == zj {
                if !(bottom < zi && zi <= top) {
                    return 0.;
                }
                (a, b)
            } else {
                let x1 = (bottom - zi) / (zj - zi) * length;
                let x2 = (top - zi) / (zj - zi) * length;
                (a.max(x1.min(x2)), b.min(x1.max(x2)))
            };
            if lo >= hi {
                return 0.;
            }
            let wlo = w1 + (w2 - w1) * ((lo - a) / (b - a));
            let whi = w1 + (w2 - w1) * ((hi - a) / (b - a));
            (hi - lo) * (wlo + whi) * 0.5
        }
    }
}
