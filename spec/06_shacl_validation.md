# 06. SHACL・RDF検証要件

## 基本方針

- 保存前検証を行う
- 作成時・更新時に検証する
- 検証に失敗した場合は保存しない
- 検証対象は backend が主語置換・metadata追加を行い、occurrence graph を維持した後の最終RDFとする

---

## SHACLの位置づけ

MVPでは SHACL は厳格な必須項目制約ではなく、構造・型・形式整合性の検証に使う。

必須項目として `dwc:scientificName` などを要求しない。

---

## MVPで最低限チェックする項目

- RDFとして parse できる
- 空 RDF ではない
- graph name が入力に含まれている
- graph name が occurrence graph である
- blank node subject が1つだけ
- object blank node がない
- `dcterms:creator` が frontend から送られていない
- `dcterms:created` が frontend から送られていない
- `dcterms:modified` が frontend から送られていない
- `dcterms:accessRights` が許可URIである
- `dcterms:accessRights` が複数指定されていない
- `dcterms:accessRights` がリテラルではない
- `dcterms:license` が URI である
- `dcterms:license` が複数指定されていない
- `dcterms:license` が `https://creativecommons.org/` で始まる
- `dcterms:created` が `xsd:dateTime`
- `dcterms:modified` が `xsd:dateTime`

---

## accessRights検証

許可値。

```text
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/private
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/public
```

不正例。

```nq
_:occurrence <http://purl.org/dc/terms/accessRights> "public" .
```

```nq
_:occurrence <http://purl.org/dc/terms/accessRights> <https://example.org/terms/access-rights/public> .
_:occurrence <http://purl.org/dc/terms/accessRights> <https://example.org/terms/access-rights/private> .
```

---

## license検証

許可。

```nq
_:occurrence <http://purl.org/dc/terms/license> <https://creativecommons.org/licenses/by/4.0/> .
```

拒否。

```nq
_:occurrence <http://purl.org/dc/terms/license> "CC BY 4.0" .
```

```nq
_:occurrence <http://purl.org/dc/terms/license> <https://example.org/license/custom> .
```

---

## 検証エラーのAPIレスポンス

SHACLまたはRDF検証に失敗した場合、JSONで返す。

```json
{
  "error": "validation_failed",
  "message": "入力が不正です",
  "details": [
    {
      "field": "rdf",
      "message": "dcterms:accessRights must be one of allowed URI values"
    }
  ]
}
```

---

## 実装メモ

RDF parse や基本構造チェックは Rust 側で行ってよい。  
SHACL shape に寄せるものと Rust 側で事前に検証するものは実装しやすさで分けてよい。

ただし、保存前に最終RDFが仕様を満たすことを必ず保証する。
