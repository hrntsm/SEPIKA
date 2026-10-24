//! 階は FL と ΔSL を入力として保持し、SL と隣接階の高さを導出する。

use super::*;

/// 剛床のレベル許容差 `[mm]`。剛床のスレーブ節点は「階の SL からこの範囲内に
/// ある節点」とする（[`Model::on_diaphragm_level`]）。
///
/// 階への帰属（[`crate::model::Model::story_at`]）が**区間**であるのに対し、剛床への
/// 帰属は**床面**である。中間高さの節点（柱の分割点・階高の途中に取り付く梁）は
/// 階には属するが剛床には入らない。面内剛体として拘束してよいのは同一床面の
/// 節点だけであり、中間節点を含めると存在しない水平剛性が生じるためである。
pub const DIAPHRAGM_LEVEL_TOL_MM: f64 = 1.0;

/// 下から 0 始まりの `index` に対して 1F・2F… の既定階名を返す。
/// 最上階も自動で RF とはしない。
pub fn default_story_name(index: usize) -> String {
    format!("{}F", index + 1)
}

/// 階に属する剛床の参照ビュー（[`Constraint::RigidDiaphragm`] の内容）。
///
/// 剛床の実体は拘束として保持されるため、階から剛床を辿る側は本ビューを介する
/// （[`Model::diaphragms_of`]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiaphragmRef<'a> {
    pub story: StoryId,
    pub master: NodeId,
    pub slaves: &'a [NodeId],
    /// この剛床が負担する地震用重量 `[N]`。多剛床の階では層の水平力 Pi を
    /// 剛床ごとの重量比で分配するために用いる（多剛床の設計用せん断力。
    /// 令88条・昭55建告1793号）。None は未算定（階に単一剛床なら層重量全量）。
    pub weight: Option<f64>,
    /// 副剛床の層せん断力係数 Ci の直接入力（令88条・昭55建告1793号の
    /// 層せん断力係数）。Some の剛床は主系統の Ai 分布から
    /// 除外され、水平力 = ci_override × 剛床重量（等価震度扱い。上階に同一系統の
    /// 剛床が積み上がらない副剛床を想定）として作用する。None は主系統（Ai 分布）。
    pub ci_override: Option<f64>,
}

/// 動的解析（固有値・時刻歴・精算周期）の質量モデルの方式。
///
/// 階の自動生成が剛床マスター節点へ与える質点質量（[`super::Node::mass`]）の
/// 算定方法と、解析側の全体質量行列の組立方法（部材密度による分布質量を
/// 含めるか）の両方を規定する。生成と組立で方式が食い違うと自重の二重計上や
/// 質量欠落が起きるため、モデル自身（[`super::Model::mass_method`]）が
/// 単一情報源として保持し、双方がこれを参照する。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MassMethod {
    /// 補正質点方式（既定）: 部材密度による分布質量に加え、剛床マスターへ
    /// 「物理質量相当重量のうち分布質量として計上されない分」（床・仕上げ・積載・
    /// 二次部材・雑壁など）を補正質点として与える。部材の分布質量による鉛直・局部の
    /// 振動モードも保たれる。公称並進総動的質量は [`MassMethod::LumpedOnly`] と
    /// 一致する（地震用重量とは一致しない。設計重量は鋼材 78.5 kN/m³、動的質量は
    /// 物理密度 7.85 t/m³ で別の値のため）。
    #[default]
    CorrectedLumped,
    /// 質点のみ方式: 質量は節点質量（剛床マスターへ与えた物理質量相当重量の質点等）
    /// のみを用い、部材密度による分布質量は質量行列に算入しない
    /// （実務の水平質点系モデル化。鉛直方向・局部の振動モードは表現されない）。
    /// 公称並進総動的質量は [`MassMethod::CorrectedLumped`] と一致する。
    LumpedOnly,
}

