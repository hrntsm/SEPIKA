//! 層指標（二次設計チェック）とレポート文字列の生成。GUI 非依存。

use sepika_core::model::Model;
use sepika_core::units::to_display::{force_kn, length_m};
use sepika_design_jp::secondary::eccentricity_analysis::story_eccentricity_from_analysis;
use sepika_design_jp::secondary::holding_capacity::{eccentricity_ratio, fes, stiffness_ratios};
use sepika_design_jp::secondary::stiffness_ratio::{
    cog_story_drifts, max_column_drift, ColumnDrift,
};
use sepika_solver::statics::analysis::SeismicDir;
use sepika_solver::statics::linear::StaticOnce;

use crate::app::{App, ResultsBundle, StaticCaseKey, StaticKey};

/// 層ごとの二次設計指標（層間変形角・剛性率・偏心率・Fes）。
#[derive(Clone, Debug)]
pub struct StoryMetric {
    pub name: String,
    /// 階高 `[mm]`
    pub height: f64,
    /// 最大角を与える対象。Ok(None)は対象なし、Errは未計算又は内部不整合。
    pub drift: Result<Option<StoryDrift>, String>,
    /// 設定値との参考比較の分母。法令適合の判定条件ではない。
    pub drift_limit_denom: f64,
    /// 設定値との参考比較。対象なし・未計算・内部不整合・設定不正はNone。
    pub drift_ok: Option<bool>,
    /// 剛性率 Rs。算定不能時は対象と理由。
    pub rs: Result<f64, String>,
    /// Rs ≥ 0.6 か（令82条の6）。算定不能時は `None`。
    pub rs_ok: Option<bool>,
    /// 偏心率 Re（加力方向）。算定不能時は対象と理由。
    pub re: Result<f64, String>,
    /// Re ≤ 0.15 か（令82条の6）。算定不能時は `None`。
    pub re_ok: Option<bool>,
    /// 形状係数 Fes = Fs·Fe。Rs 又は Re が算定不能なら同じ理由を返す。
    pub fes: Result<f64, String>,
}

/// 同じ対象から得た変位差・柱頭位置・FL間高さによる最大層間変形角。
#[derive(Clone, Debug)]
pub struct StoryDrift {
    pub column: ColumnDrift,
    /// 上下階FL間高さ `[mm]`。
    pub height_mm: f64,
    /// 水平変位差の絶対値 / FL間高さ（無次元）。
    pub angle: f64,
}

/// 層指標算定の追加入力（偏心率の精算・重心の長期軸力算定用）。
/// X/Y のいずれかがない場合は偏心率と Fes を算定できない。
#[derive(Default, Clone)]
pub struct StoryMetricsCtx<'a> {
    /// X 方向加力の弾性応力解析結果（剛心の精算 ki=Qi/δi 用）
    pub seismic_x: Option<&'a StaticOnce>,
    /// Y 方向加力の弾性応力解析結果（同上）
    pub seismic_y: Option<&'a StaticOnce>,
    /// 長期応力解析結果（重心の長期軸力算定用）
    pub long_term: Option<&'a StaticOnce>,
    /// 層間変形角・Rsに用いる選択方向の結果生成入力・計算契約の不整合。
    pub drift_error: Option<String>,
    /// Reに用いるX/Y・採用長期結果の欠測、生成入力・計算契約の不整合。
    pub re_error: Option<String>,
}

fn metrics_long_term_result<'a>(
    model: &Model,
    results: Option<&'a ResultsBundle>,
) -> Option<&'a (String, StaticOnce)> {
    results?
        .combos
        .iter()
        .find(|(name, _)| sepika_core::load_combo::is_gravity_combination(name, model))
}

/// 解析結果一式から `StoryMetricsCtx` を組み立てる。
/// 長期は保存された常時G+P組合せ結果の先頭を用い、なければNone。
pub fn metrics_ctx_from_results<'a>(
    model: &Model,
    results: Option<&'a ResultsBundle>,
) -> StoryMetricsCtx<'a> {
    let Some(r) = results else {
        return StoryMetricsCtx::default();
    };
    let find_seismic = |dir: SeismicDir| {
        r.statics
            .iter()
            .find(|(k, _)| *k == StaticCaseKey::Seismic(dir))
            .map(|(_, s)| s)
    };
    let long_term = metrics_long_term_result(model, Some(r)).map(|(_, s)| s);
    StoryMetricsCtx {
        seismic_x: find_seismic(SeismicDir::X),
        seismic_y: find_seismic(SeismicDir::Y),
        long_term,
        drift_error: None,
        re_error: None,
    }
}

