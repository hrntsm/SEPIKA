//! 両側SRC柱付き無開口耐震壁の断面と38条耐力。

use crate::face_distance::wall_support_face;
use crate::geom::vec3::{dist, dot, local_frame_axes_with_fallback, unit_from};
use crate::material_grade::{
    is_src_main_rebar_grade, main_rebar_material_strength_issue, rebar_tension_limits,
    resolve_src_wall_plate_f, resolve_steel_f, shear_rebar_material_issue,
    src_h_steel_minimum_thickness_mm, validate_src_h_steel_thickness,
    validate_src_h_width_thickness,
};
use crate::model::{
    wall_element_geometry, wall_is_seismic, ElementData, ElementKind, Material, MaterialCategory,
    Model, SrcWallSteelPlate, SupportMemberId, WallSideColumnIndex,
};
use crate::section_shape::SectionShape;
use crate::structure_kind::StructureKind;

/// 壁断面重心での面内内力。M正は下辺 a→b の b 側が圧縮となる向き。
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SrcWallEndForces {
    /// 壁板と両側柱の軸力 `[N]`、圧縮正。
    pub axial_n: f64,
    /// 両側柱のみの軸力 `[N]`、圧縮正。
    pub columns_compression_n: f64,
    /// 壁全体の断面内曲げモーメント `[Nmm]`。
    pub moment_nmm: f64,
    /// 壁全体の面内せん断力 `[N]`。
    pub shear_n: f64,
}

#[derive(Clone, Copy, Debug)]
struct Column {
    width_mm: f64,
    depth_mm: f64,
    steel_cross_width_mm: f64,
    steel_shear_area_mm2: f64,
    steel_f: f64,
    main_steel_area_mm2: f64,
    hoop_ratio: f64,
    hoop_ft: f64,
    j_mm: f64,
    fc: f64,
    main_rebar_area_mm2: f64,
    main_rebar_short_ft: f64,
    main_rebar_long_ft: f64,
    flange_area_mm2: f64,
    steel_axial_yield_n: f64,
    is_short_column: bool,
}

#[derive(Clone, Copy, Debug)]
struct Strip {
    start_mm: f64,
    end_mm: f64,
    width_mm: f64,
    compressive_stress: f64,
    tensile_stress: f64,
}

#[derive(Clone, Copy, Debug)]
struct Bar {
    at_mm: f64,
    compression_increment_n: f64,
    tension_n: f64,
}

/// 壁板と両側柱を含むSRC耐震壁の断面。
#[derive(Clone, Debug)]
pub struct SrcWallCapacity {
    columns: [Column; 2],
    strips: Vec<Strip>,
    bars: Vec<Bar>,
    centroid_mm: f64,
    column_span_mm: f64,
    length_mm: f64,
    clear_length_mm: f64,
    gross_area_mm2: f64,
    thickness_mm: f64,
    fc: f64,
    horizontal_ratio: f64,
    vertical_ratio: f64,
    horizontal_fy: f64,
    ultimate_flexure_material_issue: Option<String>,
    steel_plate: Option<(f64, f64, f64)>,
}

fn element_plate(model: &Model, wall: &ElementData) -> Option<SrcWallSteelPlate> {
    model
        .generated_wall_origins
        .get(&wall.id)
        .and_then(|id| model.wall_plate(*id))?
        .src_steel_plate
}

/// SRCの周辺架構または明示されたSRC仕様を持つ壁。材料欠落でも候補に含める。
pub fn is_src_wall_candidate(model: &Model, wall: &ElementData) -> bool {
    if wall.kind != ElementKind::Wall {
        return false;
    }
    let specified = model
        .generated_wall_origins
        .get(&wall.id)
        .and_then(|id| model.wall_plate(*id))
        .is_some_and(|plate| plate.src_steel_plate.is_some() || plate.src_wall_detailing_confirmed);
    if specified {
        return true;
    }
    let geometry = wall_element_geometry(wall, model);
    model.elements.iter().any(|member| {
        member.kind.is_primary_frame_support()
            && member.nodes.len() == 2
            && member.nodes.iter().all(|node| wall.nodes.contains(node))
            && matches!(
                model.element_section(member).and_then(|s| s.shape.as_ref()),
                Some(SectionShape::SrcColumnRect { .. } | SectionShape::SrcBeamRect { .. })
            )
            && geometry.as_ref().is_none_or(|g| {
                [
                    g.bottom,
                    g.top,
                    [g.bottom[0], g.top[0]],
                    [g.bottom[1], g.top[1]],
                ]
                .iter()
                .any(|edge| edge.iter().all(|node| member.nodes.contains(node)))
            })
    })
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn concrete(material: Option<&Material>) -> Result<&Material, String> {
    let material = material
        .filter(|m| m.category == MaterialCategory::Concrete)
        .ok_or("コンクリート材料の割当が必要です")?;
    if !material.fc.is_some_and(positive) || !positive(material.young) {
        return Err("コンクリートのFcとヤング係数に有限の正値が必要です".into());
    }
    if material.concrete_class != crate::units::ConcreteClass::Normal {
        return Err("普通コンクリート以外のSRC耐震壁は未対応です".into());
    }
    Ok(material)
}

fn rebar(material: Option<&Material>) -> Result<&Material, String> {
    material
        .filter(|m| m.category == MaterialCategory::Rebar && m.fy.is_some_and(positive))
        .ok_or_else(|| "鉄筋材料と有限・正値の降伏応力度fyが必要です".into())
}

fn wall_rebar<'a>(material: Option<&'a Material>, direction: &str) -> Result<&'a Material, String> {
    let material = material.ok_or_else(|| format!("壁{direction}筋材料が未割当です"))?;
    let material = rebar(Some(material)).map_err(|reason| format!("壁{direction}筋: {reason}"))?;
    if matches!(material.name.trim(), "SR235" | "SR295") {
        return Err(format!(
            "壁{direction}筋の材料「{}」は丸鋼です。SRC規準25条の壁筋にはD10以上の異形鉄筋が必要です",
            material.name
        ));
    }
    Ok(material)
}