/// 階の主要構造種別。設計用一次固有周期の略算式 T=h(0.02+0.01α) の
/// α（柱梁の大部分が鉄骨造である階の高さ比）の算定に用いる（令88条・告示1793号）。
///
/// 値は階に属する柱・梁の構造種別から自動判定する（準備計算の階生成。
/// [`StoryStructure::of_structure_kind`]）ため、利用者は入力しない。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StoryStructure {
    #[default]
    Rc,
    S,
    Src,
}

impl StoryStructure {
    /// 部材の構造種別を階の構造種別へ畳み込む。
    ///
    /// CFT は SRC へ寄せる。略算周期 T = h(0.02 + 0.01α) は S の階が増えるほど
    /// T が長く Rt が小さくなって地震力が下がるため、鋼管にコンクリートを充填した
    /// CFT を S へ寄せないのが安全側になる。
    pub fn of_structure_kind(kind: crate::structure_kind::StructureKind) -> Self {
        use crate::structure_kind::StructureKind;
        match kind {
            StructureKind::Rc => StoryStructure::Rc,
            StructureKind::S => StoryStructure::S,
            StructureKind::Src | StructureKind::Cft => StoryStructure::Src,
        }
    }

    /// 種別ごとの部材本数から階の主要構造種別を決める。最多の種別を採用し、
    /// 同数の場合は RC → SRC → S の順で優先する。
    ///
    /// 略算周期 T = h(0.02 + 0.01α) は S の階が増えるほど T が長く、Rt が
    /// 小さくなって地震力が下がるため、判定が割れた場合は S を採らないのが
    /// 安全側になる（同数時の優先順の根拠）。対象部材が 1 本もない階は RC。
    pub fn majority(n_rc: usize, n_s: usize, n_src: usize) -> Self {
        let max = n_rc.max(n_s).max(n_src);
        if max == 0 || n_rc == max {
            StoryStructure::Rc
        } else if n_src == max {
            StoryStructure::Src
        } else {
            StoryStructure::S
        }
    }
}

/// 層の種別。地震層せん断力の算定方法を切り替える
/// （一般階=Ai分布、PH階=震度 k、地下階=水平震度 K）。
///
/// 層の属性だが、保持先は層の**上端床**の [`Story`] である（重量と同じ場所に
/// 集約する。[`Layer`] 参照）。最下の階（基部の床）はどの層の上端でもないため、
/// そこに設定された値は用いられない。
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StoryLevelKind {
    #[default]
    Normal,
    /// 塔屋（PH）階。層せん断力 Qi = k·ΣWj（k は 0.5〜1.0 の指定震度）。
    Penthouse { k: f64 },
    /// 地下階。Qi = Q(i+1) + K·Wi、K = 0.1·(1 − H/40)·Z
    /// （H は地盤面からの深さ depth_mm `[mm]` を m 換算した値、20m 超は 20m）。
    Basement { depth_mm: f64 },
}

/// 層の動的質量（物理質量相当）。階生成が節点ごとの物理質量相当重量から一括算定する。
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryDynamicMass {
    /// 物理質量相当重量 `[N]`（質量方式に依らない全量。Σ node_mass_equiv）。
    pub mass_equiv_weight_n: f64,
    /// 質量重心 (x, y) `[mm]`。
    pub center_xy_mm: [f64; 2],
    /// 質量重心まわりの回転慣性 [t·mm²]。
    pub inertia_t_mm2: f64,
    /// 階生成で算定した代表節点質量の反映記録。未生成・代表節点なしは `None`。
    #[serde(default)]
    pub lumped_mass: Option<StoryLumpedMass>,
}

/// 階生成が算定した代表節点と質量。組立時に反映状態を照合するための生成データ。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryLumpedMass {
    pub master: NodeId,
    pub mass_method: MassMethod,
    /// 節点質量6成分 [t, t, t, t·mm², t·mm², t·mm²]。無質量は `None`。
    pub mass: Option<[f64; 6]>,
    /// この階へ配分したダンパー入力総重量 `[N]`（重力ケースのみ方式でも入力を記録）。
    pub damper_weight_n: f64,
}

/// 階の外周スラブ算定に使う標準床荷重。値の単位は内部単位 [N/mm²]。
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StandardFloorLoad {
    pub dead: f64,
    pub floor: f64,
    pub beam: f64,
    pub frame: f64,
    pub seismic: f64,
}

