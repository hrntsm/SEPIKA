# SEPIKA
Integrated Structural Analysis

SEPIKAは、日本語の「一貫計算（IKKAN）」から生まれた構造設計ソフトウェアです。名前はイカに由来し、コウイカを意味するSepiaと日本語のIKAを組み合わせています。

SEPIKA takes its name from Sepia (cuttlefish) and IKA, inspired by the Japanese phrase "Ikkan Keisan" (integrated structural calculation).

## はじめに

**SEPIKA** は、Rust で実装された日本の建築構造計算一貫プログラムです。

このサイトは、SEPIKA を利用する方に向けたドキュメントです。

利用目的に合わせて、次の章から参照してください。

- [基本操作・モデル入力](model_edit/README.md)、[準備・解析の実行・結果の確認](workflow/README.md)、[断面案の検討](section_study/README.md)
- [荷重・重量・質量](calc_basis/01_荷重/README.md)、[材料・断面諸量・弾性剛性](elastic/README.md)、[非線形モデル・復元力](nonlinear/README.md)、[構造解析の方法](calc_basis/05_構造解析/README.md)
- [許容応力度・部材検定](calc_basis/06_一次設計/README.md)、[部材終局耐力](calc_basis/08_部材終局耐力/README.md)、[建物全体の耐震評価](calc_basis/07_二次設計/README.md)
- [数量積算](calc_basis/10_数量積算/README.md)、[保存・入出力・外部AI連携](external/README.md)

式・単位・出典と実装参照の読み方は[計算根拠の読み方](calc_basis/README.md)にまとめています。

## ライセンスと免責事項

SEPIKA は MIT License のもとで公開しているソフトウェアです。
ライセンスの全文はリポジトリの [LICENSE](https://github.com/hrntsm/SEPIKA/blob/main/LICENSE) にあります。

```
MIT License

Copyright (c) 2026 Hiroaki NATSUME
```

### 無保証（AS IS）

MIT License は、ソフトウェアを「現状のまま（AS IS）」提供し、明示・黙示を問わずいかなる保証も行わないことを条件としています。

> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.

この無保証は、SEPIKA が出力する応力、変形、検定比、保有水平耐力などの計算結果にもそのまま及びます。
つまり SEPIKA は、計算結果の正確性、妥当性、および特定の目的への適合性を、いずれも保証しません。
また、SEPIKA の使用または使用不能によって生じたいかなる損害についても、著作権者および貢献者は責任を負いません。

本ドキュメントについても同じです。
このサイトは、各計算がどの法令・規準のどの式に基づいて算定されているかを説明するものであり、個々の建築物についてその結果が正しいことや、適法であることを保証するものではありません。

### 利用にあたって

SEPIKA は、国土交通大臣の認定を受けた構造計算プログラム（いわゆる大臣認定プログラム）ではありません。
そのため、確認申請における認定プログラムとしての取扱い（大臣認定プログラムを用いた場合の審査の特例）は受けられません。

入力データの妥当性、モデル化の適否、そして出力された結果の検証は、いずれも利用者の責任に属します。
計算結果は、手計算や理論解、ほかのプログラムとの突合などによる利用者自身の検証を経たうえで使用してください。
設計における式の適用可否（適用範囲や前提条件）の判断も、設計者（構造設計一級建築士等）の責任に属します。

## 開発者向け資料

設計判断・検証記録・開発運用ドキュメントは開発者向けのため本サイトには含めていません。
これらは [dev_docs/](https://github.com/hrntsm/SEPIKA/tree/main/dev_docs) に集約しており、リポジトリの以下を参照してください。

- [dev_docs/architecture.md](https://github.com/hrntsm/SEPIKA/blob/main/dev_docs/architecture.md)：クレート階層と依存方向
- [dev_docs/v_and_v/](https://github.com/hrntsm/SEPIKA/tree/main/dev_docs/v_and_v)：各要素・各設計式の Verification & Validation レポート（[未検証一覧](https://github.com/hrntsm/SEPIKA/blob/main/dev_docs/v_and_v/未検証一覧.md)）。法令・規準の埋め込み値の照合チェックリスト [原典照合リスト](https://github.com/hrntsm/SEPIKA/blob/main/dev_docs/v_and_v/原典照合リスト.md) も同ディレクトリにあります
- [GitHub Issues](https://github.com/hrntsm/SEPIKA/issues)：未完了の開発作業・不具合報告・提案
- [dev_docs/handoff/](https://github.com/hrntsm/SEPIKA/tree/main/dev_docs/handoff)：過去の実装経緯・判断の履歴と申し送り目録。[ROADMAP.md](https://github.com/hrntsm/SEPIKA/blob/main/dev_docs/handoff/ROADMAP.md) は 2026-07 完了済みの歴史的記録

## リポジトリ

ソースコードは [github.com/hrntsm/SEPIKA](https://github.com/hrntsm/SEPIKA) にあります。

ビルド・テスト・静的解析の手順はリポジトリの [CONTRIBUTING.md](https://github.com/hrntsm/SEPIKA/blob/main/CONTRIBUTING.md) を参照してください。
