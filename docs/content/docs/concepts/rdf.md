---
title: "RDF"
description: "Bio DatabaseにおけるRDF、Named Graph、N-Quads、Fuseki、SPARQL検索の基本概念"
weight: 30
toc: true
draft: false
---

## RDFを採用した理由

生物オカレンス情報には、同定、採集・観察イベント、位置情報、分類群、メディア、権利情報など、複数の関係があります。RDFはこれらをURIでつなぎ、項目の追加や外部語彙との連携を柔軟に扱えるため採用しています。

## 主語・述語・目的語

RDFは、主語・述語・目的語からなるトリプルで情報を表します。

- 主語: 説明したいリソース
- 述語: 主語と目的語の関係や項目名
- 目的語: 値または関連先のリソース

たとえば、オカレンスの学名は次のように表します。

```turtle
@prefix dwc: <http://rs.tdwg.org/dwc/terms/> .

<https://bio-database.net/occurrences/example>
    dwc:scientificName "Homo sapiens" .
```

## IRIとリテラル

IRIは、リソースを一意に指すURIです。オカレンスやGBIFの分類群など、ほかのデータへ関連付ける場合に使います。リテラルは文字列、数値、日付などの値です。

```turtle
@prefix dwc: <http://rs.tdwg.org/dwc/terms/> .
@prefix dwciri: <http://rs.tdwg.org/dwc/iri/> .

<https://bio-database.net/occurrences/example>
    dwc:scientificName "Homo sapiens" ;
    dwciri:toTaxon <https://www.gbif.org/species/2436436> .
```

## Named Graph

Named Graphは、RDFトリプルをグラフURIごとに分けて管理する仕組みです。Bio Databaseでは、データの種類や出所ごとにグラフを分離します。

### Occurrenceデータ

```text
https://bio-database.net/graphs/occurrences
```

ユーザーが登録したオカレンスのRDFデータを保存するグラフです。

### GBIF Backbone

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

GBIF Backbone TaxonomyをRDFへ変換して保存する分類群グラフです。

## N-Quads

N-Quadsは、主語・述語・目的語に加えてグラフ名も1行で表すRDFの形式です。フロントエンドからオカレンスを登録する際に使用します。

```nq
<https://bio-database.net/occurrences/example>
  <http://rs.tdwg.org/dwc/terms/scientificName>
  "Homo sapiens"
  <https://bio-database.net/graphs/occurrences> .
```

## Fusekiへの保存

APIサーバーは、受け取ったN-Quadsを検証・正規化し、Apache Jena FusekiへSPARQL Updateで保存します。作成者、作成日時、更新日時、公開範囲などの管理情報はバックエンドが付加します。

## SPARQL検索

SPARQLはRDFを検索・更新するためのクエリ言語です。APIサーバーはFusekiに対してSPARQLを発行し、オカレンスの詳細取得、条件検索、分類階層を考慮した検索を行います。
