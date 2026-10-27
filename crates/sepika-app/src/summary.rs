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

    let Some(results) = &app.core.scoped.results else {
        out.push_str("\n(解析結果なし)\n");
        return out;
    };

    if let Some(modal) = &results.modal {
        out.push_str("\n[固有値解析]\n次数,周期[s],有効質量比X,有効質量比Y\n");
        for (i, t) in modal.period.iter().enumerate() {
            let em = modal.effective_mass.get(i).copied().unwrap_or([0.0; 3]);
            out.push_str(&format!("{},{:.4},{:.3},{:.3}\n", i + 1, t, em[0], em[1]));
        }
    }

    for (key, st) in &results.statics {
        let label = match key {
            StaticCaseKey::User(lc_id) => model
                .load_cases
                .iter()
                .find(|c| c.id == *lc_id)
                .map(|c| format!("LC {} {}", lc_id.0, c.name))
                .unwrap_or_else(|| format!("LC {}", lc_id.0)),
            StaticCaseKey::Seismic(SeismicDir::X) => "地震静的 X".to_string(),
            StaticCaseKey::Seismic(SeismicDir::Y) => "地震静的 Y".to_string(),
        };
        let max_d = st
            .disp
            .iter()
            .flat_map(|u| u[..3].iter())
            .fold(0.0f64, |m, v| m.max(v.abs()));
        out.push_str(&format!(
            "\n[静的解析: {}]\n最大変位[mm],{:.4}\n",
            label, max_d
        ));
    }

    {
        let ctx = metrics_ctx_from_results(&app.core.model, app.core.scoped.results.as_ref());
        if let (Some(rx), Some(ry)) = (ctx.seismic_x, ctx.seismic_y) {
            let cfg = sepika_solver::statics::analysis::SeismicCfg {
                dir: SeismicDir::X,
                mode: app.core.analysis_cfg.ai_mode,
                z: app.core.analysis_cfg.z,
                soil: app.core.analysis_cfg.soil,
                c0: app.core.analysis_cfg.c0,
            };
            let (expanded, _wall_index, _wall_report) =
                sepika_core::wall_expand::expand_wall_elements(model);
            if let Ok(analysis) = sepika_solver::statics::analysis::Analysis::prepare(&expanded) {
                if let Ok(p) = analysis.seismic_nodal_force_magnitudes(cfg) {
                    let theta =
                        sepika_design_jp::secondary::principal_axis::principal_axis_from_results(
                            &expanded, &p, rx, ry,
                        );
                    out.push_str(&format!(
                        "\n[主軸の計算]\n主軸角Θ[deg],{:.3}\n",
                        theta.to_degrees()
                    ));
                }
            }
        }
    }

    {
        use sepika_core::load_combo::{LoadAction, LoadDuration};
        let cell = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
        let (target, key, name, terms) = match app.core.scoped.last_static {
            Some(StaticKey::Combo(index)) => {
                let name = results
                    .combos
                    .get(index)
                    .map(|(name, _)| name.as_str())
                    .unwrap_or("");
                let matches: Vec<_> = model
                    .combinations
                    .iter()
                    .filter(|c| c.name == name)
                    .collect();
                let terms = if matches.len() == 1 {
                    matches[0].terms.clone()
                } else {
                    vec![]
                };
                ("保存組合せ", format!("combo:{index}"), name, terms)
            }
            Some(StaticKey::Case(key)) => {
                let id = match key {
                    StaticCaseKey::User(id) => Some(id),
                    StaticCaseKey::Seismic(dir) => app.seismic_case_id(dir),
                };
                let case = id.and_then(|id| model.load_cases.iter().find(|c| c.id == id));
                (
                    "単独ケース",
                    id.map(|id| format!("case:{}", id.0))
                        .unwrap_or_else(|| format!("{key:?}")),
                    case.map(|c| c.name.as_str()).unwrap_or(""),
                    id.map(|id| vec![(id, 1.0)]).unwrap_or_default(),
                )
            }
            None => ("未選択", String::new(), "", vec![]),
        };
        let state = app.selected_design_load_state();
        let (duration, action, diagnostic) = match &state {
            Ok(state) => (
                match state.duration {
                    LoadDuration::Long => "長期",
                    LoadDuration::Short => "短期",
                },
                match state.action {
                    LoadAction::Gravity => "常時",
                    LoadAction::Snow => "積雪",
                    LoadAction::Wind => "風",
                    LoadAction::Seismic => "地震",
                },
                if state.combination {
                    "荷重状態判定済み（法的条件未確認）".to_owned()
                } else {
                    "法令組合せ未検定".to_owned()
                },
            ),
            Err(reason) => ("未判定", "未判定", reason.clone()),
        };
        out.push_str("\n[検定荷重状態]\n対象種別,対象キー,名称,継続時間,作用,判定\n");
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            target,
            key,
            cell(name),
            duration,
            action,
            cell(&diagnostic)
        ));
        out.push_str("法的全条件,未確認\n");
        out.push_str(if app.require_design_input().is_err() {
            "結果状態,要再計算（モデル編集前の検定結果）\n"
        } else if app.core.scoped.last_static.is_none() {
            "結果状態,未選択\n"
        } else {
            "結果状態,計算済み\n"
        });
        out.push_str("\n[検定荷重項]\nケースID,名称,種別,係数\n");
        for (id, factor) in terms {
            let case = model.load_cases.iter().find(|c| c.id == id);
            out.push_str(&format!(
                "{},{},{},{}\n",
                id.0,
                cell(case.map(|c| c.name.as_str()).unwrap_or("参照欠落")),
                case.map(|c| format!("{:?}", c.kind))
                    .unwrap_or_else(|| "未判定".into()),
                factor
            ));
        }
        let selected_scope = if state
            .as_ref()
            .is_ok_and(|s| s.duration == LoadDuration::Long && s.action == LoadAction::Gravity)
        {
            "固定＋用途別積載の独立略算（選択termsは未適用）"
        } else {
            "選択短期等は未検定（積雪・風・地震・任意組合せ）"
        };
        out.push_str("\n[小梁・床検定範囲]\n対象,対象荷重,継続時間,選択荷重状態の適用\n");
        out.push_str(&format!(
            "小梁,固定＋小梁用積載,長期略算,{}\n床,固定＋床用積載,長期略算,{}\n",
            selected_scope, selected_scope
        ));
    }

    let design_available = app.require_design_input().is_ok();
    if !design_available {
        out.push_str("\n[検定結果状態]\n利用不可,検定の生成入力が欠落または不一致です。検定を再実行してください\n");
    }
    if design_available && !results.member_checks.is_empty() {
        out.push_str("\n[部材検定]\n部材,位置,検定比,判定,根拠\n");
        for m in &results.member_checks {
            for p in &m.positions {
                match &p.outcome {
                    sepika_design_jp::CheckOutcome::Checked(cr) => {
                        out.push_str(&format!(
                            "{},{:.3},{:.4},{},{}\n",
                            m.elem.0,
                            p.xi,
                            cr.ratio(),
                            if cr.ok() { "OK" } else { "NG" },
                            cr.basis.replace(',', ";")
                        ));
                    }
                    sepika_design_jp::CheckOutcome::Skipped { reason } => {
                        out.push_str(&format!(
                            "{},{:.3},-,検定不能,{}\n",
                            m.elem.0,
                            p.xi,
                            reason.replace(',', ";")
                        ));
                    }
                }
            }
        }
    }

    if design_available && !results.wall_checks.is_empty() {
        out.push_str(if app.require_design_input().is_err() {
            "\n[耐震壁結果状態]\n要再計算,モデル編集前の検定結果です\n"
        } else {
            "\n[耐震壁結果状態]\n計算済み\n"
        });
        let summary =
            sepika_design_jp::wall_check::WallCheckSummary::from_checks(&results.wall_checks);
        out.push_str("\n[耐震壁検定集計]\n壁枚数,検定件数,合格,NG,未検定,対象外,最大検定比\n");
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            summary.n_walls,
            summary.n_checks,
            summary.n_ok,
            summary.n_ng,
            summary.n_skipped,
            summary.n_outside,
            summary
                .max_ratio
                .map(|r| format!("{r:.4}"))
                .unwrap_or_default()
        ));
        out.push_str("\n[耐震壁種別集計]\n種別,壁枚数,合格,NG,未検定,対象外\n");
        for kind in [
            sepika_design_jp::wall_check::WallCheckKind::AllowableShear,
            sepika_design_jp::wall_check::WallCheckKind::ReferenceSkeleton,
        ] {
            let s = sepika_design_jp::wall_check::WallCheckSummary::for_kind(
                &results.wall_checks,
                kind,
            );
            out.push_str(&format!(
                "{},{},{},{},{},{}\n",
                kind.label(),
                s.n_walls,
                s.n_ok,
                s.n_ng,
                s.n_skipped,
                s.n_outside
            ));
        }
        out.push_str("\n[耐震壁検定]\n壁版,生成要素,case,種別,検定比,判定,理由区分,根拠\n");
        for check in &results.wall_checks {
            let (ratio, status, reason) = match &check.outcome {
                sepika_design_jp::CheckOutcome::Checked(cr) => (
                    format!("{:.4}", cr.ratio()),
                    if cr.ok() { "OK" } else { "NG" },
                    cr.basis.as_str(),
                ),
                sepika_design_jp::CheckOutcome::Skipped { reason } => (
                    String::new(),
                    if check.seismic_target {
                        "未検定"
                    } else {
                        "対象外"
                    },
                    reason.as_str(),
                ),
            };
            let escape = |text: &str| format!("\"{}\"", text.replace('"', "\"\""));
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                check.plate.map(|id| id.0.to_string()).unwrap_or_default(),
                check.elem.map(|id| id.0.to_string()).unwrap_or_default(),
                escape(&check.case),
                check.kind.label(),
                ratio,
                status,
                check
                    .skip_kind
                    .map(|r| format!("{r:?}"))
                    .unwrap_or_default(),
                escape(reason)
            ));
        }
    }

    if !results.joint_checks.is_empty() {
        out.push_str("\n[接合部検定]\n節点,種別,検定比,判定,根拠\n");
        for j in &results.joint_checks {
            if !design_available && matches!(j.outcome, sepika_design_jp::CheckOutcome::Checked(_))
            {
                continue;
            }
            match &j.outcome {
                sepika_design_jp::CheckOutcome::Checked(cr) => {
                    out.push_str(&format!(
                        "{},{},{:.4},{},{}\n",
                        j.node.0,
                        j.label.replace(',', ";"),
                        cr.ratio(),
                        if cr.ok() { "OK" } else { "NG" },
                        cr.basis.replace(',', ";")
                    ));
                }
                sepika_design_jp::CheckOutcome::Skipped { reason } => {
                    out.push_str(&format!(
                        "{},{},-,検定不能,{}\n",
                        j.node.0,
                        j.label.replace(',', ";"),
                        reason.replace(',', ";")
                    ));
                }
            }
        }
    }

    if let Some(po) = app.displayed_pushover() {
        let control = match po.control {
            sepika_solver::nonlinear::pushover::PushoverControl::Phased => "段階制御",
            sepika_solver::nonlinear::pushover::PushoverControl::LoadOnly => "荷重増分のみ",
        };
        out.push_str(&format!(
            "\n[増分解析]\n増分方式,{}\n解析経過の最大ベースシア[kN],{:.2}\nヒンジ数,{}\n",
            control,
            force_kn(po.qu),
            po.hinges.len()
        ));
        out.push_str("\n[壁単体 確定step応答]\n項目,値または未記録理由\n");
        for (label, value) in wall_response_rows(po) {
            out.push_str(&format!(
                "\"{}\",\"{}\"\n",
                label.replace('"', "\"\""),
                value.replace('"', "\"\"")
            ));
        }
        for input in &results.holding_evaluations {
            out.push_str(&format!(
                "\n[目的別採用run {:?} {:?}]\n解析条件,\"{}\"\n",
                input.point.purpose,
                input.point.direction,
                pushover_conditions_text(&input.conditions)
            ));
            if !app.holding_evaluation_is_current(input) {
                out.push_str("集計不能,目的別採用runの生成入力が現在モデルと不一致\n");
                continue;
            }
            for (label, value) in holding_evaluation_rows(&input.run) {
                out.push_str(&format!(
                    "\"{}\",\"{}\"\n",
                    label.replace('"', "\"\""),
                    value.replace('"', "\"\"")
                ));
            }
        }
        let layers = model.layers();
        let n_stories = layers.len();
        let story_name = |i: usize| -> String {
            layers
                .get(i)
                .map(|l| l.name.clone())
                .unwrap_or_else(|| sepika_core::model::default_story_name(i))
        };
        out.push_str("step,頂部変位[mm],ベースシア[kN]");
        for i in 0..n_stories {
            out.push_str(&format!(
                ",{0} 層間変位[mm],{0} 層せん断力[kN]",
                story_name(i)
            ));
        }
        out.push('\n');
        for p in &po.capacity_curve {
            out.push_str(&format!(
                "{},{:.3},{:.2}",
                p.step,
                p.roof_disp,
                force_kn(p.base_shear)
            ));
            for i in 0..n_stories {
                let drift = p.story_drift.get(i).copied().unwrap_or(0.0);
                let shear = force_kn(p.story_shear.get(i).copied().unwrap_or(0.0));
                out.push_str(&format!(",{:.3},{:.2}", drift, shear));
            }
            out.push('\n');
        }
    }

    if let Some(th) = &results.time_history {
        let peak = th
            .history
            .node_disp
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()));
        out.push_str(&format!(
            "\n[時刻歴応答]\nステップ数,{}\n記録節点最大変位[mm],{:.4}\n",
            th.time.len(),
            peak
        ));
        if let Some(recording) = &th.recording {
            push_story_response_table(&mut out, model, "X", &recording.story_x);
            push_story_response_table(&mut out, model, "Y", &recording.story_y);
        }
    }

    out
}

