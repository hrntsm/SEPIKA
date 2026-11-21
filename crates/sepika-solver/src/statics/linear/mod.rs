use crate::common::assemble::{add_support_spring_diag, support_spring_terms};
use crate::common::constraint::Reducer;
use crate::common::csc_cache::CscCache;
use crate::common::linear_load::{
    assemble_linear_rhs, element_error, legacy_member_force, loaded_response, validate_entries,
    PhysicalLoadLedger,
};
use sepika_core::adjacency::NodeAdjacency;
use sepika_core::dof::DofMap;
use sepika_core::face_distance::{girder_column_faces, GirderColumnFace};
use sepika_core::ids::{ElemId, LoadCaseId, StoryId};
use sepika_core::model::{
    ElementData, ElementKind, FrameSectionUse, LoadCase, LoadCaseKind, MemberLoad, Model,
};
use sepika_element::behavior::{Ctx, ElementBehavior};
use sepika_element::factory::build_behavior_with_axial_factor;
use sepika_element::frame::beam::MemberForces;
use sepika_element::frame::member_load::LoadedFrameSection;
use sepika_math::solver::{make_solver, LinearSolver, SolveError, SolverBackend};
use sepika_math::sparse::{assemble_csc, Triplet};
use std::collections::HashMap;

/// 長期軸力無効化（一貫構造計算プログラムの実務慣行）で断面積に乗じる縮小係数。
const AXIAL_DISABLE_FACTOR: f64 = 1.0e-6;

/// 部材が柱用途かどうかを断面用途で判定する。
fn is_column(elem: &ElementData, model: &Model) -> bool {
    matches!(
        elem.kind,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
    ) && elem
        .section
        .and_then(|id| model.sections.get(id.index()))
        .and_then(|section| section.frame_use)
        == Some(FrameSectionUse::Column)
}

/// 長期応力解析で軸力を負担させない部材（対象: ブレース／柱）かどうかを、
/// `Model::stress_cfg` の指定に基づいて判定する。
fn is_axial_disabled_target(
    elem: &ElementData,
    model: &Model,
    cfg: &sepika_core::model::StressAnalysisCfg,
) -> bool {
    match elem.kind {
        ElementKind::Brace { .. } => cfg.no_long_axial_brace,
        ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring => {
            cfg.no_long_axial_column && is_column(elem, model)
        }
        _ => false,
    }
}

pub(crate) fn uses_long_axial_stiffness(model: &Model, kind: LoadCaseKind) -> bool {
    kind.is_long_term()
        && model
            .elements
            .iter()
            .any(|elem| is_axial_disabled_target(elem, model, &model.stress_cfg))
}

fn long_axial_factor(model: &Model, elem: &ElementData, kind: LoadCaseKind) -> f64 {
    if kind.is_long_term() && is_axial_disabled_target(elem, model, &model.stress_cfg) {
        AXIAL_DISABLE_FACTOR
    } else {
        1.0
    }
}

pub(crate) struct StaticStiffness {
    pub(crate) solver: Box<dyn LinearSolver>,
    pub(crate) behaviors: Vec<crate::statics::BehaviorEntry>,
}

impl StaticStiffness {
    pub(crate) fn build(
        model: &Model,
        dofmap: &DofMap,
        reducer: &Reducer,
        kind: LoadCaseKind,
    ) -> Result<Self, SolveError> {
        let mut behaviors = Vec::with_capacity(model.elements.len());
        let mut triplets = Vec::new();
        for elem in &model.elements {
            let behavior =
                build_behavior_with_axial_factor(elem, model, long_axial_factor(model, elem, kind));
            let gdofs = behavior.global_dofs(dofmap);
            triplets.extend(
                behavior
                    .tangent_stiffness(&Ctx { model })
                    .to_triplets(&gdofs),
            );
            behaviors.push((elem.id, behavior, gdofs));
        }
        add_support_spring_diag(model, dofmap, &mut triplets);
        let k_free = assemble_csc(dofmap.n_active(), triplets);
        let mut solver = make_solver(SolverBackend::DirectSparseCholesky);
        if reducer.n_indep > 0 {
            solver
                .factorize(&reducer.reduce_k(&k_free))
                .map_err(|error| match error {
                    SolveError::NotPositiveDefinite => SolveError::InvalidInput(
                        crate::statics::analysis::precheck::singular_diagnosis(model),
                    ),
                    other => other,
                })?;
        }
        Ok(Self { solver, behaviors })
    }
}

/// 線形弾性の剛床応答から求めた層剛性。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RigidFloorStoryRigidity {
    /// 純ねじり時の面内変位がゼロとなる位置 [mm]。
    pub center_xy_mm: [f64; 2],
    /// 純ねじり荷重を回転角で除したねじり剛性 [N·mm/rad]。
    pub torsional_stiffness_nmm: f64,
    /// 剛心での並進剛性行列の成分 [Kxx, Kxy, Kyy] [N/mm]。
    pub translational_stiffness_n_per_mm: [f64; 3],
}

