# 11. API共通仕様

## 基本方針

- APIレスポンスは JSON を基本とする
- 成功レスポンスも JSON で統一する
- エラーレスポンスも JSON で統一する
- API追加・変更時は OpenAPI を必ず更新する
- 認証・認可は backend で行う
- frontend は Jena / Garage に直接アクセスしない

---

## エラーレスポンス基本形式

```json
{
  "error": "invalid_request",
  "message": "入力が不正です"
}
```

---

## バリデーションエラー形式

```json
{
  "error": "validation_failed",
  "message": "入力が不正です",
  "details": [
    {
      "field": "email",
      "message": "メールアドレスの形式が不正です"
    }
  ]
}
```

---

## HTTPステータス方針

| 状況 | Status |
|---|---:|
| 正常作成 | 201 |
| 正常取得 | 200 |
| 正常更新 | 200 |
| 正常削除 | 200 |
| 入力不正 | 400 |
| 未ログイン | 401 |
| CSRF不正 | 403 |
| 権限不足 | 403 |
| private occurrence の存在隠蔽 | 404 |
| 見つからない | 404 |
| 競合 | 409 |
| RDF/SHACL検証失敗 | 422 |
| サイズ超過 | 413 |
| unsupported media type | 415 |
| 外部メールサービス失敗 | 502 |
| Jena/Garage等の外部ストア失敗 | 502 または 500 |
| 予期しないエラー | 500 |

---

## 削除成功レスポンス

削除成功時は `204 No Content` ではなく JSON を返す。

```json
{
  "deleted": true
}
```

---

## 認証API例

### POST /auth/login

Request。

```json
{
  "email": "user@example.com",
  "password": "password123"
}
```

Response。

```json
{
  "authenticated": true
}
```

### POST /auth/logout

Response。

```json
{
  "logged_out": true
}
```

---

## occurrence 作成API方針

RDF本文は N-Quads とする。

- Turtle不可
- 空RDF不可
- graph name必須
- graph name は occurrence graph のみ可
- occurrence graph は `https://{APP_PUBLIC_BASE_URL}/graphs/occurrences`
- frontendは従来どおり、1つのblank node subjectに述語・目的語セットを送る
- backendは述語をOccurrence、Identification、Event、Locationへ振り分けて保存する
- frontendから `hasIdentification`、`hasEvent`、`hasLocation` が送られた場合は400

成功レスポンス例。

```json
{
  "occurrence_id": "uuid",
  "occurrence_uri": "https://example.org/occurrences/uuid"
}
```

---

## 論文Occurrence抽出API

Endpoint。

```http
POST /paper-sources/paper/{paper_id}/extract-occurrences
```

各候補は少なくとも次のJSONキーを返す。

```json
{
  "scientificName": "Metaphire hilgendorfi",
  "locality": "奈良県香芝市真美ヶ丘",
  "eventDate": "1998-06",
  "decimalLatitude": null,
  "decimalLongitude": null
}
```

- `scientificName` は空文字を返さない
- `locality` は取得できなければ `null`
- `eventDate` は取得できなければ `null`
- `eventDate` が存在する場合、LLM側で `YYYY`、`YYYY-MM`、`YYYY-MM-DD` または同形式の `開始/終了` に正規化して返す
- `verbatimEventDate` はpaper importでは使用しない
- 日付の精度を勝手に上げない。年月しか分からない場合に日を補完しない

現行paper source handlerは簡略化移行中で、OpenAPIへのutoipa登録は未完了である。paper import APIをOpenAPIへ再登録する際は上記 `eventDate` をschemaへ含める。

---

## occurrence 検索・一覧API方針

Endpoint。

```http
POST /occurrences/search
Content-Type: application/json
```

空検索は一覧取得として扱う。
検索結果には閲覧可能な occurrence のみを含める。

Request。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/scientificName",
      "value": "Quercus serrata",
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

`filters` は空配列を許可する。
`filters[].predicate` は絶対URIとし、MVP UIでは `dwc:scientificName` のみ選択可能にするが、backend API は任意 predicate URI を受け取れる形にする。
`filters[].value_type` は `literal` または `uri` とする。
`filters[].match` は MVP では `exact` のみとする。

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

`items` は一覧表示用の代表フィールドのみ返す。
該当する RDF predicate が存在しないフィールドは `null` を返す。
RDF全文が必要な場合は `GET /occurrences/{occurrence_id}` を使う。

---

## occurrence 詳細取得のRDF構造

`GET /occurrences/{occurrence_id}` は保存済みN-Quadsを平坦化せず返す。
Identification、Event、LocationのNamed Node、`rdf:type`、Occurrenceからの接続RDFを含む正規化済み構造をそのまま返す。

---

## occurrence 削除API方針

```json
{
  "deleted": true
}
```

---

## CSRF

状態変更APIでは `X-CSRF-Token` を要求する。

対象。

- POST
- PUT
- PATCH
- DELETE

---

## OpenAPI

- すべてのAPIをOpenAPIに反映する
- DTO変更時は schema も更新する
- エラーレスポンスも定義する
- 認証が必要なAPIには security 要件を付ける
- paper importのsource_handlerは現在簡略化移行中のためutoipa登録が未完了。再登録時に現行API契約へ追従させる
