# 05. オカレンスRDF要件

## 基本方針

- オカレンス本体は Apache Jena に RDF として保存する
- フロントエンドからバックエンドへ送信する RDF は N-Quads のみ
- Turtle は入力形式として使わない
- フロントエンドは Jena に直接アクセスしない
- Jena との通信は Rust backend 経由のみ

---

## 入力形式

### 許可

- N-Quads
- graph name として occurrence graph を含む入力のみ

### 拒否

- Turtle
- RDF/XML
- JSON-LD
- TriG
- occurrence graph 以外の graph name を含む N-Quads
- graph name がない N-Quads
- 空 RDF
- 複数 occurrence を含む RDF
- 複数 blank node subject
- object blank node
- backend 管理述語の不正送信

---

## 作成単位

- 1リクエストで作成できる occurrence は1件だけ
- 一括作成は MVP 対象外

---

## URI設計

### occurrence URI

```text
https://{APP_PUBLIC_BASE_URL}/occurrences/{uuid}
```

- UUID は backend が発行する
- frontend は occurrence URI を指定できない
- frontend は仮主語として blank node を使う
- backend は保存前に blank node を occurrence URI に置換する

### intermediate node URI

```text
https://bio-database.net/occurrences/{occurrence_id}/identifications/{number}
https://bio-database.net/occurrences/{occurrence_id}/events/{number}
https://bio-database.net/occurrences/{occurrence_id}/locations/{number}
```

- `{number}` は各中間ノード種別の1始まりの連番とする
- MVPでは各種別を最大1ノードとし、常に `1` を使う
- 将来は1つのoccurrenceに複数のIdentification、Event、Locationを許可する
- 対象となる述語が1つもない種別の中間ノードは作成しない
- 中間ノードはblank nodeではなく上記の永続URIを持つNamed Nodeとする

### user URI

```text
https://{APP_PUBLIC_BASE_URL}/users/{uuid}
```

- `dcterms:creator` の目的語として使う
- ユーザー実体は MVP では PostgreSQL のみで管理する
- `graphs/user` は将来用

### media URI

```text
https://{APP_PUBLIC_BASE_URL}/media/{media_uuid}
```

- `{media_uuid}` は PostgreSQL `media_objects.id` と同じ

### GBIF taxon URI

```text
https://bio-database.net/taxa/gbif/{id}
```

- `{id}` は GBIF Backbone Taxonomy の taxon key とする
- URIはGBIF由来の分類群をこのシステム内で安定して参照するためのURIとする
- 元データとの対応を保持するため、`dcterms:source` で `https://www.gbif.org/species/{id}` を記録する
- 将来別の分類体系を追加する場合は `taxa/{source}/{id}` とし、GBIFの名前空間と混在させない

---

## Named graph

### occurrence graph

```text
https://{APP_PUBLIC_BASE_URL}/graphs/occurrences
```

### taxonomy graph

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

- GBIF Backbone Taxonomyから生成した分類RDFだけを格納する
- graph URIはバージョンを含めず、更新時は同一graphを新しいスナップショットで置換する
- 再現性のため、取り込みに使ったGBIF Backbone Taxonomyのバージョンと取得日時を記録する

### master graph

```text
https://{APP_PUBLIC_BASE_URL}/graphs/master
```

### user graph

```text
https://{APP_PUBLIC_BASE_URL}/graphs/user
```

### 自前語彙namespace

```text
https://{APP_PUBLIC_BASE_URL}/terms
```

---

## graph name の扱い

frontend から送信される N-Quads には graph name を必ず含める。  
graph name は occurrence graph のみ許可する。

許可する graph name。

```text
https://{APP_PUBLIC_BASE_URL}/graphs/occurrences
```

graph name がない場合、または occurrence graph 以外の graph name が含まれていた場合は 400 で拒否する。

backend は保存前に frontend 入力の occurrence graph を維持する。

---

## blank node の扱い

### 許可

全quadで同じ1つの blank node subject を使う。

```nq
_:occurrence <https://example.org/predicate> "value" <https://bio-database.net/graphs/occurrences> .
_:occurrence <https://example.org/another> <https://example.org/object> <https://bio-database.net/graphs/occurrences> .
```

### 拒否

複数の blank node subject。

```nq
_:a <https://example.org/predicate> "x" <https://bio-database.net/graphs/occurrences> .
_:b <https://example.org/predicate> "y" <https://bio-database.net/graphs/occurrences> .
```

