use sepika_core::geom::vec3::{add, cross, dot, local_frame_axes, norm, scale, sub};
use sepika_core::model::FrameSectionUse;
use sepika_core::section_shape::{face_geometry::SectionGeometry, SectionShape};

#[derive(Clone, Copy, Default)]
pub(super) struct GlobalTransform {
    pub translation_mm: [f64; 3],
    pub rotation_deg: f64,
}

impl GlobalTransform {
    pub fn vector(self, vector: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = self.rotation_deg.to_radians().sin_cos();
        [
            cos * vector[0] - sin * vector[1],
            sin * vector[0] + cos * vector[1],
            vector[2],
        ]
    }

    pub fn point(self, point: [f64; 3]) -> [f64; 3] {
        add(self.vector(point), self.translation_mm)
    }
}

pub(super) fn import_placement(
    ends: [[f64; 3]; 2],
    offsets_mm: [[f64; 3]; 2],
    rotate_deg: f64,
    usage: FrameSectionUse,
    shape: &SectionShape,
) -> Result<([[f64; 3]; 2], [f64; 3]), String> {
    let axes = reference_axes(ends, rotate_deg, usage)?;
    if offsets_mm[0] != offsets_mm[1] {
        return Err(
            "両端同一offsetのみ対応しています。異値offsetでの材軸・断面姿勢の対応保持は未対応です"
                .into(),
        );
    }
    let shift = centroid_shift(shape, usage, axes)?;
    let offsets_mm = offsets_mm.map(|offset| add(offset, shift));
    let actual = [add(ends[0], offsets_mm[0]), add(ends[1], offsets_mm[1])];
    local_frame_axes(actual[0], actual[1], axes[1]).map_err(str::to_owned)?;
    Ok((offsets_mm, axes[1]))
}

pub(super) fn export_placement(
    ends: [[f64; 3]; 2],
    offsets_mm: [[f64; 3]; 2],
    reference: [f64; 3],
    usage: FrameSectionUse,
    shape: Option<&SectionShape>,
) -> Result<([[f64; 3]; 2], f64), String> {
    if offsets_mm[0] != offsets_mm[1] {
        return Err(
            "両端同一offsetのみ対応しています。異値offsetでの材軸・断面姿勢の対応保持は未対応です"
                .into(),
        );
    }
    let actual = [add(ends[0], offsets_mm[0]), add(ends[1], offsets_mm[1])];
    let axes = local_frame_axes(actual[0], actual[1], reference).map_err(str::to_owned)?;
    let shift = match shape {
        Some(shape) => centroid_shift(shape, usage, axes)?,
        None if usage == FrameSectionUse::Column => [0.0; 3],
        None => return Err("配置変換の断面形状がありません".into()),
    };
    let nominal = reference_axes(ends, 0.0, usage)?;
    let rotate_deg = dot(axes[1], nominal[2])
        .atan2(dot(axes[1], nominal[1]))
        .to_degrees()
        .rem_euclid(360.0);
    let offsets_mm = offsets_mm.map(|offset| sub(offset, shift));
    Ok((offsets_mm, rotate_deg))
}

pub(super) fn offset_attributes(offsets_mm: [[f64; 3]; 2], column: bool) -> String {
    let ends = if column {
        ["bottom", "top"]
    } else {
        ["start", "end"]
    };
    let mut attributes = String::new();
    for (offset, end) in offsets_mm.into_iter().zip(ends) {
        for (value, axis) in offset.into_iter().zip(["X", "Y", "Z"]) {
            attributes.push_str(&format!(" offset_{end}_{axis}=\"{value}\""));
        }
    }
    attributes
}

