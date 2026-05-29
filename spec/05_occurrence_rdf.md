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

---

## Named graph

### occurrence graph

```text
https://{APP_PUBLIC_BASE_URL}/graphs/occurrences
```

### taxonomy graph

```text
https://{APP_PUBLIC_BASE_URL}/graphs/taxonomy
```

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

## 述語方針

- Darwin Core または Dublin Core Terms を基本とする
- 公開語彙を優先する
- 自前語彙はなるべく避ける
- 必要な場合のみ `https://{APP_PUBLIC_BASE_URL}/terms` 以下に定義する
- URI値を優先する
- リテラルは必要な場合に許可する
- リテラルには可能な限り明示的な datatype を付ける

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
10. blank node subject を occurrence URI に置換
11. occurrence graph が維持されていることを確認
12. backend RDFメタデータを追加
13. 最終N-Quadsに対して検証
14. SHACL/保存前検証
15. Jenaに保存
16. 監査ログを success に更新
17. JSONレスポンスを返す

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

---

## 削除処理

MVPでは、対象 occurrence URI を subject に持つ quad のみ削除する。

- linked blank node は辿らない
- media metadata は自動削除しない
- 孤立データの自動掃除は MVP 対象外
- 削除成功時は JSON で返す

```json
{
  "deleted": true
}
```
