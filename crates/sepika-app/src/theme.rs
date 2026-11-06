//! 配色・テーマの単一情報源。
//!
//! 色値を定数化したもの。UI 各所はこの定数を参照し、独自色を散らさない。
//! テーマ全体（ライト基準・ブルークローム・角丸）は [`apply_theme`] で egui に適用する。
//!
//! UI の文字サイズは `TextStyle` で指定し、生の pt は各ウィジェットに持たせない。
//! テキストを含む箱の高さは `ui.text_style_height()` から導出する。
//! 固定 px は文字を含まない形状（アイコン・線幅・当たり判定等）に限る。
//! パネル内側余白は本モジュールの共通 frame helper を使い、独自値を持たせない。
//! 項目間隔は標準を基本とし、リストやアイコン列等の密集領域だけ明示的に縮める。

use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

/// 画面背景。
pub const UI_BACKGROUND: Color32 = Color32::from_rgb(0xF4, 0xF7, 0xFA);
/// グループ見出し・ツールバー。
pub const UI_HEADER: Color32 = Color32::from_rgb(0xD6, 0xE8, 0xF5);
/// 別グループ見出し・ステータスバー。
pub const UI_HEADER_ALT: Color32 = Color32::from_rgb(0xE2, 0xE8, 0xF6);
/// 主要操作のボタン面。
pub const UI_ACTION: Color32 = Color32::from_rgb(0xC0, 0xD8, 0xEB);
/// 白い操作面のホバー。
pub const UI_HOVER: Color32 = Color32::from_rgb(0xE1, 0xEE, 0xF8);
/// 操作アクセント・選択印。
pub const UI_ACCENT: Color32 = Color32::from_rgb(0x52, 0x7A, 0x9A);
/// グループ外枠。
pub const UI_OUTLINE: Color32 = Color32::from_rgb(0x46, 0x58, 0x64);
/// 入力欄の境界。
pub const UI_INPUT_BORDER: Color32 = Color32::from_rgb(0x7D, 0x91, 0x9F);
/// 内部の区切り線。
pub const UI_DIVIDER: Color32 = Color32::from_rgb(0xD6, 0xE0, 0xE7);
/// 見出し・本文・数値。
pub const UI_TEXT: Color32 = Color32::from_rgb(0x30, 0x43, 0x4F);
/// 白〜ごく淡い背景上の補助文字。
pub const UI_MUTED: Color32 = Color32::from_rgb(0x5D, 0x6E, 0x7A);

/// ソリッド表示の鋼材面。
pub const BLUE_300: Color32 = Color32::from_rgb(0x8E, 0xB9, 0xE5);
/// モデルのホバー強調。
pub const BLUE_500: Color32 = Color32::from_rgb(0x21, 0x71, 0xC1);
/// 結果グラフの青色系列。
pub const BLUE_600: Color32 = Color32::from_rgb(0x18, 0x5A, 0x9E);

/// 図の注記・寸法文字。
pub const GRAY_900: Color32 = Color32::from_rgb(0x1A, 0x23, 0x32);
/// モデル線・図の文字。
pub const GRAY_700: Color32 = Color32::from_rgb(0x2D, 0x3A, 0x4A);
/// モデルの補助線・図の文字。
pub const GRAY_600: Color32 = Color32::from_rgb(0x4A, 0x55, 0x68);
/// キャンバスのドットグリッド
pub const GRAY_300: Color32 = Color32::from_rgb(0xA8, 0xB4, 0xC4);
/// 入力欄・2D グラフ背景
pub const WHITE: Color32 = Color32::WHITE;

/// 結果グラフの緑色系列。
pub const GREEN_600: Color32 = Color32::from_rgb(0x00, 0x8F, 0x40);
/// エラー表示（ブランド対象外の固定色）
pub const ERROR_RED: Color32 = Color32::from_rgb(0xEA, 0x43, 0x35);

