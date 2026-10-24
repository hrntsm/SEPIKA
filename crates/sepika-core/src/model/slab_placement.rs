use super::*;

/// 床板の実配置。座標・実厚は mm、境界は上面の頂点順。
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSlabPlacement {
    pub slab: SlabId,
    pub reference_story: StoryId,
    pub top_z_mm: f64,
    pub bottom_z_mm: f64,
    pub thickness_mm: f64,
    pub top_boundary: Vec<[f64; 3]>,
    pub support_boundary: Vec<SupportBoundary>,
    pub attached_anchor: Option<RegionAnchor>,
}

impl Model {
    /// FLと個別偏心から床板実配置を解決する。欠落・参照切れ・不正値は床板ID付きエラー。
    pub fn resolved_slab_placement(&self, id: SlabId) -> Result<ResolvedSlabPlacement, String> {
        let fail = |field: &str, reason: &str| format!("床板 {}: {field}: {reason}", id.0);
        let slab = self
            .slab(id)
            .ok_or_else(|| fail("床板", "参照先がありません"))?;
        let reference_story = slab
            .reference_story
            .ok_or_else(|| fail("基準階", "未入力です"))?;
        let story = self
            .stories
            .iter()
            .find(|s| s.id == reference_story)
            .ok_or_else(|| fail("基準階", "参照先がありません"))?;
        let fl_mm = story
            .fl_mm()
            .ok_or_else(|| fail("FL", "未入力または非有限です"))?;
        let eccentricity_mm = slab
            .vertical_eccentricity_mm
            .filter(|v| v.is_finite())
            .ok_or_else(|| fail("鉛直偏心", "未入力または非有限です"))?;
        let section_id = slab.section().ok_or_else(|| fail("床断面", "未割当です"))?;
        let section = self
            .sections
            .iter()
            .find(|s| s.id == section_id)
            .ok_or_else(|| fail("床断面", "参照先がありません"))?;
        let thickness_mm = section
            .thickness
            .filter(|v| v.is_finite() && *v > 0.0)
            .ok_or_else(|| fail("実板厚", "未入力、非有限または0以下です"))?;
        let top_z_mm = fl_mm + eccentricity_mm;
        let bottom_z_mm = top_z_mm - thickness_mm;
        if !top_z_mm.is_finite() || !bottom_z_mm.is_finite() {
            return Err(fail("上面・下面Z", "導出座標が非有限です"));
        }
        let (mut top_boundary, support_boundary, attached_anchor) = match slab.shape {
            SlabShape::Enclosed => {
                let region = self
                    .slab_assignment_region(id)
                    .ok_or_else(|| fail("平面位置", "割当領域がありません"))?;
                let segments = self
                    .support_boundary_segments(&region.boundary)
                    .ok_or_else(|| fail("平面位置", "支持部材を解決できません"))?;
                if segments.is_empty()
                    || !segments.iter().enumerate().all(|(i, (_, b))| {
                        let a = segments[(i + 1) % segments.len()].0;
                        (a[0] - b[0]).abs() <= crate::geom::MEMBER_AXIS_TOL_MM
                            && (a[1] - b[1]).abs() <= crate::geom::MEMBER_AXIS_TOL_MM
                    })
                {
                    return Err(fail("平面位置", "支持区間のXY境界が閉じていません"));
                }
                let points = region
                    .boundary
                    .iter()
                    .map(|edge| {
                        if !edge
                            .span
                            .iter()
                            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
                        {
                            return None;
                        }
                        let (a, b) = self.support_member_axis(edge.support)?;
                        Some(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * edge.span[0]))
                    })
                    .collect::<Option<Vec<[f64; 3]>>>()
                    .ok_or_else(|| fail("平面位置", "支持部材・区間を解決できません"))?;
                (points, region.boundary.clone(), None)
            }
            SlabShape::Attached { anchor, .. } => {
                self.validate_attached_slab_geometry(slab)
                    .map_err(|reason| fail("平面位置", &reason.to_string()))?;
                (
                    slab.boundary_coords(self)
                        .ok_or_else(|| fail("平面位置", "取付き先・張り出し量を解決できません"))?,
                    Vec::new(),
                    Some(anchor),
                )
            }
        };
        if top_boundary.len() < 3 || !top_boundary.iter().flatten().all(|v| v.is_finite()) {
            return Err(fail("平面位置", "境界が不足または非有限です"));
        }
        for p in &mut top_boundary {
            p[2] = top_z_mm;
        }
        let xy: Vec<_> = top_boundary.iter().map(|p| [p[0], p[1]]).collect();
        crate::geom::polygon::triangulate_simple(&xy).map_err(|reason| fail("平面位置", reason))?;
        Ok(ResolvedSlabPlacement {
            slab: id,
            reference_story,
            top_z_mm,
            bottom_z_mm,
            thickness_mm,
            top_boundary,
            support_boundary,
            attached_anchor,
        })
    }

    /// 全床板の実配置を検査する。版なし・未設定領域は床板実体を持たない。
    pub fn validate_slab_placements(&self) -> Result<(), String> {
        for region in &self.floor_assignment_regions.regions {
            if let PlateAssignment::Plate(id) = region.assignment {
                self.resolved_slab_placement(id)?;
            }
        }
        for slab in &self.slabs {
            self.resolved_slab_placement(slab.id)?;
        }
        Ok(())
    }

    fn support_displacement(
        &self,
        support: SupportMemberId,
        t: f64,
        coord_of: &impl Fn(NodeId) -> Option<[f64; 3]>,
        visiting: &mut std::collections::HashSet<SecondaryMemberId>,
    ) -> Option<[f64; 3]> {
        let lerp =
            |a: [f64; 3], b: [f64; 3], t: f64| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
        match support {
            SupportMemberId::Primary(id) => {
                let e = self.element(id)?;
                let displacement = |id| {
                    let p = coord_of(id)?;
                    let original = self.node(id)?.coord;
                    Some(std::array::from_fn(|i| p[i] - original[i]))
                };
                Some(lerp(
                    displacement(*e.nodes.first()?)?,
                    displacement(*e.nodes.last()?)?,
                    t,
                ))
            }
            SupportMemberId::Secondary(id) => {
                if !visiting.insert(id) {
                    return None;
                }
                let SecondaryMemberEnds::Supported([a, b]) = self.secondary_member(id)?.ends else {
                    return None;
                };
                let da = self.support_displacement(a.support, a.position, coord_of, visiting)?;
                let db = self.support_displacement(b.support, b.position, coord_of, visiting)?;
                visiting.remove(&id);
                Some(lerp(da, db, t))
            }
        }
    }
}

