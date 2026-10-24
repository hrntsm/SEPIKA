//! 部材描画・スラブ/壁・グリッド・軸ガジェット。
//!
//! `viewer` ハブからの構造分割。アルゴリズム変更は行わない。

use crate::app::App;
use crate::theme;

use super::{
    camera::q_rotate, pick::RegionPick, support::draw_arrow, CameraState, DiagramPlane,
    FrameFilter, Projector,
};

pub(super) fn diagram_offset_dir(
    p_i: [f64; 3],
    p_j: [f64; 3],
    ref_vector: [f64; 3],
    plane: DiagramPlane,
) -> Option<[f64; 3]> {
    let frame = sepika_element::transform::LocalFrame::from_nodes(p_i, p_j, ref_vector).ok()?;
    Some(match plane {
        DiagramPlane::Ey => frame.rot[1],
        DiagramPlane::Ez => frame.rot[2],
    })
}

/// 応力図の張り出し方向を構面内へ倒す（2D 構面表示）。
///
/// 3D では成分ごとに部材の局所 ey / ez 面へ張り出すが、構面を正対で見ると
/// **面外へ張り出す成分は視線方向に潰れて線になり、何も読めなくなる**
/// （たとえば局所 ez が構面の法線と平行な梁の My 図）。そこで構面表示では、
/// 張り出し方向を「材軸に直交し、かつ構面に含まれる向き」へ倒す。値と図形は
/// 変えず向きだけを回すため、読み取れる数値は 3D と同じである。どの成分の図かは
/// 成分ごとの固定色と凡例・数値ラベルで判別する。
///
/// 倒した先の向きは `normal × 材軸`。符号は元の張り出し方向に合わせ、元の方向が
/// 構面に垂直（成分が完全に面外）で符号を決められない場合は正側へ倒す。
pub(super) fn in_plane_offset_dir(
    dir: [f64; 3],
    p_i: [f64; 3],
    p_j: [f64; 3],
    normal: [f64; 3],
) -> [f64; 3] {
    let axis = [p_j[0] - p_i[0], p_j[1] - p_i[1], p_j[2] - p_i[2]];
    let t = cross3(normal, axis);
    let len = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
    if len < 1e-12 {
        return dir;
    }
    let t = [t[0] / len, t[1] / len, t[2] / len];
    let sign = dir[0] * t[0] + dir[1] * t[1] + dir[2] * t[2];
    if sign < 0.0 {
        [-t[0], -t[1], -t[2]]
    } else {
        t
    }
}

/// 材軸を持つ線材か（先頭 2 節点を結ぶ線分＝部材線として描いてよい要素か）。
///
/// 壁・シェルは面要素で材軸を持たない（壁は多角形として別に描く）。仕口パネルは
/// 「接合部の節点 ＋ 取り付く部材の他端」を節点列に持つ接合部要素であり、先頭
/// 2 節点は取り付く柱・梁そのものと同じ節点対になる（`pick_nearest_member` が
/// ピック対象から外しているのと同じ理由）。
pub(super) fn draws_as_line(kind: sepika_core::model::ElementKind) -> bool {
    element_draw_shape(kind) == DrawShape::Line
}

/// 3D ビューでの要素の描き方。部材線・選択ハイライトとも本区分に従う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DrawShape {
    /// 材軸の線分（先頭 2 節点を結ぶ）。
    Line,
    /// 節点列の多角形（面要素）。
    Polygon,
    /// 描かない。
    None,
}

/// 要素種別ごとの描き方。部材線（[`draws_as_line`]）・面要素のポリゴン・選択
/// ハイライトが同じ規約を共有するための単一情報源。
///
/// 壁・シェルは面要素で材軸を持たないため多角形で描く。仕口パネルは「接合部の
/// 節点 ＋ 取り付く部材の他端」を節点列に持つ接合部要素で、材軸も輪郭も持たない
/// ため描かない（`pick_nearest_member` がピック対象から外しているのと同じ理由）。
/// 先頭 2 節点を結ぶと取り付く柱・梁とまったく同じ線分になるため、線で描くと
/// 内部たわみ表示で部材が二重に見え、選択ハイライトでは選択していない柱・梁が
/// 選択されているように見えてしまう。
///
/// 要素種別を追加したときに描き方を決め忘れないよう、網羅 `match` で書く。
pub(super) fn element_draw_shape(kind: sepika_core::model::ElementKind) -> DrawShape {
    use sepika_core::model::ElementKind as K;
    match kind {
        K::Wall | K::Shell => DrawShape::Polygon,
        K::PanelZone => DrawShape::None,
        K::Beam
        | K::Fiber
        | K::MultiSpring
        | K::Brace { .. }
        | K::NodalSpring
        | K::Isolator
        | K::Damper => DrawShape::Line,
    }
}

use sepika_core::geom::vec3::cross as cross3;

/// 荷重分配オブジェクト（床板・要素にならない壁版）の破線パターン（描画長 / 間隔, px）。
const PLATE_DASH: f32 = 6.0;
const PLATE_GAP: f32 = 4.0;
/// 同・塗りの不透明度。実部材（壁エレメントは 50）より薄くして手前に出さない。
const PLATE_FILL_ALPHA: u8 = 28;
/// 同・輪郭の不透明度。
const PLATE_STROKE_ALPHA: u8 = 220;

/// 要素にならない壁版の多角形（淡い半透明フィル＋破線の輪郭）を 1 枚描く。
///
/// 床板と要素にならない壁版で書式を共有する。**線種が「解析要素かどうか」を、
/// 色が「床か壁か」を表す**規約であり、実部材（実線・濃い塗り）と弁別できる。
fn draw_load_plate_polygon(
    painter: &egui::Painter,
    coords: &[[f64; 3]],
    proj: &Projector,
    color: egui::Color32,
    fill: bool,
) {
    let poly: Vec<egui::Pos2> = coords.iter().copied().map(|c| proj.project(c)).collect();
    if poly.len() < 3 {
        return;
    }
    if fill {
        painter.add(egui::Shape::convex_polygon(
            poly.clone(),
            theme::translucent(color, PLATE_FILL_ALPHA),
            egui::Stroke::NONE,
        ));
    }
    let mut closed = poly.clone();
    closed.push(poly[0]);
    painter.extend(egui::Shape::dashed_line(
        &closed,
        egui::Stroke::new(1.5_f32, theme::translucent(color, PLATE_STROKE_ALPHA)),
        PLATE_DASH,
        PLATE_GAP,
    ));
}

fn slab_fill_triangles(
    model: &sepika_core::Model,
    slab: &sepika_core::model::Slab,
) -> Option<Vec<[usize; 3]>> {
    use sepika_core::geom::polygon;
    model.validate_attached_slab_geometry(slab).ok()?;
    let boundary = slab.boundary_coords(model)?;
    if !boundary.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let origin = *boundary.first()?;
    let xy: Vec<_> = boundary
        .iter()
        .map(|p| [p[0] - origin[0], p[1] - origin[1]])
        .collect();
    let (lo, hi) = polygon::bounding_box(&xy);
    let scale_mm = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    let area_mm2 = polygon::area(&xy);
    if !area_mm2.is_finite() || area_mm2 <= scale_mm * scale_mm * polygon::DEGENERATE_AREA_REL {
        return None;
    }
    polygon::triangulate_simple(&xy).ok()
}

fn draw_slab_polygon(
    painter: &egui::Painter,
    coords: &[[f64; 3]],
    proj: &Projector,
    triangles: Option<&[[usize; 3]]>,
) {
    draw_triangulated_plate_polygon(
        painter,
        coords,
        proj,
        triangles,
        theme::translucent(theme::BEST_YELLOW, PLATE_FILL_ALPHA),
        egui::Stroke::new(
            1.5_f32,
            theme::translucent(theme::BEST_YELLOW, PLATE_STROKE_ALPHA),
        ),
    );
}

