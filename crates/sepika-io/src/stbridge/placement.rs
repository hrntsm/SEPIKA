use sepika_core::geom::vec3::{add, cross, dot, local_frame_axes, norm, scale, sub};
use sepika_core::model::FrameSectionUse;
use sepika_core::section_shape::{face_geometry::SectionGeometry, SectionShape};

const UNEQUAL_OFFSET_ERROR: &str = "異値offsetは、軸方向差のみで基準材軸と実材軸が同じ向きを保つ場合に対応しています。材軸・断面姿勢の対応保持を確認できません";
const OFFSET_POSITION_ERROR: &str = "異値offsetの端点位置を数値の丸めで保持できません";
const OFFSET_ROTATION_ERROR: &str =
    "異値offsetの端点位置を共通回転の積・加減算の丸めで保持できません";
const MIN_PRODUCT_FOR_EXACT_RESIDUAL: f64 = 2.0 * f64::MIN_POSITIVE / f64::EPSILON;

#[derive(Clone, Copy, Default)]
pub(super) struct GlobalTransform {
    pub translation_mm: [f64; 3],
    pub rotation_deg: f64,
}

impl GlobalTransform {
    pub fn vector(self, vector: [f64; 3]) -> [f64; 3] {
        let (sin, cos) = rotation_sin_cos(self.rotation_deg);
        [
            cos * vector[0] - sin * vector[1],
            sin * vector[0] + cos * vector[1],
            vector[2],
        ]
    }

    pub fn point(self, point: [f64; 3]) -> [f64; 3] {
        add(self.vector(point), self.translation_mm)
    }

    pub fn offsets(self, offsets_mm: [[f64; 3]; 2]) -> Result<[[f64; 3]; 2], &'static str> {
        let transformed = offsets_mm.map(|offset| self.vector(offset));
        if offsets_mm[0] != offsets_mm[1]
            && (transformed[0] == transformed[1]
                || !exact_delta(offsets_mm)
                    .is_some_and(|delta| exact_delta(transformed) == Some(self.vector(delta))))
        {
            return Err(UNEQUAL_OFFSET_ERROR);
        }
        Ok(transformed)
    }

    pub fn validate_end_positions(
        self,
        ends: [[f64; 3]; 2],
        offsets_mm: [[f64; 3]; 2],
    ) -> Result<(), &'static str> {
        let transformed = self.offsets(offsets_mm)?;
        for ((reference, offset), transformed_offset) in
            ends.into_iter().zip(offsets_mm).zip(transformed)
        {
            let rotated_reference = self.exact_vector(reference)?;
            let rotated_actual = self.exact_vector(add(reference, offset))?;
            self.exact_vector(offset)?;
            let global_reference = add(rotated_reference, self.translation_mm);
            let global_actual = add(rotated_actual, self.translation_mm);
            if exact_delta([rotated_reference, global_reference]) != Some(self.translation_mm)
                || exact_delta([rotated_actual, global_actual]) != Some(self.translation_mm)
                || exact_delta([global_reference, global_actual]) != Some(transformed_offset)
            {
                return Err(OFFSET_POSITION_ERROR);
            }
        }
        Ok(())
    }

    fn exact_vector(self, [x, y, z]: [f64; 3]) -> Result<[f64; 3], &'static str> {
        let (sin, cos) = rotation_sin_cos(self.rotation_deg);
        Ok([
            exact_rotation_component(x, cos, y, -sin).ok_or(OFFSET_ROTATION_ERROR)?,
            exact_rotation_component(x, sin, y, cos).ok_or(OFFSET_ROTATION_ERROR)?,
            z,
        ])
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
    validate_axis_direction(ends, offsets_mm)?;
    validate_offset_end_positions(ends, offsets_mm)?;
    let shift = centroid_shift(shape, usage, axes)?;
    let offsets_mm = shift_offsets(offsets_mm, shift)?;
    validate_offset_end_positions(ends, offsets_mm)?;
    validate_axis_direction(ends, offsets_mm)?;
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
    validate_axis_direction(ends, offsets_mm)?;
    validate_offset_end_positions(ends, offsets_mm)?;
    let actual = [add(ends[0], offsets_mm[0]), add(ends[1], offsets_mm[1])];
    let axes = local_frame_axes(actual[0], actual[1], reference).map_err(str::to_owned)?;
    let unequal = offsets_mm[0] != offsets_mm[1];
    if shape.is_none() && (usage != FrameSectionUse::Column || unequal) {
        return Err("配置変換の断面形状がありません".into());
    }
    let nominal = reference_axes(ends, 0.0, usage)?;
    let rotate_deg = dot(axes[1], nominal[2])
        .atan2(dot(axes[1], nominal[1]))
        .to_degrees()
        .rem_euclid(360.0);
    // 負の微小角の剰余は360度に丸まることがあり、標準の角度範囲を外れる。
    let rotate_deg = if unequal && rotate_deg == 360.0 {
        0.0
    } else {
        rotate_deg
    };
    let shift = match shape {
        Some(shape) => {
            let axes = if unequal {
                reference_axes(ends, rotate_deg, usage)?
            } else {
                axes
            };
            centroid_shift(shape, usage, axes)?
        }
        None => [0.0; 3],
    };
    let offsets_mm = shift_offsets(offsets_mm, scale(shift, -1.0))?;
    validate_axis_direction(ends, offsets_mm)?;
    if let Some(shape) = shape.filter(|_| unequal) {
        let (reread_offsets, reread_ref) =
            import_placement(ends, offsets_mm, rotate_deg, usage, shape)?;
        let reread_actual = [
            add(ends[0], reread_offsets[0]),
            add(ends[1], reread_offsets[1]),
        ];
        if reread_actual != actual {
            return Err(UNEQUAL_OFFSET_ERROR.into());
        }
        let reread_axes = local_frame_axes(reread_actual[0], reread_actual[1], reread_ref)
            .map_err(str::to_owned)?;
        if reread_axes != axes {
            return Err("異値offsetの断面基底をSTBの回転角で保持できません".into());
        }
    }
    Ok((offsets_mm, rotate_deg))
}

