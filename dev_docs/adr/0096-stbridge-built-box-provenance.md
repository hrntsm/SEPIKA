# ST-Bridge組立BOXの再出力は原形状の来歴で区別する

Status: accepted

## 判断

ST-Bridgeから取り込んだ組立BOXの原形状を、既存の断面別ST-Bridge入力情報 `StbSectionStrength.built_box` に保持する。この情報は原配筋と同様に幾何の来歴を含む。鋼BOX・角形CFTの現在形状が原形状と完全一致する場合に限り、標準の `StbSecBuild-BOX` を出力する。外高さA・外幅B・左右t1・上下t2をそのまま出力し、半径の補完や板厚の平均化を行わない。

## 理由

モデルの鋼BOX・角形CFTは角R=0でも圧延・組立を区別できない。角Rだけで出力形式を切り替えると、半径0の圧延BOXを組立へ変更して従来の診断を失う。形状に製法フィールドを追加すると計算やUIを含む全ての形状利用側へ影響するため、ST-Bridgeの再出力に必要な来歴だけを既存の入力情報に加える。

## 契約と制約

- 原形状には同厚・異厚の鋼BOX・角形CFTを保持し、OVIKAに保存する。断面の削除・再採番には既存の断面別入力情報の追従処理を用いる。
- 形状の編集後は来歴と現在形状の不一致を診断して書出を拒否する。来歴の自動補正・製法の推定は行わない。
- 圧延BOXの半径未知・0・不正値への書出診断を維持する。製法の来歴がない異厚BOXも書出を拒否する。
- 部材未割当でも用途が定まった断面は同じ条件で書き出す。
- 断面性能の計算方式・異厚BOXの未算定性能は変更しない。

## 標準の根拠

buildingSMART Japanの[公式2.0.2 XSD](https://www.building-smart.or.jp/wp-content/uploads/2026/04/ST-Bridge_v202.zip)で、`StbSecBuild-BOX` はname・A・B・t1・t2が必須でr/type属性を持たない。`StbSecRoll-BOX` はname・type・A・B・t・rが必須。組立は前者へ直接対応させる。