/// 時刻歴応答の層応答分布（層せん断力・層せん断力係数・階加速度・階速度・階変位の
/// 各層最大値）を CSV へ追記する（1 方向分）。値は `StoryResponse::peak_*`
/// （全ステップ更新・間引きなしのピーク、中-4）を表示単位（kN・gal・m/s）へ
/// 換算する（フレーム記録からの `story_absmax` は record_every の間引きの合間に
/// 生じたピークを取りこぼすため使わない）。
fn push_story_response_table(
    out: &mut String,
    model: &Model,
    dir_label: &str,
    story: &sepika_solver::dynamic::timehistory::StoryResponse,
) {
    use crate::story_response::{mm_s2_to_gal, mm_s_to_m_s, n_to_kn};

    let n = story.stories.len();
    if n == 0 {
        return;
    }
    let shear = &story.peak_story_shear;
    let accel = &story.peak_floor_accel;
    let vel = &story.peak_floor_vel;
    let disp = &story.peak_floor_disp;

    let model_story_names: Vec<(sepika_core::ids::StoryId, String)> = model
        .stories
        .iter()
        .map(|s| (s.id, s.name.clone()))
        .collect();
    let names = crate::story_response::story_display_names(&model_story_names, &story.stories);

    out.push_str(&format!(
        "\n[時刻歴応答 層応答分布({}方向)]\n階,層せん断力[kN],層せん断力係数,階加速度[gal],階速度[m/s],階変位[mm]\n",
        dir_label
    ));
    for i in 0..n {
        let name = names.get(i).cloned().unwrap_or_default();
        let coeff = story.peak_shear_coeff.get(i).copied().unwrap_or(0.0);
        out.push_str(&format!(
            "{},{:.2},{:.4},{:.1},{:.4},{:.2}\n",
            name,
            n_to_kn(shear.get(i).copied().unwrap_or(0.0)),
            coeff,
            mm_s2_to_gal(accel.get(i).copied().unwrap_or(0.0)),
            mm_s_to_m_s(vel.get(i).copied().unwrap_or(0.0)),
            disp.get(i).copied().unwrap_or(0.0)
        ));
    }
}