pub(super) fn validate_axis_direction(
    ends: [[f64; 3]; 2],
    offsets_mm: [[f64; 3]; 2],
) -> Result<(), &'static str> {
    if offsets_mm[0] == offsets_mm[1] {
        return Ok(());
    }
    let actual = [add(ends[0], offsets_mm[0]), add(ends[1], offsets_mm[1])];
    let [Some(reference), Some(offset_difference), Some(actual)] =
        [ends, offsets_mm, actual].map(exact_delta)
    else {
        return Err(UNEQUAL_OFFSET_ERROR);
    };
    let axis = reference
        .iter()
        .position(|value| *value != 0.0)
        .ok_or(UNEQUAL_OFFSET_ERROR)?;
    if actual[axis] != 0.0
        && actual[axis].is_sign_positive() == reference[axis].is_sign_positive()
        && exact_delta([reference, actual]) == Some(offset_difference)
        && (0..3).filter(|&i| i != axis).all(|i| {
            exact_products_equal(
                reference[axis],
                offset_difference[i],
                reference[i],
                offset_difference[axis],
            )
        })
    {
        return Ok(());
    }
    Err(UNEQUAL_OFFSET_ERROR)
}

fn validate_offset_end_positions(
    ends: [[f64; 3]; 2],
    offsets_mm: [[f64; 3]; 2],
) -> Result<(), &'static str> {
    let preserved = offsets_mm[0] == offsets_mm[1]
        || ends
            .into_iter()
            .zip(offsets_mm)
            .all(|(end, offset)| exact_delta([end, add(end, offset)]) == Some(offset));
    if preserved {
        Ok(())
    } else {
        Err(OFFSET_POSITION_ERROR)
    }
}