impl StandardFloorLoad {
    pub fn intensity(self, purpose: Option<LoadPurpose>) -> f64 {
        match purpose {
            None => self.dead,
            Some(LoadPurpose::Floor) => self.floor,
            Some(LoadPurpose::Beam) => self.beam,
            Some(LoadPurpose::Frame) => self.frame,
            Some(LoadPurpose::Seismic) => self.seismic,
        }
    }
}

/// 耐火被覆種別。既定は被覆なし。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FireproofKind {
    #[default]
    None,
    Spray,
    Board,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StoryFireproof {
    pub steel_kind: FireproofKind,
    /// S柱の面重量 [N/mm²]。既定は0。
    pub steel_column_area_weight: f64,
    /// S梁の面重量 [N/mm²]。既定は0。
    pub steel_beam_area_weight: f64,
    pub cft_kind: FireproofKind,
    /// CFT柱の面重量 [N/mm²]。既定は0。
    pub cft_column_area_weight: f64,
}

impl StoryFireproof {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.steel_column_area_weight,
            self.steel_beam_area_weight,
            self.cft_column_area_weight,
        ]
        .iter()
        .all(|q| q.is_finite() && *q >= 0.0)
        {
            Ok(())
        } else {
            Err("耐火被覆面重量は有限な非負値が必要です".into())
        }
    }
}

/// 階の入力と準備計算で得られる所属・重量。FL・ΔSL の単位は mm。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Story {
    pub id: StoryId,
    /// 階名（`2F`・`PH1` など）。表示用の自由文字列。識別子は `id`。
    pub name: String,
    /// FL（床上端の基準標高）[mm]。`None` は未入力。
    pub elevation: Option<f64>,
    /// SL − FL [mm]。上向きを正とし、`None` は未入力。
    pub sl_offset_mm: Option<f64>,
    /// 取込元の原階 ID。現在の FL・SL の編集では元属性を変更しない。
    pub source_story: Option<u32>,
    /// この階に属する節点（準備計算が [`crate::model::Model::story_at`] で埋める）。
    pub node_ids: Vec<NodeId>,
    /// 設計に用いる地震用重量 `[N]`。準備計算の階生成が自動算定値を書き込むが、
    /// [`Self::weight_override`] が `Some` の場合はその手入力値が入る
    /// （解析・設計側はこのフィールドだけを読めばよい）。
    pub seismic_weight: Option<f64>,
    /// 地震用重量の手入力値 `[N]`。`Some` のときは準備計算で階を再生成しても
    /// 保持され、[`Self::seismic_weight`] へ優先して反映される。`None` は
    /// 自動算定値をそのまま用いる。フィールド無しは手入力なし扱い。
    #[serde(default)]
    pub weight_override: Option<f64>,
    /// 主要構造種別（略算周期の鉄骨造比 α 算定用）。断面形状からの自動判定値。
    /// フィールド無しは RC 扱い。
    #[serde(default)]
    pub structure: StoryStructure,
    /// 階の種別（一般/PH/地下）。フィールド無しは一般階扱い。
    #[serde(default)]
    pub level_kind: StoryLevelKind,
    /// 動的解析（質点系解析など）用の物理質量相当。準備計算の階生成が埋める。
    /// 未算定は `None`。フィールド無しは `None`。
    #[serde(default)]
    pub dynamic_mass: Option<StoryDynamicMass>,
    /// 外周スラブに用いる標準床荷重（未設定なら外周スラブを生成しない）。
    #[serde(default)]
    pub standard_floor_load: Option<StandardFloorLoad>,
    /// RC/SRC 柱の階共通仕上げ面重量 [N/mm²]。未設定は 0。
    #[serde(default)]
    pub column_finish_area_weight: f64,
    /// 階共通の耐火被覆条件。既定はS・CFTとも被覆なし。
    #[serde(default)]
    pub fireproof: StoryFireproof,
    /// 物理壁版IDごとの独立階帯集計。空は壁なしまたは未生成。
    #[serde(default)]
    pub wall_weights: Vec<WallStoryWeight>,
}

