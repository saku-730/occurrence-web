# 18. ジオコーディング / ABR / Nominatim

## 基本方針

Occurrence の住所情報から座標を補完する場合、役割を次のように分離する。

- **ABR（アドレス・ベース・レジストリ）は住所の正規化・階層分割だけに使う**
- **緯度経度を決定するジオコーダーは Nominatim とする**
- ABR が返す緯度経度は Bio-Database の geocoding 結果として使用しない
- RDF に geocoding の出典として記録するのは Nominatim のみとする
- ABR は前処理であり、`georeferenceSources` には記録しない

この役割分担を崩して、ABR の座標を保存したり、ABR と Nominatim の座標を混在させたりしない。

---

## 実行タイミング

ジオコーディングは **Occurrence 登録時** に backend が実行する。

frontend は ABR や Nominatim を直接呼び出さない。

処理順序は次のとおり。

1. frontend から N-Quads を受信する
2. backend が入力を parse / validate する
3. Location に属する住所系 Darwin Core 項目を取り出す
4. 住所文字列を ABR に渡して正規化・階層分割する
5. ABR の正規化結果から Nominatim に渡す検索文字列を作る
6. Nominatim でジオコーディングする
7. 成功した場合だけ座標と Nominatim の出典を RDF に追加する
8. 通常の Location 中間ノード正規化を行う
9. Fuseki に保存する

ABR を飛ばして生の日本語住所を直接 Nominatim に渡す実装にはしない。

---

## ABR に渡す住所

入力 RDF に存在する住所項目を、広い行政区分から狭い行政区分の順に連結する。

```text
dwc:country
dwc:stateProvince
dwc:county
dwc:municipality
dwc:locality
```

存在しない項目は飛ばす。

例。

```text
Japan + 東京都 + 千代田区 + 紀尾井町1-3
```

ABR の目的は、この住所文字列の表記揺れを吸収し、都道府県、市区町村、町域、番地等の階層に正規化・分割することである。

ABR の結果に座標が含まれていても、その座標は保存しない。

---

## Nominatim に渡す住所

Nominatim には、ABR で正規化・分割した住所情報を再構成して渡す。

原則として、取得できた範囲で最も具体的な住所を使用する。

- `locality` 相当の情報を最優先する
- municipality / county / stateProvince / country 相当の上位情報を組み合わせて曖昧性を減らす
- ABR が返した正規化済み表記を使用する

同一住所に対して raw input と ABR normalized input を別々に Nominatim へ投げるような多重問い合わせは行わない。

---

## Nominatim アクセス制御

Nominatim へのアクセスは backend 内で **直列化** する。

- 同時に複数リクエストを送信しない
- 公開 `nominatim.openstreetmap.org` を利用する場合は最大 1 request / second を超えない
- アプリケーションを識別できる `User-Agent` を送信する
- 同じ検索住所に対する結果は backend でキャッシュする
- キャッシュヒット時は Nominatim へ再問い合わせしない

キャッシュキーには、Nominatim へ実際に渡す ABR 正規化後の検索文字列を使用する。

MVP のキャッシュはプロセス内キャッシュでよい。永続キャッシュは将来拡張とする。

---

## ジオコーディング成功時の RDF

Nominatim が座標を返した場合だけ、Location ノードに次を追加する。

```text
dwc:decimalLatitude
dwc:decimalLongitude
dwciri:georeferenceSources
```

完全 URI は次のとおり。

```text
http://rs.tdwg.org/dwc/terms/decimalLatitude
http://rs.tdwg.org/dwc/terms/decimalLongitude
http://rs.tdwg.org/dwc/iri/georeferenceSources
```

`dwciri:georeferenceSources` の目的語は IRI とし、Nominatim を示す次の URI を保存する。

```text
https://nominatim.openstreetmap.org/
```

例。

