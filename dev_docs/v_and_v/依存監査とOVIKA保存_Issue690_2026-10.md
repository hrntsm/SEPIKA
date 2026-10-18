# 依存監査とOVIKA保存（Issue #690）

## 対象と記録時点

[Issue #690](https://github.com/hrntsm/SEPIKA/issues/690) の依存監査対応と、zip更新後のOVIKA保存・再読込のVerificationを記録する。
基点はmain `a1098c55f9ec82e9bed4cee8ab0c4f6bab4984c7`、作業ブランチは `fix/690-security-dependencies`。
2026-10-18の中間snapshotと、採用feature訂正後の最終ローカル検証を記録する。旧8feature明示・PPMdなしの構成でmetadata・依存監査・App default・GUI/Parquet/MCP関連構成・静的/文書検査を確認した。独立レビュー・親監査・最終head CI・mergeは未完了である。

先行する [#671](https://github.com/hrntsm/SEPIKA/issues/671) / [PR #689](https://github.com/hrntsm/SEPIKA/pull/689) の機能変更は本対応へ含めない。
製品仕様・圧縮形式・保存スキーマ・計算式を変更する対応ではなく、新たな構造計算上の近似は導入しない。
製品の保存仕様は既存 `docs/` を正とし、本書は依存の判断根拠と検証証拠を扱う。

## 変更前のCI観測

[CI run 39078652307](https://github.com/hrntsm/SEPIKA/actions/runs/39078652307) は、PR #689のhead `de3a376c6222fc2cca0ea4189bb1df036da86807` に対する失敗記録である。
audit job `117530934239` とdeny job `117530934156` のログを照合した。
作業環境では `%TEMP%/sepika671-ci689-job-117530934239-failed.log` / `sepika671-ci689-job-117530934156-failed.log` に保全している。

| 分類 | 変更前の対象 | 観測と扱い |
| --- | --- | --- |
| 実脆弱性 | zip 2.4.2、RUSTSEC-2026-0367 / 0368 / 0369 | audit/denyで検出。CIが報告する修正版は `>=9.0.0`。ignoreによる回避は採用しない |
| 保守終了 | zvariant 5.12.0、zbus_names 4.3.2、zvariant_derive 5.12.0、RUSTSEC-2026-0370 / 0371 / 0372 | auditではunmaintained警告、denyではunmaintainedエラー。実脆弱性とは区別する |
| 既存警告 | private-gemm-x86 0.1.20のyanked | warning。本Issueの失敗原因の解消・修正成果へ含めない |

既存のquick-xml勧告ignoreも本Issueで解消したとは扱わない。変更前CIログの文言だけを、最新の勧告原文や上流の更新可能性の根拠にはしない。

変更前denyログの実依存経路は、zip 2.4.2がsepika-ioの直接依存とsepika-appのdev依存である。
GUI側はsepika-app → eframe 0.34.3 → egui-winit 0.34.3 → accesskit_winit 0.32.2 → accesskit_unix 0.21.1 → atspi 0.29.0 / atspi-common 0.13.0 / zbus 5.16.0から、保守終了の3crateへ至る。
この変更前の固定lock経路と、以下の最新上流の許容版調査を区別する。

## 一次資料と採用方針

| 原資料 | 2026-10-18に照合した事項 | 証拠 |
| --- | --- | --- |
| [RustSec 0367](https://rustsec.org/advisories/RUSTSEC-2026-0367)、[0368](https://rustsec.org/advisories/RUSTSEC-2026-0368)、[0369](https://rustsec.org/advisories/RUSTSEC-2026-0369) | 0367は展開時の特権Unix権限保持、0368は展開先外へのsymlink作成、0369はsymlink経由のUnix権限適用。3件とも修正版は `>=9.0.0` | RustSec公式advisory-db原文6件を保全・全文照合 |
| [RustSec 0370](https://rustsec.org/advisories/RUSTSEC-2026-0370)、[0371](https://rustsec.org/advisories/RUSTSEC-2026-0371)、[0372](https://rustsec.org/advisories/RUSTSEC-2026-0372) | 2026-10-16の `unmaintained`。各standalone crateの修正版なし。zbus 6のwire / names / deriveへ統合 | 同上。実脆弱性の修正版待ちとは区別 |
| [zip版情報](https://crates.io/api/v1/crates/zip)、[eframe版情報](https://crates.io/api/v1/crates/eframe) | 調査時の最新stableはzip 9.0.5、eframe 0.37.0 | crates.io公式API応答を保全・照合 |

原文の取得元はRustSec公式リポジトリの `crates/zip/RUSTSEC-2026-0367.md` 等である。
作業環境では `%TEMP%/sepika690-source-RUSTSEC-2026-0367.md` から `0372.md` までと、`sepika690-source-*-dependencies.json` / `*-latest.json` に保全している。
初期原資料28ファイルのSHA-256一覧は `%TEMP%/sepika690-source-hashes.json`、ライセンス原本等を含む最終30ファイルの取得URL・SHA-256は `sepika690-source-index.json` に保持し、実ファイルとの一致を確認した。以下は6勧告の照合値である。

| 勧告 | SHA-256 |
| --- | --- |
| 0367 | `A3CEEC31DB85C2E07A2B8DAF0A7A17A5A624601BB256CEDAB4C508750FF677DF` |
| 0368 | `C056588FB9936466517E99B8644AAB34804502A01F34C6CAE323BD40E4E63BA0` |
| 0369 | `4BED2EC03D7B53525422DC136075476EAEB644E2AB3483547A525437AE722DC5` |
| 0370 | `55991B6A4484C3541C8091CB16CF7446C8610DE09AC642AB3FF88C332E394BF0` |
| 0371 | `3E618914DAB3EF0D6D2434F42AC8AA1947CB1738FD156B4F0092D6C7AD53B394` |
| 0372 | `E03677B3351DB11984E2FD3BD839B6368CFC3A93ACA08C4401980264B228635D` |

最新GUI上流からの更新可能性は、次の公式依存APIで確認した。現在のworkspaceはeframe 0.34系であり、この表の最新版へのGUI更新を実施した記録ではない。

| 公開版・公式依存資料 | 依存制約 |
| --- | --- |
| [eframe 0.37.0](https://crates.io/api/v1/crates/eframe/0.37.0/dependencies) | egui-winit `^0.37.0` |
| [egui-winit 0.37.0](https://crates.io/api/v1/crates/egui-winit/0.37.0/dependencies) | accesskit_winit `^0.34.0` |
| [accesskit_winit 0.34.0](https://crates.io/api/v1/crates/accesskit_winit/0.34.0/dependencies) | Unix対象でaccesskit_unix `^0.23.0` |
| [accesskit_unix 0.23.0](https://crates.io/api/v1/crates/accesskit_unix/0.23.0/dependencies) | zbus `^5.5`、atspi `^0.29` |
| [accesskit_unix 0.24.0](https://crates.io/api/v1/crates/accesskit_unix/0.24.0/dependencies) | 調査時の最新stableでもzbus `^5.19`、atspi `^0.29` |
| [atspi 0.30.0](https://crates.io/api/v1/crates/atspi/0.30.0/dependencies) | 調査時の最新stableでもzbus `^5.5`、atspi-common `^0.14.0` |
| [atspi-common 0.14.0](https://crates.io/api/v1/crates/atspi-common/0.14.0/dependencies) | zbus `^5.5`、zbus_names `^4.1.1`、zvariant `^5.2` |

最終採用方針はworkspaceのzip要求を9へ更新し、`default-features=false` と旧zip 2.4.2の既定8feature `aes-crypto,bzip2,deflate64,deflate,lzma,time,zstd,xz` を明示して、既存の対応機能を維持することである。
zip 9の既定featureにはPPMdが追加され、method 98の読込も増えるため、新形式を追加しないScopeに合わせてPPMdと不要な `ppmd-rust` lock entryを除く。
当初のzip 9既定featureをそのまま使った構成は中間snapshotであり、以下の旧構成の成功を最終採用構成の成功へ読み替えない。lock差分とAPI追随は必要範囲に限定する。
GUI側は上表の最新版でもaccesskit / atspi経由にzbus 5の制約が残り、単なる許容版の更新ではzbus 6へ到達できない。
この制約を根拠に、既存の保守終了依存に対する方針に従い0370 / 0371 / 0372のみをdenyのignore対象へ追加する。
GUIの大更新・機能削減や、zipの実脆弱性3件のignoreは採用しない。
上流の依存制約がzbus 6へ更新された時点で、到達可能な更新経路と3件のignore除去を再評価する。

既存のBZip2対応を維持するzip 9.0.5 → bzip2 0.6.1 → libbz2-rs-sys 0.2.5が加わり、初回denyのlicenses検査は未許可の `bzip2-1.0.6` で失敗した。
親担当が公開crateのLICENSE全文と[SPDX公式識別子](https://spdx.org/licenses/bzip2-1.0.6.html)を照合・承認し、そのIDだけをallowへ追加した。
機能削減・別backend導入・ライセンス検査全体の緩和は行っていない。原LICENSEの著作権・条件・免責を保持し、一般的な法務適合の保証へ拡張しない。
原本はローカルregistryの公開crateから保全し、[LICENSE](https://docs.rs/crate/libbz2-rs-sys/0.2.5/source/LICENSE) / [Cargo.toml](https://docs.rs/crate/libbz2-rs-sys/0.2.5/source/Cargo.toml)への参照とSHAを上記source-indexへ記録している。

## 実使用APIと維持する境界

変更前 `crates/sepika-io/src/ovika.rs` の読取では、`ZipArchive::new` / `by_name` / `size` と `Read::take` により要素をメモリへ読む。
保存は `ZipWriter::new` / `start_file` / `finish` を使用する。extract系APIはこの経路で使用しない。`crates/` のRustソースにもZIPの展開APIや `ZipStreamReader` の呼出は見つからなかった。
Unix展開APIに関する勧告と現使用APIの対応を分けて確認し、Windowsアプリであることだけを無影響の根拠にしない。

更新先9.0.5の公式ソースでも、上記読取・書込APIの存在を照合した。取得元は[公式v9.0.5ソース](https://github.com/zip-rs/zip2/tree/v9.0.5/src)、保全先は `%TEMP%/sepika690-source-zip9-src-read-*.rs` / `sepika690-source-zip9-src-write.rs` 等であり、正確なraw URLはsource-indexへ記録した。
`file_names()` の戻り値が `Result<Cow<str>>` になったため、既存保存エントリ一覧assertをResult収集へ追随した。
既存STB試験2箇所でText/CDataの `.as_ref().iter()` にE0282が発生し、同じbyteデータの `.iter()` へ追随した。STB本番処理や期待値は変更していない。新stableだけを原因とは断定しない。

| 対象境界 | 更新後に確認する観測可能な契約 |
| --- | --- |
| 正常保存と再読込 | 同じモデル・付随成果を保存して復元でき、既存形式を維持する |
| 不正archive | 非ZIP・破損要素を成功として扱わない |
| サイズ上限 | 本番の要素当たり4 GiB上限を維持し、申告サイズと実読込量の両境界で超過を拒否する |
| hash / manifest | hash不一致、必須要素の欠落、manifest不整合を拒否する |
| 参照境界 | 一致hashを持つ復号可能な不正参照モデルでも、既存のモデル検査を通過させない |
| 原子性 | 不正保存・読込失敗で既存ファイルやApp入力・既存成果を破壊しない |

スキーマ版・解析契約版・検定契約版を変更せず、既存の検証境界を更新後ライブラリ上で直接確認する。
MCP固有実装・専用受入は対象外であり、既存CIの利用側保証とは区別する。

## 実行証拠

以下の表と途中失敗は2026-10-18のzip 9既定feature（PPMdを含む）を用いた中間snapshotの記録である。採用訂正後は[最終構成のローカル検証](#最終構成のローカル検証)に分ける。
`rustup update stable` 後、Windows MSVC stableのrustc 1.99.1 / Clippy 0.1.99で確認した。
確定報告は `%TEMP%/sepika690-final-report.txt`（SHA-256 `EC9992511CCDAC298D32C140FA237FBD34E805F67FD8F6B37240EBD387B291C1`）。
run一覧・完全コマンド・環境・開始終了時刻・終了コード・前後ファイルSHAは `sepika690-coder-artifact-index.json` と各 `sepika690-run-*.result.json`、生ログは同名 `.log` に保持する。
42個の実行証拠のhash一覧は `sepika690-coder-artifact-hashes.json` に保持し、実ファイルとの一致を照合した。

| 検証 | 結果 |
| --- | --- |
| 勧告原文・最新上流制約の照合 | 上記6勧告原文と公式依存APIを照合。30一次artifactのURL/hashを保全 |
| `cargo update -p zip --precise 9.0.5` | exit 0。初期loggerのraw欠落あり。実lockと後続実行でzip 9.0.5を確認 |
| `cargo test -p sepika-io --locked` | `io-default-final3`、exit 0、195成功・既存ignored 2。lib181、integration9+1+1+1+2、doc0。`CARGO_NET_OFFLINE=true` |
| `cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195` | `audit-final`、exit 0。zipの0367/0368/0369は検出なし。既存quick-xml ignore2件を維持。保守終了/yankedの警告7件は残る。`CARGO_NET_OFFLINE=false` |
| `cargo deny --all-features check` | `deny-all-features-final`、exit 0。advisories / bans / licenses / sourcesが全てok。`CARGO_NET_OFFLINE=false` |
| 所有2Rustファイルのrustfmt `--check --edition 2021 --config skip_children=true` | `format-owned-check`、exit 0。workspace全体fmtの結果とは区別 |
| 関連App受入・GUI / Parquet・横断Clippy / strictdoc / 文書検査 | 中間構成のApp default441成功。関連featureは採用訂正のため途中中断。この段階の静的後段は未開始 |
| 文書リンク・見出し | 変更3文書のローカルリンク421件のパス存在を確認。追加した見出しアンカーを照合。後段の製品文書・差分検査は下記最終構成記録を参照 |

IO defaultは既存の正常往復・named payload・任意entry・schema・hash/manifest・復号後参照境界を含む。
新規4試験は次の観測を保証する。4 GiB実割当は行わず、本番も使用するprivate helperへ小さい上限を渡す。本番定数とmanifest / payloadの全2呼出しは4 GiBを維持する。

| `ovika::tests` の試験 | 独立した期待契約 |
| --- | --- |
| `malformed_zip_is_rejected_without_changing_input` | 非ZIPを `IoError::Zip` で拒否し入力bytesを保持 |
| `entry_size_limit_accepts_boundary_and_rejects_larger_header` | 8 byte payloadを上限8で受理、上限7で `EntryTooLarge` |
| `false_entry_size_is_rejected_by_zip_reader` | 実体64 byte / 申告1 byteを上限8で読むと、zip 9自身の `InvalidData` 拒否を受ける |
| `actual_entry_bytes_stop_at_one_byte_above_limit` | 64 byte reader / 上限8で `EntryTooLarge`、消費は9 byteで停止。境界8 byteは受理 |

偽headerのzip側拒否と、SEPIKA自身の実byte上限を別試験で観測する。
自前上限は既存の `take(limit + 1)` / `read_to_end` / 実byte比較だけを `read_entry_bytes_capped` へ抽出し、試験側へ実装をコピーしていない。

### 途中失敗と対象snapshot

| run | 保持した失敗・証拠制約 |
| --- | --- |
| `io-default` | exit 101。最初のPowerShell loggerがnative出力を捕捉せずrawは0 byte。前後4file SHAは保持。別直接runのツール出力でAPI不一致とText E0282を観測したが、この初回の完全rawとしては扱わない |
| `io-default-captured` | native直接リダイレクトへ修正後のText E0282、exit 101、raw734 byte。コンパイラ診断後の残codegen終端待ち中にText修正が入ったため最終snapshot検証ではない |
| `io-default-final` | Text修正後にCData E0282、exit 101、raw687 byte |
| `io-default-final2` / `actual-limit-diagnostic` | 偽headerで `EntryTooLarge` を期待した誤りにより179成功・1失敗・1ignored。診断runでzip 9の `Io(InvalidData)` を実観測。期待を訂正し、自前上限は別試験へ分けた |
| `deny-all-final` | `cargo deny check` はexit 4。advisories / bans / sourcesはok、licensesだけが新BZip2ライセンス未許可で失敗。単一ID追加後のall-features最終成功と区別 |

`io-default-final3` のOVIKA対象SHAは `A6F038AA01D46B0873F5AC4F32BF2D8252E904D9113C76588C1954341CD94823`。
そのrunのdeny.tomlはライセンス追加前であり、最終all-features denyでは追加後を検証した。
その後、親feedbackにより偽header試験のWhyNot注記1行だけを明確化した。最終OVIKA SHAは `43A8D5D4CE374B170EB051BFF13A473B34C3264E5A3F67F54C1C4E6ACF1DE819`。
`sepika690-comment-clarification-report.txt` に前後SHAと注記のみである照合を保持し、旧runの対象SHAを書き換えない。
最終他4ファイルSHAは `sepika690-coder-final-file-hashes.json` に保持する。
実装担当のsession終端と残processなしの確認は `sepika690-coder-handles-terminal.json`（08:33:04 JST）であり、後続Windows担当の稼働状態を表すものではない。

### 中間Windows検証の終了と採用訂正

中間構成のWindows App defaultは441成功・既存ignored 1・exit 0（`sepika690-win-app-default.log` / `.json`）。
続くMCP/GUI/Parquet構成の `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked` はApp GUIの試験実行中に到達したが、PPMdを除く採用訂正のため中断した。
本人所有のPID treeだけを終了しexit 1で終端を確認した。タイムアウトや試験失敗とは区別し、当該構成全体の成功件数へ数えない。
実行の前後SHA・途中raw・中断理由は `%TEMP%/sepika690-win-features.json` / `.log` / `sepika690-win-features-cancelled.json`、全所有session終端とCargo返却は `sepika690-win-handles-first-phase-terminal.json` に保持する。
この中断時点で横断Clippy等の静的後段は未開始だった。以下の最終構成の検証で上記の旧runやhashを上書きしない。

## 最終構成のローカル検証

旧8feature明示後の元Coder報告は `%TEMP%/sepika690-feature-final-report.txt`、Windows直接利用側の報告は `sepika690-windows-final-report.txt`。
後者は実装担当から独立した直接検証であり、fresh Reviewerの独立判定ではない。
feature補完18証拠のSHAは `sepika690-feature-artifact-hashes.json`、旧/新版の公開manifestと圧縮method実装3原資料のURL/hashは `sepika690-feature-source-index.json` に保持し、照合した。
metadataから必要なzip nodeだけを抽出した `sepika690-feature-resolution-proof.json` は、zip 9.0.5の旧8feature欠落なし・`ppmdEnabled=false`・`ppmdPackagePresent=false`・`lockContainsPpmd=false` を示す。

最終5ファイルSHAは以下のとおり。Windows最終runの前後SHA一致は `sepika690-windows-final-snapshot-proof.json`、最終hash一覧は `sepika690-windows-final-source-hashes.json` に保持する。
旧runのCargo.toml `828E5F...` / Cargo.lock `38D5E4...` と取り違えない。

| ファイル | SHA-256 |
| --- | --- |
| Cargo.toml | `C47AC2926A82E9723D7B182284DB095FBBC6A670D60ADE3E737F97CBBA725900` |
| Cargo.lock | `B92EF36B48F8348488587CE4D06BC399C95B5BAEC9A8BB7B972112105B848F79` |
| deny.toml | `1D7C79C7D93A25E45F6F5EFE7A9B66C5C5FFAED3A0B0330810ADAB33AD8517A0` |
| ovika.rs | `43A8D5D4CE374B170EB051BFF13A473B34C3264E5A3F67F54C1C4E6ACF1DE819` |
| stbridge_brace_content.rs | `07982B9A3FADA0FC6AA1728FD465315CAA5989F19CAA79A8DEB623000E44C599` |

| 最終構成のコマンド・条件 | 結果とrun名 |
| --- | --- |
| `cargo metadata --all-features --format-version 1` | exit 0、`feature-run-metadata-all`。旧8feature有効・PPMd不在 |
| `cargo audit --ignore RUSTSEC-2026-0194 --ignore RUSTSEC-2026-0195` | exit 0、`feature-run-audit`。既存quick-xml例外を維持、zip実脆弱性ignoreなし |
| `cargo deny --all-features check` | exit 0、`feature-run-deny-all-features`。advisories / bans / licenses / sources全てok |
| `cargo test -p sepika-app --locked` | exit 0、`win-app-default-final8`。441成功・既存ignored 1 |
| `cargo test -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --locked` | exit 0、`win-features-final8`。1276成功・既存ignored 3 |
| `cargo clippy -p sepika-mcp -p sepika-app -p sepika-io --all-targets --locked -- -D warnings` | exit 0、`win-clippy-default-final8` |
| 同Clippyに上記3featureを追加 | exit 0、`win-clippy-features-final8` |
| `cargo check -p sepika-app --features sepika-app/gui --locked` | exit 0、`win-gui-check-final8`。ParquetなしGUI |
| `RUSTDOCFLAGS='-D warnings' cargo doc -p sepika-mcp -p sepika-app -p sepika-io --features sepika-mcp/mcp,sepika-app/gui,sepika-io/parquet --no-deps --locked` | exit 0、`win-strictdoc-final8` |
| `cargo run -p xtask --locked -- check-deps` | exit 0、`win-checkdeps-final8`。51 upstream checks |
| `cargo run -p xtask --locked -- check-docs` | exit 0、`win-checkdocs-final8`。summary 123 / links 574 / impl refs 632 |
| `mdbook build` | exit 0、`win-mdbook-final8`。HTML生成成功 |
| `cargo fmt --all -- --check` | exit 0、`win-fmt-final8` |
| `git diff --check` | Windows担当の確定報告で成功。文書担当自身の実行ではない |

Windows実行一覧の `sepika690-windows-artifact-index.json` は15runのcommand・exit・raw/metadataパスとhashを含み、実ファイルとの一致を確認した。
この一覧には中間App構成、途中中断、rustupの初回失敗も含まれ、15run全成功とは数えない。
静的解析前の `rustup update stable` は通常sandboxの `.rustup/tmp` AccessDeniedでexit 1、同コマンドのescalationでexit 0となった。rustc 1.99.1 unchangedとClippy 0.1.99を確認した。
mdBookはrawが示すHTML生成成功までを記録し、版情報は本記録では特定しない。

metadata実コマンドのexit 0と3498385 byte raw/result保全後、巨大1行JSONの末尾を読む補助shell2件が滞留した。
本人のcommandline/PIDを照合して該当pwshだけを停止し、返却session32324の外shell exit -1と、sessionIDを保持できなかった別helperの本人PID終了を区別して確認した。
この補助shellの失敗をmetadataの成功へ取り込まず、Cargoや他担当processの終了とも混同しない。
Windows全所有session終端・残processなし・Cargo返却は `sepika690-windows-handles-terminal.json` に保持した。

最終App/feature構成を新たに実行し、旧IO default195成功/2ignoredは中間構成の証拠として引き継いだ。最終featureのIOも直接実行済みであり、旧default195を最終構成で再走した件数とは扱わない。
各構成の成功件数をunique件数へ合算しない。workspace全体・無関係core/design統合試験は最終CIへ委ねる。

## 未検証と後続ゲート

上記最終構成のローカル検証を、全workspace/全feature組合せや手動native GUI・実験Validationの証拠へ拡張しない。
最終ソースに対するfresh独立レビュー、親監査、最終headの全適用CI、通常merge/main反映も未完了として保持する。
本対応を先にmainへ反映した後、#671 / PR #689へ依存解消を連携し、同PRの最終head CIを改めて確認する必要がある。
今回の依存監査対応を、既存quick-xml ignore・yanked警告の解消や、構造計算の実験Validationの証拠へ拡張しない。
