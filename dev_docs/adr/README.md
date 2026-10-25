# 設計判断（ADR）

Squid-n の重要な設計判断の正本。用語の意味はルートの [CONTEXT.md](../../CONTEXT.md) が持つ。

## 一覧

| # | Status | タイトル |
|---|--------|----------|
| [0001](0001-internal-units.md) | accepted | 内部単位系は N・mm・s とする |
| [0002](0002-determinism.md) | accepted | 決定性は単一スレッドのビット一致を保証する |
| [0003](0003-single-writer-edit-commands.md) | accepted | モデル所有権は単一ライタとし、編集はコマンド経由に限る |
| [0004](0004-story-and-diaphragm-separation.md) | accepted | 階と剛床を分離し、階は床レベル基準の利用者定義とする |
| [0005](0005-materials-on-sections.md) | superseded by ADR-0051 | 材料は断面が持つ |
| [0006](0006-wall-elements-generated-from-wall-plates.md) | accepted | 壁エレメントは壁版から都度生成する派生物とする |
| [0007](0007-floor-regions-slabs-secondary-members.md) | accepted | 床は床領域・床板・二次部材で表し、解析要素は柱と大梁までとする |
| [0008](0008-secondary-member-load-cascade.md) | superseded by ADR-0018 | 二次部材の荷重は支持相手へ逐次伝達し、交点は常にピンとする |
| [0009](0009-fail-loud-unassigned-inputs.md) | accepted | 未設定の材料・断面を既定値で埋めない |
| 0010 | — | 欠番（HHT-α 廃止は handoff と現行仕様で足りるため ADR 化しない） |
| [0011](0011-no-gpu-reserved-crates.md) | accepted | GPU は恒久的に実装せず、予約クレートを置かない |
| [0012](0012-stbridge-partial-roundtrip.md) | accepted | ST-Bridge は完全往復を保証しない |
| [0013](0013-adopt-wall-element-model.md) | accepted | 耐震壁は壁エレメント置換モデルを採用し、TVLEM は採用しない |
| [0014](0014-schema-compatibility.md) | accepted | スキーマ互換は初回リリースまで維持しない |
| [0015](0015-cantilever-secondary-members.md) | accepted | 片持ち小梁は支持条件で表し、基端モーメントは伝達しない |
| [0016](0016-attached-slabs-between-members.md) | accepted | 取り付く床板は支持部材の間ごとに表す |
| [0017](0017-attached-slab-support-edges.md) | superseded by ADR-0037 | 取り付く床板の荷重は、全長を覆う支持部材の辺へ最近接負担面積で分配する |
| [0018](0018-explicit-gravity-supports.md) | partially superseded by ADR-0050 | 壁自重の支持辺と鉛直二次部材の端部負担率を明示する |
| [0019](0019-material-wall-section-and-directional-strength.md) | accepted | 壁の材料別実断面と正負別せん断耐力を採用する |
| [0020](0020-assign-plates-to-member-bounded-regions.md) | accepted | 囲まれた床板・壁版は支持部材で分割された領域へ割り当てる |
| [0021](0021-same-position-support-members.md) | accepted | 同じ位置に支持部材が重なる場合は同種をエラー、異種は主架構優先とする |
| [0022](0022-unmapped-slit-specification-warning.md) | accepted | 耐震スリット指定が辺へ反映できない場合は警告する |
| [0023](0023-hinge-detail-single-source-analysis-models.md) | accepted | ヒンジ詳細の表示は解析要素と同じ非線形モデルを単一情報源とする |
| [0024](0024-separate-panel-element-and-joint-check.md) | accepted | 仕口パネル解析要素と柱梁接合部の断面算定を分離する |
| [0025](0025-consistent-mass-section-properties.md) | accepted | Beam と Fiber の整合質量は共通の断面質量特性から算定する |
| [0026](0026-current-spec-single-source-docs.md) | accepted | 現在仕様の正本は docs とし、フェーズ実装仕様書を置かない |
| [0027](0027-calc-docs-use-code-units.md) | accepted | 計算根拠の単位表記は法令・規準の単位に従う |
| [0028](0028-src-cft-ns-material-or-fallback.md) | partially superseded by ADR-0062 | SRC/CFT の等価断面剛性は材料由来 ns を通常とし、算定不能時は N_S_EQ=15 へフォールバックして通知する |
| [0029](0029-live-load-four-purposes.md) | accepted | 積載荷重を床用・小梁用・大梁用・地震力用の 4 区分とする |
| [0030](0030-cft-steel-allowable-stress-fixed-young.md) | accepted | CFT の鋼管を専用材料で参照し、許容圧縮には正の有限な材料Eを用いる |
| [0031](0031-withdraw-high-strength-shear-reinforcement.md) | accepted | 高強度せん断補強筋の製品別対応を撤回する |
| [0032](0032-concrete-allowable-stress-basis.md) | accepted | コンクリートの許容圧縮・せん断応力度の採用値 |
| [0033](0033-steel-design-weight-vs-physical-mass.md) | accepted | 鋼材の設計重量（78.5 kN/m³）と物理質量（7.85 t/m³）を分離する |
| [0034](0034-rc-section-purpose-types-and-real-rebar.md) | accepted | RC 断面を用途別に型分離し、実配筋モデルへ移行する |
| [0035](0035-lumped-mass-physical-mass.md) | accepted | 質点系の層質量を物理質量相当へ統一する |
| [0037](0037-cantilever-slab-load-to-attachment-girder.md) | accepted | 線アンカーの取り付く床板は取付き大梁だけへ等分布伝達する |
| [0038](0038-cantilever-slab-tip-load-transfer.md) | accepted | 片持ちスラブの先端荷重は取付き梁へ力のみ等分布伝達する |
| [0039](0039-section-property-basis.md) | accepted | 断面性能の算定元を項目別に区別し、フィレット半径・角Rの編集時の更新を分ける |
| [0040](0040-damper-total-weight-and-mass-source.md) | accepted | ダンパーはモデル化範囲の総重量を直接入力し、断面由来質量を置き換える |
| [0041](0041-holding-result-input-validity.md) | accepted | 保有水平耐力に採用する結果ごとに生成入力を照合する（検定・時刻歴の入力所属にも適用） |