/// 階ごとの層間変形角 `[rad]`・Rs・Re・Fes を返す。階が未定義なら空となる。
/// Rs は対応方向、Re は X/Y 双方の有効な地震静的結果が必要。
/// Rs・Re・Fes の算定不能時は対象と理由を返し、対応する判定は `None` となる。
pub fn compute_story_metrics_with(
    model: &Model,
    dir: SeismicDir,
    ctx: &StoryMetricsCtx<'_>,
) -> Vec<StoryMetric> {
    let layers = model.layers();
    if layers.is_empty() {
        return Vec::new();
    }
    let d = match dir {
        SeismicDir::X => 0,
        SeismicDir::Y => 1,
    };

    let seismic = match dir {
        SeismicDir::X => ctx.seismic_x,
        SeismicDir::Y => ctx.seismic_y,
    };
    let metric_disp = seismic.map(|s| s.disp.as_slice()).unwrap_or(&[]);
    let heights: Vec<_> = layers.iter().map(|l| l.height).collect();

    let rs_all = if let Some(reason) = &ctx.drift_error {
        Err(reason.clone())
    } else if seismic.is_none() {
        Err(format!(
            "対応する {:?} 方向の地震静的解析結果がありません",
            dir
        ))
    } else {
        let rs_disp = if model.stress_cfg.misc_wall_n.is_some() {
            seismic
                .and_then(|s| s.stiffness_ratio_disp.as_ref())
                .ok_or_else(|| "自立壁を考慮したRs評価用再解析結果がありません".to_string())
                .and_then(|result| result.as_deref().map_err(Clone::clone))
        } else {
            Ok(metric_disp)
        };
        rs_disp
            .and_then(|disp| cog_story_drifts(model, disp, d))
            .and_then(|drifts| stiffness_ratios(&heights, &drifts))
    };

    let denom = model.stress_cfg.drift_limit_denom;

    layers
        .iter()
        .map(|l| {
            let i = l.index;
            let re = if let Some(reason) = &ctx.re_error {
                Err(reason.clone())
            } else {
                match (ctx.seismic_x, ctx.seismic_y) {
                    (Some(rx), Some(ry)) => {
                        story_eccentricity_from_analysis(model, l.top, rx, ry, ctx.long_term)
                            .and_then(|ecc| {
                                let (e_dist, radius) = match dir {
                                    SeismicDir::X => (ecc.ey, ecc.rex),
                                    SeismicDir::Y => (ecc.ex, ecc.rey),
                                };
                                eccentricity_ratio(e_dist, radius)
                            })
                    }
                    _ => Err("偏心率に必要な X/Y 方向の地震静的解析結果がありません".into()),
                }
            }
            .map_err(|reason| format!("{} {:?}: Re 算定不能: {reason}", l.name, dir));
            let rs = rs_all
                .as_ref()
                .map_err(Clone::clone)
                .and_then(|values| {
                    values
                        .get(i)
                        .copied()
                        .ok_or_else(|| "対応する層の剛性率がありません".into())
                })
                .map_err(|reason| format!("{} {:?}: Rs 算定不能: {reason}", l.name, dir));
            let fes = rs
                .clone()
                .and_then(|rs| re.clone().and_then(|re| fes(rs, re)));
            let rs_ok = rs.as_ref().ok().map(|rs| *rs >= 0.6);
            let re_ok = re.as_ref().ok().map(|re| *re <= 0.15);
            let drift = if let Some(reason) = &ctx.drift_error {
                Err(format!("内部不整合: {reason}"))
            } else if seismic.is_none() {
                Err(format!("未計算: 地震静的 {dir:?} の解析結果がありません"))
            } else if !l.height.is_finite() || l.height <= 0.0 {
                Err("内部不整合: FL間高さが正の有限値ではありません".into())
            } else {
                max_column_drift(model, metric_disp, d, l.top)
                    .and_then(|column| {
                        column
                            .map(|column| {
                                let angle = column.drift / l.height;
                                if !angle.is_finite() {
                                    return Err(format!(
                                        "柱{}の層間変形角が非有限です",
                                        column.elem.0
                                    ));
                                }
                                Ok(StoryDrift {
                                    column,
                                    height_mm: l.height,
                                    angle,
                                })
                            })
                            .transpose()
                    })
                    .map_err(|reason| format!("内部不整合: {reason}"))
            };
            let drift_ok = drift
                .as_ref()
                .ok()
                .and_then(|v| v.as_ref())
                .filter(|_| denom.is_finite() && denom > 0.0 && (1.0 / denom).is_finite())
                .map(|v| v.angle <= 1.0 / denom);
            StoryMetric {
                name: l.name.clone(),
                height: heights[i],
                drift,
                drift_limit_denom: denom,
                drift_ok,
                rs,
                rs_ok,
                re,
                re_ok,
                fes,
            }
        })
        .collect()
}

