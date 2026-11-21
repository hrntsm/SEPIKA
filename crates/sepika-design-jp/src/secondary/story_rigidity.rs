use sepika_core::dof::{structural_nodes, Dof, Dof6Mask, DOF_PER_NODE};
use sepika_core::ids::{NodeId, StoryId};
use sepika_core::model::{Constraint, ElementData, ElementKind, EndCondition, Model, Node};
use sepika_element::behavior::{Ctx, ElementBehavior};
use sepika_element::frame::beam::BeamElement;
use sepika_element::springs::spring::NodalSpringElement;
use sepika_element::transform::LocalFrame;
use sepika_element::wall::side_column::wall_side_column_release;
use sepika_solver::statics::analysis::Analysis;
use sepika_solver::statics::linear::{StaticOnce, StoryRigidityEvaluation};

pub use sepika_solver::statics::linear::RigidFloorStoryRigidity;

use super::eccentricity::{diagonal_misc_wall, has_misc_wall};
use super::eccentricity_analysis::validate_rigidity_column_axes;
use super::story_columns::story_layer;

const FLOOR_DOFS: [Dof; 3] = [Dof::Ux, Dof::Uy, Dof::Rz];
pub(super) const ROUND_OFF: f64 = 512.0 * f64::EPSILON;

/// 対角項から正規化した方向間連成が、丸め誤差の判定範囲を超える場合は理由を返す。
pub fn coupling_reason(story: StoryId, rigidity: RigidFloorStoryRigidity) -> Option<String> {
    let [kx, kxy, ky] = rigidity.translational_stiffness_n_per_mm;
    ((kxy / kx.sqrt() / ky.sqrt()).abs() > ROUND_OFF).then(|| {
        format!(
            "階{}の剛床にX/Y方向間連成があります（Kxy = {kxy:.6e} N/mm）",
            story.0
        )
    })
}

fn evaluation_error(story: StoryId, reason: String) -> String {
    format!(
        "階{}の剛床の剛心・ねじり剛性を算定できません: {reason}",
        story.0
    )
}

fn rigidity_from_analysis(
    model: &Model,
    master: NodeId,
    analysis: Option<&Analysis<'_>>,
) -> Result<RigidFloorStoryRigidity, String> {
    let prepared;
    let analysis = match analysis {
        Some(analysis) => analysis,
        None => {
            prepared = Analysis::prepare(model).map_err(|error| error.to_string())?;
            &prepared
        }
    };
    let flexibility = analysis
        .nodal_planar_flexibility(master)
        .map_err(|error| error.to_string())?;
    rigidity_from_flexibility(model.nodes[master.index()].coord, flexibility)
}

fn with_wall_elements(model: &Model) -> std::borrow::Cow<'_, Model> {
    if model.wall_plates.is_empty() || !model.generated_wall_origins.is_empty() {
        std::borrow::Cow::Borrowed(model)
    } else {
        std::borrow::Cow::Owned(sepika_core::wall_expand::expand_wall_elements(model).0)
    }
}

/// 節点の配列位置と、解析部材・節点拘束からの参照を検証する。
pub fn validate_rigidity_node_ids(model: &Model) -> Result<(), String> {
    if let Some((index, node)) = model
        .nodes
        .iter()
        .enumerate()
        .find(|(index, node)| *index != node.id.index())
    {
        return Err(format!(
            "節点ID {} が節点配列の位置{index}と一致しません",
            node.id.0
        ));
    }
    let missing = |id: NodeId| id.index() >= model.nodes.len();
    for element in &model.elements {
        if let Some(node) = element.nodes.iter().copied().find(|&id| missing(id)) {
            return Err(format!(
                "部材{}が参照する節点ID {}が存在しません",
                element.id.0, node.0
            ));
        }
    }
    for (index, constraint) in model.constraints.iter().enumerate() {
        let node = match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => std::iter::once(*master)
                .chain(slaves.iter().copied())
                .find(|&id| missing(id)),
            Constraint::Mpc { master, terms } => std::iter::once(*master)
                .chain(terms.iter().map(|(id, _, _)| *id))
                .find(|&id| missing(id)),
        };
        if let Some(node) = node {
            return Err(format!(
                "節点拘束{}が参照する節点ID {}が存在しません",
                index + 1,
                node.0
            ));
        }
    }
    Ok(())
}

