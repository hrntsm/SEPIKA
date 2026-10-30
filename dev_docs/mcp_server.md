# MCP サーバの開発者向け仕様

GUI利用の手順・権限・結果の読み方は[外部AI連携](../docs/mcp_server/README.md)を正本とします。本書はMCPクライアント実装者向けの起動方式、要求・応答、独立サーバのツールと保存契約です。GUIと独立サーバのモデル・権限・結果保存は別です。

## 構成と起動方式

`sepika-mcp` はstdioのMCPサーバです。MCPプロトコル層（`rmcp`/`tokio`依存）は非デフォルトのCargo feature `mcp`（`default = []`）配下です。featureなしでは起動バイナリ・ツールをビルドしません。

| 起動方式 | 状態の所有者 | 公開ツール | 保存 |
| --- | --- | --- | --- |
| `--gui-pipe` | 開いているGUIの許可済み検討 | `gui_request` | GUIの検討・保持計算・受理台帳をOVIKAへ保存。接続許可は保存しない |
| ファイル指定／引数なし | 独立サーバのモデル | 以下の6ツール | 対応する解析明細を独立結果ストアへ保存。モデル保存ツールはない |

独立起動はOVIKAからモデルだけを読み込み、GUIの検討・接続許可・保持結果を引き継ぎません。GUIのOVIKA保存条件は[保存形式](../docs/model_io/01_プロジェクト形式_ovika.md)を参照してください。

起動の分岐は[main.rs](../crates/sepika-mcp/src/main.rs)、クレート構成は[architecture.md](architecture.md)です。

## ビルドと独立起動

```bash
# ビルドのみ（バイナリは target/debug/sepika-mcp に生成される。
# --release 付きなら target/release/sepika-mcp）
cargo build -p sepika-mcp --features mcp

# モデルファイルを指定して起動
cargo run -p sepika-mcp --features mcp -- model.ovika

# 引数を省略すると空モデルで起動する
cargo run -p sepika-mcp --features mcp
```

- 起動時の第 1 引数がモデルの読み込み元になり、省略時は空モデルで起動します。
- 解析結果ストアのディレクトリは環境変数 `SEPIKA_RESULT_DIR` で指定でき、未設定の場合は OS 一時ディレクトリ配下の `sepika-mcp-results` を使います。

```bash
SEPIKA_RESULT_DIR=/path/to/results cargo run -p sepika-mcp --features mcp -- model.ovika
```

> **注意**: stdout は MCP の JSON-RPC トランスポートそのものです。ログや診断出力を stdout に書くと、クライアントは壊れたフレームとして接続を切断します。

よくあるエラー:

| 状況 | 結果 |
|---|---|
| `--features mcp` を `sepika-mcp` 以外のクレート（例: `-p sepika-app`）に付けて実行する | そのクレートに `mcp` という機能フラグがないためビルドエラーになります |
| `sepika-mcp` を `--features mcp` なしでビルド/実行しようとする | 起動バイナリ自体が `required-features = ["mcp"]` でゲートされているため、バイナリが存在せず実行できません |

