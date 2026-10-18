# Issue416 独立二次要素そり参照の保存成果

2026-10-18の開発専用参照の自作スクリプト・設定・結果を保存する。
原本コピーは [manifest.json](manifest.json) の `source_path/source_sha256/sha256` で追跡できる。
派生した配布provenanceと新しい検査スクリプトは原本コピーと区別した。
本収載時に数値再求解・環境再構築は実施していない。

| ディレクトリ | 保存対象と位置づけ |
| --- | --- |
| `sepika416-reference-environment` | Python/依存の実版・配布hash lock、円/円環6評価、初回JSON変換失敗 |
| `sepika416-reference-calibration` | 楕円/矩形25評価、独立級数、追加細分化、公開API交差照合 |
| `sepika416-h-box-reference` | 旧H/Box31評価、モデルとの差、初期失敗、別生成meshのhash不一致 |
| `sepika416-reference-reproduction` | 同時mesh保存を追加した後続31評価、8系列、旧結果との比較、保存mesh再ロード2件の履歴 |
| `sepika416-core-reference` | コア12評価、旧矩形近似との差、保存mesh再ロード1件の履歴、軸表記訂正 |

旧H/Boxの採用時meshには保存不足がある。旧監査の別生成meshと、後続再現時の実solveに対応するmeshを混同しない。
後続31評価の旧採用hash一致は17件であり、残る不一致の原因は未確定のまま保持した。
NPZは収載せず、結果中のhashは実行当時の履歴である。

コアの正しい対応は `x_ref = SEPIKA局所z（幅）`、`y_ref = 局所y（せい）`。
旧rawの逆表記は保持し、`axis-mapping-correction.json` と `corrected-metadata-results.json` を併用する。
訂正はmetadataだけで、J・座標・mesh・残差・時間は同じであり再求解していない。

完全性と既存結果の対応は、標準ライブラリだけで検査できる。

```powershell
python -B dev_docs/v_and_v/data/issue416-reference/package_verify.py
# 原TEMPが残っている場合だけ、コピー元も読み取り照合する。
python -B dev_docs/v_and_v/data/issue416-reference/package_verify.py --check-sources
```

数値再現を行う場合は、本ディレクトリ一式を**新しいTEMP作業先へ複写**し、原本とrepoを出力先にしない。
以下は将来の実行手順であり、収載版での再実行・lockからの新venv再構築は未検証。
Python 3.12.10/Windows AMD64が記録環境で、配布wheelのhashは当該環境の取得物を固定する。

```powershell
$taskCopy = Join-Path $env:TEMP ('issue416-reference-' + [guid]::NewGuid())
Copy-Item -LiteralPath 'dev_docs/v_and_v/data/issue416-reference' -Destination $taskCopy -Recurse
python -m venv (Join-Path $taskCopy 'venv')
$taskPython = Join-Path $taskCopy 'venv/Scripts/python.exe'
& $taskPython -m pip install --index-url https://pypi.org/simple --require-hashes -r (Join-Path $taskCopy 'sepika416-reference-environment/requirements-hashed.lock')
$env:PYTHONDONTWRITEBYTECODE='1'
$env:OMP_NUM_THREADS='1'
$env:OPENBLAS_NUM_THREADS='1'
$env:MKL_NUM_THREADS='1'
& $taskPython (Join-Path $taskCopy 'sepika416-reference-environment/calibrate.py')
& $taskPython (Join-Path $taskCopy 'sepika416-reference-calibration/runner.py')
& $taskPython (Join-Path $taskCopy 'sepika416-reference-calibration/refine.py')
& $taskPython (Join-Path $taskCopy 'sepika416-reference-calibration/public_crosscheck.py')
& $taskPython (Join-Path $taskCopy 'sepika416-reference-reproduction/runner.py')
& $taskPython (Join-Path $taskCopy 'sepika416-core-reference/runner.py')
```

compute runnerは各ディレクトリの設定だけで再計算し、H/Boxとコアは新しいNPZも保存する。
runnerはinspectで導入済みsourceを書き出すため、生成先もTEMPに限る。
旧H/Boxは `runner.py` の後に `refine.py`、校正は上記のrunnerの後にrefineを実行する構成。
元の `audit.py` は歴史的な全環境manifest・旧NPZ等にも依存し、収載物だけでは完走しない。
元bootstrapは未収載のinstall-reportを読み、軸訂正スクリプトは原TEMPとrepoの固定パスを使うため再実行しない。
保存mesh再ロードの履歴は確認できるが、元NPZがないため当時の配列をこのpackageだけで再検査・再solveできない。
再生成meshのhash一致も保証しない。元スクリプトはportable化していない。

配布URL・版・hash・license metadataは [distribution-provenance.json](distribution-provenance.json)、公開一次URLは元の `primary-sources.json` にある。
参照方式はTri6/P2そりNeumann・平均零拘束で、本番Prandtl/P1とは異なる。
公開元の[固定版理論](https://raw.githubusercontent.com/robbievanleeuwen/section-properties/v3.10.2/docs/user_guide/theory.rst)と[固定版source](https://github.com/robbievanleeuwen/section-properties/tree/v3.10.2/src/sectionproperties/analysis)はURLで参照し、本文・vendor source・論文・venvを転載しない。
本番P1との直接比較、現在の代表grid/Windows性能、原Issueの全6Done、最終CI・mergeは本packageの完了範囲に含まない。
