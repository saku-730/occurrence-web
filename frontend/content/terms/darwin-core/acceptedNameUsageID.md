# acceptedNameUsageID

## 基本情報

IRI  
`http://rs.tdwg.org/dwc/terms/acceptedNameUsageID`

Label  
`Accepted Name Usage ID`

目的語の形式: リテラル（識別子）

Definition

An identifier for the name usage (documented meaning of the name according to a source) of the currently valid (zoological) or accepted (botanical) taxon.

現在、動物学的に有効または植物学的に採用されている分類群について、その名前の用法を識別するための識別子。

## 説明

シノニムや誤用名に対応する、現在の採用名または有効名の分類群レコードを識別するための用語です。

`acceptedNameUsage`には採用学名を文字列で記録し、`acceptedNameUsageID`にはその採用名に対応する分類群の識別子,IDを記録します。

識別子には、GBIFのtaxonKey、Catalogue of Lifeの識別子、LSIDなどを使用できます。例えば、シマミミズの`acceptedNameUsage`は`Eisenia fetida`で`acceptedNameUsageID`は`GBIF key:5815560`です。

## Bio-Databaseでの使い方

`acceptedNameUsage`同様、通常のデータ登録では使うことはあまりないと思います。
データ編集の際に、過去に登録した分類がシノニムになり、新たな対応する採用分類群を指定する必要がある場合に、使うことができます。IDであることに注意が必要です。

## 関連用語

- `dwc:acceptedNameUsage`：現在の採用学名または有効学名
- `dwc:scientificName`：対象となる分類群の学名
- `dwc:taxonID`：分類群レコード自体の識別子
- `dwc:taxonomicStatus`：採用名、シノニム、誤用名などの分類学的状態
- `dwc:nameAccordingTo`：名前の用法が基づいている情報源

## alternative label

- 採用分類群ID
- 採用学名ID
- 採用名ID
- 有効名ID
