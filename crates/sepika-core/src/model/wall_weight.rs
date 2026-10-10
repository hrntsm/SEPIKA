//! 物理壁版の自重総量と、支持先に依存しない実領域の積分。

use super::*;
use crate::geom::polygon::{area, signed_area};
use i_overlay::core::{fill_rule::FillRule, overlay_rule::OverlayRule};
use i_overlay::float::single::SingleFloatOverlay;

/// 囲まれた壁版のDL支持方式。階重量の集計帯には影響しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallDlSupport {
    LowerBeam,
    UpperBeam,
    HeightMidpoint,
}

/// 階重量生成で明示した壁自重の算入契約。未設定とケースのみを区別する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallWeightGenerationMode {
    Geometry,
    GravityCasesOnly,
}

/// 重量用途ごとの総量 [N]。配分未算定でも総量は保持する。
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WallWeightTotals {
    pub design_n: f64,
    pub physical_n: f64,
    pub matrix_n: f64,
}

/// 同一壁版IDの総量と実領域。位置不足は`partition_issue`に保持する。
#[derive(Clone, Debug, PartialEq)]
pub struct WallWeight {
    pub plate: crate::ids::WallPlateId,
    pub totals: WallWeightTotals,
    pub net_area_mm2: f64,
    pub design_body_n: f64,
    pub physical_body_n: f64,
    pub finish_n: f64,
    pub opening_n: f64,
    pub partition_issue: Option<String>,
    pub z_range_mm: [f64; 2],
    x_range_mm: [f64; 2],
    region: Vec<Vec<Vec<[f64; 2]>>>,
    intensity: WallWeightTotals,
    origin_xy: [f64; 2],
    axis_xy: [f64; 2],
}

fn shapes_area(shapes: &[Vec<Vec<[f64; 2]>>]) -> f64 {
    shapes
        .iter()
        .map(|s| {
            s.iter()
                .enumerate()
                .map(|(i, p)| if i == 0 { area(p) } else { -area(p) })
                .sum::<f64>()
        })
        .sum()
}

fn scalar(v: f64, positive: bool, name: &str) -> Result<(), String> {
    if !v.is_finite() || v < 0.0 || (positive && v == 0.0) {
        Err(format!("{name}が不正です"))
    } else {
        Ok(())
    }
}

fn clip_half_plane(poly: &[[f64; 2]], a: [f64; 2], b: [f64; 2], inset: f64) -> Vec<[f64; 2]> {
    let dx = b[0] - a[0];
    let dz = b[1] - a[1];
    let len = dx.hypot(dz);
    let d = |p: [f64; 2]| (dx * (p[1] - a[1]) - dz * (p[0] - a[0])) / len - inset;
    let mut out = Vec::new();
    for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        let dp = d(p);
        let dq = d(q);
        if dp >= 0.0 {
            out.push(p);
        }
        if (dp >= 0.0) != (dq >= 0.0) {
            let t = dp / (dp - dq);
            out.push([p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]);
        }
    }
    out
}

fn validate_polygon(poly: &[[f64; 2]]) -> Result<(), String> {
    if poly.len() < 3
        || poly.iter().flatten().any(|x| !x.is_finite())
        || !area(poly).is_finite()
        || area(poly) <= 1e-6
    {
        return Err("壁領域が縮退または不正です".into());
    }
    let cross = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        if (b[0] - a[0]).hypot(b[1] - a[1]) <= 1e-8 {
            return Err("壁領域に長さゼロの辺があります".into());
        }
        for j in i + 1..poly.len() {
            if j == i + 1 || (i == 0 && j == poly.len() - 1) {
                continue;
            }
            let c = poly[j];
            let d = poly[(j + 1) % poly.len()];
            if cross(a, b, c) * cross(a, b, d) <= 0.0
                && cross(c, d, a) * cross(c, d, b) <= 0.0
                && a[0].min(b[0]) <= c[0].max(d[0])
                && c[0].min(d[0]) <= a[0].max(b[0])
                && a[1].min(b[1]) <= c[1].max(d[1])
                && c[1].min(d[1]) <= a[1].max(b[1])
            {
                return Err("壁領域が自己交差しています".into());
            }
        }
    }
    Ok(())
}

