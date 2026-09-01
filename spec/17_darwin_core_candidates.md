# Darwin Core候補とBio-Database用メタデータ

## 1. 目的

Darwin Core語彙全体と、Bio-Databaseで各語彙をどのように利用するかというアプリ固有メタデータをFusekiで管理する。

実行時の候補生成ではFusekiを問い合わせ先とし、`frontend/content/terms/darwin-core/list.csv` を直接フィルターとして使用しない。

---

## 2. 基本方針

- Darwin Core公式語彙はFusekiに保持する。
- Bio-Database固有の語彙設定もFusekiに保持する。
- Darwin Core公式語彙とBio-Database固有設定はnamed graphを分離する。
- backendは実行時に`list.csv`を読んで候補を絞り込まない。
- `list.csv`はGit管理可能な元データとして残し、FusekiへBio-Database固有設定を投入するためのseed/import元として利用できる。

使用する主なnamed graphは以下とする。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

- `.../vocabularies/darwin-core`: Darwin Core語彙本体
- `.../app/occurrence-profile`: Bio-Databaseでの利用可否と日本語表示名

この分離によりDarwin Core語彙を再生成・再投入しても、Bio-Database固有設定を独立して管理できるようにする。

---

## 3. Bio-Database用メタデータ

Bio-Database固有設定として各語彙に追加する情報は、当面次の2つだけとする。

1. Bio-Databaseで使用するか
2. 日本語で何と表示するか

### 3.1 Bio-Databaseで使用するか

述語:

```text
https://bio-database.net/terms/useAtBioDatabase
```

目的語は `xsd:boolean` とし、`true` / `false` を明示的に保存する。

`list.csv` の `use_at_bio_database` 列を元にする。

N-Quads例:

```nq
<http://rs.tdwg.org/dwc/terms/scientificName> <https://bio-database.net/terms/useAtBioDatabase> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> <https://bio-database.net/graphs/app/occurrence-profile> .
```

`false` の用語についてもtripleを省略せず、明示的に `false` を保存する。

### 3.2 日本語表示名

述語:

```text
http://www.w3.org/2004/02/skos/core#prefLabel
```

目的語は日本語言語タグ付きliteral (`@ja`) とする。

`list.csv` の `label_ja` 列を元にする。

N-Quads例:

```nq
<http://rs.tdwg.org/dwc/terms/scientificName> <http://www.w3.org/2004/02/skos/core#prefLabel> "学名"@ja <https://bio-database.net/graphs/app/occurrence-profile> .
```

`label_ja` が空欄の場合は、日本語ラベルを推測・英語ラベルで代用せず、`skos:prefLabel` の `@ja` tripleは生成しない。

### 3.3 主語とnamed graph

主語には各語彙そのもののIRIを使う。

Bio-Database固有の2種類のtripleはすべて次のnamed graphへ格納する。

```text
https://bio-database.net/graphs/app/occurrence-profile
```

Darwin Core公式語彙graphにはBio-Database固有tripleを追加しない。

したがって、1語彙の基本形は次のようになる。

```nq
<TERM_IRI> <https://bio-database.net/terms/useAtBioDatabase> "true"^^<http://www.w3.org/2001/XMLSchema#boolean> <https://bio-database.net/graphs/app/occurrence-profile> .
<TERM_IRI> <http://www.w3.org/2004/02/skos/core#prefLabel> "日本語名"@ja <https://bio-database.net/graphs/app/occurrence-profile> .
```

---

## 4. `list.csv`とのマージ規則

`frontend/content/terms/darwin-core/list.csv`は、Bio-Databaseで利用する語彙設定を人間がGit上で管理・レビューするための元データとして利用する。

対応は以下とする。

| `list.csv` | RDF |
| --- | --- |
| `iri` | 主語IRI |
| `use_at_bio_database` | `bio:useAtBioDatabase` の `xsd:boolean` |
| `label_ja` | `skos:prefLabel` の `@ja` literal |

N-Quads生成時は、Darwin Core語彙graphに実際に存在する主語IRIを基準にする。

