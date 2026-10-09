use super::*;
use crate::error::CoreError;
use crate::geom::{polygon, vec3};

/// 取付き線上の支持区間。`span` は梁始端からの無次元位置を取付き線の向きに並べる。
#[derive(Clone, Debug)]
pub struct AttachedSlabSupport {
    pub elem: crate::ids::ElemId,
    pub span: [f64; 2],
    /// 取付き線全長に対する支持区間の長さの割合（無次元）。
    pub fraction: f64,
}

impl Model {
    /// 有限で正面積の取付き線を一意に覆う梁区間を返す。
    /// 不正な幾何・参照、支持の欠落または正長の重複は床板 ID と理由付きで拒否する。
    pub fn attached_slab_supports(
        &self,
        slab: &Slab,
    ) -> Result<Vec<AttachedSlabSupport>, CoreError> {
        self.validate_attached_slab_geometry(slab)?;
        let fail = |reason: &str| CoreError::InvalidInput(format!("Slab {}: {reason}", slab.id.0));
        let SlabShape::Attached {
            anchor: RegionAnchor::Line { span, .. },
            ..
        } = slab.shape
        else {
            return Err(fail("線取付き床板ではありません"));
        };
        if !span_is_valid(span) {
            return Err(fail(
                "取付き線の区間 span が不正（0.0 <= t_i < t_j <= 1.0 であること）",
            ));
        }
        let boundary = slab
            .boundary_coords(self)
            .ok_or_else(|| fail("取付き線を生成できません"))?;
        let (Some(&p0), Some(&p1)) = (boundary.first(), boundary.get(1)) else {
            return Err(fail("取付き線がゼロ長です"));
        };
        let delta = vec3::sub(p1, p0);
        let len = vec3::norm(delta);
        if !len.is_finite() || len <= vec3::ZERO_TOL {
            return Err(fail("取付き線が非有限またはゼロ長です"));
        }
        let direction = vec3::scale(delta, 1.0 / len);
        let mut intervals = Vec::new();
        for elem in &self.elements {
            if elem.kind != ElementKind::Beam || elem.nodes.len() != 2 {
                continue;
            }
            let (Some(a), Some(b)) = (
                self.nodes.get(elem.nodes[0].index()),
                self.nodes.get(elem.nodes[1].index()),
            ) else {
                continue;
            };
            if !a.coord.iter().chain(b.coord.iter()).all(|x| x.is_finite()) {
                continue;
            }
            let along = |p| vec3::dot(vec3::sub(p, p0), direction);
            let near = |p| {
                vec3::norm(vec3::sub(
                    vec3::sub(p, p0),
                    vec3::scale(direction, along(p)),
                )) <= crate::geom::MEMBER_AXIS_TOL_MM
            };
            if !near(a.coord) || !near(b.coord) {
                continue;
            }
            let ta = along(a.coord);
            let tb = along(b.coord);
            let lo = ta.min(tb).max(0.0);
            let hi = ta.max(tb).min(len);
            if hi - lo <= vec3::ZERO_TOL {
                continue;
            }
            intervals.push((
                lo,
                hi,
                AttachedSlabSupport {
                    elem: elem.id,
                    span: [(lo - ta) / (tb - ta), (hi - ta) / (tb - ta)],
                    fraction: (hi - lo) / len,
                },
            ));
        }
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut reach = 0.0;
        for (lo, hi, _) in &intervals {
            if *lo < reach - vec3::ZERO_TOL {
                return Err(fail("取付き線の荷重支持先が複数あります"));
            }
            if *lo > reach + vec3::ZERO_TOL {
                return Err(fail("取付き線の荷重支持先が欠落しています"));
            }
            reach = *hi;
        }
        if (reach - len).abs() > vec3::ZERO_TOL {
            return Err(fail("取付き線の荷重支持先が欠落しています"));
        }
        Ok(intervals
            .into_iter()
            .map(|(_, _, support)| support)
            .collect())
    }
    /// 取り付く床板の幾何と荷重支持先を検査する。失敗時は床板 ID と理由を返す。
    pub fn validate_attached_slabs(&self) -> Result<(), CoreError> {
        for slab in &self.slabs {
            self.validate_attached_slab(slab)?;
        }
        Ok(())
    }

