//! 終局検定の入力・算定オプション。
//!
//! - [`MemberDemand`] — 部材の設計用需要（軸力・二軸曲げ・せん断・Rp 等）。
//! - [`ShearMethod`] — 終局せん断強度の算定方法（塑性理論式 / 靭性指針式）。
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

    /// 強軸のせん断または付着の余裕度を表す表示用の式。
    pub fn strong_shear_margin_formula(self, capacity: &str) -> String {
        let demand = self.strong_shear_demand_symbol();
        match self {
            Self::StaticHinge => format!("max({capacity}−QL, 0)/{demand}"),
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
/// 静的ヒンジ需要は強軸の QL を要し、二軸せん断の判定には弱軸の QL も参照する。
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
    /// 部材別のヒンジ回転角 Rp `[rad]`（プッシュオーバー終局時の部材変形角）。`Some`
    /// のとき ν・cotφ・μ・tanθ に用いる Rp を [`UltimateShearOptions::rp`] から置き換える。
    pub rp: Option<f64>,
    /// 強軸の長期せん断力 QL `[N]`。静的ヒンジでは必須、確定総応答では `None`（追加なし）。
    pub q_long: Option<f64>,
    /// 弱軸の長期せん断力 QL `[N]`。静的ヒンジの二軸せん断で参照し、確定総応答では `None`。
    pub q_long_weak: Option<f64>,
}

impl MemberDemand {
    /// 軸力のみ（曲げ・せん断需要 0、Rp は UI 一律指定に従う）の需要を作る。
    pub fn axial(n_axial: f64) -> Self {
        Self {
            mode: UltimateDemandMode::StaticHinge,
            n_axial,
            ..Self::default()
        }
    }

    /// プッシュオーバー応答から部材需要を作る。軸力（圧縮正）・強軸/弱軸の設計用曲げ・
    /// 強軸/弱軸の設計用せん断・部材別 Rp を直接反映する（`Qmu`・弱軸 Qmuy・Rp を
    /// 応答値で置き換える）。
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

/// 終局せん断強度の算定方法（塑性理論式／靭性指針式の選択に対応）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShearMethod {
    /// 塑性理論式（藤井・森田式系、「終局強度型設計指針」）。既定。
    #[default]
    Plastic,
    /// 靭性指針式 Vu=min(Vu1,Vu2,Vu3)（AIJ「靭性保証型耐震設計指針」6.4）。
    Ductility,
}

impl ShearMethod {
    /// 表示用の耐力記号を（せん断、付着）の順で返す。
    pub fn capacity_symbols(self) -> (&'static str, &'static str) {
        match self {
            Self::Plastic => ("Qsu", "Qbu"),
            Self::Ductility => ("Vu", "Vbu"),
        }
    }
}

/// 終局検定の算定オプション。
#[derive(Clone, Debug)]
pub struct UltimateShearOptions {
    /// 終局限界状態でのヒンジ領域の回転角 Rp `[rad]`（ν・cotφ に用いる。既定 0）。
    pub rp: f64,
    /// 軽量コンクリートを使用する場合 true（せん断終局耐力を 0.9 倍に低減）。
    pub lightweight: bool,
    /// せん断需要に掛ける上限強度倍率（無次元、既定 1.0）。
    /// 静的・両端ヒンジ略算は有限の正値、確定総応答は 1.0 を指定する。
    pub upper_strength_factor: f64,
    /// せん断補強筋の降伏強度算定用強度 σwy [N/mm²]（断面に材料が割り当てられて
    /// いない場合の代表値。既定 295 = SD295 相当）。
    pub sigma_wy: f64,
    /// 付着割裂の検定を含める場合 true。
    pub include_bond: bool,
    /// 終局せん断・付着耐力の算定方法（既定 塑性理論式）。
    pub shear_method: ShearMethod,
    /// RC 柱の二軸せん断を検定する場合 true。
    /// 静的ヒンジでの二軸せん断余裕度は、両軸の QL が明示的に 0 の場合に算定する。
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
            rp: 0.0,
            lightweight: false,
            upper_strength_factor: 1.0,
            sigma_wy: 295.0,
            include_bond: true,
            shear_method: ShearMethod::default(),
            biaxial_shear: false,
            biaxial_bending: false,
        }
    }
}