fn draw_triangulated_plate_polygon(
    painter: &egui::Painter,
    coords: &[[f64; 3]],
    proj: &Projector,
    triangles: Option<&[[usize; 3]]>,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    let poly: Vec<_> = coords
        .iter()
        .map(|p| {
            p.iter()
                .all(|v| v.is_finite())
                .then(|| proj.project(*p))
                .filter(|p| p.is_finite())
        })
        .collect();
    if poly.len() < 2 {
        return;
    }
    if let Some(triangles) = triangles.filter(|triangles| {
        poly.iter().all(Option::is_some) && triangles.iter().flatten().all(|&i| i < poly.len())
    }) {
        let mut mesh = egui::Mesh::default();
        for p in &poly {
            mesh.colored_vertex(p.unwrap(), fill);
        }
        for &[a, b, c] in triangles {
            mesh.add_triangle(a as u32, b as u32, c as u32);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    if let Some(mut closed) = poly.iter().copied().collect::<Option<Vec<_>>>() {
        closed.push(closed[0]);
        painter.extend(egui::Shape::dashed_line(
            &closed, stroke, PLATE_DASH, PLATE_GAP,
        ));
    } else {
        for i in 0..poly.len() {
            if let (Some(a), Some(b)) = (poly[i], poly[(i + 1) % poly.len()]) {
                painter.extend(egui::Shape::dashed_line(
                    &[a, b],
                    stroke,
                    PLATE_DASH,
                    PLATE_GAP,
                ));
            }
        }
    }
}

/// 床板（版）の輪郭・塗り。大梁または小梁で囲まれた床板と、取り付く床板の両方を描く。
/// 二次部材（小梁・間柱）は実部材と同じ経路（`draw_mode_rest_ghost` の二次部材
/// ブロック）で別途描画する。
///
/// `coords3` は節点インデックス順の**表示用 3D 座標**で、変形図・モード形・時刻歴では
/// 変形後の位置が入る。周囲の梁が変形するのに床板だけが元の位置に残ると、床が梁から
/// 浮いて見えて変形の読み取りを妨げるため、床板も同じ座標へ載せる。
pub(super) fn draw_slabs(
    painter: &egui::Painter,
    app: &App,
    filter: FrameFilter,
    proj: &Projector,
    coords3: &[[f64; 3]],
) {
    for slab in &app.core.model.slabs {
        if !slab_visible_on_frame(&app.core.model, slab, filter) {
            continue;
        }
        let Some(coords) =
            slab.boundary_coords_with(&app.core.model, |n| coords3.get(n.index()).copied())
        else {
            continue;
        };
        let triangles = slab_fill_triangles(&app.core.model, slab);
        draw_slab_polygon(painter, &coords, proj, triangles.as_deref());
    }
    if let Ok(perimeters) = sepika_core::model::perimeter_slabs(&app.core.model) {
        for perimeter in perimeters {
            if !perimeter_visible_on_frame(&perimeter, filter) {
                continue;
            }
            let Some(boundary) =
                perimeter_boundary_with_coords(&app.core.model, &perimeter, coords3)
            else {
                continue;
            };
            if !boundary.iter().flatten().all(|v| v.is_finite()) {
                continue;
            }
            let coords = boundary.map(|p| proj.project(p));
            if !coords.iter().all(|p| p.is_finite()) {
                continue;
            }
            let mut closed = coords.to_vec();
            closed.push(closed[0]);
            painter.add(egui::Shape::convex_polygon(
                closed.clone(),
                theme::translucent(theme::DATA_BLUE, 45),
                egui::Stroke::new(1.5_f32, theme::DATA_BLUE),
            ));
            painter.add(egui::Shape::line(
                closed,
                egui::Stroke::new(1.5_f32, theme::DATA_BLUE),
            ));
        }
    }
}

fn perimeter_visible_on_frame(
    perimeter: &sepika_core::model::PerimeterSlab,
    filter: FrameFilter,
) -> bool {
    filter.shows(perimeter.beam)
}

fn perimeter_boundary_with_coords(
    model: &sepika_core::Model,
    perimeter: &sepika_core::model::PerimeterSlab,
    coords3: &[[f64; 3]],
) -> Option<[[f64; 3]; 4]> {
    let beam = model.element(perimeter.beam)?;
    let [n0, n1] = beam.nodes.as_slice() else {
        return None;
    };
    let p0 = *coords3.get(n0.index())?;
    let p1 = *coords3.get(n1.index())?;
    let d0 = [
        p0[0] - perimeter.boundary[0][0],
        p0[1] - perimeter.boundary[0][1],
        p0[2] - perimeter.boundary[0][2],
    ];
    let d1 = [
        p1[0] - perimeter.boundary[1][0],
        p1[1] - perimeter.boundary[1][1],
        p1[2] - perimeter.boundary[1][2],
    ];
    Some([
        p0,
        p1,
        [
            perimeter.boundary[2][0] + d1[0],
            perimeter.boundary[2][1] + d1[1],
            perimeter.boundary[2][2] + d1[2],
        ],
        [
            perimeter.boundary[3][0] + d0[0],
            perimeter.boundary[3][1] + d0[1],
            perimeter.boundary[3][2] + d0[2],
        ],
    ])
}

/// 要素にならない壁版の輪郭・塗り。
///
/// 壁版は入力なので、解析要素になるか否かにかかわらず図に出す。要素になる壁版
/// （[`sepika_core::model::Model::wall_plate_becomes_element`]）は壁エレメントとして
/// 実線で描かれるため、ここでは描かない（同じ多角形の二重描画を避ける）。
///
/// 対象は要素にならない壁版すべてで、間柱で分割された壁版・5 節点以上の壁領域内の
/// 壁版・断面未割当の壁版に加え、取り付く壁版（腰壁・垂壁・パラペット・自立壁）も含む。
/// 取付き先が点（`RegionAnchor::Point`）の壁版は座標を組み立てられないため描けない
/// （壁の取付き先としては使わない組み合わせ。[`sepika_core::model::WallPlate`] 参照）。
///
/// `coords3` の意味は [`draw_slabs`] と同じ（変形図では変形後の座標が入る）。
pub(super) fn draw_wall_plates(
    painter: &egui::Painter,
    app: &App,
    filter: FrameFilter,
    proj: &Projector,
    coords3: &[[f64; 3]],
) {
    for plate in &app.core.model.wall_plates {
        if app.core.model.wall_plate_becomes_element(plate) {
            continue;
        }
        if !wall_plate_visible_on_frame(&app.core.model, plate, filter) {
            continue;
        }
        let Some(coords) =
            plate.boundary_coords_with(&app.core.model, |n| coords3.get(n.index()).copied())
        else {
            continue;
        };
        draw_load_plate_polygon(
            painter,
            &coords,
            proj,
            theme::DATA_BLUE,
            plate_fill_is_valid(&app.core.model, plate),
        );
    }
}

/// 割当領域の状態に対応する色。
///
/// 未設定＝警告色（版が無く荷重・剛性を過小評価し得る）、版なし＝灰色（明示的な
/// 選択）、版あり＝緑（割当済み）。
fn assignment_color<Id: Copy>(state: sepika_core::model::PlateAssignment<Id>) -> egui::Color32 {
    if state.is_unset() {
        theme::ERROR_RED
    } else if state.is_no_plate() {
        theme::GRAY_600
    } else {
        theme::GOOD_GREEN
    }
}

/// 割当領域 1 枚の多角形を描く。ホバー中は太い輪郭で強調する。
fn draw_assignment_region_polygon(
    painter: &egui::Painter,
    proj: &Projector,
    coords: &[[f64; 3]],
    color: egui::Color32,
    hovered: bool,
) {
    let poly: Vec<egui::Pos2> = coords.iter().map(|&c| proj.project(c)).collect();
    if poly.len() < 3 {
        return;
    }
    let fill_alpha = if hovered { 90 } else { 45 };
    painter.add(egui::Shape::convex_polygon(
        poly.clone(),
        theme::translucent(color, fill_alpha),
        egui::Stroke::NONE,
    ));
    let mut closed = poly;
    closed.push(closed[0]);
    let width = if hovered { 4.0_f32 } else { 2.0_f32 };
    painter.add(egui::Shape::line(closed, egui::Stroke::new(width, color)));
}

/// 3D ビューに床板・壁版の割当領域を描く（割当のクリック選択とホバー強調）。
///
/// `coords3` は使わず、割当領域の境界を支持部材の材軸から解決したモデル座標を
/// 投影する。描く種別は作成モードに合わせて呼び分ける。
pub(super) fn draw_assignment_regions(
    painter: &egui::Painter,
    app: &App,
    proj: &Projector,
    floor: bool,
    wall: bool,
    hover: Option<RegionPick>,
) {
    let model = &app.core.model;
    if floor {
        for region in &model.floor_assignment_regions.regions {
            let Some(coords) = model.floor_assignment_region_coords(region.id) else {
                continue;
            };
            draw_assignment_region_polygon(
                painter,
                proj,
                &coords,
                assignment_color(region.assignment),
                hover == Some(RegionPick::Floor(region.id)),
            );
        }
    }
    if wall {
        for region in &model.wall_assignment_regions.regions {
            let Some(coords) = model.wall_assignment_region_coords(region.id) else {
                continue;
            };
            draw_assignment_region_polygon(
                painter,
                proj,
                &coords,
                assignment_color(region.assignment),
                hover == Some(RegionPick::Wall(region.id)),
            );
        }
    }
}

/// 3D ビューに作業範囲（親の床領域・壁領域）を強調表示する。
pub(super) fn draw_work_scope(
    painter: &egui::Painter,
    app: &App,
    proj: &Projector,
    include_floor: bool,
    include_wall: bool,
) {
    use crate::app::WorkScope;
    let model = &app.core.model;
    let scope = app.ui.scoped.work_scope;
    if include_floor {
        for region in &model.floor_regions {
            let Some(coords) = region.boundary_coords(model) else {
                continue;
            };
            let selected = scope == Some(WorkScope::Floor(region.id));
            draw_work_scope_polygon(painter, proj, &coords, selected);
        }
    }
    if include_wall {
        for region in &model.wall_regions {
            let Some(coords) = region.boundary_coords(model) else {
                continue;
            };
            let selected = scope == Some(WorkScope::Wall(region.id));
            draw_work_scope_polygon(painter, proj, &coords, selected);
        }
    }
}

/// 作業範囲 1 枚の多角形。選択中は太い輪郭と濃い塗りで強調する。
fn draw_work_scope_polygon(
    painter: &egui::Painter,
    proj: &Projector,
    coords: &[[f64; 3]],
    selected: bool,
) {
    let poly: Vec<egui::Pos2> = coords.iter().map(|&c| proj.project(c)).collect();
    if poly.len() < 3 {
        return;
    }
    let color = theme::WARN_TEXT;
    let fill_alpha = if selected { 70 } else { 20 };
    painter.add(egui::Shape::convex_polygon(
        poly.clone(),
        theme::translucent(color, fill_alpha),
        egui::Stroke::NONE,
    ));
    let mut closed = poly;
    closed.push(closed[0]);
    let width = if selected { 4.0_f32 } else { 1.5_f32 };
    painter.add(egui::Shape::line(closed, egui::Stroke::new(width, color)));
}

/// 配置モードで 1 点目に選んだ支持部材アンカーを丸印で示す。
pub(super) fn draw_anchor_marker(
    painter: &egui::Painter,
    app: &App,
    proj: &Projector,
    anchor: sepika_core::model::SecondaryMemberAnchor,
) {
    if let Some(p) = super::displayed_anchor_point(
        &app.core.model,
        anchor,
        app.ui.view.view_mode == super::ViewMode::Shape,
    ) {
        let screen = proj.project(p);
        painter.circle_stroke(screen, 6.0, egui::Stroke::new(2.5_f32, theme::WARN_TEXT));
        painter.circle_filled(screen, 2.5_f32, theme::WARN_TEXT);
    }
}

/// 壁版の内部を塗ってよいか。
///
/// 取り付く壁版の立ち上がり高さが両端で符号反転すると、境界の 4 点が自己交差する
/// 蝶ネクタイ形になる（[`sepika_core::model::WallPlate::area`] がこの形を避けて
/// 面積を積分で求めているのと同じ理由）。塗りは凸多角形を前提に三角形へ分割する
/// ため、この形では輪郭からはみ出す。輪郭の破線だけを描いて、入力が異常である
/// ことをそのまま見せる。
pub(super) fn plate_fill_is_valid(
    model: &sepika_core::Model,
    plate: &sepika_core::model::WallPlate,
) -> bool {
    use sepika_core::model::WallPlateShape;
    match &plate.shape {
        WallPlateShape::Attached { .. } => model
            .wall_plate_extent(plate)
            .is_some_and(|e| e[0] * e[1] >= 0.0),
        WallPlateShape::Enclosed => true,
    }
}

/// 壁版が現在の構面表示に含まれるか（[`slab_visible_on_frame`] の壁版版）。
pub(super) fn wall_plate_visible_on_frame(
    model: &sepika_core::Model,
    plate: &sepika_core::model::WallPlate,
    filter: FrameFilter,
) -> bool {
    use sepika_core::model::{RegionAnchor, WallPlateShape};
    match &plate.shape {
        WallPlateShape::Enclosed => plate
            .boundary_nodes(model)
            .is_some_and(|boundary| boundary.iter().all(|n| filter.shows_node(n.index()))),
        WallPlateShape::Attached { anchor, .. } => match anchor {
            RegionAnchor::Line { nodes, .. } => nodes.iter().any(|n| filter.shows_node(n.index())),
            RegionAnchor::FloorRegion { nodes, .. } => {
                nodes.iter().any(|n| filter.shows_node(n.index()))
            }
            RegionAnchor::Point(_) => false,
        },
    }
}

fn slab_visible_on_frame(
    model: &sepika_core::model::Model,
    slab: &sepika_core::model::Slab,
    filter: FrameFilter,
) -> bool {
    use sepika_core::model::{RegionAnchor, SlabShape};
    match &slab.shape {
        SlabShape::Enclosed => slab
            .boundary_nodes(model)
            .is_some_and(|b| b.iter().all(|n| filter.shows_node(n.index()))),
        SlabShape::Attached { anchor, .. } => match anchor {
            RegionAnchor::Line { nodes, .. } => nodes.iter().any(|n| filter.shows_node(n.index())),
            RegionAnchor::Point(n) => filter.shows_node(n.index()),
            RegionAnchor::FloorRegion { .. } => false,
        },
    }
}

/// モード形の変形前架構（破線・高透過）。変形後の紫実線の下に描き、基準位置からの変化を読む。
/// 質点モードの変形前串と同じ破線（6 pt / 隙間 4 pt）とアルファ（線 90、塗り 55）。
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_mode_rest_ghost(
    painter: &egui::Painter,
    app: &App,
    model: &sepika_core::model::Model,
    proj: &Projector,
    deform_scale: f64,
    pts_rest: &[egui::Pos2],
    node_visible: &[bool],
    filter: FrameFilter,
    show_secondary: bool,
    show_sections: bool,
) {
    if !deform_scale.is_finite() || deform_scale.abs() <= 1e-12 {
        return;
    }
    const DASH: f32 = 6.0;
    const GAP: f32 = 4.0;
    const LINE_A: u8 = 90;
    const FILL_A: u8 = 55;
    let line = theme::translucent(theme::HILITE_PURPLE, LINE_A);
    let stroke_w = if show_sections { 1.0_f32 } else { 1.5_f32 };
    let stroke = egui::Stroke::new(stroke_w, line);

    for (i, &p) in pts_rest.iter().enumerate() {
        if !node_visible.get(i).copied().unwrap_or(false) {
            continue;
        }
        painter.circle_filled(p, 3.0, theme::translucent(theme::DATA_BLUE, FILL_A));
    }

    for elem in &model.elements {
        if !filter.shows(elem.id) {
            continue;
        }
        if element_draw_shape(elem.kind) == DrawShape::Polygon && elem.nodes.len() >= 3 {
            let poly: Vec<egui::Pos2> = elem
                .nodes
                .iter()
                .filter_map(|n| {
                    let idx = n.index();
                    (idx < pts_rest.len()).then(|| pts_rest[idx])
                })
                .collect();
            if poly.len() == elem.nodes.len() {
                painter.add(egui::Shape::convex_polygon(
                    poly.clone(),
                    theme::translucent(theme::DATA_BLUE, FILL_A),
                    egui::Stroke::NONE,
                ));
                let mut closed = poly;
                closed.push(closed[0]);
                painter.extend(egui::Shape::dashed_line(&closed, stroke, DASH, GAP));
            }
            continue;
        }
        if !draws_as_line(elem.kind) || elem.nodes.len() < 2 {
            continue;
        }
        let n0 = elem.nodes[0].index();
        let n1 = elem.nodes[1].index();
        if n0 >= pts_rest.len() || n1 >= pts_rest.len() {
            continue;
        }
        painter.extend(egui::Shape::dashed_line(
            &[pts_rest[n0], pts_rest[n1]],
            stroke,
            DASH,
            GAP,
        ));
    }

    if show_secondary {
        for slab in &app.core.model.slabs {
            if !slab_visible_on_frame(&app.core.model, slab, filter) {
                continue;
            }
            let Some(coords) = slab.boundary_coords(&app.core.model) else {
                continue;
            };
            let triangles = slab_fill_triangles(&app.core.model, slab);
            draw_triangulated_plate_polygon(
                painter,
                &coords,
                proj,
                triangles.as_deref(),
                theme::translucent(theme::BEST_YELLOW, FILL_A),
                egui::Stroke::new(stroke_w, theme::translucent(theme::BEST_YELLOW, LINE_A)),
            );
        }
        for plate in &app.core.model.wall_plates {
            if app.core.model.wall_plate_becomes_element(plate)
                || !wall_plate_visible_on_frame(&app.core.model, plate, filter)
            {
                continue;
            }
            let Some(coords) = plate.boundary_coords(&app.core.model) else {
                continue;
            };
            let poly: Option<Vec<_>> = coords
                .iter()
                .map(|p| {
                    p.iter()
                        .all(|v| v.is_finite())
                        .then(|| proj.project(*p))
                        .filter(|p| p.is_finite())
                })
                .collect();
            let Some(mut poly) = poly.filter(|p| p.len() >= 3) else {
                continue;
            };
            if plate_fill_is_valid(&app.core.model, plate) {
                painter.add(egui::Shape::convex_polygon(
                    poly.clone(),
                    theme::translucent(theme::DATA_BLUE, FILL_A),
                    egui::Stroke::NONE,
                ));
            }
            poly.push(poly[0]);
            painter.extend(egui::Shape::dashed_line(&poly, stroke, DASH, GAP));
        }
        let sec = egui::Stroke::new(
            if show_sections { 1.0_f32 } else { 1.5_f32 },
            theme::translucent(theme::SECONDARY_AMBER, LINE_A),
        );
        for sm in app.core.model.beams().chain(app.core.model.posts()) {
            let Some([n0, n1]) = super::secondary_end_node_indices(&app.core.model, sm) else {
                continue;
            };
            if !filter.shows_node(n0) || !filter.shows_node(n1) {
                continue;
            }
            if n0 < pts_rest.len() && n1 < pts_rest.len() {
                painter.extend(egui::Shape::dashed_line(
                    &[pts_rest[n0], pts_rest[n1]],
                    sec,
                    DASH,
                    GAP,
                ));
            }
        }
    }
}

pub(super) fn draw_grid_and_axes(painter: &egui::Painter, rect: egui::Rect, projector: &Projector) {
    let center3 = projector.center3();
    let scale = projector.scale();
    let proj = |p: [f64; 3]| projector.project(p);

    /// グリッド間隔 [mm]（1 m）。
    const STEP: f64 = 1000.0;
    let grid_stroke = egui::Stroke::new(0.5_f32, egui::Color32::from_black_alpha(36));
    let origin: [f64; 3] = [0.0; 3];

    let view_radius = (rect.width().hypot(rect.height()) / scale) as f64 * 0.75;

    let range = [
        (
            ((center3[0] - view_radius) / STEP).floor() * STEP,
            ((center3[0] + view_radius) / STEP).ceil() * STEP,
        ),
        (
            ((center3[1] - view_radius) / STEP).floor() * STEP,
            ((center3[1] + view_radius) / STEP).ceil() * STEP,
        ),
        (
            ((center3[2] - view_radius) / STEP).floor() * STEP,
            ((center3[2] + view_radius) / STEP).ceil() * STEP,
        ),
    ];

    let a = 0usize;
    let b = 1usize;
    let a_lo = (range[a].0 / STEP).round() as i64;
    let a_hi = (range[a].1 / STEP).round() as i64;
    for k in a_lo..=a_hi {
        let av = k as f64 * STEP;
        let p0 = [av, range[b].0, origin[2]];
        let p1 = [av, range[b].1, origin[2]];
        painter.line_segment([proj(p0), proj(p1)], grid_stroke);
    }
    let b_lo = (range[b].0 / STEP).round() as i64;
    let b_hi = (range[b].1 / STEP).round() as i64;
    for k in b_lo..=b_hi {
        let bv = k as f64 * STEP;
        let q0 = [range[a].0, bv, origin[2]];
        let q1 = [range[a].1, bv, origin[2]];
        painter.line_segment([proj(q0), proj(q1)], grid_stroke);
    }

    for (axis, col, name) in [
        (0usize, theme::AXIS_X, "X"),
        (1, theme::AXIS_Y, "Y"),
        (2, theme::AXIS_Z, "Z"),
    ] {
        let mut pe = origin;
        pe[axis] = range[axis].1;
        painter.line_segment([proj(origin), proj(pe)], egui::Stroke::new(1.5_f32, col));
        painter.text(
            proj(pe),
            egui::Align2::LEFT_BOTTOM,
            format!("{} ({:.1})", name, range[axis].1),
            egui::FontId::proportional(11.0),
            col,
        );
        let mut pn = origin;
        pn[axis] = range[axis].0;
        painter.line_segment(
            [proj(origin), proj(pn)],
            egui::Stroke::new(1.0_f32, theme::lighten(col, 0.45)),
        );
        painter.text(
            proj(pn),
            egui::Align2::RIGHT_TOP,
            format!("{:.1}", range[axis].0),
            egui::FontId::proportional(10.0),
            theme::lighten(col, 0.45),
        );
    }

    let op = proj(origin);
    painter.circle_filled(op, 3.0, theme::GRAY_900);
    painter.text(
        egui::pos2(op.x + 6.0, op.y - 6.0),
        egui::Align2::LEFT_BOTTOM,
        "O",
        egui::FontId::proportional(11.0),
        theme::GRAY_900,
    );
}

/// ビューポート右下にカメラの向きへ追従する座標系アイコン（XYZ 軸ガジェット）を描く。
///
/// CAD ソフトで一般的な、画面端に固定された小さな座標系。各軸をカメラの回転
/// クォータニオンで投影し、Z（手前）成分でソートして奥から描くことで
/// 手前の軸が上に重なる。軸色は 3D ビューと同一（赤=X / 緑=Y / 青=Z）。
/// 左下は支持条件凡例、右上は ViewCube が使うため右下に置く。
pub(super) fn draw_axis_gadget(painter: &egui::Painter, cam: &CameraState) {
    let rect = painter.clip_rect();
    let center = egui::pos2(rect.max.x - 45.0, rect.max.y - 45.0);
    const LEN: f32 = 28.0;

    let axes: [([f32; 3], egui::Color32, &str); 3] = [
        ([1.0, 0.0, 0.0], theme::AXIS_X, "X"),
        ([0.0, 1.0, 0.0], theme::AXIS_Y, "Y"),
        ([0.0, 0.0, 1.0], theme::AXIS_Z, "Z"),
    ];

    let mut projected: Vec<(egui::Vec2, egui::Color32, &str, f32)> = axes
        .iter()
        .map(|(v, col, name)| {
            let r = q_rotate(cam.rot, *v);
            (egui::vec2(r[0], -r[1]), *col, *name, r[2])
        })
        .collect();
    projected.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));

    painter.circle_filled(center, LEN + 8.0, theme::translucent(theme::WHITE, 200));

    for (dir, col, name, _) in &projected {
        let end = center + *dir * LEN;
        draw_arrow(painter, center, end, *col);
        let label_pos = center + *dir * (LEN + 10.0);
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            *name,
            egui::FontId::proportional(12.0),
            *col,
        );
    }
    painter.circle_filled(center, 2.0, theme::GRAY_900);
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::model::ElementKind;

    #[test]
    fn 不正な参照方向の応力図オフセットは生成しない() {
        assert!(diagram_offset_dir(
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [f64::INFINITY, 0.0, 0.0],
            DiagramPlane::Ey,
        )
        .is_none());
    }

    use sepika_core::ids::{NodeId, SlabId};
    use sepika_core::model::{LoadTransfer, Node, RegionAnchor, Slab, SlabPlate, SlabShape};

    fn slab_app(points: &[[f64; 3]], attached_extent: Option<[f64; 2]>) -> App {
        let mut app = App::default();
        app.core.model.nodes = points
            .iter()
            .enumerate()
            .map(|(i, &coord)| Node {
                id: NodeId(i as u32),
                coord,
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            })
            .collect();
        if let Some(extent) = attached_extent {
            app.core.model.slabs.push(Slab {
                id: SlabId(0),
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
            });
        } else {
            let nodes: Vec<_> = app.core.model.nodes.iter().map(|n| n.id).collect();
            app.core
                .model
                .add_enclosed_slab_from_nodes(&nodes, SlabPlate::default());
        }
        app
    }

    fn paint_slabs(
        app: &App,
        coords: &[[f64; 3]],
        rot: [f32; 4],
    ) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let cam = CameraState {
            rot,
            ..Default::default()
        };
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let proj = Projector::new([0.0; 3], &cam, 0.1, [0.0; 2]);
            draw_slabs(
                &painter,
                app,
                FrameFilter {
                    elem_on: None,
                    node_on: None,
                },
                &proj,
                coords,
            );
        })
        .shapes
    }

    fn slab_meshes(shapes: &[egui::epaint::ClippedShape]) -> Vec<&egui::Mesh> {
        shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.as_ref()),
                _ => None,
            })
            .collect()
    }

    fn mesh_area_mm2(mesh: &egui::Mesh) -> f64 {
        mesh.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize].pos);
                f64::from(((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs()) * 50.0
            })
            .sum()
    }

    fn model_coords(app: &App) -> Vec<[f64; 3]> {
        app.core.model.nodes.iter().map(|n| n.coord).collect()
    }

    fn mode_plate_fixture() -> App {
        use sepika_core::ids::{SectionId, WallRegionId};
        use sepika_core::model::{Section, WallRegion};
        let mut app = slab_app(&[[0.0, 0.0, 3000.0]], Some([1000.0, 2000.0]));
        app.core.model.slabs[0].shape = SlabShape::Attached {
            anchor: RegionAnchor::Point(NodeId(0)),
            extent: [1000.0, 2000.0],
        };
        let mut add_nodes = |points: &[[f64; 3]]| {
            points
                .iter()
                .map(|&coord| {
                    let id = NodeId(app.core.model.nodes.len() as u32);
                    app.core.model.nodes.push(Node {
                        id,
                        coord,
                        restraint: Default::default(),
                        mass: None,
                        story: None,
                        support_spring: None,
                    });
                    id
                })
                .collect::<Vec<_>>()
        };
        let floor = add_nodes(&[
            [4000.0, 0.0, 3000.0],
            [6000.0, 0.0, 3000.0],
            [6000.0, 2000.0, 3000.0],
            [4000.0, 2000.0, 3000.0],
        ]);
        let line = add_nodes(&[[8000.0, 0.0, 3000.0], [10000.0, 0.0, 3000.0]]);
        let wall = add_nodes(&[[12000.0, 0.0, 0.0], [14000.0, 0.0, 0.0]]);
        let enclosed = add_nodes(&[
            [16000.0, 0.0, 0.0],
            [18000.0, 0.0, 0.0],
            [18000.0, 0.0, 3000.0],
            [16000.0, 0.0, 3000.0],
        ]);
        let structural = add_nodes(&[
            [20000.0, 0.0, 0.0],
            [22000.0, 0.0, 0.0],
            [22000.0, 0.0, 3000.0],
            [20000.0, 0.0, 3000.0],
        ]);
        app.core
            .model
            .add_enclosed_slab_from_nodes(&floor, SlabPlate::default());
        app.core.model.slabs.push(Slab {
            id: SlabId(2),
            shape: SlabShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [line[0], line[1]],
                    span: [0.0, 1.0],
                    transfer: LoadTransfer::Anchor,
                },
                extent: [1000.0; 2],
            },
            plate: SlabPlate::default(),
            tip_loads: Vec::new(),
        });
        let mut attached = enclosed_plate();
        attached.shape = sepika_core::model::WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [wall[0], wall[1]],
                span: [0.0, 1.0],
                transfer: LoadTransfer::Anchor,
            },
            extent: Some([1000.0; 2]),
        };
        app.core.model.wall_plates.push(attached);
        app.core
            .model
            .add_enclosed_wall_plate_from_nodes(&enclosed, enclosed_plate());
        app.core.model.sections.push(Section {
            id: SectionId(0),
            name: "壁".into(),
            frame_use: None,
            floor: None,
            area: 450000.0,
            iy: 1.0,
            iz: 1.0,
            j: 1.0,
            depth: 3000.0,
            width: 150.0,
            as_y: 1.0,
            as_z: 1.0,
            panel_thickness: None,
            thickness: Some(150.0),
            shape: None,
            material: None,
            rebar_material: None,
            shear_rebar_material: None,
            steel_material: None,
            property_basis: Default::default(),
        });
        let mut plate = enclosed_plate();
        plate.section = Some(SectionId(0));
        let id = app
            .core
            .model
            .add_enclosed_wall_plate_from_nodes(&structural, plate);
        app.core.model.wall_regions.push(WallRegion {
            id: WallRegionId(0),
            name: String::new(),
            boundary: structural,
            wall_plate_ids: vec![id],
            posts: Vec::new(),
        });
        app
    }

    fn paint_mode_ghost(
        app: &App,
        scale: f64,
        show_secondary: bool,
        filter: FrameFilter,
    ) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            let cam = CameraState::default();
            let proj = Projector::new([0.0; 3], &cam, 0.1, [0.0; 2]);
            let pts: Vec<_> = model_coords(app).iter().map(|&p| proj.project(p)).collect();
            let model = super::super::wall_expanded_view_model(&app.core.model);
            draw_mode_rest_ghost(
                &painter,
                app,
                &model,
                &proj,
                scale,
                &pts,
                &vec![false; pts.len()],
                filter,
                show_secondary,
                false,
            );
        })
        .shapes
    }

    fn filled_plate_points(
        shapes: &[egui::epaint::ClippedShape],
        alpha: u8,
    ) -> Vec<Vec<egui::Pos2>> {
        shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m)
                    if m.vertices.first().is_some_and(|v| {
                        v.color == theme::translucent(theme::BEST_YELLOW, alpha)
                    }) =>
                {
                    Some(m.vertices.iter().map(|v| v.pos).collect())
                }
                egui::Shape::Path(p) if p.fill == theme::translucent(theme::DATA_BLUE, alpha) => {
                    Some(p.points.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn mode_rest_input_plates_keep_each_input_boundary_once_for_both_signs_and_axes() {
        let app = mode_plate_fixture();
        let model = super::super::wall_expanded_view_model(&app.core.model);
        let wall = model
            .elements
            .iter()
            .find(|e| e.kind == ElementKind::Wall)
            .unwrap();
        assert!(model
            .elements
            .iter()
            .any(|e| e.id.0 == app.core.model.slabs[0].id.0));
        assert!(model
            .elements
            .iter()
            .any(|e| e.id.0 == app.core.model.wall_plates[0].id.0));
        assert_ne!(wall.id.0, app.core.model.wall_plates[2].id.0);
        assert!(app
            .core
            .model
            .wall_plate_becomes_element(&app.core.model.wall_plates[2]));
        let expected = [
            vec![
                [0.0, 0.0, 3000.0],
                [1000.0, 0.0, 3000.0],
                [1000.0, 2000.0, 3000.0],
                [0.0, 2000.0, 3000.0],
            ],
            vec![
                [4000.0, 0.0, 3000.0],
                [6000.0, 0.0, 3000.0],
                [6000.0, 2000.0, 3000.0],
                [4000.0, 2000.0, 3000.0],
            ],
            vec![
                [8000.0, 0.0, 3000.0],
                [10000.0, 0.0, 3000.0],
                [10000.0, 1000.0, 3000.0],
                [8000.0, 1000.0, 3000.0],
            ],
            vec![
                [12000.0, 0.0, 0.0],
                [14000.0, 0.0, 0.0],
                [14000.0, 0.0, 1000.0],
                [12000.0, 0.0, 1000.0],
            ],
            vec![
                [16000.0, 0.0, 0.0],
                [18000.0, 0.0, 0.0],
                [18000.0, 0.0, 3000.0],
                [16000.0, 0.0, 3000.0],
            ],
            vec![
                [20000.0, 0.0, 0.0],
                [22000.0, 0.0, 0.0],
                [22000.0, 0.0, 3000.0],
                [20000.0, 0.0, 3000.0],
            ],
        ];
        let cam = CameraState::default();
        let proj = Projector::new([0.0; 3], &cam, 0.1, [0.0; 2]);
        for axis in 0..2 {
            for scale in [0.0, 1.0, -1.0, 100.0] {
                let mut disp = vec![[0.0; 6]; app.core.model.nodes.len()];
                for d in &mut disp {
                    d[axis] = 1.0;
                }
                let coords: Vec<_> = app
                    .core
                    .model
                    .nodes
                    .iter()
                    .zip(disp)
                    .map(|(n, d)| std::array::from_fn(|i| n.coord[i] + d[i] * scale))
                    .collect();
                let deformed = paint_slabs(&app, &coords, CameraState::default().rot);
                assert_eq!(slab_meshes(&deformed).len(), 3);
                let shapes = paint_mode_ghost(&app, scale, true, FrameFilter::default());
                let polygons = filled_plate_points(&shapes, 55);
                if scale == 0.0 {
                    assert!(shapes.is_empty());
                    continue;
                }
                assert_eq!(polygons.len(), expected.len());
                for (input, boundary) in expected.iter().enumerate() {
                    let points: Vec<_> = boundary.iter().map(|&p| proj.project(p)).collect();
                    assert_eq!(
                        polygons.iter().filter(|p| **p == points).count(),
                        1,
                        "input {input}, axis {axis}, scale {scale}"
                    );
                }
                assert!(shapes.iter().any(|s| matches!(&s.shape,
                    egui::Shape::LineSegment { stroke, .. } if
                    stroke.color == theme::translucent(theme::BEST_YELLOW, 90))));
            }
        }
    }

    #[test]
    fn mode_rest_plate_filters_and_floor_toggle_share_the_input_visibility() {
        let app = mode_plate_fixture();
        let hidden = paint_mode_ghost(&app, 1.0, false, FrameFilter::default());
        assert_eq!(filled_plate_points(&hidden, 55).len(), 1);
        let no_nodes = vec![false; app.core.model.nodes.len()];
        let no_elements = [false];
        assert!(paint_mode_ghost(
            &app,
            1.0,
            true,
            FrameFilter {
                elem_on: Some(&no_elements),
                node_on: Some(&no_nodes),
            }
        )
        .is_empty());
        let mut node_on = no_nodes;
        node_on[0] = true;
        let shapes = paint_mode_ghost(
            &app,
            1.0,
            true,
            FrameFilter {
                elem_on: Some(&no_elements),
                node_on: Some(&node_on),
            },
        );
        assert_eq!(filled_plate_points(&shapes, 55).len(), 1);
        node_on[5] = true;
        node_on[7] = true;
        let shapes = paint_mode_ghost(
            &app,
            1.0,
            true,
            FrameFilter {
                elem_on: Some(&no_elements),
                node_on: Some(&node_on),
            },
        );
        assert_eq!(filled_plate_points(&shapes, 55).len(), 3);
    }

    #[test]
    fn mode_rest_invalid_slabs_and_sign_reversing_wall_keep_outlines_without_fill() {
        let mut app = slab_app(
            &[
                [0.0, 0.0, 3000.0],
                [4000.0, 4000.0, 3000.0],
                [0.0, 4000.0, 3000.0],
                [4000.0, 0.0, 3000.0],
            ],
            None,
        );
        let shapes = paint_mode_ghost(&app, 1.0, true, FrameFilter::default());
        assert!(filled_plate_points(&shapes, 55).is_empty());
        assert!(!shapes.is_empty());
        let mut attached = mode_plate_fixture();
        if let sepika_core::model::WallPlateShape::Attached { extent, .. } =
            &mut attached.core.model.wall_plates[0].shape
        {
            *extent = Some([1000.0, -1000.0]);
        }
        let shapes = paint_mode_ghost(&attached, 1.0, true, FrameFilter::default());
        assert_eq!(filled_plate_points(&shapes, 55).len(), 5);
        for bad in [f64::NAN, f64::INFINITY, f64::MAX] {
            app.core.model.nodes[1].coord[0] = bad;
            let shapes = paint_mode_ghost(&app, 1.0, true, FrameFilter::default());
            assert!(filled_plate_points(&shapes, 55).is_empty());
            assert!(shapes
                .iter()
                .all(|s| s.shape.visual_bounding_rect().is_finite()));
        }
    }

    fn paint_viewer(app: &mut App) -> Vec<egui::epaint::ClippedShape> {
        egui::Context::default()
            .run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1600.0, 1200.0),
                    )),
                    ..Default::default()
                },
                |ui| super::super::viewer_panel(ui, app),
            )
            .shapes
    }

    #[test]
    fn mode_rest_viewer_gating_and_input_nodes_preserve_model_undo_results_and_saved_content() {
        use super::super::ViewMode;
        use crate::app::ResultsBundle;
        use sepika_core::frame::FrameTarget;
        use sepika_core::ids::StoryId;
        use sepika_core::model::{Axis, AxisGroup, AxisGroupKind, AxisSource};
        use sepika_solver::dynamic::eigen::ModalResult;
        let mut app = mode_plate_fixture();
        app.core.model.axes.push(AxisGroup {
            name: "入力点".into(),
            kind: AxisGroupKind::Other,
            axes: vec![Axis {
                name: "支持点のみ".into(),
                distance: None,
                nodes: vec![NodeId(0)],
                source: AxisSource::Manual,
            }],
        });
        for (id, elevation) in [(0, 0.0), (1, 3000.0)] {
            app.core.model.stories.push(
                serde_json::from_value(serde_json::json!({
                    "id": id, "name": format!("階{id}"), "elevation": elevation, "sl_offset_mm": 0.0, "node_ids": []
                }))
                .unwrap(),
            );
        }
        assert!(!sepika_core::dof::structural_nodes(&app.core.model)[0]);
        let mut node_shapes = Vec::new();
        for axis in 0..2 {
            let mut disp = vec![[0.0; 6]; app.core.model.nodes.len()];
            for d in &mut disp[13..] {
                d[axis] = 1.0;
            }
            assert_eq!(disp[0], [0.0; 6]);
            node_shapes.push(disp);
        }
        app.core.scoped.results = Some(ResultsBundle {
            modal: Some(ModalResult {
                omega2: vec![1.0; 2],
                period: vec![1.0; 2],
                shapes: vec![vec![1.0]; 2],
                node_shapes,
                participation: vec![[1.0; 3]; 2],
                effective_mass: vec![[1.0; 3]; 2],
            }),
            ..Default::default()
        });
        app.ui.view.view_mode = ViewMode::Mode;
        app.ui.view.show_floor_secondary = true;
        app.ui.view.show_sections = false;
        let input = bincode::serialize(&app.core.model).unwrap();
        let results = bincode::serialize(&app.core.scoped.results).unwrap();
        let staleness = format!("{:?}", app.core.scoped.staleness);
        let undo = (
            app.core.scoped.undo.revision(),
            app.core.scoped.undo.can_undo(),
            app.core.scoped.undo.can_redo(),
        );
        let saved_model = rmp_serde::to_vec_named(&app.core.model).unwrap();
        let path =
            std::env::temp_dir().join(format!("sepika467-viewer-{}.ovika", std::process::id()));
        let save_contents = |app: &App| {
            let results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
            sepika_io::ovika::save_ovika(
                &path,
                &app.core.model,
                sepika_io::ovika::OvikaExtras {
                    results: Some(&results),
                    ..Default::default()
                },
            )
            .unwrap();
            use std::io::Read;
            let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
            ["model.msgpack", "results.msgpack"].map(|name| {
                let mut bytes = Vec::new();
                archive
                    .by_name(name)
                    .unwrap()
                    .read_to_end(&mut bytes)
                    .unwrap();
                bytes
            })
        };
        let saved_contents = save_contents(&app);
        for axis in 0..2 {
            app.ui.scoped.view_mode_idx = axis;
            for factor in [0.1, 1.0, 10.0] {
                app.ui.view.deform_scale_factor = factor;
                let shapes = paint_viewer(&mut app);
                assert_eq!(
                    filled_plate_points(&shapes, 55).len(),
                    6,
                    "axis {axis}, factor {factor}"
                );
                assert_eq!(
                    slab_meshes(&shapes)
                        .iter()
                        .filter(
                            |m| m.vertices[0].color == theme::translucent(theme::BEST_YELLOW, 28)
                        )
                        .count(),
                    3
                );
            }
        }
        app.ui.view.show_floor_secondary = false;
        assert_eq!(filled_plate_points(&paint_viewer(&mut app), 55).len(), 1);
        app.ui.view.show_floor_secondary = true;
        app.ui.scoped.frame_target = Some(FrameTarget::Axis { group: 0, axis: 0 });
        assert_eq!(filled_plate_points(&paint_viewer(&mut app), 55).len(), 1);
        app.ui.scoped.frame_target = Some(FrameTarget::Story(StoryId(1)));
        assert_eq!(filled_plate_points(&paint_viewer(&mut app), 55).len(), 3);
        app.ui.scoped.frame_target = None;
        app.ui.view.view_mode = ViewMode::Shape;
        assert!(filled_plate_points(&paint_viewer(&mut app), 55).is_empty());
        app.ui.view.view_mode = ViewMode::Mode;
        let retained = app.core.scoped.results.take();
        assert!(filled_plate_points(&paint_viewer(&mut app), 55).is_empty());
        app.core.scoped.results = retained;
        assert_eq!(bincode::serialize(&app.core.model).unwrap(), input);
        assert_eq!(
            bincode::serialize(&app.core.scoped.results).unwrap(),
            results
        );
        assert_eq!(format!("{:?}", app.core.scoped.staleness), staleness);
        assert_eq!(
            (
                app.core.scoped.undo.revision(),
                app.core.scoped.undo.can_undo(),
                app.core.scoped.undo.can_redo()
            ),
            undo
        );
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.model).unwrap(),
            saved_model
        );
        assert_eq!(save_contents(&app), saved_contents);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn slab_line_fill_observes_input_geometry_and_keeps_support_rejection() {
        for (extent, area, vertices) in [
            ([1000.0, -1000.0], 0.0, 0),
            ([1000.0; 2], 2_000_000.0, 4),
            ([0.0, 1000.0], 1_000_000.0, 3),
            ([1000.0, 0.0], 1_000_000.0, 3),
        ] {
            let app = slab_app(&[[0.0; 3], [4000.0, 0.0, 0.0]], Some(extent));
            let model = &app.core.model;
            let slab = &model.slabs[0];
            assert!(model.validate_attached_slab(slab).is_err());
            assert!(model.attached_slab_supports(slab).is_err());
            assert_eq!(
                model.validate_attached_slab_geometry(slab).is_ok(),
                area > 0.0
            );
            let shapes = paint_slabs(&app, &model_coords(&app), [1.0, 0.0, 0.0, 0.0]);
            let meshes = slab_meshes(&shapes);
            assert!(shapes.len() > meshes.len());
            if area == 0.0 {
                assert!(meshes.is_empty());
            } else {
                assert_eq!(meshes.len(), 1);
                assert_eq!(meshes[0].vertices.len(), vertices);
                assert!((mesh_area_mm2(meshes[0]) - area).abs() < 0.1);
                assert!(meshes[0]
                    .vertices
                    .iter()
                    .all(|v| v.color == theme::translucent(theme::BEST_YELLOW, 28)));
            }
            assert!(shapes.iter().any(|s| match &s.shape {
                egui::Shape::LineSegment { stroke, .. } =>
                    stroke.width == 1.5
                        && stroke.color == theme::translucent(theme::BEST_YELLOW, 220),
                _ => false,
            }));
        }
    }

    #[test]
    fn slab_convex_and_concave_fill_cover_the_input_and_leave_the_notch_empty() {
        for points in [
            vec![
                [0.0, 0.0, 0.0],
                [4000.0, 0.0, 0.0],
                [4000.0, 1000.0, 0.0],
                [0.0, 1000.0, 0.0],
            ],
            vec![
                [0.0, 0.0, 0.0],
                [4000.0, 0.0, 0.0],
                [4000.0, 1000.0, 0.0],
                [1000.0, 1000.0, 0.0],
                [1000.0, 4000.0, 0.0],
                [0.0, 4000.0, 0.0],
            ],
        ] {
            for reverse in [false, true] {
                let mut points = points.clone();
                if reverse {
                    points.reverse();
                }
                let app = slab_app(&points, None);
                let shapes = paint_slabs(&app, &points, [1.0, 0.0, 0.0, 0.0]);
                let meshes = slab_meshes(&shapes);
                assert_eq!(meshes.len(), 1);
                let expected = if points.len() == 4 {
                    4_000_000.0
                } else {
                    7_000_000.0
                };
                assert!((mesh_area_mm2(meshes[0]) - expected).abs() < 0.1);
                if points.len() == 6 {
                    for t in meshes[0].indices.as_chunks::<3>().0 {
                        let xy = t
                            .iter()
                            .map(|&i| {
                                let p = meshes[0].vertices[i as usize].pos;
                                [f64::from(p.x) * 10.0, -f64::from(p.y) * 10.0]
                            })
                            .collect::<Vec<_>>();
                        assert!(!sepika_core::geom::polygon::contains_including_boundary(
                            &xy,
                            [2000.0, 2000.0]
                        ));
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_slab_boundaries_keep_their_original_finite_edges_without_fill() {
        for points in [
            vec![
                [0.0, 0.0, 0.0],
                [4000.0, 4000.0, 0.0],
                [0.0, 4000.0, 0.0],
                [4000.0, 0.0, 0.0],
            ],
            vec![
                [0.0, 0.0, 0.0],
                [4000.0, 3000.0, 0.0],
                [0.0, 4000.0, 0.0],
                [3000.0, 0.0, 0.0],
            ],
            vec![
                [0.0, 0.0, 0.0],
                [4000.0, 0.0, 0.0],
                [4000.0, 0.0, 0.0],
                [0.0, 4000.0, 0.0],
            ],
            vec![[0.0, 0.0, 0.0], [2000.0, 0.0, 0.0], [4000.0, 0.0, 0.0]],
        ] {
            let app = slab_app(&points, None);
            let shapes = paint_slabs(&app, &points, [1.0, 0.0, 0.0, 0.0]);
            assert!(slab_meshes(&shapes).is_empty());
            let segments: Vec<_> = shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::LineSegment { points, .. } => Some(points),
                    _ => None,
                })
                .collect();
            assert!(!segments.is_empty());
            let xy: Vec<_> = points.iter().map(|p| [p[0], p[1]]).collect();
            for segment in segments {
                assert!(xy.iter().enumerate().any(|(i, &a)| {
                    let b = xy[(i + 1) % xy.len()];
                    segment.iter().all(|p| {
                        sepika_core::geom::polygon::point_segment_dist(
                            [f64::from(p.x) * 10.0, -f64::from(p.y) * 10.0],
                            a,
                            b,
                        ) < 0.01
                    })
                }));
            }
        }
    }

    #[test]
    fn slab_fill_indices_are_stable_under_rotation_deformation_and_edge_collapse() {
        for extent in [[1000.0, -1000.0], [1000.0; 2], [0.0, 1000.0], [1000.0, 0.0]] {
            let mut app = slab_app(&[[0.0; 3], [4000.0, 0.0, 1000.0]], Some(extent));
            let slabs = std::mem::take(&mut app.core.model.slabs);
            for coord in [[5000.0, 0.0, 0.0], [6000.0, 0.0, 0.0]] {
                assert!(app.core.scoped.undo.run(
                    &mut app.core.model,
                    Box::new(sepika_edit::AddNode {
                        coord,
                        restraint: Default::default(),
                    })
                ));
            }
            app.core.scoped.undo.undo(&mut app.core.model);
            app.core.model.slabs = slabs;
            assert!(app.core.scoped.undo.can_undo());
            assert!(app.core.scoped.undo.can_redo());
            let labels = (
                app.core.scoped.undo.undo_label().map(str::to_owned),
                app.core.scoped.undo.redo_label().map(str::to_owned),
            );
            app.core.scoped.results = Some(crate::app::ResultsBundle {
                analysis_contract_version: Some(17),
                panel_moments: vec![(NodeId(0), [123.0, 456.0])],
                ..Default::default()
            });
            app.certify_design_fixture();
            assert!(!app
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .input_records
                .is_empty());
            app.core.scoped.project_path = Some("test.ovika".into());
            let results = bincode::serialize(&app.core.scoped.results).unwrap();
            let revision = app.core.scoped.undo.revision();
            let input = bincode::serialize(&app.core.model).unwrap();
            let state = format!("{:?}", app.core.scoped.staleness);
            let undo = (
                app.core.scoped.undo.can_undo(),
                app.core.scoped.undo.can_redo(),
            );
            let original = paint_slabs(&app, &model_coords(&app), [1.0, 0.0, 0.0, 0.0]);
            let original_indices = slab_meshes(&original).first().map(|m| m.indices.clone());
            for coords in [
                vec![[9000.0, 4000.0, -500.0], [9000.0, 4000.000000001, 500.0]],
                vec![[9000.0, 4000.0, -500.0], [500.0, -3000.0, 1500.0]],
            ] {
                for rot in [
                    [1.0, 0.0, 0.0, 0.0],
                    CameraState::default().rot,
                    [0.70710677, 0.70710677, 0.0, 0.0],
                ] {
                    let shapes = paint_slabs(&app, &coords, rot);
                    assert_eq!(
                        slab_meshes(&shapes).first().map(|m| m.indices.clone()),
                        original_indices
                    );
                }
            }
            assert_eq!(bincode::serialize(&app.core.model).unwrap(), input);
            assert_eq!(app.core.model.slabs[0].id, SlabId(0));
            assert_eq!(format!("{:?}", app.core.scoped.staleness), state);
            assert_eq!(
                (
                    app.core.scoped.undo.can_undo(),
                    app.core.scoped.undo.can_redo()
                ),
                undo
            );
            assert_eq!(
                bincode::serialize(&app.core.scoped.results).unwrap(),
                results
            );
            assert_eq!(app.core.scoped.undo.revision(), revision);
            assert_eq!(
                (
                    app.core.scoped.undo.undo_label().map(str::to_owned),
                    app.core.scoped.undo.redo_label().map(str::to_owned)
                ),
                labels
            );
            assert_eq!(
                app.core.scoped.project_path.as_deref(),
                Some(std::path::Path::new("test.ovika"))
            );
        }
    }

    #[test]
    fn slab_nonfinite_model_display_and_projection_never_reach_painter() {
        let mut app = slab_app(
            &[
                [0.0, 0.0, 0.0],
                [4000.0, 0.0, 0.0],
                [4000.0, 4000.0, 0.0],
                [0.0, 4000.0, 0.0],
            ],
            None,
        );
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let mut coords = model_coords(&app);
            coords[1][0] = bad;
            let shapes = paint_slabs(&app, &coords, [1.0, 0.0, 0.0, 0.0]);
            assert!(slab_meshes(&shapes).is_empty());
            assert!(!shapes.is_empty());
            for s in &shapes {
                assert!(s.shape.visual_bounding_rect().is_finite());
                if let egui::Shape::LineSegment { points, .. } = &s.shape {
                    assert!(points.iter().all(|p| p.x == 0.0 || p.y == -400.0));
                }
            }
            app.core.model.nodes[1].coord[0] = bad;
            assert!(slab_fill_triangles(&app.core.model, &app.core.model.slabs[0]).is_none());
            let shapes = paint_slabs(&app, &coords, [1.0, 0.0, 0.0, 0.0]);
            assert!(slab_meshes(&shapes).is_empty());
            assert!(shapes
                .iter()
                .all(|s| s.shape.visual_bounding_rect().is_finite()));
            app.core.model.nodes[1].coord[0] = 4000.0;
        }
    }

    fn enclosed_plate() -> sepika_core::model::WallPlate {
        sepika_core::model::WallPlate {
            dl_support: None,
            self_weight_shares: Vec::new(),
            id: sepika_core::ids::WallPlateId(0),
            shape: sepika_core::model::WallPlateShape::Enclosed,
            section: None,
            opening_area: 0.0,
            opening_weight: 0.0,
            openings: Vec::new(),
            loads: vec![],
            slit: Default::default(),
        }
    }

    /// 境界節点で囲まれた壁版と割当領域を持つモデル。
    fn model_with_enclosed(
        boundary: &[sepika_core::ids::NodeId],
    ) -> (sepika_core::Model, sepika_core::ids::WallPlateId) {
        let count = boundary.iter().map(|n| n.0 + 1).max().unwrap_or(0);
        let mut model = sepika_core::Model::default();
        for i in 0..count {
            model.nodes.push(sepika_core::model::Node {
                id: sepika_core::ids::NodeId(i),
                coord: [f64::from(i) * 1000.0, 0.0, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        let id = model.add_enclosed_wall_plate_from_nodes(boundary, enclosed_plate());
        (model, id)
    }

    /// 立ち上がり高さが両端で符号反転する取り付く壁版は、塗らずに輪郭だけ描く。
    ///
    /// 境界の 4 点が自己交差する蝶ネクタイ形になり、凸多角形前提の塗りが輪郭から
    /// はみ出すためである。面積の算定が同じ形を避けているのと同じ理由による
    /// （`WallPlate::area`）。
    #[test]
    fn 符号反転する取り付く壁版は塗らない() {
        use sepika_core::ids::NodeId;
        use sepika_core::model::{RegionAnchor, WallPlateShape};
        let model = sepika_core::Model::default();
        let mut plate = enclosed_plate();
        let mut with_extent = |extent: [f64; 2]| {
            plate.shape = WallPlateShape::Attached {
                anchor: RegionAnchor::Line {
                    nodes: [NodeId(0), NodeId(1)],
                    span: [0.0, 1.0],
                    transfer: Default::default(),
                },
                extent: Some(extent),
            };
            plate_fill_is_valid(&model, &plate)
        };
        assert!(with_extent([900.0, 900.0]), "同じ向きの立ち上がりは塗る");
        assert!(with_extent([900.0, 0.0]), "片端 0 は自己交差しない");
        assert!(with_extent([-900.0, -300.0]), "下向き同士も塗る");
        assert!(!with_extent([900.0, -900.0]), "符号が反転する壁は塗らない");

        // 囲まれた壁版は境界そのものなので、この理由では塗りを止めない。
        assert!(plate_fill_is_valid(&model, &enclosed_plate()));
    }

    /// 構面表示中は、境界節点がすべてその構面にある壁版だけを描く。
    ///
    /// 床板（`slab_visible_on_frame`）と同じ規約である。1 点でも構面外にあると
    /// 多角形が構面から飛び出して描かれ、構面図として読めなくなる。
    #[test]
    fn 囲まれた壁版は境界節点が全て構面上にあるときだけ描く() {
        use sepika_core::ids::NodeId;
        let on = [true, true, true, false];
        let filter = FrameFilter {
            elem_on: None,
            node_on: Some(&on),
        };
        // 節点 3 が構面外。
        let (model, id) = model_with_enclosed(&[NodeId(0), NodeId(1), NodeId(2), NodeId(3)]);
        assert!(!wall_plate_visible_on_frame(
            &model,
            model.wall_plate(id).unwrap(),
            filter
        ));
        let (model3, id3) = model_with_enclosed(&[NodeId(0), NodeId(1), NodeId(2)]);
        assert!(wall_plate_visible_on_frame(
            &model3,
            model3.wall_plate(id3).unwrap(),
            filter
        ));
        // 構面表示していないときはすべて描く。
        let all = FrameFilter {
            elem_on: None,
            node_on: None,
        };
        assert!(wall_plate_visible_on_frame(
            &model,
            model.wall_plate(id).unwrap(),
            all
        ));
    }

    /// 取り付く壁版は取付き先の節点が構面上にあれば描く（床板と同じ規約）。
    /// 取付き先が点の壁版は座標を組み立てられないため描かない。
    #[test]
    fn 取り付く壁版は取付き先の節点で構面を判定する() {
        use sepika_core::ids::NodeId;
        use sepika_core::model::{RegionAnchor, WallPlateShape};
        let on = [true, false, false];
        let filter = FrameFilter {
            elem_on: None,
            node_on: Some(&on),
        };
        let mut plate = enclosed_plate();
        let empty = sepika_core::Model::default();
        plate.shape = WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(0), NodeId(1)],
                span: [0.0, 1.0],
                transfer: Default::default(),
            },
            extent: Some([900.0, 900.0]),
        };
        assert!(wall_plate_visible_on_frame(&empty, &plate, filter));

        plate.shape = WallPlateShape::Attached {
            anchor: RegionAnchor::Line {
                nodes: [NodeId(1), NodeId(2)],
                span: [0.0, 1.0],
                transfer: Default::default(),
            },
            extent: Some([900.0, 900.0]),
        };
        assert!(!wall_plate_visible_on_frame(&empty, &plate, filter));

        plate.shape = WallPlateShape::Attached {
            anchor: RegionAnchor::Point(NodeId(0)),
            extent: Some([900.0, 900.0]),
        };
        assert!(!wall_plate_visible_on_frame(&empty, &plate, filter));
    }

    #[test]
    fn 外周スラブは大梁フィルタと変形後座標を使い輪郭を閉じる() {
        use sepika_core::ids::{ElemId, NodeId};
        use sepika_core::model::PerimeterSlab;
        let perimeter = PerimeterSlab {
            beam: ElemId(0),
            story: sepika_core::ids::StoryId(0),
            boundary: [
                [0.0, 0.0, 0.0],
                [1000.0, 0.0, 0.0],
                [1000.0, 500.0, 0.0],
                [0.0, 500.0, 0.0],
            ],
            extent_mm: 500.0,
        };
        let all = FrameFilter::default();
        assert!(perimeter_visible_on_frame(&perimeter, all));
        let off = [false, false, false, true];
        assert!(!perimeter_visible_on_frame(
            &perimeter,
            FrameFilter {
                elem_on: Some(&off),
                node_on: None
            }
        ));

        let mut model = sepika_core::Model::default();
        model.nodes.extend([
            sepika_core::model::Node {
                id: NodeId(0),
                coord: [0.0; 3],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            },
            sepika_core::model::Node {
                id: NodeId(1),
                coord: [1000.0, 0.0, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            },
        ]);
        model.elements.push(sepika_core::model::ElementData {
            id: ElemId(0),
            kind: ElementKind::Beam,
            nodes: vec![NodeId(0), NodeId(1)].into(),
            section: None,
            local_axis: sepika_core::model::LocalAxis {
                ref_vector: [0.0, 1.0, 0.0],
            },
            end_cond: [sepika_core::model::EndCondition::Fixed; 2],
            force_regime: sepika_core::model::ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        });
        let moved = [[10.0, 0.0, 20.0], [1020.0, 0.0, 30.0]];
        let coords = perimeter_boundary_with_coords(&model, &perimeter, &moved).unwrap();
        assert_eq!(coords[0], moved[0]);
        assert_eq!(coords[1], moved[1]);
        assert_eq!(coords[2], [1020.0, 500.0, 30.0]);
        assert_eq!(coords[3], [10.0, 500.0, 20.0]);
        let projected = coords.to_vec();
        let mut closed = projected.clone();
        closed.push(closed[0]);
        assert_eq!(closed.first(), closed.last());
    }

    #[test]
    fn 仕口パネルと面要素は部材線として描かない() {
        // 仕口パネルの節点列は「接合部の節点 ＋ 取り付く部材の他端」なので、先頭
        // 2 節点を結ぶと取り付く柱・梁と同じ線分になる。全部材が直線のうちは実部材と
        // 重なって見えないが、内部たわみ表示で梁・柱を曲線にすると弦の直線だけが
        // 残り、部材が二重に描かれてしまうため線材として扱わない。
        assert!(!draws_as_line(ElementKind::PanelZone));
        assert!(!draws_as_line(ElementKind::Wall));
        assert!(!draws_as_line(ElementKind::Shell));
        // 材軸を持つ 2 節点要素は線で描く。
        assert!(draws_as_line(ElementKind::Beam));
        assert!(draws_as_line(ElementKind::Fiber));
        assert!(draws_as_line(ElementKind::MultiSpring));
        assert!(draws_as_line(ElementKind::Brace {
            tension_only: false
        }));
        assert!(draws_as_line(ElementKind::NodalSpring));
        assert!(draws_as_line(ElementKind::Isolator));
        assert!(draws_as_line(ElementKind::Damper));
    }

    #[test]
    fn 要素の描き方は種別ごとに一意に決まる() {
        // 仕口パネルは部材線も選択ハイライトも描かない。先頭 2 節点が取り付く
        // 柱・梁と同じ節点対になるため、線を引くと選択していない柱・梁が
        // 選択されているように見えてしまう。
        assert_eq!(element_draw_shape(ElementKind::PanelZone), DrawShape::None);
        // 面要素は多角形（ハイライトはその輪郭）。
        assert_eq!(element_draw_shape(ElementKind::Wall), DrawShape::Polygon);
        assert_eq!(element_draw_shape(ElementKind::Shell), DrawShape::Polygon);
        // 材軸を持つ要素は線分。
        for kind in [
            ElementKind::Beam,
            ElementKind::Fiber,
            ElementKind::MultiSpring,
            ElementKind::Brace {
                tension_only: false,
            },
            ElementKind::NodalSpring,
            ElementKind::Isolator,
            ElementKind::Damper,
        ] {
            assert_eq!(element_draw_shape(kind), DrawShape::Line, "{kind:?}");
        }
    }
}