fn validate_rigidity_inputs(model: &Model, story: StoryId) -> Result<(), String> {
    validate_rigidity_node_ids(model).map_err(|reason| evaluation_error(story, reason))?;
    model
        .validate_wall_attr_refs()
        .map_err(|error| evaluation_error(story, error.to_string()))
}

/// 独立な X・Y・Mz 荷重から剛心と層剛性を返す。
/// 単層・上端単一剛床・基部の面内固定を確認できる線形弾性モデルに適用する。
pub fn rigid_floor_story_rigidity(
    model: &Model,
    story: StoryId,
) -> Result<RigidFloorStoryRigidity, String> {
    validate_rigidity_inputs(model, story)?;
    let model = with_wall_elements(model);
    evaluation_master(&model, story)
        .and_then(|master| rigidity_from_analysis(&model, master, None))
        .map_err(|reason| evaluation_error(story, reason))
}

fn evaluation_master(model: &Model, story: StoryId) -> Result<NodeId, String> {
    let layers = model.layers();
    let [layer] = layers.as_slice() else {
        return Err("独立な層応答の同定は全1層の場合に限ります".into());
    };
    if layer.top != story {
        return Err("指定した上端階に対応する層がありません".into());
    }
    let mut upper_floors = model.diaphragms_of(story);
    let Some(floor) = upper_floors.next() else {
        return Err("層上端に単一剛床がありません".into());
    };
    if upper_floors.next().is_some() {
        return Err("層上端に剛床が複数あります。単一剛床が必要です".into());
    }
    let floor_fixities =
        |node: &sepika_core::model::Node| FLOOR_DOFS.map(|dof| node.restraint.is_fixed(dof));
    for constraint in &model.constraints {
        match constraint {
            Constraint::RigidDiaphragm { story, .. } if *story == layer.top => {}
            Constraint::RigidDiaphragm {
                story,
                master,
                slaves,
                ..
            } if *story == layer.bottom => {
                for id in std::iter::once(master).chain(slaves) {
                    if !model.node(*id).is_some_and(|node| {
                        model.on_diaphragm_level(layer.bottom, node.coord[2])
                            && floor_fixities(node) == [true; 3]
                    }) {
                        return Err(format!(
                            "層下端剛床の節点{}の面内移動・回転が固定されていません",
                            id.0
                        ));
                    }
                }
            }
            _ => return Err("層上端の単一剛床と固定された層下端以外に節点拘束があります".into()),
        }
    }
    if model.stress_cfg.misc_wall_n.is_some() && has_misc_wall(model, story)? {
        return Err("n倍法の自立壁の付加剛性を剛床応答に組み込めません".into());
    }
    for element in &model.elements {
        match element.kind {
            ElementKind::NodalSpring if no_spring_stiffness(element) => {}
            ElementKind::NodalSpring | ElementKind::Damper => {
                return Err(format!(
                    "部材{}の付加ばね・ダンパーは対象外です",
                    element.id.0
                ));
            }
            ElementKind::Brace { tension_only: true }
                if model.stress_cfg.tension_only_iteration =>
            {
                return Err(format!(
                    "部材{}の引張専用ブレースの活動状態が加力によって変わります",
                    element.id.0
                ));
            }
            _ => {}
        }
    }
    let is_floor_node = |id| id == floor.master || floor.slaves.contains(&id);
    let structural = structural_nodes(model);
    for node in &model.nodes {
        if !structural[node.id.index()] && !is_floor_node(node.id) {
            continue;
        }
        if node.coord.iter().any(|coordinate| !coordinate.is_finite()) {
            return Err(format!("節点{}の位置が不正です", node.id.0));
        }
        if node.support_spring.is_some_and(|spring| {
            [Dof::Ux, Dof::Uy, Dof::Uz, Dof::Rx, Dof::Ry, Dof::Rz]
                .into_iter()
                .any(|dof| !node.restraint.is_fixed(dof) && spring[dof as usize] != 0.0)
        }) {
            return Err(format!("節点{}に活動する支持ばねがあります", node.id.0));
        }
        let z_mm = node.coord[2];
        let fixities = floor_fixities(node);
        let bottom = model.on_diaphragm_level(layer.bottom, z_mm);
        let top = model.on_diaphragm_level(layer.top, z_mm);
        if bottom && top {
            return Err(format!(
                "節点{}が上下のSLの剛床判定範囲に重なります",
                node.id.0
            ));
        }
        if is_floor_node(node.id) && !top {
            return Err(format!("節点{}が上端SLの剛床判定範囲外です", node.id.0));
        }
        if bottom {
            if fixities != [true; 3] {
                return Err(format!(
                    "層下端の節点{}の面内移動・回転が固定されていません",
                    node.id.0
                ));
            }
        } else if top {
            if !is_floor_node(node.id) || fixities != [false; 3] {
                return Err(format!(
                    "層上端の節点{}が剛床に従う3自由度を持ちません",
                    node.id.0
                ));
            }
        } else if z_mm < layer.bottom_sl_mm || z_mm > layer.top_sl_mm || node.restraint.0 != 0 {
            return Err(format!("節点{}の高さ又は支持条件が対象外です", node.id.0));
        }
    }
    Ok(floor.master)
}