/// 標準・既定（既定の線・要素・変位、グラフの基準系列）
pub const DATA_BLUE: Color32 = Color32::from_rgb(0x2B, 0x95, 0xFF);
/// 超過・危険側（検定比 > 1.0＝NG・応力集中・負方向の量・崩壊ヒンジ）
pub const PARETO_RED: Color32 = Color32::from_rgb(0xEA, 0x43, 0x35);
/// 注意・中間（検定比 0.8〜1.0・中間状態・予測値）
pub const BEST_YELLOW: Color32 = Color32::from_rgb(0xFB, 0xBC, 0x04);
/// 良好・OK（検定比 ≤ 0.8＝OK・収束・正常状態）
pub const GOOD_GREEN: Color32 = Color32::from_rgb(0x34, 0xA8, 0x53);
/// ハイライト（選択中の節点・部材・断面）
pub const HILITE_PURPLE: Color32 = Color32::from_rgb(0x7C, 0x4D, 0xFF);
/// 二次部材（小梁・間柱）= 解析対象外の実在部材の線・輪郭
/// （スラブの BEST_YELLOW と同族の暖色。線の視認性のため濃いめのアンバー）
pub const SECONDARY_AMBER: Color32 = Color32::from_rgb(0xD9, 0x77, 0x06);
/// 淡色パネル上の注意ラベル（⚠ stale 等）。BEST_YELLOW はグラフ塗り用で文字には薄い。
pub const WARN_TEXT: Color32 = Color32::from_rgb(0x8A, 0x49, 0x00);

/// 淡色背景上のエラー文字。
pub const ERROR_TEXT: Color32 = Color32::from_rgb(0xB0, 0x24, 0x1C);
/// 淡色背景上の正常状態文字。
pub const GOOD_TEXT: Color32 = Color32::from_rgb(0x1C, 0x68, 0x34);
/// キーボードフォーカスの輪郭。
pub const FOCUS_STROKE: Stroke = Stroke {
    width: 2.0,
    color: UI_ACCENT,
};
/// 選択領域の不透明背景。
pub const SELECTION_BG: Color32 = Color32::from_rgb(0xEA, 0xF3, 0xFA);

/// 名称を一行に省略し、ホバーで全文を表示する。
pub(crate) fn name_label(ui: &mut egui::Ui, name: &str) -> egui::Response {
    ui.add(egui::Label::new(name).truncate())
        .on_hover_text(name)
}

/// 選択項目に印と全文ホバーを付ける。キー操作は標準ボタンに従う。
pub(crate) fn selectable_name(ui: &mut egui::Ui, selected: bool, name: &str) -> egui::Response {
    let label = if selected {
        format!("▸ {name}")
    } else {
        name.to_owned()
    };
    let response = ui
        .add(egui::Button::selectable(selected, label).truncate())
        .on_hover_text(name);
    focus_outline(ui, &response);
    response
}

/// フォーカスがある操作欄の内側に輪郭を描く。
pub(crate) fn focus_outline(ui: &egui::Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            3,
            FOCUS_STROKE,
            egui::StrokeKind::Inside,
        );
    }
}

/// 補助パネルの見出しを、外枠を増やさず表示する。
pub(crate) fn panel_heading(ui: &mut egui::Ui, title: &str, access: &str) {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui
        .horizontal_wrapped(|ui| {
            ui.strong(egui::RichText::new(title).color(UI_TEXT));
            ui.label(egui::RichText::new(access).small().color(UI_TEXT));
        })
        .response;
    let mut rect = response.rect;
    rect.max.x = ui.max_rect().right().min(ui.clip_rect().right());
    rect = rect.expand2(egui::vec2(3.0, 2.0));
    ui.painter()
        .set(background, egui::Shape::rect_filled(rect, 3, UI_HEADER));
    pencil_line(
        ui.painter(),
        ui.id().with(title),
        rect.left_bottom(),
        rect.right_bottom(),
    );
    ui.add_space(ui.spacing().item_spacing.y);
}

fn pencil_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let key = egui::Id::new("pencil_border_texture");
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(key)) {
        return texture;
    }
    let pixels = (0..128 * 16)
        .map(|i| {
            let grain = ((i as u32).wrapping_mul(747796405).wrapping_add(2891336453) >> 20) as u8;
            let alpha = if grain < 35 { 35 } else { 155 + grain / 3 };
            Color32::from_white_alpha(alpha)
        })
        .collect();
    let texture = ctx.load_texture(
        "pencil_border",
        egui::ColorImage::new([128, 16], pixels),
        egui::TextureOptions::LINEAR_REPEAT,
    );
    ctx.data_mut(|data| data.insert_temp(key, texture.clone()));
    texture
}

