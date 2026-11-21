# ST-Bridge は完全往復を保証しない

Status: superseded by ADR-0141

ST-Bridge の取り込みは幾何・断面・部材・荷重までを対象とし、書き出しは荷重を含まない（解析結果・独自属性も対象外）。完全往復は保証しない。取り込みは「ST-Bridge の構造を写す」処理ではなく「正しい内部モデルへ変換する」処理と位置づけ、往復互換性が下がっても内部モデルの正しさを優先する。

Legacy source:
- 初回設計書（#313 で廃止。git 履歴参照）§12.5
- [廃止前の利用マニュアル](https://github.com/hrntsm/SEPIKA/blob/92355319350152b75e39ea1a8cd88cb779301fef/docs/model_io/02_ST-Bridge_%E5%BD%A2%E5%BC%8F.md)（荷重は取り込みのみ）
