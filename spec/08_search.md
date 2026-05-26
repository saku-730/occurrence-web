# 08. 検索要件

## 基本方針

- MVPでは検索述語を選べるUIを想定する
- ただし、MVPで選択できる述語は `dwc:scientificName` のみ
- 空検索は一覧取得として扱う
- 検索結果は閲覧権限に従ってフィルタする

---

## MVP検索対象

```text
http://rs.tdwg.org/dwc/terms/scientificName
```

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

- `limit` / `offset` を使う
- default limit は 50
- max limit は 100
- offset の default は 0

---

## 空検索

空検索は一覧取得として扱う。

- 非ログイン: public occurrence 一覧
- editor: public + 自分の private occurrence 一覧
- admin: 全 occurrence 一覧

---

## API例

```http
GET /occurrences?predicate=dwc:scientificName&value=Lumbricus%20terrestris&limit=50&offset=0
```

URI検索の例。

```http
GET /occurrences?predicate=dwc:scientificName&value=https%3A%2F%2Fexample.org%2Ftaxon%2FMammalia&limit=50&offset=0
```
