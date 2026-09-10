---
title: "Data Model"
description: "Occurrenceを中心にIdentification、Event、Location、MaterialSample、Media、Projectを関連付けるデータモデル"
weight: 10
toc: true
draft: false
---

## 全体像

Bio Databaseでは、Occurrenceを記録全体の起点として、関連する情報をそれぞれの役割に応じて分離します。

```text
Occurrence
├── Identification
├── Event
├── Location
├── MaterialSample
├── Media
└── Project
```

現行実装では、Identification、Event、LocationをOccurrenceから独立した中間ノードとして保存します。Mediaは外部メディアURIとして関連付けます。MaterialSampleとProjectは、このモデルを拡張するための概念です。

## Occurrence

記録全体の起点です。以下の情報と、ほかの構成要素との関連を保持します。

- 誰が記録を作成したか
- 公開範囲
- 作成日時
- 更新日時
- 他の構成要素との関連

## Identification

同定結果に関する情報です。

- 科学名
- 同定者
- 同定日
- 同定注記
- 分類群IRI
- GBIF Backboneとの関連

## Event

採集または観察が行われた出来事に関する情報です。

- 採集・観察日時
- 採集方法
- 採集努力量
- 生息環境
- フィールド番号

## Location

記録地点に関する情報です。

- 緯度
- 経度
- 測地系
- 座標誤差
- 地名
- 国
- 市区町村

## MaterialSample

標本などの物理的な試料に関する情報です。

- 標本番号
- 機関コード
- コレクションコード
- 標本作製方法

## Media

オカレンスに関連付ける画像、音声、動画などのファイルに関する情報です。

- 画像URI
- 作成者
- ライセンス
- 権利情報

## Project

複数のオカレンスを、調査・研究・収集などのまとまりとして関連付けるための概念です。