impl Slab {
    /// 床上面の未変形境界 [mm]。実配置を解決できなければ `None`。
    pub fn surface_coords(&self, model: &Model) -> Option<Vec<[f64; 3]>> {
        Some(model.resolved_slab_placement(self.id).ok()?.top_boundary)
    }

    /// 床上面へ支持点の変位を一度だけ加えた表示境界 [mm]。未解決・非有限は `None`。
    pub fn surface_coords_with(
        &self,
        model: &Model,
        coord_of: impl Fn(NodeId) -> Option<[f64; 3]>,
    ) -> Option<Vec<[f64; 3]>> {
        let placement = model.resolved_slab_placement(self.id).ok()?;
        let mut points = placement.top_boundary;
        match self.shape {
            SlabShape::Enclosed => {
                for (p, edge) in points.iter_mut().zip(placement.support_boundary) {
                    let d = model.support_displacement(
                        edge.support,
                        edge.span[0],
                        &coord_of,
                        &mut Default::default(),
                    )?;
                    for i in 0..3 {
                        p[i] += d[i];
                    }
                }
            }
            SlabShape::Attached { .. } => {
                let original = self.boundary_coords(model)?;
                let display = self.boundary_coords_with(model, coord_of)?;
                if original.len() != points.len() || display.len() != points.len() {
                    return None;
                }
                for ((p, a), b) in points.iter_mut().zip(original).zip(display) {
                    for i in 0..3 {
                        p[i] += b[i] - a[i];
                    }
                }
            }
        }
        points
            .iter()
            .flatten()
            .all(|v| v.is_finite())
            .then_some(points)
    }
}
