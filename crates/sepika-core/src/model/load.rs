//! 荷重関連の型（節点荷重・部材荷重・荷重ケース・荷重条件など）。

use super::*;

/// 節点に作用する荷重。同一の荷重ケース内で 1 つの節点に何件でも定義できる
/// （解析では全件が加算される）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NodalLoad {
    pub node: NodeId,
    pub values: [f64; 6],
    /// 利用者が付けた荷重の名称。空文字は無名（一覧では成分値から作った
    /// 自動ラベルで表示する）。
    pub name: String,
    /// この荷重を作ったのが準備計算か利用者か。準備計算が作った荷重は同期のたびに
    /// 再生成されるため、利用者は編集・削除できない（[`LoadSource`] を参照）。
    pub source: LoadSource,
}

/// 荷重をどこが作ったか。準備計算による自動生成分と利用者の手入力を区別し、
/// 自動同期が手入力を消さないようにする。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoadSource {
    /// 利用者が入力した荷重。自動同期の対象外で、消えることはない。
    #[default]
    Manual,
    /// 準備計算（床荷重の分配・自重の集計・Ai 分布の水平力）が生成した荷重。
    Auto,
    /// 片持ちスラブの先端荷重から同期された部材荷重。
    SlabTip,
    /// 同期DLのうち、物理壁版に由来する自重。階帯重量と独立に識別する。
    WallSelfWeight,
}

impl LoadSource {
    /// 準備計算が生成した荷重か。
    pub fn is_auto(self) -> bool {
        matches!(
            self,
            LoadSource::Auto | LoadSource::SlabTip | LoadSource::WallSelfWeight
        )
    }
}

impl NodalLoad {
    /// 利用者入力の節点荷重（名称なし）を作る。
    pub fn manual(node: NodeId, values: [f64; 6]) -> Self {
        Self {
            node,
            values,
            name: String::new(),
            source: LoadSource::Manual,
        }
    }

    /// 準備計算が生成する節点荷重を作る。
    pub fn auto(node: NodeId, values: [f64; 6]) -> Self {
        Self {
            node,
            values,
            name: String::new(),
            source: LoadSource::Auto,
        }
    }
}

/// 部材（梁）荷重の種別。位置・強度はすべて部材ローカル x 軸（i→j）に沿った
/// 距離 [mm] と強度で与える。作用方向は `MemberLoad::dir`（全体座標）で指定する。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MemberLoadKind {
    /// 中間集中荷重: i 端から距離 `a` [mm] の位置に大きさ `p` [N]。
    Point { a: f64, p: f64 },
    /// 区間分布荷重: [`a`, `b`] 区間に強度 `w1`→`w2` [N/mm] の線形分布。
    /// 等分布は `w1 == w2`、全長は `a = 0, b = L`、三角形は端の強度を 0 にする。
    Distributed { a: f64, b: f64, w1: f64, w2: f64 },
}

/// 部材荷重の区間指定。既定は i 端からの固定距離 [mm]。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemberLoadExtent {
    #[default]
    FixedDistance,
    /// 利用者入力の全長等分布。強度 [N/mm] を保持して現在材長へ追従する。
    FullLengthUniform,
}

/// 部材に作用する荷重。`dir` は全体座標系での作用方向（内部で正規化）。
/// 既定の重力方向は `[0.0, 0.0, -1.0]`。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MemberLoad {
    pub elem: ElemId,
    pub dir: [f64; 3],
    pub kind: MemberLoadKind,
    /// 全長追従か、i 端からの固定距離 [mm] か。省略時は固定距離。
    #[serde(default)]
    pub extent: MemberLoadExtent,
    /// 利用者が付けた荷重の名称。空文字は無名（[`NodalLoad::name`] と同じ規約）。
    pub name: String,
    /// 準備計算が生成した荷重か（[`NodalLoad::source`] と同じ規約）。
    pub source: LoadSource,
}

impl MemberLoad {
    /// 利用者入力の部材荷重（名称なし）を作る。
    pub fn manual(elem: ElemId, dir: [f64; 3], kind: MemberLoadKind) -> Self {
        Self {
            elem,
            dir,
            kind,
            extent: MemberLoadExtent::FixedDistance,
            name: String::new(),
            source: LoadSource::Manual,
        }
    }