```nq
<https://bio-database.net/occurrences/{occurrence_id}/locations/1> <http://rs.tdwg.org/dwc/terms/decimalLatitude> "35.681236"^^<http://www.w3.org/2001/XMLSchema#decimal> <https://bio-database.net/graphs/occurrences> .
<https://bio-database.net/occurrences/{occurrence_id}/locations/1> <http://rs.tdwg.org/dwc/terms/decimalLongitude> "139.767125"^^<http://www.w3.org/2001/XMLSchema#decimal> <https://bio-database.net/graphs/occurrences> .
<https://bio-database.net/occurrences/{occurrence_id}/locations/1> <http://rs.tdwg.org/dwc/iri/georeferenceSources> <https://nominatim.openstreetmap.org/> <https://bio-database.net/graphs/occurrences> .
```

座標値そのものは Nominatim のレスポンスを使用する。

---

## Nominatim 由来データの判定

Bio-Database が自動ジオコーディングした座標かどうかは、`verbatimLatitude`、`verbatimLongitude`、`countryCode` 等の有無では判定しない。

次の RDF が存在するかで判定する。

```nq
?location <http://rs.tdwg.org/dwc/iri/georeferenceSources> <https://nominatim.openstreetmap.org/> ?graph .
```

つまり、Nominatim による自動補完であることを示す唯一の判定根拠は `dwciri:georeferenceSources` とする。

ユーザーが元データとして入力した座標に対して、backend が勝手に Nominatim の source を付与してはならない。

---

## ABR の provenance を RDF に入れない理由

ABR はこの処理では geocoder ではなく、Nominatim に渡す住所を作るための正規化・分割処理として使用する。

したがって、次のような RDF は生成しない。

```text
dwciri:georeferenceSources <https://www.digital.go.jp/policies/base_registry_address>
```

ABR の URI を `georeferenceSources` に追加すると、最終的な座標の出典が ABR であるように見えるため禁止する。

---

## 失敗時の扱い

### ABR 失敗

住所の正規化・分割ができなかった場合は、その登録処理では Nominatim ジオコーディングを行わない。

- 元の住所 RDF は保持する
- backend 生成の緯度経度は追加しない
- `dwciri:georeferenceSources` は追加しない
- Occurrence 登録そのものは継続する

### Nominatim 失敗

以下を Nominatim 失敗として扱う。

- HTTP エラー
- timeout
- 正常レスポンスだが候補が0件
- 緯度または経度を取得できない
- レスポンスを正しく parse できない

失敗時は次のとおり。

- 元の RDF を保持する
- backend 生成の緯度経度は追加しない
- `dwciri:georeferenceSources` は追加しない
- Occurrence 登録そのものは継続する

ジオコーディング失敗だけを理由に Occurrence 登録全体を失敗させない。

---

## 既存座標との関係

frontend から元々 `dwc:decimalLatitude` / `dwc:decimalLongitude` が送られている場合、その座標はユーザー入力 RDF として扱う。

Nominatim による自動生成座標とユーザー入力座標を区別するときは `dwciri:georeferenceSources` を使用する。

自動ジオコーディング処理が既存座標をどの条件で上書きするかについては、明示的な別仕様を追加するまでは **既存座標を上書きしない**。

---

## テスト要件

最低限、次を自動テストする。

1. `country -> stateProvince -> county -> municipality -> locality` の順で ABR 入力住所が組み立てられる
2. 空の住所項目が連結結果に混入しない
3. ABR の正規化・分割結果を経由して Nominatim が呼ばれる
4. ABR を経由せず raw address を直接 Nominatim に渡さない
5. ABR が返した座標を RDF に保存しない
6. Nominatim 成功時に `dwc:decimalLatitude` / `dwc:decimalLongitude` が追加される
7. Nominatim 成功時だけ `dwciri:georeferenceSources <https://nominatim.openstreetmap.org/>` が追加される
8. ABR の URI が `georeferenceSources` に入らない
9. ABR 失敗時に元 RDF のまま登録処理を継続する
10. Nominatim 0件時に元 RDF のまま登録処理を継続する
11. Nominatim HTTP エラー / timeout / parse error で登録全体を失敗させない
12. 同じ正規化住所を複数回処理した場合にキャッシュが使われる
13. Nominatim リクエストが並列実行されない
14. Nominatim 由来判定が `dwciri:georeferenceSources` によって行われる
15. ユーザー入力済み座標を自動ジオコーディングが上書きしない

外部 ABR / Nominatim 本番サービスを通常の unit test から直接呼ばず、HTTP client を mock / stub 化して決定的にテストする。