/// 層（隣り合う 2 つの階の間）。法規上の「i 階」はこれを指す。
///
/// [`Model::layers`] が [`Model::stories`] から組み立てる**導出値**であり、
/// モデルには保持しない（保持すると [`Story::node_ids`] と同種の同期ずれを
/// 1 つ増やすことになる）。層を数える処理は必ずこれを介し、
/// [`Model::stories`] を層として直接走査してはならない。
///
/// 層と床の対応は実務の慣行に従う。
///
/// | 量 | 由来 |
/// |---|---|
/// | 名前 | 下端床（法令の「i 階」は下の床の呼び名） |
/// | 階高 | 上端床の標高 − 下端床の標高 |
/// | 重量・所属節点・階種別 | **上端床** |
///
/// 重量が上端床なのは、層の質量が上端の床に集中するためである。
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// 下から 0 始まりの層の番号。層を識別する添字。
    pub index: usize,
    /// 層の名前（下端床の階名）。
    pub name: String,
    /// 下端の階（床）。
    pub bottom: StoryId,
    /// 上端の階（床）。層の重量・所属節点・階種別はこの階が持つ。
    pub top: StoryId,
    /// 層の高さ（階高）`[mm]`。
    pub height: f64,
    /// 上下階の SL 間高さ [mm]。
    pub structural_height_mm: f64,
    /// 下端階 SL [mm]。
    pub bottom_sl_mm: f64,
    /// 上端階 SL [mm]。
    pub top_sl_mm: f64,
    /// 下端階の FL `[mm]`。
    pub bottom_elevation: f64,
    /// 上端階の FL `[mm]`。
    pub top_elevation: f64,
    /// 層の種別（一般/PH/地下）。
    pub level_kind: StoryLevelKind,
    /// 設計に用いる地震用重量 `[N]`（未算定なら `None`）。
    pub weight: Option<f64>,
    /// 層に属する節点（＝上端床の所属節点）。
    pub node_ids: Vec<NodeId>,
    /// 主要構造種別（略算周期の鉄骨造比 α 算定用）。
    pub structure: StoryStructure,
    /// 動的解析用の物理質量相当（上端床が持つ値）。未算定は `None`。
    pub dynamic_mass: Option<StoryDynamicMass>,
}

impl Story {
    /// 階 ID・名称・FL・ΔSL [mm] を保持する未算定の階を作る。
    pub fn new(
        id: StoryId,
        name: String,
        elevation: Option<f64>,
        sl_offset_mm: Option<f64>,
    ) -> Self {
        Self {
            id,
            name,
            elevation,
            sl_offset_mm,
            source_story: None,
            node_ids: Vec::new(),
            seismic_weight: None,
            weight_override: None,
            structure: Default::default(),
            level_kind: Default::default(),
            dynamic_mass: None,
            standard_floor_load: None,
            column_finish_area_weight: 0.0,
            fireproof: Default::default(),
            wall_weights: Vec::new(),
        }
    }

    /// FL [mm] を返す。未入力・非有限値は `None`。
    pub fn fl_mm(&self) -> Option<f64> {
        self.elevation.filter(|v| v.is_finite())
    }

    /// SL [mm] を返す。FL・ΔSL の不足または非有限な導出値は `None`。
    pub fn sl_mm(&self) -> Option<f64> {
        let sl = self.fl_mm()? + self.sl_offset_mm.filter(|v| v.is_finite())?;
        sl.is_finite().then_some(sl)
    }
}

impl Model {
    /// 階 ID・FL・ΔSL・SL と上下階差の不足・不正を階別に返す。
    pub fn story_level_diagnostics(&self) -> Vec<String> {
        self.story_level_errors(true)
    }

    /// 未入力を許容し、入力済み FL・ΔSL・SL・階順と参照の不正を返す。
    pub fn story_level_input_diagnostics(&self) -> Vec<String> {
        self.story_level_errors(false)
    }