fn rigidity_from_flexibility(
    origin_mm: [f64; 3],
    c: [[f64; 3]; 3],
) -> Result<RigidFloorStoryRigidity, String> {
    if !(0..3).all(|i| c[i][i].is_finite() && c[i][i] > 0.0) {
        return Err("3自由度の柔性行列の対角項が正の有限値ではありません".into());
    }
    for (i, j) in [(0, 1), (0, 2), (1, 2)] {
        let scale_i = c[i][i].sqrt();
        let scale_j = c[j][j].sqrt();
        let reciprocal_difference = (c[i][j] - c[j][i]) / scale_i / scale_j;
        if !reciprocal_difference.is_finite() || reciprocal_difference.abs() > ROUND_OFF {
            return Err("3自由度の柔性応答が相反性を満たしません".into());
        }
    }
    let rz = c[2][2];
    let [ux_mm, uy_mm] = [c[0][2], c[1][2]];
    let center_xy_mm = [origin_mm[0] - uy_mm / rz, origin_mm[1] + ux_mm / rz];
    let torsional_stiffness_nmm = rz.recip();
    let cxx = c[0][0] - ux_mm * (ux_mm / rz);
    let cyy = c[1][1] - uy_mm * (uy_mm / rz);
    let cxy = (0.5 * c[0][1] + 0.5 * c[1][0]) - ux_mm * (uy_mm / rz);
    let rho = cxy / cxx.sqrt() / cyy.sqrt();
    let determinant = 1.0 - rho * rho;
    if !(cxx > c[0][0] * ROUND_OFF && cyy > c[1][1] * ROUND_OFF && determinant > ROUND_OFF) {
        return Err("回転を除いた並進柔性を正定値として同定できません".into());
    }
    let kx = (cxx * determinant).recip();
    let ky = (cyy * determinant).recip();
    let kxy = -rho * kx.sqrt() * ky.sqrt();
    if center_xy_mm
        .into_iter()
        .chain([torsional_stiffness_nmm, kx, kxy, ky])
        .any(|x| !x.is_finite())
    {
        return Err("剛心の座標又は層剛性が非有限です".into());
    }
    Ok(RigidFloorStoryRigidity {
        center_xy_mm,
        torsional_stiffness_nmm,
        translational_stiffness_n_per_mm: [kx, kxy, ky],
    })
}

