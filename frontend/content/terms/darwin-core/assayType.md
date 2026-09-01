# assayType

## 基本情報

IRI  
`http://rs.tdwg.org/dwc/terms/assayType`

ラベル
`Assay Type`

種別
`Property`

目的語の形式: リテラル（統制語彙の使用を推奨）

## 定義

- 原文 -

A type of method used in a study to detect taxon/taxa of interest in a dwc:MaterialEntity.

`dwc:MaterialEntity`

- 日本語訳 -

dwc_molecularProtocolID から対象となる分類群を検出するために、研究で使用された方法の種類。

## 説明

試料から分類群を検出するために使用した分析方法の種類を記録するための用語です。

詳細な実験手順ではなく、検出方法の大まかな区分を示します。公式例には次の値があります。

- `targeted`：特定の分類群を対象とした検出
- `metabarcoding`：試料中の複数の分類群をまとめて検出する方法
- `other`：その他の方法

## Bio-Databaseでの使い方

環境DNAや組織試料などの分析によって分類群を検出したデータを登録・編集するときに、使用した検出方法の種類を記録します。

例えば、特定の分類群を対象とした検出なのか、メタバーコーディングによる検出なのかを記録します。

目視観察や通常の標本採集によるデータでは、基本的に使用しません。

## 関連用語

- `dwciri:assayType`：分析方法種別を IRI で示す用語
- `dwc:MaterialEntity`：分析対象となる試料
- `dwc:protocolType`：実施した手順の種別

## 別名（日本語）

- アッセイ種別
- 検出方法の種類
- 分析方法の種類

## リンク

[公式の用語一覧](https://dwc.tdwg.org/list/#dwc_assayType)