fn pencil_mesh(
    points: &[egui::Pos2],
    id: egui::Id,
    texture: egui::TextureId,
    pixels_per_point: f32,
) -> egui::epaint::Mesh {
    let mut mesh = egui::epaint::Mesh::with_texture(texture);
    let phase = (id.value() % 1024) as f32 * 0.01;
    let fringe = 0.7 / pixels_per_point;
    let mut distance = 0.0;
    for (i, &point) in points.iter().enumerate() {
        if i > 0 {
            distance += point.distance(points[i - 1]);
        }
        let tangent = points[(i + 1).min(points.len() - 1)] - points[i.saturating_sub(1)];
        let normal = egui::vec2(-tangent.y, tangent.x).normalized();
        let half_width =
            0.88 + 0.25 * (distance * 0.19 + phase).sin() + 0.12 * (distance * 0.63 + phase).sin();
        let center = point + normal * (0.12 * (distance * 0.11 + phase).sin());
        for (offset, v, color) in [
            (-half_width - fringe, 0.0, Color32::TRANSPARENT),
            (-half_width, 0.12, UI_OUTLINE),
            (half_width, 0.88, UI_OUTLINE),
            (half_width + fringe, 1.0, Color32::TRANSPARENT),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center + normal * offset,
                uv: egui::pos2(distance / 64.0 + phase, v),
                color,
            });
        }
        if i > 0 {
            let base = (i as u32 - 1) * 4;
            for band in 0..3 {
                mesh.add_triangle(base + band, base + band + 1, base + band + 4);
                mesh.add_triangle(base + band + 1, base + band + 5, base + band + 4);
            }
        }
    }
    mesh
}

fn pencil_path(painter: &egui::Painter, id: egui::Id, points: &[egui::Pos2]) {
    let texture = pencil_texture(painter.ctx());
    painter.add(pencil_mesh(
        points,
        id,
        texture.id(),
        painter.ctx().pixels_per_point(),
    ));
}

fn pencil_line(painter: &egui::Painter, id: egui::Id, start: egui::Pos2, end: egui::Pos2) {
    let length = start.distance(end);
    let steps = (length / 4.0).ceil() as usize;
    if steps == 0 {
        return;
    }
    let direction = (end - start) / length;
    let points: Vec<_> = (0..=steps)
        .map(|i| start + direction * (i as f32 * 4.0).min(length))
        .collect();
    pencil_path(painter, id, &points);
}

/// 安定した識別子で主要グループの枠を描く。操作領域や入力IDは追加しない。
pub(crate) fn group_outline(ui: &egui::Ui, rect: egui::Rect, id: egui::Id) {
    pencil_frame(ui.painter(), rect, id);
}

fn pencil_frame(painter: &egui::Painter, rect: egui::Rect, id: egui::Id) {
    if !rect.is_finite()
        || rect.width() < 12.0
        || rect.height() < 12.0
        || !painter.clip_rect().intersects(rect)
    {
        return;
    }
    let radius = 3.0;
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ];
    for side in 0..4 {
        let start = corners[side];
        let end = corners[(side + 1) % 4];
        let direction = (end - start).normalized();
        pencil_line(
            painter,
            id.with(side),
            start + direction * radius,
            end - direction * radius,
        );
        let next_direction = (corners[(side + 2) % 4] - end).normalized();
        let center = end - direction * radius + next_direction * radius;
        let points: Vec<_> = (0..=6)
            .map(|i| {
                let angle = i as f32 / 6.0 * std::f32::consts::FRAC_PI_2;
                center - next_direction * (radius * angle.cos())
                    + direction * (radius * angle.sin())
            })
            .collect();
        pencil_path(painter, id.with((side, "corner")), &points);
    }
}

/// 表の見出しを塗り、内容面との境界を描く。行高・入力IDは変更しない。
pub(crate) fn table_header(ui: &egui::Ui, width: f32, height: f32, id: egui::Id) {
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, height));
    ui.painter().rect_filled(rect, 0, UI_HEADER);
    pencil_line(ui.painter(), id, rect.left_bottom(), rect.right_bottom());
}

/// ダイアログの外周を、内容と同じレイヤーに描く。
pub(crate) fn window_outline(response: &egui::Response) {
    pencil_frame(
        &response.ctx.layer_painter(response.layer_id),
        response.rect.shrink(1.5),
        response.id.with("outline"),
    );
}

