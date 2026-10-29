# 断面案の変更・実計算結果比較の確認（Issue #980）

## 範囲と状態

2026-10-29、main `c0b737ee` から比較用の固定対象数量、指定節点変位、ケース別主架構検定、通常図の2案表示を実装した。自動回帰とWindows実機確認を分けて記録する。構造計算式の変更や、実物Validation、設計全体の適合確認は扱わない。

## 直接回帰

| 対象 | 回帰の入口 |
| --- | --- |
| 固定大梁だけの数量、6m×断面積との数量照合、通常の符号付き変位・既存検定との一致、全ケース根拠と保存往復 | `study::comparison::tests::fixed_volume_signed_displacement_and_every_case_design_match_normal_results_and_roundtrip` |
| 同名・同IDの荷重値差、支点・設定・組合せ差 | `conditions_compare_actual_definitions_not_names_ids_or_governing_case` |
| 支配ケース交替、変更外の新規NG、未検定行の計数 | `envelope_allows_governing_case_change_and_counts_skips_and_outside_new_ng_at_same_granularity` |
| xi移動・順序交替、同部材ケース全位置最大、未比較位置と新規NG保留 | `moved_or_reordered_positions_are_not_paired_by_index_and_member_case_maxima_keep_actual_positions` |
| 対象外柱が全体支配する場合の固定大梁集合の独立集計 | `fixed_member_maximum_is_independent_of_dominating_outside_column` |
| 検定項目集合差と未検定範囲では部材ケース最大の差・悪化を付けない | `member_case_envelope_does_not_subtract_different_check_items_or_incomplete_ranges` |
| 未検定件数が同じでも算定位置が交替する固定対象・全体範囲差 | `equal_skipped_counts_do_not_hide_different_rated_positions_but_fully_rated_maxima_allow_position_moves` |
| 共通根拠で空・非有限・負成分を未評価とし0/OKにしない | `result_evidence::tests::malformed_check_components_stay_unrated_in_common_member_and_joint_evidence` |
| 図内caseは共通指定に固定し、図・変位・検定が次frameも一致 | `diagram_case_controls_keep_common_case_displacement_and_checks_consistent_across_frames` |
| 通常検定比図の最大／項目フィルタとtooltipで空・非有限・負を未評価 | `viewer::check_ratio::incomplete_evidence_tests::malformed_components_are_unrated_in_diagram_filters_and_tooltips` |
| 通常検定表で空・非有限・負を未評価とし保存結果不変 | `result_evidence::tests::malformed_components_remain_unrated_when_opening_normal_design_table` |
| 開いた後の案再編集・実再計算・case切替でも旧runを全指標/図で保持し、再表示だけが新runへ更新 | `case_switch_and_normal_result_navigation_keep_the_constructed_run_until_explicit_refresh` |
| 正常／不正混在・全不正の部材/節点集約と実描画の未評価マーカー・既知NG | `incomplete_evidence_tests::mixed_and_all_malformed_components_never_make_members_or_nodes_fully_ok`、`mixed_and_all_malformed_outcomes_draw_unrated_member_and_node_markers` |
| 壁の正常＋不正/NG＋不正/全不正を通常表と凡例で同じ未検定件数・最大へ表示 | `incomplete_evidence_tests::malformed_wall_components_are_unrated_in_table_and_legend_without_hiding_known_ng` |
| 生成時の全対象・xi・項目期待構造を保存し、対象/空位置/一部項目/期待情報の欠落を検出 | `saved_expected_targets_positions_and_component_multisets_reject_missing_result_structure` |
| 両案の同じ固定/変更外部材欠落を保存再読込後も完全範囲として比較しない | `same_missing_fixed_or_outside_members_never_become_matching_complete_scopes_after_reload` |
| 部分項目欠落を比較図・保持通常結果・共通根拠の低い比へ戻さず、期待/原本は不変 | `partially_missing_check_items_cannot_reappear_as_low_ratios_in_comparison_or_common_evidence`、`normal_retained_result_and_reference_also_reject_partial_items_without_changing_saved_scope` |
| 同じChecked内のNG＋不正／適合＋不正／全不正で、比較件数・実比図・tooltip・通常表・共通根拠・壁のNGと未評価を保持し元検定数を維持 | `same_position_partial_ng_is_both_ng_and_unrated_without_comparable_difference`、`same_position_partial_ng_survives_filters_tooltips_and_wall_table_and_legend`、`same_position_partial_ng_remains_visible_in_normal_table_and_common_roots` |
| metadata全/部分棄却・正常NG0・固定対象なし・未計算・旧契約・根拠なし・失敗emptyの件数欄 | `count_labels_distinguish_unavailable_partial_normal_zero_ng_and_no_targets` |
| 壁の対象内＋対象外・全対象外・同位置NG/未評価併存＋対象外でも原本の対象内検定件数を表/凡例で保持 | `wall_counts_preserve_original_target_scope_with_outside_and_partial_ng` |
| 空・非有限検定成分、旧契約、生成入力欠落、数量対象なし・欠落・scope改変 | `invalid_design_components_and_stale_or_missing_saved_inputs_never_become_valid_zero` |
| 通常GUI描画・共通根拠閲覧の原本不変、再編集後の計算時断面と図の一致 | `study::comparison::gui::tests::comparison_frames_and_evidence_keep_model_results_and_saved_inputs_unchanged` |
| 両案で同じケースが欠落した場合、計算失敗、旧検定契約 | `same_case_missing_from_both_sides_and_failed_or_old_results_are_explicitly_incomplete` |