impl Model {
    /// 物理壁版IDごとの設計・物理・matrix総量を算定する。入力不正はID付きエラー。
    pub fn wall_weight(&self, plate: &WallPlate) -> Result<WallWeight, String> {
        self.wall_weight_inner(plate)
            .map_err(|e| format!("壁版 {}: {e}", plate.id.0))
    }

    fn wall_weight_inner(&self, plate: &WallPlate) -> Result<WallWeight, String> {
        scalar(plate.opening_area, false, "開口面積")?;
        scalar(plate.opening_weight, false, "開口部重量")?;
        for l in &plate.loads {
            scalar(l.value, false, "仕上げ・増打ち面荷重")?;
        }
        let pts = plate
            .boundary_coords(self)
            .ok_or("壁領域または取付き高さが未設定です")?;
        if pts.iter().flatten().any(|x| !x.is_finite()) {
            return Err("壁座標が不正です".into());
        }
        let origin = pts[0];
        let axis = pts
            .iter()
            .skip(1)
            .find_map(|p| {
                let dx = p[0] - origin[0];
                let dy = p[1] - origin[1];
                let l = dx.hypot(dy);
                (l > 1e-8).then_some([dx / l, dy / l])
            })
            .ok_or("鉛直取付き線の壁面方向を解決できません")?;
        if pts
            .iter()
            .any(|p| ((p[0] - origin[0]) * axis[1] - (p[1] - origin[1]) * axis[0]).abs() > 1e-5)
        {
            return Err("壁が同一鉛直構面にありません".into());
        }
        let mut poly: Vec<_> = pts
            .iter()
            .map(|p| {
                [
                    (p[0] - origin[0]) * axis[0] + (p[1] - origin[1]) * axis[1],
                    p[2],
                ]
            })
            .collect();
        let z_range = [
            pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min),
            pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max),
        ];
        let mut extra_polys = Vec::new();
        if plate.is_attached() {
            let ex = self
                .wall_plate_extent(plate)
                .ok_or("取付き高さが未設定です")?;
            if ex.iter().any(|v| !v.is_finite()) || ex.iter().all(|v| *v == 0.0) {
                return Err("取付き高さが不正です".into());
            }
            if ex[0] * ex[1] < 0.0 {
                let t = ex[0] / (ex[0] - ex[1]);
                let a = poly[0];
                let b = poly[1];
                let mid = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
                extra_polys.push(vec![mid, poly[1], poly[2]]);
                poly = vec![poly[0], mid, poly[3]];
            }
        }
        poly.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8);
        if poly.len() > 1
            && (poly[0][0] - poly[poly.len() - 1][0]).hypot(poly[0][1] - poly[poly.len() - 1][1])
                <= 1e-8
        {
            poly.pop();
        }
        validate_polygon(&poly)?;
        let mut clear = poly.clone();
        if let WallPlateShape::Enclosed = plate.shape {
            let r = self
                .wall_plate_assignment_region(plate.id)
                .ok_or("壁版割当領域が未設定です")?;
            if r.boundary.len() != poly.len() {
                return Err("壁境界の支持対応が不正です".into());
            }
            let sign = signed_area(&poly).signum();
            for (i, e) in r.boundary.iter().enumerate() {
                let section = match e.support {
                    SupportMemberId::Primary(id) => {
                        self.element(id).and_then(|e| self.element_section(e))
                    }
                    SupportMemberId::Secondary(id) => self
                        .secondary_member(id)
                        .and_then(|e| self.sections.get(e.section?.index())),
                };
                let s = section.ok_or("周辺支持材の断面が未設定です")?;
                let material = s
                    .material
                    .and_then(|id| self.materials.get(id.index()))
                    .ok_or("周辺支持材の材料が未設定です")?;
                let concrete = material.category == MaterialCategory::Concrete;
                if !concrete {
                    continue;
                }
                scalar(s.width, false, "支持材幅")?;
                scalar(s.depth, false, "支持材せい")?;
                let a = poly[i];
                let b = poly[(i + 1) % poly.len()];
                let dx = b[0] - a[0];
                let dz = b[1] - a[1];
                let length = dx.hypot(dz);
                let inside = [-dz * sign / length, dx * sign / length];
                let direction = [axis[0] * inside[0], axis[1] * inside[0], inside[1]];
                let inset = crate::face_distance::wall_support_face(self, e.support, direction)?;
                scalar(inset, false, "周辺支持材フェース距離")?;
                if inset > 0.0 {
                    if (0..poly.len()).any(|j| {
                        let a = poly[j];
                        let b = poly[(j + 1) % poly.len()];
                        let c = poly[(j + 2) % poly.len()];
                        ((b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])) * sign
                            < -1e-6
                    }) {
                        return Err("凹壁領域の周辺材内法が未解決です".into());
                    }
                    clear = if sign > 0.0 {
                        clip_half_plane(&clear, a, b, inset)
                    } else {
                        clip_half_plane(&clear, b, a, inset)
                    };
                }
            }
        }
        validate_polygon(&clear)?;
        if signed_area(&clear) < 0.0 {
            clear.reverse();
        }
        for p in &mut extra_polys {
            validate_polygon(p)?;
            if signed_area(p) < 0.0 {
                p.reverse();
            }
        }
        let mut region = vec![vec![clear.clone()]];
        region.extend(extra_polys.into_iter().map(|p| vec![p]));
        // 内包検査は開口控除前の全実領域で行い、重複開口も正当とする。
        let clear_region = region.clone();
        let gross = shapes_area(&region);
        let clear_z_range = region
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[1])
            .fold([f64::INFINITY, f64::NEG_INFINITY], |r, z| {
                [r[0].min(z), r[1].max(z)]
            });
        scalar(gross, true, "躯体実面積")?;
        let mut unknown = false;
        let mut unknown_area = 0.0;
        for o in &plate.openings {
            scalar(o.width, true, "開口幅")?;
            scalar(o.height, true, "開口高さ")?;
            scalar(o.width * o.height, true, "開口面積")?;
            let Some([x, z]) = o.offset else {
                unknown = true;
                unknown_area += o.width * o.height;
                continue;
            };
            if !x.is_finite() || !z.is_finite() {
                return Err("開口位置が不正です".into());
            }
            let z = z + z_range[0];
            let r = vec![
                [x, z],
                [x + o.width, z],
                [x + o.width, z + o.height],
                [x, z + o.height],
            ];
            validate_polygon(&r)?;
            let covered =
                shapes_area(&clear_region.overlay(&r, OverlayRule::Intersect, FillRule::NonZero));
            if (covered - o.width * o.height).abs() > 1e-6 * (o.width * o.height).max(1.0) {
                return Err("開口が躯体実領域の外側にあります".into());
            }
            region = region.overlay(&r, OverlayRule::Difference, FillRule::NonZero);
        }
        if plate.openings.is_empty() && plate.opening_area > 0.0 {
            unknown = true;
            unknown_area = plate.opening_area;
        }
        if unknown && plate.openings.len() > 1 {
            return Err("複数開口の位置不足により和集合総量が未算定です".into());
        }
        let known_area = shapes_area(&region);
        let net = known_area - unknown_area;
        if net < 0.0 || !net.is_finite() {
            return Err("開口面積が躯体実面積を超えています".into());
        }
        let (design, physical) = match (
            self.wall_plate_section(plate),
            self.wall_plate_material(plate),
        ) {
            (Some(s), Some(m)) => {
                let t = s.thickness.ok_or("板厚が未設定です")?;
                scalar(t, true, "板厚")?;
                scalar(m.density, true, "物理密度")?;
                let gamma = m.design_unit_weight_n_per_mm3();
                scalar(gamma, true, "設計単位重量")?;
                let physical = if matches!(
                    s.shape,
                    Some(crate::section_shape::SectionShape::RcWall { .. })
                ) {
                    SectionMassProperties::try_from_section(s, Some(m), None, None, None)?
                        .mass_per_length
                        / 1000.0
                        * crate::units::GRAVITY_MM_S2
                } else {
                    t * m.density * crate::units::GRAVITY_MM_S2
                };
                (t * gamma, physical)
            }
            (Some(_), None) => return Err("割当済み壁断面の主材料が未設定です".into()),
            (None, _) if plate.section.is_some() => return Err("壁断面の参照が不正です".into()),
            _ => (0.0, 0.0),
        };
        let finish = plate.finish_intensity();
        let intensity = WallWeightTotals {
            design_n: design + finish,
            physical_n: physical + finish,
            matrix_n: if self.wall_plate_becomes_element(plate) {
                physical
            } else {
                0.0
            },
        };
        for v in [
            design,
            physical,
            finish,
            intensity.design_n,
            intensity.physical_n,
            intensity.matrix_n,
        ] {
            scalar(v, false, "壁重量強度")?;
        }
        let totals = WallWeightTotals {
            design_n: intensity.design_n * net + plate.opening_weight,
            physical_n: intensity.physical_n * net + plate.opening_weight,
            matrix_n: intensity.matrix_n * net
                + if self.wall_plate_becomes_element(plate) {
                    plate.opening_weight
                } else {
                    0.0
                },
        };
        for v in [
            totals.design_n,
            totals.physical_n,
            totals.matrix_n,
            design * net,
            physical * net,
            finish * net,
        ] {
            scalar(v, false, "壁重量総量")?;
        }
        let mut reasons = Vec::new();
        if unknown {
            reasons.push("開口位置が未設定");
        }
        if plate.opening_weight > 0.0 {
            reasons.push("開口部重量の位置・分布が未設定");
        }
        if gross <= 0.0 {
            return Err("壁実面積がゼロです".into());
        }
        Ok(WallWeight {
            plate: plate.id,
            totals,
            net_area_mm2: net,
            design_body_n: design * net,
            physical_body_n: physical * net,
            finish_n: finish * net,
            opening_n: plate.opening_weight,
            partition_issue: (!reasons.is_empty()).then(|| reasons.join("、")),
            z_range_mm: clear_z_range,
            x_range_mm: clear
                .iter()
                .map(|p| p[0])
                .fold([f64::INFINITY, f64::NEG_INFINITY], |r, x| {
                    [r[0].min(x), r[1].max(x)]
                }),
            region,
            intensity,
            origin_xy: [origin[0], origin[1]],
            axis_xy: axis,
        })
    }
}