    fn story_level_errors(&self, require_complete: bool) -> Vec<String> {
        let mut errors = Vec::new();
        if require_complete && self.stories.len() < 2 {
            errors.push("階情報: 最下層を構成する上下階の FL・ΔSL を入力してください".into());
        }
        for (i, story) in self.stories.iter().enumerate() {
            let owner = format!("階 {} ({})", story.id.0, story.name);
            if story.id.index() != i {
                errors.push(format!("{owner}: 階IDと配列位置が不整合"));
            }
            if (require_complete || story.elevation.is_some()) && story.fl_mm().is_none() {
                errors.push(format!("{owner}: FL が未設定または有限値ではありません"));
            }
            if (require_complete || story.sl_offset_mm.is_some())
                && story.sl_offset_mm.is_none_or(|v| !v.is_finite())
            {
                errors.push(format!("{owner}: ΔSL が未設定または有限値ではありません"));
            }
            if story.fl_mm().is_some()
                && story.sl_offset_mm.is_some_and(f64::is_finite)
                && story.sl_mm().is_none()
            {
                errors.push(format!("{owner}: SL の導出結果が有限値ではありません"));
            }
            if let Some(id) = story.source_story {
                if !self.source_stories.iter().any(|s| s.id == id) {
                    errors.push(format!("{owner}: 原階 {id} が存在しません"));
                }
            }
            if i > 0 {
                let lower = &self.stories[i - 1];
                for (field, bottom, top) in [
                    ("FL", lower.fl_mm(), story.fl_mm()),
                    ("SL", lower.sl_mm(), story.sl_mm()),
                ] {
                    if let (Some(bottom), Some(top)) = (bottom, top) {
                        let height = top - bottom;
                        if !height.is_finite() || height <= 0.0 {
                            errors.push(format!(
                                "{owner}: 直下階 {} との {field} 間高さは有限な正値が必要です",
                                lower.id.0
                            ));
                        }
                    }
                }
            }
        }
        errors
    }

    /// 階依存の生成に必要な入力・SL 所属区間・利用者剛床を検査する。
    /// 重量・材料・荷重の完成度とは独立し、既存の座標・拘束は変更しない。
    pub fn validate_story_structure_inputs(&self) -> Result<(), String> {
        let errors: Vec<_> = self
            .story_level_diagnostics()
            .into_iter()
            .chain(self.source_story_diagnostics())
            .collect();
        if !errors.is_empty() {
            return Err(errors.join("; "));
        }
        if self.base_elevation() > self.stories[0].sl_mm().unwrap() {
            return Err(format!(
                "階 {}: SL が構造最下端より低く、所属区間が成立しません",
                self.stories[0].id.0
            ));
        }
        let spans = self.story_spans();
        let structural = crate::dof::structural_nodes(self);
        let owner = |node: &Node| {
            self.explicit_story_of_node(node.id)
                .or_else(|| self.story_at(&spans, node.coord[2]))
        };
        for node in self
            .nodes
            .iter()
            .filter(|node| !self.generated_masters.contains(&node.id))
        {
            match owner(node) {
                Some(story)
                    if self
                        .stories
                        .get(story.index())
                        .is_none_or(|s| s.id != story) =>
                {
                    return Err(format!("節点 {}: 階 {} が存在しません", node.id.0, story.0))
                }
                None if structural.get(node.id.index()).copied().unwrap_or(false) => {
                    return Err(format!(
                        "節点 {}: SL 区間内の所属階を解決できません",
                        node.id.0
                    ))
                }
                _ => {}
            }
        }
        for constraint in self
            .constraints
            .iter()
            .filter(|c| !self.is_automatic_seismic_diaphragm(c))
        {
            if let Constraint::RigidDiaphragm {
                story,
                master,
                slaves,
                ..
            } = constraint
            {
                let Some(level) = self.stories.get(story.index()).filter(|s| s.id == *story) else {
                    return Err(format!("剛床: 階 {} が存在しません", story.0));
                };
                if self.node(*master).is_none() {
                    return Err(format!(
                        "階 {} の剛床マスター {} が存在しません",
                        story.0, master.0
                    ));
                }
                if slaves.is_empty()
                    || slaves.iter().any(|id| {
                        self.node(*id).is_none_or(|node| {
                            owner(node) != Some(*story)
                                || !self.on_diaphragm_level(*story, node.coord[2])
                        })
                    })
                {
                    return Err(format!(
                        "階 {} ({}) の利用者剛床に SL 面上の所属節点が不足しています",
                        story.0, level.name
                    ));
                }
            }
        }
        Ok(())
    }