    fn validate_attached_slab_geometry(&self, slab: &Slab) -> Result<(), CoreError> {
        let SlabShape::Attached { anchor, extent } = slab.shape else {
            return Ok(());
        };
        let fail = |reason: &str| CoreError::InvalidInput(format!("Slab {}: {reason}", slab.id.0));
        if !extent.iter().all(|x| x.is_finite()) {
            return Err(fail("張り出し量 extent が非有限です"));
        }
        let nodes: Vec<_> = match anchor {
            RegionAnchor::Line { nodes, span, .. } => {
                if !span_is_valid(span) {
                    return Err(fail(
                        "取付き線の区間 span が不正（0.0 <= t_i < t_j <= 1.0 であること）",
                    ));
                }
                nodes.to_vec()
            }
            RegionAnchor::Point(node) => vec![node],
            RegionAnchor::FloorRegion { .. } => {
                return Err(fail("床領域は床板の取付き先に指定できません"))
            }
        };
        let mut coords = Vec::new();
        for id in nodes {
            let node = self
                .nodes
                .get(id.index())
                .filter(|n| n.id == id)
                .ok_or_else(|| fail(&format!("取付き先の節点 {} が存在しません", id.0)))?;
            if !node.coord.iter().all(|x| x.is_finite()) {
                return Err(fail("取付き先の節点座標が非有限です"));
            }
            coords.push(node.coord);
        }
        match anchor {
            RegionAnchor::Line { .. } => {
                if (coords[1][0] - coords[0][0]).hypot(coords[1][1] - coords[0][1])
                    <= vec3::ZERO_TOL
                {
                    return Err(fail("取付き線の XY 長さがゼロです"));
                }
                if extent[0] != 0.0
                    && extent[1] != 0.0
                    && extent[0].is_sign_positive() != extent[1].is_sign_positive()
                {
                    return Err(fail("逆符号の張り出し量によって境界が自己交差します"));
                }
            }
            RegionAnchor::Point(_) if extent.contains(&0.0) => {
                return Err(fail("点取付きの矩形面積がゼロです"))
            }
            _ => {}
        }
        let boundary = slab
            .boundary_coords(self)
            .ok_or_else(|| fail("境界座標を生成できません"))?;
        if !boundary.iter().flatten().all(|x| x.is_finite()) {
            return Err(fail("生成境界座標が非有限です"));
        }
        if boundary.len() < 3 {
            return Err(fail("生成境界の面積がゼロまたは縮退しています"));
        }
        let origin = boundary[0];
        let xy: Vec<_> = boundary
            .iter()
            .map(|p| [p[0] - origin[0], p[1] - origin[1]])
            .collect();
        let (lo, hi) = polygon::bounding_box(&xy);
        let scale = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        let area = polygon::area(&xy);
        if !area.is_finite() || area <= scale * scale * polygon::DEGENERATE_AREA_REL {
            return Err(fail("生成境界の面積がゼロまたは縮退しています"));
        }
        Ok(())
    }

    /// 取り付く床板の有限座標・正面積・支持先を検査する。囲まれた床板は対象外。
    pub fn validate_attached_slab(&self, slab: &Slab) -> Result<(), CoreError> {
        self.validate_attached_slab_geometry(slab)?;
        let SlabShape::Attached { anchor, .. } = slab.shape else {
            return Ok(());
        };
        let fail = |reason: &str| CoreError::InvalidInput(format!("Slab {}: {reason}", slab.id.0));
        if let RegionAnchor::Line {
            transfer: LoadTransfer::Anchor,
            ..
        } = anchor
        {
            self.attached_slab_supports(slab)?;
        } else {
            let node_ids: Vec<_> = match anchor {
                RegionAnchor::Point(node) => vec![node],
                RegionAnchor::Line { nodes, .. } => nodes.to_vec(),
                _ => Vec::new(),
            };
            for id in node_ids {
                if self.elements.iter().any(|e| e.nodes.contains(&id)) {
                    continue;
                }
                let point = self.nodes[id.index()].coord;
                let tol = crate::geom::MEMBER_AXIS_TOL_MM;
                let candidates = self
                    .elements
                    .iter()
                    .filter(|e| e.kind == ElementKind::Beam && e.nodes.len() == 2)
                    .filter(|e| {
                        let (Some(a), Some(b)) = (
                            self.nodes.get(e.nodes[0].index()),
                            self.nodes.get(e.nodes[1].index()),
                        ) else {
                            return false;
                        };
                        let ab = vec3::sub(b.coord, a.coord);
                        let length = vec3::norm(ab);
                        if !length.is_finite() || length <= 1.0 {
                            return false;
                        }
                        let along = vec3::dot(vec3::sub(point, a.coord), ab) / length;
                        let projected = vec3::add(a.coord, vec3::scale(ab, along / length));
                        along > tol && along < length - tol && vec3::dist(point, projected) <= tol
                    })
                    .count();
                if candidates != 1 {
                    return Err(fail(if candidates == 0 {
                        "集中荷重の支持先が欠落しています"
                    } else {
                        "集中荷重の支持先が複数あります"
                    }));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dof::Dof6Mask;
    fn model() -> Model {
        let mut model = Model::default();
        for (i, x) in [0.0, 4000.0].into_iter().enumerate() {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord: [x, 0.0, 0.0],
                restraint: Dof6Mask::FREE,
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        model.elements.push(ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed; 2],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            plastic_zone: None,
            spring: None,
        });
        model
    }
    fn slab(extent: [f64; 2]) -> Slab {
        Slab {
            id: SlabId(3),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.25, 0.75],
                    transfer: LoadTransfer::Anchor,
                },
                extent,
            },
            plate: SlabPlate::default(),
            tip_loads: Vec::new(),
        }
    }

