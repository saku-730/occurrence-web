# GBIF Backbone gzip TSV to N-Quads Converter

GBIF Backbone 由来の分類マスタ TSV を gzip 圧縮したファイルを読み込み、Apache Jena Fuseki / TDB2 に投入しやすい **N-Quads** 形式へ変換する Rust スクリプトです。

入力は `.gz` ファイルを想定しています。
gzip を事前に展開する必要はありません。

## 目的

このスクリプトは、GBIF Backbone の分類データを Web アプリ内部で参照しやすい RDF マスタデータに変換します。

出力は N-Quads 形式なので、named graph を保持したまま Fuseki に投入できます。

分類マスタ用 named graph は次を使用します。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

## 入力ファイル

入力は、**gzip 圧縮された TSV ファイル**です。

例:

```text
simple.tsv.gz
gbif-backbone.tsv.gz
name_usage_export.txt.gz
```

ファイル内部の内容は TSV 形式です。

つまり、次のような構造を想定します。

```text
gzip compressed file
└── TSV text data
```

スクリプト側で gzip を展開しながら読み込むため、次のような事前展開は不要です。

```bash
gzip -dc input.tsv.gz > input.tsv
```

## 想定するTSV列

想定する元データは、GBIF SQL から `name_usage u`、`name n`、`citation cpi` などを JOIN して作成した簡略ファイルです。

主に次の列を利用します。

```text
u.id
u.parent_fk
u.basionym_fk
u.is_synonym
u.status
u.rank
u.nom_status
u.constituent_key
u.origin
u.source_taxon_key
u.kingdom_fk
u.phylum_fk
u.class_fk
u.order_fk
u.family_fk
u.genus_fk
u.species_fk
n.id as name_id
n.scientific_name
n.canonical_name
n.genus_or_above
n.specific_epithet
n.infra_specific_epithet
n.notho_type
n.authorship
n.year
n.bracket_authorship
n.bracket_year
cpi.citation as name_published_in
u.issues
```

このうち、RDF 変換で特に重要な列は次です。

| TSV列                | 用途                   |
| ------------------- | -------------------- |
| `u.id`              | Taxon URI の識別子       |
| `u.parent_fk`       | 親分類群へのリンク            |
| `u.basionym_fk`     | basionym へのリンク       |
| `u.is_synonym`      | シノニム判定               |
| `u.status`          | taxonomic status     |
| `u.rank`            | taxon rank           |
| `u.nom_status`      | nomenclatural status |
| `u.kingdom_fk`      | kingdom 階層へのリンク      |
| `u.phylum_fk`       | phylum 階層へのリンク       |
| `u.class_fk`        | class 階層へのリンク        |
| `u.order_fk`        | order 階層へのリンク        |
| `u.family_fk`       | family 階層へのリンク       |
| `u.genus_fk`        | genus 階層へのリンク        |
| `u.species_fk`      | species 階層へのリンク      |
| `n.scientific_name` | 学名                   |
| `n.canonical_name`  | canonical name       |
| `n.authorship`      | 命名者表記                |
| `n.year`            | 公表年                  |
| `cpi.citation`      | name published in    |
| `u.issues`          | GBIF 側の issue 情報     |

## 出力形式

出力は N-Quads です。

各行は、概ね次の形になります。

```nquads
<subject> <predicate> <object> <graph> .
```

例:

```nquads
<https://bio-database.net/taxa/gbif-backbone/2435099> <http://rs.tdwg.org/dwc/terms/scientificName> "Felis catus Linnaeus, 1758" <https://bio-database.net/graphs/taxonomy/gbif-backbone> .
<https://bio-database.net/taxa/gbif-backbone/2435099> <http://rs.tdwg.org/dwc/terms/canonicalName> "Felis catus" <https://bio-database.net/graphs/taxonomy/gbif-backbone> .
<https://bio-database.net/taxa/gbif-backbone/2435099> <http://rs.tdwg.org/dwc/terms/taxonRank> "SPECIES" <https://bio-database.net/graphs/taxonomy/gbif-backbone> .
```

## URI設計

Taxon URI は、TSV の `u.id` を元に生成します。

```text
https://bio-database.net/taxa/gbif-backbone/{u.id}
```

例えば、`u.id = 2435099` の場合は次の URI になります。

```text
https://bio-database.net/taxa/gbif-backbone/2435099
```

親分類群や各階層の参照も、同じ URI 体系で表現します。

```nquads
<https://bio-database.net/taxa/gbif-backbone/2435099> <http://rs.tdwg.org/dwc/terms/parentNameUsageID> <https://bio-database.net/taxa/gbif-backbone/2435098> <https://bio-database.net/graphs/taxonomy/gbif-backbone> .
```

