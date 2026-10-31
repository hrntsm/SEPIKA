# 線材荷重・端部変換の現役試験参照（#1059）

対象は `sepika-element` のmember_load/multi_spring/panel_offset/prismaticのE045–E051である。式・積分/加算順・公差・荷重所有・入力診断順・commit/revert/rollbackを保持する。新たな構造安全性や実験Validationの認証は行わない。

## 現役の保証

以下のパスは `crates/sepika-element/src/frame/` 配下である。

| 元の対象 | 現役の試験・実装 | 保持する保証 |
| --- | --- | --- |
| `member_load.rs::brace_fixed_internal_has_no_bending` | `brace_udl_has_no_end_moment_and_preserves_resultant` | 同じL1000/w2/水平frame/等分布荷重で、全5xi=(0,.25,.5,.75,1)×内力5成分のabs<1e-9を移設。材端6モーメント0、両端荷重半分、方式間の合力保存と元の公差は保持 |
| MSの同じ初期fixture再生成 | `multi_spring.rs::test_ms_phi_positive_objectivity_and_tangent_consistency` | 実際に客観性を検証する同じMSの両φ>0を確認する。全12内力の客観性、4列×12行の接線FD・元h1e-6/誤差1e-6を保持 |
| MSの同じ増分再生成 | `multi_spring.rs::test_ms_commit_revert_roundtrip` | 同じ12成分duを2回借用。commit=true→false→revert→zero増分の順と全12成分epsilon1e-6を保持 |
| パネル荷重応答の重複変換 | `panel_offset.rs::transform_load_response` | 内側12DOF検証→外側DOFへresize→coupling順加算→有限性検証を共用。線形取得/状態取得の差、外側変位検証、所有時Noneの異なる診断は元の経路に保持。公開LoadedFrameResponseの物理端力12成分・可撓面力12成分・断面力場はそのまま保持 |
| K-onlyの未使用M計算 | `prismatic.rs::condense_expanded` | 質量なしではrigid質量計算と後段RᵀMRを省く。ばね付加・非駆動共通ねじりゲージ・active・CheckedDenseLu・Kstar・特異/非有限判定は不変。質量ありではゲージを除かず、元のRとM全成分・加算順を保持 |
| test-only質量adapterの不要構造体 | `prismatic.rs::consistent_mass` | mass/lを直接渡す。l<=0の零行列、torsion_term×6/lの非負境界、φ=0、全oracleと使用側を保持 |
| `ティモシェンコ質量は回転慣性とphiを曲げ面へ反映する` / `ティモシェンコ質量のphi零極限は全成分で標準係数に一致する` | `prismatic.rs::ティモシェンコ質量はphi正と零で独立参照の全成分と一致する` | 同じ質量特性(2,3,4)、L1000、φ=(.8,.6)/(0,0)、独立8点積分の全144成分と各25入力PSD（旧36回のunique集合と順序を保持）。誤差上界は元の2e-10/1e-10×(1+abs(expected))のまま |

パネル応答は[傾斜パネル投影の既存回帰](../../crates/sepika-element/src/frame/panel_offset.rs)の `panel_757_loaded_residual_and_state_use_the_same_sloped_projection` が、線形照会・同じ状態応答・仕事・全残差・荷重失敗時checkpoint復元を照合する。K-only/Mありの差は `prismatic.rs::非駆動共通ねじりだけを除き実ばねと他の特異性は保持する` が、全6解放・共通ねじり・他の特異性・微小連成・実ばね・NaNの受理/拒否を区別する。これらの独立契約は統合せず保持する。

過去の固定SHA・raw・実行件数・V&V状態は保持する。[整合質量の現役表](整合質量_2026-09.md#端部縮約と質量試験の現役参照1059)と併せて参照する。

## 検証状態

2026-10-31、基点 `f3692094e4b0a7b79f29de2d78d9a59d4e619f4d` に本未commit差分を適用して実行した。

| 検証 | 結果 |
| --- | --- |
| `cargo test -p sepika-element --locked` | 540成功・失敗0・ignore0、Rustdoc0件成功 |
| `cargo test -p sepika-solver -p sepika-job --locked` | 630成功・失敗0、既存監査用ignore1件、両Rustdoc0件成功 |
| `cargo clippy -p sepika-element --all-targets --locked -- -D warnings` | 成功 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p sepika-element --no-deps --locked` | 成功 |
| `cargo fmt --all -- --check`、`git diff --check` | 成功 |
| `cargo run -p xtask --locked -- check-docs`、変更V&Vのリンク・実名確認 | 成功 |

対象element/solver/jobに非default feature定義はない。ignoreは `audit_rc_wall_opening_linear_response`（#821監査用）であり成功件数へ含めない。独立レビュー・CI・Validationはこのローカル実行とは別の証拠である。


## 最新main取込後の確認

基点を `7ea16fb46dfe32d4572e4c102c12c7e2fc59109f` へfast-forwardした。本バッチの4 Rustファイルの内容と差分はFF前後で完全一致する。READMEはパネル関連記事直後の通常目録1行に集約し、その1行を除けば最新main本文と完全一致することを確認した。最新基点ではdiffcheck・check-docs・文書リンク/実名確認を行い、上表の540/630件と静的解析等は旧 `f3692094` 基点での結果として保持する。全suiteを最新基点で再実行した結果ではない。


同基点7ea16fb4で、MSの既定shear=Noneへの同値再代入と不要mutだけを除去した後、`test_ms_phi_positive_objectivity_and_tangent_consistency` のexact対象1件とfmt/diffcheckが成功した。全入力・assert・公差は保持し、旧基点での全suite結果とは区別する。


PSDの36回中11回は周期13の同一ベクトルだったため、旧unique集合25個と残した入力の評価順を保持して各25回へ整理した。二次形式の評価式・加算順・公差とφ正/零の144成分比較は不変。修正後、同7ea基点の対象exact1件・fmt/diffcheck・文書検証が成功した。重複除去前の独立freshは同7ea基点でelement540件・solver/job619件（既存ignore1）とRustdoc成功であり、旧f369の540/630件および今回修正後の限定実行とは区別する。
