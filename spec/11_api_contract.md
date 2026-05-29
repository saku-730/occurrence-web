# 11. API共通仕様

## 基本方針

- APIレスポンスは JSON を基本とする
- 成功レスポンスも JSON で統一する
- エラーレスポンスも JSON で統一する
- API追加・変更時は OpenAPI を必ず更新する
- 認証・認可は backend で行う
- frontend は Jena / MinIO に直接アクセスしない

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
| Jena/MinIO等の外部ストア失敗 | 502 または 500 |
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

成功レスポンス例。

```json
{
  "occurrence_id": "uuid",
  "occurrence_uri": "https://example.org/occurrences/uuid"
}
```

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
