# 18. Occurrence 地図表示・Geocoding要件

## 目的

Occurrence を地図上に表示する。

- 元データに `dwc:decimalLatitude` / `dwc:decimalLongitude` がある記録は、その座標をそのまま表示する
- 元データに緯度経度がなく、地名情報だけがある記録は、Occurrence登録時に Nominatim で Geocoding して表示用座標を付与する
- 元座標と Nominatim 由来座標は、RDF上の由来とフロント表示の両方で区別する
- MVP の地図APIは、閲覧権限のある座標付きOccurrenceを基本的に全件返す
- 任意の Darwin Core predicate による地図filterは通常のデータ検索と同じcontractで提供する
- bboxなどの空間filterは将来拡張とする

---

## 採用技術

### フロントエンド

- 地図描画エンジンは MapLibre GL JS を使う
- backend から GeoJSON `FeatureCollection` を取得してOccurrenceを描画する
- 背景地図は OpenFreeMap の Liberty style を既定値として使う
- 既定 style URL は `https://tiles.openfreemap.org/styles/liberty` とする
- OpenFreeMap の背景地図データは OpenStreetMap 由来で、OpenMapTiles schema のvector tileとして提供される
- 道路、街路、建物、地名、河川、公園など、国レベルより詳細な背景地図をズームに応じて表示する
- `NEXT_PUBLIC_MAP_STYLE_URL` が設定されている場合は、そのstyle URLで既定値を上書きする
- 将来自前tile配信や別providerへ移行する場合も、MapLibre側のOccurrence描画ロジックは維持する

### Geocoder

- Nominatim を使う
- MVP では `https://nominatim.openstreetmap.org/` を既定endpointとする
- `countrycodes` は使用しない
- 検索結果が複数あってもユーザー選択は行わず、Nominatim の先頭結果だけを採用する
- request は `limit=1` とする
- Nominatimは地名から座標を得るGeocoderであり、背景地図の描画には使わない

---

## Location 項目の扱い

地理情報は Location 中間ノード `/locations/1` に保存する。

Locationとして扱う主要述語は以下。

- `dwc:decimalLatitude`
- `dwc:decimalLongitude`
- `dwc:geodeticDatum`
- `dwc:coordinateUncertaintyInMeters`
- `dwc:locality`
- `dwc:verbatimLocality`
- `dwc:island`
- `dwc:islandGroup`
- `dwc:waterBody`
- `dwc:municipality`
- `dwc:county`
- `dwc:stateProvince`
- `dwc:country`
- `dwciri:georeferenceSources`
- `dwc:georeferenceProtocol`
- `dwc:georeferencedDate`
- `dwc:georeferenceRemarks`

---

## Nominatim に渡す検索文字列

### 基本方針

最も具体的な地名を先頭にし、存在する値だけをカンマ区切りで連結する。

優先順は以下。

1. `dwc:locality`
2. `dwc:island`
3. `dwc:islandGroup`
4. `dwc:waterBody`
5. `dwc:municipality`
6. `dwc:county`
7. `dwc:stateProvince`
8. `dwc:country`

`dwc:locality` が存在しない場合のみ、先頭要素として `dwc:verbatimLocality` を fallback に使う。

### 正規化

- 前後空白を除去する
- 空文字は使わない
- 同じ文字列が複数項目に入っている場合は重複除去する
- 重複判定は大文字小文字を区別しない
- `countrycodes` は生成しない

例。

```text
locality      = Arashiyama
municipality  = Kyoto
stateProvince = Kyoto
country       = Japan
```

Nominatim query。

```text
Arashiyama, Kyoto, Japan
```

---

## Geocoding を行う条件

Occurrence登録時、frontend入力をRDFとして検証・正規化した後、保存前に判定する。

### Geocodingしない

- `dwc:decimalLatitude` と `dwc:decimalLongitude` の両方が存在する

この場合は元データの座標をそのまま保存する。

### Geocodingする

- `dwc:decimalLatitude` と `dwc:decimalLongitude` の両方が存在しない
- かつ、検索文字列を構築できるLocation情報が1項目以上存在する

### 部分座標

- latitude または longitude の片方だけが存在する場合は、Nominatimで欠損側を自動補完しない
- MVPではそのOccurrenceをGeocoding対象外とする
- 地図APIでは完全な緯度経度ペアを持たないOccurrenceは返さない

---

## Nominatim 結果の採用