pub type StoryRigidityEvaluation = (StoryId, Result<Option<RigidFloorStoryRigidity>, String>);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StaticOnce {
    /// 性能曲線と同じ物理節点力・支持反力による全層せん断 `[N]`。要素種別によらず X/Y 順。
    pub story_shears: Option<[Vec<f64>; 2]>,
    /// 自立壁の付加剛性を反映したRs評価専用の節点変位 [mm, rad]。Errは評価不能理由。
    /// Noneは評価未実施であり、通常変位で補完しない。
    #[serde(default)]
    pub stiffness_ratio_disp: Option<Result<Vec<[f64; 6]>, String>>,
    /// 地震静的と同じ弾性系の層剛性。同定対象外の従来評価は Ok(None)、保留は Err。
    #[serde(default)]
    pub story_rigidity: Vec<StoryRigidityEvaluation>,
    pub disp: Vec<[f64; 6]>,
    pub member_forces: Vec<(sepika_core::ids::ElemId, MemberForces)>,
    /// 通常柱のi/j各端直内側で、端側から部材内部に作用する全体水平力 [N]（X/Y順）。
    /// 対象の応答がない場合は空。算定不能な柱は理由付きエラーを保持する。
    #[serde(default)]
    pub column_end_horizontal_forces: Vec<(ElemId, ColumnEndHorizontalForces)>,
    /// 柱のi/j各端直内側の材軸力 [N]（圧縮が正）。算定不能な柱は理由付きエラーを保持する。
    #[serde(default)]
    pub column_end_axial_forces: Vec<(ElemId, ColumnEndAxialForces)>,
    /// 全体 X・Y 軸まわりの仕口パネルせん断モーメント `[MSX, MSY]` [N·mm] を節点ごとに保持する。
    /// パネルをモデル化していない場合は空。
    #[serde(default)]
    pub panel_moments: Vec<(sepika_core::ids::NodeId, [f64; 2])>,
    /// 大梁の実柱面の断面力。大梁IDごとにi/j順。幾何を決められない端は理由付きエラー。
    /// 対象の大梁がない場合とケース応答を持たない空の組合せでは空。
    #[serde(default)]
    pub girder_column_face_forces: Vec<(ElemId, GirderColumnFaceEnds)>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GirderColumnFaceForces {
    pub face: GirderColumnFace,
    /// 梁局所系の `[N, Qy, Qz, Mx, My, Mz]` `[N, N·mm]`。算定不能時は理由付きエラー。
    pub forces: Result<[f64; 6], String>,
}

pub type ColumnEndHorizontalForces = Result<[[f64; 2]; 2], String>;
pub type ColumnEndAxialForces = Result<[f64; 2], String>;

pub type GirderColumnFaceEnds = [Result<GirderColumnFaceForces, String>; 2];

/// 同じモデルのケース応答を係数倍して足し合わせる。部材・評価断面の順序は一致していること。
/// 引張専用反復を含む応答和は、合成荷重の active-set 解を保証しない。項が空なら空の結果を返す。
/// 係数または合成結果が非有限の場合は入力エラーを返す。
pub fn superpose_static(terms: &[(&StaticOnce, f64)]) -> Result<StaticOnce, SolveError> {
    let Some((first, _)) = terms.first() else {
        return Ok(StaticOnce {
            story_shears: None,
            stiffness_ratio_disp: None,
            story_rigidity: Vec::new(),
            disp: Vec::new(),
            member_forces: Vec::new(),
            column_end_horizontal_forces: Vec::new(),
            column_end_axial_forces: Vec::new(),
            panel_moments: Vec::new(),
            girder_column_face_forces: Vec::new(),
        });
    };
    let mut disp = vec![[0.0; 6]; first.disp.len()];
    let mut member_forces: Vec<(ElemId, MemberForces)> = first
        .member_forces
        .iter()
        .map(|(id, mf)| {
            (
                *id,
                MemberForces {
                    at: mf.at.iter().map(|(xi, _)| (*xi, [0.0; 6])).collect(),
                },
            )
        })
        .collect();
    let mut panel_moments: Vec<(sepika_core::ids::NodeId, [f64; 2])> = first
        .panel_moments
        .iter()
        .map(|(node, _)| (*node, [0.0; 2]))
        .collect();
    for (res, factor) in terms {
        if !factor.is_finite() {
            return Err(SolveError::InvalidInput("応答の係数が非有限です".into()));
        }
        if *factor == 0.0 {
            continue;
        }
        for (dst, src) in disp.iter_mut().zip(res.disp.iter()) {
            for (d, s) in dst.iter_mut().zip(src.iter()) {
                *d += s * factor;
            }
        }
        for ((_, dst), (_, src)) in member_forces.iter_mut().zip(res.member_forces.iter()) {
            for ((_, d6), (_, s6)) in dst.at.iter_mut().zip(src.at.iter()) {
                for (d, s) in d6.iter_mut().zip(s6.iter()) {
                    *d += s * factor;
                }
            }
        }
        for ((_, dst), (_, src)) in panel_moments.iter_mut().zip(res.panel_moments.iter()) {
            for (d, s) in dst.iter_mut().zip(src.iter()) {
                *d += s * factor;
            }
        }
    }
    let result = StaticOnce {
        story_shears: terms.iter().try_fold(
            std::array::from_fn(|axis| {
                vec![0.0; first.story_shears.as_ref().map_or(0, |q| q[axis].len())]
            }),
            |mut sum, (result, factor)| {
                let q = result.story_shears.as_ref()?;
                for axis in 0..2 {
                    if sum[axis].len() != q[axis].len() {
                        return None;
                    }
                    if *factor != 0.0 {
                        for (out, value) in sum[axis].iter_mut().zip(&q[axis]) {
                            *out += factor * value;
                        }
                    }
                }
                Some(sum)
            },
        ),
        stiffness_ratio_disp: None,
        story_rigidity: Vec::new(),
        disp,
        member_forces,
        panel_moments,
        girder_column_face_forces: superpose_girder_column_faces(terms)?,
        column_end_horizontal_forces: superpose_column_end_forces(
            terms,
            |res| &res.column_end_horizontal_forces,
            |ends| ends.as_flattened_mut(),
            "水平力",
        ),
        column_end_axial_forces: superpose_column_end_forces(
            terms,
            |res| &res.column_end_axial_forces,
            |ends| ends,
            "軸力",
        ),
    };
    if let Some((id, _)) = result.member_forces.iter().find(|(_, forces)| {
        forces.at.iter().any(|(position, values)| {
            !position.is_finite() || values.iter().any(|v| !v.is_finite())
        })
    }) {
        return Err(SolveError::InvalidInput(format!(
            "部材 {} の合成内力または評価位置が非有限です",
            id.0
        )));
    }
    if let Some((node, _)) = result
        .panel_moments
        .iter()
        .find(|(_, moments)| moments.iter().any(|v| !v.is_finite()))
    {
        return Err(SolveError::InvalidInput(format!(
            "節点 {} の仕口パネルの合成内力が非有限です",
            node.0
        )));
    }
    if result.disp.iter().flatten().any(|v| !v.is_finite()) {
        return Err(SolveError::InvalidInput("合成変位が非有限です".into()));
    }
    if result
        .story_shears
        .iter()
        .flatten()
        .flatten()
        .any(|v| !v.is_finite())
    {
        return Err(SolveError::InvalidInput(
            "合成層せん断力が非有限です".into(),
        ));
    }
    Ok(result)
}

type ColumnEndResponses<T> = [(ElemId, Result<T, String>)];

fn superpose_column_end_forces<T: Copy + Default>(
    terms: &[(&StaticOnce, f64)],
    responses: fn(&StaticOnce) -> &ColumnEndResponses<T>,
    components: fn(&mut T) -> &mut [f64],
    quantity: &str,
) -> Vec<(ElemId, Result<T, String>)> {
    let cases = terms.iter().filter(|(_, factor)| *factor != 0.0);
    let Some((first, _)) = cases.clone().next().or_else(|| terms.first()) else {
        return Vec::new();
    };
    let mut sums: Vec<_> = responses(first)
        .iter()
        .map(|(id, _)| (*id, Ok(T::default())))
        .collect();
    for (res, factor) in cases {
        let current: HashMap<_, _> = responses(res)
            .iter()
            .map(|(id, force)| (*id, force))
            .collect();
        for (id, sum) in &mut sums {
            if let Ok(values) = sum {
                match current.get(id).copied() {
                    Some(Ok(ends)) => {
                        let mut ends = *ends;
                        for (out, &value) in components(values)
                            .iter_mut()
                            .zip(components(&mut ends).iter())
                        {
                            *out += factor * value;
                        }
                    }
                    Some(Err(reason)) => *sum = Err(reason.clone()),
                    None => *sum = Err(format!("合成対象ケースの柱端{quantity}がありません")),
                }
            }
        }
    }
    for (_, sum) in &mut sums {
        if sum
            .as_mut()
            .is_ok_and(|ends| components(ends).iter().any(|value| !value.is_finite()))
        {
            *sum = Err(format!("合成した柱端{quantity}が非有限です"));
        }
    }
    sums
}

fn superpose_girder_column_faces(
    terms: &[(&StaticOnce, f64)],
) -> Result<Vec<(ElemId, GirderColumnFaceEnds)>, SolveError> {
    let nonzero = || terms.iter().filter(|(_, factor)| *factor != 0.0);
    let base = nonzero().next().or_else(|| {
        terms
            .iter()
            .find(|(res, _)| !res.girder_column_face_forces.is_empty())
    });
    let mut sum = base.map_or_else(Vec::new, |(res, _)| res.girder_column_face_forces.clone());
    for face in sum
        .iter_mut()
        .flat_map(|(_, ends)| ends.iter_mut().flatten())
    {
        face.forces = Ok([0.0; 6]);
    }
    for (res, factor) in nonzero() {
        if sum.len() != res.girder_column_face_forces.len() {
            return Err(SolveError::InvalidInput(
                "合成する結果間で実柱面の大梁数が一致しません".into(),
            ));
        }
        for ((id, dest), (source_id, source)) in sum.iter_mut().zip(&res.girder_column_face_forces)
        {
            if id != source_id {
                return Err(SolveError::InvalidInput(format!(
                    "合成する結果間で実柱面の大梁IDが一致しません（{}と{}）",
                    id.0, source_id.0
                )));
            }
            for (side, (dest, source)) in dest.iter_mut().zip(source).enumerate() {
                let Ok(dest_face) = dest else { continue };
                let source_face = match source {
                    Ok(source_face) if dest_face.face == source_face.face => source_face,
                    Ok(_) => {
                        *dest = Err(column_face_reason(
                            *id,
                            side,
                            "合成する結果間で評価位置または支持柱が一致しません",
                        ));
                        continue;
                    }
                    Err(reason) => {
                        *dest = Err(reason.clone());
                        continue;
                    }
                };
                if let Ok(values) = &mut dest_face.forces {
                    dest_face.forces =
                        source_face
                            .forces
                            .as_ref()
                            .map_err(Clone::clone)
                            .and_then(|current| {
                                let next = std::array::from_fn(|d| values[d] + factor * current[d]);
                                if next.iter().all(|value| value.is_finite()) {
                                    Ok(next)
                                } else {
                                    Err(column_face_reason(*id, side, "合成した断面力が非有限です"))
                                }
                            });
                }
            }
        }
    }
    Ok(sum)
}

fn column_face_reason(id: ElemId, side: usize, reason: &str) -> String {
    format!("大梁 ID {} {}端の実柱面: {reason}", id.0, ["i", "j"][side])
}

fn recover_girder_column_faces(
    model: &Model,
    elem: &ElementData,
    adjacency: &NodeAdjacency,
    section: Option<&LoadedFrameSection>,
    results: &mut Vec<(ElemId, GirderColumnFaceEnds)>,
) {
    if let Some([i, j]) = girder_column_faces(model, elem, adjacency) {
        results.push((
            elem.id,
            [(0, i), (1, j)].map(|(side, face)| {
                face.map(|face| {
                    let forces = section
                        .ok_or_else(|| "要素に荷重込みの断面力回収がありません".to_owned())
                        .and_then(|section| {
                            section.at_continuous_section_mm(face.position_mm, face.length_mm)
                        })
                        .map_err(|reason| column_face_reason(elem.id, side, &reason));
                    GirderColumnFaceForces { face, forces }
                })
            }),
        ));
    }
}

/// 不存在の荷重ケース ID は、拘束状態・解析経路に関係なく ID を含む `SolveError::InvalidInput` を返す。
/// 全ケースの不正な部材荷重方向は、強度・拘束状態によらず `SolveError::InvalidInput` を返す。
/// 壁属性の参照先が不正なら、部材番号を含む `SolveError::InvalidInput` を返す。
pub fn linear_static_once(model: &Model, lc: LoadCaseId) -> Result<StaticOnce, SolveError> {
    model
        .validate_wall_attr_refs()
        .map_err(|e| SolveError::InvalidInput(e.to_string()))?;
    sepika_core::panel_zone::validate_panel_connection_offsets(model)
        .map_err(SolveError::InvalidInput)?;
    for panel in model
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::PanelZone)
    {
        if let Some(node) = panel.nodes.first() {
            sepika_core::panel_zone::resolve_explicit_panel_joint(model, *node, &model.elements)
                .map_err(|reason| SolveError::InvalidInput(reason.to_string()))?;
        }
    }
    sepika_element::factory::ensure_wall_side_column_analysis(model, None, "線形解析", None)
        .map_err(SolveError::InvalidInput)?;
    model
        .validate_member_load_directions()
        .map_err(SolveError::InvalidInput)?;
    model
        .validate_frame_node_counts()
        .map_err(|e| SolveError::InvalidInput(e.to_string()))?;
    model
        .validate_section_properties()
        .map_err(|e| SolveError::InvalidInput(e.to_string()))?;
    model
        .validate_rc_section_usage()
        .map_err(|e| SolveError::InvalidInput(e.to_string()))?;
    model
        .validate_secondary_section_usage()
        .map_err(|e| SolveError::InvalidInput(e.to_string()))?;
    let load_case = model
        .load_cases
        .iter()
        .find(|c| c.id == lc)
        .ok_or_else(|| SolveError::InvalidInput(format!("荷重ケース {} が存在しません", lc.0)))?;
    for elem in model.elements.iter().filter(|elem| {
        elem.kind.is_primary_frame_support() || matches!(elem.kind, ElementKind::Brace { .. })
    }) {
        if matches!(
            elem.kind,
            ElementKind::Beam | ElementKind::Fiber | ElementKind::MultiSpring
        ) {
            sepika_element::frame::beam::composite_props_of(model, elem)
                .map_err(SolveError::InvalidInput)?;
        }
        if let Some(reason) = sepika_element::factory::elastic_stiffness_input_issue(elem, model) {
            return Err(SolveError::InvalidInput(reason));
        }
    }
    sepika_math::parallelism::apply_to_faer();
    let dofmap = DofMap::build(model);
    let reducer = Reducer::build(model, &dofmap);
    solve_static_case(
        model,
        &dofmap,
        &reducer,
        load_case,
        None,
        TENSION_ONLY_MAX_ITER,
    )
}