impl WallWeight {
    /// 水平帯`[lower,upper]` [mm]との交差重量 [N]。位置不足はID付きエラー。
    pub fn band(&self, lower: f64, upper: f64) -> Result<WallWeightTotals, String> {
        if let Some(reason) = &self.partition_issue {
            return Err(format!("壁版 {}: 階配分未算定（{reason}）", self.plate.0));
        }
        if !lower.is_finite() || !upper.is_finite() || lower >= upper {
            return Err(format!("壁版 {}: 階帯境界が未設定・不正です", self.plate.0));
        }
        let bounds = self
            .region
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[0])
            .fold([f64::INFINITY, f64::NEG_INFINITY], |r, x| {
                [r[0].min(x), r[1].max(x)]
            });
        let clip = vec![
            [bounds[0] - 1.0, lower],
            [bounds[1] + 1.0, lower],
            [bounds[1] + 1.0, upper],
            [bounds[0] - 1.0, upper],
        ];
        let a = shapes_area(
            &self
                .region
                .overlay(&clip, OverlayRule::Intersect, FillRule::NonZero),
        );
        for v in [
            a,
            self.intensity.design_n * a,
            self.intensity.physical_n * a,
            self.intensity.matrix_n * a,
        ] {
            scalar(v, false, "階帯交差重量")?;
        }
        Ok(WallWeightTotals {
            design_n: self.intensity.design_n * a,
            physical_n: self.intensity.physical_n * a,
            matrix_n: self.intensity.matrix_n * a,
        })
    }
}