    #[test]
    fn support_resolution_directly_rejects_unvalidated_geometry_and_references() {
        let base = model();
        for (span, extent) in [
            ([0.0, 0.0], [0.0, 0.0]),
            ([0.25, 0.75], [0.0, 0.0]),
            ([f64::NAN, 0.75], [1000.0; 2]),
            ([0.25, 0.75], [f64::INFINITY, 1000.0]),
            ([0.25, 0.75], [1e-12; 2]),
        ] {
            let mut slab = slab(extent);
            if let SlabShape::Attached {
                anchor: RegionAnchor::Line { span: value, .. },
                ..
            } = &mut slab.shape
            {
                *value = span;
            }
            assert_eq!(
                base.attached_slab_supports(&slab).unwrap_err(),
                base.validate_attached_slab(&slab).unwrap_err()
            );
        }
        for coordinate in [f64::NAN, f64::INFINITY, 0.0] {
            let mut model = base.clone();
            model.nodes[1].coord[0] = coordinate;
            let slab = slab([1000.0; 2]);
            assert_eq!(
                model.attached_slab_supports(&slab).unwrap_err(),
                model.validate_attached_slab(&slab).unwrap_err()
            );
        }
        let mut model = base;
        model.nodes.pop();
        let slab = slab([1000.0; 2]);
        assert_eq!(
            model.attached_slab_supports(&slab).unwrap_err(),
            model.validate_attached_slab(&slab).unwrap_err()
        );
    }