/// 補助領域の表示範囲を一つのグループとして囲む。
pub(crate) fn panel_outline(ui: &egui::Ui, salt: &str) {
    group_outline(
        ui,
        ui.available_rect_before_wrap().expand(4.0),
        ui.id().with(salt),
    );
}

/// 検定比の表示用文字色と状態名。コンターの色には使用しない。
pub(crate) fn ratio_status(ratio: f64) -> (Color32, &'static str) {
    if ratio <= 0.8 {
        (GOOD_TEXT, "良好")
    } else if ratio <= 1.0 {
        (WARN_TEXT, "注意")
    } else {
        (ERROR_TEXT, "超過")
    }
}

/// 2D グラフの背景。
pub const VIEW_BG: Color32 = Color32::WHITE;
/// 3D ビューの背景。
pub const VIEW_3D_BG: Color32 = Color32::from_rgb(0xF7, 0xF9, 0xFC);
/// 通常部材の一時的なホバー強調。
pub const VIEW_HOVER_STROKE: Stroke = Stroke {
    width: 6.0,
    color: BLUE_500,
};
/// 選択部材の継続的な強調。
pub const VIEW_SELECTED_STROKE: Stroke = Stroke {
    width: 8.0,
    color: HILITE_PURPLE,
};
/// X 軸（赤系）
pub const AXIS_X: Color32 = Color32::from_rgb(0xD2, 0x64, 0x64);
/// Y 軸（緑系）
pub const AXIS_Y: Color32 = Color32::from_rgb(0x50, 0xAA, 0x50);
/// Z 軸（青系）
pub const AXIS_Z: Color32 = Color32::from_rgb(0x64, 0x64, 0xC8);
/// 免震支承マーカー（軸方向を持たない支持装置の記号。矢印・円弧・ばねのジグザグ／
/// 渦巻（軸色）とも、剛床マーク・ハイライト（紫）とも弁別できる専用色）
pub const ISOLATOR_TEAL: Color32 = Color32::from_rgb(0x0F, 0x76, 0x6E);

/// パレット色を指定アルファで半透明化する（面要素・応力図の塗りつぶし用）。
pub fn translucent(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// 色を白側へ `t`（0.0–1.0）だけ寄せて淡くする。
pub fn lighten(c: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |ch: u8| (ch as f32 + (255.0 - ch as f32) * t).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// テーブルのヘッダ・本文に共通する上下余白の合計 [pt]。
const TABLE_ROW_PADDING_PT: f32 = 6.0;

/// ヘッダ・本文共通の行高 `[pt]`。本文フォントの行高に上下余白を加える。
pub fn table_row_height(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&TextStyle::Body) + TABLE_ROW_PADDING_PT
}

/// 検定比などの「状態」をセマンティック 3 色へ写像する。
/// 良好(≤0.8)=緑／注意(≤1.0)=黄／超過(>1.0)=赤。
pub fn status_color(ratio: f64) -> Color32 {
    if ratio <= 0.8 {
        GOOD_GREEN
    } else if ratio <= 1.0 {
        BEST_YELLOW
    } else {
        PARETO_RED
    }
}

/// 検定比のグラデーション配色（[`check_ratio_color`]）のアンカー点。
///
/// t は検定比そのもの（0.0–1.0）。0.8 に同じ t のアンカーを2つ置くことで、
/// 良好域（緑）と注意域（黄）の境界を連続補間せず不連続に切り替える
/// （判定基準 0.8 をまたいだことを色相の変化で明示するため）。
/// t=0.0 の淡緑は `lighten(GOOD_GREEN, 0.6)` の値（const 文脈では関数を
/// 呼べないため展開した定数）。
const CHECK_RATIO_LUT: [(f32, Color32); 4] = [
    (0.0, Color32::from_rgb(0xAE, 0xDC, 0xBA)),
    (0.8, GOOD_GREEN),
    (0.8, BEST_YELLOW),
    (1.0, SECONDARY_AMBER),
];

/// 検定比を連続グラデーション（コンター）へ写像する（3D ビューの検定比図用）。
///
/// [`status_color`] と同じ判定境界（0.8／1.0）を色相の切り替わりとして保ちつつ、
/// 各帯域の内部を連続的に濃くすることで、数値ラベルを出さなくても余裕度の
/// 大小を色から読み取れるようにしたもの。
///
/// - ≤0.8: 淡緑→緑（余裕が小さいほど濃い緑）
/// - ≤1.0: 黄→アンバー（注意域。0.8 の境界で緑から明確に色相が変わる）
/// - >1.0: 赤（NG。連続値ではなく単色）
pub fn check_ratio_color(ratio: f64) -> Color32 {
    if ratio > 1.0 {
        PARETO_RED
    } else {
        lut_sample(&CHECK_RATIO_LUT, ratio as f32)
    }
}

/// LUT（`(t, Color32)` のアンカー点列。t 昇順・[0,1] を覆う）を区間線形補間する
/// 共通ヘルパ。各カラーマップ（[`ColorMap::sample`]）はこれを呼ぶだけの薄い実装にする。
/// `t` は 0.0–1.0 にクランプしてから探索する。
fn lut_sample(lut: &[(f32, Color32)], t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    for w in lut.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let local = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * local).round() as u8;
            return Color32::from_rgb(
                mix(c0.r(), c1.r()),
                mix(c0.g(), c1.g()),
                mix(c0.b(), c1.b()),
            );
        }
    }
    lut.last().map(|&(_, c)| c).unwrap_or(WHITE)
}