    /// 全長等分布を作る。材長 [mm] と強度 [N/mm] を与え、方向は全体座標で保持する。
    pub fn full_length_uniform(elem: ElemId, dir: [f64; 3], length_mm: f64, w: f64) -> Self {
        Self {
            extent: MemberLoadExtent::FullLengthUniform,
            ..Self::manual(
                elem,
                dir,
                MemberLoadKind::Distributed {
                    a: 0.0,
                    b: length_mm,
                    w1: w,
                    w2: w,
                },
            )
        }
    }

    /// 全長追従と分布諸元の整合を検証する。固定距離は検証対象外。
    /// 全長追従は材長 [mm] が非正・非有限、または指定が矛盾すれば失敗する。
    pub fn validate_extent(&self, length_mm: f64) -> Result<(), String> {
        if self.extent == MemberLoadExtent::FixedDistance {
            return Ok(());
        }
        if self.source != LoadSource::Manual {
            return Err("全長追従は利用者入力荷重にのみ指定できます".into());
        }
        if !length_mm.is_finite() || length_mm <= 1e-9 {
            return Err("全長追従の材長が非正または非有限です".into());
        }
        match self.kind {
            MemberLoadKind::Distributed { a, b, w1, w2 }
                if a == 0.0 && b.is_finite() && b == length_mm && w1.is_finite() && w1 == w2 =>
            {
                Ok(())
            }
            _ => Err("全長追従は a=0、b=現在材長、w1=w2 の等分布が必要です".into()),
        }
    }

    /// 準備計算が生成する部材荷重を作る。
    pub fn auto(elem: ElemId, dir: [f64; 3], kind: MemberLoadKind) -> Self {
        Self {
            elem,
            dir,
            kind,
            extent: MemberLoadExtent::FixedDistance,
            name: String::new(),
            source: LoadSource::Auto,
        }
    }
}

/// 荷重ケースの種別。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoadCaseKind {
    /// 固定荷重（自重・仕上げ）
    Dead,
    /// 積載荷重（架構用・長期）
    Live,
    /// 積載荷重（地震用）。地震用重量の集計にはこちらを用いる（令85条）。
    LiveSeismic,
    /// 積雪荷重
    Snow,
    /// 風荷重
    Wind,
    /// 地震荷重（自動生成された水平力など）
    Seismic,
    #[default]
    Other,
}

