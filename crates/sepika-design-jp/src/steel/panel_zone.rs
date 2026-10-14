//! S 造パネルゾーンの断面検定（許容応力度検定。
//! 鋼構造接合部設計指針のパネルゾーン部分に準拠）。
//!
//! 数値入力を受け取る純関数として実装する。パネルの寸法・形状係数 κ・
//! 実効体積 `Ve` は仕口パネルのモデル化と同一の値
//! （[`sepika_core::panel_zone::PanelGeometry`] を唯一の出所とする）。

use crate::{CheckComponent, CheckKind, CheckResult};
use sepika_core::panel_zone::{
    standard_flange_differences, BeamFlangePositions, PanelGeometry, PanelShapeKind,
};

/// S 造パネルゾーンの検定の入力。
pub struct SPanelInput {
    /// 局所上下位置を検証する梁対。片側梁は同じ位置を2回渡す。未指定は未算定。
    pub beam_flanges: Option<[BeamFlangePositions; 2]>,
    /// 柱断面から解決したパネル諸元（寸法 `dc`・板厚 `tp`・断面区分）。
    /// CFT も対象に含める（鋼管部を S 造と同じ式で評価する）。
    pub geometry: PanelGeometry,
    /// 梁フランジ板厚中心間距離 db `[mm]`。
    pub db: f64,
    /// パネルの降伏強さ F 値 [N/mm²]。
    pub fy: f64,
    /// 軸力比 n = N / (Fy・A)（符号は問わない。内部で絶対値を用いる）。
    pub axial_ratio: f64,
    /// 左梁フェイスモーメント [N·mm]（符号付き）。
    pub beam_moment_left: f64,
    /// 右梁フェイスモーメント [N·mm]（符号付き）。
    pub beam_moment_right: f64,
    /// 上柱せん断力 `[N]`。
    pub col_shear_upper: f64,
    /// 下柱せん断力 `[N]`。
    pub col_shear_lower: f64,
    /// 設計用パネルモーメント `pM` [N·mm] を直接与える場合の値。
    ///
    /// 仕口パネルをモデル化した接合部では、解析が出力するパネルせん断モーメント
    /// `{MSX, MSY}` を検討面へ投影した値を与える。節点まわりのモーメント
    /// 釣り合いが解析上厳密に満たされた値であり、梁端モーメント・柱せん断から
    /// 手で組み立てる近似（`beam_moment_*` / `col_shear_*`）を経ない。
    ///
    /// `None` のときは
    /// `pM = bML + bMR − (cQU + cQL)・db/2` で組み立てる。
    pub design_moment: Option<f64>,
}