| [0042](0042-attached-slab-validation-and-support-resolution.md) | accepted | 取り付く床板の幾何と支持先を入力境界で共通検査する |

| [0043](0043-member-load-explicit-extent.md) | accepted | 部材荷重の全長追従は強度を保持する明示属性とする |

| [0044](0044-wall-horizontal-ratio-and-component-validation.md) | accepted | 壁横筋比は最小筋比から推定せず出力成分ごとに入力不足を扱う |

| [0045](0045-wall-committed-response.md) | accepted | 壁単体の確定step応答を線材応答から分離する |

| [0046](0046-polygon-floor-finite-segment-integration.md) | accepted | 全周支持の凹形床を有限線分最近接と境界実面積で分配し、モデル差と格子誤差を区別する |
| [0047](0047-source-story-and-analysis-membership.md) | accepted | 原階の明示所属は解析用所属から独立して保存する |
| [0048](0048-rc-beam-reference-total-rotation.md) | accepted | RC梁の明示基準総角と追加端回転を分離する |
| [0049](0049-purpose-specific-story-evaluation.md) | accepted | 目的別確定評価点と層切断面力を保持する |
| [0050](0050-wall-weight-purpose-partitions.md) | accepted | 壁版の物理総量からDL支持と地震用階帯を独立生成する |

| [0051](0051-stbridge-strength-input-and-section-material.md) | accepted | ST-Bridgeの元強度指定と断面材料を分離する |

| [0052](0052-stbridge-rebar-source-and-analysis-boundary.md) | proposed | 標準配筋の元入力と解析用配筋を分離する |

| [0053](0053-src-material-regions-and-discrete-rebar.md) | accepted | 矩形SRCの内蔵H控除と離散主筋近似・独立質量を分離する |

| [0054](0054-mcp-result-publication-provenance.md) | accepted | MCP 保存成果の開始入力・計算契約と公開ファイルを結び付ける |