fn frame_main_rebar(material: Option<&Material>, diameter: f64) -> Result<&Material, String> {
    let material = rebar(Some(material.ok_or("主筋材料が未割当です")?))?;
    if !is_src_main_rebar_grade(&material.name) {
        return Err(format!(
            "主筋材料「{}」はSRC規準の異形主筋に対応する材種ではありません",
            material.name
        ));
    }
    if !(13.0..=41.0).contains(&diameter) {
        return Err("主筋はD13〜D41の異形鉄筋が必要です".into());
    }
    Ok(material)
}

fn frame_shear_rebar(material: Option<&Material>) -> Result<&Material, String> {
    let material = rebar(Some(material.ok_or("せん断補強筋材料が未割当です")?))?;
    if let Some(issue) = shear_rebar_material_issue(Some(material)) {
        return Err(issue);
    }
    Ok(material)
}

/// 検証・壁展開済みモデルから断面を解決する。SRC候補の不備は理由付きエラー。
pub fn resolve_src_wall(
    model: &Model,
    wall: &ElementData,
) -> Result<Option<SrcWallCapacity>, String> {
    if !is_src_wall_candidate(model, wall) {
        return Ok(None);
    }
    resolve(model, wall)
        .map(Some)
        .map_err(|reason| format!("SRC耐震壁 ID {}: {reason}", wall.id.0))
}