pub(crate) fn solve_static_case(
    model: &Model,
    dofmap: &DofMap,
    reducer: &Reducer,
    load_case: &LoadCase,
    fixed: Option<(&dyn LinearSolver, &[crate::statics::BehaviorEntry])>,
    max_iter: usize,
) -> Result<StaticOnce, SolveError> {
    let physical = PhysicalLoadLedger::new(model, load_case)?;
    if reducer.n_indep > 0
        && model.stress_cfg.tension_only_iteration
        && has_tension_only_brace(model)
    {
        return solve_tension_only_iterative(
            model, dofmap, reducer, load_case, &physical, max_iter,
        );
    }
    let owned;
    let (solver, behaviors) = match fixed {
        Some(fixed) => fixed,
        None => {
            owned = StaticStiffness::build(model, dofmap, reducer, physical.source.kind)?;
            (owned.solver.as_ref(), owned.behaviors.as_slice())
        }
    };
    let entries: Vec<_> = behaviors
        .iter()
        .map(|(id, behavior, gdofs)| (*id, behavior.as_ref(), gdofs.as_slice()))
        .collect();
    validate_entries(model, dofmap, &entries)?;
    let rhs = assemble_linear_rhs(model, dofmap, &physical, &entries)?;
    let u_free = if reducer.n_indep == 0 {
        vec![0.0; dofmap.n_active()]
    } else {
        reducer.expand_u(&solver.solve(&reducer.reduce_f(&rhs))?)
    };
    recover_static(model, dofmap, &u_free, &physical, &entries).map(|(result, _)| result)
}

