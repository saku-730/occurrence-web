# assayType

## 基本情報

IRI  
`http://rs.tdwg.org/dwc/terms/assayType`

Label  
`Assay Type`

目的語の形式: リテラル（統制語彙の使用を推奨）

Definition

A type of method used in a study to detect taxon/taxa of interest in a dwc:MaterialEntity.

`dwc:MaterialEntity`から対象となる分類群を検出するために、研究で使用された方法の種類。

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

- `dwciri:assayType`：検出方法の種類をIRIで記録する用語
- `dwc:MaterialEntity`：分析や検出の対象となる物理的な試料

## alternative label

- アッセイ種別
- 検出方法の種類
- 分析方法の種類