/// Rsは選択方向、ReはX/Yと採用長期結果の生成入力・計算契約を照合して集計する。
pub fn seismic_story_metrics(app: &App) -> Vec<StoryMetric> {
    use crate::app::ResultInputKey;
    let dir = app.core.analysis_cfg.seismic_dir;
    let mut ctx = metrics_ctx_from_results(&app.core.model, app.core.scoped.results.as_ref());
    let seismic_error = |direction, result: Option<&StaticOnce>| {
        if result.is_none() {
            Some(format!("地震静的 {direction:?} の解析結果がありません"))
        } else {
            app.require_result_input(ResultInputKey::Static(StaticCaseKey::Seismic(direction)))
                .err()
                .map(|reason| format!("地震静的 {direction:?}: {reason}"))
        }
    };
    let x_error = seismic_error(SeismicDir::X, ctx.seismic_x);
    let y_error = seismic_error(SeismicDir::Y, ctx.seismic_y);
    ctx.drift_error = match dir {
        SeismicDir::X => ctx.seismic_x.and(x_error.clone()),
        SeismicDir::Y => ctx.seismic_y.and(y_error.clone()),
    };
    let long_term_error =
        metrics_long_term_result(&app.core.model, app.core.scoped.results.as_ref()).and_then(
            |(name, _)| {
                app.require_result_input(ResultInputKey::Combo(name.clone()))
                    .err()
                    .map(|reason| format!("長期組合せ「{name}」: {reason}"))
            },
        );
    let reasons: Vec<_> = [x_error, y_error, long_term_error]
        .into_iter()
        .flatten()
        .collect();
    ctx.re_error = (!reasons.is_empty()).then(|| reasons.join(" / "));
    compute_story_metrics_with(&app.core.model, dir, &ctx)
}