/// 準備計算の結果（[`crate::app::PreparationResult`]）を CSV 文字列に整形する
/// （GUI 非依存）。建物概要・階の分布・地震力(Ai分布)・剛域・断面性能・
/// 幅厚比・部材剛性・荷重集計の各セクションを出力する。
/// 準備計算が未実行なら空文字列を返す。
pub fn build_preparation_csv(app: &App) -> String {
    use crate::app::{
        ai_mode_label, load_case_kind_label, member_kind_label, member_rank_label,
        soil_class_label, steel_member_use_label, story_level_kind_label, story_structure_label,
        zone_source_label,
    };

    let Some(p) = app.core.scoped.preparation.as_ref() else {
        return String::new();
    };
    let kn = force_kn;
    let mut out = String::new();

    out.push_str("# SEPIKA 準備計算\n");
    out.push_str("\n[建物概要]\n");
    let s = &p.summary;
    out.push_str(&format!(
        "節点数,{}\n部材数,{}\n支点数,{}\n階数,{}\n剛床数,{}\n\
         地盤面GL[mm],{:.0}\n建物高さh[m],{:.3}\n鉄骨造高さ比α,{:.4}\n\
         地震用重量ΣW[kN],{:.2}\n",
        s.n_nodes,
        s.n_elements,
        s.n_supports,
        s.n_stories,
        s.n_diaphragms,
        s.ground_elevation,
        length_m(s.height_mm),
        s.steel_height_ratio,
        kn(s.total_seismic_weight),
    ));
    out.push_str(&format!(
        "整合性チェック エラー,{}\n整合性チェック 警告,{}\n",
        p.diag_errors, p.diag_warnings
    ));

    if !p.stories.is_empty() {
        out.push_str(
            "\n[階の分布]\n階,床レベル[mm],階高[mm],節点数,剛床数,地震用重量Wi[kN],累積ΣWj[kN],構造,種別\n",
        );
        for r in p.stories.iter().rev() {
            out.push_str(&format!(
                "{},{:.0},{:.0},{},{},{:.2},{:.2},{},{}\n",
                r.name,
                r.elevation,
                r.height,
                r.n_nodes,
                r.n_diaphragms,
                kn(r.weight),
                kn(r.cumulative_weight),
                story_structure_label(r.structure),
                story_level_kind_label(r.level_kind),
            ));
        }
    }

    match (&p.seismic, &p.seismic_note) {
        (Some(sm), _) => {
            out.push_str("\n[地震力 (Ai分布)]\n");
            out.push_str(&format!(
                "設計用固有周期T[s],{:.4}\nTの算定法,{}\n地盤種別,{}\nTc[s],{:.2}\n\
                 振動特性係数Rt,{:.4}\n地域係数Z,{:.2}\n標準せん断力係数C0,{:.3}\n\
                 基部せん断力Q1[kN],{:.2}\n",
                sm.t,
                ai_mode_label(sm.t_mode),
                soil_class_label(sm.soil),
                sm.tc,
                sm.rt,
                sm.z,
                sm.c0,
                kn(sm.base_shear),
            ));
            out.push_str("階,Wi[kN],ΣWj[kN],αi,Ai,Ci,Qi[kN],Pi[kN],種別\n");
            for r in sm.rows.iter().rev() {
                out.push_str(&format!(
                    "{},{:.2},{:.2},{:.4},{:.4},{:.5},{:.2},{:.2},{}\n",
                    r.name,
                    kn(r.weight),
                    kn(r.cumulative_weight),
                    r.alpha,
                    r.ai,
                    r.ci,
                    kn(r.qi),
                    kn(r.pi),
                    story_level_kind_label(r.level_kind),
                ));
            }
        }
        (None, Some(note)) => {
            out.push_str(&format!("\n[地震力 (Ai分布)]\n算定不可,{}\n", note));
        }
        (None, None) => {}
    }

    out.push_str(&format!(
        "\n[剛域]\n剛域・危険断面位置を持つ部材数,{}\n梁要素数,{}\n",
        p.rigid_zones.len(),
        p.rigid_zone_candidates
    ));
    if !p.rigid_zones.is_empty() {
        out.push_str(
            "部材ID,種別,節点i,節点j,材長L[mm],λi[mm],λi出所,λj[mm],λj出所,可とう長L'[mm],フェースi[mm],フェースj[mm],剛域比\n",
        );
        for r in &p.rigid_zones {
            out.push_str(&format!(
                "{},{},{},{},{:.1},{:.1},{},{:.1},{},{:.1},{:.1},{:.1},{:.4}\n",
                r.elem.0,
                member_kind_label(r.kind),
                r.node_i.0,
                r.node_j.0,
                r.length,
                r.zone_i,
                zone_source_label(r.source_i),
                r.zone_j,
                zone_source_label(r.source_j),
                r.clear_length,
                r.face_i,
                r.face_j,
                r.ratio,
            ));
        }
    }

    if !p.sections.is_empty() {
        out.push_str(
            "\n[断面性能]\n断面ID,断面,形状,部材数,D[mm],B[mm],A[mm2],Iy[mm4],Iz[mm4],J[mm4],Asy[mm2],Asz[mm2],iy[mm],iz[mm],材料,E[N/mm2],未算定理由\n",
        );
        for r in &p.sections {
            out.push_str(&format!(
                "{},{},{},{},{:.1},{:.1},{},{},{},{},{:.1},{:.1},{},{},{},{},{}\n",
                r.section.0,
                r.name,
                r.shape_label.as_deref().unwrap_or("数値直入力"),
                r.n_elements,
                r.depth,
                r.width,
                r.area
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.iy.map(|v| format!("{v:.4e}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.iz.map(|v| format!("{v:.4e}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.j.map(|v| format!("{v:.4e}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.as_y,
                r.as_z,
                r.ry.map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.rz.map(|v| format!("{v:.2}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.material.as_deref().unwrap_or(""),
                r.young.map(|e| format!("{:.0}", e)).unwrap_or_default(),
                r.unavailable_reason.as_deref().unwrap_or(""),
            ));
        }
    }

    if !p.width_thickness.is_empty() {
        out.push_str(
            "\n[幅厚比・部材ランク]\n断面ID,断面,用途,材料,部材数,最大幅厚比,ランク,未算定理由\n",
        );
        for r in &p.width_thickness {
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{}\n",
                r.section.0,
                r.section_name,
                steel_member_use_label(r.member_use),
                r.material,
                r.n_elements,
                r.max_ratio.map(|v| format!("{:.2}", v)).unwrap_or_default(),
                r.rank.map(member_rank_label).unwrap_or("判定不可"),
                r.reason.as_deref().unwrap_or(""),
            ));
        }
    }

    if !p.member_stiffness.is_empty() {
        out.push_str(&format!(
            "\n[部材剛性の割増し・等価換算]\n該当部材数,{}\n梁要素数,{}\n",
            p.member_stiffness.len(),
            p.member_stiffness_candidates
        ));
        out.push_str(
            "部材ID,種別,断面,材料,スラブ増大率,壁上下梁倍率,元A[mm2],元Iy[mm4],実効A[mm2],実効Iy[mm4],総増大率,等価Aax[mm2],等価Iy[mm4],等価Iz[mm4],等価J[mm4],等価Asy[mm2],等価Asz[mm2],未算定理由\n",
        );
        for r in &p.member_stiffness {
            let c = r.composite;
            let fmt = |v: Option<f64>| v.map(|x| format!("{:.4e}", x)).unwrap_or_default();
            out.push_str(&format!(
                "{},{},{},{},{:.4},{:.1},{:.1},{:.4e},{},{},{},{},{},{},{},{},{},{}\n",
                r.elem.0,
                member_kind_label(r.kind),
                r.section_name,
                r.material,
                r.slab_factor,
                r.wall_girder_factor,
                r.section_area,
                r.section_iy,
                r.effective_area
                    .map(|v| format!("{v:.1}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.effective_iy
                    .map(|v| format!("{v:.4e}"))
                    .unwrap_or_else(|| "未算定".into()),
                r.effective_iy
                    .filter(|_| r.section_iy > 0.0)
                    .map(|v| format!("{:.4}", v / r.section_iy))
                    .unwrap_or_default(),
                fmt(c.map(|c| c.area_ax)),
                fmt(c.map(|c| c.iy)),
                fmt(c.map(|c| c.iz)),
                fmt(c.map(|c| c.j)),
                fmt(c.map(|c| c.as_y)),
                fmt(c.map(|c| c.as_z)),
                r.unavailable_reason.as_deref().unwrap_or(""),
            ));
        }
    }

    if !p.beam_upstands.is_empty() {
        out.push_str("\n[梁の床板嵩上げ]\n梁,床板ID,材料ID,採用元,側,始端[mm],終端[mm],幅[mm],始端空き[mm],終端空き[mm],体積[m³],設計重量[kN],物理質量[t],物理質量相当重量[kN],設計単位重量[kN/m³],物理密度[t/m³]\n");
        for s in &p.beam_upstands {
            let beam = match s.beam {
                sepika_core::model::SupportMemberId::Primary(id) => format!("大梁 {}", id.0),
                sepika_core::model::SupportMemberId::Secondary(id) => format!("小梁 {}", id.0),
            };
            out.push_str(&format!(
                "{},{},{},{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.9},{:.6},{:.9},{:.6},{:.9},{:.9}\n",
                beam,
                s.slab.0,
                s.material.0,
                s.material_source_label(),
                if s.side == 0 { "左" } else { "右" },
                s.interval_mm[0],
                s.interval_mm[1],
                s.width_mm,
                s.gap_mm[0],
                s.gap_mm[1],
                s.volume_mm3 / 1e9,
                kn(s.design_weight_n),
                s.physical_mass_t,
                kn(s.mass_equivalent_weight_n),
                s.design_unit_weight_n_per_mm3 / 1e-6,
                s.physical_density_t_per_mm3 / 1e-9
            ));
        }
    }
    if !p.girder_self_weights.is_empty() {
        out.push_str("\n[矩形RC/SRC大梁自重（仕上げ・付加重量を除く）]\n部材ID,梁本体[kN],床板重複控除[kN],嵩上げ加算[kN],合計[kN]\n");
        for w in &p.girder_self_weights {
            let upstand: f64 = p
                .beam_upstands
                .iter()
                .filter(|s| s.beam == sepika_core::model::SupportMemberId::Primary(w.element))
                .map(|s| s.design_weight_n)
                .sum();
            out.push_str(&format!(
                "{},{:.2},{:.2},{:.2},{:.2}\n",
                w.element.0,
                kn(w.body_weight_n),
                kn(w.slab_overlap_weight_n),
                kn(upstand),
                kn(w.body_weight_n - w.slab_overlap_weight_n + upstand)
            ));
        }
    }
    if !p.load_cases.is_empty() {
        out.push_str(
            "\n[荷重集計]\n荷重ケース,種別,節点荷重数,部材荷重数,ΣFx[kN],ΣFy[kN],ΣFz[kN]\n",
        );
        for r in &p.load_cases {
            out.push_str(&format!(
                "{},{},{},{},{:.2},{:.2},{:.2}\n",
                r.name,
                load_case_kind_label(r.kind),
                r.n_nodal,
                r.n_member,
                kn(r.sum_force[0]),
                kn(r.sum_force[1]),
                kn(r.sum_force[2]),
            ));
        }
    }

    out
}

/// 数量積算の CSV 文字列を生成する（GUI 非依存）。
///
/// 部位別の概算数量
/// （[`sepika_design_jp::quantity::try_compute_quantity_takeoff`]）を、
/// 部位別・階別・鉄骨種類別・鉄筋径別・明細・注記のセクションに整形する。
pub fn build_quantity_csv(model: &Model) -> String {
    use sepika_design_jp::quantity::{try_compute_quantity_takeoff, QuantityTotals};
    fn subtotal(out: &mut String, name: &str, t: &QuantityTotals) {
        out.push_str(&format!(
            "{},{:.3},{:.2},{:.6},{:.4},{:.1},{},{}\n",
            csv_cell(name),
            t.concrete_m3,
            t.formwork_m2,
            t.rebar_t,
            t.steel_t,
            t.rebar_joints,
            t.rebar_status().label(),
            csv_cell(
                &t.rebar_issues
                    .iter()
                    .map(|i| i.description())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        ));
    }
    let q = match try_compute_quantity_takeoff(model, &model.quantity_cfg) {
        Ok(q) => q,
        Err(reason) => return format!("[数量積算 未算定]\n理由\n{}\n", csv_cell(&reason)),
    };
    let mut out = String::new();
    for (heading, rows) in [
        (
            "部位別",
            q.totals_by_category()
                .into_iter()
                .map(|(k, t)| (k.label().to_string(), t))
                .collect::<Vec<_>>(),
        ),
        ("階別", q.totals_by_story()),
        (
            "構造別",
            q.totals_by_structure()
                .into_iter()
                .map(|(k, t)| (k.label().to_string(), t))
                .collect(),
        ),
    ] {
        out.push_str(&format!("\n[数量積算 {heading}]\n分類,コンクリート[m3],型枠[m2],鉄筋[t],鉄骨[t],鉄筋継手[個所],鉄筋状態,未算定理由\n"));
        for (key, t) in rows {
            subtotal(&mut out, &key, &t);
        }
    }
    out.push_str("\n[数量積算 全体]\n分類,コンクリート[m3],型枠[m2],鉄筋[t],鉄骨[t],鉄筋継手[個所],鉄筋状態,未算定理由\n");
    subtotal(&mut out, "合計", &q.totals());
    out.push_str("\n[数量積算 鉄骨種類別]\n断面,長さ[m],重量[t]\n");
    for s in q.steel_by_section() {
        out.push_str(&format!(
            "{},{:.2},{:.3}\n",
            csv_cell(&s.section_name),
            s.length_m,
            s.weight_t
        ));
    }
    out.push_str("\n[数量積算 鉄筋径別]\n呼び径,長さ[m],重量[t],鉄筋状態\n");
    for (dia, len, w) in q.rebar_by_dia() {
        out.push_str(&format!(
            "{dia},{len:.6},{w:.6},{}\n",
            q.totals().rebar_status().label()
        ));
    }
    out.push_str("\n[数量積算 明細]\nID,壁版ID,断面ID,階,部位,構造,符号,コンクリート[m3],型枠[m2],鉄筋[t],鉄骨[t],鉄筋継手[個所],鉄筋状態,未算定理由\n");
    for it in &q.items {
        let id = it
            .elem
            .map(|e| e.0.to_string())
            .or_else(|| it.slab.map(|s| format!("S{}", s.0)))
            .or_else(|| it.secondary.map(|s| format!("SM{}", s.0)))
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{:.3},{:.2},{:.6},{:.4},{:.1},{},{}\n",
            id,
            it.wall_plate.map(|i| i.0.to_string()).unwrap_or_default(),
            it.section.map(|i| i.0.to_string()).unwrap_or_default(),
            csv_cell(&it.story),
            it.category.label(),
            it.structure.label(),
            csv_cell(&it.label),
            it.concrete_m3,
            it.formwork_m2,
            it.rebar_weight_t(),
            it.steel_weight_t(),
            it.rebar_joints,
            it.rebar_status().label(),
            csv_cell(
                &it.rebar_issues
                    .iter()
                    .map(|i| i.description())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        ));
    }
    out.push_str(
        "\n[数量積算 鉄筋方向別]\n要素ID,壁版ID,断面ID,方向,呼び径[mm],長さ[m],重量[t],状態,理由\n",
    );
    for it in &q.items {
        let ids = format!(
            "{},{},{}",
            it.elem.map(|i| i.0.to_string()).unwrap_or_default(),
            it.wall_plate.map(|i| i.0.to_string()).unwrap_or_default(),
            it.section.map(|i| i.0.to_string()).unwrap_or_default()
        );
        for r in &it.rebar {
            out.push_str(&format!(
                "{ids},{},{},{:.6},{:.6},算定済み概算,\n",
                r.usage.label(),
                r.dia.map(|d| d.to_string()).unwrap_or_default(),
                r.total_length_m,
                r.weight_t
            ));
        }
        for i in &it.rebar_issues {
            out.push_str(&format!(
                "{ids},{},,,,未算定,{}\n",
                i.usage.label(),
                csv_cell(&i.description())
            ));
        }
    }
    out.push_str("\n[数量積算 注記]\n");
    for n in &q.notes {
        out.push_str(&format!("{}\n", csv_cell(n)));
    }
    out
}

/// ResultsBundle が空でないか（レポートに載せる内容があるか）。
pub fn has_report_content(results: &Option<ResultsBundle>) -> bool {
    results
        .as_ref()
        .map(|r| {
            !r.statics.is_empty()
                || r.modal.is_some()
                || r.pushover_x.is_some()
                || r.pushover_y.is_some()
                || r.time_history.is_some()
                || r.lumped.is_some()
        })
        .unwrap_or(false)
}
pub(crate) fn wall_response_rows(
    po: &sepika_solver::nonlinear::pushover::PushoverResult,
) -> Vec<(String, String)> {
    use sepika_solver::nonlinear::pushover::wall_response::WallUnavailableReason;
    let mut rows = holding_evaluation_rows(po);
    let Some(records) = &po.wall_history else {
        return vec![(
            "壁応答".into(),
            WallUnavailableReason::LegacyNotRecorded
                .description()
                .into(),
        )];
    };
    if let Some(run) = &po.wall_run {
        rows.push(("壁解析run".into(), run.run_id.clone()));
        rows.push((
            "入力世代".into(),
            if run.input_generation.is_some() {
                "記録あり".into()
            } else {
                WallUnavailableReason::InputIdentityNotRecorded
                    .description()
                    .into()
            },
        ));
    } else {
        rows.push((
            "壁解析run・入力世代".into(),
            WallUnavailableReason::InputIdentityNotRecorded
                .description()
                .into(),
        ));
    }
    for record in records {
        let prefix = format!(
            "step {} 壁要素 {} 壁版 {} 壁単体",
            record.step,
            record.elem.0,
            record
                .plate
                .map(|id| id.0.to_string())
                .unwrap_or_else(|| "対応未記録".into())
        );
        let r = match po.wall_response_at(record.elem, record.step) {
            Ok(response) => response,
            Err(reason) => {
                rows.push((prefix, reason.description().into()));
                continue;
            }
        };
        rows.push((
            format!("{prefix} 正規化節点/面内軸/仮想壁柱軸"),
            format!(
                "{:?} / {:?} / {:?}",
                r.nodes, r.inplane_axis, r.virtual_column_axes
            ),
        ));
        for (name, value) in [
            ("Qw[N]", r.qw_n),
            ("Qdir[N]", r.qdir_n),
            ("H[mm]", r.height_mm),
            ("Δw[mm]", r.delta_wall_mm),
            ("R_wall[rad]", r.chord_rotation_rad),
        ] {
            rows.push((format!("{prefix} {name}"), value.to_string()));
        }
        for (name, result) in [
            ("下辺", &r.bottom),
            ("上辺", &r.top),
            ("釣合い残差", &r.equilibrium_residual),
        ] {
            rows.push((
                format!("{prefix} {name} O[mm]/F[N]/M_O[Nmm]"),
                format!(
                    "{:?} / {:?} / {:?}",
                    result.origin_mm, result.force_n, result.moment_nmm
                ),
            ));
        }
        rows.push((
            format!("{prefix} γ_wall"),
            r.material_shear_strain
                .map(|v| v.to_string())
                .unwrap_or_else(|| {
                    r.material_shear_unavailable
                        .unwrap_or(WallUnavailableReason::MaterialShearNotRecovered)
                        .description()
                        .into()
                }),
        ));
        rows.push((
            format!("{prefix} 線材イベント"),
            r.line_events_unavailable
                .unwrap_or(WallUnavailableReason::LineEventsNotApplicable)
                .description()
                .into(),
        ));
        if let Some(mf) = &r.virtual_column {
            for (pos, f) in &mf.at {
                rows.push((
                    format!("{prefix} 仮想壁柱 位置{pos} N引張正/Qy/Qz/Mx/My/Mz[N・Nmm]"),
                    format!("{:?}", f),
                ));
            }
        } else {
            rows.push((
                format!("{prefix} 仮想壁柱"),
                r.virtual_column_unavailable
                    .unwrap_or(WallUnavailableReason::VirtualColumnNotRecorded)
                    .description()
                    .into(),
            ));
        }
    }
    rows
}

fn holding_evaluation_rows(
    po: &sepika_solver::nonlinear::pushover::PushoverResult,
) -> Vec<(String, String)> {
    use sepika_solver::nonlinear::pushover::story_response::EvaluationPurpose;
    let mut rows = vec![("増分解析終了理由".into(), format!("{:?}", po.termination))];
    for (purpose, point, label) in [
        (EvaluationPurpose::Ds, &po.ds_evaluation, "Ds判定"),
        (
            EvaluationPurpose::HoldingCapacity,
            &po.capacity_evaluation,
            "保有耐力比較",
        ),
    ] {
        let Some(point) = point else {
            rows.push((label.into(), "目的別評価点が未指定".into()));
            continue;
        };
        rows.push((
            format!("{label}採用点"),
            format!(
                "run={} direction={:?} step={} generation_sha256={} reason={}",
                point.run_id,
                point.direction,
                point.step,
                input_generation_hash(&point.input_generation),
                point.selection_reason
            ),
        ));
        rows.push((
            format!("{label} 群耐力重み[N]"),
            format!("{:?}", point.member_capacities_n),
        ));
        match po.evaluate_stories(point, purpose) {
            Ok(stories) => {
                for s in stories {
                    rows.push((format!("{label} 層{} 符号付き層切断面力[N]", s.layer+1), format!("Qu={} Wall={} Brace={} Frame={} βu={} 上層外力={} 基準外力={} 上側支持抵抗={} 残差={} 許容差={}", s.qu_n, s.wall_n, s.brace_n, s.frame_n, s.beta_u, s.external_n, s.reference_n, s.support_n, s.residual_n, s.tolerance_n)));
                }
            }
            Err(reason) => rows.push((format!("{label}集計不能"), reason)),
        }
    }
    rows
}

pub(crate) fn pushover_conditions_text(cfg: &sepika_job::AnalysisSettings) -> String {
    format!("Ai={:?}, Z={}, C0={}, 地盤={:?}, 方式={:?}, 刻み={}, 目標変位={:?} mm, 目標層間角={:?}, 長期載荷={}, 塑性率={:?}",
        cfg.ai_mode, cfg.z, cfg.c0, cfg.soil, cfg.push_control, cfg.push_steps,
        cfg.push_use_max_disp.then_some(cfg.push_max_disp),
        cfg.push_use_drift_angle.then_some(1.0/cfg.push_drift_denom), cfg.push_apply_long_term, cfg.ductility_method)
}

pub(crate) fn input_generation_hash(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics_810_app(long_term: bool) -> App {
        let mut app = App::default();
        let mut model = crate::sample::portal_frame();
        model.load_cases.push(sepika_core::model::LoadCase {
            id: sepika_core::ids::LoadCaseId(2),
            name: "P0".into(),
            kind: sepika_core::model::LoadCaseKind::Live,
            nodal: vec![],
            member: vec![],
        });
        model
            .combinations
            .push(sepika_core::model::LoadCombination {
                name: "G+P".into(),
                terms: vec![
                    (sepika_core::ids::LoadCaseId(0), 1.0),
                    (sepika_core::ids::LoadCaseId(2), 1.0),
                ],
            });
        app.load_model(model);
        app.generate_stories_action();
        if long_term {
            app.run_combination(0);
        }
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        app.core.analysis_cfg.seismic_dir = SeismicDir::X;
        app
    }

    #[test]
    fn misc740_代表内柱条件とrs専用変位の成功失敗をovikaに保存する() {
        use sepika_core::model::{MiscWallRepresentativeColumn, MiscWallStiffnessMethod};
        let mut app = metrics_810_app(false);
        let representatives: Vec<_> = app
            .core
            .model
            .layers()
            .iter()
            .map(|layer| {
                let column = sepika_design_jp::secondary::story_columns::story_columns(
                    &app.core.model,
                    layer.top,
                )[0]
                .top_elem;
                MiscWallRepresentativeColumn {
                    story: layer.top,
                    column,
                }
            })
            .collect();
        app.core.model.stress_cfg.misc_wall_n = Some(1.5);
        app.core.model.stress_cfg.misc_wall_method = MiscWallStiffnessMethod::RepresentativeColumn;
        app.core.model.stress_cfg.misc_wall_representatives = representatives;
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let expected = app
            .core
            .scoped
            .results
            .as_ref()
            .unwrap()
            .seismic(SeismicDir::X)
            .unwrap()
            .stiffness_ratio_disp
            .clone()
            .unwrap()
            .unwrap();
        let cfg = app.core.model.stress_cfg.clone();
        let path = std::env::temp_dir().join(format!(
            "sepika-misc740-rs-save-{}.ovika",
            std::process::id()
        ));
        app.save_project_to_opts(path.clone(), Some(true));
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );
        let mut loaded = App::default();
        loaded.open_project_from(path.clone());
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        assert_eq!(loaded.core.model.stress_cfg, cfg);
        assert_eq!(
            loaded
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .seismic(SeismicDir::X)
                .unwrap()
                .stiffness_ratio_disp,
            Some(Ok(expected))
        );
        assert!(seismic_story_metrics(&loaded)[0].rs.is_ok());
        loaded
            .core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .statics
            .iter_mut()
            .find(|(key, _)| *key == StaticCaseKey::Seismic(SeismicDir::X))
            .unwrap()
            .1
            .stiffness_ratio_disp = Some(Err("壁の支持床が不明です".into()));
        loaded.save_project_to_opts(path.clone(), Some(true));
        assert!(
            loaded.core.scoped.last_error.is_none(),
            "{:?}",
            loaded.core.scoped.last_error
        );
        let mut failed = App::default();
        failed.open_project_from(path.clone());
        assert!(
            failed.core.scoped.last_error.is_none(),
            "{:?}",
            failed.core.scoped.last_error
        );
        assert_eq!(
            failed
                .core
                .scoped
                .results
                .as_ref()
                .unwrap()
                .seismic(SeismicDir::X)
                .unwrap()
                .stiffness_ratio_disp,
            Some(Err("壁の支持床が不明です".into()))
        );
        let metric = seismic_story_metrics(&failed).remove(0);
        assert!(metric.rs.unwrap_err().contains("支持床"));
        assert!(metric.drift.is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn misc740_rs専用結果の欠落や失敗を通常変位で補完しない() {
        let app = metrics_810_app(false);
        let mut model = app.core.model.clone();
        model.stress_cfg.misc_wall_n = Some(1.0);
        let mut results = app.core.scoped.results.clone().unwrap();
        for state in 0..4 {
            let result = &mut results
                .statics
                .iter_mut()
                .find(|(key, _)| *key == StaticCaseKey::Seismic(SeismicDir::X))
                .unwrap()
                .1;
            result.stiffness_ratio_disp = match state {
                0 => None,
                1 => Some(Err("評価用剛床が不明".into())),
                2 => Some(Ok(vec![[0.0; 6]; result.disp.len()])),
                _ => Some(Ok(result.disp.clone())),
            };
            let ctx = metrics_ctx_from_results(&model, Some(&results));
            let metrics = compute_story_metrics_with(&model, SeismicDir::X, &ctx);
            assert!(!metrics.is_empty());
            assert!(metrics[0].drift.is_ok());
            assert_eq!(metrics[0].rs.is_ok(), state == 3);
            assert_eq!(metrics[0].fes.is_ok(), state == 3);
        }
    }

    #[test]
    fn metrics_810_directional_missing_and_unknown_inputs_only_disable_dependencies() {
        use crate::app::ResultInputKey;
        for direction in [SeismicDir::X, SeismicDir::Y] {
            for failure in ["欠測", "識別欠落", "入力不一致"] {
                let mut app = metrics_810_app(false);
                let results = app.core.scoped.results.as_mut().unwrap();
                if failure == "欠測" {
                    results
                        .statics
                        .retain(|(key, _)| *key != StaticCaseKey::Seismic(direction));
                } else if failure == "識別欠落" {
                    results.input_records.retain(|(key, _)| {
                        *key != ResultInputKey::Static(StaticCaseKey::Seismic(direction))
                    });
                } else {
                    results
                        .input_records
                        .iter_mut()
                        .find(|(key, _)| {
                            *key == ResultInputKey::Static(StaticCaseKey::Seismic(direction))
                        })
                        .unwrap()
                        .1
                        .push(0);
                }
                let before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
                let m = seismic_story_metrics(&app).remove(0);
                assert_eq!(m.rs_ok.is_none(), direction == SeismicDir::X);
                assert_eq!(m.drift_ok.is_none(), direction == SeismicDir::X);
                assert_eq!(m.re_ok, None);
                let reason = m.re.as_ref().unwrap_err();
                assert!(
                    reason.contains(&format!("地震静的 {direction:?}")),
                    "{reason}"
                );
                assert!(
                    reason.contains(match failure {
                        "欠測" => "ありません",
                        "識別欠落" => "入力識別情報",
                        _ => "一致しません",
                    }),
                    "{reason}"
                );
                assert!(m.fes.is_err());
                let csv = build_report_csv(&app);
                assert!(csv.contains(reason));
                assert_eq!(
                    rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
                    before
                );
            }
        }
    }

    #[test]
    fn metrics_810_selected_long_term_unknown_or_mismatched_never_falls_back() {
        use crate::app::ResultInputKey;
        for unknown in [false, true] {
            let mut app = metrics_810_app(true);
            let results = app.core.scoped.results.as_mut().unwrap();
            let name = results.combos[0].0.clone();
            assert!(sepika_core::load_combo::is_gravity_combination(
                &name,
                &app.core.model
            ));
            let key = ResultInputKey::Combo(name.clone());
            if unknown {
                results.input_records.retain(|(saved, _)| *saved != key);
            } else {
                results
                    .input_records
                    .iter_mut()
                    .find(|(saved, _)| *saved == key)
                    .unwrap()
                    .1
                    .push(0);
            }
            let m = seismic_story_metrics(&app).remove(0);
            assert_eq!(m.rs, Ok(1.0));
            assert!(m.drift.is_ok());
            assert_eq!(m.re_ok, None);
            let reason = m.re.as_ref().unwrap_err();
            assert!(reason.contains(&name), "{reason}");
            assert!(reason.contains(if unknown {
                "入力識別情報"
            } else {
                "一致しません"
            }));
            assert!(m.fes.is_err());
            assert!(build_report_csv(&app).contains(reason));
        }
    }

    #[test]
    fn metrics_810_current_results_ignore_unrelated_settings_and_reject_related_edits() {
        for long_term in [false, true] {
            let mut app = metrics_810_app(long_term);
            let results = app.core.scoped.results.as_mut().unwrap();
            results
                .combos
                .push(("無関係な組合せ".into(), results.statics[0].1.clone()));
            let before = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
            app.core.analysis_cfg.push_steps += 1;
            app.core.analysis_cfg.push_max_disp += 1.0;
            app.core.model.stress_cfg.drift_limit_denom = 120.0;
            for dir in [SeismicDir::X, SeismicDir::Y] {
                app.core.analysis_cfg.seismic_dir = dir;
                let m = seismic_story_metrics(&app).remove(0);
                assert_eq!((m.rs, m.fes), (Ok(1.0), Ok(1.0)));
                assert!(m.re.unwrap().abs() < 1e-12);
            }
            assert_eq!(
                rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
                before
            );
            app.core.analysis_cfg.c0 += 0.1;
            let m = seismic_story_metrics(&app).remove(0);
            assert!(m.rs.as_ref().unwrap_err().contains("計算設定"));
            assert!(m.re.as_ref().unwrap_err().contains("計算設定"));
            assert_eq!((m.rs_ok, m.re_ok), (None, None));
            assert!(m.fes.is_err());
        }
    }

    #[test]
    fn metrics_810_unknown_or_incompatible_contract_has_no_values_or_verdicts() {
        use sepika_core::result_contract::ANALYSIS_CONTRACT_VERSION;
        for version in [
            None,
            Some(ANALYSIS_CONTRACT_VERSION - 1),
            Some(ANALYSIS_CONTRACT_VERSION + 1),
        ] {
            let mut app = metrics_810_app(false);
            app.core
                .scoped
                .results
                .as_mut()
                .unwrap()
                .analysis_contract_version = version;
            let m = seismic_story_metrics(&app).remove(0);
            assert_eq!((m.rs_ok, m.re_ok), (None, None));
            assert!(m.rs.as_ref().unwrap_err().contains("地震静的 X"));
            let reason = m.re.as_ref().unwrap_err();
            assert!(reason.contains("地震静的 X"));
            assert!(reason.contains("地震静的 Y"));
            assert!(reason.contains("保存版="));
            assert!(m.fes.is_err());
            assert!(build_report_csv(&app).contains(&reason.replace(',', ";")));
        }
    }

    fn drift_result(model: &Model, dir: SeismicDir, response: &StaticOnce) -> StoryMetric {
        let ctx = StoryMetricsCtx {
            seismic_x: (dir == SeismicDir::X).then_some(response),
            seismic_y: (dir == SeismicDir::Y).then_some(response),
            ..Default::default()
        };
        compute_story_metrics_with(model, dir, &ctx).remove(0)
    }

    fn drift_fixture() -> (Model, StaticOnce) {
        let mut model = crate::sample::portal_frame();
        model.stories[1].elevation = Some(3000.0);
        for node in &mut model.nodes[2..] {
            node.coord[2] = 3000.0;
        }
        (
            model,
            StaticOnce {
                stiffness_ratio_disp: None,
                disp: vec![[0.0; 6]; 4],
                member_forces: vec![],
                panel_moments: vec![],
            },
        )
    }

    #[test]
    fn drift_490_signed_directions_common_height_and_controlling_column() {
        let (model, mut response) = drift_fixture();
        for (dir, d) in [(SeismicDir::X, 0), (SeismicDir::Y, 1)] {
            for sign in [-1.0, 1.0] {
                response.disp[2][d] = sign * 18.0;
                response.disp[3][d] = sign * 10.0;
                let m = drift_result(&model, dir, &response);
                let v = m.drift.unwrap().unwrap();
                assert_eq!(v.column.elem.0, 0);
                assert_eq!(v.column.pos, [0.0, 0.0]);
                assert_eq!(v.column.drift, 18.0);
                assert_eq!(v.height_mm, 3000.0);
                assert_eq!(v.angle, 0.006);
                response.disp[2][d] = sign * 20.0;
                response.disp[3][d] = sign * 18.0;
                let v = drift_result(&model, dir, &response).drift.unwrap().unwrap();
                assert_eq!(v.column.elem.0, 0);
                assert_eq!(v.angle, 1.0 / 150.0);
            }
        }
    }

    #[test]
    fn drift_490_sl_displacements_use_fl_height_for_sloped_split_column() {
        use sepika_core::ids::{ElemId, NodeId};
        let (mut model, mut response) = drift_fixture();
        model.stories[0].elevation = Some(3500.0);
        model.stories[0].sl_offset_mm = Some(-500.0);
        model.stories[1].elevation = Some(7000.0);
        model.stories[1].sl_offset_mm = Some(-600.0);
        for (i, node) in model.nodes.iter_mut().enumerate() {
            node.coord[2] = if i < 2 { 3000.0 } else { 6400.0 };
        }
        model.elements.retain(|e| e.id.0 == 0);
        model.nodes[2].coord[0] = 1000.0;
        let mut middle = model.nodes[2].clone();
        middle.id = NodeId(4);
        middle.coord = [500.0, 0.0, 4700.0];
        model.nodes.push(middle);
        let mut segment = model.elements[0].clone();
        segment.id = ElemId(3);
        segment.nodes = [NodeId(4), NodeId(2)].into_iter().collect();
        model.elements[0].nodes = [NodeId(0), NodeId(4)].into_iter().collect();
        model.elements.push(segment);
        response.disp.push([999.0; 6]);
        response.disp[2][0] = 18.0;
        for top_sl in [6400.0, 6300.0] {
            model.stories[1].sl_offset_mm = Some(top_sl - 7000.0);
            model.nodes[2].coord[2] = top_sl;
            let v = drift_result(&model, SeismicDir::X, &response)
                .drift
                .unwrap()
                .unwrap();
            assert_eq!(v.column.elem, ElemId(3));
            assert_eq!(v.column.pos, [1000.0, 0.0]);
            assert_eq!(v.column.drift, 18.0);
            assert_eq!(v.height_mm, 3500.0);
            assert_eq!(v.angle, 18.0 / 3500.0);
        }
    }

    #[test]
    fn drift_490_nonbranching_column_need_not_be_monotone_in_height() {
        use sepika_core::ids::{ElemId, NodeId};
        let (mut model, mut response) = drift_fixture();
        model.elements.retain(|e| e.id.0 == 0);
        for (id, coord) in [(4, [300.0, 0.0, 1800.0]), (5, [600.0, 0.0, 2000.0])] {
            let mut middle = model.nodes[2].clone();
            middle.id = NodeId(id);
            middle.coord = coord;
            model.nodes.push(middle);
            response.disp.push([999.0; 6]);
        }
        model.elements[0].nodes = [NodeId(2), NodeId(4)].into_iter().collect();
        for (id, nodes) in [(3, [NodeId(4), NodeId(5)]), (4, [NodeId(5), NodeId(0)])] {
            let mut segment = model.elements[0].clone();
            segment.id = ElemId(id);
            segment.nodes = nodes.into_iter().collect();
            model.elements.push(segment);
        }
        response.disp[2][0] = -18.0;
        let v = drift_result(&model, SeismicDir::X, &response)
            .drift
            .unwrap()
            .unwrap();
        assert_eq!(v.column.elem, ElemId(0));
        assert_eq!(v.column.drift, 18.0);
        assert_eq!(v.angle, 0.006);
    }

    #[test]
    fn drift_490_zero_absent_missing_nonfinite_and_bad_setting_are_distinct() {
        let (mut model, mut response) = drift_fixture();
        let zero = drift_result(&model, SeismicDir::X, &response);
        assert_eq!(zero.drift.as_ref().unwrap().as_ref().unwrap().angle, 0.0);
        assert_eq!(zero.drift_ok, Some(true));
        for value in [
            0.0,
            -120.0,
            f64::NAN,
            f64::INFINITY,
            f64::MIN_POSITIVE / 100.0,
        ] {
            model.stress_cfg.drift_limit_denom = value;
            let m = drift_result(&model, SeismicDir::X, &response);
            assert_eq!(m.drift_ok, None);
            assert!(drift_cells(&m)[5].contains("比較なし"));
        }
        response.disp.truncate(3);
        let m = drift_result(&model, SeismicDir::X, &response);
        assert!(m.drift.unwrap_err().contains("節点3"));
        assert_eq!(m.drift_ok, None);
        response.disp.push([f64::NAN; 6]);
        assert!(drift_result(&model, SeismicDir::X, &response)
            .drift
            .is_err());
        model.elements.clear();
        let m = drift_result(&model, SeismicDir::X, &response);
        assert!(m.drift.unwrap().is_none());
        assert_eq!(m.drift_ok, None);
        let m = compute_story_metrics_with(&model, SeismicDir::Y, &StoryMetricsCtx::default())
            .remove(0);
        assert!(m.drift.unwrap_err().contains("未計算"));
    }

    #[test]
    fn drift_490_reference_setting_changes_only_comparison_of_current_results() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.core.analysis_cfg.seismic_dir = SeismicDir::X;
        let response = &mut app.core.scoped.results.as_mut().unwrap().statics[0].1;
        response.disp[2][0] = 18.0;
        response.disp[3][0] = 10.0;
        app.run_design_check();
        assert!(app.current_design_results().is_some());
        let before_results = rmp_serde::to_vec_named(&app.core.scoped.results).unwrap();
        let before = seismic_story_metrics(&app).remove(0);
        assert_eq!(before.drift_ok, Some(false));
        let angle = before.drift.unwrap().unwrap().angle;
        app.core.model.stress_cfg.drift_limit_denom = 120.0;
        let after = seismic_story_metrics(&app).remove(0);
        assert_eq!(after.drift.as_ref().unwrap().as_ref().unwrap().angle, angle);
        assert_eq!(after.drift_ok, Some(true));
        assert!(app.current_design_results().is_some());
        assert_eq!(
            rmp_serde::to_vec_named(&app.core.scoped.results).unwrap(),
            before_results
        );
        app.core.model.stress_cfg.drift_limit_denom = f64::NAN;
        let invalid = seismic_story_metrics(&app).remove(0);
        assert_eq!(invalid.drift.unwrap().unwrap().angle, angle);
        assert_eq!(invalid.drift_ok, None);
        assert!(app.current_design_results().is_some());
    }

    #[test]
    fn drift_490_app_and_csv_share_case_direction_provenance_and_reason() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        assert!(app.core.scoped.last_error.is_none());
        let st = &mut app.core.scoped.results.as_mut().unwrap().statics[0].1;
        st.disp[2][0] = 18.0;
        st.disp[3][0] = 10.0;
        app.core.analysis_cfg.seismic_dir = SeismicDir::X;
        let mut other_case = app.core.scoped.results.as_ref().unwrap().statics[0]
            .1
            .clone();
        other_case.disp.fill([999.0; 6]);
        let user_case = StaticCaseKey::User(sepika_core::ids::LoadCaseId(0));
        app.core
            .scoped
            .results
            .as_mut()
            .unwrap()
            .statics
            .push((user_case, other_case));
        app.core.scoped.last_static = Some(StaticKey::Case(user_case));
        let metric = seismic_story_metrics(&app).remove(0);
        assert_eq!(
            metric
                .drift
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .column
                .drift,
            18.0
        );
        let cells = drift_cells(&metric);
        let csv = build_report_csv(&app);
        assert!(csv.contains("地震静的,X,3500.000"));
        for cell in &cells {
            assert!(csv.contains(cell), "{cell}: {csv}");
        }
        app.core.analysis_cfg.seismic_dir = SeismicDir::Y;
        let m = seismic_story_metrics(&app).remove(0);
        assert!(m.drift.unwrap_err().contains("地震静的 Y"));
        assert!(build_report_csv(&app).contains("未計算: 地震静的 Y"));
        app.core.analysis_cfg.seismic_dir = SeismicDir::X;
        app.core.model.nodes[2].coord[0] += 10.0;
        let m = seismic_story_metrics(&app).remove(0);
        assert!(m.drift.as_ref().unwrap_err().contains("不整合"));
        assert!(build_report_csv(&app).contains(m.drift.as_ref().unwrap_err()));
        app.core.model.stories[1].elevation = Some(f64::NAN);
        assert!(seismic_story_metrics(&app).is_empty());
        assert!(build_report_csv(&app).contains("層間変形角 算定不能"));
    }

    #[test]
    fn fes_718_missing_height_displacement_and_direction_are_unavailable() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        app.run_seismic(SeismicDir::Y);
        assert!(app.core.scoped.last_error.is_none());
        let results = app.core.scoped.results.as_ref().unwrap();
        let ctx = metrics_ctx_from_results(&app.core.model, Some(results));
        let normal = compute_story_metrics_with(&app.core.model, SeismicDir::X, &ctx);
        assert_eq!(normal[0].rs, Ok(1.0));
        assert_eq!(normal[0].re, Ok(0.0));
        assert_eq!(normal[0].fes, Ok(1.0));
        let absent =
            compute_story_metrics_with(&app.core.model, SeismicDir::X, &StoryMetricsCtx::default());
        assert!(absent[0].re.as_ref().unwrap_err().contains("X/Y 方向"));
        assert_eq!(absent[0].re_ok, None);
        assert!(absent[0].fes.is_err());

        let mut missing = ctx.seismic_x.unwrap().clone();
        missing.disp.truncate(1);
        let missing_ctx = StoryMetricsCtx {
            seismic_x: Some(&missing),
            ..ctx.clone()
        };
        let invalid = compute_story_metrics_with(&app.core.model, SeismicDir::X, &missing_ctx);
        assert!(invalid[0].rs.is_err());
        assert!(invalid[0].re.is_err());
        assert_eq!(invalid[0].rs_ok, None);
        assert_eq!(invalid[0].re_ok, None);
        assert!(invalid[0].fes.is_err());

        let mut model = app.core.model.clone();
        model.stories[1].elevation = Some(f64::NAN);
        let invalid = compute_story_metrics_with(&model, SeismicDir::X, &ctx);
        assert!(invalid.is_empty());
        assert!(!model.story_level_diagnostics().is_empty());
    }

    #[test]
    fn test_report_and_metrics_from_sample_flow() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        app.run_seismic(SeismicDir::X);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );

        // 層指標
        let ctx = metrics_ctx_from_results(&app.core.model, app.core.scoped.results.as_ref());
        let metrics = compute_story_metrics_with(&app.core.model, SeismicDir::X, &ctx);
        assert_eq!(metrics.len(), 1);
        assert!(
            metrics[0]
                .drift
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .column
                .drift
                > 0.0
        );
        assert!(*metrics[0].rs.as_ref().unwrap() > 0.0);

        // レポート
        let csv = build_report_csv(&app);
        assert!(csv.contains("[モデル概要]"));
        assert!(csv.contains("[層指標(二次設計)]"));
        assert!(csv.contains("[部材検定]"));
        // 節点単位の検定（柱梁接合部・仕口パネル・耐震壁）もレポートへ出力する。
        // サンプルは S 造ラーメンのため、パネルゾーンの検定行が現れる。
        assert!(csv.contains("[接合部検定]"), "{csv}");
        assert!(csv.contains("パネルゾーン"), "{csv}");
        // 数量積算セクションも常時含まれる。
        assert!(csv.contains("[数量積算 部位別]"));
    }

    /// 未設定の割当領域があるとき、CSV レポートに件数と対象 ID を記載する。
    #[test]
    fn test_report_csv_records_unset_assignment_regions() {
        use sepika_core::ids::{ElemId, NodeId};
        use sepika_core::model::{
            ElementData, ElementKind, EndCondition, ForceRegime, LocalAxis, Node,
        };
        let mut app = App::default();
        let mut model = Model::default();
        for (i, (x, y)) in [(0.0, 0.0), (4000.0, 0.0), (4000.0, 4000.0), (0.0, 4000.0)]
            .into_iter()
            .enumerate()
        {
            model.nodes.push(Node {
                id: NodeId(i as u32),
                coord: [x, y, 0.0],
                restraint: Default::default(),
                mass: None,
                story: None,
                support_spring: None,
            });
        }
        for (i, (a, b)) in [(0u32, 1u32), (1, 2), (2, 3), (3, 0)]
            .into_iter()
            .enumerate()
        {
            model.elements.push(ElementData {
                id: ElemId(i as u32),
                kind: ElementKind::Beam,
                nodes: [NodeId(a), NodeId(b)].into_iter().collect(),
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
            });
        }
        model.rebuild_floor_assignment_regions();
        app.core.model = model;
        let csv = build_report_csv(&app);
        assert!(csv.contains("[未設定の割当領域]"), "{csv}");
        assert!(csv.contains("床板 未設定件数,1"), "{csv}");
        assert!(csv.contains("床板 対象ID,R0"), "{csv}");
    }

    #[test]
    fn test_quantity_csv_from_sample_model() {
        // サンプルモデル（門型ラーメン）で数量積算 CSV が生成される
        // エンドツーエンド確認。柱・大梁が分類され、合計行が出力される。
        let model = crate::sample::portal_frame();
        let csv = build_quantity_csv(&model);
        assert!(csv.contains("[数量積算 部位別]"), "{csv}");
        assert!(csv.contains("柱"), "{csv}");
        assert!(csv.contains("大梁"), "{csv}");
        assert!(csv.contains("合計"), "{csv}");
        assert!(csv.contains("[数量積算 明細]"));
        assert!(csv.contains("[数量積算 注記]"));
    }

    #[test]
    fn test_drift_limit_denom_relaxation() {
        let mut app = App::default();
        app.load_model(crate::sample::portal_frame());
        app.generate_stories_action();
        app.core.model.stress_cfg.drift_limit_denom = 120.0;
        app.run_seismic(SeismicDir::X);
        assert!(
            app.core.scoped.last_error.is_none(),
            "{:?}",
            app.core.scoped.last_error
        );

        let ctx = metrics_ctx_from_results(&app.core.model, app.core.scoped.results.as_ref());
        let metrics = compute_story_metrics_with(&app.core.model, SeismicDir::X, &ctx);
        assert_eq!(metrics[0].drift_limit_denom, 120.0);
        assert_eq!(
            metrics[0].drift_ok,
            Some(metrics[0].drift.as_ref().unwrap().as_ref().unwrap().angle <= 1.0 / 120.0)
        );
        let csv = build_report_csv(&app);
        assert!(csv.contains("設定値との参考比較"));
        assert!(csv.contains(",120,"));
    }

    #[test]
    fn test_report_without_results() {
        let app = App::default();
        let csv = build_report_csv(&app);
        assert!(csv.contains("解析結果なし"));
        assert!(!has_report_content(&app.core.scoped.results));
    }
    #[test]
    fn panel_505_csv_keeps_incomplete_and_checked_planes_separate() {
        use crate::app::{JointCheck, ResultsBundle};
        use sepika_design_jp::{CheckComponent, CheckKind, CheckOutcome, CheckResult};
        let mut app = App::default();
        let results = ResultsBundle {
            joint_checks: vec![
                JointCheck {
                    node: sepika_core::ids::NodeId(0),
                    label: "パネルゾーン(S) X-Z".into(),
                    outcome: CheckOutcome::Skipped {
                        reason: "接合部 ID 0 db=580/480 上位置差=50 下位置差=-50 段違い".into(),
                    },
                },
                JointCheck {
                    node: sepika_core::ids::NodeId(0),
                    label: "パネルゾーン(S) Y-Z".into(),
                    outcome: CheckOutcome::Checked(CheckResult {
                        basis: "標準形式".into(),
                        detail: String::new(),
                        components: vec![CheckComponent {
                            kind: CheckKind::Shear,
                            ratio: 0.5,
                            detail: String::new(),
                        }],
                    }),
                },
            ],
            ..Default::default()
        };
        app.core.scoped.results = Some(results);
        app.certify_design_fixture();
        let csv = build_report_csv(&app);
        assert!(csv.contains("パネルゾーン(S) X-Z,-,検定不能,接合部 ID 0"));
        assert!(csv.contains("上位置差=50 下位置差=-50"));
        assert!(csv.contains("パネルゾーン(S) Y-Z,0.5000,OK,標準形式"));
    }
}