    /// 保存された明示所属を返す。自動生成時の所属値は除外する。
    pub fn explicit_story_of_node(&self, node: NodeId) -> Option<StoryId> {
        if let Some(story) = self.stories.iter().find(|story| {
            story.source_story.is_some_and(|source| {
                self.source_stories
                    .iter()
                    .any(|s| s.id == source && s.node_ids.iter().any(|n| n.node == Some(node)))
            })
        }) {
            return Some(story.id);
        }
        let story = self
            .node(node)?
            .story
            .filter(|id| {
                self.stories
                    .get(id.index())
                    .is_none_or(|story| story.source_story.is_none())
            })
            .or_else(|| {
                self.stories
                    .iter()
                    .find(|s| s.source_story.is_none() && s.node_ids.contains(&node))
                    .map(|s| s.id)
            })?;
        if self
            .seismic_weight_generation
            .as_ref()
            .is_some_and(|record| record.automatic_node_stories.contains(&(node, story)))
        {
            return None;
        }
        Some(story)
    }

    pub fn is_rc_src_column(&self, elem: &ElementData) -> bool {
        elem.kind.is_weight_frame()
            && self.element_section(elem).is_some_and(|sec| {
                sec.frame_use == Some(FrameSectionUse::Column)
                    && matches!(
                        crate::structure_kind::member_structure_kind(self, elem),
                        crate::structure_kind::StructureKind::Rc
                            | crate::structure_kind::StructureKind::Src
                    )
            })
    }

    /// 入力済みの隣接階対から層を返す。階情報が不成立なら空。
    pub fn layers(&self) -> Vec<Layer> {
        if !self.story_level_diagnostics().is_empty() {
            return Vec::new();
        }
        self.stories
            .windows(2)
            .enumerate()
            .map(|(i, w)| {
                let (bottom, top) = (&w[0], &w[1]);
                Layer {
                    index: i,
                    name: bottom.name.clone(),
                    bottom: bottom.id,
                    top: top.id,
                    height: top.fl_mm().unwrap() - bottom.fl_mm().unwrap(),
                    structural_height_mm: top.sl_mm().unwrap() - bottom.sl_mm().unwrap(),
                    bottom_elevation: bottom.fl_mm().unwrap(),
                    bottom_sl_mm: bottom.sl_mm().unwrap(),
                    top_elevation: top.fl_mm().unwrap(),
                    top_sl_mm: top.sl_mm().unwrap(),
                    level_kind: top.level_kind,
                    weight: top.seismic_weight,
                    node_ids: top.node_ids.clone(),
                    structure: top.structure,
                    dynamic_mass: top.dynamic_mass,
                }
            })
            .collect()
    }

    /// 層の数（`stories.len() - 1`、階が 1 つ以下なら 0）。
    ///
    /// [`Self::layers`] を組み立てずに個数だけ要るときに使う。
    pub fn layer_count(&self) -> usize {
        self.stories.len().saturating_sub(1)
    }

    /// 構造節点（生成代表節点を除く）の最小 Z [mm]。節点なしは 0。
    pub fn base_elevation(&self) -> f64 {
        let excluded: std::collections::HashSet<NodeId> =
            self.generated_masters.iter().copied().collect();
        let base = self
            .nodes
            .iter()
            .filter(|n| !excluded.contains(&n.id))
            .map(|n| n.coord[2])
            .fold(f64::INFINITY, f64::min);
        if base.is_finite() {
            base
        } else {
            0.0
        }
    }

