//! 変形表示・スケール・未参照節点の変位補間。
//!
//! `viewer` ハブからの構造分割。アルゴリズム変更は行わない。

use crate::app::App;

use sepika_core::geom::vec3::dist as member_len3;

/// 変形図・モード形で梁の曲げ変形曲線を描く際の要素分割数（点数は +1）。
pub(super) const DEFORM_CURVE_SEGMENTS: usize = 12;

/// 梁要素の Hermite 3 次変形曲線を評価するための前処理データ。
///
/// 端部 6 自由度（節点変位、無倍率）を要素ローカル系へ一度だけ変換して保持し、
/// 材軸パラメータ ξ∈[0,1] での変位・曲線上の点を安価に評価する。曲線描画・応力図
/// の基準線・床節点の追従・変形スケール上限で共有し、「梁の変形後の形」の評価を
/// 一箇所へ集約する（ループでの `LocalFrame` 再構築も避ける）。
///
/// 軸方向は線形内挿、曲げ 2 面は Hermite 3 次形状関数で内挿する（等価節点力
/// [`sepika_element::frame::member_load`] と同一の形状関数・符号規約。局所 z 面は θy の
/// 符号反転）。ξ=0,1 では回転項が消え端点は節点変位に一致する。本内挿は表示専用
/// であり解析結果（節点変位・内力）は変更しない。要素はせん断変形を含む
/// Timoshenko 梁だが、変形図は Euler–Bernoulli の Hermite 曲線で近似する
/// （変形形状の可視化として実務上標準的）。
pub(super) struct BeamDeflection {
    /// 要素ローカル系（`rot` 行 = ex, ey, ez）。
    frame: sepika_element::transform::LocalFrame,
    /// 部材長。
    l: f64,
    /// 未変形材軸の始点・終点（グローバル）。
    p_i: [f64; 3],
    p_j: [f64; 3],
    /// i 端のローカル端部変位 `[ux, uy, uz, ry, rz]`。
    ui: [f64; 5],
    /// j 端のローカル端部変位 `[ux, uy, uz, ry, rz]`。
    uj: [f64; 5],
}

impl BeamDeflection {
    /// 端部変位 `d_i`, `d_j`（節点変位 6 成分、無倍率）から前処理する。
    pub(super) fn new(
        p_i: [f64; 3],
        p_j: [f64; 3],
        d_i: [f64; 6],
        d_j: [f64; 6],
        ref_vector: [f64; 3],
    ) -> Option<Self> {
        let l = member_len3(p_i, p_j);
        let frame = sepika_element::transform::LocalFrame::from_nodes(p_i, p_j, ref_vector).ok()?;
        let g = [
            d_i[0], d_i[1], d_i[2], d_i[3], d_i[4], d_i[5], d_j[0], d_j[1], d_j[2], d_j[3], d_j[4],
            d_j[5],
        ];
        let u = frame.rotate_to_local(&g);
        Some(Self {
            frame,
            l,
            p_i,
            p_j,
            ui: [u[0], u[1], u[2], u[4], u[5]],
            uj: [u[6], u[7], u[8], u[10], u[11]],
        })
    }

    /// 材軸パラメータ ξ での「未変形材軸上の点へ加えるグローバル並進変位」（無倍率）。
    /// 床・二次部材の節点を梁曲線へ載せる補間で用いる（描画曲線から浮かないよう
    /// 同じ評価を共有する）。
    fn disp_at(&self, xi: f64) -> [f64; 3] {
        let l = self.l;
        let n1 = 1.0 - 3.0 * xi * xi + 2.0 * xi * xi * xi;
        let n2 = l * (xi - 2.0 * xi * xi + xi * xi * xi);
        let n3 = 3.0 * xi * xi - 2.0 * xi * xi * xi;
        let n4 = l * (-xi * xi + xi * xi * xi);
        let [uxi, uyi, uzi, ryi, rzi] = self.ui;
        let [uxj, uyj, uzj, ryj, rzj] = self.uj;
        let ux = (1.0 - xi) * uxi + xi * uxj;
        let uy = n1 * uyi + n2 * rzi + n3 * uyj + n4 * rzj;
        let uz = n1 * uzi - n2 * ryi + n3 * uzj - n4 * ryj;
        let r = &self.frame.rot;
        [
            r[0][0] * ux + r[1][0] * uy + r[2][0] * uz,
            r[0][1] * ux + r[1][1] * uy + r[2][1] * uz,
            r[0][2] * ux + r[1][2] * uy + r[2][2] * uz,
        ]
    }