/// 引張専用ブレースの active-set 反復の最大回数。
pub(crate) const TENSION_ONLY_MAX_ITER: usize = 50;

/// モデルに引張専用ブレース（`ElementKind::Brace { tension_only: true }`）が
/// 少なくとも1本存在するか。
pub(crate) fn has_tension_only_brace(model: &Model) -> bool {
    model
        .elements
        .iter()
        .any(|e| matches!(e.kind, ElementKind::Brace { tension_only: true }))
}

/// active-set 反復で追跡する引張専用ブレース1本の情報。
struct ToBrace {
    /// `model.elements` 内の要素 index。
    elem: usize,
    /// i 端・j 端の節点 index。
    ni: usize,
    nj: usize,
    /// 部材軸単位ベクトル（i→j）。軸伸び δ = t·(u_j − u_i) の判定に用いる。
    t: [f64; 3],
}

/// ケースの長期係数を採用して引張専用反復を行う。
/// active 集合が不変なら採用剛性で内力を回復し、上限到達は `NonConvergence` を返す。
fn solve_tension_only_iterative(
    model: &Model,
    dofmap: &DofMap,
    reducer: &Reducer,
    load_case: &LoadCase,
    physical: &PhysicalLoadLedger<'_>,
    max_iter: usize,
) -> Result<StaticOnce, SolveError> {
    let mut braces: Vec<ToBrace> = Vec::new();
    for (i, e) in model.elements.iter().enumerate() {
        if !matches!(e.kind, ElementKind::Brace { tension_only: true }) || e.nodes.len() < 2 {
            continue;
        }
        let (ni, nj) = (e.nodes[0].index(), e.nodes[1].index());
        let (Some(n0), Some(n1)) = (model.nodes.get(ni), model.nodes.get(nj)) else {
            continue;
        };
        let [p_i, p_j] = model
            .primary_frame_axis_ends(e)
            .map_err(SolveError::InvalidInput)?
            .unwrap_or([n0.coord, n1.coord]);
        let d = sepika_core::geom::vec3::sub(p_j, p_i);
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l < 1e-12 {
            continue;
        }
        braces.push(ToBrace {
            elem: i,
            ni,
            nj,
            t: [d[0] / l, d[1] / l, d[2] / l],
        });
    }

    let n_active = dofmap.n_active();
    let assembly = BraceIterAssembly::build(model, dofmap, &braces, load_case.kind);
    let mut k_free_cache = CscCache::new();
    let mut k_red_cache = CscCache::new();
    let mut solver = make_solver(SolverBackend::DirectSparseCholesky);

    let mut active = vec![true; braces.len()];

    for _ in 0..max_iter {
        let active_used = active.clone();
        let entries: Vec<_> = assembly
            .gdofs
            .iter()
            .enumerate()
            .map(|(i, gdofs)| {
                (
                    model.elements[i].id,
                    assembly.behavior_for(i, &active_used),
                    gdofs.as_slice(),
                )
            })
            .collect();
        let f_red = reducer.reduce_f(&assemble_linear_rhs(model, dofmap, physical, &entries)?);
        let triplets = assembly.triplets(&active_used);
        let k_free = k_free_cache.assemble(n_active, &triplets);
        let k_red = reducer.reduce_k_cached(&k_free, &mut k_red_cache);

        solver.factorize(&k_red).map_err(|error| match error {
            SolveError::NotPositiveDefinite => SolveError::InvalidInput(
                crate::statics::analysis::precheck::singular_diagnosis(model),
            ),
            other => other,
        })?;
        let u_indep = solver.solve(&f_red)?;
        let u_free = reducer.expand_u(&u_indep);

        let new_active: Vec<bool> = braces
            .iter()
            .map(|b| {
                let du = |ni: usize, d: usize| -> f64 {
                    let g = ni * sepika_core::dof::DOF_PER_NODE + d;
                    dofmap.active(g).map(|a| u_free[a as usize]).unwrap_or(0.0)
                };
                let physical = |node, side| {
                    let translation = [du(node, 0), du(node, 1), du(node, 2)];
                    let rotation = [du(node, 3), du(node, 4), du(node, 5)];
                    sepika_core::geom::vec3::add(
                        translation,
                        sepika_core::geom::vec3::cross(
                            rotation,
                            model.elements[b.elem].end_offsets_mm[side],
                        ),
                    )
                };
                let delta = sepika_core::geom::vec3::sub(physical(b.nj, 1), physical(b.ni, 0));
                sepika_core::geom::vec3::dot(b.t, delta) >= 0.0
            })
            .collect();

        if new_active == active_used {
            return recover_static(model, dofmap, &u_free, physical, &entries)
                .map(|(result, _)| result);
        }
        active = new_active;
    }
    Err(SolveError::NonConvergence(format!(
        "荷重ケース「{}」(ID {}) の引張専用反復が上限 {} 回で収束しません",
        load_case.name, load_case.id.0, max_iter
    )))
}

