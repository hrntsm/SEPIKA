//! 標準配筋の原入力。非適用の属性も省略を含めて保存する。
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StbRebarElement {
    pub element: String,
    pub attributes: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StbRebarArrangement {
    pub container: StbRebarElement,
    pub positions: Vec<StbRebarElement>,
    #[serde(default)]
    pub auxiliary: Vec<StbRebarElement>,
    /// 解析型へ写した実効入力。原入力や共通条件だけが変わった場合も旧座標を使わない。
    #[serde(default)]
    pub projected_effective: Vec<StbRebarElement>,
    /// 原入力から生成した形状。直接編集での旧原入力の再適用を防ぐ。
    pub projected_shape: Option<crate::section_shape::SectionShape>,
}

impl StbRebarArrangement {
    pub fn family(&self) -> &str {
        self.container
            .element
            .strip_prefix("StbSecBarArrangement")
            .unwrap_or("")
    }

    /// 条件なしは非適用。trueの場合だけ、指定された属性を既定値として供給する。
    pub fn effective_attributes(
        &self,
        input: &StbRebarElement,
        conditions: &[StbRebarElement],
    ) -> BTreeMap<String, String> {
        let mut result = input.attributes.clone();
        result.extend(self.container.attributes.clone());
        let beam = self.family().starts_with("Beam");
        for group in ["RebarPosition", "BarWeb", "BarSpacing"] {
            let condition = conditions
                .iter()
                .find(|c| c.element == format!("Stb{}_{}Apply", self.family(), group));
            let keys: Vec<_> = result
                .keys()
                .filter(|key| match group {
                    "RebarPosition" => {
                        key.starts_with("depth_cover")
                            || key.starts_with("center")
                            || key.starts_with("interval")
                            || key.starts_with("length_to_center")
                            || key.starts_with("length_bar_")
                    }
                    "BarWeb" => key.ends_with("_web"),
                    _ => key.contains("bar_spacing"),
                })
                .cloned()
                .collect();
            let Some(condition) = condition else {
                for key in keys {
                    result.remove(&key);
                }
                continue;
            };
            if matches!(
                condition.attributes.get("set_default").map(String::as_str),
                Some("true" | "1")
            ) {
                for (key, value) in &condition.attributes {
                    if key == "set_default" {
                        continue;
                    }
                    let targets: Vec<String> = if group != "RebarPosition" {
                        vec![key.clone()]
                    } else if beam {
                        match key.as_str() {
                            "depth_cover_top_bottom" => {
                                vec!["depth_cover_top".into(), "depth_cover_bottom".into()]
                            }
                            "center_top_bottom" => {
                                vec!["center_top".into(), "center_bottom".into()]
                            }
                            "depth_cover_side" | "center_side" => vec![key.clone()],
                            _ => vec![key.clone()],
                        }
                    } else if matches!(key.as_str(), "depth_cover" | "center") {
                        ["start_X", "end_X", "start_Y", "end_Y"]
                            .iter()
                            .map(|side| format!("{key}_{side}"))
                            .collect()
                    } else {
                        vec![key.clone()]
                    };
                    for target in targets {
                        result.entry(target).or_insert_with(|| value.clone());
                    }
                }
            }
        }
        result
    }

    /// 必須値・正の本数・位置の一意性を検査する。XSD検証とは独立した意味検査。
    pub fn validate_standard(&self) -> Result<(), String> {
        let first = self.positions.first().ok_or("配筋部位がありません")?;
        let tag = &first.element;
        let expected: &[&str] = if tag.ends_with("_StartEnd") {
            &["START", "END"]
        } else if tag.ends_with("_ThreeTypes") {
            &["START", "CENTER", "END"]
        } else if tag.ends_with("_RectNotSame") || tag.ends_with("_CircleNotSame") {
            &["BASE", "TOP"]
        } else {
            &[""]
        };
        let mut actual = Vec::new();
        for part in &self.positions {
            if &part.element != tag {
                return Err("配筋系列が混在しています".into());
            }
            let pos = part.attributes.get("pos").map(String::as_str).unwrap_or("");
            if !expected.contains(&pos) || actual.contains(&pos) {
                return Err(format!("配筋pos {pos} が不正または重複しています"));
            }
            actual.push(pos);
            let beam = self.family().starts_with("Beam");
            let circle = tag.contains("Circle");
            let required = if beam {
                vec![
                    "D_main",
                    "N_main_top_1st",
                    "N_main_bottom_1st",
                    "D_stirrup",
                    "N_stirrup",
                    "pitch_stirrup",
                ]
            } else if circle {
                vec!["D_main", "D_band", "N_main", "N_band", "pitch_band"]
            } else {
                vec![
                    "D_main",
                    "D_band",
                    "N_main_X_1st",
                    "N_main_Y_1st",
                    "N_main_total",
                    "pitch_band",
                    "N_band_direction_X",
                    "N_band_direction_Y",
                ]
            };
            for key in required {
                if !part.attributes.contains_key(key) {
                    return Err(format!(
                        "{} / {pos}: 必須属性 {key} がありません",
                        part.element
                    ));
                }
            }
            for (key, value) in &part.attributes {
                let stage = key.rsplit_once('_').and_then(|(_, suffix)| {
                    ["st", "nd", "rd", "th"]
                        .iter()
                        .find_map(|ordinal| suffix.strip_suffix(ordinal))
                        .and_then(|number| number.parse::<u32>().ok())
                });
                if key.starts_with("N_") && stage.is_some_and(|n| n > if beam { 3 } else { 2 }) {
                    return Err(format!("{key} は標準段数を超えています"));
                }
                if value.trim().is_empty() {
                    return Err(format!("{key} が空です"));
                }
                if key.starts_with("N_") && {
                    let integer = value.trim().strip_prefix('+').unwrap_or(value.trim());
                    integer.is_empty()
                        || !integer.bytes().all(|b| b.is_ascii_digit())
                        || !integer.bytes().any(|b| b != b'0')
                } {
                    return Err(format!("{key} は正の整数が必要です"));
                }
                if key.starts_with("pitch_")
                    && !value.parse::<f64>().is_ok_and(|n| n.is_finite() && n > 0.0)
                {
                    return Err(format!("{key} は正の長さが必要です"));
                }
            }
            if !beam && !circle {
                if let Some(total) = self.known_main_total(part) {
                    let declared: u64 = part.attributes["N_main_total"]
                        .parse()
                        .map_err(|_| "N_main_total が不正")?;
                    if total != declared {
                        return Err(format!("N_main_total {declared} と隅筋重複除去後の実本数 {total} が一致しません（軸筋は別扱い）"));
                    }
                }
            }
        }
        if actual.len() != expected.len() {
            return Err("配筋posが不足しています".into());
        }
        Ok(())
    }

    /// Only independently confirmed shared-corner topologies may reject a declared total.
    /// Other standard-valid arrangements remain authoritative and are gated at consumption.
    fn known_main_total(&self, part: &StbRebarElement) -> Option<u64> {
        if part.attributes.keys().any(|key| key.contains("2nd_main")) {
            return None;
        }
        let layers = |direction: &str| -> Option<Vec<u32>> {
            ["1st", "2nd"]
                .iter()
                .filter_map(|stage| {
                    part.attributes
                        .get(&format!("N_main_{direction}_{stage}"))
                        .map(|value| value.parse::<u32>().ok())
                })
                .collect()
        };
        let x = layers("X")?;
        let y = layers("Y")?;
        let corner = self
            .container
            .attributes
            .get("kind_corner")
            .map(String::as_str)
            .unwrap_or("NONE");
        let shared = match (corner, x.as_slice(), y.as_slice()) {
            ("NONE", [nx], [ny]) if *nx >= 2 && *ny >= 2 => 4,
            // Official p124/p125: four shared L-shaped groups of three bars.
            ("NONE" | "DIR_XY", [7, 2], [7, 2]) => 12,
            // Official p127: four shared 2-by-2 groups; axial bars are separate.
            ("NONE", [7, 7], [7, 6]) => 16,
            _ => return None,
        };
        Some(2 * x.iter().chain(&y).map(|n| u64::from(*n)).sum::<u64>() - shared)
    }
}

impl Model {
    pub fn stb_rebar_analysis_diagnostics(&self) -> Vec<String> {
        let used: Vec<_> = self
            .elements
            .iter()
            .filter_map(|e| e.section)
            .chain(self.beams().chain(self.posts()).filter_map(|m| m.section))
            .collect();
        self.stb_rebar_diagnostics()
            .into_iter()
            .filter(|message| {
                used.iter().any(|id| {
                    message.starts_with(&format!("断面 {}:", id.0))
                        || message.starts_with(&format!("断面 {} /", id.0))
                })
            })
            .collect()
    }

    pub fn ensure_stb_rebar_supported(&self, section: SectionId) -> Result<(), String> {
        let prefix = format!("断面 {}:", section.0);
        let alternate = format!("断面 {} /", section.0);
        let errors: Vec<_> = self
            .stb_rebar_diagnostics()
            .into_iter()
            .filter(|e| e.starts_with(&prefix) || e.starts_with(&alternate))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// 保存可能でも現解析型が表現できない標準配筋を、計算前に理由付きで拒否する。
    pub fn stb_rebar_diagnostics(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for input in &self.stb_strengths.sections {
            let Some(raw) = &input.rebar else {
                continue;
            };
            let prefix = format!("断面 {}", input.section.0);
            let Some(section) = self.sections.get(input.section.index()) else {
                continue;
            };
            let effective: Vec<_> = raw
                .positions
                .iter()
                .map(|p| StbRebarElement {
                    element: p.element.clone(),
                    attributes: raw.effective_attributes(p, &self.stb_strengths.apply_conditions),
                })
                .collect();
            if effective != raw.projected_effective {
                errors.push(format!(
                    "{prefix}: 原配筋・Apply条件と解析用入力が一致しません"
                ));
                continue;
            }
            if raw.projected_shape != section.shape {
                errors.push(format!("{prefix}: 原配筋と編集後形状が一致しません"));
                continue;
            }
            if let Err(reason) = raw.validate_standard() {
                errors.push(format!("{prefix}: {reason}"));
                continue;
            }
            if !raw.auxiliary.is_empty() {
                errors.push(format!(
                    "{prefix}: X形付加配筋は現解析モデルで表現できません"
                ));
            }
            if raw.positions.len() != 1 {
                errors.push(format!(
                    "{prefix}: 位置別配筋は現解析モデルで表現できません"
                ));
            }
            if raw
                .container
                .attributes
                .get("kind_corner")
                .is_some_and(|v| v != "NONE")
            {
                errors.push(format!(
                    "{prefix}: kind_cornerの寄せ配置は現解析モデルで表現できません"
                ));
            }
            if matches!(
                raw.container.attributes.get("isSpiral").map(String::as_str),
                Some("true" | "1")
            ) {
                errors.push(format!(
                    "{prefix}: スパイラル帯筋は現解析モデルで表現できません"
                ));
            }
            for pos in &raw.positions {
                let effective = raw.effective_attributes(pos, &self.stb_strengths.apply_conditions);
                if pos.element.contains("Rect") && raw.known_main_total(pos).is_none() {
                    errors.push(format!(
                        "{prefix}: 隅筋共有位相とN_main_totalを独立に解決できません"
                    ));
                }
                if pos.element.contains("Circle")
                    && effective.get("N_band").and_then(|v| v.parse::<u32>().ok()) != Some(2)
                {
                    errors.push(format!(
                        "{prefix}: 円形帯筋N_bandは現解析モデルの2脚で表現できません"
                    ));
                }
                if effective
                    .iter()
                    .any(|(k, v)| k.starts_with("N_") && v.parse::<u32>().is_err())
                {
                    errors.push(format!("{prefix}: 鉄筋本数が現解析型の範囲を超えています"));
                }
                if effective.keys().any(|k| {
                    k.starts_with("N_main_") && (k.ends_with("_2nd") || k.ends_with("_3rd"))
                }) {
                    errors.push(format!("{prefix}: 段間位置を標準入力から解決できません"));
                }
                if effective.keys().any(|k| {
                    k.contains("2nd_main")
                        || k.contains("axial")
                        || k.contains("bar_spacing")
                        || k.ends_with("_web")
                        || k.starts_with("interval")
                        || k == "center_interval"
                        || k.starts_with("length_to_center")
                        || k.starts_with("length_bar_")
                }) {
                    errors.push(format!("{prefix} / {}: 副主筋・軸筋・腹筋・巾止筋・指定間隔は現解析モデルで表現できません", pos.element));
                }
                let beam = raw.family().starts_with("Beam");
                let sides: &[&str] = if beam {
                    &["top", "bottom", "side"]
                } else {
                    &["start_X", "end_X", "start_Y", "end_Y"]
                };
                let mut distances = Vec::new();
                let main = effective
                    .get("D_main")
                    .and_then(|v| v.strip_prefix('D').unwrap_or(v).parse::<f64>().ok());
                let shear = effective
                    .get(if beam { "D_stirrup" } else { "D_band" })
                    .and_then(|v| v.strip_prefix('D').unwrap_or(v).parse::<f64>().ok());
                for side in sides {
                    let center = effective
                        .get(&format!("center_{side}"))
                        .and_then(|v| v.parse::<f64>().ok());
                    let cover = effective
                        .get(&format!("depth_cover_{side}"))
                        .and_then(|v| v.parse::<f64>().ok());
                    if let Some(distance) = center {
                        distances.push(distance);
                    } else if cover.is_some() {
                        errors.push(format!(
                            "{prefix}: depth_coverから主筋中心を独立に解決できません"
                        ));
                    }
                }
                if distances
                    .first()
                    .is_some_and(|first| distances.iter().any(|d| (d - first).abs() > 1e-6))
                {
                    errors.push(format!(
                        "{prefix}: 面別の異なる主筋中心位置は現解析モデルで表現できません"
                    ));
                }
                if distances.len() != sides.len() {
                    errors.push(format!("{prefix}: 全面の主筋中心位置が未解決です"));
                }
                if main.is_none_or(|d| !d.is_finite() || d <= 0.0)
                    || shear.is_none_or(|d| !d.is_finite() || d <= 0.0)
                {
                    errors.push(format!(
                        "{prefix}: 主筋またはせん断補強筋の径を解決できません"
                    ));
                }
                if beam {
                    if let Some(
                        crate::section_shape::SectionShape::RcBeamRect { b, d, rebar }
                        | crate::section_shape::SectionShape::SrcBeamRect { b, d, rebar, .. },
                    ) = &section.shape
                    {
                        if rebar.bar_positions(*b, *d).is_err() {
                            errors
                                .push(format!("{prefix}: 標準梁配筋を現解析座標へ写像できません"));
                        }
                    }
                }
                if !beam {
                    if let Some(
                        crate::section_shape::SectionShape::RcColumnRect { b, d, rebar }
                        | crate::section_shape::SectionShape::SrcColumnRect { b, d, rebar, .. },
                    ) = &section.shape
                    {
                        if rebar.bar_positions(*b, *d).is_err() {
                            errors
                                .push(format!("{prefix}: 標準柱配筋を現解析座標へ写像できません"));
                        }
                    }
                }
                if let Some(crate::section_shape::SectionShape::RcColumnCircle { d, rebar }) =
                    &section.shape
                {
                    if rebar.bar_positions(*d).is_err() {
                        errors.push(format!(
                            "{prefix}: 標準円形柱配筋を現解析座標へ写像できません"
                        ));
                    }
                }
                let has_position = effective
                    .keys()
                    .any(|k| k.starts_with("depth_cover") || k.starts_with("center"));
                if !has_position {
                    errors.push(format!("{prefix}: 配筋位置が非適用または未指定です"));
                }
            }
        }
        errors
    }
}

/// 役割別の意味入力。未解決座標を0や既定配置へ置換しない。
#[derive(Clone, Debug, PartialEq)]
pub struct StbRebarRole {
    pub position: Option<String>,
    pub part: String,
    pub diameter: Option<String>,
    pub counts: BTreeMap<String, u32>,
    pub pitch_mm: Option<f64>,
    pub applied: bool,
    pub strength: Result<ResolvedStrength, String>,
    pub center_mm: BTreeMap<String, f64>,
    pub cover_mm: BTreeMap<String, f64>,
    pub points_mm: Option<Vec<crate::section_shape::RebarPoint>>,
    pub unresolved_position: Option<String>,
}

impl Model {
    /// 元部位・posと既存強度resolverを一致させた役割別入力を返す。
    pub fn stb_rebar_roles(&self, section: SectionId) -> Vec<StbRebarRole> {
        let Some(input) = self
            .stb_strengths
            .sections
            .iter()
            .find(|s| s.section == section)
        else {
            return Vec::new();
        };
        let Some(raw) = &input.rebar else {
            return Vec::new();
        };
        let mut roles = Vec::new();
        for position in &raw.positions {
            let attrs = raw.effective_attributes(position, &self.stb_strengths.apply_conditions);
            let pos = position.attributes.get("pos").cloned();
            for part in [
                "main",
                "2nd_main",
                "axial",
                "band",
                "stirrup",
                "web",
                "bar_spacing",
            ] {
                let original = input.reinforcement.iter().find(|bar| {
                    bar.element == position.element && bar.position == pos && bar.part == part
                });
                let counts: BTreeMap<_, _> = attrs
                    .iter()
                    .filter(|(key, _)| {
                        **key == format!("N_{part}") || key.starts_with(&format!("N_{part}_"))
                    })
                    .filter(|(key, _)| !key.ends_with("_total"))
                    .filter_map(|(key, value)| value.parse().ok().map(|value| (key.clone(), value)))
                    .collect();
                if original.is_none()
                    && counts.is_empty()
                    && !attrs.contains_key(&format!("D_{part}"))
                {
                    continue;
                }
                let applied = match part {
                    "web" => attrs.contains_key("N_web"),
                    "bar_spacing" => attrs.keys().any(|key| key.starts_with("N_bar_spacing")),
                    _ => true,
                };
                let number = |key: &str| attrs.get(key).and_then(|v| v.parse::<f64>().ok());
                let mut effective_strength =
                    original.cloned().unwrap_or_else(|| StbRebarStrength {
                        element: position.element.clone(),
                        part: part.into(),
                        position: pos.clone(),
                        native_material: None,
                        diameter: None,
                        strength: attrs.get(&format!("strength_{part}")).cloned(),
                    });
                if effective_strength.diameter.is_none() {
                    effective_strength.diameter = attrs.get(&format!("D_{part}")).cloned();
                }
                let main_geometry =
                    part == "main" && self.ensure_stb_rebar_supported(section).is_ok();
                let points = if main_geometry {
                    use crate::section_shape::SectionShape;
                    self.sections
                        .get(section.index())
                        .and_then(|s| s.shape.as_ref())
                        .and_then(|shape| match shape {
                            SectionShape::RcBeamRect { b, d, rebar }
                            | SectionShape::SrcBeamRect { b, d, rebar, .. } => {
                                rebar.bar_positions(*b, *d).ok()
                            }
                            SectionShape::RcColumnRect { b, d, rebar }
                            | SectionShape::SrcColumnRect { b, d, rebar, .. } => {
                                rebar.bar_positions(*b, *d).ok()
                            }
                            SectionShape::RcColumnCircle { d, rebar } => {
                                rebar.bar_positions(*d).ok()
                            }
                            _ => None,
                        })
                } else {
                    None
                };
                roles.push(StbRebarRole {
                    position: pos.clone(),
                    part: part.into(),
                    diameter: effective_strength.diameter.clone(),
                    counts,
                    pitch_mm: number(&format!("pitch_{part}")),
                    applied,
                    strength: if !applied {
                        Err("非適用".into())
                    } else {
                        self.resolve_stb_rebar(&effective_strength)
                    },
                    center_mm: attrs
                        .iter()
                        .filter(|(k, _)| k.starts_with("center_"))
                        .filter_map(|(k, v)| v.parse().ok().map(|n| (k.clone(), n)))
                        .collect(),
                    cover_mm: attrs
                        .iter()
                        .filter(|(k, _)| k.starts_with("depth_cover_"))
                        .filter_map(|(k, v)| v.parse().ok().map(|n| (k.clone(), n)))
                        .collect(),
                    unresolved_position: points.is_none().then(|| {
                        if applied {
                            "標準入力を現解析座標へ写像できません".into()
                        } else {
                            "非適用".into()
                        }
                    }),
                    points_mm: points,
                });
            }
        }
        roles
    }
}
