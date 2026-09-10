# 08. 検索要件

## 基本方針

- frontend / backend ともに任意の Darwin Core predicate を検索条件として扱えるようにする
- Darwin Core以外のBio-Database管理項目も検索条件として扱う
- 検索画面では検索項目を複数追加できる
- 複数条件は AND とする
- 空検索は一覧取得として扱う
- 検索結果は閲覧権限に従ってフィルタする
- 同じ検索条件を地図表示にも利用できるようにする
- ユーザー向けUIではIRIではなく日本語項目名を主表示する
- literal / URI の型選択はユーザーに要求せずfrontendで自動判定する

---

## 検索画面

初期状態では `dwc:scientificName` と `dwciri:toTaxon` の入力行を表示するが、検索項目はこの2つに限定しない。

ユーザーが操作する各検索条件は以下だけを持つ。

- 検索項目
- 検索値

### 検索項目候補の出所

検索項目候補は2系統をUI上で統合する。

#### Bio-Database管理項目

以下はDarwin Coreではないが、Occurrence管理情報として常に検索候補へ含める。

| 表示名 | predicate |
| --- | --- |
| 作成者 | `dcterms:creator` |
| データ作成日 | `dcterms:created` |
| データ更新日 | `dcterms:modified` |

これらはfrontendの固定候補として保持する。

##### 作成者検索

`dcterms:creator` はRDFではユーザーIRIを目的語として持つが、ユーザーにはUUIDやIRIを入力させない。

作成者を選択すると検索値欄をユーザー名入力に切り替え、入力文字列で次のAPIを呼び出す。

```http
GET /users/search?user_name={query}
```

- PostgreSQL `users.user_name` をcase-insensitiveな部分一致で検索する
- 最大20件返す
- emailなどの認証情報は返さない
- レスポンスは `user_id` と `user_name` のみ
- `user_name` はDB上UNIQUEではないため、同名ユーザーを1人へ決め打ちしない
- 同名候補はuser UUIDを補助表示してユーザーが選択する

ユーザーが候補を選択した後、frontendは内部的に次のURIへ変換して `dcterms:creator` のURI完全一致検索を行う。

```text
https://bio-database.net/users/{user_id}
```

入力したユーザー名だけでは検索条件を確定せず、候補選択によってuser IDが確定した条件だけをOccurrence検索APIへ送る。

`dcterms:created` / `dcterms:modified` は現時点では保存されている日時文字列への完全一致検索とする。

#### Darwin Core項目

Darwin Core 項目候補は以下から取得する。

```text
GET /vocabularies/darwin-core
```

backendはFuseki内の以下2 graphをJOINして候補を返す。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

- Darwin Core vocabulary graphから語彙IRIと `localName` を取得する
- occurrence-profile graphで `bio:useAtBioDatabase true` の語だけを候補にする
- occurrence-profile graphの `skos:prefLabel @ja` があれば日本語表示名として優先する
- 日本語表示名がなければDarwin Coreの `localName` をfallbackとして使う
- `dwciri:toTaxon` はGBIF分類階層検索に使うため検索候補へ常に含める

項目選択UIでは日本語表示名を主表示する。
predicate IRIは識別・検索API送信用に内部保持し、通常UIでは小さな補助情報として表示する。

例。

```text
学名
  IRI: http://rs.tdwg.org/dwc/terms/scientificName
```

候補は入力補助であり、検索可能な predicate を候補一覧だけに制限しない。
候補にないpredicateは「候補にないIRIを直接指定」から絶対IRIを入力できる。

値の型を選択するUIは設けない。
frontendは検索実行時に値を次のように自動判定し、既存backend contractの `value_type` へ変換する。

- `scheme:...` 形式のIRI値 -> `uri`
- それ以外 -> `literal`
- 作成者は候補選択後のuser URIを必ず `uri` として送る

`match` もユーザーには選択させず、MVPでは常に `exact` とする。

空の条件行は frontend から検索APIへ送らない。

---

## API

```http
POST /occurrences/search
Content-Type: application/json
```