/// active-set 反復1回分の要素アセンブリ情報。反復間で不変な
/// （global_dofs・局所剛性行列・内力回収用 behavior）を1回だけ計算して保持し、
/// 各反復では「引張専用ブレースの active/disabled のどちらの局所剛性を使うか」を
/// 選択して triplet 化するだけにする。
struct BraceIterAssembly {
    /// 要素ごとの global_dofs（active-set に依らず不変）。
    gdofs: Vec<smallvec::SmallVec<[usize; 24]>>,
    /// 要素ごとの「有効時」局所剛性行列（ケースの長期係数を採用）。
    k_active: Vec<sepika_element::behavior::LocalMat>,
    /// 引張専用ブレース要素のみ Some：無効化時（軸剛性 ×[`AXIAL_DISABLE_FACTOR`]）の
    /// 局所剛性行列。それ以外の要素は常に None（無効化され得ない）。
    k_disabled: Vec<Option<sepika_element::behavior::LocalMat>>,
    /// 内力回収用の behavior（有効時）。
    behavior_active: Vec<Box<dyn ElementBehavior>>,
    /// 引張専用ブレース要素のみ Some：内力回収用の behavior（無効化時）。
    behavior_disabled: Vec<Option<Box<dyn ElementBehavior>>>,
    /// 要素 index → `braces` 配列上のインデックス（引張専用ブレースのみ）。
    brace_of_elem: HashMap<usize, usize>,
    /// 支点ばねの対角項（ブレースの有効/無効に依存せず不変）。
    spring_terms: Vec<(usize, f64)>,
}