    #[test]
    fn unconnected_point_requires_exactly_one_actual_beam_receiver() {
        let mut model = model();
        model.nodes.push(Node {
            id: NodeId(2),
            coord: [2000.0, 0.0, 0.0],
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        });
        let mut point = slab([1000.0; 2]);
        point.shape = SlabShape::Attached {
            anchor: RegionAnchor::Point(NodeId(2)),
            extent: [1000.0; 2],
        };
        assert_eq!(model.validate_attached_slab(&point), Ok(()));
        let mut duplicate = model.elements[0].clone();
        duplicate.id = ElemId(1);
        model.elements.push(duplicate);
        assert!(model
            .validate_attached_slab(&point)
            .unwrap_err()
            .to_string()
            .contains("集中荷重の支持先が複数"));
        model.elements.clear();
        assert!(model
            .validate_attached_slab(&point)
            .unwrap_err()
            .to_string()
            .contains("集中荷重の支持先が欠落"));
    }
    #[test]
    fn span_coordinate_reference_and_point_area_checks_share_slab_diagnostic() {
        let base = model();
        for span in [
            [0.75, 0.25],
            [-0.1, 0.5],
            [0.5, 1.1],
            [f64::NAN, 0.5],
            [0.25, f64::INFINITY],
        ] {
            let mut slab = slab([1000.0; 2]);
            slab.shape = SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span,
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1000.0; 2],
            };
            assert!(base
                .validate_attached_slab(&slab)
                .unwrap_err()
                .to_string()
                .contains("Slab 3: 取付き線の区間 span"));
        }
        for coordinate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for axis in 0..3 {
                let mut model = base.clone();
                model.nodes[1].coord[axis] = coordinate;
                assert!(model
                    .validate_attached_slab(&slab([1000.0; 2]))
                    .unwrap_err()
                    .to_string()
                    .contains("節点座標が非有限"));
            }
        }
        let mut point = slab([1000.0; 2]);
        point.shape = SlabShape::Attached {
            anchor: RegionAnchor::Point(NodeId(0)),
            extent: [0.0, 1000.0],
        };
        assert!(base
            .validate_attached_slab(&point)
            .unwrap_err()
            .to_string()
            .contains("矩形面積がゼロ"));
        point.shape = SlabShape::Attached {
            anchor: RegionAnchor::Point(NodeId(99)),
            extent: [1000.0; 2],
        };
        assert!(base
            .validate_attached_slab(&point)
            .unwrap_err()
            .to_string()
            .contains("節点 99 が存在しません"));
        let mut near_bounds = slab([1000.0; 2]);
        near_bounds.shape = SlabShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [-0.5e-9, 1.0 + 0.5e-9],
                transfer: LoadTransfer::Anchor,
            },
            extent: [1000.0; 2],
        };
        assert_eq!(base.validate_attached_slab(&near_bounds), Ok(()));
    }
    #[test]
    fn valid_partial_trapezoids_and_triangles_preserve_area() {
        let model = model();
        for (extent, area, vertices) in [
            ([1000.0, 2000.0], 3000000.0, 4),
            ([-1000.0, -2000.0], 3000000.0, 4),
            ([0.0, 2000.0], 2000000.0, 3),
            ([2000.0, 0.0], 2000000.0, 3),
            ([0.0, -2000.0], 2000000.0, 3),
            ([-2000.0, 0.0], 2000000.0, 3),
        ] {
            let slab = slab(extent);
            assert_eq!(model.validate_attached_slab(&slab), Ok(()));
            let boundary = slab.boundary_coords(&model).unwrap();
            assert_eq!(boundary.len(), vertices);
            assert!((polygon::area_xy(&boundary) - area).abs() < 1e-6);
            let supports = model.attached_slab_supports(&slab).unwrap();
            assert_eq!(supports.len(), 1);
            assert_eq!(supports[0].elem, ElemId(0));
            assert_eq!(supports[0].span, [0.25, 0.75]);
        }
    }
    #[test]
    fn invalid_geometry_and_ambiguous_or_missing_support_are_diagnosed_with_slab_id() {
        let mut model = model();
        for extent in [
            [0.0, 0.0],
            [1000.0, -1000.0],
            [f64::NAN, 1000.0],
            [f64::INFINITY, 1000.0],
        ] {
            let error = model
                .validate_attached_slab(&slab(extent))
                .unwrap_err()
                .to_string();
            assert!(error.contains("Slab 3:"));
        }
        model.elements.clear();
        assert!(model
            .validate_attached_slab(&slab([1000.0; 2]))
            .unwrap_err()
            .to_string()
            .contains("欠落"));
        model = self::model();
        let mut duplicate = model.elements[0].clone();
        duplicate.id = ElemId(1);
        model.elements.push(duplicate);
        assert!(model
            .validate_attached_slab(&slab([1000.0; 2]))
            .unwrap_err()
            .to_string()
            .contains("複数"));
        model.elements.pop();
        model.nodes[1].coord = [0.0, 0.0, 4000.0];
        assert!(model
            .validate_attached_slab(&slab([1000.0; 2]))
            .unwrap_err()
            .to_string()
            .contains("XY 長さ"));
    }
    #[test]
    fn point_rectangles_and_distinct_slabs_on_same_beam_are_valid() {
        let mut model = model();
        for extent in [
            [1000.0, 2000.0],
            [-1000.0, 2000.0],
            [1000.0, -2000.0],
            [-1000.0, -2000.0],
        ] {
            let mut slab = slab(extent);
            slab.shape = SlabShape::Attached {
                anchor: RegionAnchor::Point(NodeId(0)),
                extent,
            };
            assert_eq!(model.validate_attached_slab(&slab), Ok(()));
        }
        let mut first = slab([1000.0; 2]);
        first.id = SlabId(0);
        let mut second = first.clone();
        second.id = SlabId(1);
        model.slabs = vec![first, second];
        assert_eq!(model.validate_attached_slabs(), Ok(()));
    }
}
