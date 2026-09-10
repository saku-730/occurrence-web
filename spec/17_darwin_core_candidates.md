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

### 2.1 named graphを分離する理由

ここで分けているのは、語彙IRIのnamespaceではなくFuseki上のnamed graphである。

同じDarwin Core用語IRIを主語にしていても、情報の出所と意味が異なるためgraphを分ける。

Darwin Core語彙graphにはTDWG由来の公式語彙情報を保持する。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
```

一方、次の情報はDarwin Core公式仕様ではなくBio-Database独自の設定である。

- Bio-Databaseでその用語を使用するか
- Bio-Database上で日本語で何と表示するか

そのため、これらは次のアプリ固有graphに保持する。

```text
https://bio-database.net/graphs/app/occurrence-profile
```

概念的には次の構造になる。

```text
dwc:scientificName
      │
      ├─ Darwin Core公式情報
      │     → graphs/vocabularies/darwin-core
      │
      └─ Bio-Database独自情報
            → graphs/app/occurrence-profile
```

この分離の主な目的は、公式語彙データとアプリ固有設定のライフサイクルを独立させることである。

たとえばDarwin Core公式語彙を最新版に更新するときは、Darwin Core語彙graphだけを削除・再投入できる。この操作によってBio-Database独自の利用可否や日本語名を失わない。

逆にBio-Database側の設定だけを作り直す場合は、`occurrence-profile` graphだけを削除・再投入できる。

したがって、Darwin Core公式データの更新とBio-Database独自設定の更新を互いに巻き込まずに実施できる。

述語のnamespaceについても同様に、Bio-Database固有概念はBio-Database独自namespaceを使用する。

```text
https://bio-database.net/terms/useAtBioDatabase
```

一方、日本語表示名は既存標準語彙で意味を十分表現できるため、独自述語を作らず `skos:prefLabel` を使用する。

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

## 5. Fusekiへの投入・置き換え

生成した `darwin_core_master.nq` は、Darwin Core公式語彙graphとBio-Database固有設定graphの両方を含む。

既存Fuseki上のDarwin Core関連データを完全に置き換える場合は、対象の2 graphだけを削除してから、新しいN-Quadsを投入する。

削除対象:

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

SPARQL Update例:

```sparql
DROP SILENT GRAPH <https://bio-database.net/graphs/vocabularies/darwin-core>;
DROP SILENT GRAPH <https://bio-database.net/graphs/app/occurrence-profile>;
```

この操作ではOccurrence RDFやGBIF Backboneなど、他のnamed graphは削除しない。

その後、生成済みN-QuadsをFusekiの `/data` endpointへ `application/n-quads` として投入する。

```bash
curl -fsS \
  -u "${FUSEKI_USER}:${FUSEKI_PASSWORD}" \
  -X POST \
  -H 'Content-Type: application/n-quads' \
  --data-binary @darwin_core_master.nq \
  "${FUSEKI_URL}/${FUSEKI_DATASET}/data"
```

現在生成済みのN-Quadsでは、概ね次の件数を想定する。

```text
graphs/vocabularies/darwin-core : 3654 triples
graphs/app/occurrence-profile   : 799 triples
```

投入後はgraphごとのtriple数を確認し、対象2 graphが期待どおり再作成されたことを検証する。

---

## 6. Darwin Core候補API

対象API:

```text
GET /vocabularies/darwin-core
```

候補取得はFuseki内で完結させる。

1. `https://bio-database.net/graphs/vocabularies/darwin-core` から語彙IRIと `bio:localName` を取得する。
2. 同じ語彙IRIを `https://bio-database.net/graphs/app/occurrence-profile` とJOINする。
3. `https://bio-database.net/terms/useAtBioDatabase true` を持つ語彙だけを返す。
4. `false` の語彙、および `useAtBioDatabase` が存在しない語彙は新規入力候補として返さない。
5. 候補の識別にはIRIを使用する。表示ラベルは識別子として扱わない。

概念的なSPARQLは以下。

```sparql
SELECT DISTINCT ?term ?localName
WHERE {
  GRAPH <https://bio-database.net/graphs/vocabularies/darwin-core> {
    ?term <https://bio-database.net/terms/localName> ?localName .
    FILTER(isIRI(?term))
  }
  GRAPH <https://bio-database.net/graphs/app/occurrence-profile> {
    ?term <https://bio-database.net/terms/useAtBioDatabase> true .
  }
}
ORDER BY LCASE(STR(?localName)) STR(?term)
```

現時点のAPIレスポンスは従来どおり `uri` と `local_name` を返す。`skos:prefLabel` の `@ja` はFusekiには格納済みだが、backend APIレスポンスへの日本語表示名追加は別実装とする。

既存Occurrenceに、現在の新規入力候補ではないpredicateが含まれていても、自動削除・自動変換はしない。

---

## 7. 実装状態

FusekiへのBio-Database固有メタデータ投入後、Darwin Core候補backendの絞り込みを実装済みとする。

現在の動作:

- runtimeで `list.csv` は読まない。
- backendのDarwin Core候補取得はFusekiだけを参照する。
- Darwin Core語彙graphと`occurrence-profile` graphをIRIでJOINする。
- `bio:useAtBioDatabase true` の語彙だけを `GET /vocabularies/darwin-core` の候補として返す。
- `false` または設定なしの語彙は候補に返さない。
- APIレスポンス形式は現時点では `uri` / `local_name` のまま維持する。
- 日本語 `skos:prefLabel @ja` のAPI返却は未実装であり、次段階で追加できる。

---

## 8. 受け入れ条件

N-Quads生成:

- 元のDarwin Core語彙graphに存在する全主語について `bio:useAtBioDatabase` が1件存在する。
- `list.csv` にIRIが存在すれば `use_at_bio_database` の値を使用する。
- `list.csv` にIRIが存在しなければ `bio:useAtBioDatabase false` とする。
- `label_ja` が存在する語彙について `skos:prefLabel` + `@ja` を生成する。
- `label_ja` が空欄なら日本語ラベルtripleを生成しない。
- `list.csv`にのみ存在し、元の語彙graphには存在しないIRIの設定tripleは生成しない。
- Bio-Database固有tripleは `https://bio-database.net/graphs/app/occurrence-profile` に入れる。
- 元のDarwin Core語彙tripleは `https://bio-database.net/graphs/vocabularies/darwin-core` のまま保持する。

backend候補取得:

- backendに`list.csv`のruntime依存がない。
- `GET /vocabularies/darwin-core` はFusekiの2 named graphをJOINして取得する。
- `bio:useAtBioDatabase true` の語彙だけを返す。
- `bio:useAtBioDatabase false` の語彙を返さない。
- `useAtBioDatabase` が存在しない語彙も返さない。
- 既存OccurrenceのRDFは候補設定変更によって自動削除・自動変換しない。

今後:

- 日本語表示名をAPIで返す場合は、`occurrence-profile` graph内の `skos:prefLabel` `@ja` を取得してレスポンスへ追加する。
- runtimeの候補判定に`list.csv`を直接使用しない方針は維持する。
