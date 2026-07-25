# acceptedNameUsage

## 基本情報

IRI
`http://rs.tdwg.org/dwc/terms/acceptedNameUsage`

Label
`Accepted Name Usage`

目的語の形式:リテラル

Definition

The full name, with authorship and date information if known, of the currently valid (zoological) or accepted (botanical) dwc:Taxon.
植物学的に受け入れられている、または動物学的に有効とされている現在、動物学的に有効または植物学的に採用されている分類群の完全な学名。判明している場合は著者名および日付情報を含む。

## 説明

ある分類群で現在採用されている学名を記録するためのもの。dwc:scientificname がつけられたオカレンスデータについて、あとから分類体系の変更などで学名が変わった場合に、acceptedNameUsageを用いることで現在採用されているされる学名を示すことにつかわれたりする。例えば、ある標本についてシノニムがscientific nameとしてついているときに、分類学上採用されている方の名前を示すときに使われたりします。

## Bio-Databaseでの使い方

データ登録で使うことは基本的には無いと思います。一度登録したデータが古い学名になった場合やシノニムとなったときに、新しい学名を示すために使ったりします。このとき、古いdwc:scientificnameは残したままにすることが推奨されています。

## 関連用語

[acceptedNameUsageID](https://bio-database.net/terms/darwin-core/acceptedNameUsageID):目的語がIRIの場合

## alternative label

- 採用名:accepted(botanical)より
- 有効名:valid(zoological)より
