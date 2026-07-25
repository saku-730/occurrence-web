# acceptedNameUsageID

## 基本情報

IRI
`http://rs.tdwg.org/dwc/terms/acceptedNameUsageID`

Label
`Accepted Name Usage ID`

目的語の形式: リテラル

Definition

An identifier for the name usage of the currently valid (zoological) or accepted (botanical) taxon.

現在、動物学的に有効または植物学的に採用されている分類群の名前の用法を識別する識別子。

## 説明

シノニムや誤用名に対応する、採用名または有効名の分類群レコードを識別するための項目です。

`acceptedNameUsage`が採用学名を文字列で記録するのに対し、`acceptedNameUsageID`にはGBIFのtaxonKeyなどの識別子を記録します。

## Bio-Databaseでの使い方

利用者が学名候補を選択した際、その名前がシノニムであれば、対応する採用分類群のIDを自動的に設定する用途に使えます。

フロントでは通常の自由入力欄にはせず、次のような場面で使用します。

* 選択した学名がシノニムであることを表示する
* 対応する採用学名を表示する
* 採用分類群の詳細ページへ移動する
* GBIF Backboneの分類群と紐づける

基本的には、学名候補の選択結果をもとにシステム側で設定します。

## 関連用語

* `dwc:acceptedNameUsage`: 採用学名の文字列
* `dwc:scientificName`: 対象レコードに記録されている学名
* `dwc:taxonID`: 分類群レコード自体の識別子
* `dwc:taxonomicStatus`: acceptedやsynonymなどの分類学的状態

## alternative label

* 採用学名ID
* 採用名ID
* 有効名ID