/// 上下フランジ位置が一致する標準形式の検定。未検証・段違いは理由付きエラー。
pub fn s_panel_zone_check(inp: &SPanelInput) -> Result<CheckResult, String> {
    let [left, right] = inp
        .beam_flanges
        .ok_or("上下フランジ位置未検証: dbだけでは標準形式を確認できません")?;
    let du = left.upper - right.upper;
    let dl = left.lower - right.lower;
    if !standard_flange_differences(du, dl) {
        return Err(format!("段違い接合の標準形式は未対応: 左db={} mm、右db={} mm、上位置差={du} mm、下位置差={dl} mm", left.db(), right.db()));
    }
    if [left, right].iter().any(|p| {
        !p.db().is_finite()
            || p.db() <= 0.0
            || (p.db() - inp.db).abs() > 2.0 * sepika_core::panel_zone::FLANGE_POSITION_TOL_MM
    }) {
        return Err("dbとフランジ位置が不整合です".into());
    }
    let ve = inp.geometry.effective_volume(inp.db);
    let kappa = inp.geometry.kappa();
    let shape_label = match inp.geometry.kind {
        PanelShapeKind::H { .. } => "H形",
        PanelShapeKind::Box { .. } => "角形",
        PanelShapeKind::Pipe => "円形",
    };

    let n = inp.axial_ratio.abs();
    let reduction = if n >= 1.0 { 0.0 } else { (1.0 - n * n).sqrt() };

    let kappa = if kappa.abs() > 1e-9 { kappa } else { 1e-9 };
    let p_my = ve / kappa * reduction * inp.fy / 3f64.sqrt();
    let p_m = inp.design_moment.unwrap_or_else(|| {
        inp.beam_moment_left + inp.beam_moment_right
            - (inp.col_shear_upper + inp.col_shear_lower) * inp.db / 2.0
    });

    let ratio = if p_my > 0.0 {
        p_m.abs() / p_my
    } else {
        f64::INFINITY
    };
    let basis = format!("鋼構造接合部設計指針 パネルゾーン検定 {}断面", shape_label);
    Ok(CheckResult {
        basis,
        detail: String::new(),
        components: vec![CheckComponent {
            kind: CheckKind::Shear,
            ratio,
            detail: format!(
                "Ve={:.1} mm3, kappa={:.4}, n={:.4}, pM={:.1} N*mm, pMy={:.1} N*mm, ratio={:.4}",
                ve, kappa, n, p_m, p_my, ratio
            ),
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_panel_h_input(axial_ratio: f64) -> SPanelInput {
        SPanelInput {
            beam_flanges: Some(
                [BeamFlangePositions {
                    upper: 250.0,
                    lower: -250.0,
                }; 2],
            ),
            geometry: PanelGeometry {
                kind: PanelShapeKind::H {
                    bc: 300.0,
                    tf: 20.0,
                },
                dc: 400.0,
                tp: 12.0,
                filled: false,
            },
            db: 500.0,
            fy: 235.0,
            axial_ratio,
            beam_moment_left: 200_000_000.0,
            beam_moment_right: 200_000_000.0,
            col_shear_upper: 50_000.0,
            col_shear_lower: 50_000.0,
            design_moment: None,
        }
    }

    /// CFT 柱も断面検定の対象に含める（鋼管部を S 造と同じ式で評価する）。
    /// 仕口パネルの**モデル化**は CFT を除外するため、対象範囲が食い違う点を押さえる。
    #[test]
    fn s_panel_covers_cft_columns() {
        let steel = PanelGeometry {
            kind: PanelShapeKind::Box { bc: 400.0 },
            dc: 384.0,
            tp: 16.0,
            filled: false,
        };
        let cft = PanelGeometry {
            filled: true,
            ..steel
        };
        assert!(!steel.filled, "角形鋼管はモデル化の対象");
        assert!(cft.filled, "CFT はモデル化の対象外");

        // 検定側は充填の有無で結果が変わらない。
        let check = |geometry: PanelGeometry| {
            let mut inp = base_panel_h_input(0.0);
            inp.geometry = geometry;
            s_panel_zone_check(&inp).unwrap().ratio()
        };
        let (rs, rc) = (check(steel), check(cft));
        assert!(rs > 0.0 && rs.is_finite());
        assert!(
            (rs - rc).abs() < 1e-12,
            "CFT も同じ検定比になる: {rs} vs {rc}"
        );
    }

    /// `design_moment` を与えると、梁端モーメント・柱せん断からの組み立てに代えて
    /// その値が設計用パネルモーメント pM に用いられる（仕口パネルをモデル化した
    /// 接合部で、解析出力のせん断モーメントを使う経路）。
    #[test]
    fn s_panel_uses_design_moment_when_given() {
        let assembled = s_panel_zone_check(&base_panel_h_input(0.0)).unwrap();
        // 手組み式の pM = 200e6 + 200e6 − (50e3 + 50e3)・500/2 = 375e6
        let assembled_pm = 375_000_000.0_f64;

        let mut inp = base_panel_h_input(0.0);
        inp.design_moment = Some(assembled_pm);
        let same = s_panel_zone_check(&inp).unwrap();
        assert!(
            (same.ratio() - assembled.ratio()).abs() < 1e-12,
            "同じ pM を与えれば結果は一致する: {} vs {}",
            same.ratio(),
            assembled.ratio()
        );

        // 別の値を与えれば検定比は pM に比例して変わる。
        let mut half = base_panel_h_input(0.0);
        half.design_moment = Some(assembled_pm / 2.0);
        let half = s_panel_zone_check(&half).unwrap();
        assert!(
            (half.ratio() - assembled.ratio() / 2.0).abs() < 1e-12,
            "pM が半分なら検定比も半分: {} vs {}",
            half.ratio(),
            assembled.ratio() / 2.0
        );

        // 符号は絶対値で扱う（左右どちら向きのパネルモーメントでも同じ検定比）。
        let mut neg = base_panel_h_input(0.0);
        neg.design_moment = Some(-assembled_pm);
        assert!((s_panel_zone_check(&neg).unwrap().ratio() - assembled.ratio()).abs() < 1e-12);
    }

    #[test]
    fn s_panel_axial_ratio_reduces_capacity() {
        let n0 = rc_or_s_pmy(&base_panel_h_input(0.0));
        let n08 = rc_or_s_pmy(&base_panel_h_input(0.8));
        assert!(n08 < n0, "n=0.8 の pMy は n=0 より小さいはず");
        // sqrt(1-0.8^2) = 0.6 のスケーリングになっていることを確認。
        assert!((n08 / n0 - 0.6).abs() < 1e-9);
    }

    // pMy を検定比から逆算するテスト用ヘルパ。
    fn rc_or_s_pmy(inp: &SPanelInput) -> f64 {
        let res = s_panel_zone_check(inp).unwrap();
        let p_m = inp.beam_moment_left + inp.beam_moment_right
            - (inp.col_shear_upper + inp.col_shear_lower) * inp.db / 2.0;
        p_m.abs() / res.ratio()
    }

    #[test]
    fn s_panel_moment_hand_calc() {
        let inp = base_panel_h_input(0.0);
        let res = s_panel_zone_check(&inp).unwrap();
        let expected_pm = inp.beam_moment_left + inp.beam_moment_right
            - (inp.col_shear_upper + inp.col_shear_lower) * inp.db / 2.0;
        // pM = 200e6+200e6 - (50000+50000)*500/2 = 400e6 - 25e6 = 375e6
        assert!((expected_pm - 375_000_000.0).abs() < 1e-3);
        assert!(res.ratio() > 0.0);
    }

    #[test]
    fn s_panel_axial_ratio_at_or_above_one_clamps_to_zero() {
        let inp = base_panel_h_input(1.0);
        let res = s_panel_zone_check(&inp).unwrap();
        assert!(
            res.ratio().is_infinite(),
            "pMy=0 のとき ratio は無限大になる"
        );
        let inp2 = base_panel_h_input(1.5);
        let res2 = s_panel_zone_check(&inp2).unwrap();
        assert!(res2.ratio().is_infinite());
    }
    #[test]
    fn panel_505_positions_gate_precedes_direct_moment_and_rejects_db_only() {
        let mut inp = base_panel_h_input(0.0);
        inp.db = 580.0;
        let positions = BeamFlangePositions::from_dimensions(600.0, 20.0, 20.0, 0.0).unwrap();
        inp.beam_flanges = Some([positions; 2]);
        let check = s_panel_zone_check(&inp).unwrap();
        assert!(check.components[0].detail.contains("pM=371000000.0 N*mm"));
        inp.beam_moment_left = -200_000_000.0;
        inp.beam_moment_right = -200_000_000.0;
        inp.col_shear_upper = -50_000.0;
        inp.col_shear_lower = -50_000.0;
        assert!((s_panel_zone_check(&inp).unwrap().ratio() - check.ratio()).abs() < 1e-12);
        inp.beam_moment_left = 200_000_000.0;
        inp.beam_moment_right = 200_000_000.0;
        inp.col_shear_upper = 50_000.0;
        inp.col_shear_lower = 50_000.0;
        for direct in [None, Some(371_000_000.0), Some(-371_000_000.0)] {
            inp.design_moment = direct;
            let standard = s_panel_zone_check(&inp).unwrap();
            assert!((standard.ratio() - check.ratio()).abs() < 1e-12);
            inp.beam_flanges = None;
            assert!(s_panel_zone_check(&inp).unwrap_err().contains("位置未検証"));
            inp.beam_flanges = Some([
                positions,
                BeamFlangePositions::from_dimensions(600.0, 20.0, 20.0, 20.0).unwrap(),
            ]);
            assert!(s_panel_zone_check(&inp)
                .unwrap_err()
                .contains("上位置差=-20"));
            inp.beam_flanges = Some([positions; 2]);
        }
    }
}
