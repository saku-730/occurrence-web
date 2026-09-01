# 17. Darwin Core 入力候補の選定

## 目的

オカレンス作成・編集画面の述語候補に Darwin Core 語彙を無条件ですべて表示せず、Bio-Database で実際に利用する用語だけを提示する。

## 候補選定の source of truth

Bio-Database で新規入力候補として利用する用語は、次のファイルを source of truth とする。

- `frontend/content/terms/darwin-core/list.csv`

候補判定には次の列を使う。

- `namespace`: 語彙名前空間
- `iri`: 用語を一意に識別する IRI
- `use_at_bio_database`: Bio-Database で新規入力候補として利用するか

`GET /vocabularies/darwin-core` の候補として採用する条件は、`namespace=dwc` かつ `use_at_bio_database=true` とする。照合は term 名や英語ラベルではなく IRI で行う。

`label` と `label_ja` は表示名であり、候補採否の判定には使用しない。

## Fuseki との責務分離

`use_at_bio_database` は Darwin Core 自体の語彙定義ではなく、Bio-Database 固有の UI・運用ポリシーである。そのため、このフラグを Fuseki の Darwin Core 語彙データには追加しない。

責務は次のように分離する。

- Fuseki: Darwin Core RDF 語彙の保存・検索
- `list.csv`: Bio-Database で利用する用語の選定と表示用メタデータ
- backend: Fuseki の語彙取得結果を `list.csv` の許可 IRI で絞り込む
- frontend: backend が返した候補を表示する

## 候補 API

`GET /vocabularies/darwin-core` は次の順序で候補を作る。

1. Fuseki から Darwin Core terms namespace の語彙を取得する
2. `list.csv` から `namespace=dwc` かつ `use_at_bio_database=true` の IRI 集合を作る
3. Fuseki の取得結果と許可 IRI 集合の積集合だけを返す
4. 返却候補を従来どおり `local_name`、次に IRI の順で並べる

不整合時は次の扱いとする。

- Fuseki に存在しても `list.csv` で `false` または未定義なら、新規候補には表示しない
- `list.csv` で `true` でも Fuseki に存在しなければ、新規候補には表示しない

## 既存データの扱い

この制限は「新規に選択できる候補」にだけ適用する。

既存オカレンスに `use_at_bio_database=false` または `list.csv` 未定義の述語が保存されている場合でも、その RDF を削除・変換しない。作成・編集画面が既存データを読み込んだときは、その既存述語を表示できる状態を維持する。

## 実装上の扱い

backend は `list.csv` をコンパイル時に読み込み、許可 IRI 集合を生成する。実行時に frontend ディレクトリを参照する必要はない。

`list.csv` の列位置を固定値として扱わず、ヘッダ名 `namespace`、`iri`、`use_at_bio_database` から対象列を決定する。

## 受け入れ条件

- `dwc:scientificName` のように `use_at_bio_database=true` の Darwin Core 用語が候補 API に含まれる
- `dwc:acceptedScientificName` のように `use_at_bio_database=false` の用語が候補 API に含まれない
- `dcterms:*` など別 namespace の行は Darwin Core 候補 API の対象外とする
- 既存オカレンスに候補外述語があっても、その値は自動削除されない
- `use_at_bio_database` のための RDF triple を Fuseki に追加しない
