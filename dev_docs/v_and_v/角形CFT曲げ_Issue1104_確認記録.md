# 角形CFTの曲げ耐力（Issue #1104）

状態：🔶。以下は幾何・採用応力モデル・利用側の限定Verificationであり、規準全条件・実験Validationとは区別する。実行結果を後段に示す。

## 原典照合と補足範囲

2026-11-03、[藤本ほか2009公開原典](https://www.jstage.jst.go.jp/article/aijt/15/31/15_31_757/_pdf)の誌面758–760を画像で確認した。式(14)–(17)はR/rの同心等厚材料領域、式(18)(19)はr≤xn≤cD−r、式(20)(21)はcD<xn、式(23)–(26)はr≤xn≤cD−rである。式(22)のコア基準曲げと式(27)の鋼管Zpを中長柱へ接続する記述も確認した。

0<R<t、角部内とコア外の中立軸は、同じ材料領域・応力分布の積分による補足であり、印刷式の外挿ではない。長方形の同軸曲げ計算と、面外・横座屈の検討は別である。誌面760の経験較正の留意点を踏まえ、厳密な積分を全条件の安全保証としない。

## 独立算例

原典の印刷式(18)(19)(23)–(26)を、本番とは別のPython算術で評価した。B=D=400、t=16、R=40、r=24、xn=100 mm、許容fc=10、終局Fc=30、Fy=325 N/mm²の結果：

| 量 | 独立期待値 |
| --- | ---: |
| 許容コアN [N] | 181660.31800017736 |
| 許容コアM [N·mm] | 27304207.159748003 |
| 終局鋼管N [N] | −1747200.0 |
| 終局鋼管M [N·mm] | 1020860058.9516826 |
| 終局コアN [N] | 1096583.3605270158 |
| 終局コアM [N·mm] | 146611097.68432254 |
| 終局合計N [N] | −650616.6394729842 |
| 終局合計M [N·mm] | 1167471156.6360052 |

加えて面積解析式、独立幅積分、矩形解析解と比較し、R→0、R=t両側の差を縮小して数値誤差と形状差を分ける。具体的な公差・実行結果を後段に記録する。

## 未確認範囲

- 中長柱の許容低減曲線・軸力分担の未確定6セルは本Issueの断面モデル拡張と別。追跡は[Issue #1124](https://github.com/hrntsm/SEPIKA/issues/1124)。
- #955の現行終局式・柱長境界に関する原典照合全体、実建物・実験値によるValidationは未実施。
- Windowsの実画面操作は専用ウィンドウを取得できない環境制約があり、本番編集コマンドでの確認と区別する。

## 公差と境界の検証

本番の幾何は円弧解析積分を用い、固定メッシュを耐力モデルにしない。中立軸は80回の二分探索で軸力に整合させる。

- コアの切断A/Q/Iは、正方形400×400・長方形600×400／400×600 mm、R=0/8/16/40/半寸法と、角部境界・中立軸0・両端近傍を独立幅積分と比較する。sin変換した中点積分65536→131072点の両方で、k次モーメントの絶対誤差と分割差を1e−9 b h^(k+1)未満とする。断面全体のスケールに対する公差であり、零近傍の相対誤差を用いない。
- 短柱の材料別N/Mは独立65536点幅積分と比較する。Nの公差は2e−9 Fy B D、Mの公差は2e−9 Fy B D²。材料別の合計軸力を設計軸力へ渡して回収するMuは、材料別Mの和に対し1e−12 Fy B D²以内とする。角部境界・コア境界・全引張／全圧縮端を含む。
- 原典重複域の前掲固定値は絶対誤差1e−5 N又はN·mm以内、中長柱の独立算例Mu=380701610.601 N·mmは0.01 N·mm以内で照合する。
- R=0及びR=t=16 mmについてε=0.1→0.01→0.001 mmと縮小し、幾何性能・許容曲げ・短中長柱Muの境界値との差が前段の0.2倍未満、最終相対差1e−5未満へ収束することを確認する。境界で異なる柱長区分や材料へ切り替えない。長方形は両軸と幅せい交換で同値性を確認する。

## 実行記録

統合前のcore/design-jpテストは成功した。利用側App/MCPの途中ビルドは、先行PR #1122のmain統合と実行が重なり、旧・新API混在によるStoryIdentification等の未解決参照で失敗した。これは成功として数えず、統合後の再実行は下表のとおり成功した。

文書検査を修正した先行PR #1123のmainを取り込み、新版check-docsとmdbook buildに成功した。初回はsummary141、links864、impl refs425、最終は141/863/425。


最終core libは595件成功。追加したQD1/min接続試験では、初回の明示QD1期待値を強軸だけで評価したため1件失敗した。独立レビューでも同じ失敗を再現し、明示QD1は既存仕様として両軸のMu由来要求を評価することを確認した。期待値を両軸のMu／せん断耐力の最大へ訂正し、本番計算は変更していない。訂正後は別のfresh Reviewerで独立レビューし、design-jp最終689件が成功した。


Muの呼出しをcrates全体で検索した範囲では、本番利用は終局検定とQD1/minである。新しい骨格モデル・利用経路は追加していない。GUI featureの失効回帰は本番のモデル編集コマンドを通す確認であり、eguiの入力イベントやWindows実画面の手操作を実施した証拠とは扱わない。


### ローカル最終集計

ツールはclippy 0.1.99（58cf8f9c35、2026-10-13）で確認した。

| 実行コマンド | 結果 |
| --- | --- |
| `cargo test -p sepika-core -p sepika-design-jp --locked --quiet` | 統合前のlib・関連統合一式成功。追加テストの最終結果は次の2行 |
| `cargo test -p sepika-core -p sepika-design-jp --lib --locked --quiet` | core 595件成功。designは追加QD1期待値1件を前述のとおり訂正 |
| `cargo test -p sepika-design-jp --lib --locked --quiet` | 訂正後689件成功 |
| `cargo test -p sepika-app --features gui cft_ultimate_app_exposes_axial --locked --quiet` | 1件成功。半径編集・失効・保存再読込・再解析Mu更新 |
| `cargo test -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp cft --locked --quiet` | App lib21件・関連統合1件・MCP lib5件、計27件成功 |
| `cargo test -p sepika-edit radius --locked --quiet` | 4件成功。カタログ／直接性能保持・半径未知拒否・Undo |
| `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp --all-targets --locked -- -D warnings` | 成功 |
| `cargo clippy -p sepika-core -p sepika-design-jp -p sepika-app -p sepika-mcp --features sepika-app/gui,sepika-mcp/mcp --all-targets --locked -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` / `git diff --check` | 成功 |
| `cargo run -p xtask -- check-docs` / `mdbook build` | 最新main統合後も成功。summary141、links863、impl refs425 |

GUI featureの初回・併用構成の依存再ビルドと共有targetのロック待ちが長かったが、これを試験成功の証拠には数えず、終了コードと対象試験の結果を用いた。CIはローカル結果と別にPRで確認する。


### 独立レビュー

実装者とは別のfresh ReviewerがIssue #1104・#418全文、原典誌面758–759の画像、最終差分を確認し、必須修正事項なしと判断した。独立実行の `cargo test -p sepika-design-jp --lib cft --locked` は61件成功・0失敗。検定契約46、旧結果失効・保存、App/MCPの値／理由伝播、準用と未確定許容モデルの文書上の区別を確認した。Windows実画面・実験Validation・規準全条件への適合は、このレビューでも未実施である。