impl LoadCaseKind {
    /// 長期応力解析の対象となる荷重ケース種別か。
    /// 固定・積載・積雪と種別未指定 `Other` を長期として扱う。
    pub fn is_long_term(&self) -> bool {
        matches!(
            self,
            LoadCaseKind::Dead | LoadCaseKind::Live | LoadCaseKind::Snow | LoadCaseKind::Other
        )
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoadCase {
    pub id: LoadCaseId,
    pub name: String,
    pub nodal: Vec<NodalLoad>,
    /// 部材（梁）荷重。
    #[serde(default)]
    pub member: Vec<MemberLoad>,
    /// 荷重種別。フィールド無しは `Other`。
    #[serde(default)]
    pub kind: LoadCaseKind,
}

impl LoadCase {
    /// 手入力の節点荷重を「`nodal` 上の添字」付きで列挙する。
    /// 編集・削除コマンドは添字で対象を指すため、表示側も添字を持ち回る。
    pub fn manual_nodal(&self) -> impl Iterator<Item = (usize, &NodalLoad)> {
        self.nodal
            .iter()
            .enumerate()
            .filter(|(_, nl)| !nl.source.is_auto())
    }

    /// 手入力の部材荷重を「`member` 上の添字」付きで列挙する。
    pub fn manual_member(&self) -> impl Iterator<Item = (usize, &MemberLoad)> {
        self.member
            .iter()
            .enumerate()
            .filter(|(_, ml)| !ml.source.is_auto())
    }

    /// 通常の自動生成分だけを `auto_nodal` / `auto_member` へ入れ替える。
    /// 手入力の荷重は順序を保ったまま残す。
    ///
    /// 渡された荷重の `source` は [`LoadSource::Auto`] に揃える。手入力扱いのまま
    /// 積むと次回の入れ替えで残ってしまい、同期のたびに荷重が増え続けるため。
    pub fn replace_auto_loads(&mut self, auto_nodal: Vec<NodalLoad>, auto_member: Vec<MemberLoad>) {
        self.nodal
            .retain(|nl| !matches!(nl.source, LoadSource::Auto | LoadSource::WallSelfWeight));
        self.nodal.extend(auto_nodal.into_iter().map(|mut nl| {
            if nl.source != LoadSource::WallSelfWeight {
                nl.source = LoadSource::Auto;
            }
            nl
        }));
        self.member
            .retain(|ml| !matches!(ml.source, LoadSource::Auto | LoadSource::WallSelfWeight));
        self.member.extend(auto_member.into_iter().map(|mut ml| {
            if ml.source != LoadSource::WallSelfWeight {
                ml.source = LoadSource::Auto;
            }
            ml
        }));
    }

    /// 通常の自動生成分が `auto_nodal` / `auto_member` と一致するか（同期の要否判定）。
    /// 手入力分は比較に含めない。
    pub fn auto_loads_match(&self, auto_nodal: &[NodalLoad], auto_member: &[MemberLoad]) -> bool {
        self.nodal
            .iter()
            .filter(|nl| matches!(nl.source, LoadSource::Auto | LoadSource::WallSelfWeight))
            .eq(auto_nodal.iter())
            && self
                .member
                .iter()
                .filter(|ml| matches!(ml.source, LoadSource::Auto | LoadSource::WallSelfWeight))
                .eq(auto_member.iter())
    }

    pub fn replace_tip_loads(&mut self, mut loads: Vec<MemberLoad>) {
        self.member.retain(|ml| ml.source != LoadSource::SlabTip);
        for load in &mut loads {
            load.source = LoadSource::SlabTip;
        }
        self.member.extend(loads);
    }

    pub fn tip_loads_match(&self, loads: &[MemberLoad]) -> bool {
        self.member
            .iter()
            .filter(|ml| ml.source == LoadSource::SlabTip)
            .eq(loads.iter())
    }
}

/// 固定荷重（DL）の標準荷重ケース名。躯体自重（柱・梁・ブレース・壁・ダンパー・二次部材）と
/// スラブの固定荷重（仕上げ等）が解析実行前の同期アクションで自動集計される。
/// 自動集計が入れ替えるのは [`LoadSource::Auto`] の荷重だけなので、このケースへ
/// 手入力で荷重を足しても消えない。
pub const DL_CASE_NAME: &str = "DL";

/// 積載荷重（LL・架構用）の標準荷重ケース名。スラブ用途（令別表第1）の
/// 骨組用積載が自動分配される（長期骨組解析用。令85条1項）。
pub const LL_FRAME_CASE_NAME: &str = "LL(架構用)";

/// 積載荷重（LL・地震用）の標準荷重ケース名。スラブ用途（令別表第1）の
/// 地震用積載が自動分配され、地震用重量の集計に用いる（令85条1項・令88条）。
pub const LL_SEISMIC_CASE_NAME: &str = "LL(地震用)";

/// 地震荷重（X 方向・Ai 分布）の標準荷重ケース名。階の定義があるとき、
/// 解析実行前の同期アクションで水平力（Ai 分布）が自動生成される。
pub const EX_CASE_NAME: &str = "EX";

/// 地震荷重（Y 方向・Ai 分布）の標準荷重ケース名。[`EX_CASE_NAME`] の Y 方向版。
pub const EY_CASE_NAME: &str = "EY";

/// 新規モデルにデフォルトで用意する標準荷重ケース一式
/// （DL・LL(架構用)・LL(地震用)・EX・EY。内容は空で、解析実行前の
/// 同期アクションが自動計算値を書き込む）。ID は 0 起点の連番
/// （`Model::validate` の「id == 添字」規約に従う）。
pub fn default_load_cases() -> Vec<LoadCase> {
    let make = |i: u32, name: &str, kind: LoadCaseKind| LoadCase {
        id: LoadCaseId(i),
        name: name.to_string(),
        nodal: Vec::new(),
        member: Vec::new(),
        kind,
    };
    vec![
        make(0, DL_CASE_NAME, LoadCaseKind::Dead),
        make(1, LL_FRAME_CASE_NAME, LoadCaseKind::Live),
        make(2, LL_SEISMIC_CASE_NAME, LoadCaseKind::LiveSeismic),
        make(3, EX_CASE_NAME, LoadCaseKind::Seismic),
        make(4, EY_CASE_NAME, LoadCaseKind::Seismic),
    ]
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoadCombination {
    pub name: String,
    pub terms: Vec<(LoadCaseId, f64)>,
}

/// 新規モデルにデフォルトで用意する標準荷重組合せ一式
/// （長期 1 + 短期地震 4 の計 5 組合せ）。
///
/// [`default_load_cases`] が生成する標準ケースの並び
/// （0:DL、1:LL(架構用)、2:LL(地震用)、3:EX、4:EY）を前提に、
/// [`crate::load_combo::standard_combinations`] を積雪なし・非多雪区域で呼ぶ。
///
/// - 長期: `DL + LL`
/// - 短期地震: `DL + LL + EX`／`DL + LL - EX`／`DL + LL + EY`／`DL + LL - EY`
///
/// 長期には架構用の積載（令85条1項の長期骨組解析用）を用いる。
/// 生成規則は令82条の一般実装と同一の関数である。
pub fn default_combinations() -> Vec<LoadCombination> {
    crate::load_combo::standard_combinations(&crate::load_combo::ComboInput {
        dl: LoadCaseId(0),
        ll: LoadCaseId(1),
        seismic_x: Some(LoadCaseId(3)),
        seismic_y: Some(LoadCaseId(4)),
        snow: None,
        heavy_snow_zone: false,
        snow_factors: None,
    })
}

/// ダンパーとしてモデル化した範囲の総重量。両端節点へ半分ずつ伝達する。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamperSpec {
    pub elem: ElemId,
    /// 総重量 [N]（有限・非負）。範囲内の支持部・付加重量を含み、別モデルの支持部を除く。
    pub total_weight: f64,
}

/// K 型ブレースの重量配分規則（固定荷重の重量配分規則）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum KBraceWeightRule {
    /// 内部節点（ブレース同士のみが接続する節点）にも重量を配分する（両端 1/2）。
    #[default]
    InternalNodes,
    /// 基準節点（柱梁が接続する節点）にのみ重量を配分する。
    BaseNodesOnly,
}

/// 自重算定の付加設定（固定荷重の鉄骨重量割増率・
/// 仕上げ荷重・耐火被覆・ダンパー自重・K型ブレース配分に対応する簡易版）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoadCfg {
    /// 鉄骨重量割増率 α（デフォルト 1.0）。コンクリート材（`fc` あり）には適用しない。
    /// 0 以下が入力された場合は 1.0 として扱う（本実装の規則）。
    pub steel_weight_factor: f64,
    /// 部材固有の例外的な付加線重量 [N/mm]。
    pub extra_line_weight: Vec<(ElemId, f64)>,
    /// 部材ごとの仕上げ面重量 w_f [N/mm²]。RC/SRC Column は対象外。
    /// その他の部材は鉛直材 2(b+D)、それ以外 b+2D で線重量へ換算する。
    #[serde(default)]
    pub finish_area_weight: Vec<(ElemId, f64)>,
    /// ダンパー総重量。対象部材の断面自重・要素質量・付加重量を置き換える。
    #[serde(default)]
    pub dampers: Vec<DamperSpec>,
    /// K 型ブレース（`ElementKind::Brace`）の重量配分規則。
    #[serde(default)]
    pub k_brace_rule: KBraceWeightRule,
    /// 支える床の数に応じた柱軸力算定時の積載荷重低減（令85条2項）を考慮するか。
    /// デフォルトは「低減を考慮しない」。
    #[serde(default)]
    pub live_load_reduction: bool,
}