fn resolve(model: &Model, wall: &ElementData) -> Result<SrcWallCapacity, String> {
    let geometry = wall_element_geometry(wall, model).ok_or("壁幾何が不明です")?;
    if !wall_is_seismic(wall, model)
        || geometry.ex_bottom[2].abs() > 1e-9
        || (geometry.lw_bottom - geometry.lw_top).abs() > 1e-9 * geometry.lw
        || unit_from(geometry.bottom_center, geometry.top_center).is_none_or(|x| x[2] < 1.0 - 1e-9)
    {
        return Err("両側柱付きの矩形鉛直耐震壁が必要です".into());
    }
    let plate = model
        .generated_wall_origins
        .get(&wall.id)
        .and_then(|id| model.wall_plate(*id))
        .ok_or("SRC耐震壁の壁版が未割当です")?;
    if plate.opening_area != 0.0 || !plate.openings.is_empty() {
        return Err("開口のあるSRC耐震壁は未対応です".into());
    }
    if !plate.src_wall_detailing_confirmed {
        return Err("壁筋の周辺定着・径・間隔・複筋配置が未確認です".into());
    }
    let section = model
        .element_section(wall)
        .ok_or("RC壁板の断面が必要です")?;
    if section.steel_material.is_some() {
        return Err("内蔵鋼板は壁版の鋼板欄で板厚・鋼材を明示してください".into());
    }
    let Some(SectionShape::RcWall {
        thickness,
        ps,
        pwh_ratio,
        pwv_ratio,
    }) = section.shape.as_ref()
    else {
        return Err("RC壁板の断面が必要です".into());
    };
    let material = concrete(model.element_material(wall))?;
    let fc = material.fc.unwrap();
    let horizontal = wall_rebar(model.element_shear_rebar_material(wall), "横")?;
    let vertical = wall_rebar(model.element_rebar_material(wall), "縦")?;
    if let Some(issue) = shear_rebar_material_issue(Some(horizontal)) {
        return Err(issue);
    }
    let ratio = |value: Option<f64>, direction: &str| {
        value
            .filter(|v| v.is_finite() && *v >= 0.0025 && *v >= *ps && *v < 1.0)
            .ok_or_else(|| {
                format!("壁{direction}筋比は0.0025以上かつ直交最小筋比以上の明示値が必要です")
            })
    };
    let horizontal_ratio = ratio(*pwh_ratio, "横")?;
    let vertical_ratio = ratio(*pwv_ratio, "縦")?;
    if !ps.is_finite() || *ps < 0.0025 {
        return Err("壁の直交最小筋比は0.0025以上が必要です".into());
    }
    let mut height = geometry.h;
    for (edge, z) in [(geometry.bottom, 1.0), (geometry.top, -1.0)] {
        let matches: Vec<_> = model
            .elements
            .iter()
            .filter(|member| {
                member.kind.is_primary_frame_support()
                    && member.nodes.len() == 2
                    && edge.iter().all(|node| member.nodes.contains(node))
            })
            .collect();
        let [beam] = matches.as_slice() else {
            return Err("上下の周辺SRC梁を一意に解決できません".into());
        };
        let Some(
            shape @ SectionShape::SrcBeamRect {
                b,
                d,
                rebar,
                steel_height,
                steel_width,
                steel_web_thick,
                steel_flange_thick,
                ..
            },
        ) = model.element_section(beam).and_then(|s| s.shape.as_ref())
        else {
            return Err("上下の周辺梁はSRC梁が必要です".into());
        };
        let beam_error = |reason| format!("周辺梁 ID {}: {reason}", beam.id.0);
        if beam.end_offsets_mm != [[0.0; 3]; 2] {
            return Err(beam_error(
                "壁と整合する材軸が必要です。材端偏心のある周辺梁は未対応です".into(),
            ));
        }
        let material = concrete(model.element_material(beam)).map_err(beam_error)?;
        if material.fc.unwrap() > 60.0 {
            return Err(beam_error(
                "コンクリートの設計基準強度が60 N/mm²を超えています".into(),
            ));
        }
        frame_main_rebar(
            model.element_rebar_material(beam),
            rebar.main_dia.designation_or_diameter_mm(),
        )
        .map_err(beam_error)?;
        frame_shear_rebar(model.element_shear_rebar_material(beam)).map_err(beam_error)?;
        if rebar.is_unset() {
            return Err(beam_error("主筋の配筋が未入力です".into()));
        }
        if !positive(rebar.aw_mm2()) || !positive(rebar.stirrup.pitch) {
            return Err(beam_error(
                "あばら筋の脚数・断面積・間隔は正の有限値が必要です".into(),
            ));
        }
        frame_steel_strength(
            model,
            beam,
            [
                *steel_height,
                *steel_width,
                *steel_web_thick,
                *steel_flange_thick,
            ],
            false,
        )
        .map_err(beam_error)?;
        validate_wall_frame_geometry(shape, [*b, *d, *steel_height, *steel_width])
            .map_err(beam_error)?;
        height -= wall_support_face(model, SupportMemberId::Primary(beam.id), [0.0, 0.0, z])?;
    }
    if !positive(height) || !thickness.is_finite() || *thickness < 120.0_f64.max(height / 30.0) {
        return Err("壁厚は120 mm以上かつ内法高さの1/30以上が必要です".into());
    }
    let index = WallSideColumnIndex::build(model);
    let bindings = index
        .wall_replacement_bindings(wall.id)
        .map_err(|issues| format!("周辺骨組の入力が不正です ({issues:?})"))?;
    let [a, b] = bindings.as_slice() else {
        return Err("両側のSRC柱を一意に解決できません".into());
    };
    if a.side != 0
        || b.side != 1
        || [a, b]
            .iter()
            .any(|binding| binding.column_structure != Some(StructureKind::Src))
    {
        return Err("両側にSRC柱が必要です".into());
    }
    let mut strips = Vec::new();
    let mut bars = Vec::new();
    let mut cols = Vec::new();
    let mut ultimate_flexure_material_issue =
        main_rebar_material_strength_issue(vertical).map(|reason| format!("壁縦筋: {reason}"));
    for (side, binding) in [a, b].into_iter().enumerate() {
        let center = (side as f64 - 0.5) * geometry.lw;
        let col = model.element(binding.column).ok_or("側柱が不明です")?;
        let (column, issue) = resolve_column(
            model,
            col,
            geometry.ex_bottom,
            center,
            &mut strips,
            &mut bars,
        )
        .map_err(|err| format!("側柱 ID {}: {err}", col.id.0))?;
        cols.push(column);
        if let Some(issue) = issue {
            ultimate_flexure_material_issue
                .get_or_insert_with(|| format!("側柱 ID {} の主筋: {issue}", col.id.0));
        }
    }
    let columns: [Column; 2] = cols.try_into().unwrap();
    let low = (-geometry.lw + columns[0].depth_mm) / 2.0;
    let high = (geometry.lw - columns[1].depth_mm) / 2.0;
    let clear_length_mm = high - low;
    if !positive(clear_length_mm) || columns.iter().any(|col| col.width_mm < *thickness) {
        return Err("両側柱の内法長さまたは見付断面の幅が不正です".into());
    }
    let steel_plate = element_plate(model, wall).map(|steel_plate| {
        if !steel_plate.force_transfer_confirmed {
            return Err("内蔵鋼板とコンクリート・周辺鉄骨との一体性および軸力・曲げ・せん断の伝達が未確認です".into());
        }
        let t = steel_plate.thickness_mm;
        if !positive(t) || t >= *thickness {
            return Err("内蔵鋼板厚は正で壁厚未満としてください".into());
        }
        if (*thickness - t) / 2.0 < 50.0 {
            return Err("内蔵鋼板から各壁面まで50 mm以上のコンクリートかぶりが必要です".into());
        }
        let mat = model.materials.get(steel_plate.material.index())
            .filter(|m| m.id == steel_plate.material)
            .ok_or("内蔵鋼板の材料が不明です")?;
        let f = resolve_src_wall_plate_f(mat, t)?;
        if !positive(mat.young) || !positive(mat.shear_modulus()) {
            return Err("内蔵鋼板のヤング係数・せん断弾性係数が不正です".into());
        }
        strips.push(Strip { start_mm: low, end_mm: high, width_mm: t, compressive_stress: f, tensile_stress: -f });
        Ok::<_, String>((t, f, mat.young / material.young))
    }).transpose()?;
    strips.push(Strip {
        start_mm: low,
        end_mm: high,
        width_mm: thickness - steel_plate.map_or(0.0, |p| p.0),
        compressive_stress: 0.75 * fc,
        tensile_stress: 0.0,
    });
    let wall_vertical_area = vertical_ratio * thickness * clear_length_mm;
    let wall_vertical_fy = vertical.fy.unwrap();
    if ultimate_flexure_material_issue.is_none() {
        bars.push(Bar {
            at_mm: (low + high) / 2.0,
            compression_increment_n: (2.0 * wall_vertical_fy - 0.75 * fc) * wall_vertical_area,
            tension_n: -wall_vertical_fy * wall_vertical_area,
        });
    }
    let [left, right] = columns.map(|col| col.width_mm * col.depth_mm);
    let web = thickness * clear_length_mm;
    let gross_area_mm2 = left + right + web;
    let centroid_mm =
        ((right - left) * geometry.lw / 2.0 + web * (low + high) / 2.0) / gross_area_mm2;
    Ok(SrcWallCapacity {
        columns,
        strips,
        bars,
        centroid_mm,
        column_span_mm: geometry.lw,
        length_mm: clear_length_mm + columns[0].depth_mm + columns[1].depth_mm,
        clear_length_mm,
        gross_area_mm2,
        thickness_mm: *thickness,
        fc,
        horizontal_ratio,
        vertical_ratio,
        horizontal_fy: horizontal
            .fy
            .unwrap()
            .min(crate::material_grade::rebar_grade_f_value(&horizontal.name).unwrap_or(0.0)),
        steel_plate,
        ultimate_flexure_material_issue,
    })
}