    /// 変形後曲線上の点（未変形材軸上の点 + 倍率付き変位）を ξ で返す。
    /// ξ=0,1 では端点＝節点変位（`scale` 倍）に厳密一致する（節点マーカーと連続）。
    pub(super) fn point_at(&self, xi: f64, scale: f64) -> [f64; 3] {
        let dg = self.disp_at(xi);
        [
            self.p_i[0] + (self.p_j[0] - self.p_i[0]) * xi + dg[0] * scale,
            self.p_i[1] + (self.p_j[1] - self.p_i[1]) * xi + dg[1] * scale,
            self.p_i[2] + (self.p_j[2] - self.p_i[2]) * xi + dg[2] * scale,
        ]
    }

    /// 変形後曲線を両端含む `segments + 1` 点の折れ線で返す（曲線描画用）。
    pub(super) fn polyline(&self, scale: f64, segments: usize) -> Vec<[f64; 3]> {
        let seg = segments.max(1);
        (0..=seg)
            .map(|k| self.point_at(k as f64 / seg as f64, scale))
            .collect()
    }
}

fn interpolate_unreferenced_disp(
    model: &sepika_core::model::Model,
    mut disp: Vec<[f64; 6]>,
    use_beam_hermite: bool,
) -> Vec<[f64; 6]> {
    let n = model.nodes.len().min(disp.len());

    let mut referenced = sepika_core::dof::structural_nodes(model);
    referenced.truncate(n);
    referenced.resize(n, false);
    if referenced.iter().all(|&r| r) {
        return disp;
    }

    struct AnchorSeg {
        a: usize,
        b: usize,
        beam: bool,
        ref_vec: [f64; 3],
    }
    let segments: Vec<AnchorSeg> = model
        .elements
        .iter()
        .filter(|e| e.nodes.len() == 2)
        .map(|e| AnchorSeg {
            a: e.nodes[0].index(),
            b: e.nodes[1].index(),
            beam: e.kind == sepika_core::model::ElementKind::Beam,
            ref_vec: e.local_axis.ref_vector,
        })
        .filter(|s| s.a < n && s.b < n)
        .collect();

    let anchor_tol = (model_bbox_size(model) * 1e-3).max(1e-9);

    let mut finalized = referenced.clone();
    let mut proj_disp = vec![[0.0_f64; 6]; n];
    let mut proj_ok = vec![false; n];
    for i in 0..n {
        if referenced[i] {
            continue;
        }
        let p = model.nodes[i].coord;
        let mut best: Option<(f64, usize, f64)> = None;
        for (si, s) in segments.iter().enumerate() {
            let pa = model.nodes[s.a].coord;
            let pb = model.nodes[s.b].coord;
            let ab = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
            let t = if len2 < 1e-12 {
                0.0
            } else {
                (((p[0] - pa[0]) * ab[0] + (p[1] - pa[1]) * ab[1] + (p[2] - pa[2]) * ab[2]) / len2)
                    .clamp(0.0, 1.0)
            };
            let q = [pa[0] + ab[0] * t, pa[1] + ab[1] * t, pa[2] + ab[2] * t];
            let d2 = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
            if best.is_none_or(|(bd, _, _)| d2 < bd) {
                best = Some((d2, si, t));
            }
        }
        if let Some((d2, si, t)) = best {
            let s = &segments[si];
            let (da, db) = (disp[s.a], disp[s.b]);
            let interp: [f64; 6] = if s.beam && use_beam_hermite {
                let Some(hermite) = BeamDeflection::new(
                    model.nodes[s.a].coord,
                    model.nodes[s.b].coord,
                    da,
                    db,
                    s.ref_vec,
                ) else {
                    continue;
                };
                let hermite = hermite.disp_at(t);
                std::array::from_fn(|k| match k {
                    0..=2 => hermite[k],
                    _ => da[k] * (1.0 - t) + db[k] * t,
                })
            } else {
                std::array::from_fn(|k| da[k] * (1.0 - t) + db[k] * t)
            };
            proj_disp[i] = interp;
            proj_ok[i] = true;
            if d2.sqrt() <= anchor_tol {
                disp[i] = interp;
                finalized[i] = true;
            }
        }
    }

    let mut sec_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for sm in model.beams().chain(model.posts()) {
        let Some([a, b]) = super::secondary_end_node_indices(model, sm) else {
            continue;
        };
        if a < n && b < n {
            sec_adj[a].push(b);
            sec_adj[b].push(a);
        }
    }
    let node_dist = |a: usize, b: usize| -> f64 {
        let pa = model.nodes[a].coord;
        let pb = model.nodes[b].coord;
        ((pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2)).sqrt()
    };

    let mut best_dist = vec![f64::INFINITY; n];
    let mut src_disp = vec![[0.0_f64; 6]; n];
    let mut has_source = vec![false; n];
    for u in 0..n {
        if !finalized[u] {
            continue;
        }
        for &j in &sec_adj[u] {
            if finalized[j] {
                continue;
            }
            let d = node_dist(u, j);
            if d < best_dist[j] {
                best_dist[j] = d;
                src_disp[j] = disp[u];
                has_source[j] = true;
            }
        }
    }
    loop {
        let mut pick: Option<(usize, f64)> = None;
        for i in 0..n {
            if finalized[i] || !has_source[i] {
                continue;
            }
            if pick.is_none_or(|(_, bd)| best_dist[i] < bd) {
                pick = Some((i, best_dist[i]));
            }
        }
        let Some((u, _)) = pick else { break };
        disp[u] = src_disp[u];
        finalized[u] = true;
        for &j in &sec_adj[u] {
            if finalized[j] {
                continue;
            }
            let d = node_dist(u, j);
            if d < best_dist[j] {
                best_dist[j] = d;
                src_disp[j] = disp[u];
                has_source[j] = true;
            }
        }
    }

    for i in 0..n {
        if !finalized[i] && proj_ok[i] {
            disp[i] = proj_disp[i];
        }
    }
    disp
}

