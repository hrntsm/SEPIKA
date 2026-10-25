# 異厚組立BOXの検証記録（Issue #831）

状態: 🔶。2026-10-26、鋼BOX・角形CFTの鋭角材料領域と入力・編集・保存契約を対象とする。

## 根拠と採否

[Issue #831](https://github.com/hrntsm/SEPIKA/issues/831)の外形−内法長方形による独立算術を照合基準とする。
A/Iは面積・二次モーメントの積分、鋼BOXのZpは中心軸の上下半断面の一次モーメントの和から求める。
この上下・左右対称形状では、図心軸と等面積軸が外形中心に一致する。
CFT充填部の幾何は内法長方形で、重量・物理質量は既存の鋼材と無筋コンクリートの材料別算定へ渡す。

J・せん断の異厚採用式と適用条件、および独立期待値は未確認。
平均・最小・最大いずれかの代表厚を一様厚式へ代入する方式は、安全側と保証できず採用しない。
J/Asは未算定のまま保存し、必要な解析・鋼部材検定・CFT等価剛性には理由付き拒否を返す。
CFT終局曲げ耐力・冷間成形耐力比・部材ランクの一様厚式も異厚へ無条件に拡張しない。
接合部の対象方向の実壁厚とFの配線確認は、耐力式の原典適合を意味しない（[別調査 #723](https://github.com/hrntsm/SEPIKA/issues/723)）。
STB固有の異厚受理と属性報告は[別実装 #683](https://github.com/hrntsm/SEPIKA/issues/683)に残し、既存の暫定拒否を維持する。異厚のSTB出力は一様厚への変換で情報を失うため、両公開出力入口で理由付き拒否とする。

## 独立期待値とテスト索引

外高さ300 mm・外幅200 mm・左右板厚9 mm。

| 上下板厚 mm | 鋼A mm² | Iy mm⁴ | Iz mm⁴ | 鋼Zp mm³ | 充填A mm² |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 6 | 7584 | 87700608 | 55314368 | 726048 | 52416 |
| 9 | 8676 | 109875852 | 58328652 | 881658 | 51324 |
| 12 | 9768 | 131127264 | 61342936 | 1033992 | 50232 |

| 対象 | 自動回帰の所在 | 確認範囲 |
| --- | --- | --- |
| A/I/Z/Zp、内法、不正寸法、未算定J/As、材料別質量 | `crates/sepika-core/tests/built_box.rs` | 上表と有限正内法、供給済みフラグによる未確認性能の成功化拒否 |
| 接合部方向別幾何 | 同上 | 強軸実厚9/中心線せい288、弱軸実厚12/中心線幅191 mm |
| F区分 | `crates/sepika-design-jp/tests/built_box.rs` | t1/t2のどちらが41 mmでも最大厚41 mm、SN400BのF=215 N/mm² |
| 材料別ファイバー領域 | `crates/sepika-section/tests/built_box.rs` | 両異厚の鋼・充填面積（面積許容差1e-7 mm²） |
| 編集・Undo/Redo・削除再採番 | `sepika-edit::tests::built_box_edit_undo_redo_and_renumber_preserve_horizontal_thickness` | 同厚→異厚の再生成と参照追随 |
| OVIKA保存・読込とID置換 | `sepika-io::ovika_reference_tests::built_box_ovika_roundtrip_keeps_both_thicknesses_pending_properties_and_references` | 主架構・床領域小梁・壁領域間柱・未割当小梁/間柱の5参照先 |
| 同厚・圧延角R | 既存core/section/edit/io/design/element/load回帰 | 既存同厚入力は両板厚を同値として明示 |

## 検証の限界

幾何Verificationと保存・編集契約の検証であり、異厚BOX骨組の解析精度、J/せん断/耐力式の原典照合、物理Validation、手動GUI操作は未確認。
GUIの作成欄と断面輪郭はコードとfeature付き検証で確認する。汎用作図UIやSRCへのBOX適用は追加しない。
OVIKAの版間互換移行はADR0014に従い追加せず、解析・設計の計算契約版をそれぞれ58・32へ更新した。
独立レビュー・最新headのCI結果は公開PRで別に確認する。