- `limit=1` で検索する
- 1件返ればその座標を採用する
- 複数候補の確認画面は作らない
- Nominatimのランキングをそのまま信頼し、先頭候補を一意な結果として扱う

---

## Geocoding失敗時

Geocoding失敗は以下を区別する。

### 検索対象なし

Location情報から検索文字列を作れない場合。

- Nominatimを呼ばない
- Occurrenceは座標なしで通常保存する

### 0件

Nominatim が正常応答したが検索結果が0件の場合。

- Occurrence登録自体は失敗させない
- 座標なしで保存する
- 同一queryの0件結果はキャッシュしてよい

### 通信・サービス障害

- timeout
- HTTP 429
- HTTP 5xx
- JSON parse失敗など

この場合もOccurrence登録自体は失敗させない。

- Geocodingだけを諦め、元のOccurrence RDFを保存する
- HTTP障害は0件結果としてキャッシュしない
- 将来、再Geocoding機能を追加してよいがMVP対象外

---

## Nominatim アクセス制御

公開 Nominatim へ負荷を集中させないため、backend内で以下を行う。

### 直列化

- 1プロセス内の Nominatim request は必ず直列化する
- 外部request開始間隔は最低1秒とする
- 論文から複数Occurrenceを一括登録する場合も同じ制御を通す

### キャッシュ

- 同一の正規化済み検索文字列は再度 Nominatim に送らない
- cache key は実際に `q` に渡す正規化済み文字列とする
- 成功結果と0件結果をキャッシュする
- HTTP/通信失敗はキャッシュしない
- MVPのキャッシュは backend process 内メモリでよい
- process restart をまたぐ永続キャッシュは将来拡張とする

---

## Geocoding結果のRDF保存

Nominatimで座標を生成できた場合、Locationノードに以下を追加する。

```nq
<.../locations/1> <http://rs.tdwg.org/dwc/terms/decimalLatitude> "35.0116"^^<http://www.w3.org/2001/XMLSchema#decimal> <https://bio-database.net/graphs/occurrences> .
<.../locations/1> <http://rs.tdwg.org/dwc/terms/decimalLongitude> "135.7681"^^<http://www.w3.org/2001/XMLSchema#decimal> <https://bio-database.net/graphs/occurrences> .
<.../locations/1> <http://rs.tdwg.org/dwc/iri/georeferenceSources> <https://nominatim.openstreetmap.org/> <https://bio-database.net/graphs/occurrences> .
<.../locations/1> <http://rs.tdwg.org/dwc/terms/georeferenceProtocol> "Nominatim search; first-ranked result selected automatically" <https://bio-database.net/graphs/occurrences> .
<.../locations/1> <http://rs.tdwg.org/dwc/terms/georeferencedDate> "2026-09-02"^^<http://www.w3.org/2001/XMLSchema#date> <https://bio-database.net/graphs/occurrences> .
```

### `georeferenceSources`

Nominatim由来であることは以下のIRIで表す。

```text
predicate: http://rs.tdwg.org/dwc/iri/georeferenceSources
object:    https://nominatim.openstreetmap.org/
```

地図表示上の座標由来判定は `verbatimLatitude` / `verbatimLongitude` の有無では行わない。

判定は以下とする。

```text
georeferenceSources == https://nominatim.openstreetmap.org/
    -> nominatim

それ以外
    -> original
```

`georeferenceSources` が単に存在するかどうかでは判定しない。他のgeoreference sourceを持つ通常データをNominatim由来と誤判定しないためである。

`verbatimLatitude` / `verbatimLongitude` は原資料の表記を保存する必要がある場合だけ使用し、地図表示判定のために必須追加しない。

---

## 地図API

### Endpoint

全件取得。

```text
GET /occurrences/map
```

任意Darwin Core条件での絞り込み。

```text
POST /occurrences/map/search
Content-Type: application/json
```

`POST /occurrences/map/search` の `filters` は `POST /occurrences/search` と同じ形式を使う。
複数filterはANDで評価する。

```json
{
  "filters": [
    {
      "predicate": "http://rs.tdwg.org/dwc/terms/stateProvince",
      "value": "Kyoto",
      "value_type": "literal",
      "match": "exact"
    }
  ]
}
```

### 基本動作

- bbox filterはMVPでは受け付けない
- 完全な `decimalLatitude` / `decimalLongitude` ペアを持つOccurrenceを返す
- 非ログインユーザーにはpublic Occurrenceだけを返す
- 通常ユーザーにはpublic + 自分のprivate Occurrenceを返す
- adminには全Occurrenceを返す
- private Occurrenceの存在を権限外ユーザーへ漏らさない