/// 従来の方向別集計で扱えない層の剛性を剛床応答から同定する。適用外なら階と理由を返す。
/// `story` は層の上端階。方向別集計を使う層では `Ok(None)`。
pub fn story_rigidity_requiring_coupled_analysis(
    model: &Model,
    story: StoryId,
    responses: [Option<&StaticOnce>; 2],
) -> Result<Option<RigidFloorStoryRigidity>, String> {
    if story_layer(model, story).is_none() {
        return Err(evaluation_error(
            story,
            "指定した上端階に対応する層がありません".into(),
        ));
    }
    validate_rigidity_inputs(model, story)?;
    if let Some((_, evaluation)) = recorded_story_rigidity(story, responses)
        .map_err(|reason| evaluation_error(story, reason))?
    {
        return evaluation.clone();
    }
    let expanded = with_wall_elements(model);
    evaluate_story_rigidity(&expanded, story, None).or_else(|reason| {
        validate_rigidity_column_axes(model, story)?;
        Err(reason)
    })
}

fn recorded_story_rigidity(
    story: StoryId,
    responses: [Option<&StaticOnce>; 2],
) -> Result<Option<&StoryRigidityEvaluation>, String> {
    let mut recorded = [None; 2];
    for (axis, response) in responses.into_iter().enumerate() {
        let Some(response) = response else { continue };
        let direction = ["X", "Y"][axis];
        let mut entries = response
            .story_rigidity
            .iter()
            .filter(|(id, _)| *id == story);
        let Some(entry @ (_, evaluation)) = entries.next() else {
            continue;
        };
        if entries.next().is_some() {
            return Err(format!("{direction}方向の層剛性記録が重複しています"));
        }
        if let Ok(Some(rigidity)) = evaluation {
            let [kx, kxy, ky] = rigidity.translational_stiffness_n_per_mm;
            let kr = rigidity.torsional_stiffness_nmm;
            let finite = rigidity
                .center_xy_mm
                .into_iter()
                .chain([kx, kxy, ky, kr])
                .all(f64::is_finite);
            let positive =
                kx > 0.0 && ky > 0.0 && kr > 0.0 && (kxy / kx.sqrt() / ky.sqrt()).abs() < 1.0;
            if !(finite && positive) {
                return Err(format!(
                    "{direction}方向の層剛性記録に非有限の剛心・剛性又は正定値でない層剛性があります"
                ));
            }
        }
        recorded[axis] = Some(entry);
    }
    let [x, y] = recorded;
    if responses.iter().all(Option::is_some) && x != y {
        return Err("X/Y方向の層剛性記録が一致しません".into());
    }
    Ok(x.or(y))
}

/// 地震静的と同じ壁展開済みモデル・剛性因子で層を評価し、層順に返す。
pub fn evaluate_story_rigidities(
    model: &Model,
    analysis: &Analysis<'_>,
) -> Vec<StoryRigidityEvaluation> {
    model
        .layers()
        .iter()
        .map(|layer| {
            (
                layer.top,
                evaluate_story_rigidity(model, layer.top, Some(analysis)),
            )
        })
        .collect()
}

fn evaluate_story_rigidity(
    model: &Model,
    story: StoryId,
    analysis: Option<&Analysis<'_>>,
) -> Result<Option<RigidFloorStoryRigidity>, String> {
    let indicator = coupling_indicator_from_expanded(model, story)?;
    let master = match evaluation_master(model, story) {
        Ok(master) => master,
        Err(reason) => {
            return match indicator {
                CouplingIndicator::Candidate(cause) => {
                    Err(evaluation_error(story, format!("{cause}。{reason}")))
                }
                _ => Ok(None),
            };
        }
    };
    let rigidity = rigidity_from_analysis(model, master, analysis)
        .map_err(|reason| evaluation_error(story, reason))?;
    Ok((!matches!(indicator, CouplingIndicator::None)
        || coupling_reason(story, rigidity).is_some())
    .then_some(rigidity))
}

enum CouplingIndicator {
    Candidate(String),
    InclinedMember,
    None,
}

