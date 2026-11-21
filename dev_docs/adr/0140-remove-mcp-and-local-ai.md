# MCP サーバと GUI の外部 AI 接続を廃止する

Status: accepted

## 判断

[Issue #1740](https://github.com/hrntsm/SEPIKA/issues/1740) により、独立 MCP サーバと GUI への中継、接続許可、依頼、受理台帳、成果提出を廃止する。対応する実行ファイル、通信クレート、外部 AI 専用の GUI 操作と保存項目を削除する。

GUI が本案と検討を所有する。断面案の編集・比較・採否には既存の編集と計算を使う。基準案と比較案、保持計算の開始時入力と準備後入力、結果・数量の根拠、採否と理由を引き続き OViKA に保存する。採用時の入力照合と Undo、採用後の再計算も従来の手順を使う。解析・検定や Arrow/Parquet の入出力に必要な下位クレートの責務は維持する。

開発中の保存形式の扱いは [ADR 0014](0014-schema-compatibility.md) に従う。外部 AI 専用の依頼・成果・受理履歴と計算履歴の外部 AI ジョブ識別は保存対象から外す。

## 既存 ADR の適用範囲

過去の ADR が引き続き accepted であっても、MCP や GUI の外部 AI 接続に固有の操作・入出力・表示・保存の契約はすべて失効する。共通契約と通常 GUI・OVIKA に関する判断は引き継ぐ。

- 床板・壁版消失時の GUI 確認と、取り付く床板の共通入力検査・編集診断は引き継ぐ。MCP の確認引数と診断応答は廃止する。[ADR 0020](0020-assign-plates-to-member-bounded-regions.md)、[ADR 0042](0042-attached-slab-validation-and-support-resolution.md)
- 壁の確定応答の記録と GUI の入力照合は引き継ぐ。MCP 固有の入力世代識別は廃止する。[ADR 0045](0045-wall-committed-response.md)
- OVIKA の鉄筋寸法表現は引き継ぐ。MCP の配筋通知と JSON 表現は廃止する。当時は ST-Bridge 取込の配筋通知を引き継ぎ、後に [ADR 0141](0141-remove-st-bridge.md) で廃止した。[ADR 0052](0052-stbridge-rebar-source-and-analysis-boundary.md)、[ADR 0119](0119-standard-rebar-designation.md)。
- 保存モデルの数量設定、GUI と CSV の方向別壁筋小計、比較 GUI と共通の固定鋼材数量根拠は引き継ぐ。MCP の数量設定・小計・根拠の応答は廃止する。[ADR 0057](0057-quantity-settings-and-directional-wall-subtotals.md)、[ADR 0095](0095-fixed-steel-quantity-evidence.md)
- CFT 柱の共通・GUI 検定の座屈長さ条件は引き継ぐ。MCP の軸終局検定の入口は廃止する。[ADR 0113](0113-cft-column-explicit-buckling-length.md)
- SRC 接合部帯筋を登録する GUI のリビジョン照合と、共通検定結果の構面・未算定理由・伝達の別途確認は引き継ぐ。MCP の構面照会証跡、帯筋の登録・更新・解除、検定応答は廃止する。[ADR 0130](0130-src-joint-hoops-by-plane.md)、[ADR 0131](0131-src-joint-allowable-shear.md)

## 理由

GUI で断面案を比較し採否を判断する機能は外部接続から独立している。接続を無効化するだけでは、専用の通信、権限、再送・受理、保存形式の保守が残るため、これらの責務を製品から取り除く。

## 関連

- [ADR 0054](0054-mcp-result-publication-provenance.md) の MCP 成果公開
- [ADR 0087](0087-gui-owned-study-and-local-ai.md) のうち外部 AI の接続・操作
- [ADR 0093](0093-gui-ai-connection-protocol.md) の GUI 接続プロトコル
- [ADR 0094](0094-prepared-ai-study-requests.md) の AI 依頼と保存
- [廃止前の外部 AI 連携マニュアル](https://github.com/hrntsm/SEPIKA/tree/8ae832fa7b87dc61dfd03198a1a4bdbbb2eca0cf/docs/mcp_server)