object blank node。

```nq
_:occurrence <https://example.org/predicate> _:object <https://bio-database.net/graphs/occurrences> .
```

---

## 保存時の中間ノード正規化

frontendの入力形式は従来どおり、1つのblank node subjectに対する述語と目的語の組とする。
frontendはIdentification、Event、Locationの中間ノードや接続述語を組み立てない。

backendはoccurrence URI発行後、述語ごとに保存先ノードを判定してRDFを正規化する。
目的語のURI、リテラル、datatype、language tagは変更せず、そのまま対象ノードへ移す。

### Occurrence直下

- `dwc:basisOfRecord`
- `dwc:occurrenceRemarks`
- `dcterms:accessRights`
- `dcterms:creator`。backend管理
- `dcterms:created`。backend管理
- `dcterms:modified`。backend管理
- 振り分け一覧にない任意の述語

`dcterms:license`、media参照、自前語彙など、一覧にない述語もOccurrence直下に保存する。

### Identification

対象述語。

- `dwc:scientificName`
- `dwc:identifiedBy`
- `dwc:dateIdentified`
- `dwc:identificationQualifier`
- `dwc:identificationRemarks`
- `dwc:nameAccordingTo`
- `dwciri:toTaxon`

```nq
<https://bio-database.net/occurrences/{occurrence_id}> <https://bio-database.net/terms/hasIdentification> <https://bio-database.net/occurrences/{occurrence_id}/identifications/1> <https://bio-database.net/graphs/occurrences> .
<https://bio-database.net/occurrences/{occurrence_id}/identifications/1> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://rs.tdwg.org/dwc/terms/Identification> <https://bio-database.net/graphs/occurrences> .
```

### Event

対象述語。

- `dwc:eventDate`
- `dwc:samplingProtocol`
- `dwc:samplingEffort`
- `dwc:fieldNumber`
- `dwc:habitat`
- `dwc:recordedBy`

```nq
<https://bio-database.net/occurrences/{occurrence_id}> <https://bio-database.net/terms/hasEvent> <https://bio-database.net/occurrences/{occurrence_id}/events/1> <https://bio-database.net/graphs/occurrences> .
<https://bio-database.net/occurrences/{occurrence_id}/events/1> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://rs.tdwg.org/dwc/terms/Event> <https://bio-database.net/graphs/occurrences> .
```

### Location

対象述語。

- `dwc:decimalLatitude`
- `dwc:decimalLongitude`
- `dwc:geodeticDatum`
- `dwc:coordinateUncertaintyInMeters`
- `dwc:locality`
- `dwc:country`
- `dwc:municipality`

```nq
<https://bio-database.net/occurrences/{occurrence_id}> <https://bio-database.net/terms/hasLocation> <https://bio-database.net/occurrences/{occurrence_id}/locations/1> <https://bio-database.net/graphs/occurrences> .
<https://bio-database.net/occurrences/{occurrence_id}/locations/1> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://purl.org/dc/terms/Location> <https://bio-database.net/graphs/occurrences> .
```

### 正規化規則

- 同じtarget nodeに分類された全述語・全値は、MVPでは同じ `/1` ノードへ保存する
- 対象述語が存在するときだけ中間ノード、`rdf:type`、Occurrenceからの接続RDFを作成する
- 対象述語が存在しなければ空の中間ノードや接続RDFは作成しない
- unknown predicateは拒否せずOccurrence直下へ保存する
- `dwciri:toTaxon` の完全URIは `http://rs.tdwg.org/dwc/iri/toTaxon` とする
- backend生成接続述語は `hasIdentification`、`hasEvent`、`hasLocation` の3つとする
- 3つの完全URIは `https://bio-database.net/terms/hasIdentification`、`https://bio-database.net/terms/hasEvent`、`https://bio-database.net/terms/hasLocation` とする
- 上記3述語はbackend管理とし、frontendから送られた場合は400で拒否する
- `rdf:type` はfrontend送信禁止述語にしない。frontendから送られた場合はOccurrence直下へ保存する

---

## 述語方針

- Darwin Core または Dublin Core Terms を基本とする
- 公開語彙を優先する
- 自前語彙はなるべく避ける
- 必要な場合のみ `https://{APP_PUBLIC_BASE_URL}/terms` 以下に定義する
- 述語はIRIのみとし、リテラル述語は許可しない
- URI値を優先する
- リテラルは目的語として必要な場合に許可する
- 目的語リテラルには可能な限り明示的な datatype を付ける

