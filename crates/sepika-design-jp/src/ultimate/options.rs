//! 終局検定の入力・算定オプション。
//!
//! - [`MemberDemand`] — 部材の設計用需要（軸力・二軸曲げ・せん断・Rp 等）。
//! - [`UltimateShearOptions`] — 終局検定の算定オプション。

/// 終局需要の評価方式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UltimateDemandMode {
    #[default]
    StaticHinge,
    ConfirmedTotal,
}

impl UltimateDemandMode {
    /// 比較に用いる強軸せん断需要の表示記号。
    pub fn strong_shear_demand_symbol(self) -> &'static str {
        match self {
            Self::StaticHinge => "Qmu",
            Self::ConfirmedTotal => "|Qtotal|",
        }
    }

    /// 強軸せん断余裕度の表示式。静的・両端ヒンジ略算では「未算定」。
    pub fn strong_shear_margin_formula(self, capacity: &str) -> String {
        let demand = self.strong_shear_demand_symbol();
        match self {
            Self::StaticHinge => "未算定".into(),
            Self::ConfirmedTotal => format!("{capacity}/{demand}"),
        }
    }
}

/// 静的RC梁の強軸曲げ符号の確認結果。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StaticBeamBending {
    /// `MemberDemand::mz` の符号で引張側を指定する。
    #[default]
    Signed,
    /// 部材の評価位置に正と負の強軸曲げがある。
    Mixed,
    /// 符号を集約前の評価位置から確認できない。
    Unverified,
}

/// 設計者が対象の載荷方向で確認した、RC梁両端の強軸曲げ降伏時の引張側。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaticBeamHingeMechanism {
    /// i端が上端引張、j端が下端引張。
    ITopJBottom,
    /// i端が下端引張、j端が上端引張。
    IBottomJTop,
}

/// 圧縮正の軸力 `[N]`、曲げ `[N·mm]` と方式を持つ終局需要。
/// 静的ヒンジ需要は強軸の QL を要する。荒川mean式には確定総応答が必要。
#[derive(Clone, Copy, Debug, Default)]
pub struct MemberDemand {
    pub mode: UltimateDemandMode,
    /// 設計軸力 `[N]`（**圧縮正**）。柱の Mu・軸余裕度に用いる。
    pub n_axial: f64,
    /// 強軸曲げ [N·mm]。符号を確認した静的RC梁は、最大絶対値をとる評価位置の符号を保つ（負が上端引張）。
    pub mz: f64,
    /// 静的RC梁の強軸曲げ符号。直接の需要入力では既定で符号付きの値を指定する。
    pub static_beam_bending: StaticBeamBending,
    /// 静的RC梁で、伝達・定着と他の破壊の先行を照査し、両端の逆符号降伏を確認した場合に指定する。
    /// 既定の `None` では、長期応力や端部拘束だけから降伏機構を推定しない。
    pub static_beam_hinge_mechanism: Option<StaticBeamHingeMechanism>,
    /// 弱軸（幅方向）まわりの設計用曲げモーメント Mmy [N·mm]（符号は内部で abs）。
    pub my: f64,
    /// 設計用せん断力 Qm `[N]`（プッシュオーバー応答の強軸せん断。`Some` のとき
    /// 確定総応答モードで `Qmu = |Qm|` として採用する。上限強度倍率は1のみ対応）。
    pub shear: Option<f64>,
    /// 弱軸の確定総せん断力 `[N]`。総応答モードで `Qmuy = |Qmy|` として採用する。
    pub shear_weak: Option<f64>,
    /// 検討区間で確認した最小軸力 [N]（圧縮正）。未確認は None。
    /// 部材荷重がなく、両フェースを含む区間の確定総応答として曲げとせん断を確認し、最小軸力を得た場合にだけ指定できる。
    pub min_axial_for_shear_n: Option<f64>,
    /// 大地震時の付着応力度と必要延長を確認する配筋組。未確認は None。
    pub bond: Option<super::rc_bond::RcUltimateBondDemand>,
    /// 確定応答の部材変形角 [rad]。
    pub rp: Option<f64>,
    /// 強軸の長期せん断力 QL `[N]`。静的ヒンジでは必須、確定総応答では `None`（追加なし）。
    pub q_long: Option<f64>,
    /// 弱軸の長期せん断力 QL `[N]`。確定総応答では `None`。
    pub q_long_weak: Option<f64>,
}

impl MemberDemand {
    /// 軸力のみ（曲げ・せん断需要 0）の需要を作る。
    pub fn axial(n_axial: f64) -> Self {
        Self {
            mode: UltimateDemandMode::StaticHinge,
            n_axial,
            ..Self::default()
        }
    }

    /// プッシュオーバー応答から部材需要を作る。軸力（圧縮正）・強軸/弱軸の設計用曲げ・
    /// 強軸/弱軸の設計用せん断・部材別 Rp を直接反映する（`Qmu`・弱軸 Qmuy・Rp を
    /// 応答値で置き換える）。検討区間の最小軸力は別途確認して指定する。
    #[allow(clippy::too_many_arguments)]
    pub fn from_pushover(
        n_axial: f64,
        mz: f64,
        my: f64,
        shear: f64,
        shear_weak: f64,
        rp: f64,
    ) -> Self {
        Self {
            mode: UltimateDemandMode::ConfirmedTotal,
            n_axial,
            mz,
            my,
            shear: Some(shear),
            shear_weak: Some(shear_weak),
            rp: Some(rp),
            ..Self::default()
        }
    }
}

/// 終局検定の算定オプション。
#[derive(Clone, Debug)]
pub struct UltimateShearOptions {
    /// RC柱の静的・両端ヒンジ略算の参考せん断需要に掛ける倍率（無次元、既定 1.0）。
    /// 静的・両端ヒンジ略算は有限の正値、確定総応答は 1.0 を指定する。
    pub upper_strength_factor: f64,
    /// 大地震時の付着照査を有効にする場合 true。カットオフ筋の必要延長は応力度と独立に判定する。
    pub include_bond: bool,
    /// RC 柱の二軸せん断を検定する場合 true。
    /// 両軸の確定総応答に対する終局せん断耐力が必要。
    pub biaxial_shear: bool,
    /// 柱の曲げを 2 軸曲げとして検定する場合 true（採用応力）。
    /// 両軸の設計用曲げ Mm と終局曲げ強度 Mu を相互作用式
    /// `1/((Mmx/Mux)^αx+(Mmy/Muy)^αy)^(1/α)`（RC は α=2.0）で合成する。
    /// 設計用曲げ需要 Mmx/Mmy は [`MemberDemand`] の mz/my を用いる。
    pub biaxial_bending: bool,
}

impl Default for UltimateShearOptions {
    fn default() -> Self {
        Self {
            upper_strength_factor: 1.0,
            include_bond: true,
            biaxial_shear: false,
            biaxial_bending: false,
        }
    }
}