/// Viridis カラーマップのアンカー点（matplotlib 版, t=0.000〜1.000 を 0.125 刻み）。
const VIRIDIS_LUT: [(f32, Color32); 9] = [
    (0.000, Color32::from_rgb(0x44, 0x01, 0x54)),
    (0.125, Color32::from_rgb(0x48, 0x28, 0x78)),
    (0.250, Color32::from_rgb(0x3E, 0x49, 0x89)),
    (0.375, Color32::from_rgb(0x31, 0x68, 0x8E)),
    (0.500, Color32::from_rgb(0x26, 0x82, 0x8E)),
    (0.625, Color32::from_rgb(0x1F, 0x9E, 0x89)),
    (0.750, Color32::from_rgb(0x35, 0xB7, 0x79)),
    (0.875, Color32::from_rgb(0x6D, 0xCD, 0x59)),
    (1.000, Color32::from_rgb(0xFD, 0xE7, 0x25)),
];

/// Plasma カラーマップのアンカー点（matplotlib 版, t=0.000〜1.000 を 0.125 刻み）。
const PLASMA_LUT: [(f32, Color32); 9] = [
    (0.000, Color32::from_rgb(0x0D, 0x08, 0x87)),
    (0.125, Color32::from_rgb(0x46, 0x03, 0x9F)),
    (0.250, Color32::from_rgb(0x72, 0x01, 0xA8)),
    (0.375, Color32::from_rgb(0x9C, 0x17, 0x9E)),
    (0.500, Color32::from_rgb(0xBD, 0x37, 0x86)),
    (0.625, Color32::from_rgb(0xD8, 0x57, 0x6B)),
    (0.750, Color32::from_rgb(0xED, 0x79, 0x53)),
    (0.875, Color32::from_rgb(0xFB, 0x9F, 0x3A)),
    (1.000, Color32::from_rgb(0xF0, 0xF9, 0x21)),
];

/// Turbo カラーマップのアンカー点（t=0.000〜1.000 を 0.125 刻み）。
/// 端点（t=0.000/1.000）は Google 公開の 256 点参照テーブルの正確な値
/// （インデックス 0 = `#30123B`、255 = `#7A0403`）、中間 7 点は広く知られた
/// Turbo の 5 次多項式近似から算出した値で、暗青→シアン→緑→黄→橙→暗赤の
/// 帯域配分を保つ。
const TURBO_LUT: [(f32, Color32); 9] = [
    (0.000, Color32::from_rgb(0x30, 0x12, 0x3B)),
    (0.125, Color32::from_rgb(0x45, 0x69, 0xEE)),
    (0.250, Color32::from_rgb(0x26, 0xBC, 0xE1)),
    (0.375, Color32::from_rgb(0x3F, 0xF3, 0x93)),
    (0.500, Color32::from_rgb(0x95, 0xFB, 0x51)),
    (0.625, Color32::from_rgb(0xEC, 0xD1, 0x2E)),
    (0.750, Color32::from_rgb(0xFF, 0x82, 0x1D)),
    (0.875, Color32::from_rgb(0xCB, 0x2F, 0x0D)),
    (1.000, Color32::from_rgb(0x7A, 0x04, 0x03)),
];