---

## backend が作成時に必ず追加する RDF

作成時、backend は以下を必ず追加する。

| 述語 | 値 |
|---|---|
| `dcterms:creator` | user URI |
| `dcterms:created` | `xsd:dateTime` UTC |
| `dcterms:modified` | `xsd:dateTime` UTC |
| `dcterms:accessRights` | 指定がなければ public |

作成直後の `dcterms:created` と `dcterms:modified` は同じ時刻にする。

---

## frontend から送信された場合に拒否する述語

以下は backend 管理述語であり、frontend から送られた場合は 400 で拒否する。

- `dcterms:creator`
- `dcterms:created`
- `dcterms:modified`
- `https://bio-database.net/terms/hasIdentification`
- `https://bio-database.net/terms/hasEvent`
- `https://bio-database.net/terms/hasLocation`

`rdf:type` はfrontend送信禁止述語に含めない。

---

## frontend から送信可能な backend 認識述語

### `dcterms:accessRights`

frontend から送信可能。  
送信されなかった場合は public を付与する。

許可値は以下の2つのみ。

```text
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/private
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/public
```

制約。

- 目的語は URI のみ
- 文字列リテラルは禁止
- 複数指定は禁止
- 許可値以外は禁止

### `dcterms:license`

frontend から送信可能。  
送信されなかった場合は未指定とする。

制約。

- 目的語は URI のみ
- 文字列リテラルは禁止
- 複数指定は禁止
- `https://creativecommons.org/` で始まる URI のみ許可

---

## デフォルト occurrence 項目

MVPでは、以下のような項目をデフォルト必須・デフォルト付与しない。

- `dwc:scientificName`
- `dwc:eventDate`
- `dwc:locality`
- `dwc:occurrenceRemarks`

オカレンス項目は任意の RDF として扱う。  
ただし、検索MVPでは `dwc:scientificName` を検索対象として扱う。

---

## 作成処理

1. 認証確認
2. 認可確認
3. 入力N-Quadsをparse
4. 空RDFなら 400
5. graph name がない、または occurrence graph 以外なら 400
6. blank node subject が1つだけであることを検証
7. object blank node がないことを検証
8. backend管理述語の不正送信を検証
9. occurrence UUID / URI を発行
10. 入力述語をOccurrence、Identification、Event、Locationへ振り分ける
11. 必要な中間ノードURI、`rdf:type`、接続RDFを生成する
12. unknown predicateをOccurrence直下へ配置する
13. occurrence graph が維持されていることを確認
14. backend RDFメタデータをOccurrence直下へ追加する
15. 最終N-Quadsに対して検証
16. SHACL/保存前検証
17. Jenaに保存
18. 監査ログを success に更新
19. JSONレスポンスを返す

---

## 更新処理

MVPでは部分更新ではなく、対象 occurrence の RDF を丸ごと置換する。

### 更新時に維持するもの

- `dcterms:creator`
- `dcterms:created`

### 更新時に更新するもの

- `dcterms:modified`

### 更新時の `dcterms:accessRights`

- 新しい RDF に含まれていればその値を採用する
- 含まれていなければ public にする

### 更新時の `dcterms:license`

- 新しい RDF に含まれていればその値を採用する
- 含まれていなければ未指定に戻る

### 更新時の中間ノード

- frontendからは作成時と同じ星型の述語・目的語セットを受け取る
- backendは更新入力を同じ振り分けルールで再正規化する
- 既存のIdentification、Event、Locationと接続RDFを削除してから丸ごと置換する
- MVPでは再生成後の中間ノード番号も各種別 `1` とする
- 対象述語がなくなった種別の中間ノードは保存しない

---

## 削除処理

MVPでは、対象occurrenceとbackendが生成した中間ノード構造を物理削除する。

削除対象。

- 対象 occurrence URI をsubjectに持つ全quad
- `/identifications/{number}` をsubjectに持つ全quad
- `/events/{number}` をsubjectに持つ全quad
- `/locations/{number}` をsubjectに持つ全quad
- Occurrenceから中間ノードへの接続RDF

外部URIの目的語やmedia metadataは自動削除しない。
削除成功時はJSONで返す。

```json
{
  "deleted": true
}
```
