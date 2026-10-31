//! 非線形解析の入力チェック。
//!
//! - [`nonlinear_input_issues`] — 部材耐力を算定できない設定不備の列挙
//! - [`ensure_nonlinear_input`] — 不備があれば是正内容を示すエラーを返す

use sepika_core::model::{ElementData, ElementKind, Model};
use sepika_core::section_shape::SectionShape;

/// エラーメッセージへ列挙する不備の最大件数（超過分は件数のみを示す）。
const MAX_LISTED: usize = 5;

/// 実解析名・幾何剛性・要素質量形式に対する壁側柱接続の適用条件を検証する。
/// `kind=None` は線形構造、`mass=None` は要素質量を使わない解析を表す。
pub fn ensure_wall_side_column_analysis(
    model: &Model,
    kind: Option<sepika_core::model::AnalysisKind>,
    analysis: &str,
    use_kg: bool,
    mass: Option<crate::behavior::MassOption>,
) -> Result<(), String> {
    let index = sepika_core::model::WallSideColumnIndex::build(model);
    let label = match kind {
        None => "線形解析",
        Some(sepika_core::model::AnalysisKind::Incremental) => "増分解析",
        Some(sepika_core::model::AnalysisKind::TimeHistory) => "非線形時刻歴解析",
    };
    for data in &model.elements {
        if let Some(issue) = wall_side_column_input_issue_with_index(data, model, kind, &index) {
            return Err(issue.replace(&format!("{label}:"), &format!("{analysis}:")));
        }
    }
    for data in model
        .elements
        .iter()
        .filter(|data| data.kind.is_primary_frame_support())
    {
        if wall_side_column_normal_with_index(data, model, kind, &index)?.is_none() {
            continue;
        }
        let bindings = index.replacement_bindings(data.id).map_err(|issues| {
            issues
                .iter()
                .map(|issue| wall_side_column_issue_message(issue, kind))
                .collect::<Vec<_>>()
                .join("\n")
        })?;
        let walls = bindings
            .iter()
            .map(|binding| binding.wall.0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let failure = |reason: &str| {
            format!(
                "{analysis}: 部材 ID {}・壁 ID [{walls}] の壁側柱接続: {reason}",
                data.id.0
            )
        };
        if use_kg {
            return Err(failure(
                "壁法線まわりの解放と幾何剛性（P-Δ）の連成は未定式化です。",
            ));
        }
        if matches!(mass, Some(crate::behavior::MassOption::Consistent))
            && model.mass_method != sepika_core::model::MassMethod::LumpedOnly
            && !model.uses_damper_total_weight(data)
        {
            model.validate_local_frames()?;
            if let Some(reason) = elastic_stiffness_input_issue(data, model) {
                return Err(failure(&reason));
            }
            super::build_behavior(data, model)
                .try_mass_matrix(crate::behavior::MassOption::Consistent)
                .map_err(|error| failure(&error))?;
        }
    }
    Ok(())
}

/// 壁側柱の構造判定・解析適用の停止理由を返す。`None` は線形解析を表す。
pub fn wall_side_column_input_issue(
    data: &ElementData,
    model: &Model,
    kind: Option<sepika_core::model::AnalysisKind>,
) -> Option<String> {
    use sepika_core::model::WallSideColumnIndex;
    let index = WallSideColumnIndex::build(model);
    wall_side_column_input_issue_with_index(data, model, kind, &index)
}

/// 同じ入力モデルで構築した共通分類を再利用して、壁側柱の停止理由を返す。
pub fn wall_side_column_input_issue_with_index(
    data: &ElementData,
    model: &Model,
    kind: Option<sepika_core::model::AnalysisKind>,
    index: &sepika_core::model::WallSideColumnIndex,
) -> Option<String> {
    wall_side_column_normal_with_index(data, model, kind, index).err()
}

pub(super) fn wall_side_column_normal_with_index(
    data: &ElementData,
    model: &Model,
    kind: Option<sepika_core::model::AnalysisKind>,
    index: &sepika_core::model::WallSideColumnIndex,
) -> Result<Option<[f64; 3]>, String> {
    let owner_bindings: Vec<_> = index
        .bindings()
        .iter()
        .filter(|binding| binding.column == data.id)
        .collect();
    let walls = owner_bindings
        .iter()
        .map(|binding| binding.wall.0.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let analysis = match kind {
        None => "線形解析",
        Some(sepika_core::model::AnalysisKind::Incremental) => "増分解析",
        Some(sepika_core::model::AnalysisKind::TimeHistory) => "非線形時刻歴解析",
    };
    let failure = |reason: &str| {
        format!(
            "{analysis}: 部材 ID {}・壁 ID [{walls}] の壁側柱接続: {reason}",
            data.id.0
        )
    };
    for binding in &owner_bindings {
        if data.nodes.as_slice() != binding.edge
            && data.nodes.as_slice() != [binding.edge[1], binding.edge[0]]
        {
            return Err(failure(
                "構築用部材の端点が、登録された壁側縁の端点と一致しません。",
            ));
        }
    }
    let owner = model.elements.iter().find(|element| element.id == data.id);
    if !owner_bindings.is_empty()
        && !owner.is_some_and(|element| {
            data.kind.is_primary_frame_support()
                && element.kind.is_primary_frame_support()
                && element.section == data.section
        })
    {
        return Err(failure(
            "構築用部材の断面参照が、登録された側柱の断面・材料・用途の所有と一致しません。",
        ));
    }
    let selected = if data.kind == ElementKind::Wall {
        index.wall_replacement_bindings(data.id)
    } else if data.kind.is_primary_frame_support() {
        index.replacement_bindings(data.id)
    } else {
        return Ok(None);
    };
    let bindings = selected.map_err(|issues| {
        issues
            .iter()
            .map(|issue| wall_side_column_issue_message(issue, kind))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    if bindings.is_empty() || data.kind == ElementKind::Wall {
        return Ok(None);
    }
    if kind.is_some()
        && data.kind == ElementKind::Beam
        && matches!(
            super::resolve_force_regime(data, model),
            super::ResolvedRegime::ConcentratedSpring
        )
    {
        return Err(failure("材端集中ばねと壁法線まわりの解放の連成は未定式化です。ファイバー要素への暗黙置換は行いません。"));
    }
    if kind == Some(sepika_core::model::AnalysisKind::TimeHistory) {
        return Err(failure(
            "壁法線まわりの非線形解放と従属回転の慣性の連成は未定式化です。",
        ));
    }
    Ok(Some(bindings[0].normal))
}

/// 共通分類の全関連IDと停止理由を日本語で返す。`None` は線形解析を表す。
pub fn wall_side_column_issue_message(
    issue: &sepika_core::model::WallSideColumnIssue,
    kind: Option<sepika_core::model::AnalysisKind>,
) -> String {
    use sepika_core::model::WallSideColumnReason as R;
    let reason = match issue.reason {
        R::NotFrameMember => "柱梁の線材ではありません",
        R::NotTwoNodes => "側柱の節点数は2である必要があります",
        R::MissingNode => "参照節点がありません",
        R::NotVertical => "鉛直な側縁ではありません",
        R::NoMatchingWall => "一致する壁側縁がありません",
        R::WallNotSeismic => "耐震壁の成立条件を満たしません",
        R::InvalidWallGeometry => "壁の幾何条件が不正です",
        R::MissingMaterial => "材料が設定されていません",
        R::InvalidPrimaryMaterial => "主材料の区分が不適合です",
        R::MissingSteelMaterial => "内蔵鉄骨・鋼管の材料が設定されていません",
        R::InvalidSteelMaterial => "内蔵鉄骨・鋼管の材料区分が不適合です",
        R::ShapeUseMismatch => "断面形状と柱梁用途が一致しません",
        R::ShapeMaterialMismatch => "断面形状と材料区分が一致しません",
        R::ColumnUseRequired => "側柱には柱用断面が必要です",
        R::GirderUseRequired => "壁上下梁には梁用断面が必要です",
        R::FrameMaterialMismatch => "壁と付帯部材の構造種別が不適合です",
        R::DuplicateColumns => "同一側縁に側柱候補が重複しています",
        R::NonParallelWalls => "非平行な壁で共有する側柱の接続は未定式化です",
        R::InvalidSteelYield => "鋼板の降伏強度が有限正値ではありません",
    };
    let ids = |entries: &[sepika_core::ids::ElemId]| {
        entries
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let analysis = match kind {
        None => "線形解析",
        Some(sepika_core::model::AnalysisKind::Incremental) => "増分解析",
        Some(sepika_core::model::AnalysisKind::TimeHistory) => "非線形時刻歴解析",
    };
    format!(
        "{analysis}: 壁 ID [{}]・部材 ID [{}] の壁側柱判定: {reason}",
        ids(&issue.walls),
        ids(&issue.members)
    )
}

/// ファイバー断面に鋼材領域（形鋼・鋼管・内蔵鉄骨・管壁）を持つ形状か。
fn shape_has_steel_fiber_region(shape: &SectionShape) -> bool {
    !matches!(
        shape,
        SectionShape::RcBeamRect { .. }
            | SectionShape::RcColumnRect { .. }
            | SectionShape::RcColumnCircle { .. }
            | SectionShape::RcWall { .. }
    )
}

/// 非線形解析で部材耐力を算定できない設定不備を列挙する（是正内容を示す日本語文）。
///
/// 検査対象は、非線形解析で降伏を扱う要素に限る:
/// - 耐震壁（`Wall`）: 面内せん断の終局強度 Qu
///   （[`crate::wall::wall_element::WallElement::wall_shear_capacity_issue`]）
/// - 線材（`Beam` / `Fiber` / `MultiSpring`）: 曲げ・せん断の終局耐力に要する材料強度
///
/// 壁上下の剛梁は弾性剛性・形状・質量に必要な入力だけを検査する。
///
/// 仕口パネルは採用柱の解決に失敗した場合も診断する。
/// 弾性としてモデル化することが仕様である要素（`Shell` / `NodalSpring`、
/// および特性を専用の属性で持つ `Isolator` / `Damper` / `Brace`）は対象外。
pub fn nonlinear_input_issues(model: &Model) -> Vec<String> {
    nonlinear_input_issues_for_kind(model, sepika_core::model::AnalysisKind::Incremental)
}

/// 公称値で当該解析種別の履歴則と共通材料・幾何を診断する。未実行側の履歴則は拒否根拠にしない。
pub fn nonlinear_input_issues_for_kind(
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
) -> Vec<String> {
    nonlinear_input_issues_with_basis(model, kind, super::StrengthBasis::Nominal)
}

fn nonlinear_input_issues_with_basis(
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
    basis: super::StrengthBasis,
) -> Vec<String> {
    let mut issues = Vec::new();
    let index = sepika_core::model::WallSideColumnIndex::build(model);
    for elem in &model.elements {
        if let Some(issue) =
            wall_side_column_input_issue_with_index(elem, model, Some(kind), &index)
        {
            if !issues.contains(&issue) {
                issues.push(issue);
            }
            continue;
        }
        if crate::frame::beam::is_elastic_wall_girder(model, elem) {
            if let Some(issue) = member_strength_issue(elem, model) {
                issues.push(issue);
            }
            continue;
        }
        if let Some(issue) = super::springs::rc_reference_target_issue(elem, model) {
            issues.push(format!("部材 ID {} のRC梁基準接続: {issue}", elem.id.0));
            continue;
        }
        let issue = match elem.kind {
            ElementKind::PanelZone => elem.nodes.first().and_then(|node| {
                sepika_core::panel_zone::resolve_explicit_panel_joint(model, *node, &model.elements)
                    .err()
                    .map(|reason| reason.to_string())
            }),
            ElementKind::Wall => crate::wall::wall_element::WallElement::wall_shear_capacity_issue(
                elem, model, basis,
            )
            .map(|reason| {
                format!(
                    "RC壁 {}: {reason}",
                    model.stress_cfg.rc_wall_shear_method.label()
                )
            }),
            ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring => {
                member_strength_issue(elem, model)
                    .or_else(|| {
                        super::springs::rc_reference_issue(
                            elem,
                            model,
                            super::springs::resolve_member_hysteresis(elem, model, kind),
                            basis,
                        )
                        .map(|issue| format!("部材 ID {} のRC梁基準接続: {issue}", elem.id.0))
                    })
                    .or_else(|| {
                        let fiber = elem.kind != ElementKind::Beam
                            || matches!(
                                super::resolve_force_regime(elem, model),
                                super::ResolvedRegime::Fiber
                            );
                        if !fiber {
                            return None;
                        }
                        crate::frame::fiber::FiberBeam::try_new(elem, model, basis, kind)
                            .and_then(|beam| beam.validate_initial_section_stiffness())
                            .err()
                            .map(|reason| {
                                format!("部材 ID {} のファイバー初期断面: {reason}", elem.id.0)
                            })
                    })
            }
            _ => None,
        };
        if let Some(msg) = issue {
            issues.push(msg);
        }
    }
    issues
}

/// 明示RC梁基準の入力診断。未指定なら `None`、不適合なら当該解析種別の理由を返す。
pub fn rc_beam_reference_input_issue(
    data: &ElementData,
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
    basis: super::StrengthBasis,
) -> Option<String> {
    if crate::frame::beam::is_elastic_wall_girder(model, data) {
        return None;
    }
    model.member_rc_beam_reference(data.id)?;
    super::springs::rc_reference_target_issue(data, model)
        .or_else(|| member_strength_issue(data, model))
        .or_else(|| {
            super::springs::rc_reference_issue(
                data,
                model,
                super::springs::resolve_member_hysteresis(data, model, kind),
                basis,
            )
        })
}

/// [`nonlinear_input_issues`] が不備を検出した場合に、解析を停止するための
/// エラーメッセージ（先頭 `MAX_LISTED` 件＋残件数）を返す。
pub fn ensure_nonlinear_input(model: &Model) -> Result<(), String> {
    ensure_nonlinear_input_for_kind(model, sepika_core::model::AnalysisKind::Incremental)
}

/// 公称値で解析種別を指定して理由付き入力診断を行う。材料強度基準には `ensure_nonlinear_input_with_basis` を使う。
pub fn ensure_nonlinear_input_for_kind(
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
) -> Result<(), String> {
    ensure_nonlinear_input_with_basis(model, kind, super::StrengthBasis::Nominal)
}

/// 当該解析種別と実使用強度基準に整合する理由付き入力診断。
pub fn ensure_nonlinear_input_with_basis(
    model: &Model,
    kind: sepika_core::model::AnalysisKind,
    basis: super::StrengthBasis,
) -> Result<(), String> {
    let issues = nonlinear_input_issues_with_basis(model, kind, basis);
    if issues.is_empty() {
        return Ok(());
    }
    let head: Vec<String> = issues.iter().take(MAX_LISTED).cloned().collect();
    let more = if issues.len() > MAX_LISTED {
        format!("\n他 {} 件", issues.len() - MAX_LISTED)
    } else {
        String::new()
    };
    Err(format!("{}{}", head.join("\n"), more))
}

/// 材料の区分が断面形状と矛盾する場合に、その内容を返す。
/// 検出するのは次の 2 つに絞る。
///
/// - 配筋を持つ断面（RC 矩形・RC 円形）に鋼材の材料が付いている。
/// - 線材の材料に鉄筋が割り当てられている。
fn category_mismatch_issue(
    data: &ElementData,
    sec: &sepika_core::model::Section,
    mat: &sepika_core::model::Material,
) -> Option<String> {
    use sepika_core::model::MaterialCategory;
    if mat.category == MaterialCategory::Rebar {
        return Some(format!(
            "部材 ID {} の材料「{}」は区分が鉄筋です。\
             線材の材料には鋼材またはコンクリートを割り当ててください。\
             RC 断面の配筋は断面タブで主筋の材質として設定します。",
            data.id.0, mat.name
        ));
    }
    let has_rebar = sec.shape.as_ref().is_some_and(|s| {
        matches!(
            s,
            SectionShape::RcBeamRect { .. }
                | SectionShape::RcColumnRect { .. }
                | SectionShape::RcColumnCircle { .. }
                | SectionShape::SrcBeamRect { .. }
                | SectionShape::SrcColumnRect { .. }
        )
    });
    if has_rebar && mat.category == MaterialCategory::Steel {
        return Some(format!(
            "部材 ID {} は配筋を持つ RC 断面ですが、材料「{}」の区分が鋼材です。\
             材料タブで区分をコンクリートに変更してください。\
             区分は構造種別の判定に用い、鋼材のまま検定すると耐力を過大評価します。",
            data.id.0, mat.name
        ));
    }
    None
}

/// 線材の終局耐力を算定できない設定不備があれば、その内容を返す。
///
/// 検出する不備:
/// - 材料が設定されていない。
/// - コンクリート系断面（RC/SRC/CFT）なのに材料の Fc が未設定または 0 以下。
/// - 断面形状未設定の部材で、材料に正の fy も正の Fc もない。
pub(crate) fn member_strength_issue(data: &ElementData, model: &Model) -> Option<String> {
    let elastic_wall_girder = crate::frame::beam::is_elastic_wall_girder(model, data);
    let Some(sec) = data.section.and_then(|sid| model.sections.get(sid.index())) else {
        return Some(format!(
            "部材 ID {} に断面が設定されていません。\
             部材タブで断面を割り当ててください。\
             非線形解析では断面諸元から部材の剛性・終局耐力を算定します。",
            data.id.0
        ));
    };
    let Some(mat) = model.element_material(data) else {
        return Some(format!(
            "部材 ID {} に材料が設定されていません。\
             材料タブで材料を割り当ててください。\
             非線形解析では材料強度から部材の終局耐力を算定します。",
            data.id.0
        ));
    };
    if let Some(msg) = category_mismatch_issue(data, sec, mat) {
        return Some(msg);
    }
    let shear_material = if sec.is_cft() {
        model.element_steel_material(data)
    } else {
        Some(mat)
    };
    if let Some(material) =
        shear_material.filter(|material| material.young.is_finite() && material.young > 0.0)
    {
        let g = material.shear_modulus();
        if !g.is_finite() || g <= 0.0 {
            if let Err(reason) = crate::frame::beam::BeamElement::try_new(data, model) {
                return Some(reason);
            }
        } else if let Some(reason) = elastic_stiffness_input_issue(data, model) {
            return Some(reason);
        }
    }
    let stiffness = match crate::frame::beam::stiffness_breakdown(model, data) {
        Ok(stiffness) => stiffness,
        Err(error) => {
            return Some(format!(
                "部材 ID {} の剛性を算定できません: {error}",
                data.id.0
            ));
        }
    };
    if let Some(shape) = sec.shape.as_ref() {
        if let Err(error) = shape.rounded_steel_properties() {
            return Some(format!(
                "部材 ID {} の断面材料領域を解決できません: {error}",
                data.id.0
            ));
        }
        if let SectionShape::RcBeamRect { b, d, rebar } = shape {
            if !b.is_finite() || *b <= 0.0 || !d.is_finite() || *d <= 0.0 {
                return Some(format!(
                    "部材 ID {} の梁幅 b・全せい D は正の有限値が必要です",
                    data.id.0
                ));
            }
            for (name, value) in [
                ("Ec", mat.young),
                (
                    "Es",
                    model
                        .element_rebar_material(data)
                        .map(|m| m.young)
                        .unwrap_or(sepika_core::section_shape::E_STEEL),
                ),
            ] {
                if !value.is_finite() || value <= 0.0 {
                    return Some(format!(
                        "部材 ID {} の {name} は正の有限値が必要です",
                        data.id.0
                    ));
                }
            }
            if !elastic_wall_girder {
                for top_tension in [false, true] {
                    let steel = rebar.bending_steel(*d, top_tension).tension;
                    if let Err(error) = sepika_core::rc_capacity::rc_rebar_ratios(
                        *b,
                        *d,
                        steel.effective_depth_mm,
                        steel.area_mm2,
                        sepika_core::rc_capacity::RcAlphaSection::Rectangular,
                    ) {
                        return Some(format!(
                            "部材 ID {} の{}引張時の鉄筋比入力が不正です: {error}",
                            data.id.0,
                            if top_tension { "上端" } else { "下端" }
                        ));
                    }
                }
            }
            if super::hinge_view::resolves_to_concentrated_spring(data, model)
                && stiffness.slab > 1.0 + 1e-12
            {
                return Some(format!("部材 ID {} はスラブ協力付きRC梁ですが、集中ばねの方向別T形鉄筋比に必要なスラブ引張筋面積と正負別骨格を設定できません。矩形梁への代用は行わず解析を停止します", data.id.0));
            }
        }

        if let Err(err) = shape.validate_rebar() {
            return Some(format!(
                "部材 ID {} の断面「{}」は実配筋の幾何が不整合です（{}）。\
                 断面タブで段別本数・かぶり・断面寸法を見直してください。\
                 非線形解析では実配筋から曲げ降伏・せん断終局の各耐力を算定するため解析を開始できません。",
                data.id.0,
                sec.name.as_str(),
                err
            ));
        }
    }
    let is_concrete = sec.shape.as_ref().is_some_and(|s| s.is_concrete_like());
    if is_concrete {
        match mat.fc {
            None => {
                return Some(format!(
                    "部材 ID {} はコンクリート系断面ですが、材料「{}」にコンクリート強度 Fc が\
                     設定されていません。材料タブで Fc を設定してください。\
                     非線形解析では Fc から曲げひび割れ・曲げ降伏・せん断終局の各耐力を算定します。",
                    data.id.0, mat.name
                ));
            }
            Some(fc) if fc <= 0.0 => {
                return Some(format!(
                    "部材 ID {} の材料「{}」のコンクリート強度 Fc が {} で 0 以下です。\
                     材料タブで Fc を設定してください。\
                     非線形解析では Fc から曲げひび割れ・曲げ降伏・せん断終局の各耐力を算定します。",
                    data.id.0, mat.name, fc
                ));
            }
            Some(_) => {}
        }
        if elastic_wall_girder {
            return elastic_member_input_issue(data, model);
        }
        let has_rebar = sec.shape.as_ref().is_some_and(|s| {
            s.beam_rebar().is_some()
                || s.rect_column_rebar().is_some()
                || s.circle_column_rebar().is_some()
        });
        if has_rebar
            && sepika_core::material_grade::rebar_yield_strength(model.element_rebar_material(data))
                .is_none()
        {
            return Some(format!(
                "部材 ID {} の断面に主筋の材料が割り当てられていません。\
                 断面タブで主筋の材料（SD295A・SD345 等）を割り当ててください。\
                 非線形解析では主筋の降伏強度 σy から曲げ降伏耐力を算定します。",
                data.id.0
            ));
        }
        let fiber_shape = sec.shape.as_ref();
        let is_src = matches!(
            fiber_shape,
            Some(SectionShape::SrcBeamRect { .. } | SectionShape::SrcColumnRect { .. })
        );
        if is_src {
            let Some(steel_mat) = model.element_steel_material(data) else {
                return Some(format!(
                    "部材 ID {} の SRC 断面に内蔵鉄骨の材料が設定されていません。\
                     断面タブで内蔵鉄骨の材料を割り当ててください。\
                     ファイバー断面は内蔵鉄骨の材料からヤング係数 E と降伏強度を算定します.",
                    data.id.0
                ));
            };
            if !steel_mat.young.is_finite() || steel_mat.young <= 0.0 {
                return Some(format!(
                    "部材 ID {} の SRC 断面に割り当てた内蔵鉄骨材料「{}」のヤング係数 E が未設定または不正です。\
                     材料タブで正のヤング係数 E を設定してください。",
                    data.id.0, steel_mat.name
                ));
            }
        }
        if let Some(msg) = sepika_core::material_grade::shear_rebar_material_issue(
            model.element_shear_rebar_material(data),
        ) {
            return Some(format!("部材 ID {} の{}", data.id.0, msg));
        }
        if let (Some(shape), Some(steel_mat)) = (fiber_shape, model.element_steel_material(data)) {
            let thickness = match shape {
                SectionShape::SrcBeamRect {
                    steel_web_thick,
                    steel_flange_thick,
                    ..
                }
                | SectionShape::SrcColumnRect {
                    steel_web_thick,
                    steel_flange_thick,
                    ..
                } => Some(steel_web_thick.max(*steel_flange_thick)),
                SectionShape::CftBox {
                    thick,
                    horizontal_thick,
                    ..
                } => Some(thick.max(*horizontal_thick)),
                SectionShape::CftPipe { thick, .. } => Some(*thick),
                _ => None,
            };
            if let Some(reason) = thickness
                .and_then(|t| sepika_core::material_grade::steel_f_value_issue(&steel_mat.name, t))
            {
                return Some(format!("部材 ID {}: {reason}", data.id.0));
            }
        }
        if fiber_shape.is_some_and(shape_has_steel_fiber_region)
            && !crate::frame::fiber::resolve_steel_fiber_fy(
                fiber_shape,
                model.element_steel_material(data),
                mat.fy,
            )
            .is_some_and(|fy| fy > 0.0)
        {
            return Some(format!(
                "部材 ID {} の断面は鋼材領域（内蔵鉄骨・鋼管）を含みますが、鋼材の降伏強度を\
                 解決できません。断面へ内蔵鉄骨の材料を割り当てるか、材料「{}」の fy を\
                 設定してください。ファイバー断面は鋼材の降伏進展を追うため必須です。",
                data.id.0, mat.name
            ));
        }
        return None;
    }
    if elastic_wall_girder {
        return elastic_member_input_issue(data, model);
    }
    if sec.shape.as_ref().is_some() {
        if !mat.fy.is_some_and(|fy| fy > 0.0) {
            return Some(format!(
                "部材 ID {} は鋼材断面ですが、材料「{}」に降伏強度 fy が設定されていません。\
                 材料タブで fy を設定してください。\
                 ファイバー断面は鋼材の降伏進展を追うため fy が必須です。",
                data.id.0, mat.name
            ));
        }
        return None;
    }
    match mat.fc {
        Some(fc) if fc > 0.0 => None,
        Some(fc) => Some(format!(
            "部材 ID {} の材料「{}」のコンクリート強度 Fc が {} で 0 以下です。\
             材料タブで Fc を設定してください。\
             非線形解析では Fc から部材の終局耐力を算定します。",
            data.id.0, mat.name, fc
        )),
        None if mat.fy.is_some_and(|fy| fy > 0.0) => None,
        None => Some(format!(
            "部材 ID {} の材料「{}」に正の降伏強度 fy もコンクリート強度 Fc も設定されていません。\
             材料タブでいずれかを設定してください。\
             非線形解析では材料強度から部材の終局耐力を算定します。",
            data.id.0, mat.name
        )),
    }
}

fn elastic_member_input_issue(data: &ElementData, model: &Model) -> Option<String> {
    if let Some(reason) = elastic_stiffness_input_issue(data, model) {
        return Some(reason);
    }
    if let Err(reason) = crate::frame::beam::BeamElement::try_new(data, model) {
        return Some(format!(
            "部材 ID {} の弾性剛性入力が不正です: {reason}",
            data.id.0
        ));
    }
    model
        .element_mass_properties(data)
        .err()
        .map(|reason| format!("部材 ID {} の質量入力が不正です: {reason}", data.id.0))
}

#[cfg(test)]
mod tests;

/// 指定した有効荷重ケースだけで、RC逆対称基準と非三角部材荷重の矛盾を診断する。
/// ケース未適用の重力荷重は拒否根拠としない。実M/Qの確認を代行するAPIではない。
pub fn ensure_rc_beam_reference_loads(
    model: &Model,
    active_cases: &[sepika_core::ids::LoadCaseId],
) -> Result<(), String> {
    for case in model
        .load_cases
        .iter()
        .filter(|case| active_cases.contains(&case.id))
    {
        for load in &case.member {
            let Some(data) = model
                .elements
                .iter()
                .find(|element| element.id == load.elem)
            else {
                continue;
            };
            if model.member_rc_beam_reference(data.id).is_none()
                || !super::hinge_view::resolves_to_concentrated_spring(data, model)
            {
                continue;
            }
            let nonzero = match load.kind {
                sepika_core::model::MemberLoadKind::Point { p, .. } => p != 0.0,
                sepika_core::model::MemberLoadKind::Distributed { w1, w2, .. } => {
                    w1 != 0.0 || w2 != 0.0
                }
            };
            if nonzero {
                return Err(format!("部材 ID {} の有効荷重ケース「{}」に中間部材荷重があります。一定せん断・三角形曲げのRC逆対称基準へ自動接続できません", data.id.0, case.name));
            }
        }
    }
    Ok(())
}

/// 線材の E・採用 G、弾性梁の剛性・端部縮約が不正なら部材 ID 付きの理由を返す。
/// SRC/CFT の構築不備と他の入力不備は既存の断面・材料診断で扱う。
pub fn elastic_stiffness_input_issue(data: &ElementData, model: &Model) -> Option<String> {
    let section = model.element_section(data)?;
    let material = model.element_material(data)?;
    match crate::frame::beam::BeamElement::resolve_properties(data, model) {
        Ok(beam) => elastic_young_issue(data, beam.e).or_else(|| {
            matches!(
                data.kind,
                ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
            )
            .then(|| beam.validate_stiffness().err())
            .flatten()
        }),
        Err(error) => {
            let composite = matches!(
                section.shape,
                Some(
                    SectionShape::SrcBeamRect { .. }
                        | SectionShape::SrcColumnRect { .. }
                        | SectionShape::CftBox { .. }
                        | SectionShape::CftPipe { .. }
                )
            );
            if composite {
                // 材料別・部材群別の既存診断を先取りしない。
                return None;
            }
            if let Some(reason) = elastic_young_issue(data, material.young) {
                return Some(reason);
            }
            let g = material.shear_modulus();
            (matches!(
                data.kind,
                ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
            ) && (!g.is_finite() || g <= 0.0))
                .then_some(error)
        }
    }
}

fn elastic_young_issue(data: &ElementData, young_n_mm2: f64) -> Option<String> {
    if !young_n_mm2.is_finite() || young_n_mm2 <= 0.0 {
        Some(format!(
            "部材 ID {} の弾性梁のヤング係数 E は正の有限値が必要です",
            data.id.0
        ))
    } else {
        None
    }
}