pub(crate) fn drift_cells(m: &StoryMetric) -> [String; 6] {
    match &m.drift {
        Ok(Some(value)) => [
            "算定済み".into(),
            value.column.elem.0.to_string(),
            format!("x={:.3}; y={:.3}", value.column.pos[0], value.column.pos[1]),
            format!("{:.3}", value.column.drift),
            format!("{:.9}", value.angle),
            m.drift_ok
                .map(metric_verdict_text_bool)
                .unwrap_or_else(|| "比較なし: 比較分母が正の有限値ではありません".into()),
        ],
        Ok(None) => [
            "対象なし".into(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "比較なし".into(),
        ],
        Err(reason) => [
            reason.clone(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            "比較なし".into(),
        ],
    }
}

fn metric_verdict_text_bool(ok: bool) -> String {
    if ok {
        "OK".into()
    } else {
        "NG".into()
    }
}

fn csv_cell(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub(crate) fn metric_value_text(value: &Result<f64, String>) -> String {
    match value {
        Ok(value) => format!("{value:.3}"),
        Err(reason) => format!("算定不能: {}", reason.replace(',', ";").replace('\n', " ")),
    }
}

pub(crate) fn metric_verdict_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "OK",
        Some(false) => "NG",
        None => "判定なし",
    }
}

/// 解析・検定結果を CSV テキストにまとめる（レポートタブの出力）。
pub fn build_report_csv(app: &App) -> String {
    let mut out = String::new();
    let model = &app.core.model;

    out.push_str("# SEPIKA レポート\n");
    out.push_str("\n[モデル概要]\n");
    out.push_str(&format!(
        "節点数,{}\n部材数,{}\n断面数,{}\n材料数,{}\n荷重ケース数,{}\n階数,{}\n",
        model.nodes.len(),
        model.elements.len(),
        model.sections.len(),
        model.materials.len(),
        model.load_cases.len(),
        model.stories.len()
    ));

    if !model.stories.is_empty() {
        out.push_str("\n[階]\n階,標高[mm],地震重量[kN]\n");
        for s in &model.stories {
            out.push_str(&format!(
                "{},{},{:.2}\n",
                s.name,
                s.fl_mm()
                    .map(|v| format!("{v:.0}"))
                    .unwrap_or_else(|| "未設定".into()),
                force_kn(s.seismic_weight.unwrap_or(0.0))
            ));
        }
    }

    out.push_str(&build_quantity_csv(model));

    {
        let (floors, walls) = model.unset_plate_assignment_regions();
        if !floors.is_empty() || !walls.is_empty() {
            let list = |ids: &[u32]| {
                ids.iter()
                    .map(|id| format!("R{id}"))
                    .collect::<Vec<_>>()
                    .join(";")
            };
            let floors_list: Vec<u32> = floors.iter().map(|id| id.0).collect();
            let walls_list: Vec<u32> = walls.iter().map(|id| id.0).collect();
            out.push_str("\n[未設定の割当領域]\n");
            out.push_str(&format!("床板 未設定件数,{}\n", floors.len()));
            out.push_str(&format!("床板 対象ID,{}\n", list(&floors_list)));
            out.push_str(&format!("壁版 未設定件数,{}\n", walls.len()));
            out.push_str(&format!("壁版 対象ID,{}\n", list(&walls_list)));
            out.push_str("備考,未設定の領域は床荷重・躯体自重と壁の剛性が解析に入らず応力・変形を過小評価し得ます\n");
        }
    }

    if let Some(reason) = &app.core.scoped.result_contract_notice {
        out.push_str(&format!(
            "\n[計算契約]\n結果状態,利用不可\n理由,{}\n",
            reason.replace(',', ";").replace('\n', " ")
        ));
    }
    for reason in model.story_level_diagnostics() {
        out.push_str(&format!("層間変形角 算定不能,{}\n", csv_cell(&reason)));
    }
    {
        let metrics = seismic_story_metrics(app);
        if !metrics.is_empty() {
            out.push_str("\n[層指標(二次設計)]\n階,ケース,方向,FL間高さ[mm],状態・理由,支配柱ID,柱頭位置[mm],層間変位[mm],層間変形角,設定比較分母,設定値との参考比較,剛性率Rs,Rs判定,偏心率Re,Re判定,Fes\n");
            for m in &metrics {
                let [state, id, pos, delta, angle, comparison] = drift_cells(m);
                out.push_str(&format!(
                    "{},地震静的,{:?},{:.3},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                    csv_cell(&m.name),
                    app.core.analysis_cfg.seismic_dir,
                    m.height,
                    csv_cell(&state),
                    id,
                    csv_cell(&pos),
                    delta,
                    angle,
                    m.drift_limit_denom,
                    csv_cell(&comparison),
                    metric_value_text(&m.rs),
                    metric_verdict_text(m.rs_ok),
                    metric_value_text(&m.re),
                    metric_verdict_text(m.re_ok),
                    metric_value_text(&m.fes)
                ));
            }
            for m in &metrics {
                if let Err(reason) = &m.fes {
                    out.push_str(&format!(
                        "必要保有水平耐力・合否,算定不能,{}\n",
                        reason.replace(',', ";").replace('\n', " ")
                    ));
                }
            }
        }
    }

    for reason in app.holding_adoption_unavailable_reasons() {
        out.push_str(&format!(
            "必要保有水平耐力・合否,採用保留,{}\n",
            csv_cell(&reason)
        ));
    }
    if let Some(reason) = app.holding_ds_conditions_unavailable_reason() {
        out.push_str(&format!(
            "\n必要保有水平耐力・合否,算定不能,{}\n",
            csv_cell(&reason)
        ));
    }