モデル入力の形式と制約は[独立起動のモデル入力制約](#独立起動のモデル入力制約)を参照してください。

## 独立起動クライアント設定

### Claude Code

```bash
claude mcp add sepika -- /path/to/sepika-mcp model.ovika
```

もしくはプロジェクトの `.mcp.json` に登録します。

```json
{
  "mcpServers": {
    "sepika": {
      "command": "/path/to/sepika-mcp",
      "args": ["model.ovika"]
    }
  }
}
```

### Claude Desktop

`claude_desktop_config.json` に以下のエントリを追加します。

```json
{
  "mcpServers": {
    "sepika": {
      "command": "/path/to/sepika-mcp",
      "args": ["model.ovika"],
      "env": {
        "SEPIKA_RESULT_DIR": "/path/to/results"
      }
    }
  }
}
```

## GUI接続の要求と応答

MCPツール引数は `{"envelope": {...}}` です。`Request` は `version: u32`、`id: String`、`operation: String`、任意の `session` / `project` / `study` / `request` / `proposal`（文字列）、任意の `input`（JSON）、`arguments`（省略時null）を持ちます。未知のトップレベルキーを拒否します。応答は `id`、`result`、`error` を持ち、成功時は `error: null`、拒否時は `error: {"code": ..., "message": ...}` です。

GUIとのパイプ通信は改行を含め最大1MiBのUTF-8 JSON行です。途中EOF、不正JSON・UTF-8、上限超過を拒否します。stdioのMCP JSON-RPC外枠とパイプ内の要求型を区別してください。

GUI接続で公開するツールは`gui_request`だけです。`envelope`に版1、要求ID、操作を指定します。最初は`{"version":1,"id":"接続確認1","operation":"connect"}`を渡し、GUI承認後に再度接続確認して接続セッション、プロジェクト、検討、案と入力版を取得します。その後の要求には、それらと依頼IDを明示します。依頼開始は`begin_request`、成果提出は`submit`です。

| 操作 | 内容 |
| --- | --- |
| `study` / `conditions` | 検討、固定対象、基準・比較案の版、解析・検定条件 |
| `proposals` / `fixed_targets` / `decisions` | 案一覧、固定対象参照、採否概要のページ取得 |
| `members` / `nodes` / `materials` / `load_cases` / `combinations` / `sections` | 許可範囲の入力と有効候補断面のページ取得 |
| `create_comparison` | 基準案から比較案を作成。案名を指定 |
| `assign_section` | 明示した固定対象参照の列へ既存有効断面を一括割当 |
| `calculate` / `job` | 固定条件で既存計算を一件実行し、状態を照会 |
| `changes` / `results` / `quantity` | 断面差、保持計算の数値と図表参照、固定対象の数量 |
| `runs` | 指定案の保持計算の日時・開始/準備後入力版・実行時状態・条件をページ取得 |
| `evidence` | 結果参照に対応する計算時入力・詳細検定のページ取得 |
| `receipt` | 受理済み要求IDと受理内容・結果の照会 |
| `submit` | 対象入力版、結果参照、説明、未確認事項を提出 |

ページは開始位置`offset`と件数`limit`（1〜100）を指定します。全件数、現在位置、省略の有無、次の開始位置が返ります。結果は保存計算の位置を`run_index`で明示します。根拠には`reference`、表の種類`table`、ページを指定します。表は`nodes`、`members`、`sections`、`materials`、`load_cases`、`combinations`、`design_details`、`joint_details`、`wall_details`です。最大1MiBのUTF-8 JSON行で通信し、大きな応答はページを狭めて取得します。

提出の`explanation`は説明、`unchecked`は未確認事項の列、`references`は結果参照の列です。参照の存在と計算時入力・結果の有効性はSEPIKAが照合します。説明だけの提出では参照列を空にできます。成果提出、計算成功、検定結果、人による採用は別であり、説明に「完了」「合格」と書いても計算・検定状態は変わりません。

計算結果を添える提出は`run_index`を一つ明示し、結果参照をすべてその保持計算にそろえます。提出の状態と検定集計は選んだ保持計算から取得し、同入力の後続ジョブで置き換えません。応答の`source: "run"`が保持計算の状態を示します。`run_index`を省略した場合は説明・数量だけの提出となり、結果参照は指定できません。同依頼・同入力のジョブがあれば`source: "job"`と`job_id`で提出時の観測状態を記録し、なければ`source: "none"`です。run未指定の提出へ保持結果や検定集計を自動で補完しません。現在のジョブ状態は別途`job`で照会します。

数量だけの確認では、`quantity`で取得した固定対象の根拠識別を任意の`quantity_references`列へ指定して提出できます。SEPIKAが同じ入力版・固定対象の既存数量根拠を照合して保存します。

保存後に新しく接続した確認のみのクライアントも、`proposals`で対象案の現在入力版を取得し、`runs`へその案・入力版とページを指定して保持計算を選べます。応答の`run_index`と開始入力版`input`を`results`へ渡し、返った参照を`evidence`へ渡してください。準備後入力版`prepared_input`も明示されます。保存旧版の閲覧許可がなければ現在入力に対応する計算だけを全件数に含め、許可があれば当該案の保存旧版も含めます。`history_included`で一覧の範囲を示します。表示する`recorded_calculation_state`は保持計算の実行時状態であり、現在の数値利用可否・検定合格・採用を意味しません。

### 受理・権限の照合

応答を失った場合は受理結果を推定せず、GUIで再有効化して新しい接続先へ接続し、新規承認後に要求IDを照会してください。応答IDが要求と一致しない場合も、成功扱いせず接続を終了します。同じ受理済みID・同じ内容の再送は元の受理結果を返し、同じIDの内容違いは拒否します。成果と受理台帳は検討とともに保存されます。接続許可は保存されません。

承認後の検討操作は参照や状態照会も要求・応答を保存するため、数値ページの取得回数と応答量に応じて検討を含むOVIKAの容量が増えます。台帳は自動で間引きません。同じ要求IDの状態照会を再送すると最初の受理結果を返すため、最新状態の確認には新しい要求IDを使ってください。

保存した応答の再送・受理照会でも現在の閲覧許可と元の計算根拠を確認します。以前に旧版閲覧を許可していても、新しい接続で許可しなければ旧版の保存応答を返しません。元の計算根拠が失効した場合も、当時の応答を利用可能な結果として返しません。確認のみ接続から変更要求を再送することもできません。過去の変更が受理されたかは明示的に受理照会してください。

明示した保存入力から作った依頼は、対象の入力版の一致より先に現在の旧版閲覧許可を確認し、許可がなければ `permission_denied` を返します。開始・参照・同ID再送・受理照会でこの順序は共通です。許可があることは、対象版の不一致を許容したり、新しい開始・編集を認めたりすることを意味しません。

提出に表示する計算・検定状態は提出時の根拠へ結び付き、後から別の計算結果で補完しません。結果参照を付けなかった説明にもこの規則を適用します。数量だけの提出に保存した固定数量根拠も、その元の案を削除できない保持根拠として扱います。

保持結果の状態は明示した保持計算のものであり、同じ入力で別のジョブが実行中・確認待ちになっても変わりません。計算結果を提出する際も、同じ一つの保持計算を明示してください。runを選ばない説明のジョブ状態は提出時の観測で、現在のジョブ状態や計算結果の有効性とは別です。観測後にジョブが完了しても、過去の説明へrunや検定集計を付け足しません。

必要な検定根拠が取得不能な場合、全範囲の検定件数は取得不能を示し、0件へ置き換えません。取得できた有効な根拠の件数は「部分集計」と範囲・数える単位を併記します。評価行は部材×位置×ケースや接合部・壁の既存検定行を数え、部材の本数とは区別します。取得できた根拠が一つもなければ部分件数も取得不能です。確認済みNG、有効範囲の最大、未評価は併存します。

保持計算の一覧も旧版閲覧許可に従います。保存旧版を含む一覧の保存応答は、旧版を許可しない接続では再送・受理照会できません。一覧にある過去の計算済みという記録は、その結果が現在の契約で利用可能であることを保証せず、数値・根拠の取得時に通常の有効性を照合します。

実装入口：[通信型](../crates/sepika-ai-connection/src/protocol.rs)、[GUI要求の受付](../crates/sepika-app/src/app/study/ai/operations.rs)。共有範囲と人の承認は[製品の制約](../docs/mcp_server/05_制約・注意点.md)を正本とします。

## 独立起動ツール契約

公開されている MCP ツールは以下の 6 個です。

### `model_query`

原階の属性と明示所属は `kind: "source_stories"`、原階入力の不整合と解析用推定との差は
`kind: "source_story_diagnostics"` で確認できます。診断文は GUI と共通です。

節点・部材・断面・壁版・床板・床領域・壁領域・二次部材を検索します。

| 引数 | 型 | 必須/任意 | 意味 |
|---|---|---|---|
| `kind` | `String` | 必須 | 下表の種別・別名。未知の値は対応種別を示す入力エラー（`invalid_params`） |
| `filter` | `Option<String>` | 任意 | 各アイテムを JSON 文字列化した内容に対する部分一致フィルタ |

| 種別・別名 | 対象 |
|---|---|
| `node` / `nodes` | 節点 |
| `member` / `members` / `element` / `elements` | 部材 |
| `section` / `sections` | 断面 |
| `wall_plate` / `wall_plates` | 壁版 |
| `slab` / `slabs` | 床板 |
| `floor_region` / `floor_regions` | 床領域 |
| `wall_region` / `wall_regions` | 壁領域 |
| `floor_assignment_region` / `floor_assignment_regions` | 床板割当領域 |
| `wall_assignment_region` / `wall_assignment_regions` | 壁版割当領域 |
| `secondary_member_end` / `secondary_member_ends` | 支持先を持つ小梁・間柱 |
| `unassigned_beam` / `unassigned_beams` | 未割当の小梁 |
| `unassigned_post` / `unassigned_posts` | 未割当の間柱 |
| `secondary_beam` / `secondary_beams` | 全小梁 |
| `source_stories` | 原階の入力属性と明示所属 |
| `source_story_diagnostics` | 原階入力・解析用所属の診断 |
| `stb_strengths` | STB強度入力（1項目） |
| `resolved_strengths` | 部材ごとの強度解決結果 |
| `strength_diagnostics` | 強度入力の診断 |
| `rebar_notices` | 鉄筋入力の非阻害通知 |

既知の種別で対象がない場合や、フィルタに一致しない場合は正常な `items: []` です。
未完成のモデルも照会できます。

返り値:

```json
{ "items": [ /* 検索結果の JSON オブジェクト配列 */ ] }
```

- `node`: `{ "id", "coord", "story" }`
- `member`/`element`: `{ "id", "kind", "nodes", "section", "material" }`。部材付帯情報（ハンチ・継手位置）があれば `haunch_i`/`haunch_j`（`length`/`depth_increase`/`width_increase`）と `joints`（`distance`/`kind`）を追加。入力モデルに保存されている部材だけを返す（壁版から都度生成する耐震壁は含まない。壁は `kind=wall_plate` で照会する）
- `section`: `{ "id", "name", "area", "iy", "iz" }`
- `wall_plate`: `{ "id", "shape", "section", "opening_area", "opening_weight", "slit", "openings", "loads", "becomes_element" }`。`slit` は `{ "column_face": [bool, bool], "beam_face": [bool, bool] }`（耐震スリット。梁際は 0 が下辺、1 が上辺）、`loads` は仕上げ・増打ちの面荷重 [N/mm²]（躯体の自重は含まない）、`becomes_element` はその壁版から壁エレメントが生成されるか。`shape.kind` は `"Enclosed"`（`boundary`）または `"Attached"`（`anchor`・`extent`・`resolved_extent`）。`extent` は入力値そのもので `null` は階高いっぱいを表し、`resolved_extent` は階レベルから解決した高さ（`null` は直上に階が無く解決できない。解析前チェックが止める）
- `slab`: `{ "id", "shape", "section", "loads", "usage", "method", "one_way" }`。`shape.kind` は `"Enclosed"`（`boundary`）または `"Attached"`（`anchor`・`extent`）
- `floor_region`: `{ "id", "name", "boundary", "slab_ids", "secondary_beams" }`。`secondary_beams` は所属小梁の実体配列
- `wall_region`: `{ "id", "name", "boundary", "wall_plate_ids", "posts" }`。`posts` は所属間柱の実体配列
- `unassigned_beam` / `unassigned_post`: どの領域にも入らなかった二次部材の配列
- `secondary_beam`: 領域内と未割当を合わせた小梁の実体配列

囲まれた壁版・床板の `shape.boundary` は解決できた節点ID列です。
割当領域がない、支持部材を解決できない、境界頂点に一致する節点がない場合は `null` です。
空の割当境界も未解決です。未解決を空の節点列として表示しません。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::model_query`（`crates/sepika-mcp/src/server.rs`）で、検索本体は `sepika_mcp::query::query_model`（`crates/sepika-mcp/src/query.rs`）が担います。

### `model_edit`

#### 階・強度入力

| `command` | 必須引数 | 契約 |
| --- | --- | --- |
| `SetStoryLevelKind` | `story`, `level_kind` | 明示階種別を設定。例は下記 |
| `SetSourceStoryNodes` | `source_story`, `nodes` | 元STB階IDと内部節点IDの配列 |
| `SetStbStrengths` | `input` | 共有のSTB強度入力。構造は `sepika_core::model::StbStrengthInput` |
| `SetSourceStoryConcreteStrength` | `source_story`, `strength` | 元STB階IDと強度文字列。`strength: null` は未指定 |

強度入力の構造は[公開型](../crates/sepika-core/src/model/strength.rs)、JSONからの解決は[編集入口](../crates/sepika-mcp/src/edit.rs)を参照してください。

明示階種別は `SetStoryLevelKind`（例: `{"command":"SetStoryLevelKind","story":1,"level_kind":{"Penthouse":{"k":0.7}}}`）でGUIと同じ編集を行います。取り込み原階kindは保持し、native由来の原階kindだけを更新します。

原階の明示所属は `{"command":"SetSourceStoryNodes","source_story":51,"nodes":[0,1]}` で変更します。
`source_story` は元 STB 階 ID、`nodes` は GUI と同じ内部節点 ID の配列です。
未知節点・生成代表節点・重複 ID は適用を拒否し、undo 履歴を増やしません。

モデルを編集します。GUI と同じ `EditCommand` と Undo 履歴を使います。引数は `command` キーで種別を指定する JSON オブジェクトです（トップレベルに `command` を置く）。`{ "body": { "command": "...", ... } }` という入れ子も受け付けます。

壁版・床板・領域・断面・二次部材の ID は `0`〜`4294967295` の JSON 整数で指定します。
負数、小数、範囲外、文字列、真偽値、配列、オブジェクトを指定すると、対象項目と理由を付けて拒否します。
必須 ID の省略・`null` も拒否します。任意の断面 ID は省略・`null` が未割当で、不正な指定値は未割当として扱いません。
拒否時はモデル・割当・Undo 履歴を変更しません。有効範囲内でも、対象 ID の存在や各操作の適用条件は別途確認します。
たとえば `{"body":{"command":"DeleteSlab","id":4294967296}}` は床板0を削除せずエラーになります。
MCP のツール引数はこの `body` 形式で渡します。

返り値:

```json
{ "op_id": "op-1", "applied": true, "undoable": true, "summary": "..." }
```

- `applied` が `false` のときは、参照検証などでコマンドが Noop になった（モデルは変わらない）
- 節点・部材・荷重・断面そのものの追加削除など、下表にない `EditCommand` は未対応（エラー）

#### 断面形状と数量設定（2）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `EditSectionShape` | `section`（断面ID）、`new_shape`（断面形状） | `frame_use`（線材用途、省略時は`null`。壁・床断面は`null`） |
| `SetQuantityCfg` | `cfg`（共有数量設定の全5項目） | — |

RC壁の方向比は`EditSectionShape`で共有断面の`pwh_ratio`・`pwv_ratio`を編集します。
値は小数比で、`null`は未入力、数量上の明示0は`0.0`です。
数量設定の5項目・単位・既定値は[係数設定とCSV出力](../docs/calc_basis/10_数量積算/01_数量積算の算定式.md#係数設定とcsv出力)を参照してください。
`SetQuantityCfg`は解析結果を失効させず、設定値を保存モデルへ反映します。

```json
{"body":{"command":"SetQuantityCfg","cfg":{"anchorage_dia_factor":40.0,"assumed_wall_bar_dia":13.0,"beam_main_ratio":0.008,"beam_stirrup_ratio":0.001,"slab_rebar_ratio":0.01}}}
```

#### 壁版・間柱の自重負担率

囲まれた壁版は任意の節点境界からは作れません。柱・梁・間柱から導出した壁版割当領域
（準備計算が作る）へ `AssignWallPlateToRegion` で割り当てます。四辺の対称性から
割当領域の境界がそのまま壁版の境界になります。

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `AssignWallPlateToRegion` | `region`（壁版割当領域 ID） | `section`（既定 `null`）、`opening_area`（既定 `0`）、`opening_weight`（既定 `0`）。領域が既に版ありなら `applied: false` |
| `SetWallPlateRegionNoPlate` | `region` | 既存の版があれば削除する（同一 Undo 単位） |
| `UnsetWallPlateRegion` | `region` | 既存の版があれば削除する（同一 Undo 単位） |
| `AddAttachedWallPlate` | `anchor` | `extent`（2 要素 [mm]。省略または `null` で階高いっぱい。自立壁のみ）、`section`（既定 `null`）、`self_weight_shares`（境界辺順の負担率。非負・有限・総和 1、既定 `[]` は未指定）、`opening_area`（既定 `0`）、`opening_weight`（既定 `0`） |
| `DeleteWallPlate` | `id` | — |
| `SetWallPlateSection` | `id` | `section`（`null` で未割当） |
| `SetPostGravityEndShares` | `member`（間柱の安定 ID） | `shares`（端の並び順の負担率2値、非負・有限・総和1。省略・nullは未指定） |
| `SetWallPlateAttrs` | `id` | `self_weight_shares`（境界辺順の負担率。非負・有限・総和 1、既定 `[]` は未指定）、`opening_area`（既定 `0`）、`opening_weight`（既定 `0`）、`openings`（既定 `[]`）、`loads`（仕上げ・増打ちの面荷重。既定 `[]`）、`slit`（`{ "column_face": [bool, bool], "beam_face": [bool, bool] }`。片方のキーだけでも可。既定は 4 辺とも `false`） |
| `SetAttachedWallPlateExtent` | `id` | `extent`（省略または `null` で階高いっぱい。自立壁のみ） |
| `SetAttachedWallPlateAnchor` | `id`、`anchor` | — |

壁版割当領域への割当の例:

```json
{
  "command": "AssignWallPlateToRegion",
  "region": 0,
  "section": null,
  "opening_area": 0.0,
  "opening_weight": 0.0
}
```

取付き先 `anchor` が線のときの形（`nodes` は取付き梁の 2 節点、`span` は梁全長に対する無次元区間）:

```json
{
  "Line": { "nodes": [0, 1], "span": [0.0, 1.0], "transfer": "Anchor" }
}
```

#### 床板（10）

囲まれた床板は任意の節点境界からは作れません。大梁・小梁から導出した床板割当領域
（`floor_assignment_region`）へ割り当てます。割当・版なし・未設定は同一 Undo 単位です。

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `AssignSlabToFloorPlateRegion` | `region`（床板割当領域 ID） | 版仕様はフラット引数（`section`/`loads`/`usage`/`method`/`one_way`）か `plate` オブジェクト。`method` 未指定は `"TriTrapezoid"`。`loads` 未指定は `[]`。領域が既に版ありなら `applied: false` |
| `SetFloorPlateRegionNoPlate` | `region` | 既存の版があれば削除する（同一 Undo 単位） |
| `UnsetFloorPlateRegion` | `region` | 既存の版があれば削除する（同一 Undo 単位） |
| `AddAttachedSlab` | `anchor`、`extent` | 版仕様はフラット引数（`section`/`loads`/`usage`/`method`/`one_way`）か `plate` オブジェクト。`method` 未指定は `"TriTrapezoid"`。`loads` 未指定は `[]` |
| `DeleteSlab` | `id` | — |
| `SetSlabSection` | `id` | `section`（`null` で未割当） |
| `SetSlabUsage` | `id` | `usage`（`null` で積載荷重なし） |
| `SetSlabOneWay` | `id` | `one_way`（`null` で境界辺 0・2 が負担） |
| `SetAttachedExtent` | `id`、`extent` | — |
| `SetAttachedAnchor` | `id`、`anchor` | — |

`AddAttachedSlab` のフラット引数の例:

```json
{
  "command": "AddAttachedSlab",
  "anchor": { "Line": { "nodes": [0, 1], "span": [0.0, 1.0], "transfer": "Anchor" } },
  "extent": [1000.0, 1000.0],
  "section": 0
}
```

#### 床領域（2）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `SetFloorRegionName` | `id`、`name` | — |
| `SetFloorRegionSecondaryBeams` | `floor_region`（別名 `id`）、`secondary_beams`（`SecondaryMember` の配列。空にするときは `[]`。空にすると旧所属は未割当へ移る） | — |

`secondary_beams` を省略するとエラーになります（欠落を空配列とみなしてリストを消すことはしません）。旧小梁語彙のコマンドやキーは受け付けずエラーになります。

#### 未割当二次部材（4）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `AddUnassignedBeam` | `beam`（`SecondaryMember`） | — |
| `DeleteUnassignedBeam` | `index` | — |
| `AddUnassignedPost` | `post`（`SecondaryMember`） | — |
| `DeleteUnassignedPost` | `index` | — |

同じ端点の小梁・間柱が既にあるときは適用されず `applied: false` になります。`SecondaryMember` は `id`（安定 ID）・`kind`・`ends`（支持部材アンカー／片持ち自由端）・`section`・`name` を持ちます。端部支持条件は `SetSecondaryMemberEndSupport` で設定できます。

#### 二次部材の端部支持条件（1）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `SetSecondaryMemberEndSupport` | `member`（二次部材の安定 ID）、`end_support`（`"Supported"` / `"Free"` の 2 要素配列） | — |

安定 ID で対象の小梁・間柱を探し、床領域内・壁領域内・未割当のいずれにあっても設定します。支持端は現在の端部座標から支持部材アンカーへ再解決し、片持ち小梁は自由端の位置を `"Free"` にします（例: `"end_support": ["Supported", "Free"]`）。同じ条件のときは適用されず `applied: false` になります。

#### 二次部材の配置・移動・削除（3）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `PlaceSecondaryMember` | `parent`（`"floor"` / `"wall"` / `"unassigned"`）、`kind`（`"Beam"` / `"Post"`）、`ends`（取付き位置表現） | `region`（`parent` が `"floor"` / `"wall"` のとき必須。対象の床領域／壁領域 ID）、`section`（既定は未割当）、`name`（既定は空） |
| `SetSecondaryMemberEnds` | `member`（二次部材の安定 ID）、`ends`（取付き位置表現） | — |
| `DeleteSecondaryMember` | `member`（二次部材の安定 ID） | — |

`PlaceSecondaryMember` は安定 ID を新規に払い出し、親領域（`floor` なら小梁、`wall` なら間柱、`unassigned` なら未割当）へ追加したうえで床板・壁版割当領域を再構築します。`ends` は `{"Supported": [{"support": {"Primary": 0}, "position": 0.5}, ...]}` のように支持部材アンカーで指定します。支持が決まらない端（`Detached`）、種別に合わない親、支持端が親領域（`SecondaryParent` が指す床領域・壁領域）の内側または境界上（許容差 10 mm）にない場合は適用されず `applied: false` になります（片持ちへの読み替えはしません）。`SetSecondaryMemberEnds` も同じ親領域内判定を行い、移動先が現在の所属領域の外へ出る場合は適用されません。

配置・移動・削除で旧版を失う場合は、まず除去する版 ID・件数・入力面荷重・室用途・開口重量と、1 Undo で復元できることをエラー応答で知らせます。モデル・ID の払い出し・Undo/Redo 履歴は未更新です。内容を確認した後、同じコマンドへ `"confirm_plate_loss": true` を追加して再送してください（既定は確認なし）。確認後は旧版を取り除き、新しい割当領域を未設定にします。成功応答の `summary` に除去対象を記載し、版・入力荷重の除去まで含めて 1 つの Undo 単位とします。`ends` の解析に失敗した場合は `ends` の解析エラーを返します。

版の割当は支持部材の種類・ID・厳密な材軸区間が同じ領域だけへ保持します。重心・面積・親名が同じ別境界へ転用しません。境界キーの衝突や同じ版の重複割当は確認フラグで許可できず、理由を返してモデル・履歴を更新しません。解析準備で予期しない孤立版が生じる場合も、入力荷重と版 ID を示して候補の確定を停止します。

#### 壁領域（1）

| `command` | 必須 | 任意と既定 |
|---|---|---|
| `SetWallRegionPosts` | `wall_region`（別名 `id`）、`posts`（`SecondaryMember` の配列。空にするときは `[]`） | — |

`posts` を省略するとエラーになります。壁領域の新規作成・削除は公開していません（準備計算が壁領域を作り直すため）。

床領域の新規作成・削除は公開していません（準備計算が床領域を作り直すため）。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::model_edit`（`crates/sepika-mcp/src/server.rs`）で、コマンドの解析と GUI と同じ Undo 経路での適用は `sepika_mcp::edit::{parse_edit_command, apply_edit}`（`crates/sepika-mcp/src/edit.rs`）が担います。

### `quantity_takeoff`

数量積算（コンクリート体積・型枠面積・鉄筋/鉄骨重量の概算）を集計します。

| 引数 | 型 | 必須/任意 | 意味 | 既定値 |
|---|---|---|---|---|
| `group_by` | `Option<String>` | 任意 | `"category"`（部位別）/`"story"`/`"stories"`（階別）/`"structure"`（構造別）/`"steel"`（鉄骨種類別）/`"rebar"`（鉄筋径別）/`"detail"`/`"items"`（明細） | `"category"` |

`group_by` の未指定・空文字列は `category` です。明示した未知の文字列は
入力エラー（`invalid_params`）となり、数量を計算できないモデルでも集計軸の誤指定を先に通知します。

返り値:

```json
{ "status": "complete", "rows": [ /* group_by に応じた行 */ ], "totals": { "concrete_m3": .., "formwork_m2": .., "rebar_t": .., "rebar_status": "complete", "rebar_issues": [], "steel_t": .., "rebar_joints": .. }, "notes": [ /* 注記 */ ] }
```

`rows` の形は `group_by` により異なります。

| `group_by` | 行の形 |
|---|---|
| `category`（既定） | `{ category, concrete_m3, formwork_m2, rebar_t, steel_t, rebar_joints }` |
| `story` | `{ story, concrete_m3, formwork_m2, rebar_t, steel_t, rebar_joints }` |
| `structure` | `{ structure, concrete_m3, formwork_m2, rebar_t, steel_t, rebar_joints }` |
| `steel` | `{ section, length_m, weight_t }` |
| `rebar` | `{ dia_mm, length_m, weight_t }` |
| `detail` | `{ elem, slab, wall_plate, secondary_member, section, label, story, category, structure, concrete_m3, formwork_m2, rebar_t, steel_t, rebar_joints, rebar_status, rebar_issues, rebar }` |

部位別・階別・構造別と全体には`rebar_status`・`rebar_issues`を付します。
状態は`empty`（対象なし）、`complete`（算定済み概算）、`incomplete`（未算定を伴う小計）です。
`complete`は実配筋数量との照合済みを意味しません。`rebar_issues`には
`elem`・`wall_plate`・`section`・`usage`・`kind`・`reason`を返し、欠落／不正／未実装を区別します。
明細の`rebar`は方向・径・総延長・重量・状態の算定済み行です。未入力方向は0の行にせず理由を返します。
鉄筋径別の行は算定済み分の小計で、全体の状態・理由も確認してください。
数量全体の算定が不成立の場合は`status="unavailable"`、`totals=null`と理由を返します。

`detail`（別名`items`）では、`secondary_member`が二次部材IDの整数、他の対象では`null`です。
二次部材を`elem`・`slab`のIDで代用しません。端点にモデル節点がなくても解決実長で集計します。
実材への集約条件を満たす明細は、実材の`elem`を保持し、`secondary_member=null`です。
階だけが未確定の場合は、ID付きの注記と`story="-"`の行を残し、数量を全体・各小計へ含めます。
鉄骨種類別の集計は鋼材だけを対象とします。
断面・材料・端点等の不足で数量が決まらない場合は、対象IDと原因付きの`unavailable`です。
階の確定条件と実材との重複防止は[二次部材の数量](../docs/calc_basis/10_数量積算/01_数量積算の算定式.md#二次部材の実長階明細)を参照してください。

集計にはモデルに保存された共有数量設定を用います。`model_edit`の`SetQuantityCfg`で
数量設定を編集でき、Undo／Redo・保存後の再読込も同じ設定を使用します。

数量の算定式は[数量積算](../docs/calc_basis/10_数量積算/README.md)を参照してください。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::quantity_takeoff`（`crates/sepika-mcp/src/server.rs`）で、集計本体は `sepika_mcp::query::quantity_takeoff_json`（`crates/sepika-mcp/src/query.rs`）が担います。

### `analysis_run`

解析を非同期で実行し、ジョブを登録して `job_id` を即座に返します（計算完了は待ちません）。

| 引数 | 型 | 必須/任意 | 意味 | 既定値 |
|---|---|---|---|---|
| `kind` | `JobKind` | 必須 | `LinearStatic`/`Eigen`/`Pushover`/`TimeHistory`/`DesignCheck`/`UltimateCheck`（質点系は含まれません） | — |
| `load_case` | `Option<u32>` | 任意 | LinearStatic/DesignCheck/UltimateCheck: 対象荷重ケース ID | 先頭の荷重ケース |
| `load_combination` | `Option<usize>` | 任意 | DesignCheck専用: 保存組合せindex。load_caseとの同時指定・不正indexは拒否 | 未指定 |
| `n_modes` | `Option<usize>` | 任意 | Eigen: モード数 | 3 |
| `dir` | `Option<String>` | 任意 | Pushover/TimeHistory: 加力・入力方向。`"X"`/`"Y"` 以外はエラー | `"X"` |
| `steps` | `Option<usize>` | 任意 | Pushover: 最大ステップ数 | 50 |
| `max_disp` | `Option<f64>` | 任意 | Pushover: 目標変位 [mm]。未指定なら目標層間変形角のみで判定 | なし |
| `max_drift_denom` | `Option<f64>` | 任意 | Pushover: 目標最大層間変形角の分母 n（角度 1/n）。`max_disp` と両方指定した場合は早く達した方で終了 | 150（`max_disp` 未指定時） |
| `dt` | `Option<f64>` | 任意 | TimeHistory: サンプル波の時間刻み [s] | 0.01 |
| `duration` | `Option<f64>` | 任意 | TimeHistory: サンプル波の継続時間 [s] | 2.0 |
| `period` | `Option<f64>` | 任意 | TimeHistory: サンプル波の周期 [s] | 0.5 |
| `amp` | `Option<f64>` | 任意 | TimeHistory: サンプル波の振幅 [mm/s²] | 1000 |
| `z` | `Option<f64>` | 任意 | 荷重自動同期: 地域係数 Z（令88条） | 1.0 |
| `soil` | `Option<String>` | 任意 | 荷重自動同期: 地盤種別。`"I"`/`"II"`/`"III"` 以外はエラー | `"II"` |
| `c0` | `Option<f64>` | 任意 | 荷重自動同期: 標準せん断力係数 C0 | 0.2 |
| `ai_mode` | `Option<String>` | 任意 | 荷重自動同期: Ai 算定法。`"Approx"`/`"SemiPrecise"` 以外はエラー | `"Approx"` |
| `design_period` | `Option<f64>` | 任意 | 荷重自動同期: 参考の固有周期 T [s]。数値だけは告示精算の証拠にならず、`SemiPrecise` の標準EX/EYは未対応 | なし |

返り値:

```json
{ "job_id": "job-0" }
```

`kind` ごとに使用するパラメータと概略処理:

| `kind` | 使用パラメータ | 処理概要 |
|---|---|---|
| `LinearStatic` | `load_case`, `z`, `soil`, `c0`, `ai_mode`, `design_period` | 指定/先頭の荷重ケースで線形静解析（解析前に荷重自動同期） |
| `Eigen` | `n_modes`, `z`, `soil`, `c0`, `ai_mode`, `design_period` | 固有値解析（周期・刺激係数・有効質量。解析前に荷重自動同期） |
| `Pushover` | `dir`, `steps`, `max_disp`, `max_drift_denom`（解析前同期: `z`, `soil`, `c0`, `ai_mode`, `design_period`） | 増分解析（プッシュオーバー解析）。水平載荷の Ai 分布は solver 側で略算 T・Z=1・地盤 II・C0=0.2 固定。地震引数は階重量・EX/EY の同期にのみ効く。結果ストアには書かずサマリ JSON のみを返す |
| `TimeHistory` | `dir`, `dt`, `duration`, `period`, `amp`, `z`, `soil`, `c0`, `ai_mode`, `design_period` | サンプル波（`amp * sin(ωt) * e^{-0.3t}`）による時刻歴応答解析。減衰は剛性比例減衰（h=0.02、1 次固有振動数使用）固定（解析前に荷重自動同期）。結果ストアには書かずサマリ JSON のみを返す |
| `DesignCheck` | `load_case` または `load_combination`, `z`, `soil`, `c0`, `ai_mode`, `design_period` | 保存組合せはGUIと同じterms/kind・継続時間・線形内力で検定。単独ケースは種別による状態を表示し、雪/風単体は組合せ未検定。地震単独指定は架構用G+Pを自動合成し、必要重力の一部/全部失敗で拒否。積雪ケースがあれば保存組合せの明示が必要。結果のload_targetがsingle_case/automatic_gravity_combination/saved_combination、terms、状態と理由を区別。法的全条件確認を意味しない。付着はRc1999、QDは既定方式。検定結果はサマリJSON。単独ケース内力だけ結果ストアへ保存し、組合せ内力は下記サマリから取得 |
| `UltimateCheck` | `load_case`, `z`, `soil`, `c0`, `ai_mode`, `design_period` | RC 部材の終局せん断・付着・軸余裕度、CFT 柱の軸終局検定（靭性保証型耐震設計指針）。単独ケースの指定と未指定時の先頭ケース採用には、架構用長期総組合せの完全性に関する未解決事項があり、終局需要の専用受入は未完了。[共通の需要モード・不足条件](../docs/calc_basis/08_部材終局耐力/07_終局検定の統括.md)を確認。結果ストアには書かずサマリ JSON のみを返す |

解析前に、現在のモデルから既存階の地震用重量・剛床重量・物理質量と自動荷重を同期します。
密度・仕上げ・地震用積載などの編集後も、準備ボタンを経由せず最新重量を使用します。
重量・周期・係数の生成が失敗すると旧 Auto EX/EY を両方向とも除去し、通常の手入力荷重は保持します。
失敗した旧派生質量を使う Eigen/TimeHistory は拒否しますが、独立した DL 解析は可能なら継続します。
階が未定義のモデルの初期化は、この解析入口では行いません。

DesignCheckの保存組合せ・自動G+P+Kは`case: null`、`store.persisted: false`です。既存単独ケースの変位・内力と同じIDで保存しません。`load_target`の`source`・`combination_index`・`requested_case`・`terms`で計算対象を確認してください。単独ケースは`case`にケースIDを返し、`store.persisted: true`なら`result_get`で内力を取得できます。

組合せ内力の取得は、`analysis_status`の公開レスポンスにある`status.Done.result_ref`を**JSON文字列として解析**し、サマリ内の`member_forces`と`load_target`を読み取ります。`member_forces.rows`は最大10,000行、内部単位はN/QがN、MがN·mmです。`total_rows`は計算した全行数、`truncated: true`は取得行の省略を示し、全内力の取得完了を意味しません。`store.retrieval`に取得ツール・フィールド・JSON文字列の形式を示します。

組合せサマリと内力はサーバープロセス中のジョブ情報だけに保持します（`store.lifetime: "server_process"`）。結果ストア/プロジェクトへの永続保存、`result_get`での組合せ取得、10,000行を超えた内力のページ取得は未対応です。サーバー終了前に必要なサマリを取得してください。

`Done` の `result_ref`（サマリ JSON）には、解析結果に加え、前処理の注意事項があれば `notices` 配列が付きます。たとえば `ai_mode="SemiPrecise"` では告示精算の適用証拠の検証が未対応のため、`design_period` の有無にかかわらず EX/EY の自動水平力を生成せず、理由が `notices` に入ります（DL等の独立した解析は続行できますが、Ai生成に失敗した標準EX/EYと依存組合せは実行できません）。未設定の床板・壁版割当領域が残っている場合も、件数と対象 ID を記した警告が `notices` に入ります。GUI のような計算前の確認は挟みませんが、矩形RC/SRC大梁が支持する床板の割当が未設定なら、[実重複控除を確定できないため停止](#独立起動のモデル入力制約)します。それ以外は警告を添えて続行します（未設定は荷重・剛性を過小評価し得るため、結果を採用する前に必ず解消してください）。

標準 EX/EY の `LinearStatic` が Ai 地震力の再生成失敗で止まる場合、`Failed.error` に選択方向の具体的な理由を表示します。C0 下限、告示精算の未対応、精算周期の未取得などを確認してください。別方向の再生成通知は失敗原因へ混在させず、DL 等の独立した解析は続行できます。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::analysis_run`（`crates/sepika-mcp/src/server.rs`）で、`kind` ごとの振り分けは `sepika_mcp::job::compute_job`（`crates/sepika-mcp/src/job/mod.rs`）が担います。

### `analysis_status`

ジョブの状態を取得します。

| 引数 | 型 | 必須/任意 | 意味 |
|---|---|---|---|
| `job_id` | `String` | 必須 | `analysis_run` が返した ID |

返り値:

```json
{ "id": "job-0", "kind": "LinearStatic", "status": { /* Queued | Running{progress} | Done{result_ref} | Failed{error} */ } }
```

- `status` は次のいずれかの形を取ります。
  - `"Queued"`（ジョブ登録直後の内部状態。通常は `analysis_run` がすぐに実行を開始するため、ポーリングではほとんど見えません）
  - `{ "Running": { "progress": null } }`
  - `{ "Done": { "result_ref": "<サマリ JSON 文字列>" } }`
  - `{ "Failed": { "error": "<エラーメッセージ>" } }`
- `Running.progress` の `null` は未計測を表します。実測値 `0.0`（0%）とは区別します。現在の解析ジョブは進捗を計測しません。結果保存中も状態を照会でき、保存が終わるまで `Running` です。
- `job_id` が存在しない場合はエラー（`invalid_params`）。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::analysis_status`（`crates/sepika-mcp/src/server.rs`）で、ジョブの登録・状態保持は `sepika_mcp::JobRegistry`（`crates/sepika-mcp/src/lib.rs`）が担います。

### `result_get`

解析結果ストアから結果を取得します。

| 引数 | 型 | 必須/任意 | 意味 |
|---|---|---|---|
| `case` | `u32` | 必須 | 荷重ケース ID（Eigen の結果は `case=0` 固定） |
| `kind` | `String` | 必須 | `"NodalDisp"`/`"MemberForce"`/`"Modal"`/`"TimeHistory"` のいずれか |
| `node_ids` | `Option<Vec<u32>>` | 任意 | 節点 ID での絞り込み |
| `member_ids` | `Option<Vec<u32>>` | 任意 | 部材 ID での絞り込み |
| `step_range` | `Option<Vec<u64>>` | 任意 | `[start, end)` のちょうど 2 要素。それ以外はエラー |

返り値:

```json
{ "case": 1, "kind": "NodalDisp", "status": "current", "current_scope": "model_input_only", "entry": { /* 選択成果の生成元・版・公開path/hash */ }, "rows": [ /* 明細行 */ ], "truncated": false }
```

`(case, kind)` の組が結果ストアにない場合はエラーになります（`analysis_run` を先に実行する必要があります）。
各 `kind` の列構成は[結果ストアのスキーマ](#結果ストアのスキーマ)を参照してください。
生成入力不一致でも保存値は `stale` として照会できます。生成元不明・版不一致・公開ファイル不整合では `status: "unavailable"`、機械可読な `reason`、`action: "analysis_run"` を返し、`rows` は返しません。[保存成果の生成元と利用可否](#保存成果の生成元と利用可否)で判定範囲を確認してください。

実装入口：ハンドラは `sepika_mcp::server::SepikaServer::result_get`（`crates/sepika-mcp/src/server.rs`）で、結果の読み出しと絞り込み・行数上限の適用は `sepika_mcp::persist::result_get_json`（`crates/sepika-mcp/src/persist.rs`）が担います。

## ジョブと結果取得

1. `analysis_run` を呼び、`job_id` を受け取る（この時点でジョブは `Running` としてバックグラウンド実行される）
2. `analysis_status(job_id)` を `Done`/`Failed` になるまでポーリングする
3. `Done` の場合、`analysis_status` の `result_ref` にサマリ JSON が入っている。さらに詳細な明細（節点変位・部材内力など）が必要なら `result_get` を呼ぶ

結果ストアへ保存するジョブは、対象の全種別の書き込み・完了処理とマニフェストの同期に成功した後にだけ `Done` になります。保存失敗は `Failed` です。保存中の `model_edit` と `result_get` は保存完了を待ちます。計算中にモデルが編集されても、そのジョブは開始時のモデルと解析条件による成果を保存します。

```json
// 1. analysis_run(kind="LinearStatic") の返り値
{ "job_id": "job-0" }

// 2. analysis_status(job_id="job-0") の返り値（Done になった時点）
{
  "id": "job-0",
  "kind": "LinearStatic",
  "status": {
    "Done": {
      "result_ref": "{\"kind\":\"LinearStatic\",\"case\":1,\"n_nodes\":2,\"n_member_force_rows\":3,\"max_abs_disp\":1.23,\"store\":{\"case\":1,\"kinds\":[\"NodalDisp\",\"MemberForce\"]}}"
    }
  }
}

// 3. result_get(case=1, kind="NodalDisp") の返り値
{ "case": 1, "kind": "NodalDisp", "rows": [ { "node_id": 1, "ux": 0.0, "uy": 0.0, "uz": 1.23, "rx": 0.0, "ry": 0.0, "rz": 0.0 } ], "truncated": false }
```

`Eigen` ジョブの結果は結果ストアに `case=0` として格納されます（実荷重ケース番号との衝突を避けるための固定値）。`result_get(case=0, kind="Modal")` で取得します。

`TimeHistory` は既存のサンプル波で実行します。サマリの `n_steps` は積分区間数、`n_points` は初期点込みの応答点数、`end_time` は終時刻 [s] です。
正の有限な `duration`・`dt` に対して区間数は ceil(duration/dt)、点数はその数+1、終時刻は区間数×dt となります。
0<duration<dt は 1 区間・2 点です。継続時間または刻みが 0 以下・非有限の場合は生成前に失敗し、解析結果を返しません。
時間軸と相対・絶対加速度の意味は [時刻歴応答解析](../docs/calc_basis/05_構造解析/05_時刻歴応答解析.md#入力時刻と初期状態) を参照してください。

実装入口：ジョブの登録・実行・状態は `sepika_mcp::server::SepikaServer::{analysis_run, analysis_status}`（`crates/sepika-mcp/src/server.rs`）が、結果の取得は `sepika_mcp::persist::result_get_json`（`crates/sepika-mcp/src/persist.rs`）が担います。

## 保存成果の生成元と利用可否

静的解析の節点変位・部材内力、単独ケース検定の部材内力、固有値のモード情報は、開始時モデルの計算入力、実行したジョブ、解決済み生成条件とともに保存します。通り芯と保存振動荷重ケースは入力照合から除きます。その他のモデル変更は `stale` として区別します。節点・部材の絞り込みは保存成果の ID に適用するため、現在モデルから対象がなくなっていても過去の行を照会できます。

`result_get` の `status` は、入力一致の `current`、入力不一致の `stale`、利用不可の `unavailable` です。`current_scope: "model_input_only"` は現在モデルとの一致だけを示します。MCP は現在の解析設定を保持しないため、現在設定との一致や法的適合・合否を証明しません。`stale` の値を現在モデルの成果や検定の根拠として採用せず、解析を再実行してください。

応答の `entry` は選択成果の `case`、`kind`、`rows`、公開 `path` と `sha256`、`provenance` を含みます。`provenance` は実行識別 `run`（再起動後も別実行と区別）、`job_kind`、開始入力の `input_sha256`、`conditions`、計算契約版、結果スキーマ版を保持します。`conditions` は荷重生成に使った地域係数・地盤・C0・Ai算定法・設計周期 [s] と、実対象ケース／単独検定の荷重状態、固有値の要求モード数を示します。`analysis_status` の保存サマリにも今回公開した `store.entries` を返します。同じケースでも `run` が違う種別を同じ解析の組として扱わないでください。

計算契約版は解析 **65**、検定 **44**、結果スキーマ版は **1** です。リリース版やOVIKAコンテナ版とは独立します。すべての対象成果で解析版と結果スキーマ版、単独検定由来の部材内力では検定版も完全一致を要求します。正常な行の返却前に生成元・必要条件・版と公開ファイルのSHA-256を検査します。版の欠落・旧版・未対応の新版を保存や編集で補完せず、現行契約で対象を再実行してください。

| `unavailable` の `reason` | 意味 |
|---|---|
| `unknown_provenance` | 生成元・入力識別・必要条件・版・公開 hash の欠落または不正 |
| `old_contract` | 計算契約版または結果スキーマ版が旧版 |
| `unsupported_contract` | 未来版または未対応のジョブ／結果種別 |
| `inconsistent_file` | 選択した公開ファイルの欠落・hash 不一致・読出し不整合 |

利用不可の応答は `rows` を含まず、`action: "analysis_run"` と再実行案内を返します。開き直し・編集・Undo・別種別の再解析や保存は過去成果の生成元や版を補完しません。正常に公開できた新しい成果だけを記録します。保存失敗では以前の公開ファイルと成果情報を保持し、新しい部分成果を照会対象にしません。これは通常の書込み・公開失敗時の契約であり、電源断後の復元保証ではありません。

保存組合せ・自動 G+P+K 検定は引き続きサマリ内だけの非永続成果です。時刻歴の永続化・ページ取得は対象外です。

要求モード数が `0` の Eigen は、既知の生成条件を持つ空のモード成果として保存・照会します。同じ入力では `current`、`rows: []` になります。要求数 `0` を条件の欠落や不明と読み替えません。

## 計算契約の変更記録

以下は互換性判定に関係する変更の記録です。現在の利用可否は[完全一致の条件](#保存成果の生成元と利用可否)で判定します。

計算契約版は解析 **65**、検定 **44** です。結果スキーマ版は **1** で、リリース版や OVIKA コンテナの版とは独立します。解析の版はすべての対象成果で、検定の版は単独検定由来の部材内力で、結果スキーマ版とともに完全一致を要求します。正常な行の返却前に生成元・必要条件・版と公開ファイルの SHA-256 を検査します。仕口パネルの柱主軸への座標変換と部材端回転への接続、S造の非線形仕口パネルの降伏式と履歴ばねの降伏値の一致、軸耐力到達・ゼロ耐力・設定可能域外の理由付き停止を解析契約に含みます。解析契約版44以前の解析成果と、単独検定由来の検定契約版27以前の成果は旧版として扱い、現行契約での再計算が必要です。剛心・ねじり剛性を共有する3次元質点系の準備でも、集計・除算不能を正常な座標や剛性へ戻さず拒否します。

検定契約版34には、CFT充填コンクリートのFcが24 N/mm²未満の場合に理由付き検定不能とする条件を含みます。単独検定由来の検定契約版33以前の成果は再利用せず、現行契約で再検定してください。

共通局所座標の断面参照方向に非有限値がある場合、または単位直交基底を解決できない場合は、対象部材と理由を示して計算を拒否する解析契約を含みます。旧解析契約版37以前の成果は、入力が同じでも現在の成果として公開せず、現行条件で再解析してください。

支持面には、共通局所座標で補完なしに得た単位直交基底の断面姿勢を使います。有限投影の丸めによるフェース・小梁自重作用区間・需要の差を防ぐ条件を解析40・検定26の契約に含みます。旧解析39・旧直接検定25の保存成果は再実行が必要です。

解析契約には、弾性梁の有限入力から計算不能な剛性・端部縮約が生じた場合に、対象部材と理由を示して停止する扱いを含みます。解析契約版33以前の保存成果は再解析が必要です。

解析契約には、増分部材応答履歴を有限な単精度値で記録できない場合に正常成果を返さず停止する扱いを含みます。旧契約の保存成果は再解析が必要です。

解析契約には、鉛直柱が支持する二次小梁の実最外側面・有限柱閉区間と、是正フェース区間による設計自重／物理質量相当の転送を含みます。旧フェース契約の成果は、入力が同じでも再解析が必要です。

## 結果ストアのスキーマ

独立結果ストアはArrowのRecordBatchをParquetへ書き出し、保存ディレクトリ内の `manifest.json` で公開成果を管理します。同じ `(case, kind)` の新しい公開成果は既存エントリを置き換えます。ファイル名をクライアントで合成せず、`entry.path` を使用してください。公開パスが保存ディレクトリ直下にあり、保存SHA-256と一致することを照会時に検査します。GUIのOVIKAへの検討保存とは別です。実装は[FsResultStore](../crates/sepika-io/src/results/fs_store.rs)と[Parquet入出力](../crates/sepika-io/src/results/parquet_io.rs)です。

結果ストアの列構成は以下のとおりです（すべて Arrow の `UInt32`/`UInt64`/`Float64` 列で構成されます）。

### NodalDisp

| 列名 | 型 |
|---|---|
| `node_id` | UInt32 |
| `ux`, `uy`, `uz` | Float64 |
| `rx`, `ry`, `rz` | Float64 |

### MemberForce

| 列名 | 型 | 備考 |
|---|---|---|
| `elem_id` | UInt32 | |
| `pos` | Float64 | 評価位置（0..1 の正規化座標） |
| `n`, `qy`, `qz`, `mx`, `my`, `mz` | Float64 | 部材内力（軸力・せん断・ねじり・曲げ） |

### Modal

| 列名 | 型 |
|---|---|
| `mode` | UInt32 |
| `period` | Float64 |
| `omega2` | Float64 |
| `part_x`, `part_y`, `part_z` | Float64 |
| `eff_x`, `eff_y`, `eff_z` | Float64 |

### TimeHistory

IO の時刻歴用列構成です。MCP はこの種別を保存せず、現在 `result_get` で正常な行を返す対象は `NodalDisp`・`MemberForce`・`Modal` のみです。

| 列名 | 型 |
|---|---|
| `step` | UInt64 |
| `time` | Float64 |
| `node_id` | UInt32 |
| `ux`, `uy`, `uz` | Float64 |
| `rx`, `ry`, `rz` | Float64 |

`result_get` が 1 回に返す行数の上限は 10,000 行です。超過分は切り詰められ、応答の `truncated` が `true` になります。

実装入口：スキーマは `sepika_io::results::{nodal_disp_schema, member_force_schema, modal_schema, time_history_schema}`（`crates/sepika-io/src/results/schema.rs`）が定め、結果の書き込みは `sepika_mcp::persist::persist_job_outcome`（`crates/sepika-mcp/src/persist.rs`）が行います。

RC壁の参考骨格は同じ静的case/comboの壁脚共通face内力を使い、初期Quは同じrunの静的一次参照から正負別に固定する契約を含みます。建物共通の荒川min／mean方式と新規既定mean、両consumerと同じrunの採用Quへの方式反映を含みます。採用Quのない旧結果や旧計算契約の結果を補完せず、再計算を要します。

## 独立起動のモデル入力制約

- **モデルの読み込みは起動時引数のみ**。`.ovika` ファイルパスをコマンドライン引数として渡す方法しかなく、実行中にモデルを差し替えたり読み込んだりする `model.load` のような MCP ツールは存在しません。ST-Bridge ファイルを直接渡すことはできないため、GUI で ST-Bridge を読み込み、`.ovika` として保存してから、そのファイルパスを MCP サーバに渡してください。保存（`model.save`）と計算書出力（`report.export`）も未公開です。
- **`model_edit` の編集範囲**。階種別・原階の明示所属、STB強度入力・原階のコンクリート強度、断面形状・共有数量設定に加え、壁版・床板・床領域・壁領域・二次部材（未割当を含む）を編集できます。囲まれた床板は任意境界ではなく床板割当領域 `region` への割当で作ります。対応コマンドは[model_edit](#model_edit)を正本とし、GUIと同じUndo経路で適用します。上記以外の節点・主架構部材・荷重などの編集コマンド、および実行中のモデル差し替えは公開していません。存在しない対象など通常の参照不成立は `applied: false`（Noop）になります。割当境界の衝突・版の重複割当・未確認の旧版除去は、モデル・履歴が未更新であることと理由を返します（[二次部材の編集確認](#二次部材の配置移動削除3)）。未対応のコマンド名や必須キーの欠落はエラーになります。
- **主架構線材は異なる2節点のみ対応**。`Beam`・`Fiber`・`MultiSpring`・`Brace` の0・1・3節点以上と同一NodeId二重指定は、`quantity_takeoff` で `status=unavailable`・`totals=null`、`analysis_run` で入力不正の失敗となります。ElemId・kind・実節点数・期待2・入力NodeId列を含む同じ診断を返します。床接触や断面割当の有無によらず拒否し、先頭2節点の採用・自動分割はしません。起動時OVIKA読込も同じ条件です。
- 質点系解析は GUI の右バーからのみ実行できます。`analysis_run` の `kind` には含まれません。
- `Pushover`・`TimeHistory`・`UltimateCheck` ジョブは結果ストアへ書き込みません。対応する結果スキーマがない、あるいはスキーマが要求する粒度（全節点×全ステップ）のデータを持たないため、結果は `analysis_status` の `result_ref`（サマリ JSON）としてのみ得られます。
- `DesignCheck`の検定結果（OK/NG・検定比）はサマリのみです。単独ケースの`MemberForce`は結果ストアへ保存します。保存組合せ・自動G+P+Kは`case: null`・`store.persisted: false`で、組合せ内力もサマリのみです。`analysis_status`の`status.Done.result_ref`をJSON文字列として解析し、`member_forces`と`load_target`を取得してください。内力は最大10,000行（`truncated`/`total_rows`で省略を確認）、サーバープロセス中だけ保持し、`result_get`/永続化/ページ取得は未対応です。
- 未設定の床板・壁版割当領域は、前処理の注意事項（`notices`）に件数と対象 ID を記します。矩形RC/SRC大梁が支持する床板の割当が未設定の場合は、[実重複控除](../docs/calc_basis/01_荷重/06_自重の長期応力解析への接続.md#矩形rcsrc大梁と床板の実重複控除)を確定できないため停止します。それ以外の未設定は警告を添えて続行しますが、床荷重・躯体自重と壁の剛性が解析に入らず応力・変形を過小評価し得るため、解消後に採用してください（GUI では計算前に続行確認を挟みます）。

実装入口：公開ツールと結果ストア書き込みの有無は `sepika_mcp::server::SepikaServer`（`crates/sepika-mcp/src/server.rs`）・`sepika_mcp::JobKind`（`crates/sepika-mcp/src/lib.rs`）・`sepika_mcp::persist::persist_job_outcome`（`crates/sepika-mcp/src/persist.rs`）が定め、モデルの読み込みは起動バイナリ（`crates/sepika-mcp/src/main.rs`）がコマンドライン引数で行います。

DesignCheckの`load_target`は、保存組合せ・自動G+P合成・単独ケースを区別します。未判定の継続時間は`term: null`、必要応力の解析失敗は入力不備となり、部分成功で合格を返しません。`legal_conditions_verified: false`は地域指定・風時実況等の法的適用条件を自動確認していないことを示します。小梁・床の長期略算はGUIで別計算し、MCPの主架構サマリには含めません。