## 主なRDFマッピング

代表的なマッピングは次の通りです。

| TSV列                | RDF predicate                  |
| ------------------- | ------------------------------ |
| `u.id`              | `dwc:taxonID`                  |
| `u.parent_fk`       | `dwc:parentNameUsageID`        |
| `u.basionym_fk`     | `dwc:originalNameUsageID`      |
| `u.status`          | `dwc:taxonomicStatus`          |
| `u.rank`            | `dwc:taxonRank`                |
| `u.nom_status`      | `dwc:nomenclaturalStatus`      |
| `n.scientific_name` | `dwc:scientificName`           |
| `n.canonical_name`  | `dwc:canonicalName`            |
| `n.authorship`      | `dwc:scientificNameAuthorship` |
| `cpi.citation`      | `dwc:namePublishedIn`          |
| `u.issues`          | 内部用 issue predicate            |

`dwc:` は Darwin Core Terms を指します。

```text
http://rs.tdwg.org/dwc/terms/
```

## 使い方

### ビルド

```bash
cargo build --release
```

### 実行

入力には gzip 圧縮された TSV ファイルを指定します。

```bash
cargo run --release -- input.tsv.gz output.nq
```

例:

```bash
cargo run --release -- gbif-backbone.tsv.gz gbif-backbone.nq
```

または、ビルド済みバイナリを直接実行します。

```bash
./target/release/tsv-to-nquads gbif-backbone.tsv.gz gbif-backbone.nq
```

通常実行では全文変換を行うが、途中まででいい場合は以下の用に範囲となる行番号を引数につける

```bash
cargo run --release -- gbif-backbone.tsv.gz gbif-backbone.nq 200
```

## 入力ファイルについての注意

このスクリプトは gzip ファイルを直接読む前提です。

そのため、次のように展開済み TSV を渡す使い方は基本的に想定していません。

```bash
cargo run --release -- gbif-backbone.tsv gbif-backbone.nq
```

展開済み TSV を扱いたい場合は、別途 TSV 直接入力に対応する処理を追加してください。

## Fusekiへの投入

基本的に、サイズが大きすぎるので、web API経由ではやめよう。20GBも送っていたらメモリエラーとかおきるだろうし。

データ検証。

```bash
docker compose run --rm \
  -v "$PWD/tools/gbif-backbone-to-rdf:/workspace:ro" \
  --entrypoint riot \
  fuseki \
  --validate /workspace/gbif-backbone.nq
```

データ投入。

```bash
docker compose run --rm \
  -v "$PWD/tools/gbif-backbone-to-rdf:/workspace:ro" \
  --entrypoint tdb2.tdbloader \
  fuseki \
  --loader=phased \
  --loc /fuseki/databases/occurrence \
  /workspace/gbif-backbone.nq
```

N-Quads には named graph が含まれているため、投入後は次の graph に分類マスタが入ります。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

## 変換方針

このスクリプトでは、gzip 内部の TSV の 1 行を 1 taxon として扱います。

基本方針は次の通りです。

* gzip 圧縮された TSV を直接読み込む
* `u.id` を taxon の一意な識別子として使う
* `u.parent_fk` がある場合は親分類群への参照を出力する
* `n.scientific_name` を学名として出力する
* `n.canonical_name` を canonical name として出力する
* `u.rank` を taxon rank として出力する
* `u.status` を taxonomic status として出力する
* 空文字や NULL 相当の値は RDF に出力しない
* すべての quad に taxonomy graph URI を付与する
* occurrence data とは別 named graph に保存する

## 注意点

このスクリプトは、GBIF Backbone 由来の分類マスタを `occurrence-web` 内で参照するための変換補助ツールです。

GBIF の完全なデータモデルを RDF として完全再現することは目的ではありません。

そのため、MVP では次のような設計にしています。

* 分類マスタは occurrence data とは別 graph に保存する
* Fuseki の推論エンジンには依存しない
* 階層検索が必要な場合は SPARQL property path などで辿る
* 入力 TSV に存在しない情報は無理に補完しない
* URI は `u.id` を基準に安定的に生成する

## 今後の改善候補

* TSV列名の自動検証
* 欠損値・不正値のログ出力
* 変換件数の集計表示
* gzip ファイル破損時のエラーメッセージ改善
* `--graph-uri` オプション対応
* `--taxon-uri-base` オプション対応
* `--limit` による一部変換
* `--dry-run` による検証のみ実行
* RDF predicate の設定ファイル化
* 大容量ファイル向けのストリーミング処理強化

## ライセンス

このスクリプト自体のライセンスは、このリポジトリのライセンスに従います。

GBIF 由来データを利用する場合は、GBIF および元データセットのライセンス・引用条件に従ってください。