# Issue416 本番P1の保存証拠

最終校正4試験・144完了levelのscalar原記録を2026-10-18に複写した。数値再求解は収載時に実施していない。
`manifest.json`のsource path/source SHA256と収載SHA256は原本コピーの対応を示す。
`.gitattributes`は改行の自動変換を無効化し、原記録のbyteを保持する。

| 保存物 | 位置づけ |
| --- | --- |
| `final-calibration-run.log` | 元4試験のstdout/stderrと当時の絶対出力先 |
| `final-calibration-metadata.json` | 実行command、OS/CPU、数値sourceとCargo.lockの実行時hash |
| `*-13080-*/rows.jsonl` | 変換8件、楕円27levels、矩形15levels、半径極限102levelsのscalar原記録 |
| `final-calibration-levels.csv` | 144levelsの品質・時間の集計派生物 |
| `final-calibration-summary.json` | 元4試験の期待値・差・系列・品質集計派生物 |
| `final-calibration-raw-sha256.json` | 当時の全rawのpath/size/hash。未収載mesh JSONへの参照も履歴として保持 |
| `audit-final-calibration.ps1` | 元TEMPで集計・品質監査した原script。絶対pathと未収載meshを参照し、このpackageだけでの完走用ではない |

楕円27meshと変換8meshの全座標・三角形配列は巨大なため収載していない。JSONLにはscalar観測を保存したが、このpackageだけで当時の全配列を再検査できない。
独立参照P2の証拠は[別package](../issue416-reference/README.md)、結果の解釈・未検証範囲は[V&V報告](../../断面JとCFTコアねじり_Issue416_2026-10.md)を参照する。

`windows-grid/`には6processの全scalar calls JSONL/CSV、manifest、環境・source hash、OS process観測、stdout/stderr、元集計とwrapperを保存する。96labelsの160fresh全成功・32alias、3形状各3反復両向き18freshを含む。`artifact-sha256.json`は元TEMP全体の歴史的hash一覧であり、未収載source snapshot等への参照も含む。収載コピーの対応は上位`manifest.json`で確認する。

`app-final/`はApp fixture整合後の壁integration18成功と最終GUI全回帰945成功/0失敗/既存1ignoreの原raw2件、および担当handoffとrawに基づくcommand metadataを保存する。18件は945件に重複加算しない。metadataは新しい派生記録としてmanifestの`authored_derivatives`に分け、校正/gridの件数をApp件数に混ぜない。

source snapshotと巨大meshは収載しない。wrapperは元repo/TEMPパスを既定値に含みCIMを要し、集計scriptも元TEMP配置を前提にするため、portableな再実行を保証しない。再現は新TEMP出力先で行い原証拠を上書きしない。数値再求解・全Done・最終CI/mergeの完了をpackageの存在だけで認定しない。