impl Default for LoadCfg {
    fn default() -> Self {
        Self {
            steel_weight_factor: 1.0,
            extra_line_weight: Vec::new(),
            finish_area_weight: Vec::new(),
            dampers: Vec::new(),
            k_brace_rule: KBraceWeightRule::default(),
            live_load_reduction: false,
        }
    }
}

impl LoadCfg {
    /// ダンパー総重量の値・参照・明示 Damper の指定漏れを検証する。重量は生成しない。
    pub fn validate_damper_weights(&self, model: &Model) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for damper in &self.dampers {
            if !ids.insert(damper.elem) {
                return Err(format!(
                    "部材 {} のダンパー総重量が重複しています",
                    damper.elem.0
                ));
            }
            if !damper.total_weight.is_finite() || damper.total_weight < 0.0 {
                return Err(format!("部材 {} のダンパー総重量が不正です", damper.elem.0));
            }
            let elem = model.element(damper.elem).ok_or_else(|| {
                format!("ダンパー総重量の参照部材 {} がありません", damper.elem.0)
            })?;
            if !(elem.kind.is_weight_frame()
                || matches!(elem.kind, ElementKind::Brace { .. } | ElementKind::Damper))
                || elem.nodes.len() != 2
                || elem.nodes.iter().any(|id| {
                    model
                        .node(*id)
                        .is_none_or(|node| node.coord.iter().any(|v| !v.is_finite()))
                })
            {
                return Err(format!(
                    "部材 {} のダンパー総重量の対象・節点参照が不正です",
                    elem.id.0
                ));
            }
        }
        for elem in &model.elements {
            if elem.kind == ElementKind::Damper && !ids.contains(&elem.id) {
                return Err(format!("ダンパー部材 {} の総重量が未設定です", elem.id.0));
            }
        }
        Ok(())
    }

    /// 有効な鉄骨重量割増率（0 以下の入力は 1.0 とみなす）。
    pub fn effective_steel_factor(&self) -> f64 {
        if self.steel_weight_factor > 0.0 {
            self.steel_weight_factor
        } else {
            1.0
        }
    }
}

