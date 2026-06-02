# 08. 検索要件

## 基本方針

- MVPでは検索述語を選べるUIを想定する
- ただし、MVPでUIから選択できる述語は `dwc:scientificName` のみ
- backend API は任意の predicate URI を検索条件として受け取れる形にする
- 空検索は一覧取得として扱う
- 検索結果は閲覧権限に従ってフィルタする

---

## MVP UI検索対象

```text
http://rs.tdwg.org/dwc/terms/scientificName
```

backend は `filters[].predicate` に絶対URIを受け取る。
MVP UIでは `dwc:scientificName` のみを選択肢として出すが、API contract は将来の任意述語検索に備えて固定しない。

---

## 値の形式

`dwc:scientificName` の値は以下の両方を許可する。

- リテラル
- URI

ただし、taxonomy 階層探索は URI の場合のみ有効。

---

## リテラル検索

リテラルの場合。

- 完全一致
- case-insensitive
- 前後空白 trim
- 連続空白の厳密な正規化は MVP では必須にしない

例。

```text
"Lumbricus terrestris"
" lumbricus terrestris "
```

上記は同じものとして扱う。

---

## URI検索

URIの場合。

- 完全一致
- taxonomy graph の階層探索を行う
- 指定 taxon 自身と下位分類群を含める

---

## taxonomy graph

taxonomy graph URI。

```text
https://{APP_PUBLIC_BASE_URL}/graphs/taxonomy
```

taxonomy ontology は外部提供されたものを使う。

---

## 階層述語

MVPでは分類階層述語を `rdfs:subClassOf` 固定とする。

```text
http://www.w3.org/2000/01/rdf-schema#subClassOf
```

SKOS の `skos:broader` / `skos:narrower` は MVP 対象外。

---

## 推論エンジン

MVPでは Jena の推論エンジンは使わない。  
検索時に SPARQL property path で階層を辿る。

例。

```sparql
?taxon rdfs:subClassOf* ?targetTaxon .
```

---

## 検索結果の認可

検索結果には閲覧可能な occurrence のみを含める。

### 非ログイン

- public occurrence のみ

### editor

- public occurrence
- 自分の private occurrence

### admin

- 全 occurrence

---

## ページネーション

- cursor-based pagination を使う
- default limit は 50
- max limit は 100
- `cursor` が `null` または未指定の場合は先頭ページを返す
- cursor は opaque string とし、frontend は中身を解釈しない
- 並び順は `created desc, occurrence_id desc` を基本とし、同一 `created` のデータでもページ境界が安定するようにする

---

## 空検索

空検索は一覧取得として扱う。

- 非ログイン: public occurrence 一覧
- editor: public + 自分の private occurrence 一覧
- admin: 全 occurrence 一覧

---

## API例

```http
POST /occurrences/search
Content-Type: application/json
```

空検索、つまり一覧取得。

```json
{
  "filters": [],
  "page": {
    "limit": 50,
    "cursor": null
  }
}
```

任意 predicate の検索。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/scientificName",
      "value": "Lumbricus terrestris",
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

URI検索の例。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/scientificName",
      "value": "https://example.org/taxon/Mammalia",
      "value_type": "uri",
      "match": "exact"
    }
  ],
  "page": {
    "limit": 50,
    "cursor": null
  }
}
```

Response。

```json
{
  "items": [
    {
      "occurrence_id": "uuid",
      "occurrence_uri": "https://bio-database.net/occurrences/uuid",
      "scientific_name": "Quercus serrata",
      "basis_of_record": "PreservedSpecimen",
      "recorded_by": "Yamada Taro",
      "created": "2026-06-02T10:20:30Z",
      "modified": "2026-06-02T10:20:30Z",
      "access_rights": "public"
    }
  ],
  "page": {
    "limit": 50,
    "next_cursor": "opaque-cursor-string",
    "has_next": true
  }
}
```

一覧用 response は表示に必要な代表フィールドのみ返す。
RDF全文が必要な場合は `GET /occurrences/{occurrence_id}` で detail を取得する。