支配ケース交替と新規NGの行集計は合成した契約データによる。これを構造計算の独立数値照合や実機操作の代用にはしない。正常門型モデルの計算は既存準備・静的解析・検定・数量を実行し、通常結果との抽出一致を確認する。

## 実行結果

2026-10-29、最新stableへ更新し、`rustc 1.99.1`／`clippy 0.1.99`を使用した。最終製品ソースで以下を確認した。

| コマンド／構成 | 結果 |
| --- | --- |
| `cargo check -p sepika-app --features gui --tests` | 成功 |
| `cargo test -p sepika-app --features gui --lib wall_counts_preserve_original_target_scope_with_outside_and_partial_ng -- --nocapture` | 1成功 |
| `cargo test -p sepika-app --features gui --lib --quiet` | 1130成功、2 ignored |
| `cargo test -p sepika-app --no-default-features --lib --quiet` | 451成功 |
| `cargo clippy -p sepika-app --all-targets --locked --features gui -- -D warnings` | 成功 |
| `cargo clippy -p sepika-app --all-targets --locked --no-default-features -- -D warnings` | 成功 |
| `cargo fmt --all -- --check` | 成功 |
| `target/debug/xtask.exe check-docs` | 成功（summary 125、links 676、impl refs 722） |
| `mdbook build` | 成功 |
| `git diff --check`／`git diff --cached --check` | 成功（新規実装ファイルを含む） |

同位置内のNG＋不正／適合＋不正／全不正は専用3回帰、取得状態8種はGUIテキスト回帰でも成功した。件数ラベル修正後にGUI構成の統合106件、nonGUI構成の統合100件も成功した（各構成の性能probe1件はignored）。最後の壁の分母修正はGUI表示用集計のみで、対象内外と原本不変を上記の専用回帰・全GUI libで確認した。

独立したfresh12は修正必須なし。比較18件、未評価図表6件と根拠回帰を独立実行した。独立レビューを、計算式の独立数値照合や実機操作の代用にはしない。

`cargo build -p sepika-app --features gui --bin sepika --locked`も成功した。専用`target/debug/sepika.exe`は2026-10-29 16:04:13 JST、52,792,320 bytes。実機確認は次節で実施結果を分ける。

## 実機確認