### Response

GeoJSON `FeatureCollection` を返す。

```json
{
  "type": "FeatureCollection",
  "features": [
    {
      "type": "Feature",
      "id": "occurrence-uuid",
      "geometry": {
        "type": "Point",
        "coordinates": [135.7681, 35.0116]
      },
      "properties": {
        "occurrenceId": "occurrence-uuid",
        "occurrenceUri": "https://bio-database.net/occurrences/occurrence-uuid",
        "scientificName": "Example species",
        "eventDate": "2026-01-01",
        "locality": "Kyoto City",
        "municipality": "Kyoto",
        "county": null,
        "stateProvince": "Kyoto",
        "country": "Japan",
        "coordinateSource": "nominatim"
      }
    }
  ]
}
```

GeoJSON座標順は `[longitude, latitude]` とする。

`coordinateSource` は以下の2値。

- `original`
- `nominatim`

`georeferenceSources` が Nominatim のIRIと一致する場合だけ `nominatim` とする。

---

## フロントエンド地図画面

### Path

```text
/map
```

### MVP表示

- MapLibre GL JS で地図を表示する
- 背景地図は OpenFreeMap Liberty style を使う
- 背景地図は OpenStreetMap 由来の詳細なvector mapを表示する
- 初期表示は全件相当の地図データを読み込む
- 任意DwC条件を適用する場合は `POST /occurrences/map/search` を使う
- 検索条件UIはデータ検索画面と共通化し、複数条件はANDとする
- GeoJSONのPointを描画する
- `coordinateSource=original` と `coordinateSource=nominatim` は別layerにする
- 両layerは見た目を変える
- Nominatim由来座標の最終的な表現方法（別色pin、不確実性円など）は未確定のため、MVPでは明確に識別できる簡易表現とする
- Pointをクリックすると以下を表示する
  - scientificName
  - locality
  - municipality / stateProvince / country の存在する値
  - eventDate
  - 座標由来（Original coordinates / Geocoded by Nominatim）
  - Occurrence詳細ページへのリンク

### 地図style

既定値は以下とする。

```text
https://tiles.openfreemap.org/styles/liberty
```

- `NEXT_PUBLIC_MAP_STYLE_URL` で別のMapLibre style URLへ差し替え可能にする
- 環境変数未指定時は OpenFreeMap Liberty を使う
- OpenFreeMap public instanceを利用するためAPI keyは不要
- 背景地図の描画エンジンは MapLibre、背景データ/style providerは OpenFreeMap と役割を分離する
- Nominatimは背景地図providerではなく、座標を持たないOccurrenceの地名Geocoding専用とする
- 将来自前OpenMapTiles等へ移行する場合は `NEXT_PUBLIC_MAP_STYLE_URL` を差し替える方針とする

---

## OpenAPI

`GET /occurrences/map` と `POST /occurrences/map/search` をOpenAPIへ追加する。

GeoJSON response schemaはbackend DTOとして定義する。

---

## テスト要件

### Geocoding service

最低限以下をテストする。

- localityが検索文字列の先頭になる
- localityがなければ verbatimLocality を使う
- hierarchyの重複文字列を除去する
- 完全な元座標ペアがあればGeocoderを呼ばない
- 座標がなく地名があればGeocoderを呼ぶ
- Nominatim結果の座標と `georeferenceSources` がRDFに追加される
- 0件でもOccurrence保存を継続する
- 同じqueryはcache hitになり外部requestを繰り返さない

### 地図API

最低限以下をテストする。

- longitude, latitude の順でGeoJSONを返す
- Nominatim sourceを持つ場合 `coordinateSource=nominatim`
- それ以外は `coordinateSource=original`
- 完全な座標ペアを持たないOccurrenceは返さない
- 非ログインではprivateを返さない

### フロントエンド

- map APIのFeatureCollectionを読み込める
- original / nominatim を別layerとして描画する
- popupからOccurrence詳細へ遷移できる
- `NEXT_PUBLIC_MAP_STYLE_URL` 未指定時に OpenFreeMap Liberty を既定styleとして使用する

---

## MVP対象外

- bbox filter
- taxon filter
- 年代 filter
- cluster
- heatmap
- GeoSPARQL spatial index
- 市区町村polygon表示
- Nominatim結果候補のユーザー選択
- Geocoding失敗記録の再処理UI
- 永続Geocoding cache
- 不確実性円の最終デザイン