impl BraceIterAssembly {
    /// `dofmap` は元モデル（`model`）から構築したもの。
    fn build(model: &Model, dofmap: &DofMap, braces: &[ToBrace], kind: LoadCaseKind) -> Self {
        let brace_of_elem: HashMap<usize, usize> = braces
            .iter()
            .enumerate()
            .map(|(k, b)| (b.elem, k))
            .collect();

        let n_elem = model.elements.len();
        let mut gdofs = Vec::with_capacity(n_elem);
        let mut k_active = Vec::with_capacity(n_elem);
        let mut k_disabled = Vec::with_capacity(n_elem);
        let mut behavior_active = Vec::with_capacity(n_elem);
        let mut behavior_disabled = Vec::with_capacity(n_elem);

        for (i, elem) in model.elements.iter().enumerate() {
            let factor = long_axial_factor(model, elem, kind);
            let b_active = build_behavior_with_axial_factor(elem, model, factor);
            let g = b_active.global_dofs(dofmap);
            let k = b_active.tangent_stiffness(&Ctx { model });

            if brace_of_elem.contains_key(&i) {
                let b_disabled =
                    build_behavior_with_axial_factor(elem, model, factor * AXIAL_DISABLE_FACTOR);
                let kd = b_disabled.tangent_stiffness(&Ctx { model });
                k_disabled.push(Some(kd));
                behavior_disabled.push(Some(b_disabled));
            } else {
                k_disabled.push(None);
                behavior_disabled.push(None);
            }
            gdofs.push(g);
            k_active.push(k);
            behavior_active.push(b_active);
        }

        let spring_terms = support_spring_terms(model, dofmap);

        Self {
            gdofs,
            k_active,
            k_disabled,
            behavior_active,
            behavior_disabled,
            brace_of_elem,
            spring_terms,
        }
    }