/// Jet カラーマップのアンカー点（伝統的な虹色レインボー。青→シアン→緑→黄→赤）。
const JET_LUT: [(f32, Color32); 9] = [
    (0.000, Color32::from_rgb(0x00, 0x00, 0x7F)),
    (0.125, Color32::from_rgb(0x00, 0x00, 0xFF)),
    (0.250, Color32::from_rgb(0x00, 0x7F, 0xFF)),
    (0.375, Color32::from_rgb(0x00, 0xFF, 0xFF)),
    (0.500, Color32::from_rgb(0x7F, 0xFF, 0x7F)),
    (0.625, Color32::from_rgb(0xFF, 0xFF, 0x00)),
    (0.750, Color32::from_rgb(0xFF, 0x7F, 0x00)),
    (0.875, Color32::from_rgb(0xFF, 0x00, 0x00)),
    (1.000, Color32::from_rgb(0x7F, 0x00, 0x00)),
];

/// 発散型（青-白-赤）カラーマップのアンカー点。独自色は使わず theme.rs の
/// 既存色（[`DATA_BLUE`] / [`WHITE`] / [`PARETO_RED`]）のみで構成する。
const BLUE_WHITE_RED_LUT: [(f32, Color32); 3] = [(0.0, DATA_BLUE), (0.5, WHITE), (1.0, PARETO_RED)];

/// コンター等の連続値表示で選択可能なカラーマップ。
///
/// 既定は知覚均等で色覚多様性に配慮した Viridis（本 enum の `#[default]`）。
/// 他の選択肢は UI からの明示的な切替のためのものであり、
/// 既定動作を変更するものではない。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorMap {
    /// 知覚均等・色覚多様性配慮（既定）
    #[default]
    Viridis,
    /// 知覚均等系（紫→赤→黄）
    Plasma,
    /// Google 開発の改良レインボー（知覚的な歪みを抑えたレインボー配色）
    Turbo,
    /// 伝統的な虹色レインボー（知覚均等ではないが慣れ親しまれている）
    Jet,
    /// 発散型（負=青／中央=白／正=赤）。符号のある値の可視化向け
    BlueWhiteRed,
}

impl ColorMap {
    /// UI 表示用ラベル。
    pub fn label(&self) -> &'static str {
        match self {
            ColorMap::Viridis => "Viridis",
            ColorMap::Plasma => "Plasma",
            ColorMap::Turbo => "Turbo",
            ColorMap::Jet => "Jet",
            ColorMap::BlueWhiteRed => "青-白-赤",
        }
    }

    /// `t`（0.0–1.0、範囲外はクランプ）をこのカラーマップの色へ写像する。
    pub fn sample(&self, t: f32) -> Color32 {
        let lut: &[(f32, Color32)] = match self {
            ColorMap::Viridis => &VIRIDIS_LUT,
            ColorMap::Plasma => &PLASMA_LUT,
            ColorMap::Turbo => &TURBO_LUT,
            ColorMap::Jet => &JET_LUT,
            ColorMap::BlueWhiteRed => &BLUE_WHITE_RED_LUT,
        };
        lut_sample(lut, t)
    }
}