fn frame_steel_strength(
    model: &Model,
    member: &ElementData,
    [h, w, tw, tf]: [f64; 4],
    is_column: bool,
) -> Result<(f64, f64), String> {
    if model
        .steel_design_attrs
        .iter()
        .any(|attr| attr.elem == member.id && attr.has_section_loss())
    {
        return Err("欠損を含む内蔵鉄骨の有効断面を評価できません".into());
    }
    let steel = model
        .element_steel_material(member)
        .ok_or("内蔵H形鋼の材料が未割当です")?;
    let minimum_mm = src_h_steel_minimum_thickness_mm(steel)?;
    validate_src_h_steel_thickness(&steel.name, tw, tf, minimum_mm)?;
    let flange_f = resolve_steel_f(Some(steel), Some(tf))?;
    let web_f = resolve_steel_f(Some(steel), Some(tw))?;
    validate_src_h_width_thickness(
        w / (2.0 * tf),
        (h - 2.0 * tf) / tw,
        flange_f,
        web_f,
        is_column,
    )?;
    Ok((flange_f, web_f))
}

fn validate_wall_frame_geometry(
    shape: &SectionShape,
    [b, d, h, w]: [f64; 4],
) -> Result<(), String> {
    shape
        .validate_src_geometry()
        .map_err(|reason| format!("SRC断面・配筋: {reason}"))?;
    if (b - w) / 2.0 < 50.0 || (d - h) / 2.0 < 50.0 {
        return Err("内蔵H形鋼のコンクリートかぶりは各面50 mm以上が必要です".into());
    }
    Ok(())
}