/// 剛床代表節点（マスター）の鉛直変位（Uz）を、スレーブ節点の鉛直変位の平均で
/// 表示用に補う。あくまで描画専用の近似で、解析結果（`StaticOnce::disp`）は変更
/// しない。
///
/// マスターの面内自由度（Ux・Uy・Rz）は解析結果をそのまま使うため水平変形には
/// 追従するが、面外自由度（Uz・Rx・Ry）は零剛性による特異行列を避けるための数値
/// ダミー拘束で 0 に固定されている（`sepika-load` の `story_gen`）。そのままだと
/// 変形図で代表点だけが原標高に浮き、床の鉛直変形（重力たわみ・地震の転倒による
/// 床の上下動）へ追従しない。スレーブ節点の Uz 平均を代表点の Uz とすることで、
/// 代表点を変形後の床の平均標高へ載せる。
fn fill_diaphragm_master_disp_for_display(
    model: &sepika_core::model::Model,
    mut disp: Vec<[f64; 6]>,
) -> Vec<[f64; 6]> {
    let n = model.nodes.len().min(disp.len());
    for c in &model.constraints {
        let sepika_core::model::Constraint::RigidDiaphragm { master, slaves, .. } = c else {
            continue;
        };
        let mi = master.index();
        if mi >= n {
            continue;
        }
        let mut sum = 0.0_f64;
        let mut cnt = 0.0_f64;
        for sl in slaves {
            let si = sl.index();
            if si < n {
                sum += disp[si][2];
                cnt += 1.0;
            }
        }
        if cnt >= 0.5 {
            disp[mi][2] = sum / cnt;
        }
    }
    disp
}

/// 解析変位を表示用に加工する（いずれも描画専用の近似で、解析結果は変更しない）。
///
/// 1. 主架構に接続しない床・二次部材の節点を主架構の変形へ追従させる
///    （[`interpolate_unreferenced_disp`]。梁に載る節点は内部たわみ表示 ON なら
///    梁の Hermite 曲線上へ、OFF なら弦上へ）。
/// 2. 剛床代表節点の鉛直変位をスレーブ平均で補い、代表点を床の変形へ追従させる
///    （[`fill_diaphragm_master_disp_for_display`]）。
pub(super) fn display_disp(
    model: &sepika_core::model::Model,
    raw: Vec<[f64; 6]>,
    use_beam_hermite: bool,
) -> Vec<[f64; 6]> {
    let d = interpolate_unreferenced_disp(model, raw, use_beam_hermite);
    fill_diaphragm_master_disp_for_display(model, d)
}

/// 変形図の実効表示倍率（自動倍率 × 手動係数）を算定する。変位がない（`None`）・
/// 全並進成分がゼロなら 0 を返す（変形を描かない）。
///
/// 自動倍率は次の小さい方:
/// - **バウンディングボックス基準**: 最大並進変位がモデル対角長の 10% で表示される
///   倍率 `0.1 · model_size / δ_max`。
/// - **梁スパン基準**（`use_beam_interpolation` が真のときのみ）: 各梁の Hermite 内部
///   たわみがスパンの一定割合を超えない上限（[`beam_deflection_scale_limit`]）。
///   内部たわみ OFF（梁を直線で描く）ではふくらみが生じないため併用しない。
///
/// これに手動係数 `factor`（スライダー）を掛けた値を実効倍率とする。
pub(super) fn deform_display_scale(
    model: &sepika_core::model::Model,
    disp: Option<&[[f64; 6]]>,
    model_size: f64,
    use_beam_interpolation: bool,
    factor: f32,
) -> f64 {
    let Some(d) = disp else {
        return 0.0;
    };
    let max_disp = d
        .iter()
        .map(|v| v[0].abs().max(v[1].abs()).max(v[2].abs()))
        .fold(0.0_f64, f64::max);
    if max_disp <= 1e-12 {
        return 0.0;
    }
    let bbox_scale = model_size * 0.1 / max_disp;
    let auto = if use_beam_interpolation {
        beam_deflection_scale_limit(model, d).map_or(bbox_scale, |lim| bbox_scale.min(lim))
    } else {
        bbox_scale
    };
    auto * factor as f64
}