/// 青系パステルの操作面・白い内容面と文字サイズを適用する。起動時に一度呼ぶ。
pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::light());

    let mut style = (*ctx.global_style()).clone();
    let mut v = egui::Visuals::light();

    v.panel_fill = UI_BACKGROUND;
    v.window_fill = WHITE;
    v.window_stroke = Stroke::new(1.0_f32, UI_OUTLINE);
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.extreme_bg_color = WHITE;
    v.text_edit_bg_color = Some(WHITE);
    v.faint_bg_color = UI_BACKGROUND;
    v.warn_fg_color = WARN_TEXT;
    v.error_fg_color = ERROR_TEXT;
    v.code_bg_color = WHITE;
    v.hyperlink_color = UI_ACCENT;

    v.selection.bg_fill = SELECTION_BG;
    v.selection.stroke = Stroke::new(1.5_f32, UI_TEXT);

    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);

    let r4 = CornerRadius::same(4);
    let r6 = CornerRadius::same(6);

    v.widgets.noninteractive.bg_fill = UI_BACKGROUND;
    v.widgets.noninteractive.weak_bg_fill = UI_BACKGROUND;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, UI_DIVIDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.noninteractive.corner_radius = r6;

    v.widgets.inactive.bg_fill = WHITE;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, UI_INPUT_BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.inactive.corner_radius = r4;

    v.widgets.hovered.bg_fill = UI_HOVER;
    v.widgets.hovered.weak_bg_fill = UI_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, UI_ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.hovered.corner_radius = r4;

    v.widgets.active.bg_fill = SELECTION_BG;
    v.widgets.active.weak_bg_fill = SELECTION_BG;
    v.widgets.active.bg_stroke = FOCUS_STROKE;
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, UI_TEXT);
    v.widgets.active.corner_radius = r4;

    v.widgets.open.bg_fill = WHITE;
    v.widgets.open.weak_bg_fill = UI_HEADER;
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, UI_DIVIDER);
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.open.corner_radius = r4;

    style.visuals = v;

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(14.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(6.0, 4.0);

    ctx.set_global_style(style);
}

/// 上部ツールバーの淡青色の枠。
pub fn toolbar_frame() -> egui::Frame {
    egui::Frame::new()
        .inner_margin(8)
        .fill(UI_HEADER)
        .stroke(Stroke::new(1.0_f32, UI_DIVIDER))
}

/// 左右ドックの白い内容面。
pub fn content_panel_frame() -> egui::Frame {
    egui::Frame::new().inner_margin(8).fill(WHITE)
}

/// 中央キャンバスおよび下ドックの枠（白）。
pub fn central_panel_frame() -> egui::Frame {
    egui::Frame::new()
        .inner_margin(8)
        .fill(WHITE)
        .stroke(Stroke::new(1.0_f32, UI_DIVIDER))
}

/// 下部ステータスバーの淡い青紫色の枠。
pub fn status_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .inner_margin(4)
        .fill(UI_HEADER_ALT)
        .stroke(Stroke::new(1.0_f32, UI_ACCENT))
}