fn resolve_column(
    model: &Model,
    column: &ElementData,
    wall_axis: [f64; 3],
    center: f64,
    strips: &mut Vec<Strip>,
    bars: &mut Vec<Bar>,
) -> Result<(Column, Option<String>), String> {
    let Some(
        shape @ SectionShape::SrcColumnRect {
            b,
            d,
            rebar: main,
            steel_height: h,
            steel_width: w,
            steel_web_thick: tw,
            steel_flange_thick: tf,
            ..
        },
    ) = model.element_section(column).and_then(|s| s.shape.as_ref())
    else {
        return Err("SRC矩形柱断面が必要です".into());
    };
    let [b, d, h, w, tw, tf] = [*b, *d, *h, *w, *tw, *tf];
    let fc = concrete(model.element_material(column))?.fc.unwrap();
    let main_material = frame_main_rebar(
        model.element_rebar_material(column),
        main.main_dia.designation_or_diameter_mm(),
    )?;
    let fy = main_material.fy.unwrap();
    let material_issue = main_rebar_material_strength_issue(main_material);
    let (long_ft, short_ft) = rebar_tension_limits(
        &main_material.name,
        main.main_dia.designation_or_diameter_mm(),
    )
    .ok_or("側柱主筋の許容応力度が未定義です")?;
    let hoop = frame_shear_rebar(model.element_shear_rebar_material(column))?;
    let (flange_f, web_f) = frame_steel_strength(model, column, [h, w, tw, tf], true)?;
    validate_wall_frame_geometry(shape, [b, d, h, w])?;
    let points = main.bar_positions(b, d).map_err(|e| e.to_string())?;
    if points.is_empty() || !positive(main.main_dia.area_mm2()) || !positive(main.hoop.pitch) {
        return Err("柱主筋と帯筋の配筋が必要です".into());
    }
    let (i, j) = model
        .support_member_axis(SupportMemberId::Primary(column.id))
        .ok_or("柱材軸が不明です")?;
    let (axes, fallback) = local_frame_axes_with_fallback(i, j, column.local_axis.ref_vector)?;
    let y = dot(axes[1], wall_axis);
    let z = dot(axes[2], wall_axis);
    let along_b = z.abs() > 1.0 - 1e-9;
    if fallback || (!along_b && y.abs() <= 1.0 - 1e-9) || column.end_offsets_mm != [[0.0; 3]; 2] {
        return Err("壁と整合する柱材軸・断面方向が必要です".into());
    }
    let (width_mm, depth_mm, steel_cross_width_mm, hoop_area_mm2) = if along_b {
        (d, b, h, main.aw_y_mm2())
    } else {
        (b, d, w, main.aw_x_mm2())
    };
    let hoop_ratio = hoop_area_mm2 / (width_mm * main.hoop.pitch);
    if !positive(hoop_ratio) {
        return Err("側柱帯筋比が不正です".into());
    }
    let area = main.main_dia.area_mm2();
    let mut furthest = [-f64::INFINITY; 2];
    for point in &points {
        let x = if along_b {
            z.signum() * point.x
        } else {
            y.signum() * point.y
        };
        furthest[0] = furthest[0].max(x);
        furthest[1] = furthest[1].max(-x);
        if material_issue.is_none() {
            bars.push(Bar {
                at_mm: center + x,
                compression_increment_n: (2.0 * fy - 0.75 * fc) * area,
                tension_n: -fy * area,
            });
        }
    }
    let steel_sections = if along_b {
        [
            (-w / 2.0, w / 2.0, 2.0 * tf, flange_f),
            (-tw / 2.0, tw / 2.0, h - 2.0 * tf, web_f),
            (0.0, 0.0, 0.0, 0.0),
        ]
    } else {
        [
            (-h / 2.0, -h / 2.0 + tf, w, flange_f),
            (-h / 2.0 + tf, h / 2.0 - tf, tw, web_f),
            (h / 2.0 - tf, h / 2.0, w, flange_f),
        ]
    };
    strips.push(Strip {
        start_mm: center - depth_mm / 2.0,
        end_mm: center + depth_mm / 2.0,
        width_mm,
        compressive_stress: 0.75 * fc,
        tensile_stress: 0.0,
    });
    for (lo, hi, width, f) in steel_sections.into_iter().filter(|&(lo, hi, ..)| hi > lo) {
        strips.push(Strip {
            start_mm: center + lo,
            end_mm: center + hi,
            width_mm: width,
            compressive_stress: f - 0.75 * fc,
            tensile_stress: -f,
        });
    }
    let (steel_shear_area_mm2, steel_f) = if along_b {
        (4.0 / 3.0 * w * tf, flange_f)
    } else {
        (tw * (h - 2.0 * tf), web_f)
    };
    let flange_area_mm2 = w * tf;
    let web_area_mm2 = (h - 2.0 * tf) * tw;
    let attr = model
        .steel_design_attrs
        .iter()
        .find(|a| a.elem == column.id);
    let (ly, lz) = crate::model::column_buckling_lengths_mm(
        dist(i, j),
        attr.and_then(|a| a.lk_y_direct),
        attr.and_then(|a| a.lk_z_direct),
    );
    let is_short_column = positive(ly) && positive(lz) && ly <= 12.0 * d && lz <= 12.0 * b;
    Ok((
        Column {
            width_mm,
            depth_mm,
            steel_cross_width_mm,
            steel_shear_area_mm2,
            steel_f,
            main_steel_area_mm2: area * points.len() as f64 + 2.0 * flange_area_mm2 + web_area_mm2,
            hoop_ratio,
            hoop_ft: hoop
                .fy
                .unwrap()
                .min(crate::material_grade::rebar_grade_f_value(&hoop.name).unwrap_or(0.0)),
            j_mm: 0.875 * (depth_mm / 2.0 + furthest[0].min(furthest[1])),
            fc,
            main_rebar_area_mm2: area * points.len() as f64,
            main_rebar_short_ft: short_ft.min(fy),
            main_rebar_long_ft: long_ft.min(fy),
            flange_area_mm2,
            steel_axial_yield_n: 2.0 * flange_area_mm2 * flange_f + web_area_mm2 * web_f,
            is_short_column,
        },
        material_issue,
    ))
}

impl SrcWallCapacity {
    pub fn column_depths_mm(&self) -> [f64; 2] {
        self.columns.map(|col| col.depth_mm)
    }
    pub fn clear_length_mm(&self) -> f64 {
        self.clear_length_mm
    }
    pub fn centroid_from_column_midpoint_mm(&self) -> f64 {
        self.centroid_mm
    }
    pub fn has_steel_plate(&self) -> bool {
        self.steel_plate.is_some()
    }