1. 元N-QuadsのDarwin Core語彙graphに存在する各主語IRIを列挙する。
2. 同じIRIが`list.csv`に存在する場合、`use_at_bio_database`をそのまま `bio:useAtBioDatabase` として生成する。
3. 同じIRIが`list.csv`に存在しない場合、`bio:useAtBioDatabase false` を明示的に生成する。
4. 同じIRIが`list.csv`に存在し、かつ`label_ja`が空でなければ `skos:prefLabel` `@ja` を生成する。
5. `label_ja`が空欄、またはIRI自体が`list.csv`に存在しない場合、日本語ラベルtripleは生成しない。
6. `list.csv`には存在するが元N-QuadsのDarwin Core語彙graphに存在しないIRIについては、Bio-Database固有graphにもtripleを生成しない。語彙本体に存在しないIRIだけを設定graphへ作成しないためである。

これにより、Fuseki内のDarwin Core語彙graphに存在するすべての語彙について、`useAtBioDatabase` が `true` または `false` のどちらかで必ず明示される。

Rust backendが実行時に`include_str!`等で`list.csv`を読み、Fusekiの結果と照合して候補を決定する構成にはしない。

想定する流れは以下。

```text
list.csv + Darwin Core source N-Quads
                ↓ setup / seed / import
Fuseki
 ├─ Darwin Core vocabulary graph
 └─ Bio-Database occurrence-profile graph
          ↓
      Rust backend
          ↓
       frontend
```

この構成では実行時の問い合わせ先はFusekiに一本化される。

---

## 5. Darwin Core候補API

対象API:

```text
GET /vocabularies/darwin-core
```

最終的な処理方針は以下。

1. Fuseki内のDarwin Core語彙graphを対象にする。
2. Bio-Databaseの`occurrence-profile` graphと結合する。
3. `bio:useAtBioDatabase true` の語彙だけを新規入力候補として返す。
4. `skos:prefLabel` の `@ja` が存在すれば日本語表示名として返す。
5. 候補の識別にはIRIを使用する。表示ラベルは識別子として扱わない。

既存Occurrenceに、現在の新規入力候補ではないpredicateが含まれていても、自動削除・自動変換はしない。

---

## 6. 移行状態

この方針への変更時点では、`list.csv`を実行時フィルターとして利用する実装を撤回する。

そのため、FusekiへのBio-Database固有メタデータ投入と、そのメタデータを使ったSPARQL絞り込みが実装されるまでは、`GET /vocabularies/darwin-core`はFusekiに存在するDarwin Core語彙を全件返す。

これは移行中の一時的な挙動であり、最終仕様は前節のFuseki内メタデータによる絞り込みとする。

---

## 7. 受け入れ条件

移行時点:

- backendに`list.csv`を実行時読み込みする依存がない。
- `darwin_core_policy`によるCSVパース・allowlist生成がない。
- `GET /vocabularies/darwin-core`はFusekiから取得したDarwin Core語彙をそのまま候補として返す。

N-Quads生成:

- 元のDarwin Core語彙graphに存在する全主語について `bio:useAtBioDatabase` が1件存在する。
- `list.csv` にIRIが存在すれば `use_at_bio_database` の値を使用する。
- `list.csv` にIRIが存在しなければ `bio:useAtBioDatabase false` とする。
- `label_ja` が存在する語彙について `skos:prefLabel` + `@ja` を生成する。
- `label_ja` が空欄なら日本語ラベルtripleを生成しない。
- `list.csv`にのみ存在し、元の語彙graphには存在しないIRIの設定tripleは生成しない。
- Bio-Database固有tripleは `https://bio-database.net/graphs/app/occurrence-profile` に入れる。
- 元のDarwin Core語彙tripleは `https://bio-database.net/graphs/vocabularies/darwin-core` のまま保持する。

最終形:

- Darwin Core公式語彙graphとBio-Database固有設定graphが分離されている。
- backendはFuseki内の `bio:useAtBioDatabase` を使って候補を絞り込む。
- 日本語表示名はFuseki内の `skos:prefLabel` `@ja` から取得できる。
- runtimeの候補判定に`list.csv`を直接使用しない。