/// 階帯へ積分した同一物理壁版の内訳 [N, mm, t·mm²]。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WallStoryWeight {
    pub plate: crate::ids::WallPlateId,
    pub total: WallWeightTotals,
    pub band: WallWeightTotals,
    pub center_xy_mm: [f64; 2],
    pub inertia_t_mm2: f64,
}

impl WallWeight {
    /// 有限階帯との交差総量・面積重心・物理質量の回転慣性を返す。
    pub fn story_band(&self, lower: f64, upper: f64) -> Result<WallStoryWeight, String> {
        let band = self.band(lower, upper)?;
        let bounds = self
            .region
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[0])
            .fold([f64::INFINITY, f64::NEG_INFINITY], |r, x| {
                [r[0].min(x), r[1].max(x)]
            });
        let clip = vec![
            [bounds[0] - 1.0, lower],
            [bounds[1] + 1.0, lower],
            [bounds[1] + 1.0, upper],
            [bounds[0] - 1.0, upper],
        ];
        let region = self
            .region
            .overlay(&clip, OverlayRule::Intersect, FillRule::NonZero);
        let mut m = [0.0; 3];
        for shape in &region {
            for (j, p) in shape.iter().enumerate() {
                let sign = if j == 0 { 1.0 } else { -1.0 };
                let orient = signed_area(p).signum() * sign;
                for i in 0..p.len() {
                    let a = p[i];
                    let b = p[(i + 1) % p.len()];
                    let cross = (a[0] * b[1] - b[0] * a[1]) * orient;
                    m[0] += cross / 2.0;
                    m[1] += (a[0] + b[0]) * cross / 6.0;
                    m[2] += (a[0] * a[0] + a[0] * b[0] + b[0] * b[0]) * cross / 12.0;
                }
            }
        }
        if m.iter().any(|v| !v.is_finite()) {
            return Err(format!(
                "壁版 {}: 階帯の面積モーメントが非有限です",
                self.plate.0
            ));
        }
        let mean = if m[0] > 0.0 { m[1] / m[0] } else { 0.0 };
        let result = WallStoryWeight {
            plate: self.plate,
            total: self.totals,
            band,
            center_xy_mm: [
                self.origin_xy[0] + self.axis_xy[0] * mean,
                self.origin_xy[1] + self.axis_xy[1] * mean,
            ],
            inertia_t_mm2: self.intensity.physical_n / crate::units::GRAVITY_MM_S2
                * (m[2] - m[0] * mean * mean).max(0.0),
        };
        if result.center_xy_mm.iter().any(|v| !v.is_finite()) || !result.inertia_t_mm2.is_finite() {
            return Err(format!(
                "壁版 {}: 階帯の重心・慣性が非有限です",
                self.plate.0
            ));
        }
        Ok(result)
    }
}