検索条件例。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/scientificName",
      "value": "Lumbricus terrestris",
      "value_type": "literal",
      "match": "exact"
    },
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/stateProvince",
      "value": "Kyoto",
      "value_type": "literal",
      "match": "exact"
    }
  ],
  "page": {
    "limit": 50,
    "cursor": null
  }
}
```

`filters` の各要素は AND で評価する。

backend は `filters[].predicate` に http/https の絶対IRIを受け取る。
`value_type` は `literal` または `uri`、`match` は現時点では `exact` のみ許可する。
これらはfrontendが自動生成する内部contractであり、通常ユーザーには選択させない。

---

## RDF構造を透過した検索

frontendは、指定したDwC項目がOccurrence直下・Identification・Event・Locationのどこに保存されているかを意識しない。

backendは検索時に以下を透過的に探索する。

- Occurrence root
- `hasIdentification` で接続された Identification node
- `hasEvent` で接続された Event node
- `hasLocation` で接続された Location node

特に任意DwC predicateについては、既知の保存先分類だけに依存せず、必要に応じて管理中間ノードも探索する。
これにより、例えば以下のLocation項目も同じfilter形式で検索できる。

- `dwc:locality`
- `dwc:municipality`
- `dwc:county`
- `dwc:stateProvince`
- `dwc:country`
- `dwc:verbatimLocality`
- `dwc:island`
- `dwc:islandGroup`
- `dwc:waterBody`
- `dwc:georeferenceProtocol`
- `dwc:georeferencedDate`
- `dwciri:georeferenceSources`

legacyのOccurrence直下に保存された値も検索対象に含める。

Bio-Database管理項目 `dcterms:creator` / `dcterms:created` / `dcterms:modified` はOccurrence root上を検索する。

---

## リテラル検索

frontendが `value_type = literal` と判定した場合。

- 完全一致
- case-insensitive
- 検索値の前後空白をtrimする
- RDF literalの文字列値を比較する
- datatypeやlanguage tagの違いは文字列比較時には区別しない

例。

```text
"Lumbricus terrestris"
" lumbricus terrestris "
```

上記は同じ検索値として扱う。

---

## URI検索

frontendが `value_type = uri` と判定した場合、通常のURI項目はURI完全一致のみを行う。

`dwciri:toTaxon` にGBIF公開URIを指定した場合だけ、完全一致に加えてGBIF Backbone Taxonomyの下位分類群を含める。

Occurrenceに保存する `toTaxon` は次のGBIF公開URIを使う。

```text
https://www.gbif.org/species/{id}
```

一方、FusekiのGBIF Backbone Taxonomy graphでは分類群を次の内部URIで保持する。

```text
https://bio-database.net/taxa/gbif/{id}
```

検索時にbackendはGBIF公開URIから `{id}` を取り出し、内部taxon URIへ変換する。
Occurrence側の `toTaxon` URIや既存データを書き換えない。

GBIF Backbone内の親子関係は以下のpredicateで保持する。

```text
https://bio-database.net/terms/parentNameUsage
```

指定taxonの下位分類群探索には次のproperty pathを使う。

```sparql
?internalTaxon <https://bio-database.net/terms/parentNameUsage>+ <https://bio-database.net/taxa/gbif/{targetId}> .
```

したがって例えば `dwciri:toTaxon = <https://www.gbif.org/species/42>` で検索すると、Annelida自身に加えて、GBIF Backbone上で `parentNameUsage+` によりAnnelidaへ到達する下位分類群を `toTaxon` に持つOccurrenceも検索結果に含める。

非GBIF URIを `toTaxon` に指定した場合はURI完全一致のみとする。
`dcterms:creator`、`sourcePaper`、その他URI値の検索にはGBIF階層探索を適用しない。

GBIF taxonomy graph URI。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

---

## 地図との共通検索

地図では通常の全件取得に加え、検索画面と同じfilter contractを利用する。

```http
POST /occurrences/map/search
Content-Type: application/json
```

Request。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/locality",
      "value": "Kyoto",
      "value_type": "literal",
      "match": "exact"
    }
  ]
}
```

- filter semantics は `/occurrences/search` と同じ
- 複数条件はAND
- 閲覧権限も通常検索と同じ
- 条件に一致したOccurrenceのうち、完全な緯度経度ペアを持つものだけGeoJSONとして返す
- bboxなどの空間filterは別機能として将来追加する
- UIは通常検索と同じ日本語優先の項目選択を使い、値型選択は表示しない
- Bio-Database管理項目も通常検索と同様に地図絞り込みで利用できる
- 作成者条件も通常検索と同じユーザー名候補選択を利用する

---

## 検索結果の認可

### 非ログイン

- public occurrence のみ

### editor

- public occurrence
- 自分の private occurrence

### admin

- 全 occurrence

認可filterはFuseki検索段階で適用し、limit/cursorや地図件数からprivate occurrenceの存在を推測できないようにする。

---

## ページネーション

通常の一覧検索ではcursor-based paginationを使う。

- default limit: 50
- max limit: 100
- `cursor = null` または未指定で先頭ページ
- cursorはopaque string
- frontendはcursor内部を解釈しない
- 並び順は `created desc, occurrence_id desc` を基本とする
