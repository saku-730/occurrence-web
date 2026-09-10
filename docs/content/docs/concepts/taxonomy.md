---
title: "Taxonomy"
description: "GBIF Backbone Taxonomyの基本概念とBio DatabaseへのRDF変換・Fuseki投入処理"
weight: 40
toc: true
draft: false
---

## GBIF Backbone Taxonomyとは何か

GBIF Backbone Taxonomyは、GBIFが提供する統合的な分類群データセットです。多様なデータ提供元の分類情報を整理し、分類群ごとに安定した識別子を提供します。

## 分類候補として使う理由

学名の文字列だけでは、表記揺れ、同名異物、分類体系の違いを十分に扱えません。GBIF Backbone Taxonomyの分類群URIを記録することで、入力候補の統一と分類群を起点とする検索を可能にします。

## scientificNameとcanonicalName

`scientificName` は、著者名や命名情報を含む完全な学名表記です。`canonicalName` は、著者名などを除いた基本となる学名です。用途に応じて、表示や検索の対象を区別できます。

## taxonKey

`taxonKey` は、GBIF Backbone Taxonomy内で分類群を識別する数値IDです。Bio Databaseでは、分類群URIを `https://www.gbif.org/species/{taxonKey}` の形式で記録します。

## 親分類群

分類群は、属・科・目などの上位分類群と親子関係を持ちます。この関係を使い、ある分類群とその下位分類群を含めた階層検索を行えます。

## シノニム

シノニムは、現在受容されている分類群とは別名として扱われる分類群名です。GBIF Backbone Taxonomyの関連を利用することで、シノニムから受容名へたどることができます。

## 分類階級

分類階級は、界、門、綱、目、科、属、種など、分類体系における位置を表します。分類群データには各分類群の階級情報が含まれます。

## accepted taxon

accepted taxonは、GBIF Backbone Taxonomyで受容されている分類群です。シノニムや誤用名は、対応するaccepted taxonとの関係を持ちます。

## 実装上の特徴

Bio Databaseでは、GBIF Backbone Taxonomyの配布データをRDFとして利用するため、次の処理を行います。

1. GBIF Backboneの配布データを取得する
2. RustでTSVをN-Quadsへ変換する
3. 約20GB規模のRDFを生成する
4. riotでRDF構文を検証する
5. TDB2 LoaderでFusekiへ一括投入する

投入先のNamed Graphは次のURIです。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```