    /// 25条の両側柱分担法による軸力検定比。側柱の長期圧縮軸力 `[N]` は a、b 順。
    pub fn allowable_bending_ratio(
        &self,
        end: SrcWallEndForces,
        gravity: [f64; 2],
        long_term: bool,
        seismic: bool,
    ) -> Result<f64, String> {
        if gravity.iter().any(|n| !n.is_finite()) || !end.moment_nmm.is_finite() {
            return Err(
                "曲げに対する両側柱分担法に有限の柱別長期軸力と壁曲げモーメントが必要です".into(),
            );
        }
        let gravity_moment_nmm = (gravity[1] - gravity[0]) * self.column_span_mm / 2.0;
        let remaining_moment_nmm =
            end.moment_nmm + self.centroid_mm * end.axial_n - gravity_moment_nmm;
        let couple_n = remaining_moment_nmm / self.column_span_mm;
        let axial_remainder_n = (end.axial_n - gravity[0] - gravity[1]) / 2.0;
        let demands = [
            gravity[0] + axial_remainder_n - couple_n,
            gravity[1] + axial_remainder_n + couple_n,
        ];
        let mut ratio = 0.0_f64;
        for (col, demand) in self.columns.iter().zip(demands) {
            if !col.is_short_column || !demand.is_finite() || col.fc > 60.0 {
                return Err("25条の曲げに対する両側柱分担法では両側柱の短柱条件、Fc 60以下と有限の設計軸力が必要です".into());
            }
            let gross = col.depth_mm * col.width_mm;
            let n = if col.fc <= 27.0 {
                15.0
            } else if col.fc <= 36.0 {
                13.0
            } else if col.fc <= 48.0 {
                11.0
            } else {
                9.0
            };
            let fc = col.fc * if long_term { 1.0 / 3.0 } else { 2.0 / 3.0 };
            let fc_reduced = fc * (1.0 - 15.0 * col.flange_area_mm2 / gross).max(0.0);
            let ft = if long_term {
                col.main_rebar_long_ft
            } else {
                col.main_rebar_short_ft
            };
            let steel_limit = col.steel_axial_yield_n / if long_term { 1.5 } else { 1.0 };
            let ae = gross + (n - 1.0) * col.main_rebar_area_mm2;
            let concrete_limit = ae * fc_reduced.min(ft / n);
            let compression_limit = if seismic {
                (concrete_limit + steel_limit).min(gross * col.fc / 3.0 + 2.0 * steel_limit / 3.0)
            } else {
                concrete_limit + steel_limit
            };
            let tension_limit = col.main_rebar_area_mm2 * ft + steel_limit;
            let side_ratio = if demand >= 0.0 {
                demand / compression_limit
            } else {
                -demand / tension_limit
            };
            ratio = ratio.max(side_ratio);
        }
        if ratio.is_finite() {
            Ok(ratio)
        } else {
            Err("両側柱の軸力検定比を有限値で算定できません".into())
        }
    }

    /// 25条の許容せん断耐力 `[N]`。長期時は式(25.1)、短期時は式(25.2)。
    pub fn allowable_shear_n(&self, long_term: bool) -> f64 {
        let concrete_shear = |fc: f64| {
            let long = (fc / 30.0).min(0.49 + fc / 100.0);
            if long_term {
                long
            } else {
                1.5 * long
            }
        };
        let fs = concrete_shear(self.fc);
        let beta = self
            .steel_plate
            .map_or(0.0, |(t, _, n)| n * t / self.thickness_mm);
        let q1 = self.thickness_mm * self.length_mm * fs * (1.0 + beta);
        if long_term {
            return q1;
        }
        let ratio = self.columns.iter().fold(
            self.horizontal_ratio
                .min(0.012)
                .min(2.0 * self.vertical_ratio),
            |ratio, col| ratio.min(col.hoop_ratio * col.width_mm / self.thickness_mm),
        );
        let side_q: f64 = self
            .columns
            .iter()
            .map(|col| {
                let fs = concrete_shear(col.fc);
                let tie = col.hoop_ft * col.hoop_ratio;
                let b_net = col.width_mm - col.steel_cross_width_mm;
                let q1 = col.width_mm * col.j_mm * (1.5 * fs + 0.5 * tie);
                let q2 = col.j_mm * (2.0 * b_net * fs + col.width_mm * tie);
                q1.min(q2) + col.steel_shear_area_mm2 * col.steel_f / 3.0_f64.sqrt()
            })
            .sum();
        let plate_q = self.steel_plate.map_or(0.0, |(t, f, _)| {
            t * self.clear_length_mm * f / 3.0_f64.sqrt()
        });
        let q2 = ratio * self.thickness_mm * self.clear_length_mm * self.horizontal_fy
            + side_q
            + plate_q;
        let factor = if self.has_steel_plate() { 0.8 } else { 1.0 };
        factor * q1.max(q2)
    }

    /// 断面応力を用いる38条終局せん断耐力 `[N]`。断面内のM・Qが階の最大値である場合に使う。
    pub fn ultimate_shear_n(&self, end: SrcWallEndForces) -> Result<f64, String> {
        self.ultimate_shear_with_span_n(end, end.moment_nmm.abs(), end.shear_n.abs())
    }