    /// 要素 index の、現在の `active` 集合における局所剛性行列を返す。
    fn k_local_for(&self, i: usize, active: &[bool]) -> &sepika_element::behavior::LocalMat {
        match self.brace_of_elem.get(&i) {
            Some(&bidx) if !active[bidx] => self.k_disabled[i]
                .as_ref()
                .expect("引張専用ブレース要素は k_disabled を必ず持つ"),
            _ => &self.k_active[i],
        }
    }

    /// 要素 index の、現在の `active` 集合における内力回収用 behavior を返す。
    fn behavior_for(&self, i: usize, active: &[bool]) -> &dyn ElementBehavior {
        match self.brace_of_elem.get(&i) {
            Some(&bidx) if !active[bidx] => self.behavior_disabled[i]
                .as_deref()
                .expect("引張専用ブレース要素は behavior_disabled を必ず持つ"),
            _ => self.behavior_active[i].as_ref(),
        }
    }

    /// 現在の `active` 集合に基づき全体剛性 K（縮約前）の triplet 列を組み立てる。
    fn triplets(&self, active: &[bool]) -> Vec<Triplet> {
        let mut triplets = Vec::new();
        for i in 0..self.gdofs.len() {
            triplets.extend(self.k_local_for(i, active).to_triplets(&self.gdofs[i]));
        }
        for &(a, k) in &self.spring_terms {
            triplets.push(Triplet {
                row: a,
                col: a,
                val: k,
            });
        }
        triplets
    }
}

fn recover_static(
    model: &Model,
    dofmap: &DofMap,
    u_free: &[f64],
    physical: &PhysicalLoadLedger<'_>,
    entries: &[(ElemId, &dyn ElementBehavior, &[usize])],
) -> Result<(StaticOnce, Vec<[f64; 6]>), SolveError> {
    validate_entries(model, dofmap, entries)?;
    let full = dofmap.including_restrained_node_dofs();
    let mut reactions: Vec<_> = physical
        .nodal
        .iter()
        .map(|values| values.map(|v| -v))
        .collect();
    let mut member_forces = Vec::new();
    let mut column_end_horizontal_forces = Vec::new();
    let mut column_end_axial_forces = Vec::new();
    let mut panel_moments = Vec::new();
    let adjacency = NodeAdjacency::primary_frame(model);
    let mut girder_column_face_forces = Vec::new();
    for (elem, &(_, behavior, gdofs)) in model.elements.iter().zip(entries) {
        let u_elem = crate::common::elem_loop::gather_u_elem(gdofs, u_free);
        let loads = physical.member_loads(elem.id);
        if behavior.owns_frame_member_loads() {
            let response = loaded_response(elem, behavior, &u_elem, loads)?;
            let positions = behavior
                .recover_forces(&u_elem)
                .ok_or_else(|| element_error(elem.id, "部材内力の評価位置がありません"))?
                .at
                .iter()
                .map(|(xi, _)| *xi)
                .collect::<Vec<_>>();
            let forces = response
                .section
                .recover(&positions)
                .map_err(|reason| element_error(elem.id, reason))?;
            member_forces.push((elem.id, forces));
            if elem.kind.is_primary_frame_support()
                && model
                    .element_section(elem)
                    .and_then(|section| section.frame_use)
                    == Some(FrameSectionUse::Column)
            {
                let end_forces = response.section.inward_end_forces();
                if elem.kind == ElementKind::Beam {
                    column_end_horizontal_forces.push((elem.id, end_forces.horizontal));
                }
                column_end_axial_forces.push((elem.id, end_forces.axial));
            }
            recover_girder_column_faces(
                model,
                elem,
                &adjacency,
                Some(&response.section),
                &mut girder_column_face_forces,
            );
            for (i, node) in elem.nodes.iter().enumerate() {
                for d in 0..6 {
                    reactions[node.index()][d] += response.nodal_residual.data[i * 6 + d];
                }
            }
        } else {
            recover_girder_column_faces(
                model,
                elem,
                &adjacency,
                None,
                &mut girder_column_face_forces,
            );
            if let Some(mut forces) = behavior.recover_forces(&u_elem) {
                superpose_member_loads(model, elem, loads, &mut forces);
                member_forces.push((elem.id, forces));
            }
            let k = behavior.tangent_stiffness(&Ctx { model });
            let physical_gdofs = behavior.global_dofs(&full);
            for (i, &a) in physical_gdofs.iter().enumerate() {
                if a == usize::MAX {
                    continue;
                }
                let g = full.global(a as u32);
                if full.is_node_dof(g) {
                    reactions[g / 6][g % 6] += u_elem
                        .iter()
                        .enumerate()
                        .map(|(j, u)| k.get(i, j) * u)
                        .sum::<f64>();
                }
            }
            let equivalent = legacy_member_force(model, elem, loads);
            let equivalent =
                sepika_element::frame::eccentric::transform_nodal_load(elem, model, &equivalent);
            for (i, node) in elem.nodes.iter().take(2).enumerate() {
                for d in 0..6 {
                    reactions[node.index()][d] -= equivalent.data[i * 6 + d];
                }
            }
        }
        if let (Some(&node), Some(m)) = (elem.nodes.first(), behavior.panel_moments_from(&u_elem)) {
            panel_moments.push((node, m));
        }
    }
    ensure_line_member_forces(model, &member_forces)?;
    let nodal = reactions
        .iter()
        .zip(&physical.nodal)
        .enumerate()
        .map(|(i, (r, p))| (i, std::array::from_fn(|d| r[d] + p[d])))
        .collect();
    let state = crate::nonlinear::pushover::physical_response::PhysicalStep::from_nodal(
        model,
        dofmap,
        crate::nonlinear::pushover::physical_response::fixed_dofs(model, dofmap),
        u_free,
        vec![nodal],
        physical.source.clone(),
        physical.source.clone(),
    )
    .map_err(SolveError::InvalidInput)?;
    let story_shears = Some([
        state
            .capacity_forces(model, dofmap, crate::statics::analysis::SeismicDir::X)
            .map_err(SolveError::InvalidInput)?
            .story,
        state
            .capacity_forces(model, dofmap, crate::statics::analysis::SeismicDir::Y)
            .map_err(SolveError::InvalidInput)?
            .story,
    ]);
    Ok((
        StaticOnce {
            story_shears,
            stiffness_ratio_disp: None,
            story_rigidity: Vec::new(),
            disp: dofmap.expand_to_nodes(u_free, model.nodes.len()),
            member_forces,
            panel_moments,
            girder_column_face_forces,
            column_end_horizontal_forces,
            column_end_axial_forces,
        },
        reactions,
    ))
}

