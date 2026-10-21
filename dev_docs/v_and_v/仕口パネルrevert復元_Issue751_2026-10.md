# 仕口パネルの直接revert復元（Issue #751）

## 対象と変更範囲

2026-10-21、基点 `c35d07d3` の [Issue #751](https://github.com/hrntsm/SEPIKA/issues/751) を対象とする。現行の計算モデル・制限は[製品の計算根拠](../../docs/calc_basis/04_要素剛性/06_パネルゾーン.md)を正本とする。[Issue724の調査](仕口パネル軸力と履歴_Issue724_2026-10.md)の赤再現・原典取得不能・未検証範囲は、その時点の履歴として保持する。

`PanelZone::revert_state` で柱の試行変位を確定変位へ戻した後に、既存の `apply_axial_interaction` で材料降伏値を戻す1行を追加した。既存の材料trial復元と合わせ、変位・降伏値・塑性履歴を同じ確定状態へ戻す。降伏式・2%下限・採用柱規則・移動硬化則・非線形算法・snapshot/checkpointの保存形式は変更しない。

## 直接Verification

`crates/sepika-element/src/springs/panel.rs` の `cross_joint_model` と独立寸法の `panel_724_reference_scales` を使う。柱L=3000 mm、E=205000 N/mm²、F=235 N/mm²、A=21870 mm²、dc=379 mm、tp=13 mm、db=583 mm。独立にK=226480925000 N·mm、M0=373111832.4986924 N·mm、γ0=M0/K=0.0016474315993662266 radを得る。x=γ/γ0、m=M/M0、二次勾配比0.01とする。

n=0.6の降伏比は0.8なので、処女載荷のx=1.6では `m=0.8+0.01(1.6−0.8)=0.808`。同じ角度でn=0.8へ未確定試行すると、直前確定塑性角0.792を保持しつつ、降伏比0.6に縮小した応答m=0.61となる。これを取り消せば、直前の確定値m=0.808に戻る必要がある。正負反転・二方向独立の応答にも同じ契約を課す。数値は現行関数の出力を期待値へ転記せず、区分直線から算定した。降伏角付近の接線の片側選択は新たに規定しない。

| 恒久試験名（`panel_751_`以下） | 入力と観測契約 |
| --- | --- |
| `revert_restores_committed_moment_after_axial_trial` | 元Issueのx=1.6,n=0.6,m=0.808確定→同角度n=0.8,m=0.61試行→直接revert。n=0.6・公開降伏比0.8・内力比0.808を絶対許容10⁻¹²で確認 |
| `revert_and_retry_match_control_in_both_directions` | X±・Y±・X正/Y負同時の5条件。n=0.8/0.3へ変える別試行を各々破棄し、連続2回revertも確認。同じ確定点から除荷・反転・増載と軸力変化を含む3種類の再試行を、失敗試行のない対照と比較 |
| `restored_uncommitted_snapshot_and_checkpoint_can_revert` | n=0.6塑性確定後、n=0.8の未確定状態をsnapshot/checkpointへ保存。別の塑性状態を確定してから保存状態を完全復元し、その後直接revertと再試行を確認。保存時のtrialとcommittedを区別 |

対照比較は軸力比・公開降伏値・パネルモーメント・局所14成分の内力・14×14全接線を完全一致で検証する。checkpointの全バイト一致も要求し、パネル/柱の確定・試行変位、両材料の降伏値・確定/試行塑性履歴を含む保存状態の欠落を検出する。snapshot/checkpointの既存確定状態復元試験（Issue724）も維持する。

## 実際の利用経路と計算契約版

コード読取と `rg` による呼出箇所確認では、`sepika-solver/src/common/transaction.rs` の `revert_all` は定義のみで、solver内に呼出しはない。増分解析の `nonlinear/pushover/driver.rs` の刻み失敗・重力ステップ失敗、非線形時刻歴の `dynamic/timehistory/nonlinear.rs` の刻み失敗は `StateSnapshot::restore` を用いる。共通要素更新の `common/elem_loop.rs` も同じsnapshot復元を用い、最終的に `PanelZone::restore_state` が材料全体を復元する。今回変更した直接revertを通らない。

したがって、本修正から公開solverの保存成果が変化する経路は確認されず、解析契約版35・検定契約版21を維持する。直接要素APIの振る舞いは修正するが、公開solverの解析式・準備・適用条件や検定式は変えない。強制未収束を伴う骨組試験や保存OVIKAの差分を実行確認したという意味ではない。

## 実行証拠

専用worktree `issue751`、共有target `C:/Users/hiroa/Desktop/ikkan/target` で実行した。raw logは同targetに保存する。

- 修正前 `cargo test -p sepika-element --locked panel_751 -- --nocapture`：`issue751-red.log`、0成功・3失敗・508除外。元の0.808復元契約と対照比較の不整合を検出した。修正前の失敗を成功として扱わない。
- 修正後 `cargo test -p sepika-element --locked`：`issue751-element.log`、511成功・失敗/ignore 0、doc-test 0。

- `cargo test -p sepika-solver --locked`：`issue751-solver.log`、単体317＋統合63の合計380成功・失敗/ignore 0、doc-test 0。
- `cargo clippy -p sepika-element -p sepika-solver --all-targets --locked -- -D warnings`：`issue751-clippy.log`、成功。親が最新stable更新済みと確認したrustc 1.99.1 / Clippy 0.1.99を使用。
- `cargo fmt --all -- --check`：`issue751-fmt.log`、成功。`git diff --check` も成功。
- `cargo run -p xtask --locked -- check-docs`：`issue751-doccheck.log`、123ページ・607リンク・656実装参照で成功。
- worktreeルートの `mdbook build`：`issue751-mdbook.log`、成功。

独立レビュー・最終head CI・mergeは記録時点で未確認。

## 残る範囲

このVerificationは直接復元と再試行の状態整合を確認する。変動軸力下の連成接線・軸方向仕事 [#752](https://github.com/hrntsm/SEPIKA/issues/752)、高軸力2%下限 [#753](https://github.com/hrntsm/SEPIKA/issues/753)、全体未収束方針 [#714](https://github.com/hrntsm/SEPIKA/issues/714)の全面検証は含まない。一般荷重・塑性柱を含む軸力推定の全経路一致、規準原典への適合、実験Validationも未確認であり、今回の復元成功から接合部モデル全体の妥当性を主張しない。新たな安全側/危険側の近似は導入していない。
