# 鉄骨造の柱脚・接合・構造細則：2025年版限定照合（#878）

## 対象と判定

2026-10-27、基点 `de662dc6` の現行機能を対象に調査した。状態は🔶。
柱脚・接合詳細の専用検定の有無、ブレース軸力検定の保証範囲、防火被覆の重量算定と防火性能の区別を重点とする。
全節・全式の照合、設計モデルの追加、計算実装の修正は行わない。
「原画像確認」はその頁の条件を読んだ意味であり、式の実装一致・法令適合・専門家サインオフを意味しない。

## 原典と版・公開更新

[刊行元の書誌](https://www2.icba.or.jp/products/detail.php?product_id=606)と指定原本の奥付原画像で、
『2025年版 建築物の構造関係技術基準解説書』第2版第1刷、2026年2月11日を確認した。
[ICBAの関連情報](https://www.icba.or.jp/kenchikuhorei/)から次を各閲覧した。

| 公開資料 | 今回の適用確認 |
| --- | --- |
| [正誤表 R8.1.22](https://www.icba.or.jp/zzfilebox/kenshuka/2025seigo.pdf) | 第1版第1刷用。p658–659の筋かいガセット溶接例、p685/687の柱・ベースプレート寸法の訂正を確認。今回の重点頁には直接の訂正なし。これらの例題頁は未照合で、旧版数値を採用しない |
| [Q&A R8.3.27](https://www.icba.or.jp/zzfilebox/kenshuka/2025qa.pdf) No.2（PDF3頁） | 令67条1項の保有水平耐力計算での適用除外と、普通ボルトずれのモデル化への注意を確認。ルート3を選ぶことだけで接合部ずれの影響を確認した扱いにしない。層間変形角の補足計算という取扱いも、現行機能の自動対応とはしない |
| [刊行後の技術基準改正等一覧 2026.7.1](https://www.icba.or.jp/gijutukijunkaiseilist2025.html) | 表の全行を閲覧。今回の柱脚・接合詳細・防火被覆の重点頁を直接変更する掲載項目はない。併用構造のルート等は別範囲。全現行法令の網羅照合とはしない |

原画像はPyMuPDF 1.27.1で各PDF頁を1.3倍のPNGに描画し、全頁を目視した。
印刷頁とPDF頁の対応は「3.06 鉄骨造」が印刷174頁＝PDF1頁、付録1-02が印刷639頁＝PDF1頁。
確認画像は印刷174、176–183、189–192、645–646、667–677頁と奥付。
原典画像・抽出本文・作業ログはgitignore対象の `target/issue878/` にのみ置き、リポジトリには転載しない。

## 条件・式と現行機能の対応

| 原典の節・印刷頁 | 確認した内容・単位 | 実装・製品docsの対応 | 状態・限界 |
| --- | --- | --- | --- |
| §3.6.1 p174、令63条 | 鉄骨造部分への適用。RC等との併用でも鉄骨部分に適用 | `sepika-core/src/structure_kind.rs`、製品docsの構造種別判定／鋼構造断面検定 | 条件を確認。材料区分による部材検定の分類だけで、鉄骨部分の全細則を自動確認する機能ではない |
| §3.6.4 pp176–178、令66条・平12建告1456号 | 露出・根巻・埋込で別条件。露出の定着長20d、全アンカー断面積比20%以上、ベースプレート厚1.3d等。許容応力度計算で除外される項目と、戻り止め・原則として残る定着の条件を区別 | coreの境界条件、design-jpの部材検定・接合部検定には柱脚形式・アンカー配置/定着等を検定する入口なし。製品docs「柱脚・接合部とブレース検定の範囲」 | 専用検定は未対応。列挙した値を新たな計算採用値にしない。計算を行うだけで全仕様規定が除外されるとは扱わず、必要条件の確認を#697で継続 |
| 付録1-2.6 pp667–669 | 柱脚3形式の区分、露出柱脚を骨組上ピンとした場合も柱脚Mの評価が必要。p669表の反曲点高比は特定の1層1スパン・断面・アンカー条件のケーススタディ | ピン/固定/ばねは解析条件。柱脚の強度検定・ディテール入力の代用ではない | 条件を確認。反曲点高比0.3を全建物・全ルートへ転用しない。ピン解析の存在自体を不具合や一律危険としない |
| 付録1-2.6 pp670–672、式(付1.2-20) | 回転剛性 KBS=E nt Ab(dt+dc)^2/(2ℓb)。E:N/mm²、Ab:mm²、dt,dc,ℓb:mm、KBS:N·mm/rad。ベースプレート曲げ剛性・基礎との密着・アンカー抜出し防止等の前提。ルート1-1のピン近似と他ルートの計算を区別 | 柱脚のアンカー諸元からKBSを自動算出する入口なし。一般の回転ばね値の入力は当該式の自動適用ではない | 式・条件を原画像確認、実装一致は該当せず。計算例や任意のばね値を確認済み柱脚モデルとしない |
| 付録1-2.6 pp671–676 | アンカーの伸び能力・保有耐力接合・せん断破壊防止・コンクリート破壊防止とルート別分岐。pp673–675の耐力式を含む | 専用検定なし。Ds/Qu側は既存#871/#872の責務 | 条件の存在・入力不足を確認。各耐力式・分岐全体の数値照合は今回は未確認。Ds補正の調査#719を柱脚検定の完了根拠にしない |
| 付録1-2.6 p677、§3.6.4 p177 | 根巻の力伝達・頂部補強等と、根巻高/埋込深さの条件 | 根巻・埋込専用検定なし | 専用検定未対応。p678以降の根巻・埋込耐力式・設計例は今回は未確認。施工細則を自動的な新機能要求にしない |
| §3.6.5 pp179–180、令67条 | 炭素鋼/ステンレス鋼と接合方法、普通ボルトの規模条件と戻り止め。令6国交告955号の規模条件は旧来の軒高/張間/面積条件と区別 | `joint_wiring::steel_panel` はパネル検定。普通ボルト・高力ボルト・溶接等の接合方法を検定する入力ではない | 接合方法・規模・戻り止め等の自動適用確認は未対応。パネル適合を令67条全体の適合としない |
| §3.6.5 p181 | 普通ボルト孔ずれの層間変形角増分 Rs=n1 c/H+n2 c/(H cosθ)。c,H:mm、θ:rad、Rs:無次元。引張筋かい骨組の条件、梁継手普通ボルトへの不適用を保持 | 孔径/呼び径・接合数を入力してRsを加算する入口なし。一般の層間変形角とは別量 | 未対応。剛性率のRsと同一視しない。補足数値を下記に記録し、機能一致とはしない |
| §3.6.5 pp182–183、平12建告1464号 | ボルト縁端距離・摩擦面処理、ダイアフラム/梁フランジのずれ・食違いと溶接材料等の適用条件 | パネル検定の梁位置ゲートはフランジ中心位置等を扱うが、溶接の品質・ずれ許容値検定ではない | 詳細専用検定は未対応。幾何ゲートやパネル耐力から溶接適合を推定しない |
| §3.6.6 pp189–190、令68条 | ボルト中心間隔2.5d以上、孔径の加算上限と例外条件。高力ボルトの通常+2mm、径27mm以上で支障なしの場合+3mm等 | ボルト径・孔径・間隔の接合部データと専用検定なし | 未対応。径や板厚を持つ一般鋼断面はこれらの入力の代用ではない |
| §3.6.7 p190、令69条・昭62建告1899号 | 全方向の水平力に対する安全性と釣合いのよい配置。規定適用除外の構造計算の要件を確認 | `SteelDesign::check` → `steel::brace::check_brace` は部材の総断面軸力検定。`run_member_design_checks` から接続。曲げ・せん断・接合破断は対象外 | 部材検定の限定範囲は公開入口で確認。配置・全体系の法令要件は今回は未確認、#697継続。p190を筋かい接合部の耐力式と誤認しない |
| 付録1-2.2 pp645–646 | 通し/内/外ダイアフラムの形式と溶接詳細の区別。2018年版冷間成形角形鋼管設計・施工マニュアル等を参照 | パネル諸元と標準形式の幾何ゲートだけでは各ダイアフラム詳細を検定しない | 詳細専用検定は未対応。pp647以降・接合指針本体は今回は未確認。N/A/Fとパネル耐力式全体は#723を維持し重複照合しない |
| §3.6.8 pp190–192、令70条・平12建告1356号 | 地上階数3以上、一定の耐火/準耐火建築物の除外、一の柱の火熱耐力低下時の倒壊条件と30分性能。柱除去モデルの常時荷重・短期許容応力度による確認 | `sepika-load/src/fireproof.rs` の階別種別・面重量と `self_weight_case_content` 等は重量だけ。製品docs「階共通耐火被覆の重量」 | 火災安全性/被覆要否/認定/厚さ判定は未対応。重量式qPL、密度・標準面重量・周長の一般的適用根拠はこの節に定義されず、本書では照合不可（別原典・実仕様が必要）。重量入口のVerificationと区別 |

## 独立数値と実入口

### ブレースの公開検定入口

調査用の短命Cargoプロジェクトを `target/issue878/probe` に作り、リポジトリのcore/design-jpをpath依存として
`SteelDesign.check` を呼んだ。恒久回帰テストは追加していない。最初のmanifestの相対パス誤り、属性のDefault未実装によるコンパイル失敗を修正後に実行した。
入力は `SectionShape::SteelFlatBar { width:100, thick:10 }.to_section(...)`、用途Brace、材料SN400B、
E=205000 N/mm²、長期、材長3000mm。断面積は独立に100×10=1000mm²。
長期引張の材料値そのものの原典照合は#879であり、本件では現行のF=235 N/mm²を入力条件として使用する。
許容軸力は独立算術で1000×235/1.5=156666.666666…N。

| 入力N [N] | 独立検定比 | 観測 |
| --- | --- | --- |
| 156666 | 0.999995744681 | Checked、軸力1項、OK |
| 156666.66666666666 | 1 | Checked、軸力1項、OK |
| 156667 | 1.000002127660 | Checked、軸力1項、NG |
| 0 | 0 | Checked、軸力1項、OK。接合部の無荷重時適合という意味ではない |
| 100000、My=Mz=10^9 N·mm、Qy=Qz=10^6 N | 0.638297872340 | Checked、軸力1項、OK。曲げ・せん断の合格ではない |

各行について `DesignCtx.steel_attr` の継手フランジ欠損率とスカラップウェブ欠損率を0%/50%としても同値だった。
これらはフランジ・ウェブの曲げ断面性能用の属性であり、軸力の有効断面積やボルト孔そのものを表す入力ではない。
平鋼の孔欠損50%を正当にモデル化した試験とはしない。
現行ブレースがこれらの属性から有効引張断面積を算定しないことを示す観測であり、
接合部の孔欠損・偏心・破断耐力を評価できるという保証には使えない。

再現時は上記の断面・材料・`DesignCtx { kind: MemberKind::Brace, length:3000, ..Default::default() }` を作り、
`MemberForcesAt { pos:0.5, n, qy, qz, my, mz }` を各行の値で渡す。
`CheckOutcome::Checked` の `components.len()`、`ratio()`、`ok()` を出力する。
最終実行コマンドは `cargo run --locked --manifest-path target/issue878/probe/Cargo.toml`。
以下を独立Cargo packageの `src/main.rs` とすれば再現できる。作業用 `Cargo.toml` は次のとおり（probeは `target/issue878/probe` に配置）。

```toml
[package]
name = "issue878-probe"
version = "0.0.0"
edition = "2021"
[workspace]
[dependencies]
sepika-core = { path = "../../../crates/sepika-core" }
sepika-design-jp = { path = "../../../crates/sepika-design-jp" }
```

初回は `cargo generate-lockfile --manifest-path target/issue878/probe/Cargo.toml` で作業用lockを作成する。


```rust
use sepika_core::{
    ids::{MaterialId, SectionId},
    model::{FrameSectionUse, Material, MaterialCategory, SteelDesignAttr},
    section_shape::SectionShape,
};
use sepika_design_jp::{
    CheckOutcome, DesignCheck, DesignCtx, MemberForcesAt, MemberKind, SteelDesign,
};
fn main() {
    let mut sec = SectionShape::SteelFlatBar {
        width: 100.0,
        thick: 10.0,
    }
    .to_section(SectionId(0), "平鋼100x10".into());
    sec.frame_use = Some(FrameSectionUse::Brace);
    let mat = Material {
        id: MaterialId(0),
        name: "SN400B".into(),
        category: MaterialCategory::Steel,
        young: 205000.0,
        poisson: 0.3,
        density: 7.85e-9,
        shear: None,
        fc: None,
        fy: None,
        concrete_class: Default::default(),
        strength_factor: None,
    };
    for (name, n, m) in [
        ("below", 156666.0, 0.0),
        ("boundary", 156666.66666666666, 0.0),
        ("above", 156667.0, 0.0),
        ("zero", 0.0, 0.0),
        ("bending", 100000.0, 1e9),
    ] {
        let forces = MemberForcesAt {
            pos: 0.5,
            n,
            qy: m / 1000.0,
            qz: m / 1000.0,
            my: m,
            mz: m,
        };
        for loss in [0.0, 50.0] {
            let ctx = DesignCtx {
                kind: MemberKind::Brace,
                length: 3000.0,
                steel_attr: Some(SteelDesignAttr {
                    elem: sepika_core::ids::ElemId(0),
                    joint_flange_loss: loss,
                    joint_web_loss: 0.0,
                    scallop_web_loss: loss,
                    lb_direct: None,
                    lateral_brace_count: None,
                    lk_y_direct: None,
                    lk_z_direct: None,
                    c_direct: None,
                }),
                ..Default::default()
            };
            match SteelDesign.check(&forces, &sec, &mat, &ctx) {
                CheckOutcome::Checked(r) => println!(
                    "{name} loss={loss} ratio={:.12} ok={} count={}",
                    r.ratio(),
                    r.ok(),
                    r.components.len()
                ),
                v => panic!("{v:?}"),
            }
        }
    }
}
```
作業用依存解決はリポジトリのCargo.lockとは別で、製品回帰の依存固定結果には読み替えない。

### 実装を持たない式の独立算術

p181の孔ずれ増分は、1スパン・上下梁端接合数平均n1=2、筋かい両端n2=2、
c=2mm、H=4000mm、水平投影3000mmの筋かい（長さ5000mm、cosθ=0.6）なら、
Rs=4/4000+4/2400=0.002666666666…（1/375）。c=0なら0、n1=n2=0なら0となる。
この式を剛性率Rsへ流用せず、孔ずれ込みの実装やルート適合の確認済み値にはしない。

p670のKBSはE=205000 N/mm²、nt=2、Ab=314mm²、dt=200mm、dc=150mm、ℓb=600mmで
13,142,208,333.333…N·mm/rad。ℓbを1200mmとすれば半分になる。
単位・寸法依存の確認だけで、当該形状の密着・剛性・抜出し防止の成立や一般回転ばねの物理Validationはしていない。

### 被覆重量の既存公開経路

`cargo test -p sepika-load --locked fireproof -- --nocapture` で既存の焦点6試験を実行し、6成功・失敗0・ignore0だった。
`story_gen::tests::fireproof_shapes_roles_and_weight_paths_agree_without_steel_markup` は
`self_weight_case_content`、`generate_stories`、`generate_stories_with_synced_self_weight` の公開入口を通す。
例えば200×20mmの平鋼、L=5000mm、S柱・吹きつけq=0.001 N/mm²（1000N/m²）では、
実装から独立した矩形周長2×(200+20)=440mmにより被覆増分は2200N。
重量割増1.9をこの増分へ重ねず、DL・地震重量・物理質量相当重量の増分が一致することを検査する。
形状・用途・なし/吹きつけ/成形版、階差・S/CFT条件独立、対象外・不正入力を既存試験で確認する。
原典p191–192はq=1000N/m²や当該断面の防火性能を保証しておらず、単なる検証用入力である。

## 要素→テスト索引と残範囲

| 要素 | 証拠 | 状態 |
| --- | --- | --- |
| ブレース総断面軸力・許容境界、接合未検定の範囲 | 上記短命probeの公開 `SteelDesign::check`。新規恒久テストなし | 🔶 有限Verification。全モデル/GUI・接合Validationなし |
| 耐火被覆の付加重量と公開荷重経路 | `sepika-load/src/story_gen/tests.rs` の `fireproof` を含む焦点試験 | 🔶 既存Verification。防火性能のValidationなし |
| 柱脚・接合詳細、普通ボルト孔ずれ | 原画像と公開入力・実装の対応調査、独立算術 | ❌ 対応式の実入口なし。法令条件の適合は確認できない |

今回の確認範囲の外に残す項目は次のとおり。

- **今回は未確認**：柱脚の全耐力式・設計例、付録1-2.2 p647以降、付録1-2.6 p678以降、§3.6.5 p184–188の溶接材料全表、個別建物の仕様規定適用除外、全方向の配置安全性、全法令本文の現行改正照合、専門家サインオフ、手動GUI・実建物/実験Validation。
- **本書では照合不可（別原典必要）**：閲覧した防火被覆節にない面重量・qPLの実仕様適用根拠、現行ブレースの接合部有効断面積・偏心等を含めた採用方式。未掲載だけで不具合認定しない。
- **今回対象外（別の追跡）**：材料・細長比・許容応力度は[#879](https://github.com/hrntsm/SEPIKA/issues/879)、耐震ルートは[#880](https://github.com/hrntsm/SEPIKA/issues/880)。S/CFTパネルN/A/F・耐力式全体は[#723](https://github.com/hrntsm/SEPIKA/issues/723)、変動軸力は[#752](https://github.com/hrntsm/SEPIKA/issues/752)、微小断面積は[#892](https://github.com/hrntsm/SEPIKA/issues/892)。これらの未確認状態を維持する。
- **必須条件の未解決**：令66〜68条の柱脚緊結・戻り止め・定着・接合詳細の条件、普通ボルトずれの考慮、令69条全方向安全性、令70条の適用と被覆性能は自動確認未対応または適合未確認として[#697](https://github.com/hrntsm/SEPIKA/issues/697)へ残す。[今回の具体的な申し送りコメント](https://github.com/hrntsm/SEPIKA/issues/697#issuecomment-6420241958)に条件と不足を反映した。別途確認が必要という制約を製品docsへ反映したが、法令要件を解消した意味ではない。柱脚のDs/Qu条件別採用は[#871](https://github.com/hrntsm/SEPIKA/issues/871)／[#872](https://github.com/hrntsm/SEPIKA/issues/872)を維持する。

`gh issue list --state all --search '柱脚 OR アンカーボルト OR 接合 OR ボルト'` で既存課題を確認した。
本調査で修正必要と確定した新たな計算実装差異はなく、新規修正Issueは起票しない。
施工細則を新規機能要求へ一括変換しない。上記の必須条件・危険側となり得る柱脚M無視の注意を、対象外や単なる未掲載を理由に解決済みにはしない。

## ローカル検証結果

| コマンド・作業 | 結果 |
| --- | --- |
| 原典PDFをPyMuPDFで描画し全対象PNGを目視 | 上記重点頁と奥付を確認。OCRだけで式・条件を決めていない |
| `cargo run --locked --manifest-path target/issue878/probe/Cargo.toml` | 10条件の公開検定結果を観測、上記独立値と一致。初回の準備エラーは前述 |
| `cargo test -p sepika-load --locked fireproof -- --nocapture` | 6成功、0失敗、0ignore。他の絞込除外試験を成功件数に含めない |
| `cargo run -p xtask --locked -- check-docs` | 成功（summary 124、links 656、impl refs 703） |
| `mdbook build` | 成功 |
| `git diff --check` | 成功 |

文書だけの変更であり、計算実装修正や全クレート回帰は行っていない。新規V&Vと索引の相対リンク先・見出しも確認した。
最終headの独立レビュー・CIは別ゲートで、ここに予記しない。