/// 淡色のステータスバー上で文字・操作状態を表示する。
pub fn apply_status_bar_visuals(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, UI_HOVER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, UI_TEXT);
    v.widgets.hovered.bg_fill = UI_HOVER;
    v.widgets.hovered.weak_bg_fill = UI_HOVER;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, UI_TEXT);
    v.widgets.active.bg_fill = UI_ACTION;
    v.widgets.active.weak_bg_fill = UI_ACTION;
    v.selection.bg_fill = SELECTION_BG;
    v.selection.stroke = Stroke::new(1.0_f32, UI_TEXT);
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_text_remains_readable_on_panel_and_selection_backgrounds() {
        let luminance = |color: Color32| {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
        };
        for background in [
            WHITE,
            UI_BACKGROUND,
            SELECTION_BG,
            UI_HEADER,
            UI_HEADER_ALT,
            UI_ACTION,
        ] {
            for text in [UI_TEXT, WARN_TEXT, ERROR_TEXT, GOOD_TEXT] {
                let contrast = (luminance(background) + 0.05) / (luminance(text) + 0.05);
                assert!(contrast >= 4.5, "{text:?} / {background:?}: {contrast}");
            }
        }
    }

    #[test]
    fn pencil_border_stays_stable_on_repaint_and_keeps_existing_grain_when_extended() {
        let id = egui::Id::new("section_list");
        let texture = egui::TextureId::Managed(1);
        for scale in [1.0, 1.5] {
            let points: Vec<_> = (0..=20).map(|i| egui::pos2(i as f32 * 4.0, 0.0)).collect();
            let mesh = pencil_mesh(&points, id, texture, scale);
            assert_eq!(mesh, pencil_mesh(&points, id, texture, scale));
            let extended: Vec<_> = (0..=30).map(|i| egui::pos2(i as f32 * 4.0, 0.0)).collect();
            let longer = pencil_mesh(&extended, id, texture, scale);
            assert_eq!(mesh.vertices, longer.vertices[..mesh.vertices.len()]);
            assert!(mesh
                .vertices
                .iter()
                .any(|v| v.color == Color32::TRANSPARENT));
            let widths: Vec<_> = mesh
                .vertices
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| v[1].pos.distance(v[2].pos))
                .collect();
            assert!(
                widths.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                    - widths.iter().copied().fold(f32::INFINITY, f32::min)
                    > 0.5
            );
            assert!(mesh.vertices.iter().all(|v| v.pos.y.abs() < 2.5));
        }
    }

    #[test]
    fn group_decoration_reuses_texture_and_does_not_allocate_widget_ids() {
        let ctx = egui::Context::default();
        apply_theme(&ctx);
        let mut texture = None;
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let before = ui.next_auto_id();
                group_outline(
                    ui,
                    egui::Rect::from_min_size(ui.cursor().min, egui::vec2(240.0, 100.0)),
                    ui.id().with("attributes"),
                );
                assert_eq!(before, ui.next_auto_id());
                let current = pencil_texture(ui.ctx()).id();
                if let Some(previous) = texture {
                    assert_eq!(current, previous);
                }
                texture = Some(current);
            });
        }
        assert_eq!(VIEW_HOVER_STROKE.color, Color32::from_rgb(0x21, 0x71, 0xC1));
        assert_eq!(BLUE_300, Color32::from_rgb(0x8E, 0xB9, 0xE5));
    }

    #[test]
    fn ratio_state_is_explicit_at_display_boundaries() {
        assert_eq!(ratio_status(0.8).1, "良好");
        assert_eq!(ratio_status(0.80001).1, "注意");
        assert_eq!(ratio_status(1.0).1, "注意");
        assert_eq!(ratio_status(1.00001).1, "超過");
    }

    #[test]
    fn long_names_stay_inside_narrow_panel_at_two_product_scales() {
        for scale in [1.0, 1.5] {
            let ctx = egui::Context::default();
            apply_theme(&ctx);
            ctx.set_zoom_factor(scale);
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_width(160.0);
                let name = "一階大梁の長い日本語の断面名称と識別の補足";
                let label = name_label(ui, name);
                assert!(label.rect.width() <= 160.0);
                let selected = selectable_name(ui, true, name);
                assert!(selected.rect.width() <= 160.0);
                assert!(table_row_height(ui) > label.rect.height());
            });
        }
    }

    /// 発散型（青-白-赤）は中央 t=0.5 がニュートラルな白になる。
    #[test]
    fn colormap_blue_white_red_midpoint_is_white() {
        assert_eq!(ColorMap::BlueWhiteRed.sample(0.5), WHITE);
    }

    /// 範囲外の値はクランプされる（全カラーマップ共通の `lut_sample` の挙動）。
    #[test]
    fn colormap_sample_clamps_out_of_range() {
        assert_eq!(
            ColorMap::Viridis.sample(-5.0),
            ColorMap::Viridis.sample(0.0)
        );
        assert_eq!(ColorMap::Viridis.sample(5.0), ColorMap::Viridis.sample(1.0));
    }

    /// 検定比のグラデーションは、判定境界で `status_color` と同じ色になる
    /// （良好域の上端＝緑、注意域の下端＝黄、NG＝赤）。
    #[test]
    fn check_ratio_color_matches_status_color_at_boundaries() {
        assert_eq!(check_ratio_color(0.8), GOOD_GREEN);
        assert_eq!(check_ratio_color(0.80001), BEST_YELLOW);
        assert_eq!(check_ratio_color(1.0), SECONDARY_AMBER);
        assert_eq!(check_ratio_color(1.0001), PARETO_RED);
        assert_eq!(check_ratio_color(5.0), PARETO_RED);
    }

    /// 良好域（≤0.8）は淡緑から緑へ連続的に変化する（同じ緑一色にはならない）。
    #[test]
    fn check_ratio_color_is_gradient_below_caution() {
        let c0 = check_ratio_color(0.0);
        let c4 = check_ratio_color(0.4);
        let c8 = check_ratio_color(0.8);
        assert_ne!(c0, c4);
        assert_ne!(c4, c8);
        assert!(c0.g() > c4.g());
        assert!(c4.g() > c8.g());
    }

    /// 負値は 0 側へクランプされる（下限の淡緑）。
    #[test]
    fn check_ratio_color_clamps_negative() {
        assert_eq!(check_ratio_color(-1.0), check_ratio_color(0.0));
    }
}