pub(super) fn coupling_indicator(model: &Model, story: StoryId) -> Result<Option<String>, String> {
    validate_rigidity_inputs(model, story)?;
    let model = with_wall_elements(model);
    match coupling_indicator_from_expanded(&model, story)? {
        CouplingIndicator::Candidate(reason) => Ok(Some(reason)),
        _ => Ok(None),
    }
}

fn coupling_indicator_from_expanded(
    model: &Model,
    story: StoryId,
) -> Result<CouplingIndicator, String> {
    validate_rigidity_node_ids(model).map_err(|reason| evaluation_error(story, reason))?;
    if model.stress_cfg.misc_wall_n.is_some() {
        if let Some(wall) =
            diagonal_misc_wall(model, story).map_err(|reason| evaluation_error(story, reason))?
        {
            return Ok(CouplingIndicator::Candidate(format!(
                "付加剛性を持つ自立壁{}がX/Y両方向に傾いています",
                wall.0
            )));
        }
    }
    let connected = std::cell::OnceCell::new();
    let component = |nodes: &[NodeId]| match connected.get_or_init(|| {
        connected_components_in_layer(model, story, |node| node.restraint != Dof6Mask::FIXED, true)
    }) {
        Some(components) => nodes
            .iter()
            .find_map(|id| components.get(id.index()).copied().flatten()),
        None => Some(0),
    };
    let affects_layer = |nodes: &[NodeId]| component(nodes).is_some();
    if let Some((index, _)) = model
        .constraints
        .iter()
        .enumerate()
        .find(|(_, constraint)| match constraint {
            Constraint::RigidDiaphragm { .. } => false,
            Constraint::RigidLink { master, slaves, .. } => {
                affects_layer(&[*master]) || affects_layer(slaves)
            }
            Constraint::Mpc { master, terms } => {
                affects_layer(&[*master])
                    || terms.iter().any(|(node, _, _)| affects_layer(&[*node]))
            }
        })
    {
        return Ok(CouplingIndicator::Candidate(format!(
            "剛床以外の節点拘束（順序{}）があります",
            index + 1
        )));
    }
    let structural = structural_nodes(model);
    if let Some(node) = model.nodes.iter().find(|node| {
        structural[node.id.index()]
            && node.support_spring.is_some_and(|spring| {
                FLOOR_DOFS
                    .into_iter()
                    .any(|dof| !node.restraint.is_fixed(dof) && spring[dof as usize] != 0.0)
            })
            && affects_layer(&[node.id])
    }) {
        return Ok(CouplingIndicator::Candidate(format!(
            "節点{}に活動する面内支持ばねがあります",
            node.id.0
        )));
    }
    let significant = |x: f64| x.abs() > ROUND_OFF;
    let mut sloped: [Vec<&ElementData>; 2] = Default::default();
    let mut horizontal: [Vec<&ElementData>; 2] = Default::default();
    let mut planes: [Vec<PlaneMember<'_>>; 2] = Default::default();
    for element in &model.elements {
        let kind = element.kind;
        if kind == ElementKind::PanelZone
            || element.nodes.iter().all(|id| {
                model
                    .node(*id)
                    .is_some_and(|node| node.restraint == Dof6Mask::FIXED)
            })
        {
            continue;
        }
        if matches!(kind, ElementKind::NodalSpring | ElementKind::Damper) {
            if affects_layer(&element.nodes) {
                if kind == ElementKind::NodalSpring {
                    let independent = independent_horizontal_spring(model, element)
                        .map_err(|reason| evaluation_error(story, reason))?;
                    if independent {
                        continue;
                    }
                }
                return Ok(CouplingIndicator::Candidate(format!(
                    "接続する架構に部材{}の付加ばね・ダンパーがあります",
                    element.id.0
                )));
            }
            continue;
        }
        let points = element
            .nodes
            .iter()
            .filter_map(|id| model.node(*id).map(|node| node.coord));
        if matches!(kind, ElementKind::Wall | ElementKind::Shell) {
            let mut points = points;
            if let Some(origin) = points.next() {
                let (mut spans_x, mut spans_y, mut spans_z) = (false, false, false);
                for point in points {
                    spans_x |= point[0] != origin[0];
                    spans_y |= point[1] != origin[1];
                    spans_z |= point[2] != origin[2];
                }
                if spans_x && spans_y && affects_layer(&element.nodes) {
                    return Ok(CouplingIndicator::Candidate(format!(
                        "面材{}がX/Y両方向にまたがる構面にあります",
                        element.id.0
                    )));
                }
                for (axis, span) in [spans_x, spans_y].into_iter().enumerate() {
                    if span {
                        horizontal[axis].push(element);
                        if spans_z {
                            planes[axis].push(PlaneMember {
                                element,
                                axial_vertical: false,
                            });
                        }
                    }
                }
            }
            continue;
        }
        let points: Vec<_> = points.collect();
        let [start, end] = points.as_slice() else {
            continue;
        };
        let Ok(ends) = model.primary_frame_axis_end_positions(element) else {
            continue;
        };
        let [start, end] = ends.unwrap_or([*start, *end]);
        let frame = LocalFrame::from_nodes(start, end, element.local_axis.ref_vector);
        let Ok(frame) = frame else {
            continue;
        };
        let [axis, y, z] = frame.rot;
        let [x_span, y_span, z_span] = axis.map(significant);
        let [offset_x, offset_y] = [0, 1].map(|axis| {
            element
                .end_offsets_mm
                .iter()
                .any(|offset| offset[axis] != 0.0)
        });
        if x_span && y_span && affects_layer(&element.nodes) {
            return Ok(CouplingIndicator::Candidate(format!(
                "部材{}の実材軸がX/Y両方向に傾いています",
                element.id.0
            )));
        }
        for (axis, (span, offset)) in [x_span, y_span]
            .into_iter()
            .zip([offset_x, offset_y])
            .enumerate()
        {
            if span {
                horizontal[axis].push(element);
                if z_span {
                    sloped[axis].push(element);
                }
            }
            let axial_vertical = z_span && span;
            if axial_vertical
                || (z_span && offset)
                || (!z_span && span && kind.is_primary_frame_support())
            {
                planes[axis].push(PlaneMember {
                    element,
                    axial_vertical,
                });
            }
        }
        if (x_span || offset_x) && (y_span || offset_y) && affects_layer(&element.nodes) {
            return Ok(CouplingIndicator::Candidate(format!(
                "部材{}の材軸・端部偏心がX/Y両方向にまたがります",
                element.id.0
            )));
        }
        if matches!(kind, ElementKind::Brace { .. }) {
            continue;
        }
        let mixed_axis = (0..2).any(|i| significant(y[i]) && significant(z[i]));
        if mixed_axis && affects_layer(&element.nodes) {
            let beam = BeamElement::try_new(element, model).map_err(|reason| {
                evaluation_error(
                    story,
                    format!(
                        "部材{}の実効断面剛性を確認できません。{reason}",
                        element.id.0
                    ),
                )
            })?;
            if beam.iy != beam.iz
                || beam.as_y != beam.as_z
                || wall_side_column_release(element, model).is_some()
            {
                return Ok(CouplingIndicator::Candidate(format!(
                    "部材{}の方向別断面剛性の軸が全体の直交軸と一致しません",
                    element.id.0
                )));
            }
        }
    }
    for (inclined, transverse) in sloped.iter().zip(horizontal.iter().rev()) {
        if inclined.is_empty() {
            continue;
        }
        let other_plane: std::collections::HashMap<_, _> = transverse
            .iter()
            .filter_map(|member| Some((component(&member.nodes)?, member.id)))
            .collect();
        for member in inclined {
            if let Some(other) = component(&member.nodes).and_then(|id| other_plane.get(&id)) {
                return Ok(CouplingIndicator::Candidate(format!(
                    "傾斜部材{}と直交側の構面材{}が変位を伝え得る同じ架構にあります",
                    member.id.0, other.0
                )));
            }
        }
    }
    if let Some(reason) = shared_plane_dof(model, story, planes, affects_layer) {
        return Ok(CouplingIndicator::Candidate(reason));
    }
    if sloped
        .into_iter()
        .flatten()
        .any(|member| affects_layer(&member.nodes))
    {
        return Ok(CouplingIndicator::InclinedMember);
    }
    Ok(CouplingIndicator::None)
}

fn no_spring_stiffness(element: &ElementData) -> bool {
    element.spring.is_none_or(|stiffness| stiffness == [0.0; 6])
}

fn independent_horizontal_spring(model: &Model, element: &ElementData) -> Result<bool, String> {
    if no_spring_stiffness(element) {
        return Ok(true);
    }
    let invalid = || format!("節点ばね{}の有効な方向別剛性を確認できません", element.id.0);
    if element.nodes.len() != 2
        || element
            .spring
            .is_some_and(|k| k.into_iter().any(|value| !value.is_finite() || value < 0.0))
    {
        return Err(invalid());
    }
    let nodes =
        [element.nodes[0], element.nodes[1]].map(|id| model.node(id).expect("節点参照を確認済み"));
    if nodes.iter().any(|node| {
        [Dof::Uz, Dof::Rx, Dof::Ry, Dof::Rz]
            .into_iter()
            .any(|dof| !node.restraint.is_fixed(dof))
    }) {
        return Ok(false);
    }
    model.validate_element_local_frame(element)?;
    let spring = NodalSpringElement::new(element, model);
    let stiffness = spring.tangent_stiffness(&Ctx { model });
    if stiffness.data.iter().any(|value| !value.is_finite()) {
        return Err(invalid());
    }
    for (i, x_node) in nodes.into_iter().enumerate() {
        for (j, y_node) in nodes.into_iter().enumerate() {
            if x_node.restraint.is_fixed(Dof::Ux) || y_node.restraint.is_fixed(Dof::Uy) {
                continue;
            }
            let ix = i * DOF_PER_NODE + Dof::Ux as usize;
            let iy = j * DOF_PER_NODE + Dof::Uy as usize;
            let kxy = stiffness.get(ix, iy);
            if kxy == 0.0 {
                continue;
            }
            let coupling = kxy / stiffness.get(ix, ix).sqrt() / stiffness.get(iy, iy).sqrt();
            if !coupling.is_finite() || coupling.abs() > ROUND_OFF {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[derive(Clone, Copy)]
struct PlaneMember<'a> {
    element: &'a ElementData,
    axial_vertical: bool,
}

fn shared_plane_dof(
    model: &Model,
    story: StoryId,
    planes: [Vec<PlaneMember<'_>>; 2],
    affects_layer: impl Fn(&[NodeId]) -> bool,
) -> Option<String> {
    if planes.iter().any(Vec::is_empty) {
        return None;
    }
    let mut floor_nodes = vec![false; model.nodes.len()];
    for constraint in &model.constraints {
        if let Constraint::RigidDiaphragm { master, slaves, .. } = constraint {
            for node in std::iter::once(master).chain(slaves) {
                if let Some(on_floor) = floor_nodes.get_mut(node.index()) {
                    *on_floor = true;
                }
            }
        }
    }
    for dof in [Dof::Uz, Dof::Rz] {
        let connects = |node: &Node| {
            !node.restraint.is_fixed(dof) && (dof != Dof::Rz || !floor_nodes[node.id.index()])
        };
        let components = connected_components_in_layer(model, story, connects, false)?;
        let attached = |plane: &[PlaneMember<'_>], bending: Dof| {
            let mut joints = std::collections::BTreeMap::new();
            for member in plane.iter().copied() {
                if !affects_layer(&member.element.nodes)
                    || !transmits_shared_displacement(model, member, dof, bending)
                {
                    continue;
                }
                for node in &member.element.nodes {
                    if let Some(id) = components.get(node.index()).copied().flatten() {
                        joints.entry(id).or_insert((member.element.id, *node));
                    }
                }
            }
            joints
        };
        let x = attached(&planes[0], Dof::Ry);
        let y = attached(&planes[1], Dof::Rx);
        if let Some(((x, x_node), (y, y_node))) = x
            .into_iter()
            .find_map(|(component, first)| Some((first, *y.get(&component)?)))
        {
            return Some(format!(
                "直交構面材{}・{}の節点{}・{}は自由な{dof:?}を介して変位を伝え得ます",
                x.0, y.0, x_node.0, y_node.0
            ));
        }
    }
    None
}

fn transmits_shared_displacement(
    model: &Model,
    member: PlaneMember<'_>,
    dof: Dof,
    bending: Dof,
) -> bool {
    if dof == Dof::Uz && member.axial_vertical {
        return true;
    }
    let member = member.element;
    if matches!(member.kind, ElementKind::Wall | ElementKind::Shell) {
        return dof == Dof::Uz;
    }
    if dof == Dof::Rz && matches!(member.kind, ElementKind::Brace { .. }) {
        return false;
    }
    let rotation = if dof == Dof::Uz { bending } else { Dof::Rz };
    member
        .nodes
        .iter()
        .zip(member.end_cond)
        .zip(member.end_offsets_mm)
        .any(|((&node, end), offset)| {
            if model
                .node(node)
                .is_none_or(|node| node.restraint.is_fixed(rotation))
            {
                return false;
            }
            if dof == Dof::Uz && offset[usize::from(bending == Dof::Rx)] != 0.0 {
                return true;
            }
            match end {
                EndCondition::Fixed => true,
                EndCondition::Pinned => false,
                EndCondition::SemiRigid { k_theta } => k_theta > 0.0,
            }
        })
}

fn connected_components_in_layer(
    model: &Model,
    story: StoryId,
    available: impl Fn(&Node) -> bool,
    join_constraints: bool,
) -> Option<Vec<Option<usize>>> {
    let layers = model.layers();
    let layer = layers.iter().find(|layer| layer.top == story)?;
    let free = |id: NodeId| {
        model
            .node(id)
            .filter(|node| available(node))
            .map(|node| node.id.index())
    };
    let mut adjacent = vec![Vec::new(); model.nodes.len()];
    let mut link = |nodes: &[NodeId]| {
        let mut free_nodes = nodes.iter().filter_map(|&id| free(id));
        if let Some(first) = free_nodes.next() {
            for next in free_nodes {
                adjacent[first].push(next);
                adjacent[next].push(first);
            }
        }
    };
    let mut seeds = Vec::new();
    for element in &model.elements {
        link(&element.nodes);
        let mut z = element
            .nodes
            .iter()
            .filter_map(|id| model.node(*id).map(|node| node.coord[2]));
        let Some(first) = z.next() else {
            continue;
        };
        let (min, max) = z.fold((first, first), |(min, max), z| (min.min(z), max.max(z)));
        if min <= layer.top_sl_mm && max > layer.bottom_sl_mm {
            seeds.extend(element.nodes.iter().filter_map(|&id| free(id)));
        }
    }
    for constraint in model.constraints.iter().filter(|_| join_constraints) {
        let nodes: Vec<_> = match constraint {
            Constraint::RigidDiaphragm { master, slaves, .. }
            | Constraint::RigidLink { master, slaves, .. } => std::iter::once(*master)
                .chain(slaves.iter().copied())
                .collect(),
            Constraint::Mpc { master, terms } => std::iter::once(*master)
                .chain(terms.iter().map(|(node, _, _)| *node))
                .collect(),
        };
        link(&nodes);
    }
    let mut components = vec![None; model.nodes.len()];
    let mut pending = Vec::new();
    for seed in seeds {
        pending.push(seed);
        while let Some(node) = pending.pop() {
            if components[node].is_none() {
                components[node] = Some(seed);
                pending.extend(&adjacent[node]);
            }
        }
    }
    Some(components)
}