fn reference_axes(
    ends: [[f64; 3]; 2],
    rotate_deg: f64,
    usage: FrameSectionUse,
) -> Result<[[f64; 3]; 3], String> {
    if !ends.iter().flatten().all(|value| value.is_finite()) || !rotate_deg.is_finite() {
        return Err("基準端・断面回転に非有限値があります".into());
    }
    let corresponding = if usage == FrameSectionUse::Column {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let basis = local_frame_axes(ends[0], ends[1], corresponding).map_err(str::to_owned)?;
    if norm(sub(
        corresponding,
        scale(basis[0], dot(corresponding, basis[0])),
    )) <= 1e-12
    {
        return Err(format!(
            "基準材軸がSTB対応全体{}軸と平行で断面基底を確定できません",
            if usage == FrameSectionUse::Column {
                "Y"
            } else {
                "Z"
            }
        ));
    }
    let mut axes = if usage == FrameSectionUse::Column {
        let column = basis;
        [column[0], cross(column[1], column[0]), column[1]]
    } else {
        basis
    };
    let (sin, cos) = rotate_deg.to_radians().sin_cos();
    axes[1] = add(scale(axes[1], cos), scale(cross(axes[0], axes[1]), sin));
    axes[2] = cross(axes[0], axes[1]);
    if !axes.iter().flatten().all(|value| value.is_finite())
        || dot(sub(ends[1], ends[0]), sub(ends[1], ends[0])) == 0.0
    {
        return Err("基準材軸を有限な正長さで解決できません".into());
    }
    Ok(axes)
}

fn centroid_shift(
    shape: &SectionShape,
    usage: FrameSectionUse,
    axes: [[f64; 3]; 3],
) -> Result<[f64; 3], String> {
    if usage == FrameSectionUse::Column {
        return Ok([0.0; 3]);
    }
    let symmetric_depth_mm = match shape {
        SectionShape::SteelH { height, .. }
        | SectionShape::SteelBox { height, .. }
        | SectionShape::CftBox { height, .. } => Some(*height),
        SectionShape::SteelPipe { outer_dia, .. } | SectionShape::CftPipe { outer_dia, .. } => {
            Some(*outer_dia)
        }
        SectionShape::RcBeamRect { d, .. } | SectionShape::SrcBeamRect { d, .. } => Some(*d),
        SectionShape::SteelFlatBar { thick, .. } => Some(*thick),
        _ => None,
    };
    if let Some(depth_mm) = symmetric_depth_mm {
        if !depth_mm.is_finite() || depth_mm <= 0.0 {
            return Err("断面せいは有限な正値が必要です".into());
        }
        return Ok(if usage == FrameSectionUse::Brace {
            [0.0; 3]
        } else {
            scale(axes[1], -depth_mm / 2.0)
        });
    }
    let geometry = SectionGeometry::of(shape)?;
    let y = geometry.projected_range([1.0, 0.0])?;
    let z = geometry.projected_range([0.0, 1.0])?;
    let (reference_y, reference_z) = if usage == FrameSectionUse::Brace {
        ((y[0] + y[1]) / 2.0, (z[0] + z[1]) / 2.0)
    } else {
        (y[1], (z[0] + z[1]) / 2.0)
    };
    Ok(add(
        scale(axes[1], -reference_y),
        scale(axes[2], -reference_z),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle() -> SectionShape {
        SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 600.0,
        }
    }

    #[test]
    fn parallel_stb_corresponding_axis_rejects_both_directions_without_fallback() {
        for (usage, endpoint, axis) in [
            (FrameSectionUse::Column, [0.0, 3000.0, 0.0], "Y"),
            (FrameSectionUse::Girder, [0.0, 0.0, 3000.0], "Z"),
            (FrameSectionUse::Brace, [0.0, 0.0, 3000.0], "Z"),
        ] {
            let ends = [[0.0; 3], endpoint];
            let imported =
                import_placement(ends, [[0.0; 3]; 2], 0.0, usage, &rectangle()).unwrap_err();
            let exported = export_placement(
                ends,
                [[0.0; 3]; 2],
                [1.0, 0.0, 0.0],
                usage,
                Some(&rectangle()),
            )
            .unwrap_err();
            for reason in [imported, exported] {
                assert!(
                    reason.contains(&format!("全体{axis}軸")) && reason.contains("断面基底"),
                    "{reason}"
                );
            }
        }
    }

    #[test]
    fn omitted_offset_places_a_600_mm_deep_horizontal_beam_axis_at_2700_mm() {
        let ends = [[0.0, 0.0, 3000.0], [6000.0, 0.0, 3000.0]];
        let (offsets, reference) = import_placement(
            ends,
            [[0.0; 3]; 2],
            0.0,
            FrameSectionUse::Girder,
            &rectangle(),
        )
        .unwrap();
        assert_eq!(offsets, [[0.0, 0.0, -300.0]; 2]);
        assert_eq!(add(ends[0], offsets[0]), [0.0, 0.0, 2700.0]);
        assert_eq!(add(ends[1], offsets[1]), [6000.0, 0.0, 2700.0]);
        assert_eq!(reference, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn rotation_precedes_global_offset_and_column_reference_is_centroid() {
        let (offsets, reference) = import_placement(
            [[0.0; 3], [6000.0, 0.0, 0.0]],
            [[30.0, 40.0, 50.0]; 2],
            90.0,
            FrameSectionUse::Girder,
            &rectangle(),
        )
        .unwrap();
        for offset in offsets {
            assert!((offset[0] - 30.0).abs() < 1e-10);
            assert!((offset[1] - 340.0).abs() < 1e-10);
            assert!((offset[2] - 50.0).abs() < 1e-10);
        }
        assert!((reference[1] + 1.0).abs() < 1e-10);
        let (offsets, reference) = import_placement(
            [[0.0; 3], [0.0, 0.0, 3000.0]],
            [[30.0, 40.0, 50.0]; 2],
            0.0,
            FrameSectionUse::Column,
            &rectangle(),
        )
        .unwrap();
        assert_eq!(offsets, [[30.0, 40.0, 50.0]; 2]);
        assert_eq!(reference, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn asymmetric_brace_uses_envelope_center_instead_of_centroid() {
        let shape = SectionShape::SteelTee {
            height: 300.0,
            width: 200.0,
            web_thick: 10.0,
            flange_thick: 20.0,
        };
        let (offsets, _) = import_placement(
            [[0.0; 3], [3000.0, 0.0, 0.0]],
            [[0.0; 3]; 2],
            0.0,
            FrameSectionUse::Brace,
            &shape,
        )
        .unwrap();
        // 独立面積一次モーメント: 2800×140+4000×290、全断面積6800 mm²。
        let expected_mm = 1330.0 / 17.0;
        for offset in offsets {
            assert!((offset[2] - expected_mm).abs() < 1e-10, "{offset:?}");
            assert_eq!([offset[0], offset[1]], [0.0; 2]);
        }
    }
}
