//! 節点変位0の梁CMQ図（端部固定値・荷重込み曲げ・せん断）。
//!
//! 描画ソースは表示中荷重ケース（`nav.focus_load_case`。応力図と異なり解析実行を
//! 要しない）の `member`（部材荷重）そのもの。床分配・自重・取り付く壁版の線
//! アンカー・手入力荷重のいずれも同じ図に重ねて描く。

use std::collections::HashMap;

use crate::app::App;
use crate::theme;

use super::{
    diagram,
    scene::{diagram_offset_dir, in_plane_offset_dir},
    CmqAxes, CmqComponent, DiagramPlane, FrameFilter, Projector,
};
use sepika_core::geom::vec3::dist as member_len3;
use sepika_core::ids::ElemId;
use sepika_core::model::{
    LoadCase, MemberLoadKind, Model, DL_CASE_NAME, LL_FRAME_CASE_NAME, LL_SEISMIC_CASE_NAME,
};

/// 名前が「準備計算で `member` が埋まる自動ケース」（DL・LL(架構用)・LL(地震用)）と
/// 一致するか。**EX・EY は含まない**: これらは水平力を節点（`NodalLoad`）でしか
/// 表現しない設計（`build_seismic_load_case_from_model`）のため、準備計算を実行しても
/// `member` は常に空のままであり、「準備計算を実行すると更新される」という案内が
/// 当てはまらない。
///
/// DL・LL(架構用)・LL(地震用) の `member` は準備計算（`sync_gravity_load_cases_action`。
/// 解析実行の入口 `ensure_preparation` からしか呼ばれない）が書き込む。モデル読込直後・
/// スラブ編集直後など、準備計算を一度も実行していない状態では中身が空のままになりうる
/// ため、CMQ図の空表示メッセージでこの可能性を案内する（`draw_cmq_diagram`）。
fn is_gravity_auto_case_name(name: &str) -> bool {
    matches!(
        name,
        DL_CASE_NAME | LL_FRAME_CASE_NAME | LL_SEISMIC_CASE_NAME
    )
}

/// 選択中の軸（`app.ui.view.cmq_axes`）を表す表示ラベル。凡例と空表示メッセージの
/// 双方で使う（`axes.is_empty()` は呼び出し前に早期 return 済みの前提。
/// 両方 false のまま呼ぶと該当なしでパニックする）。
fn selected_axis_label(axes: CmqAxes) -> &'static str {
    match (axes.ey, axes.ez) {
        (true, true) => "強軸+弱軸",
        (true, false) => "強軸(ey)",
        (false, true) => "弱軸(ez)",
        (false, false) => unreachable!("axes.is_empty() で早期 return 済み"),
    }
}

fn is_primary_beam_for_cmq(model: &Model, elem: &sepika_core::model::ElementData) -> bool {
    if elem.kind != sepika_core::model::ElementKind::Beam || elem.nodes.len() != 2 {
        return false;
    }
    let (n0, n1) = (elem.nodes[0], elem.nodes[1]);
    let (Some(c0), Some(c1)) = (
        model.nodes.get(n0.index()).map(|n| n.coord),
        model.nodes.get(n1.index()).map(|n| n.coord),
    ) else {
        return true;
    };
    let tol = sepika_core::geom::MEMBER_AXIS_TOL_MM;
    let near = |p: [f64; 3], q: [f64; 3]| sepika_core::geom::vec3::dist(p, q) <= tol;
    let is_materialized_beam = model.beams().any(|sm| {
        model
            .secondary_member_end_points(sm)
            .is_some_and(|(a, b)| (near(a, c0) && near(b, c1)) || (near(a, c1) && near(b, c0)))
    });
    !is_materialized_beam
}

/// 一つの主架構の大梁（`ElemId`）に載る、表示中荷重ケースの全 `MemberLoad` を束ねた
/// グループ。荷重は（種別, 世界座標の作用方向）の対で持ち、局所 ey/ez 面への投影は
/// 描画時に行う（投影に要素の現在の節点座標が要るため）。
struct CmqElemGroup {
    /// 対象の大梁。構面表示の絞り込み（`FrameFilter`）で使う。
    elem: ElemId,
    n0: usize,
    n1: usize,
    ref_vec: [f64; 3],
    loads: Vec<(MemberLoadKind, [f64; 3])>,
}

/// 表示中荷重ケースの `member` を要素（大梁）単位でグループ化する。大梁の中間区間
/// （小梁がとりつく位置）の荷重も同じ `ElemId` に変換済みのため、大梁1本=1グループに
/// なる。小梁・柱・スラブには `MemberLoad` が付かない（または実部材化小梁として
/// `is_primary_beam_for_cmq` で除外される）ため自然に描画対象から外れる。柱への
/// 節点集中荷重（`NodalLoad`）はそもそも `member` に含まれないため、同じ理由で
/// 梁の図に出ない（梁に載らない荷重であり、これは正しい）。
fn group_member_loads_by_elem(model: &Model, case: &LoadCase) -> Vec<CmqElemGroup> {
    let mut order: Vec<ElemId> = Vec::new();
    let mut groups: HashMap<ElemId, CmqElemGroup> = HashMap::new();
    for ml in &case.member {
        let Some(elem) = model.element(ml.elem) else {
            continue;
        };
        if !is_primary_beam_for_cmq(model, elem) {
            continue;
        }
        let group = groups.entry(ml.elem).or_insert_with(|| {
            order.push(ml.elem);
            CmqElemGroup {
                elem: ml.elem,
                n0: elem.nodes[0].index(),
                n1: elem.nodes[1].index(),
                ref_vec: elem.local_axis.ref_vector,
                loads: Vec::new(),
            }
        });
        group.loads.push((ml.kind.clone(), ml.dir));
    }
    order
        .into_iter()
        .filter_map(|id| groups.remove(&id))
        .collect()
}

/// `MemberLoadKind` の大きさ（強度・集中荷重）を、世界座標の作用方向 `dir` から
/// 局所軸の単位ベクトル `axis`（ey または ez。単位ベクトル前提）へ投影する。
/// 位置（`a`/`b`）は部材長に沿った値のためそのまま、強度・大きさだけを
/// `-dot(dir, axis)` 倍する（`dir` は呼び出し前に正規化していなくてもよい。
/// 内部で正規化する。長さがほぼ 0 の退化した向きは、どの面にも投影できないため
/// 寄与 0 として扱う）。
///
/// **符号は `dot` ではなく `-dot`。** `sepika_load::floor::fem` の固定端モーメント・
/// 単純梁公式（`fixed_end_moments`/`simple_reactions`/`simple_beam_moment_at`）は、
/// 正の強度・集中荷重を「`axis` の負方向（重力のように）へ作用する」ものとして
/// 扱う教科書的な単純梁公式であり、この描画コードの規約（`+ey側=梁上側`、
/// `sagging正=-ey側=下`）とも整合する。標準的な水平梁（`ref_vector=[0,0,1]` →
/// 局所 ey は世界 +Z）に鉛直荷重（`dir=[0,0,-1]`）が作用する既定ケースでは
/// `dot(dir,ey) = -1` になるため、素朴な `dot` を使うと、この改修より前から
/// 検証済みだった符号（投影なしで `w` をそのまま使っていた）が反転してしまう。
/// `-dot(dir,axis)` はこの既定ケースで `+1` となり、在来の符号を保ったまま
/// 傾いた部材・弱軸成分（軸が世界上下と一致しない場合）へ一般化する。
///
/// **`dir` の正規化について**: `MemberLoad.dir` は構造体のドキュメントで「内部で
/// 正規化」と説明されているが、`MemberLoad::manual`/`auto` のコンストラクタ自体は
/// 正規化しない（呼び出し側が単位ベクトルを渡す前提）。実際の解析で部材荷重を
/// 局所軸へ分解する `sepika_element::frame::member_load::resolve` は、この前提に
/// 頼らず明示的に正規化してから内積を取っている。本関数も同じ流儀に揃え、
/// 呼び出し側の実装詳細（すべて単位ベクトル定数）に暗黙に依存しないようにする。
fn project_load(kind: MemberLoadKind, dir: [f64; 3], axis: [f64; 3]) -> MemberLoadKind {
    let dl = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let s = if dl < 1e-12 {
        0.0
    } else {
        -(dir[0] * axis[0] + dir[1] * axis[1] + dir[2] * axis[2]) / dl
    };
    match kind {
        MemberLoadKind::Point { a, p } => MemberLoadKind::Point { a, p: p * s },
        MemberLoadKind::Distributed { a, b, w1, w2 } => MemberLoadKind::Distributed {
            a,
            b,
            w1: w1 * s,
            w2: w2 * s,
        },
    }
}