    /// SL を境界とする所属区間 (下端, 上端] [mm]。最下階のみ構造最下端を含む。
    /// 階情報の不足・逆転時は空。
    pub fn story_spans(&self) -> Vec<(f64, f64)> {
        if !self.story_level_diagnostics().is_empty() {
            return Vec::new();
        }
        self.stories
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let bottom = if i == 0 {
                    self.base_elevation()
                } else {
                    self.stories[i - 1].sl_mm().unwrap()
                };
                (bottom, s.sl_mm().unwrap())
            })
            .collect()
    }

    /// レベル `z` `[mm]` が属する階を、[`Self::story_spans`] の区間列から引く。
    ///
    /// 階への帰属は**区間**である。中間高さの節点や段差床の節点も、区間に入れば
    /// 当該階に属する。剛床への帰属とは規則が異なる（[`Self::on_diaphragm_level`]）。
    /// どの区間にも入らない場合は `None`（基部レベル未満、または最上階より上）。
    ///
    /// 区間列は標高の昇順で連続しているため二分探索で引く（伏図の描画が毎フレーム
    /// 全節点に対して呼ぶため、線形探索では階数に比例して重くなる）。
    pub fn story_at(&self, spans: &[(f64, f64)], z: f64) -> Option<StoryId> {
        let i = spans.partition_point(|&(_, top)| top < z);
        let &(bottom, top) = spans.get(i)?;
        let above_bottom = if i == 0 { z >= bottom } else { z > bottom };
        if !above_bottom || z > top {
            return None;
        }
        self.stories.get(i).map(|s| s.id)
    }

    /// 各節点の所属階（[`Self::nodes`] と同順・同長）。
    ///
    /// 階への帰属規則（区間）の単一情報源。準備計算の階生成も UI の表示も
    /// これを用いる。
    pub fn node_stories(&self) -> Vec<Option<StoryId>> {
        let spans = self.story_spans();
        self.nodes
            .iter()
            .map(|n| {
                self.explicit_story_of_node(n.id)
                    .or_else(|| self.story_at(&spans, n.coord[2]))
            })
            .collect()
    }

    /// レベル `z` `[mm]` が階 `story` の床面上にあるか（剛床のスレーブ判定）。
    ///
    /// 判定は階のレベルからの差が [`DIAPHRAGM_LEVEL_TOL_MM`] 以内かどうかで、
    /// 階への帰属（区間）とは規則が異なる。
    pub fn on_diaphragm_level(&self, story: StoryId, z: f64) -> bool {
        self.stories
            .get(story.index())
            .and_then(Story::sl_mm)
            .is_some_and(|sl| (z - sl).abs() <= DIAPHRAGM_LEVEL_TOL_MM)
    }

    /// 階 `story` に属する剛床（[`Constraint::RigidDiaphragm`]）を定義順に返す。
    ///
    /// 剛床は階の一部ではなく拘束として保持されるため、「この階の剛床」が要る
    /// ところは常にこのヘルパーを情報源とする。
    pub fn diaphragms_of(&self, story: StoryId) -> impl Iterator<Item = DiaphragmRef<'_>> {
        self.constraints.iter().filter_map(move |c| match c {
            Constraint::RigidDiaphragm {
                story: s,
                master,
                slaves,
                weight,
                ci_override,
            } if *s == story => Some(DiaphragmRef {
                story: *s,
                master: *master,
                slaves,
                weight: *weight,
                ci_override: *ci_override,
            }),
            _ => None,
        })
    }

    /// 節点 `id` がいずれかの剛床のマスターまたはスレーブか。
    pub fn node_on_rigid_diaphragm(&self, id: NodeId) -> bool {
        self.constraints.iter().any(|c| match c {
            Constraint::RigidDiaphragm { master, slaves, .. } => {
                *master == id || slaves.contains(&id)
            }
            _ => false,
        })
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;
    use crate::frame_gen::{frame_model, FrameSpec};

    fn model() -> Model {
        frame_model(&FrameSpec {
            base_elevation: 3500.0,
            story_heights: vec![3500.0],
            sl_offsets_mm: vec![Some(-500.0), Some(-600.0)],
            with_slabs: false,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn fl_and_sl_have_distinct_heights_and_structure_coordinates() {
        let mut m = model();
        assert_eq!(m.stories[0].sl_mm(), Some(3000.0));
        assert_eq!(m.stories[1].sl_mm(), Some(6400.0));
        let layer = &m.layers()[0];
        assert_eq!((layer.bottom, layer.top), (StoryId(0), StoryId(1)));
        assert_eq!((layer.height, layer.structural_height_mm), (3500.0, 3400.0));
        assert_eq!(
            (layer.bottom_elevation, layer.top_elevation),
            (3500.0, 7000.0)
        );
        assert_eq!((layer.bottom_sl_mm, layer.top_sl_mm), (3000.0, 6400.0));
        assert_eq!(m.base_elevation(), 3000.0);
        assert!(m
            .nodes
            .iter()
            .all(|n| [3000.0, 6400.0].contains(&n.coord[2])));
        assert!(m
            .nodes
            .iter()
            .filter(|n| n.coord[2] == 3000.0)
            .all(|n| n.restraint != crate::dof::Dof6Mask::FREE));
        let coordinates: Vec<_> = m.nodes.iter().map(|n| n.coord).collect();
        m.stories[1].sl_offset_mm = Some(200.0);
        assert_eq!(m.layers()[0].height, 3500.0);
        assert_eq!(m.layers()[0].structural_height_mm, 4200.0);
        m.stories[1].elevation = Some(7500.0);
        assert_eq!(m.layers()[0].height, 4000.0);
        assert_eq!(
            m.nodes.iter().map(|n| n.coord).collect::<Vec<_>>(),
            coordinates
        );
    }

    #[test]
    fn signed_offsets_and_sl_membership_use_the_same_floor_id() {
        let mut m = model();
        for offset in [-500.0, 0.0, 500.0] {
            m.stories[1].sl_offset_mm = Some(offset);
            let sl = 7000.0 + offset;
            assert_eq!(m.stories[1].sl_mm(), Some(sl));
            assert_eq!(m.story_at(&m.story_spans(), sl), Some(StoryId(1)));
            assert!(m.on_diaphragm_level(StoryId(1), sl));
            assert!(!m.on_diaphragm_level(StoryId(1), sl - 100.0));
        }
    }

    #[test]
    fn incomplete_nonfinite_overflow_or_reversed_levels_never_supply_layers() {
        let base = model();
        let invalid: [fn(&mut Model); 11] = [
            |m| m.stories[0].elevation = None,
            |m| m.stories[1].sl_offset_mm = None,
            |m| m.stories[1].elevation = Some(f64::NAN),
            |m| m.stories[1].sl_offset_mm = Some(f64::INFINITY),
            |m| m.stories[1].elevation = Some(3500.0),
            |m| m.stories[1].elevation = Some(1000.0),
            |m| m.stories[1].sl_offset_mm = Some(-5000.0),
            |m| {
                m.stories[1].elevation = Some(f64::MAX);
                m.stories[1].sl_offset_mm = Some(f64::MAX);
            },
            |m| {
                m.stories[0].elevation = Some(-f64::MAX);
                m.stories[0].sl_offset_mm = Some(0.0);
                m.stories[1].elevation = Some(f64::MAX);
                m.stories[1].sl_offset_mm = Some(0.0);
            },
            |m| m.stories[1].id = StoryId(8),
            |m| m.stories[1].source_story = Some(999),
        ];
        for corrupt in invalid {
            let mut m = base.clone();
            corrupt(&mut m);
            assert!(!m.story_level_diagnostics().is_empty());
            assert!(m.layers().is_empty());
            assert!(m.story_spans().is_empty());
        }
    }

    #[test]
    fn frame_input_never_fills_missing_offsets() {
        let mut spec = FrameSpec::default();
        spec.sl_offsets_mm[1] = None;
        assert!(frame_model(&spec).unwrap_err().contains("階 1"));
        spec.sl_offsets_mm.clear();
        assert!(frame_model(&spec).unwrap_err().contains("ΔSL"));
    }
}