/// 時刻歴アニメーションの変形倍率キャッシュ（高-2）。
///
/// 通常の変形図（`deform_display_scale`）は現在フレームの変位から自動倍率を
/// 算定するため、時刻歴アニメーションへそのまま適用すると振幅の小さいフレームで
/// 倍率が発散し、逆に無変形（初期状態）フレームでは 0 になって表示が消える。
/// 記録全体のピーク変位から 1 回だけ算定した固定倍率を使うことでこれを避ける。
///
/// `auto_scale` は手動係数（`App::deform_scale_factor`）を掛ける前の自動倍率。
/// 記録の同一性は「フレーム数＋ピーク変位」で判定する（解析をやり直すと
/// フレーム数かピーク値のいずれかが変わるため、それで十分にキャッシュを無効化できる）。
/// モデルサイズ・内部たわみ表示 ON/OFF が変わった場合も再計算する。
#[derive(Clone, Copy, Debug, Default)]
pub struct TimeHistoryScaleCache {
    n_frames: usize,
    peak_max_disp: f64,
    model_size: f64,
    use_beam_interpolation: bool,
    auto_scale: f64,
}

/// `ResponseResult::peak_disp`（全ステップ間引きなしのピーク変位、節点×6成分）から、
/// 並進成分（ux/uy/uz）の絶対値最大を求める（純粋関数）。
fn th_peak_translation_disp(result: &sepika_solver::dynamic::timehistory::ResponseResult) -> f64 {
    result
        .peak_disp
        .iter()
        .map(|d| d[0].abs().max(d[1].abs()).max(d[2].abs()))
        .fold(0.0_f64, f64::max)
}

/// 時刻歴アニメーションの実効表示倍率（自動倍率 × 手動係数）。
/// `app.ui.view.th_scale_cache` を記録の同一性で使い回し、フレーム切替のたびに
/// 自動倍率を再計算しない（高-2）。時刻歴の詳細記録・結果がなければ 0。
pub(super) fn time_history_deform_scale_with_view(
    view: &mut crate::viewer::ViewportState,
    app: &mut App,
    model_size: f64,
) -> f64 {
    let Some(result) = view.spatial_result(app) else {
        view.th_scale_cache = None;
        return 0.0;
    };
    let n_frames = result.recording.as_ref().map_or(0, |r| r.frame_time.len());
    let peak_max_disp = th_peak_translation_disp(result);
    let use_beam_interpolation = view.show_beam_interpolation;

    let reuse = view.th_scale_cache.is_some_and(|c| {
        c.n_frames == n_frames
            && c.peak_max_disp == peak_max_disp
            && c.model_size == model_size
            && c.use_beam_interpolation == use_beam_interpolation
    });
    let auto_scale = if reuse {
        view.th_scale_cache.expect("reuse implies Some").auto_scale
    } else {
        let auto = deform_display_scale(
            &app.core.model,
            Some(&result.peak_disp),
            model_size,
            use_beam_interpolation,
            1.0,
        );
        view.th_scale_cache = Some(TimeHistoryScaleCache {
            n_frames,
            peak_max_disp,
            model_size,
            use_beam_interpolation,
            auto_scale: auto,
        });
        auto
    };
    auto_scale * view.deform_scale_factor as f64
}

/// 梁のスパンに対する内部たわみが過大にならないよう、表示倍率の上限を算定する。
/// 制約する梁がなければ `None`。
///
/// 変形図の梁は端部 6 自由度からの Hermite 3 次曲線で描くため、端部回転が大きいと
/// 中央のふくらみ（変形後両端を結ぶ弦からの逸脱）がスパンに対して過大になり得る。
/// 各梁について無倍率のたわみ（弦からの最大逸脱）を評価し、
/// `倍率 × たわみ ≤ FRAC × スパン` を満たす倍率上限 `FRAC × スパン / たわみ` の
/// 最小値を返す。バウンディングボックス基準の倍率と併せて小さい方を採ることで、
/// 全体変形も梁のふくらみも過大にならないスケールにする。
fn beam_deflection_scale_limit(
    model: &sepika_core::model::Model,
    disp: &[[f64; 6]],
) -> Option<f64> {
    /// 梁の内部たわみ（弦からの逸脱）が許容されるスパン比。
    const BEAM_DEFLECTION_DISPLAY_FRAC: f64 = 0.1;
    /// たわみ評価の内部サンプル点数（両端を除く分割）。
    const SAMPLES: usize = 9;

    let n = model.nodes.len().min(disp.len());
    let mut limit: Option<f64> = None;
    for elem in &model.elements {
        if elem.kind != sepika_core::model::ElementKind::Beam || elem.nodes.len() != 2 {
            continue;
        }
        let a = elem.nodes[0].index();
        let b = elem.nodes[1].index();
        if a >= n || b >= n {
            continue;
        }
        let p_i = model.nodes[a].coord;
        let p_j = model.nodes[b].coord;
        let l = member_len3(p_i, p_j);
        if l < 1e-9 {
            continue;
        }
        let (d_i, d_j) = (disp[a], disp[b]);
        let Some(bd) = BeamDeflection::new(p_i, p_j, d_i, d_j, elem.local_axis.ref_vector) else {
            continue;
        };
        let mut max_dev = 0.0_f64;
        for k in 1..SAMPLES {
            let xi = k as f64 / SAMPLES as f64;
            let h = bd.disp_at(xi);
            let dev = ((h[0] - (d_i[0] * (1.0 - xi) + d_j[0] * xi)).powi(2)
                + (h[1] - (d_i[1] * (1.0 - xi) + d_j[1] * xi)).powi(2)
                + (h[2] - (d_i[2] * (1.0 - xi) + d_j[2] * xi)).powi(2))
            .sqrt();
            max_dev = max_dev.max(dev);
        }
        if max_dev > 1e-12 {
            let lim = BEAM_DEFLECTION_DISPLAY_FRAC * l / max_dev;
            limit = Some(limit.map_or(lim, |cur: f64| cur.min(lim)));
        }
    }
    limit
}