/// 内力回収の欠落検出（線材）。
/// 線材（梁・柱＝`Beam`、`Fiber`、`MultiSpring`、ブレース）は必ず
/// `ElementBehavior::recover_forces` を実装している必要がある。実装されていない
/// 要素は `recover_forces` が `None` を返し、線形静解析のループで**黙って
/// 読み飛ばされて** `member_forces` から丸ごと欠落する。欠落した部材は応力図・
/// 断面検定・柱梁接合部検定・設計用せん断力 QD のいずれにも現れず、
/// 「結果が空である」ことがユーザーからは正常な計算結果と区別できない。
pub(crate) fn ensure_line_member_forces(
    model: &Model,
    member_forces: &[(sepika_core::ids::ElemId, MemberForces)],
) -> Result<(), SolveError> {
    use std::collections::HashSet;

    let recovered: HashSet<sepika_core::ids::ElemId> =
        member_forces.iter().map(|(id, _)| *id).collect();
    let missing: Vec<u32> = model
        .elements
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                ElementKind::Beam
                    | ElementKind::Fiber
                    | ElementKind::MultiSpring
                    | ElementKind::Brace { .. }
            )
        })
        .filter(|e| !recovered.contains(&e.id))
        .map(|e| e.id.0)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let head: Vec<String> = missing.iter().take(5).map(|id| id.to_string()).collect();
    let more = if missing.len() > 5 {
        format!(" 他{}件", missing.len() - 5)
    } else {
        String::new()
    };
    Err(SolveError::InvalidInput(format!(
        "線材の部材内力を回収できませんでした: 部材 ID {}{}。\
         要素実装の不具合です（このまま続けると応力図・断面検定から当該部材が\
         無言で欠落します）。",
        head.join(", "),
        more
    )))
}

/// 部材荷重の固定端内力を、`K·u` 由来の回復内力へ各断面で重ね合わせる。
/// 線形重ね合わせ: 実内力 = （等価節点力に対する応答 K·u）＋（両端固定梁のスパン内力）。
/// `loads` は当該要素に作用する部材荷重のみを、元の出現順で与える。
pub(crate) fn superpose_member_loads(
    model: &Model,
    elem: &sepika_core::model::ElementData,
    loads: &[MemberLoad],
    forces: &mut sepika_element::frame::beam::MemberForces,
) {
    if loads.is_empty() {
        return;
    }
    let Some((frame, length)) = crate::common::assemble::member_load_frame(model, elem) else {
        return;
    };
    for (xi, vals) in forces.at.iter_mut() {
        let fixed = sepika_element::frame::member_load::fixed_internal_local(
            loads,
            &frame,
            length,
            *xi,
            crate::common::assemble::span_load_transfer(elem),
        );
        for k in 0..6 {
            vals[k] += fixed[k];
        }
    }
}

#[cfg(test)]
mod tests;