impl Model {
    /// 保存した階帯内訳が現在の壁実領域と一致するか。DL方式は比較対象外。
    pub fn validate_wall_weight_generation(&self) -> Result<(), String> {
        if let Some(wall) = self.elements.iter().find(|e| {
            e.kind == super::ElementKind::Wall && !self.generated_wall_origins.contains_key(&e.id)
        }) {
            return Err(format!(
                "解析壁要素 {}: 物理壁版IDが未定義です。壁版入力へ変換して重量を再生成してください",
                wall.id.0
            ));
        }
        if self.wall_weight_generation == Some(WallWeightGenerationMode::GravityCasesOnly) {
            if let Some(w) = self.stories.iter().flat_map(|s| &s.wall_weights).next() {
                return Err(format!(
                    "壁版 {}: ケースのみ指定と幾何階内訳が混在しています。階を再生成してください",
                    w.plate.0
                ));
            }
            return Ok(());
        }
        let saved: Vec<_> = self.stories.iter().flat_map(|s| &s.wall_weights).collect();
        if self.wall_plates.is_empty() && saved.is_empty() {
            return Ok(());
        }
        let id = self
            .wall_plates
            .first()
            .map(|p| p.id)
            .or_else(|| saved.first().map(|w| w.plate))
            .unwrap();
        if self.wall_weight_generation.is_none() || self.stories.is_empty() {
            return Err(format!(
                "壁版 {}: 階重量の生成方式が未設定です。階を再生成してください",
                id.0
            ));
        }
        let levels: Vec<_> = self.stories.iter().map(|s| s.elevation).collect();
        if levels.iter().any(|z| !z.is_finite()) || levels.windows(2).any(|p| p[0] >= p[1]) {
            return Err(format!("壁版 {}: 階帯境界が不正です", id.0));
        }
        for p in &self.wall_plates {
            let weight = self.wall_weight(p)?;
            if weight.z_range_mm[0] < levels[0] - 1e-6
                || weight.z_range_mm[1] > *levels.last().unwrap() + 1e-6
            {
                return Err(format!("壁版 {}: 未定義端帯で実領域を覆えません", p.id.0));
            }
            for (i, story) in self.stories.iter().enumerate() {
                let lo = if i == 0 {
                    levels[0]
                } else {
                    (levels[i - 1] + levels[i]) / 2.0
                };
                let hi = if i + 1 == levels.len() {
                    levels[i]
                } else {
                    (levels[i] + levels[i + 1]) / 2.0
                };
                if lo >= hi {
                    continue;
                }
                let expected = weight.story_band(lo, hi)?;
                let rows: Vec<_> = story
                    .wall_weights
                    .iter()
                    .filter(|w| w.plate == p.id)
                    .collect();
                if rows.len() != 1 || !wall_story_weights_close(rows[0], &expected) {
                    return Err(format!("壁版 {}: 階 {} の重量・物理質量内訳が現在形状と不一致です。階を再生成してください",p.id.0,story.name));
                }
            }
        }
        if saved
            .iter()
            .any(|w| !self.wall_plates.iter().any(|p| p.id == w.plate))
        {
            return Err(format!(
                "壁版 {}: 削除された壁の階内訳が残っています。階を再生成してください",
                id.0
            ));
        }
        Ok(())
    }
}
fn wall_story_weights_close(a: &WallStoryWeight, b: &WallStoryWeight) -> bool {
    let close = |x: f64, y: f64| {
        x.is_finite() && y.is_finite() && (x - y).abs() <= 1e-7 * x.abs().max(y.abs()).max(1.0)
    };
    let values = |w: &WallStoryWeight| {
        [
            w.total.design_n,
            w.total.physical_n,
            w.total.matrix_n,
            w.band.design_n,
            w.band.physical_n,
            w.band.matrix_n,
            w.center_xy_mm[0],
            w.center_xy_mm[1],
            w.inertia_t_mm2,
        ]
    };
    values(a)
        .into_iter()
        .zip(values(b))
        .all(|(x, y)| close(x, y))
}