/// モデルのバウンディングボックス（min, max）。空なら原点を返す。
pub(super) fn model_bbox(model: &sepika_core::model::Model) -> ([f64; 3], [f64; 3]) {
    if model.nodes.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    for n in &model.nodes {
        for k in 0..3 {
            min[k] = min[k].min(n.coord[k]);
            max[k] = max[k].max(n.coord[k]);
        }
    }
    (min, max)
}

pub(super) fn frame_bbox(
    model: &sepika_core::model::Model,
    frame: &sepika_core::frame::Frame,
) -> Option<([f64; 3], [f64; 3])> {
    let mut min = [f64::MAX; 3];
    let mut max = [f64::MIN; 3];
    let mut found = false;
    for (i, e) in model.elements.iter().enumerate() {
        if !frame.elem_on.get(i).copied().unwrap_or(false) {
            continue;
        }
        for nid in &e.nodes {
            let Some(n) = model.nodes.get(nid.index()) else {
                continue;
            };
            found = true;
            for k in 0..3 {
                min[k] = min[k].min(n.coord[k]);
                max[k] = max[k].max(n.coord[k]);
            }
        }
    }
    found.then_some((min, max))
}

/// バウンディングボックスの対角線長。
pub(super) fn bbox_diagonal(min: [f64; 3], max: [f64; 3]) -> f64 {
    let d =
        ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt();
    if d > 1e-9 {
        d
    } else {
        1.0
    }
}

/// モデルのバウンディングボックス対角線長。
pub(super) fn model_bbox_size(model: &sepika_core::model::Model) -> f64 {
    if model.nodes.is_empty() {
        return 1.0;
    }
    let (min, max) = model_bbox(model);
    ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2)).sqrt()
}
#[cfg(test)]
mod tests {
    use super::*;
    use sepika_core::dof::Dof6Mask;
    use sepika_core::ids::{ElemId, NodeId};
    use sepika_core::model::{
        ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Model, Node,
        SecondaryMember, SecondaryMemberKind,
    };

    /// 補間テスト用の節点を作る（拘束なし・付加情報なし）。
    fn test_node(id: u32, coord: [f64; 3]) -> Node {
        Node {
            id: NodeId(id),
            coord,
            restraint: Dof6Mask::FREE,
            mass: None,
            story: None,
            support_spring: None,
        }
    }

    /// 補間テスト用の二次部材（小梁）を作る。両端は節点 `i`・`j` の座標。
    fn test_secondary(model: &Model, i: u32, j: u32) -> SecondaryMember {
        SecondaryMember {
            id: sepika_core::ids::SecondaryMemberId(i),
            vertical_eccentricity: Some(0.0),
            gravity_end_shares: None,
            kind: SecondaryMemberKind::Beam,
            ends: sepika_core::model::SecondaryMemberEnds::Detached([
                model.nodes[i as usize].coord,
                model.nodes[j as usize].coord,
            ]),
            section: None,
            name: String::new(),
        }
    }

    /// 補間テスト用の 2 節点梁要素を作る。
    fn test_beam(id: u32, i: u32, j: u32) -> ElementData {
        ElementData {
            id: ElemId(id),
            kind: ElementKind::Beam,
            nodes: [NodeId(i), NodeId(j)].into_iter().collect(),
            section: None,
            local_axis: LocalAxis {
                ref_vector: [0.0, 0.0, 1.0],
            },
            end_cond: [EndCondition::Fixed, EndCondition::Fixed],
            force_regime: ForceRegime::Auto,
            rigid_zone: Default::default(),
            vertical_eccentricity: Some(0.0),
            plastic_zone: None,
            spring: None,
        }
    }

