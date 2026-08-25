# assayType

## 基本情報

IRI  
`http://rs.tdwg.org/dwc/iri/assayType`

ラベル
`Assay Type (IRI)`

種別
`Property`

目的語の形式: IRI（検出方法の種類を表すリソース）

## 定義

- 原文 -

A type of method used in a study to detect taxon/taxa of interest in a dwc:MaterialEntity.

`dwc:MaterialEntity`

- 日本語訳 -

に含まれる対象分類群を検出するために、研究で使用された方法の種類。

## 説明

試料から分類群を検出するために使用した分析方法の種類を示す用語です。

詳細な実験手順ではなく、検出方法の大まかな区分を記録します。公式例には次のものがあります。

- `targeted`：特定の分類群を対象とする検出
- `metabarcoding`：試料に含まれる複数の分類群をまとめて検出する方法
- `other`：その他の方法

`dwciri:assayType`では、これらの種類を表すIRIを目的語に使用します。

## Bio-Databaseでの使い方

環境DNAや組織試料などを分析して分類群を検出したデータを登録・編集するときに、使用した検出方法の種類を記録します。

例えば、特定の分類群を対象とした検出なのか、メタバーコーディングによる検出なのかを記録するために使用します。

目視観察や通常の標本採集によるオカレンスデータでは、基本的に使用しません。

## 関連用語

- `dwc:assayType`：分析方法種別を文字列で記録する用語
- `dwc:MaterialEntity`：分析対象となる試料
- `dwc:protocolType`：実施した手順の種別

## 別名（日本語）
- アッセイ種別
- 検出方法
- 検出法種別

## リンク

[公式の用語一覧](https://dwc.tdwg.org/list/#dwciri_assayType)