    /// 断面位置のN・Mと同じ荷重状態の階内最大 `|M| [Nmm]`・`|Q| [N]` を渡す。
    pub fn ultimate_shear_with_span_n(
        &self,
        end: SrcWallEndForces,
        max_moment_nmm: f64,
        max_shear_n: f64,
    ) -> Result<f64, String> {
        if !end.columns_compression_n.is_finite()
            || !end.moment_nmm.is_finite()
            || !end.axial_n.is_finite()
            || !end.shear_n.is_finite()
            || !max_moment_nmm.is_finite()
            || !positive(max_shear_n)
            || max_moment_nmm < end.moment_nmm.abs()
            || max_shear_n < end.shear_n.abs()
        {
            return Err("38条のせん断スパンを確定する階内最大M・Qが不足または不正です".into());
        }
        let span = max_moment_nmm / (max_shear_n * self.length_mm);
        if !(1.0..=3.0).contains(&span) {
            return Err(format!(
                "38条のせん断スパン比 M/(Ql)={span} は適用範囲1〜3外です"
            ));
        }
        let te = if self
            .columns
            .iter()
            .all(|column| column.width_mm > self.thickness_mm)
        {
            self.gross_area_mm2 / self.length_mm
        } else {
            self.thickness_mm
        };
        let sigma_0 = end.columns_compression_n / self.gross_area_mm2;
        let strength = |compression: usize| {
            let d = self.length_mm - self.columns[compression].depth_mm / 2.0;
            let pte = 100.0 * self.columns[1 - compression].main_steel_area_mm2 / (te * d);
            let pwe = self.horizontal_ratio * self.thickness_mm / te;
            let q = (0.068 * pte.powf(0.23) * (self.fc + 18.0) / (span + 0.12).sqrt()
                + 0.85 * (pwe * self.horizontal_fy).sqrt()
                + 0.1 * sigma_0)
                * te
                * 0.875
                * d;
            self.steel_plate.map_or(q, |(t, f, _)| {
                0.8 * (q + t * self.clear_length_mm * f / 3.0_f64.sqrt())
            })
        };
        let result = if end.moment_nmm > 0.0 {
            strength(1)
        } else if end.moment_nmm < 0.0 {
            strength(0)
        } else {
            strength(1).min(strength(0))
        };
        positive(result)
            .then_some(result)
            .ok_or_else(|| "38条終局せん断耐力が有限の正値になりません".into())
    }