impl WallWeight {
    /// 実領域の取付き水平軸への射影。各区間の局所x両端[mm]と設計線荷重[N/mm]。
    /// 多角形頂点・開口の境界で分けるため区間内の強度は正確に線形となる。
    pub fn projected_design_line_loads(&self) -> Result<Vec<[f64; 4]>, String> {
        if let Some(reason) = &self.partition_issue {
            return Err(format!("壁版 {}: DL分布未算定（{reason}）", self.plate.0));
        }
        let mut cuts: Vec<_> = self
            .region
            .iter()
            .flatten()
            .flatten()
            .map(|p| p[0])
            .collect();
        cuts.extend(self.x_range_mm);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        let height = |x: f64| -> f64 {
            self.region
                .iter()
                .map(|shape| {
                    shape
                        .iter()
                        .enumerate()
                        .map(|(ring, p)| {
                            let mut zs = Vec::new();
                            for i in 0..p.len() {
                                let a = p[i];
                                let b = p[(i + 1) % p.len()];
                                if x > a[0].min(b[0]) && x < a[0].max(b[0]) {
                                    zs.push(a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0]));
                                }
                            }
                            zs.sort_by(f64::total_cmp);
                            let length: f64 = zs
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|pair| pair[1] - pair[0])
                                .sum();
                            if ring == 0 {
                                length
                            } else {
                                -length
                            }
                        })
                        .sum::<f64>()
                })
                .sum()
        };
        let mut loads = Vec::new();
        for pair in cuts.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            if a >= b {
                continue;
            }
            let h1 = height(a + (b - a) / 3.0);
            let h2 = height(a + 2.0 * (b - a) / 3.0);
            let w1 = (2.0 * h1 - h2) * self.intensity.design_n;
            let w2 = (2.0 * h2 - h1) * self.intensity.design_n;
            if !w1.is_finite() || !w2.is_finite() || w1 < -1e-8 || w2 < -1e-8 {
                return Err(format!("壁版 {}: DL射影線荷重が不正です", self.plate.0));
            }
            if w1 + w2 > 0.0 {
                loads.push([a, b, w1.max(0.0), w2.max(0.0)]);
            }
        }
        Ok(loads)
    }
    /// 取付き水平区間との交差設計重量[N]。床領域へのDL伝達割合にも用いる。
    pub fn design_weight_between_x(&self, lower: f64, upper: f64) -> Result<f64, String> {
        let mut total = 0.0;
        for [a, b, w1, w2] in self.projected_design_line_loads()? {
            let lo = lower.max(a);
            let hi = upper.min(b);
            if lo >= hi {
                continue;
            }
            let w = |x: f64| w1 + (w2 - w1) * (x - a) / (b - a);
            total += (w(lo) + w(hi)) * (hi - lo) / 2.0;
        }
        if !total.is_finite() {
            return Err(format!("壁版 {}: DL交差量が非有限です", self.plate.0));
        }
        Ok(total)
    }
}