impl Model {
    /// 全長追従の属性・諸元を検証する。不整合は荷重ケース・部材荷重添字を付けて返す。
    pub fn validate_member_load_extents(&self) -> Result<(), String> {
        for case in &self.load_cases {
            for (index, load) in case.member.iter().enumerate() {
                let length_mm = self
                    .element(load.elem)
                    .map_or(0.0, |e| self.member_length(e));
                load.validate_extent(length_mm).map_err(|reason| {
                    format!(
                        "荷重ケース {} ({}) member[{}] 部材 {}: {}",
                        case.id.0, case.name, index, load.elem.0, reason
                    )
                })?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod extent_tests {
    use super::*;

    #[test]
    fn missing_extent_keeps_fixed_distance_even_for_equal_intensity_full_span() {
        let load: MemberLoad = serde_json::from_value(serde_json::json!({
            "elem": 0, "dir": [0, 0, -1], "name": "", "source": "Manual",
            "kind": {"Distributed": {"a": 0, "b": 6000, "w1": 10, "w2": 10}}
        }))
        .unwrap();
        assert_eq!(load.extent, MemberLoadExtent::FixedDistance);
    }

    #[test]
    fn full_length_intent_rejects_inconsistent_distribution_and_generated_source() {
        let mut load = MemberLoad::full_length_uniform(ElemId(0), [0.0, 0.0, -1.0], 6000.0, 10.0);
        assert!(load.validate_extent(6000.0).is_ok());
        assert!(load.validate_extent(8000.0).is_err());
        assert!(load.validate_extent(0.0).is_err());
        load.source = LoadSource::Auto;
        assert!(load.validate_extent(6000.0).is_err());
        load.source = LoadSource::Manual;
        load.kind = MemberLoadKind::Distributed {
            a: 0.0,
            b: 6000.0,
            w1: 10.0,
            w2: 11.0,
        };
        assert!(load.validate_extent(6000.0).is_err());
        load.kind = MemberLoadKind::Point { a: 1000.0, p: 10.0 };
        assert!(load.validate_extent(6000.0).is_err());
    }
}