代表fixtureは `write_issue980_manual_fixture` を明示実行して専用worktreeの `target/issue980-artifacts` に一意名で出力する。通常の3部材portal、部材2の固定大梁1本、基準案・案A（大断面）・案B（基準断面）、全ケース計算を使う。未設定領域警告の続行操作は含めない。

生成時の期待構造を含むfixture `target/issue980-artifacts/comparison-c2104296-945d-4cb2-a52f-e5d44dc19192.ovika` は、2026-10-29に `write_issue980_manual_fixture --ignored --nocapture` で生成し、保存再読込一致を確認した。旧fixtureとは区別する。

| 正常fixtureの指標 | 基準案／案B | 案A |
| --- | --- | --- |
| 部材2の固定大梁1本の対象鋼材体積 [m³] | 0.05002242504925996 | 0.07695042504925996 |
| 長期・節点3・全体Z変位 [mm] | -0.04324120377185385 | -0.043241203771853864 |
| 同ケース集合の最大検定比 | 0.23465835433923296 | 0.13054491476175248 |
| 最大の支配 | 部材2、xi=0.5、荷重ケース0 長期 | 同左 |
| 全主架構の検定済／NG／未検定行 | 26／0／52 | 同左 |

組合せ「長期＋地震X」は静的変位を持つ一方、既存検定の未対応を未検定として保持する。NG0は未検定範囲の合格を意味しない。実機で地震X・節点3・全体Xを選ぶ場合の期待変位は基準／案Bが3.1027841140808 mm、案Aが2.5551598986541824 mm。

### Windows実機で確認した範囲（メイン担当）

メイン担当が上記の最終バイナリとfixtureを用い、通常窓1282×831で操作した。OS Settingsの起動・取得・操作・変更は行わず、DPIも変更しない。AI・外部接続・本案採用は行っていない。自動回帰を実機操作の代用として扱わない。

| 操作 | 実際に確認した結果 |
| --- | --- |
| native Openでfixtureを読込 | 本案世代0、3部材・4節点・3断面、固定対象1、基準＋A＋B、Run1基準世代0／Run2 A世代1／Run3 B世代0、未保存印なし |
| 変更・結果を比較 | 通常図2つ・読取専用・差は案−基準案と表示。基準／A／Bの体積と最大比が上表の丸め値に一致。A体積差+0.026928 m³、固定対象最大比差-0.104113。固定対象は6算定・NG0・12未検定、全主架構は26算定・NG0・52未検定。条件一致、全体最大差は位置／未検定範囲不一致で抑止する理由も表示 |
| 共通ケース・指定変位・集合最大の切替 | User1地震X・節点3・全体Xは基準+3.102784、A+2.555160、差-0.547624 mm。両図の共通ケース／単一ケース表示を保持。集合最大offでは固定0算定・3未検定、全体0算定・13未検定となり、最大を0／合格に補完しない。長期・節点3・Zは-0.043241 mm、節点0・Zは+0.000000 mm。符号とゼロを維持し、改善率・改善判定なし |
| 変更表と図の連動 | Aの基準断面名「梁H-400x200x8x13」、部材2、階未割当、変更後「比較用大梁」、対象1を確認。部材2ボタンから左右図の同じ大梁が紫選択。任意のA対Bへ切替可能 |
| 図の共通設定 | 通常の変形モード・地震X・比較共通絶対100倍を両側で実画像とAX確認。左を2Fへ変更し、共通入口から両側2F／同向き／100倍へ同期。このfixtureには通りがなく、階で構面同期を確認。コンター個別自動、検定比共通0–1とNG色の凡例を確認 |
| 数量の共通根拠と計算時入力 | A固定対象1・明細1・体積0.076950 m³・鉄骨0.604061 t、案世代1・数量契約1を確認。計算時入力popupは読取専用で、本案とは別の保持入力。閉じて比較へ復帰し、本案世代0を維持 |
| 負変位と最大比の共通根拠 | 変位は節点3、座標[6000,0,3500] mm、全体Z、-0.043241 mm、ケース0長期、解析契約65。固定最大比は部材2・xi=0.5・0.1305、比較用大梁、面積12825.07084… mm²、SN400B Fy235、検定契約44、長期1ケース。曲げ0.1305／せん断0.0000と式名を確認。入力編集操作は行わない |
| 変更外悪化と根拠への移動 | DL柱0／1の全位置最大が基準0.012638（xi=0.942857）→A 0.015042（xi=0.928571）、差+0.002404、各未検定0。位置集合差を別表示。「案の根拠」から柱0・xi約0.929・比0.0150・軸曲げ・DL・契約44へ移動し、計算時の柱H300と1ケース根拠を確認。xi一致の位置別悪化も別表示 |
| 注記・別名保存・AIなし再読込 | 比較を閉じ、注記「断面比較の実機確認。計算値と未評価範囲を確認。」を日本語入力。未保存印が付き、本案世代0／Run1–3不変。事前不存在を確認した専用`target/issue980-artifacts/issue980-cua-reviewed.ovika`へ別名保存し、保存通知と未保存印の消失を確認。native Openで再読込し、注記・固定1・3案Run1–3・本案世代0・A世代1と体積／最大比／26・0・52を復元。出力169639 bytes、2026-10-29 16:27:41 JST |