fn shift_offsets(
    offsets_mm: [[f64; 3]; 2],
    shift: [f64; 3],
) -> Result<[[f64; 3]; 2], &'static str> {
    let shifted = offsets_mm.map(|offset| add(offset, shift));
    if offsets_mm[0] != offsets_mm[1]
        && offsets_mm
            .into_iter()
            .zip(shifted)
            .any(|(offset, shifted)| exact_delta([offset, shifted]) != Some(shift))
    {
        return Err(OFFSET_POSITION_ERROR);
    }
    Ok(shifted)
}

fn exact_delta([start, end]: [[f64; 3]; 2]) -> Option<[f64; 3]> {
    Some([
        exact_difference(start[0], end[0])?,
        exact_difference(start[1], end[1])?,
        exact_difference(start[2], end[2])?,
    ])
}

fn exact_difference(start: f64, end: f64) -> Option<f64> {
    let delta = end - start;
    let (larger, smaller) = if end.abs() >= start.abs() {
        (end, -start)
    } else {
        (-start, end)
    };
    // 丸めた差だけでは、失われた桁を判定できない。
    (delta.is_finite() && smaller == delta - larger).then_some(delta)
}

fn exact_rotation_component(x: f64, x_factor: f64, y: f64, y_factor: f64) -> Option<f64> {
    let from_x = exact_product(x, x_factor)?;
    let from_y = exact_product(y, y_factor)?;
    let component = from_x + from_y;
    (exact_difference(from_x, component) == Some(from_y)).then_some(component)
}

fn exact_product(value: f64, factor: f64) -> Option<f64> {
    let product = value * factor;
    (product.is_finite()
        && (value == 0.0
            || factor == 0.0
            || factor.abs() == 1.0
            || (product.abs() > MIN_PRODUCT_FOR_EXACT_RESIDUAL
                && value.mul_add(factor, -product) == 0.0)))
        .then_some(product)
}

fn exact_products_equal(a: f64, b: f64, c: f64, d: f64) -> bool {
    if a == 0.0 || b == 0.0 || c == 0.0 || d == 0.0 {
        return (a == 0.0 || b == 0.0) && (c == 0.0 || d == 0.0);
    }
    let product = a * b;
    // 丸め誤差が相殺される場合と、残差を f64 で表せない場合は、丸めた積だけでは比較できない。
    product.is_finite()
        && product.abs() > MIN_PRODUCT_FOR_EXACT_RESIDUAL
        && product == c * d
        && a.mul_add(b, -product) == c.mul_add(d, -product)
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
    let (sin, cos) = rotation_sin_cos(rotate_deg);
    axes[1] = add(scale(axes[1], cos), scale(cross(axes[0], axes[1]), sin));
    axes[2] = cross(axes[0], axes[1]);
    if !axes.iter().flatten().all(|value| value.is_finite())
        || dot(sub(ends[1], ends[0]), sub(ends[1], ends[0])) == 0.0
    {
        return Err("基準材軸を有限な正長さで解決できません".into());
    }
    Ok(axes)
}

fn rotation_sin_cos(degrees: f64) -> (f64, f64) {
    match degrees % 360.0 {
        0.0 => (0.0, 1.0),
        90.0 | -270.0 => (1.0, 0.0),
        180.0 | -180.0 => (0.0, -1.0),
        270.0 | -90.0 => (-1.0, 0.0),
        _ => degrees.to_radians().sin_cos(),
    }
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
    fn centroid_conversion_rejects_axial_difference_lost_to_rounding() {
        let ends = [[0.0; 3], [6000.0, 0.0, 3000.0]];
        let offsets = [[0.0; 3], [2.0, 0.0, 1.0]];
        let shape = SectionShape::SteelFlatBar {
            width: 400.0,
            thick: 1e20,
        };
        let imported =
            import_placement(ends, offsets, 0.0, FrameSectionUse::Girder, &shape).unwrap_err();
        let exported = export_placement(
            ends,
            offsets,
            [0.0, 0.0, 1.0],
            FrameSectionUse::Girder,
            Some(&shape),
        )
        .unwrap_err();
        for reason in [imported, exported] {
            assert!(
                reason.contains("端点位置を数値の丸めで保持できません"),
                "{reason}"
            );
        }
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