| [0055](0055-rc-wall-static-face-and-initial-capacity.md) | accepted | RC壁の同一face内力とrun初期耐力を分離する |

| [0056](0056-rc-wall-shear-method.md) | accepted | RC壁の終局せん断耐力方式を建物共通の二択にする |

| [0057](0057-quantity-settings-and-directional-wall-subtotals.md) | accepted | 数量設定を保存モデルに持ち、方向別壁筋の不足を小計へ伝播する |

| [0058](0058-rc-beam-signed-q0.md) | accepted | RC梁Q0を符号付きlocal Qy端反力の合成から求める |

| [0059](0059-incomplete-model-reference-integrity.md) | accepted | 入力完成度と保存可能な参照ID整合性を分離する |

| [0060](0060-steel-directional-shear-ratio-sum.md) | accepted | 鋼柱・鋼梁の二方向せん断は方向別検定比の和を採用する |

| [0061](0061-ultimate-gravity-demand-modes.md) | accepted | 終局需要を静的ヒンジ略算と初期重力を含む確定総応答に分ける |

| [0062](0062-cft-core-torsion-resolution.md) | accepted | 断面 J の算定元を解決し、CFT コアの数値ねじりを幾何性能から分離する |

| [0063](0063-wall-side-column-foundations.md) | accepted | 側柱の物理分類・回転接続・局所釣合いを要素表現から分離する |

| [0064](0064-fiber-ms-axial-biaxial-consistency.md) | accepted | Fiber と MS の軸・二軸曲げを同じ材料応答へ整合させる |

| [0065](0065-elastic-wall-top-bottom-girders.md) | accepted | 壁上下の剛梁を連続した壁板の弾性高剛性部材として扱う |

| [0067](0067-nonlinear-timehistory-unconverged-rollback.md) | accepted | 骨組の非線形時刻歴の全体未収束は時刻開始状態へ復元して停止する |

| [0068](0068-panel-reference-column-lower-first.md) | accepted | 仕口パネルの参照柱を物理的な下側優先で統一する |
| [0069](0069-panel-column-axes-and-h-weak-shear.md) | accepted | 仕口パネルを柱2主軸へ接続し、H形弱軸はフランジ2枚の純せん断モデルとする |
| [0074](0074-pushover-physical-support-resistance.md) | accepted | 増分性能曲線を物理支持反力から回復し、層切断面力と区別する |

| [0070](0070-story-fl-sl.md) | accepted | 階の FL・差分を入力とし SL と用途別の高さを導出する |

| [0071](0071-beam-actual-placement.md) | accepted | 梁の実配置を基準材軸・解析接続から分離する |

| [0072](0072-slab-actual-placement.md) | accepted | 床板の実配置を支持座標から分離する |

| [0073](0073-panel-connection-face-distance.md) | accepted | 仕口パネルの梁端接合面を既存入力から一意に解決する |
| [0075](0075-rc-beam-length-weighted-slab-thickness.md) | accepted | RC梁の床板厚を各側の取付長で加重平均する |

| [0076](0076-beam-eccentric-connection.md) | accepted | 梁の実材軸への偏心接続を既存要素の外側で合成する |

## 規約

- ADR にするのは「後から覆すコストが高い」「背景を知らないと意外に見える」「実際のトレードオフがあった」の 3 条件をすべて満たす**現在有効な判断**だけ。単なる実装手順・容易に変えられる選択・未実装の計画・自明な判断は書かない。
- 番号は本ディレクトリ内の新規連番。欠番は許容し、削除した番号を再利用しない。本文で D 番号（文書ローカルな識別子）を引用するときは「文書パス＋定義節＋番号」で書く。
- 判断を覆すときは、新しい ADR を作成し、旧 ADR の Status を `superseded by ADR-XXXX` に更新する。
- Legacy source には移行元の一次資料を残す。移行元を現行仕様へ書き換えた場合は、改訂前の要点を handoff に転記してから参照する。
- 追加・更新したら、この一覧にも 1 行追記する。
