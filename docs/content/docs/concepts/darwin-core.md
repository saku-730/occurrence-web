---
title: "Darwin Core"
description: "Bio DatabaseにおけるDarwin Core語彙、dwcとdwciri、リテラルとIRIの正規化"
weight: 20
toc: true
draft: false
---

## Darwin Coreとは何か

Darwin Coreは、生物多様性データを記述するための標準語彙です。分類、採集・観察イベント、位置情報、標本、同定などに共通の項目名を提供します。

## このアプリで採用している理由

Bio Databaseでは、プロジェクトごとに項目名や意味がばらつくことを避けるため、Darwin Coreを主要な入力語彙として採用しています。共通語彙を使うことで、登録したデータを検索・共有・再利用しやすくします。

## dwcとdwciriの違い

`dwc` は主にリテラル値を取るDarwin Core Termsの語彙です。`dwciri` は主にIRIを目的語として関連先のリソースを指すDarwin Core RDFの語彙です。

- `dwc:scientificName`: 学名を文字列として記録する
- `dwciri:toTaxon`: 分類群をURIとして関連付ける

## リテラル値とIRI値

リテラル値は、学名や注記のような文字列・日付・数値です。IRI値は、GBIFの分類群ページのように、別のリソースを一意に指すURIです。

```turtle
@prefix dwc: <http://rs.tdwg.org/dwc/terms/> .
@prefix dwciri: <http://rs.tdwg.org/dwc/iri/> .

<https://bio-database.net/occurrences/example>
    dwc:scientificName "Homo sapiens" ;
    dwciri:toTaxon <https://www.gbif.org/species/2436436> .
```

この例では、`"Homo sapiens"` はリテラル値で、GBIFの分類群ページはIRI値です。

## 入力語彙の正規化

フロントエンドではDarwin Coreの項目として入力します。バックエンドは目的語がリテラルかIRIかを判定し、Fusekiに保存した語彙メタデータを参照します。

対象の述語に `objectKind` が定義され、目的語の型と異なる場合は、`iriEquivalent` または `literalEquivalent` があれば対応する述語へ書き換えます。これにより、IRIを取る関係は `dwciri` の語彙として正規化できます。

## 未対応語彙の扱い

`objectKind`、`iriEquivalent`、`literalEquivalent` が語彙メタデータに存在しない場合、バックエンドは述語を変換せず、入力された語彙をそのまま保存します。混在型の語彙も変換対象にしません。