/// グループ内の全荷重の両端固定端モーメントを合算する（C 図）。
#[cfg(test)]
fn sum_fixed_end_moments(loads: &[MemberLoadKind], l: f64) -> (f64, f64) {
    loads
        .iter()
        .map(|ld| sepika_load::floor::fixed_end_moments(ld, l))
        .fold((0.0, 0.0), |(ai, aj), (ci, cj)| (ai + ci, aj + cj))
}

/// グループ内の全荷重の単純梁反力を合算する（Q 図）。
#[cfg(test)]
fn sum_simple_reactions(loads: &[MemberLoadKind], l: f64) -> (f64, f64) {
    loads
        .iter()
        .map(|ld| sepika_load::floor::simple_reactions(ld, l))
        .fold((0.0, 0.0), |(ai, aj), (ri, rj)| (ai + ri, aj + rj))
}

/// M・Q図の折れ線サンプリング位置 ξ∈[0,1] を返す。
/// 等分割に加え、`loads` に含まれる区間分布荷重の両端 a/L, b/L・集中荷重の a/L を
/// 折れ点として正確に出すため追加する。
fn cmq_m_sample_xis(loads: &[MemberLoadKind], l: f64) -> Vec<f64> {
    const N: usize = 32;
    let mut xis: Vec<f64> = (0..=N).map(|k| k as f64 / N as f64).collect();
    if l > 1e-9 {
        for load in loads {
            match *load {
                MemberLoadKind::Point { a, .. } => {
                    let xi = (a / l).clamp(0.0, 1.0);
                    xis.push(xi);
                    if xi < 1.0 {
                        xis.push((a.next_up() / l).next_up().min(1.0));
                    }
                }
                MemberLoadKind::Distributed { a, b, .. } => {
                    xis.push((a / l).clamp(0.0, 1.0));
                    xis.push((b / l).clamp(0.0, 1.0));
                }
            }
        }
    }
    xis.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    xis.dedup_by(|a, b| *a == *b);
    xis
}

/// ポリゴンを塗り（`convex_polygon`, `Stroke::NONE`）と輪郭（閉じない折れ線
/// `Shape::line`）に分けて描画する。塗り+輪郭を1シェイプ（閉路）にすると、
/// p0/p1 で軸線と曲線が浅い角度で接する折り返し点の epaint マイター結合が発散し、
/// 部材軸方向に画面外まで伸びるスパイク描画になるため、輪郭は閉じない折れ線にする。
pub(super) fn paint_diagram_polygon(
    painter: &egui::Painter,
    points: Vec<egui::Pos2>,
    fill: egui::Color32,
    stroke_color: egui::Color32,
) {
    painter.add(egui::Shape::convex_polygon(
        points.clone(),
        fill,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.5_f32, stroke_color),
    ));
}

/// 1 大梁・1 軸（ey または ez）分の描画対象。荷重は `axis`（局所軸の単位ベクトル。
/// 世界座標）へ投影済みの `MemberLoadKind` 列で持つ。
struct CmqTrace {
    ends: [[f64; 3]; 2],
    l: f64,
    /// 描画位置の張り出し方向（構面表示では `in_plane_offset_dir` で構面内へ倒し済み）。
    offset_dir: [f64; 3],
    loads: Vec<MemberLoadKind>,
    section: sepika_element::frame::member_load::LoadedFrameSection,
    c: (f64, f64),
    /// 投影元の面（強軸 ey／弱軸 ez）。色は成分（C/M/Q）で決まるため軸では変えないが、
    /// **2D 構面表示では ey・ez の張り出し方向が同一直線上に潰れる**
    /// （`in_plane_offset_dir` は材軸と構面法線だけから向きを決め、投影元の面を見ない
    /// ため。応力図の強軸 Mz・弱軸 My が同じ制約を持つのと同じ理由）。応力図は成分ごとに
    /// 色そのものが違う（Mz=紫・My=赤）ため重なっても判別できるが、CMQ 図は軸に関わらず
    /// 成分色が共通のため、区別できるよう ez を半透明で描く（`plane_alpha_scale`）。
    plane: DiagramPlane,
}

fn trace_max_shear(trace: &CmqTrace) -> f64 {
    let index = match trace.plane {
        DiagramPlane::Ey => 1,
        DiagramPlane::Ez => 2,
    };
    cmq_m_sample_xis(&trace.loads, trace.l)
        .into_iter()
        .map(|xi| trace.section.at(xi).expect("CMQの有限な全長比")[index].abs())
        .fold(0.0_f64, f64::max)
}

fn trace_moment(trace: &CmqTrace, xi: f64) -> f64 {
    let force = trace.section.at(xi).expect("CMQの有限な全長比");
    match trace.plane {
        DiagramPlane::Ey => force[5],
        DiagramPlane::Ez => -force[4],
    }
}

/// `CmqTrace::plane` に応じた塗り・輪郭の不透明度倍率（0.0〜1.0）。
/// 2D 構面表示で強軸・弱軸のトレースが同一直線上に重なっても判別できるよう、
/// 弱軸(ez)を半透明にする（3D 表示では方向自体が分かれるため実害はないが、
/// 軸ごとに描き分けを変えると挙動が複雑になるため 3D でも同じ規則で統一する）。
fn plane_alpha_scale(plane: DiagramPlane) -> f32 {
    match plane {
        DiagramPlane::Ey => 1.0,
        DiagramPlane::Ez => 0.5,
    }
}

