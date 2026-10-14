/// 数量積算の設定。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QuantityCfg {
    /// 主筋・壁筋の定着長さ係数（L2・S = 係数×呼び径。既定 35d）。
    pub anchorage_dia_factor: f64,
    /// 壁筋の仮定呼び径 `[mm]`。既定 10。有限かつ正を数量算定時に要求する。
    pub assumed_wall_bar_dia: f64,
    /// 小梁主筋の鉄筋比（コンクリート体積比。既定 0.8%）。
    pub beam_main_ratio: f64,
    /// 小梁スターラップの鉄筋比（既定 0.1%）。
    pub beam_stirrup_ratio: f64,
    /// 床・片持ち床の鉄筋比（既定 1.0%）。
    pub slab_rebar_ratio: f64,
}

impl Default for QuantityCfg {
    fn default() -> Self {
        Self {
            anchorage_dia_factor: 35.0,
            assumed_wall_bar_dia: 10.0,
            beam_main_ratio: 0.008,
            beam_stirrup_ratio: 0.001,
            slab_rebar_ratio: 0.01,
        }
    }
}