正常fixtureに新規NGはない。新規NG・支配ケース交替・失敗／欠落／古い成果／不正成分の状態は実機入力を改変して作らず、上記の契約回帰と区別する。

### 実機での終了観測と未確認範囲

初回の専用プロセスPID7916／window14418054は、入力やfixture読込前の最初のactivateで`failed to activate captured window`となった。fresh窓一覧で消失、CLIのPID不在を確認し、stderrはフォント出力のみだった。規定の再取得後、同じexeを一度再起動したPID34020／window21299906で上記操作を行った。初回終了原因は未断定であり、製品不具合やユーザー操作と推測しない。

外枠のnative drag後、1282×831→802×831の実画像を取得した。高さは縮まらなかった。次のstate取得で`foreground window did not report a process id`となり、fresh窓一覧に専用窓なし、CLIのPID34020不在を確認した。stderrはフォント出力のみでpanic出力はなかった。旧座標を破棄してCU入力を停止した。ユーザー停止・ロック等の明示はなく、終了原因は未断定。

その後、root指示による狭幅段階の正規回復として、同じexeを一度だけPID35420／window6556490で起動した。PowerShell側でProcessObject・WaitForExit・ExitCodeを保持し、通常窓1282×827から右edge dragで802×827へ縮小、次frameの主画面コンパクト再レイアウトを確認した。底edgeで高さ631pxへは変わらず、無理な反復は行わなかった。

802×827で専用保存済み`issue980-cua-reviewed.ovika`を再読込し、日本語注記・本案世代0・Run1–3の保持を確認した。左ナビから検討・比較を開くと画面幅へ再レイアウトし、案選択・共通ケース・集合最大・固定体積0.050022・比0.234658・2図・右上Closeが画面内に表示されることを実画像とAXで確認した。比較Close→検討Close→主画面native Closeで終了し、保持したプロセスがEXIT=0、PowerShell実行もexit0を返した。CLIではPID7916・34020・35420全ての不存在を確認した。最終`cua-narrow-recovery-stderr.txt`はフォント出力のみでpanic出力はなかった。

元の二度の原因不明の消失は未断定の履歴として残す。別段階の正規回復では再発せず、802px幅の代表操作への到達と通常Closeを確認した事実と区別する。高さ631pxとDPI全条件は未確認。

専用保存出力のSHA256は`BB531A5EAFDD9B8728C996280AD60F7497550FDDF5A9EE039829070223855782`。OS Settingsの操作や#979のR0警告続行の再試行は行っていない。製品ソースの追加変更はない。

## 未確認

全モデル・全断面・全ケース・全表示条件、高さ631px・DPI全条件、支援技術、独立した規準式の数値照合、実物Validationは未確認。自動最適案、本案採用、外部AI通信は対象外。既知の材料名編集不具合#998は変更しない。
