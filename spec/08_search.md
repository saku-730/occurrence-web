# 08. 検索要件

## 基本方針

- frontend / backend ともに任意の Darwin Core predicate を検索条件として扱えるようにする
- 検索画面では Darwin Core 項目を複数追加できる
- 複数条件は AND とする
- 空検索は一覧取得として扱う
- 検索結果は閲覧権限に従ってフィルタする
- 同じ検索条件を地図表示にも利用できるようにする

---

## 検索画面

初期状態では `dwc:scientificName` の入力行を1つ表示するが、学名だけに限定しない。

各検索条件は以下を持つ。

- predicate
- value
- value type (`literal` / `uri`)
- match (`exact`)

Darwin Core 項目候補は以下から取得する。

```text
GET /vocabularies/darwin-core
```

候補は入力補助であり、検索可能な predicate を候補一覧だけに制限しない。
ユーザーは `http://rs.tdwg.org/dwc/terms/...` や `http://rs.tdwg.org/dwc/iri/...` の絶対IRIを直接入力できる。

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

---

## リテラル検索

`value_type = literal` の場合。

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

`value_type = uri` の場合。

- URI完全一致を行う
- URI値は有効なIRIでなければならない
- taxonomy graph の下位分類群探索も維持する

分類群検索では指定taxon自身に加え、以下のproperty pathで下位分類群を含める。

```sparql
?taxon rdfs:subClassOf+ ?targetTaxon .
```

完全一致条件と組み合わせるため、target自身も結果に含む。

taxonomy graph URI。

```text
https://bio-database.net/graphs/taxonomy/gbif-backbone
```

分類群URI。

```text
https://bio-database.net/taxa/gbif/{id}
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

地図検索はMVPでは一致する座標付きOccurrenceを全件GeoJSON化するため、内部でcursor paginationを繰り返して全ページを取得する。

---

## 空検索

`filters: []` は一覧取得として扱う。

- 非ログイン: public occurrence一覧
- editor: public + 自分のprivate occurrence一覧
- admin: 全occurrence一覧

地図では `filters: []` は `GET /occurrences/map` と同じ対象集合を意味する。

---

## テスト要件

最低限以下を確認する。

- scientificName以外のDwC predicateを検索できる
- Location nodeに保存された `stateProvince` 等を検索できる
- 複数filterがANDになる
- literal検索はcase-insensitiveかつtrimされる
- URI完全一致が動く
- taxonomy URI検索で下位分類群を含める
- public/privateの認可を維持する
- 地図検索へ同じfilterが渡される
- 空検索が一覧取得になる