/// 部材ローカルに沿って CMQ 図（両端固定端モーメント C・荷重込み曲げモーメント M・
/// せん断 Q）を描く。
///
/// 応力図と同じ「面」の区別（強軸 ey・弱軸 ez。`app.ui.view.cmq_axes`）を持つ。部材の局所軸が
/// 傾いている（斜め柱・ひねりのある断面等）場合、鉛直荷重でも ey・ez 両方に成分が
/// 生じうるため、実際に `MemberLoad.dir` を局所軸へ投影して評価する
/// （[`project_load`]）。両軸を同時表示するときは同一スケールで重ねて描く。
pub(super) fn draw_cmq_diagram(
    painter: &egui::Painter,
    app: &App,
    _coords3: &[[f64; 3]],
    proj: &Projector,
    filter: FrameFilter,
    frame_normal: Option<[f64; 3]>,
) {
    let scale = proj.scale();

    fn info_text(painter: &egui::Painter, msg: &str) {
        painter.text(
            egui::pos2(
                painter.clip_rect().min.x + 10.0,
                painter.clip_rect().min.y + 30.0,
            ),
            egui::Align2::LEFT_TOP,
            msg,
            egui::FontId::proportional(13.0),
            theme::GRAY_600,
        );
    }

    let Some(case) = app.cmq_display_load_case() else {
        info_text(
            painter,
            "荷重ケースがありません。荷重タブで作成するか、「準備計算」タブで準備計算を\
             実行してください（スラブ等が定義されていれば DL 等のケースが自動生成されます）",
        );
        return;
    };

    let axes: Vec<DiagramPlane> = [
        (app.ui.view.cmq_axes.ey, DiagramPlane::Ey),
        (app.ui.view.cmq_axes.ez, DiagramPlane::Ez),
    ]
    .into_iter()
    .filter_map(|(on, plane)| on.then_some(plane))
    .collect();
    if axes.is_empty() {
        info_text(painter, "「軸:」で強軸・弱軸のどちらかを選んでください");
        return;
    }

    let groups: Vec<CmqElemGroup> = group_member_loads_by_elem(&app.core.model, case)
        .into_iter()
        .filter(|g| filter.shows(g.elem))
        .collect();

    if groups.is_empty() {
        let sync_hint = if is_gravity_auto_case_name(&case.name) {
            "。モデル読込直後やスラブ編集直後で「準備計算」タブをまだ実行していない場合、\
             実行すると内容が更新されます"
        } else {
            ""
        };
        info_text(
            painter,
            &format!(
                "荷重ケース「{}」に大梁への部材荷重がありません。\
                 荷重タブで荷重を追加するか、ナビゲータで他のケースを選択してください{sync_hint}",
                case.name
            ),
        );
        return;
    }

    let mut cmq_error = None;
    let responses: HashMap<_, _> = groups
        .iter()
        .filter_map(|group| {
            let data = app.core.model.element(group.elem)?;
            let loads = group
                .loads
                .iter()
                .map(|(kind, dir)| {
                    sepika_core::model::MemberLoad::manual(group.elem, *dir, kind.clone())
                })
                .collect::<Vec<_>>();
            match sepika_element::frame::member_load::cmq_with_rigid_zone(
                &app.core.model,
                data,
                &loads,
            ) {
                Ok(response) => {
                    let ends = app
                        .core
                        .model
                        .primary_beam_axis_ends(data)
                        .expect("CMQ応答で検証済みの実材軸")
                        .unwrap_or_else(|| {
                            [
                                app.core.model.nodes[group.n0].coord,
                                app.core.model.nodes[group.n1].coord,
                            ]
                        });
                    Some((group.elem, (response.section, ends)))
                }
                Err(reason) => {
                    cmq_error = Some(reason);
                    None
                }
            }
        })
        .collect();
    if let Some(reason) = cmq_error {
        info_text(painter, &format!("CMQを計算できません: {reason}"));
        return;
    }
    let responses = &responses;
    let traces: Vec<CmqTrace> = groups
        .iter()
        .flat_map(|g| {
            let (section, ends) = &responses[&g.elem];
            let [p_i, p_j] = *ends;
            let l = member_len3(p_i, p_j);
            axes.iter().filter_map(move |&plane| {
                let axis_dir = diagram_offset_dir(p_i, p_j, g.ref_vec, plane)?;
                let offset_dir = match frame_normal {
                    Some(n) => in_plane_offset_dir(axis_dir, p_i, p_j, n),
                    None => axis_dir,
                };
                let loads = g
                    .loads
                    .iter()
                    .map(|(kind, dir)| project_load(kind.clone(), *dir, axis_dir))
                    .collect();
                let section = section.clone();
                let (m_index, sign) = match plane {
                    DiagramPlane::Ey => (5, 1.0),
                    DiagramPlane::Ez => (4, -1.0),
                };
                let fi = section.at(0.0).ok()?;
                let fj = section.at(1.0).ok()?;
                Some(CmqTrace {
                    ends: *ends,
                    l,
                    offset_dir,
                    loads,
                    plane,
                    c: (-sign * fi[m_index], sign * fj[m_index]),
                    section,
                })
            })
        })
        .collect();

    let max_c = traces
        .iter()
        .map(|t| {
            let (c_i, c_j) = t.c;
            c_i.abs().max(c_j.abs())
        })
        .fold(0.0_f64, f64::max);
    let max_q = traces.iter().map(trace_max_shear).fold(0.0_f64, f64::max);
    let max_m = traces
        .iter()
        .map(|t| {
            cmq_m_sample_xis(&t.loads, t.l)
                .into_iter()
                .fold(0.0_f64, |acc, xi| acc.max(trace_moment(t, xi).abs()))
        })
        .fold(0.0_f64, f64::max);
    if max_c < 1e-12 && max_q < 1e-12 && max_m < 1e-12 {
        info_text(
            painter,
            &format!(
                "選択中の軸（{}）には有意な荷重成分がありません。\
                 「軸:」で他の軸も試すか、他の成分・荷重ケースを確認してください",
                selected_axis_label(app.ui.view.cmq_axes)
            ),
        );
        return;
    }
    let c_amp = 60.0 / max_c.max(1e-12) / scale as f64;
    let q_amp = 60.0 / max_q.max(1e-12) / scale as f64;
    let m_amp = 60.0 / max_m.max(1e-12) / scale as f64;

    for t in &traces {
        let [p_i, p_j] = t.ends;
        let p0 = proj.project(p_i);
        let p1 = proj.project(p_j);
        let ey = t.offset_dir;
        let alpha = plane_alpha_scale(t.plane);
        let fill_alpha = (60.0 * alpha).round() as u8;
        let stroke_alpha = (255.0 * alpha).round() as u8;

        match app.ui.view.cmq_component {
            CmqComponent::C => {
                let (c_i, c_j) = t.c;
                let peak_px = (60.0 * c_i.abs().max(c_j.abs()) / max_c.max(1e-12)) as f32;
                if peak_px < diagram::MIN_DIAGRAM_PX {
                    continue;
                }
                let c_poly = vec![
                    p0,
                    proj.project_offset(p_i, ey, c_i * c_amp),
                    proj.project_offset(p_j, ey, -c_j * c_amp),
                    p1,
                ];
                paint_diagram_polygon(
                    painter,
                    c_poly,
                    theme::translucent(theme::DATA_BLUE, fill_alpha),
                    theme::translucent(theme::DATA_BLUE, stroke_alpha),
                );
            }
            CmqComponent::M => {
                let xis = cmq_m_sample_xis(&t.loads, t.l);
                let mut val_max = 0.0_f64;
                let samples: Vec<(f64, f64)> = xis
                    .into_iter()
                    .map(|xi| {
                        let val = trace_moment(t, xi);
                        val_max = val_max.max(val.abs());
                        (xi, val)
                    })
                    .collect();
                let peak_px = (60.0 * val_max / max_m.max(1e-12)) as f32;
                if peak_px < diagram::MIN_DIAGRAM_PX
                    || samples
                        .iter()
                        .any(|&(xi, val)| !xi.is_finite() || !val.is_finite())
                {
                    continue;
                }
                let project_sample = |(xi, val): (f64, f64)| {
                    let base3 = [
                        p_i[0] + (p_j[0] - p_i[0]) * xi,
                        p_i[1] + (p_j[1] - p_i[1]) * xi,
                        p_i[2] + (p_j[2] - p_i[2]) * xi,
                    ];
                    proj.project_offset(base3, ey, -val * m_amp)
                };
                let projected: Vec<_> = samples.iter().copied().map(project_sample).collect();
                if !p0.is_finite() || !p1.is_finite() || projected.iter().any(|pt| !pt.is_finite())
                {
                    continue;
                }
                for poly in diagram::diagram_fill_polygons(&samples, 1) {
                    let points: Vec<_> = poly.into_iter().map(project_sample).collect();
                    if points.iter().any(|pt| !pt.is_finite()) {
                        continue;
                    }
                    let origin = points[0];
                    let area_twice: f64 = points[1..]
                        .windows(2)
                        .map(|edge| {
                            let a = edge[0] - origin;
                            let b = edge[1] - origin;
                            a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64
                        })
                        .sum();
                    if area_twice == 0.0 || !area_twice.is_finite() {
                        continue;
                    }
                    painter.add(egui::Shape::convex_polygon(
                        points,
                        theme::translucent(theme::HILITE_PURPLE, fill_alpha),
                        egui::Stroke::NONE,
                    ));
                }
                let mut m_poly = Vec::with_capacity(samples.len() + 2);
                m_poly.push(p0);
                let mut last = p0;
                for pt in projected {
                    if (pt.x - last.x).hypot(pt.y - last.y) < diagram::MIN_SEGMENT_PX {
                        continue;
                    }
                    last = pt;
                    m_poly.push(pt);
                }
                m_poly.push(p1);
                painter.add(egui::Shape::line(
                    m_poly,
                    egui::Stroke::new(
                        1.5_f32,
                        theme::translucent(theme::HILITE_PURPLE, stroke_alpha),
                    ),
                ));
            }
            CmqComponent::Q => {
                let peak_px = (60.0 * trace_max_shear(t) / max_q.max(1e-12)) as f32;
                if peak_px < diagram::MIN_DIAGRAM_PX {
                    continue;
                }
                let samples: Vec<_> = cmq_m_sample_xis(&t.loads, t.l)
                    .into_iter()
                    .map(|xi| {
                        let force = t.section.at(xi).expect("CMQの有限な全長比");
                        let value = force[match t.plane {
                            DiagramPlane::Ey => 1,
                            DiagramPlane::Ez => 2,
                        }];
                        (xi, value)
                    })
                    .collect();
                let project_sample = |(xi, value): (f64, f64)| {
                    let base = std::array::from_fn(|d| p_i[d] + (p_j[d] - p_i[d]) * xi);
                    proj.project_offset(base, ey, value * q_amp)
                };
                let projected: Vec<_> = samples.iter().copied().map(project_sample).collect();
                if !p0.is_finite()
                    || !p1.is_finite()
                    || projected.iter().any(|point| !point.is_finite())
                {
                    continue;
                }
                for polygon in diagram::diagram_fill_polygons(&samples, 1) {
                    let points: Vec<_> = polygon.into_iter().map(project_sample).collect();
                    let origin = points[0];
                    let area_twice: f64 = points[1..]
                        .windows(2)
                        .map(|edge| {
                            let a = edge[0] - origin;
                            let b = edge[1] - origin;
                            a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64
                        })
                        .sum();
                    if points.iter().all(|point| point.is_finite())
                        && area_twice.is_finite()
                        && area_twice != 0.0
                    {
                        painter.add(egui::Shape::convex_polygon(
                            points,
                            theme::translucent(theme::GOOD_GREEN, fill_alpha),
                            egui::Stroke::NONE,
                        ));
                    }
                }
                let mut outline = vec![p0];
                outline.extend(projected);
                outline.push(p1);
                painter.add(egui::Shape::line(
                    outline,
                    egui::Stroke::new(1.5_f32, theme::translucent(theme::GOOD_GREEN, stroke_alpha)),
                ));
            }
        }
    }

    let axis_label = selected_axis_label(app.ui.view.cmq_axes);
    let both_axes_note = if app.ui.view.cmq_axes.ey && app.ui.view.cmq_axes.ez {
        "・弱軸は半透明"
    } else {
        ""
    };
    let legend = match app.ui.view.cmq_component {
        CmqComponent::C => format!(
            "解析梁CMQ C（節点変位0）(max={:.2}, {}{}) 青",
            max_c, axis_label, both_axes_note
        ),
        CmqComponent::M => format!(
            "解析梁CMQ M（荷重込み・節点変位0）(max={:.2}, {}{}) 紫",
            max_m, axis_label, both_axes_note
        ),
        CmqComponent::Q => format!(
            "解析梁CMQ Q（荷重込み・節点変位0）(max={:.2}, {}{}) 緑",
            max_q, axis_label, both_axes_note
        ),
    };
    painter.text(
        egui::pos2(
            painter.clip_rect().min.x + 10.0,
            painter.clip_rect().min.y + 10.0,
        ),
        egui::Align2::LEFT_TOP,
        legend,
        egui::FontId::proportional(14.0),
        theme::GRAY_700,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::ids::NodeId;
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LoadCaseKind, LocalAxis, MemberLoad,
        Node,
    };

    #[test]
    fn signed_cmq_projected_loads_close_both_simple_beam_ends() {
        let kind = MemberLoadKind::Distributed {
            a: 0.0,
            b: 6000.0,
            w1: 1.0,
            w2: -1.0,
        };
        let loads = [project_load(kind, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0])];
        assert_eq!(sum_simple_reactions(&loads, 6000.0), (1000.0, -1000.0));
        for (x, m) in [
            (0.0, 0.0),
            (1500.0, 562500.0),
            (4500.0, -562500.0),
            (6000.0, 0.0),
        ] {
            assert!(
                (sepika_load::floor::simple_beam_moment_at(&loads, 6000.0, x) - m).abs() < 1e-6
            );
        }
    }

    #[test]
    fn out_of_span_cmq_uses_original_interval_without_centroid_clamp() {
        let kind = MemberLoadKind::Distributed {
            a: 7000.0,
            b: 8000.0,
            w1: 1.0,
            w2: 1.0,
        };
        let loads = [project_load(kind, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0])];
        assert_eq!(sum_simple_reactions(&loads, 6000.0), (-250.0, 1250.0));
        for (x, m) in [
            (0.0, 0.0),
            (1500.0, -375000.0),
            (4500.0, -1125000.0),
            (6000.0, -1500000.0),
        ] {
            assert_eq!(
                sepika_load::floor::simple_beam_moment_at(&loads, 6000.0, x),
                m
            );
        }
    }

    fn mk_node(id: u32, x: f64, y: f64, z: f64) -> Node {
        Node {
            id: NodeId(id),
            coord: [x, y, z],
            restraint: Default::default(),
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    fn mk_beam(id: u32, i: u32, j: u32, ref_vector: [f64; 3]) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis { ref_vector },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }
    }

    fn mk_wall(id: u32, i: u32, j: u32) -> ElementData {
        ElementData {
            kind: ElementKind::Wall,
            ..mk_beam(id, i, j, [1.0, 0.0, 0.0])
        }
    }

    fn drawing_app(loads: Vec<MemberLoadKind>, reverse: bool, plane: DiagramPlane) -> App {
        let mut app = App::default();
        app.ui.view.cmq_component = CmqComponent::M;
        app.ui.view.cmq_axes = CmqAxes {
            ey: plane == DiagramPlane::Ey,
            ez: plane == DiagramPlane::Ez,
        };
        let ref_vec = if plane == DiagramPlane::Ey {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        app.core.model = Model {
            nodes: vec![mk_node(0, 0.0, 0.0, 0.0), mk_node(1, 1000.0, 0.0, 0.0)],
            elements: vec![mk_beam(0, u32::from(reverse), u32::from(!reverse), ref_vec)],
            load_cases: vec![LoadCase {
                origin: Default::default(),
                id: sepika_core::ids::LoadCaseId(0),
                name: "CMQ fixture".into(),
                kind: LoadCaseKind::Dead,
                nodal: Vec::new(),
                member: loads
                    .into_iter()
                    .map(|load| MemberLoad::manual(ElemId(0), [0.0, -1.0, 0.0], load))
                    .collect(),
            }],
            ..Default::default()
        };
        let mut section =
            sepika_core::model::Section::zero(sepika_core::ids::SectionId(0), "CMQ".into());
        section.area = 1000.0;
        section.iy = 1e7;
        section.iz = 1e7;
        section.j = 1e6;
        section.as_y = 800.0;
        section.as_z = 800.0;
        section.material = Some(sepika_core::ids::MaterialId(0));
        app.core.model.sections.push(section);
        app.core.model.materials.push(sepika_core::model::Material {
            id: sepika_core::ids::MaterialId(0),
            name: "CMQ".into(),
            category: sepika_core::model::MaterialCategory::Steel,
            young: 205000.0,
            poisson: 0.3,
            density: 0.0,
            shear: None,
            fc: None,
            fy: None,
            concrete_class: Default::default(),
            strength_factor: None,
        });
        app.core.model.elements[0].section = Some(sepika_core::ids::SectionId(0));
        app
    }

    fn draw_paths(app: &App, scale: f32) -> Vec<egui::epaint::PathShape> {
        let coords: Vec<_> = app.core.model.nodes.iter().map(|node| node.coord).collect();
        draw_paths_from_coords(app, scale, &coords)
    }

    fn draw_paths_from_coords(
        app: &App,
        scale: f32,
        coords: &[[f64; 3]],
    ) -> Vec<egui::epaint::PathShape> {
        let ctx = egui::Context::default();
        let camera = super::super::CameraState {
            rot: [1.0, 0.0, 0.0, 0.0],
            ..Default::default()
        };
        let proj = Projector::new([0.0; 3], &camera, scale, [100.0, 100.0]);
        ctx.run_ui(egui::RawInput::default(), |ui| {
            draw_cmq_diagram(
                &ui.ctx().layer_painter(egui::LayerId::background()),
                app,
                coords,
                &proj,
                FrameFilter::default(),
                None,
            );
        })
        .shapes
        .into_iter()
        .filter_map(|shape| match shape.shape {
            egui::Shape::Path(path) => Some(path),
            _ => None,
        })
        .collect()
    }

    fn assert_fill_inside_sampled_outline(paths: &[egui::epaint::PathShape]) {
        let outlines: Vec<_> = paths.iter().filter(|path| !path.closed).collect();
        assert_eq!(outlines.len(), 1);
        let outline = outlines[0];
        let mut filled_area = 0.0_f64;
        for fill in paths.iter().filter(|path| path.closed) {
            assert_eq!(fill.stroke, egui::epaint::PathStroke::NONE);
            assert!(fill.points.iter().all(|p| p.is_finite()));
            let mut signed_sides = Vec::new();
            for edge in fill
                .points
                .iter()
                .zip(fill.points.iter().cycle().skip(1))
                .take(fill.points.len())
            {
                let a = edge.0;
                let b = edge.1;
                for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    let p = *a + (*b - *a) * fraction;
                    let ordinates: Vec<_> = outline
                        .points
                        .windows(2)
                        .filter_map(|segment| {
                            let [u, v] = [segment[0], segment[1]];
                            if (u.x - v.x).abs() < 1e-5
                                || p.x < u.x.min(v.x) - 1e-4
                                || p.x > u.x.max(v.x) + 1e-4
                            {
                                return None;
                            }
                            Some(u.y + (v.y - u.y) * (p.x - u.x) / (v.x - u.x))
                        })
                        .collect();
                    assert!(!ordinates.is_empty(), "塗りの点は輪郭の材軸区間内");
                    let low = ordinates.iter().copied().fold(100.0_f32, f32::min);
                    let high = ordinates.iter().copied().fold(100.0_f32, f32::max);
                    assert!(
                        p.y >= low - 2e-4 && p.y <= high + 2e-4,
                        "輪郭外の塗り: {p:?}, range={low}..{high}"
                    );
                }
                if (a.y - 100.0).abs() > 1e-4 {
                    signed_sides.push(a.y > 100.0);
                }
            }
            assert!(signed_sides.iter().all(|side| *side == signed_sides[0]));
            let area = fill
                .points
                .iter()
                .zip(fill.points.iter().cycle().skip(1))
                .take(fill.points.len())
                .map(|(a, b)| a.x as f64 * b.y as f64 - a.y as f64 * b.x as f64)
                .sum::<f64>()
                .abs()
                / 2.0;
            assert!(area > 0.0);
            filled_area += area;
        }
        let curve_area = outline
            .points
            .windows(2)
            .map(|edge| {
                let a = (edge[0].y - 100.0).abs() as f64;
                let b = (edge[1].y - 100.0).abs() as f64;
                let dx = (edge[1].x - edge[0].x).abs() as f64;
                if (edge[0].y - 100.0) * (edge[1].y - 100.0) < 0.0 {
                    dx * (a * a + b * b) / (2.0 * (a + b))
                } else {
                    dx * (a + b) / 2.0
                }
            })
            .sum::<f64>();
        assert!(
            (filled_area - curve_area).abs() < 0.01,
            "fill={filled_area} outline={curve_area}"
        );
    }

    #[test]
    fn cmq_m_drawing_keeps_fill_inside_positive_signed_nonconvex_and_reversed_outlines() {
        let udl = MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: 0.01,
            w2: 0.01,
        };
        let signed = MemberLoadKind::Distributed {
            a: 0.0,
            b: 1000.0,
            w1: 0.01,
            w2: -0.01,
        };
        let fixtures = [
            vec![udl.clone()],
            vec![signed],
            vec![udl, MemberLoadKind::Point { a: 370.0, p: -10.0 }],
            vec![
                MemberLoadKind::Point { a: 200.0, p: 10.0 },
                MemberLoadKind::Point { a: 400.0, p: -20.0 },
                MemberLoadKind::Point { a: 600.0, p: 20.0 },
                MemberLoadKind::Point { a: 800.0, p: -10.0 },
            ],
        ];
        for loads in fixtures {
            for reverse in [false, true] {
                for plane in [DiagramPlane::Ey, DiagramPlane::Ez] {
                    let app = drawing_app(loads.clone(), reverse, plane);
                    let paths = draw_paths(&app, 0.5);
                    assert!(paths.iter().filter(|path| path.closed).count() >= 32);
                    assert_fill_inside_sampled_outline(&paths);
                    let alpha = if plane == DiagramPlane::Ey {
                        (60, 255)
                    } else {
                        (30, 128)
                    };
                    for path in &paths {
                        if path.closed {
                            assert_eq!(
                                path.fill,
                                theme::translucent(theme::HILITE_PURPLE, alpha.0)
                            );
                        } else {
                            assert_eq!(
                                path.stroke.color,
                                egui::epaint::ColorMode::Solid(theme::translucent(
                                    theme::HILITE_PURPLE,
                                    alpha.1
                                ))
                            );
                            assert_eq!(path.stroke.width, 1.5);
                        }
                    }
                    if loads
                        .iter()
                        .any(|load| matches!(load, MemberLoadKind::Point { a: 370.0, .. }))
                    {
                        let point_x = if reverse { 415.0 } else { 285.0 };
                        assert!(paths
                            .iter()
                            .find(|path| !path.closed)
                            .unwrap()
                            .points
                            .iter()
                            .any(|p| (p.x - point_x).abs() < 1e-4));
                    }
                    if loads.len() == 4 {
                        let outline = paths.iter().find(|path| !path.closed).unwrap();
                        for (x_mm, y_px) in
                            [(200.0, 120.0), (400.0, 40.0), (600.0, 160.0), (800.0, 80.0)]
                        {
                            let x_px = 100.0 + 0.5 * if reverse { 1000.0 - x_mm } else { x_mm };
                            assert!(outline
                                .points
                                .iter()
                                .any(|p| (p.x - x_px).abs() < 1e-4 && (p.y - y_px).abs() < 1e-4));
                        }
                        for x_px in [225.0, 350.0, 475.0] {
                            assert!(paths.iter().filter(|path| path.closed).any(|path| path
                                .points
                                .iter()
                                .any(|p| (p.x - x_px).abs() < 1e-4 && p.y == 100.0)));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cmq_m_drawing_preserves_closed_form_ordinates_and_single_outline() {
        let app = drawing_app(
            vec![MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 0.01,
                w2: 0.01,
            }],
            false,
            DiagramPlane::Ey,
        );
        let paths = draw_paths(&app, 0.5);
        let outline = paths.iter().find(|path| !path.closed).unwrap();
        let mut expected = vec![egui::pos2(100.0, 100.0)];
        for k in 0..=32 {
            let xi = k as f32 / 32.0;
            expected.push(egui::pos2(
                100.0 + 500.0 * xi,
                100.0 + 360.0 * xi * (1.0 - xi) - 60.0,
            ));
        }
        expected.push(egui::pos2(600.0, 100.0));
        assert_eq!(outline.points.len(), expected.len());
        for (actual, expected) in outline.points.iter().zip(expected) {
            assert!((*actual - expected).length() < 1e-4);
        }
        assert_eq!(paths.iter().filter(|path| path.closed).count(), 34);
    }

    #[test]
    fn cmq_m_drawing_rejects_zero_nonfinite_and_collapsed_fill() {
        let load = MemberLoadKind::Point { a: 370.0, p: 10.0 };
        for p in [0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let app = drawing_app(
                vec![MemberLoadKind::Point { a: 370.0, p }],
                false,
                DiagramPlane::Ey,
            );
            assert!(draw_paths(&app, 0.5).is_empty());
        }
        for a in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let app = drawing_app(
                vec![MemberLoadKind::Point { a, p: 10.0 }],
                false,
                DiagramPlane::Ey,
            );
            assert!(draw_paths(&app, 0.5).iter().all(|p| !p.closed));
        }
        let app = drawing_app(vec![load.clone()], false, DiagramPlane::Ey);
        for scale in [f32::NAN, f32::INFINITY, 0.0] {
            assert!(draw_paths(&app, scale).is_empty());
        }
        let mut zero_length = drawing_app(vec![load.clone()], false, DiagramPlane::Ey);
        zero_length.core.model.nodes[1].coord = [0.0; 3];
        assert!(draw_paths(&zero_length, 0.5).is_empty());
        let mut edge_on = drawing_app(vec![load], false, DiagramPlane::Ey);
        edge_on.core.model.elements[0].local_axis.ref_vector = [0.0, 0.0, 1.0];
        edge_on.core.model.load_cases[0].member[0].dir = [0.0, 0.0, -1.0];
        assert!(draw_paths(&edge_on, 0.5).iter().all(|p| !p.closed));
    }

    #[test]
    fn cmq_drawing_preserves_model_loads_end_moments_saved_results_and_undo_history() {
        let loads = vec![
            MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 0.01,
                w2: -0.01,
            },
            MemberLoadKind::Point { a: 370.0, p: -10.0 },
        ];
        let mut app = drawing_app(loads.clone(), false, DiagramPlane::Ey);
        for coord in [[2000.0, 0.0, 0.0], [3000.0, 0.0, 0.0]] {
            assert!(app.core.scoped.undo.run(
                &mut app.core.model,
                Box::new(sepika_edit::AddNode {
                    coord,
                    restraint: Default::default()
                })
            ));
        }
        app.core.scoped.undo.undo(&mut app.core.model);
        assert!(app.core.scoped.undo.can_undo());
        assert!(app.core.scoped.undo.can_redo());
        let mut result = serde_json::to_value(crate::app::ResultsBundle::default()).unwrap();
        result["input_records"] = serde_json::json!([["Modal", [5, 0, 0]]]);
        app.core.scoped.results = Some(serde_json::from_value(result).unwrap());
        app.core.scoped.results.as_mut().unwrap().member_forces = vec![(
            ElemId(0),
            sepika_element::frame::beam::MemberForces {
                at: vec![(0.0, [1.0; 6]), (1.0, [2.0; 6])],
            },
        )];
        let before_model = bincode::serialize(&app.core.model).unwrap();
        let before_results = bincode::serialize(&app.core.scoped.results).unwrap();
        let before_undo = (
            app.core.scoped.undo.revision(),
            app.core.scoped.undo.undo_label().map(str::to_owned),
            app.core.scoped.undo.redo_label().map(str::to_owned),
        );
        let before_c = sum_fixed_end_moments(&loads, 1000.0);
        let before_q = sum_simple_reactions(&loads, 1000.0);
        let before_m = sepika_load::floor::simple_beam_moment_at(&loads, 1000.0, 370.0);
        for (component, color) in [
            (CmqComponent::C, theme::DATA_BLUE),
            (CmqComponent::M, theme::HILITE_PURPLE),
            (CmqComponent::Q, theme::GOOD_GREEN),
        ] {
            app.ui.view.cmq_component = component;
            let paths = draw_paths(&app, 0.5);
            assert!(!paths.is_empty());
            assert_eq!(paths.iter().filter(|p| !p.closed).count(), 1);
            for path in &paths {
                if path.closed {
                    assert_eq!(path.fill, theme::translucent(color, 60));
                } else {
                    assert_eq!(path.stroke.color, egui::epaint::ColorMode::Solid(color));
                }
            }
        }
        assert_eq!(bincode::serialize(&app.core.model).unwrap(), before_model);
        assert_eq!(
            bincode::serialize(&app.core.scoped.results).unwrap(),
            before_results
        );
        assert_eq!(
            (
                app.core.scoped.undo.revision(),
                app.core.scoped.undo.undo_label().map(str::to_owned),
                app.core.scoped.undo.redo_label().map(str::to_owned)
            ),
            before_undo
        );
        assert_eq!(sum_fixed_end_moments(&loads, 1000.0), before_c);
        assert_eq!(sum_simple_reactions(&loads, 1000.0), before_q);
        assert_eq!(
            sepika_load::floor::simple_beam_moment_at(&loads, 1000.0, 370.0),
            before_m
        );
    }

    // `ElementKind` は柱・梁を区別しない（幾何で決まる。`sepika_core::frame::is_column`
    // 参照）ため、`is_primary_beam_for_cmq` が実際に除外するのは非 `Beam` 種別の要素
    // （壁・仕口パネル等）と実部材化小梁だけ。柱への集中荷重が CMQ 図に出ない理由は
    // 別にある: `LoadTransfer::Columns`（雑壁・取り付く壁版の柱伝達）は `NodalLoad`
    // を生成し、`LoadCase.member` に一切入らないため（`sepika_load::wall_attached`）。
    //
    /// 表示中ケースの `member` は、由来を問わず（床分配・自重・取り付く壁版・
    /// 手入力のいずれでも）大梁ごとに束ねられる。非 `Beam` 種別（壁等）への
    /// 部材荷重は対象外（`is_primary_beam_for_cmq`）。
    #[test]
    fn group_member_loads_by_elem_merges_any_source_and_excludes_non_beam_kind() {
        let model = Model {
            nodes: vec![mk_node(0, 0.0, 0.0, 0.0), mk_node(1, 4000.0, 0.0, 0.0)],
            elements: vec![mk_beam(0, 0, 1, [0.0, 0.0, 1.0]), mk_wall(1, 0, 1)],
            ..Default::default()
        };
        let case = LoadCase {
            origin: Default::default(),
            id: sepika_core::ids::LoadCaseId(0),
            name: "DL".into(),
            kind: LoadCaseKind::Dead,
            nodal: Vec::new(),
            member: vec![
                // 床分配由来（自動）
                MemberLoad::auto(
                    ElemId(0),
                    [0.0, 0.0, -1.0],
                    MemberLoadKind::Distributed {
                        a: 0.0,
                        b: 4000.0,
                        w1: 1.0,
                        w2: 1.0,
                    },
                ),
                // 手入力（取り付く壁版・梁自重相当）
                MemberLoad::manual(
                    ElemId(0),
                    [0.0, 0.0, -1.0],
                    MemberLoadKind::Point {
                        a: 2000.0,
                        p: 100.0,
                    },
                ),
                // 壁（非 Beam 種別）への部材荷重は CMQ 対象外
                MemberLoad::auto(
                    ElemId(1),
                    [0.0, 0.0, -1.0],
                    MemberLoadKind::Distributed {
                        a: 0.0,
                        b: 3000.0,
                        w1: 1.0,
                        w2: 1.0,
                    },
                ),
            ],
        };

        let groups = group_member_loads_by_elem(&model, &case);
        assert_eq!(groups.len(), 1, "壁(ElemId(1))はグループに現れないはず");
        assert_eq!(groups[0].elem, ElemId(0));
        assert_eq!(
            groups[0].loads.len(),
            2,
            "床分配・手入力の両方が同じ大梁のグループへ束ねられるはず"
        );
    }

    /// 局所軸へ投影すると、ey（強軸）成分とez（弱軸）成分の二乗和が
    /// 元の大きさの二乗と一致する（ey・ez は直交単位ベクトルのため）。
    #[test]
    fn project_load_splits_into_orthogonal_components() {
        let dir = [0.6, 0.0, -0.8]; // 正規化済み（斜め方向）
        let ey = [0.0, 0.0, -1.0];
        let ez = [1.0, 0.0, 0.0];
        let kind = MemberLoadKind::Point { a: 100.0, p: 10.0 };

        let proj_ey = project_load(kind.clone(), dir, ey);
        let proj_ez = project_load(kind, dir, ez);
        let (MemberLoadKind::Point { p: p_ey, .. }, MemberLoadKind::Point { p: p_ez, .. }) =
            (proj_ey, proj_ez)
        else {
            unreachable!()
        };
        assert!(
            (p_ey * p_ey + p_ez * p_ez - 10.0 * 10.0).abs() < 1e-9,
            "ey成分・ez成分の二乗和は元の大きさの二乗と一致するはず: p_ey={p_ey} p_ez={p_ez}"
        );
        // dir はほぼ ey 向き（0.8）で ez 成分（0.6）より大きい
        assert!(p_ey.abs() > p_ez.abs());
    }

    /// 荷重方向が局所軸と直交するとき、その面への投影はゼロになる。
    #[test]
    fn project_load_zero_when_orthogonal() {
        let dir = [0.0, 1.0, 0.0];
        let ey = [0.0, 0.0, -1.0];
        let kind = MemberLoadKind::Distributed {
            a: 0.0,
            b: 1.0,
            w1: 5.0,
            w2: 5.0,
        };
        let MemberLoadKind::Distributed { w1, w2, .. } = project_load(kind, dir, ey) else {
            unreachable!()
        };
        assert!(w1.abs() < 1e-12 && w2.abs() < 1e-12);
    }

    /// `dir` が単位ベクトルでなくても、正規化してから投影するため結果は同じになる
    /// （`MemberLoad::manual`/`auto` は `dir` を正規化しないため、`project_load` 側で
    /// 正規化を保証する必要がある。実際の解析側 `resolve` と同じ流儀）。
    #[test]
    fn project_load_normalizes_non_unit_dir() {
        let ey = [0.0, 0.0, 1.0];
        let kind = MemberLoadKind::Point { a: 0.0, p: 10.0 };
        let unit = project_load(kind.clone(), [0.0, 0.0, -1.0], ey);
        let scaled = project_load(kind, [0.0, 0.0, -3.5], ey); // 長さ3.5・向きは同じ
        let (MemberLoadKind::Point { p: p_unit, .. }, MemberLoadKind::Point { p: p_scaled, .. }) =
            (unit, scaled)
        else {
            unreachable!()
        };
        assert!(
            (p_unit - p_scaled).abs() < 1e-9,
            "正規化前の長さに関わらず同じ投影結果になるはず: p_unit={p_unit} p_scaled={p_scaled}"
        );
    }

    /// `dir` が退化（長さほぼ0）のときは、どの面にも投影できないため寄与0とする
    /// （パニック・NaN を出さない）。
    #[test]
    fn project_load_degenerate_dir_contributes_zero() {
        let ey = [0.0, 0.0, 1.0];
        let kind = MemberLoadKind::Point { a: 0.0, p: 10.0 };
        let MemberLoadKind::Point { p, .. } = project_load(kind, [0.0, 0.0, 0.0], ey) else {
            unreachable!()
        };
        assert_eq!(p, 0.0);
    }

    /// **回帰テスト**: 既定ケース（強軸のみ表示・標準的な水平梁・鉛直荷重）で
    /// 投影後の符号・大きさが保たれること。
    ///
    /// この既定ケースで `dot(dir,ey) = -1` となるため、素朴な `dot` で投影すると
    /// 全ての梁の C/M/Q が符号反転する（サギング/ホギングが入れ替わり、逆側に
    /// 描かれる）。本テストは基準（`s=+1`）を既定ケースで再現し続けることを固定する。
    #[test]
    fn project_load_preserves_legacy_sign_for_default_horizontal_beam_under_gravity() {
        let ey = [0.0, 0.0, 1.0]; // 標準的な水平梁（ref_vector=[0,0,1]）の局所 ey
        let dir = [0.0, 0.0, -1.0]; // 鉛直荷重（DIR_DOWN）
        let kind = MemberLoadKind::Distributed {
            a: 0.0,
            b: 4000.0,
            w1: 1.0,
            w2: 1.0,
        };
        let MemberLoadKind::Distributed { w1, w2, .. } = project_load(kind, dir, ey) else {
            unreachable!()
        };
        assert!(
            (w1 - 1.0).abs() < 1e-12 && (w2 - 1.0).abs() < 1e-12,
            "既定ケースでは投影後も w の符号・大きさが変わらないはず（在来の CMQ 図と\
             一致させるため）: w1={w1} w2={w2}"
        );
    }

    /// 準備計算で `member` が埋まる自動ケース名（DL・LL(架構用)・LL(地震用)）だけを
    /// `is_gravity_auto_case_name` が拾うこと。EX・EY は水平力が常に `NodalLoad`
    /// のみで `member` を持たないため対象外、手入力ケース名も対象外。
    #[test]
    fn is_gravity_auto_case_name_matches_only_member_bearing_auto_cases() {
        for name in [DL_CASE_NAME, LL_FRAME_CASE_NAME, LL_SEISMIC_CASE_NAME] {
            assert!(is_gravity_auto_case_name(name), "{name} は自動ケースのはず");
        }
        assert!(
            !is_gravity_auto_case_name(sepika_core::model::EX_CASE_NAME),
            "EXは常にmemberが空のため対象外のはず"
        );
        assert!(
            !is_gravity_auto_case_name(sepika_core::model::EY_CASE_NAME),
            "EYは常にmemberが空のため対象外のはず"
        );
        assert!(!is_gravity_auto_case_name("手入力ケース1"));
        assert!(!is_gravity_auto_case_name(""));
    }

    /// `selected_axis_label` は選択中の軸の組をそのまま表す（凡例・空表示メッセージ
    /// 双方が「今まさに選択している軸」を指すこと。片方だけ選択中に「その軸を
    /// 追加してください」という矛盾した案内を出さないための前提となる）。
    #[test]
    fn selected_axis_label_reflects_current_selection() {
        assert_eq!(
            selected_axis_label(CmqAxes {
                ey: true,
                ez: false
            }),
            "強軸(ey)"
        );
        assert_eq!(
            selected_axis_label(CmqAxes {
                ey: false,
                ez: true
            }),
            "弱軸(ez)"
        );
        assert_eq!(
            selected_axis_label(CmqAxes { ey: true, ez: true }),
            "強軸+弱軸"
        );
    }

    /// 弱軸(ez)は強軸(ey)より不透明度が低い（2D構面表示で強軸・弱軸のトレースが
    /// 同一直線上に重なっても判別できるようにするため。`plane_alpha_scale`）。
    #[test]
    fn plane_alpha_scale_dims_weak_axis_relative_to_strong_axis() {
        let ey_alpha = plane_alpha_scale(DiagramPlane::Ey);
        let ez_alpha = plane_alpha_scale(DiagramPlane::Ez);
        assert_eq!(ey_alpha, 1.0, "強軸は不透明のはず");
        assert!(
            ez_alpha > 0.0 && ez_alpha < ey_alpha,
            "弱軸は強軸より薄いが完全透明ではないはず: ez_alpha={ez_alpha}"
        );
    }
    #[test]
    fn building_rigid_zone_setting_changes_all_rendered_cmq_components() {
        let mut app = drawing_app(
            vec![MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 2.0,
                w2: 2.0,
            }],
            false,
            DiagramPlane::Ey,
        );
        app.core.model.elements[0].rigid_zone.length_i = 200.0;
        app.core.model.elements[0].rigid_zone.length_j = 100.0;
        for component in [CmqComponent::C, CmqComponent::M, CmqComponent::Q] {
            app.ui.view.cmq_component = component;
            app.core.model.stress_cfg.consider_rigid_zones = true;
            let on = draw_paths(&app, 0.5);
            app.core.model.stress_cfg.consider_rigid_zones = false;
            let off = draw_paths(&app, 0.5);
            assert!(!on.is_empty() && !off.is_empty());
            assert_ne!(on, off, "{component:?}");
        }
        let loads = [MemberLoadKind::Point {
            a: 500.0,
            p: 1000.0,
        }];
        let xis = cmq_m_sample_xis(&loads, 1000.0);
        assert!(xis.contains(&0.5));
        assert!(xis.iter().any(|&xi| xi > 0.5 && xi < 0.500000000001));
    }
    #[test]
    fn cmq_q_fill_stays_inside_signed_curve_and_point_load_jump() {
        for loads in [
            vec![MemberLoadKind::Distributed {
                a: 0.0,
                b: 1000.0,
                w1: 2.0,
                w2: 2.0,
            }],
            vec![MemberLoadKind::Distributed {
                a: 100.0,
                b: 800.0,
                w1: 1.0,
                w2: 3.0,
            }],
            vec![MemberLoadKind::Point {
                a: 370.0,
                p: 1000.0,
            }],
        ] {
            for reverse in [false, true] {
                let mut app = drawing_app(loads.clone(), reverse, DiagramPlane::Ey);
                app.ui.view.cmq_component = CmqComponent::Q;
                app.core.model.elements[0].rigid_zone.length_i = 200.0;
                app.core.model.elements[0].rigid_zone.length_j = 100.0;
                for enabled in [true, false] {
                    app.core.model.stress_cfg.consider_rigid_zones = enabled;
                    let paths = draw_paths(&app, 0.5);
                    assert_fill_inside_sampled_outline(&paths);
                    if matches!(loads[0], MemberLoadKind::Point { .. }) {
                        let outline = paths.iter().find(|path| !path.closed).unwrap();
                        let x = if reverse { 415.0 } else { 285.0 };
                        let at_point: Vec<_> = outline
                            .points
                            .iter()
                            .filter(|point| (point.x - x).abs() < 1e-4)
                            .collect();
                        assert_eq!(at_point.len(), 2);
                        assert!((at_point[0].y - at_point[1].y).abs() > 1.0);
                    }
                }
            }
        }
    }
    #[test]
    fn eccentric_cmq_uses_actual_axis_for_point_jump_length_and_local_components() {
        for plane in [DiagramPlane::Ey, DiagramPlane::Ez] {
            let mut app = drawing_app(
                vec![MemberLoadKind::Point {
                    a: 370.0,
                    p: 1000.0,
                }],
                false,
                plane,
            );
            app.ui.view.cmq_component = CmqComponent::Q;
            app.core.model.sections[0].frame_use =
                Some(sepika_core::model::FrameSectionUse::Girder);
            app.core.model.elements[0].vertical_eccentricity = Some(200.0);
            app.core.model.elements[0].local_axis.ref_vector = [0.0, 1.0, 0.0];
            app.core.model.nodes[1].coord = [1000.0, 0.0, 300.0];
            app.core.model.load_cases[0].member[0].dir = [0.0, -1.0, -1.0];
            let expected = draw_paths(&app, 0.5);
            assert!(!expected.is_empty());
            let display_coords = [[0.0, 0.0, 0.0], [900.0, 100.0, 0.0]];
            assert_eq!(draw_paths_from_coords(&app, 0.5, &display_coords), expected);
            if plane == DiagramPlane::Ey {
                let outline = expected.iter().find(|path| !path.closed).unwrap();
                let point_x = 100.0 + 500.0 * 370.0 / 1090000.0_f64.sqrt();
                let jump: Vec<_> = outline
                    .points
                    .iter()
                    .filter(|point| (point.x as f64 - point_x).abs() < 1e-4)
                    .collect();
                assert_eq!(jump.len(), 2);
                assert!((jump[0].y - jump[1].y).abs() > 1.0);
            }
        }
    }
}