    /// 指定軸力と曲げ方向に対する38条全塑性モーメント `[Nmm]`。
    pub fn ultimate_moment_nmm(&self, end: SrcWallEndForces) -> Result<f64, String> {
        if let Some(reason) = &self.ultimate_flexure_material_issue {
            return Err(format!("38条1項の終局曲げ耐力に必要な材料強度: {reason}"));
        }
        if !end.axial_n.is_finite() || !end.moment_nmm.is_finite() {
            return Err("38条の曲げ耐力に有限の軸力・曲げモーメントが必要です".into());
        }
        let capacity = |sign: f64| {
            let mut breaks: Vec<_> = self
                .strips
                .iter()
                .flat_map(|s| [s.start_mm, s.end_mm])
                .chain(self.bars.iter().map(|b| b.at_mm))
                .collect();
            breaks.sort_by(|a, b| (b * sign).total_cmp(&(a * sign)));
            breaks.dedup();
            let state = |neutral: f64, include_neutral: bool| {
                let mut n = 0.0;
                let mut m = 0.0;
                let mut add = |a: f64, x: f64| {
                    n += a;
                    m += a * (x - self.centroid_mm);
                };
                for s in &self.strips {
                    add(
                        s.tensile_stress * s.width_mm * (s.end_mm - s.start_mm),
                        (s.start_mm + s.end_mm) / 2.0,
                    );
                    let x = neutral.clamp(s.start_mm, s.end_mm);
                    let (lo, hi) = if sign > 0.0 {
                        (x, s.end_mm)
                    } else {
                        (s.start_mm, x)
                    };
                    add(
                        (s.compressive_stress - s.tensile_stress) * s.width_mm * (hi - lo),
                        (lo + hi) / 2.0,
                    );
                }
                for bar in &self.bars {
                    let compressed = sign * (bar.at_mm - neutral) > 0.0
                        || (include_neutral && bar.at_mm == neutral);
                    add(
                        bar.tension_n
                            + if compressed {
                                bar.compression_increment_n
                            } else {
                                0.0
                            },
                        bar.at_mm,
                    );
                }
                (n, m)
            };
            for (i, &x) in breaks.iter().enumerate() {
                let (n0, m0) = state(x, false);
                let (n1, _) = state(x, true);
                if (n0..=n1).contains(&end.axial_n) {
                    return Ok((sign * (m0 + (end.axial_n - n0) * (x - self.centroid_mm))).max(0.0));
                }
                let Some(&next) = breaks.get(i + 1) else {
                    break;
                };
                let (n2, _) = state(next, false);
                if (n1..=n2).contains(&end.axial_n) {
                    let neutral = x + (next - x) * (end.axial_n - n1) / (n2 - n1);
                    return Ok((sign * state(neutral, true).1).max(0.0));
                }
            }
            Err("作用軸力が38条の全塑性断面の範囲外です".to_string())
        };
        if end.moment_nmm > 0.0 {
            capacity(1.0)
        } else if end.moment_nmm < 0.0 {
            capacity(-1.0)
        } else {
            Ok(capacity(1.0)?.min(capacity(-1.0)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(plate: bool) -> SrcWallCapacity {
        let column = Column {
            width_mm: 600.0,
            depth_mm: 600.0,
            steel_cross_width_mm: 250.0,
            steel_shear_area_mm2: 3200.0,
            steel_f: 235.0,
            main_steel_area_mm2: 23_552.4,
            hoop_ratio: 0.004,
            hoop_ft: 235.0,
            j_mm: 450.0,
            fc: 27.0,
            main_rebar_area_mm2: 2_000.0,
            main_rebar_short_ft: 345.0,
            main_rebar_long_ft: 195.0,
            flange_area_mm2: 4_000.0,
            steel_axial_yield_n: 2_632_000.0,
            is_short_column: true,
        };
        SrcWallCapacity {
            columns: [column; 2],
            strips: vec![],
            bars: vec![],
            centroid_mm: 0.0,
            column_span_mm: 4600.0,
            length_mm: 5200.0,
            clear_length_mm: 4000.0,
            gross_area_mm2: 1_520_000.0,
            thickness_mm: 200.0,
            fc: 27.0,
            horizontal_ratio: 0.005,
            vertical_ratio: 0.005,
            horizontal_fy: 345.0,
            ultimate_flexure_material_issue: None,
            steel_plate: plate.then_some((12.0, 235.0, 10.0)),
        }
    }

    #[test]
    fn article_25_shear_matches_hand_check_for_ordinary_and_embedded_plate_walls() {
        for (plate, long_n, short_n) in [
            (false, 790_400.0, 3_425_534.804861197),
            (true, 1_264_640.0, 7_950_436.673056141),
        ] {
            let wall = section(plate);
            assert!((wall.allowable_shear_n(true) - long_n).abs() < 1e-6);
            assert!((wall.allowable_shear_n(false) - short_n).abs() < 1e-6);
        }
    }

    #[test]
    fn article_25_flexure_checks_column_axial_utilization_from_same_wall_resultant() {
        let mut end = SrcWallEndForces {
            axial_n: 1_400_000.0,
            moment_nmm: 5_520_000_000.0,
            ..Default::default()
        };
        for plate in [false, true] {
            let wall = section(plate);
            let expected = 1_900_000.0 / 4_664_666.666666667;
            let ratio = wall
                .allowable_bending_ratio(end, [500_000.0, 900_000.0], true, false)
                .unwrap();
            assert!((ratio - expected).abs() < 1e-10);
            end.axial_n = f64::NAN;
            assert!(wall
                .allowable_bending_ratio(end, [500_000.0, 900_000.0], true, false)
                .is_err());
            end.axial_n = 1_400_000.0;
        }
    }

    #[test]
    fn article_38_shear_matches_hand_check_and_rejects_outside_span() {
        let mut end = SrcWallEndForces {
            axial_n: 2_900_000.0,
            columns_compression_n: 1_900_000.0,
            moment_nmm: 7_280_000_000.0,
            shear_n: 1_000_000.0,
        };
        for (plate, expected_n) in [(false, 4_801_553.2199748), (true, 9_051_251.405147)] {
            let wall = section(plate);
            assert!((wall.ultimate_shear_n(end).unwrap() - expected_n).abs() < 1e-4);
            assert!(wall
                .ultimate_shear_with_span_n(end, 20e9, end.shear_n)
                .is_err());
            let at_bound = |span: f64| {
                wall.ultimate_shear_n(SrcWallEndForces {
                    moment_nmm: span * 5_200.0 * end.shear_n,
                    ..end
                })
            };
            assert!(at_bound(0.999).is_err());
            assert!(at_bound(1.0).unwrap() > at_bound(3.0).unwrap());
            assert!(at_bound(3.001).is_err());
            end.shear_n = 0.0;
            assert!(wall.ultimate_shear_n(end).is_err());
            end.shear_n = 1_000_000.0;
        }
    }

    #[test]
    fn article_38_uses_wall_thickness_for_t_and_rectangular_sections() {
        let end = SrcWallEndForces {
            axial_n: 2_900_000.0,
            columns_compression_n: 1_900_000.0,
            moment_nmm: 7_280_000_000.0,
            shear_n: 1_000_000.0,
        };
        for (other_column_width_mm, expected_ordinary, expected_embedded) in [
            (600.0, 3_688_457.478901435, 8_160_774.812288333),
            (200.0, 3_717_830.976497589, 8_184_273.610365256),
        ] {
            for (plate, expected_n) in [(false, expected_ordinary), (true, expected_embedded)] {
                let mut wall = section(plate);
                wall.columns[0].width_mm = 200.0;
                wall.columns[1].width_mm = other_column_width_mm;
                wall.gross_area_mm2 = 800_000.0 + (200.0 + other_column_width_mm) * 600.0;
                assert!((wall.ultimate_shear_n(end).unwrap() - expected_n).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn article_38_flexure_integrates_compression_block_and_both_steel_yield_signs() {
        let end = SrcWallEndForces {
            axial_n: 600.0,
            moment_nmm: 1.0,
            ..Default::default()
        };
        let mut wall = section(false);
        wall.strips = vec![Strip {
            start_mm: -2.0,
            end_mm: 2.0,
            width_mm: 10.0,
            compressive_stress: 30.0,
            tensile_stress: 0.0,
        }];
        wall.bars = [-1.0, 1.0]
            .map(|at_mm| Bar {
                at_mm,
                compression_increment_n: 100.0,
                tension_n: -50.0,
            })
            .to_vec();
        assert_eq!(wall.ultimate_moment_nmm(end).unwrap(), 700.0);
        wall.strips.push(Strip {
            start_mm: -2.0,
            end_mm: 2.0,
            width_mm: 2.0,
            compressive_stress: 20.0,
            tensile_stress: -20.0,
        });
        assert_eq!(wall.ultimate_moment_nmm(end).unwrap(), 860.0);
        assert!(wall
            .ultimate_moment_nmm(SrcWallEndForces {
                axial_n: 1e9,
                ..end
            })
            .is_err());
    }
}