    #[test]
    fn 主架構に接続する節点の変位は補間で変更されない() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));

        let disp = vec![
            [1.0, 2.0, 3.0, 0.1, 0.2, 0.3],
            [4.0, 5.0, 6.0, 0.4, 0.5, 0.6],
        ];
        let out = interpolate_unreferenced_disp(&model, disp.clone(), true);
        assert_eq!(out, disp);
    }

    #[test]
    fn 大梁スパン中間の未参照節点は梁のエルミート変位で追従する() {
        // 梁はHermite曲線で描くため、直付き節点を線形補間すると描画曲線から離れる。
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [8000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [2000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));

        let disp = vec![
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [4.0, 8.0, -12.0, 0.0, 0.0, 0.0],
            [0.0; 6],
        ];
        let out = interpolate_unreferenced_disp(&model, disp, true);
        // t = 2000/8000 = 0.25。端部回転は 0 のため、軸方向（+X）は線形（0.25·4=1.0）、
        // 材軸直交成分（Y,Z）は Hermite の N3(0.25)=0.15625 倍で追従する
        // （線形補間の 0.25 倍より小さく、梁の描画曲線上に載る）。
        assert!((out[2][0] - 1.0).abs() < 1e-12, "X={}", out[2][0]);
        assert!((out[2][1] - 8.0 * 0.15625).abs() < 1e-12, "Y={}", out[2][1]);
        assert!(
            (out[2][2] + 12.0 * 0.15625).abs() < 1e-12,
            "Z={}",
            out[2][2]
        );
    }

    #[test]
    fn 大梁スパン中間の未参照節点は内部たわみオフで線形補間になる() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [8000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [2000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));

        let disp = vec![
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [4.0, 8.0, -12.0, 0.0, 0.0, 0.0],
            [0.0; 6],
        ];
        let out = interpolate_unreferenced_disp(&model, disp, false);
        assert!((out[2][0] - 1.0).abs() < 1e-12, "X={}", out[2][0]);
        assert!((out[2][1] - 2.0).abs() < 1e-12, "Y={}", out[2][1]);
        assert!((out[2][2] + 3.0).abs() < 1e-12, "Z={}", out[2][2]);
    }

    #[test]
    fn 梁上の未参照節点は梁の描画曲線に厳密一致する() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [3000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));

        let d_i = [0.0, 0.0, 0.0, 0.0, 0.0, 0.01];
        let d_j = [0.0, 0.0, 0.0, 0.0, 0.0, -0.01];
        let disp = vec![d_i, d_j, [0.0; 6]];
        let out = interpolate_unreferenced_disp(&model, disp, true);

        let poly = BeamDeflection::new(
            [0.0, 0.0, 0.0],
            [6000.0, 0.0, 0.0],
            d_i,
            d_j,
            [0.0, 0.0, 1.0],
        )
        .unwrap()
        .polyline(1.0, 12);
        let curve_disp = [poly[6][0] - 3000.0, poly[6][1] - 0.0, poly[6][2] - 0.0];
        for k in 0..3 {
            assert!(
                (out[2][k] - curve_disp[k]).abs() < 1e-9,
                "軸 {k}: 補間 {} と曲線 {} が不一致",
                out[2][k],
                curve_disp[k]
            );
        }
        assert!(
            out[2][1].abs() > 1.0,
            "Hermite たわみが出ていない: {}",
            out[2][1]
        );
    }

    #[test]
    fn 梁軸から外れた未参照節点も最寄り線分の射影位置で補間される() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [4000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [2000.0, 500.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));

        let disp = vec![[0.0; 6], [10.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.0; 6]];
        let out = interpolate_unreferenced_disp(&model, disp, true);
        assert!((out[2][0] - 5.0).abs() < 1e-12);
    }

    #[test]
    fn 主架構の線材がなければ未参照節点の変位はゼロのまま() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [1000.0, 0.0, 0.0]));
        let out = interpolate_unreferenced_disp(&model, vec![[0.0; 6]; 2], true);
        assert!(out.iter().all(|v| v.iter().all(|&x| x == 0.0)));
    }

    #[test]
    fn 剛床マスター節点の変位は補間で上書きされない() {
        // 剛床マスターは要素に非接続でも解析自由度を持つため、補間で変位を上書きできない。
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [8000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [4000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        model
            .constraints
            .push(sepika_core::model::Constraint::rigid_diaphragm(
                sepika_core::ids::StoryId(0),
                NodeId(2),
                vec![NodeId(0), NodeId(1)],
            ));

        let disp = vec![
            [0.0; 6],
            [10.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [7.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ];
        let out = interpolate_unreferenced_disp(&model, disp.clone(), true);
        assert_eq!(out, disp);
    }

    #[test]
    fn 変形曲線の端部は節点変位に一致する() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d_i = [0.0, 1.0, 0.0, 0.0, 0.0, 0.001];
        let d_j = [2.0, 3.0, 0.0, 0.0, 0.0, -0.002];
        let scale = 2.0;
        let poly = BeamDeflection::new(p_i, p_j, d_i, d_j, [0.0, 0.0, 1.0])
            .unwrap()
            .polyline(scale, 12);
        assert_eq!(poly.len(), 13);
        for k in 0..3 {
            assert!(
                (poly[0][k] - (p_i[k] + scale * d_i[k])).abs() < 1e-6,
                "i端 axis{k}: {}",
                poly[0][k]
            );
            assert!(
                (poly[12][k] - (p_j[k] + scale * d_j[k])).abs() < 1e-6,
                "j端 axis{k}: {}",
                poly[12][k]
            );
        }
    }

    #[test]
    fn 不正な参照方向の梁曲線は生成しない() {
        assert!(BeamDeflection::new(
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [0.0; 6],
            [0.0; 6],
            [f64::NAN, 0.0, 0.0],
        )
        .is_none());
    }

    #[test]
    fn 端部回転で中央がたわむ() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [1000.0, 0.0, 0.0];
        let d_i = [0.0, 0.0, 0.0, 0.0, 0.0, 0.01];
        let d_j = [0.0, 0.0, 0.0, 0.0, 0.0, -0.01];
        let poly = BeamDeflection::new(p_i, p_j, d_i, d_j, [0.0, 1.0, 0.0])
            .unwrap()
            .polyline(1.0, 12);
        let mid = poly[6];
        assert!((mid[0] - 500.0).abs() < 1e-6, "中央 x={}", mid[0]);
        assert!(
            mid[1].abs() > 1.0,
            "中央のたわみが小さすぎる: dy={}",
            mid[1]
        );
        assert!(poly[0][1].abs() < 1e-9 && poly[12][1].abs() < 1e-9);
    }

    #[test]
    fn 梁変形後曲線の端点は節点変位に一致し中央は弦から外れる() {
        let p_i = [0.0, 0.0, 0.0];
        let p_j = [6000.0, 0.0, 0.0];
        let d_i = [0.0, 0.0, 0.0, 0.0, 0.0, 0.01];
        let d_j = [0.0, 0.0, 0.0, 0.0, 0.0, -0.01];
        let scale = 2.0;
        let bd = BeamDeflection::new(p_i, p_j, d_i, d_j, [0.0, 0.0, 1.0]).unwrap();
        let a = bd.point_at(0.0, scale);
        let b = bd.point_at(1.0, scale);
        for k in 0..3 {
            assert!(
                (a[k] - (p_i[k] + scale * d_i[k])).abs() < 1e-6,
                "端点i k={k}"
            );
            assert!(
                (b[k] - (p_j[k] + scale * d_j[k])).abs() < 1e-6,
                "端点j k={k}"
            );
        }
        let mid = bd.point_at(0.5, scale);
        let chord_mid = [
            (a[0] + b[0]) * 0.5,
            (a[1] + b[1]) * 0.5,
            (a[2] + b[2]) * 0.5,
        ];
        let dev = ((mid[0] - chord_mid[0]).powi(2)
            + (mid[1] - chord_mid[1]).powi(2)
            + (mid[2] - chord_mid[2]).powi(2))
        .sqrt();
        assert!(dev > 1.0, "中央が弦から外れていない: dev={}", dev);
    }

    #[test]
    fn 大梁に直付きしない二次部材の先端は接続先を辿って追従する() {
        // 最寄り線分だけなら先端が別梁G2へ追従してしまう配置を使う。
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [8000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [2000.0, 0.0, 0.0]));
        model.nodes.push(test_node(3, [2000.0, 4000.0, 0.0]));
        model.nodes.push(test_node(4, [0.0, 5000.0, 0.0]));
        model.nodes.push(test_node(5, [8000.0, 5000.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        model.elements.push(test_beam(1, 4, 5));
        model.unassigned_beams.push(test_secondary(&model, 2, 3));

        let disp = vec![
            [0.0; 6],
            [100.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0; 6],
            [0.0; 6],
            [0.0; 6],
            [0.0; 6],
        ];
        let out = interpolate_unreferenced_disp(&model, disp, true);
        assert!((out[2][0] - 25.0).abs() < 1e-9, "node2={:?}", out[2]);
        assert!((out[3][0] - 25.0).abs() < 1e-9, "node3={:?}", out[3]);
    }

    #[test]
    fn 二次部材の連鎖でも主架構に近い側から順に追従する() {
        // 単純射影なら終端が別梁G2へ追従してしまうため、連鎖の必要な配置を使う。
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [4000.0, 0.0, 0.0]));
        model.nodes.push(test_node(2, [4000.0, 2000.0, 0.0]));
        model.nodes.push(test_node(3, [4000.0, 4000.0, 0.0]));
        model.nodes.push(test_node(4, [0.0, 5000.0, 0.0]));
        model.nodes.push(test_node(5, [8000.0, 5000.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        model.elements.push(test_beam(1, 4, 5));
        model.unassigned_beams.push(test_secondary(&model, 1, 2));
        model.unassigned_beams.push(test_secondary(&model, 2, 3));

        let disp = vec![
            [8.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [8.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            [0.0; 6],
            [0.0; 6],
            [0.0; 6],
            [0.0; 6],
        ];
        let out = interpolate_unreferenced_disp(&model, disp, true);
        assert!((out[2][0] - 8.0).abs() < 1e-9, "node2={:?}", out[2]);
        assert!((out[3][0] - 8.0).abs() < 1e-9, "node3={:?}", out[3]);
    }

    #[test]
    fn 剛床マスターの鉛直変位はスレーブ平均で補完される() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 3000.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 3000.0]));
        model.nodes.push(test_node(2, [3000.0, 0.0, 3000.0]));
        model
            .constraints
            .push(sepika_core::model::Constraint::rigid_diaphragm(
                sepika_core::ids::StoryId(0),
                NodeId(2),
                vec![NodeId(0), NodeId(1)],
            ));
        let disp = vec![
            [1.0, 0.0, -4.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, -6.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0, 0.0, 0.02],
        ];
        let out = fill_diaphragm_master_disp_for_display(&model, disp);
        // Uz は (-4 + -6)/2 = -5 に補完される。
        assert!((out[2][2] + 5.0).abs() < 1e-12, "Uz={}", out[2][2]);
        assert!((out[2][0] - 1.0).abs() < 1e-12, "Ux={}", out[2][0]);
        assert!((out[2][5] - 0.02).abs() < 1e-12, "Rz={}", out[2][5]);
    }

    #[test]
    fn 梁の内部たわみで変形スケール上限が算定される() {
        // 端部に等・逆回転（θz=±0.01）を与えた L=6000 の梁。両端並進 0 のため弦は
        // 直線で、弦からの逸脱＝Hermite たわみ w(ξ)=0.01·L·ξ(1−ξ)。9 分割の内部
        // サンプルでの最大は ξ=4/9,5/9 の 0.01·6000·(20/81)。
        // 上限 = 0.1·L / w_max = 0.1·6000·81 / (0.01·6000·20) = 40.5。
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        let disp = vec![
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.01],
            [0.0, 0.0, 0.0, 0.0, 0.0, -0.01],
        ];
        let limit = beam_deflection_scale_limit(&model, &disp).expect("上限が算定される");
        assert!((limit - 40.5).abs() < 1e-9, "limit={}", limit);
    }

    #[test]
    fn 変位ゼロなら梁スケール上限は無し() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        let disp = vec![[0.0; 6], [0.0; 6]];
        assert!(beam_deflection_scale_limit(&model, &disp).is_none());
    }

    #[test]
    fn 表示倍率は変位なし又は全ゼロでゼロ() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [10000.0, 0.0, 0.0]));
        let size = model_bbox_size(&model);
        assert_eq!(deform_display_scale(&model, None, size, true, 1.0), 0.0);
        let zero = vec![[0.0; 6], [0.0; 6]];
        assert_eq!(
            deform_display_scale(&model, Some(&zero), size, true, 1.0),
            0.0
        );
    }

    #[test]
    fn 内部たわみオフの表示倍率はbox基準に手動係数を掛ける() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [10000.0, 0.0, 0.0]));
        let disp = vec![[0.0; 6], [100.0, 0.0, 0.0, 0.0, 0.0, 0.0]];
        let size = model_bbox_size(&model);
        // box 基準 = 0.1·10000 / 100 = 10、手動係数 2 → 20。
        let s = deform_display_scale(&model, Some(&disp), size, false, 2.0);
        assert!((s - 20.0).abs() < 1e-9, "s={}", s);
    }

    #[test]
    fn 内部たわみオンは梁スパン上限で倍率が制限される() {
        let mut model = Model::default();
        model.nodes.push(test_node(0, [0.0, 0.0, 0.0]));
        model.nodes.push(test_node(1, [6000.0, 0.0, 0.0]));
        model.elements.push(test_beam(0, 0, 1));
        // 端部回転で内部たわみを生み、並進は微小にして box 基準を大きくする。
        let disp = vec![
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.01],
            [0.001, 0.0, 0.0, 0.0, 0.0, -0.01],
        ];
        let size = model_bbox_size(&model);
        let on = deform_display_scale(&model, Some(&disp), size, true, 1.0);
        let off = deform_display_scale(&model, Some(&disp), size, false, 1.0);
        // OFF は box 基準のみ、ON は梁スパン上限（前掲テストの 40.5）も併用。
        assert!(on < off, "on={on} off={off}");
        assert!((on - 40.5).abs() < 1e-6, "on={on}");
    }
}
