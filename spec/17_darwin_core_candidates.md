# Darwin Core候補とBio-Database用メタデータ

## 1. 目的

Darwin Core語彙全体と、Bio-Databaseで各語彙をどのように利用するかというアプリ固有メタデータをFusekiで管理する。

実行時の候補生成ではFusekiを問い合わせ先とし、`frontend/content/terms/darwin-core/list.csv` を直接フィルターとして使用しない。

---

## 2. 基本方針

- Darwin Core公式語彙はFusekiに保持する。
- Bio-Database固有の語彙設定もFusekiに保持する。
- Darwin Core公式語彙とBio-Database固有設定はnamed graphを分離する。
- backendは実行時に`list.csv`を読んで候補を絞り込まない。
- `list.csv`はGit管理可能な元データとして残し、FusekiへBio-Database固有設定を投入するためのseed/import元として利用できる。

使用する主なnamed graphは以下とする。

```text
https://bio-database.net/graphs/vocabularies/darwin-core
https://bio-database.net/graphs/app/occurrence-profile
```

- `.../vocabularies/darwin-core`: Darwin Core語彙本体
- `.../app/occurrence-profile`: Bio-Databaseでの利用可否や表示情報などのアプリ固有設定

この分離によりDarwin Core語彙を再生成・再投入しても、Bio-Database固有設定を独立して管理できるようにする。

---

## 3. Bio-Database用メタデータ

最低限、各語彙について以下をFuseki側で表現できるようにする。

- Bio-Databaseで新規入力候補として使用するか
- 日本語表示名

`list.csv`では現在それぞれ以下の列に対応する。

```text
use_at_bio_database
label_ja
```

将来的には必要に応じて以下もBio-Database固有メタデータとして追加できる。

- 表示順
- 入力形式
- 必須・推奨区分
- カテゴリ
- literal / IRI の入力制約

RDF上の具体的なBio-Database独自predicate URIは、Fusekiへの投入処理を実装する際に確定する。Darwin Core公式語彙のpredicateとして偽装せず、Bio-Database独自namespaceを使用する。

概念例:

```turtle
# predicate URIは設計例。正式URIは投入処理実装時に確定する。
dwc:scientificName
    bio:useAtBioDatabase true ;
    bio:labelJa "学名"@ja .
```

これらのtripleはBio-Database用named graphに格納し、Darwin Core公式語彙graphそのものは改変しない。

---

## 4. `list.csv`の位置づけ

`frontend/content/terms/darwin-core/list.csv`は、Bio-Databaseで利用する語彙設定を人間がGit上で管理・レビューするための元データとして利用できる。

ただし、Rust backendが実行時に`include_str!`等で`list.csv`を読み、Fusekiの結果と照合して候補を決定する構成にはしない。

想定する流れは以下。

```text
list.csv
   ↓ setup / seed / import
Fuseki
 ├─ Darwin Core vocabulary graph
 └─ Bio-Database occurrence-profile graph
          ↓
      Rust backend
          ↓
       frontend
```

この構成では実行時の問い合わせ先はFusekiに一本化される。

---

## 5. Darwin Core候補API

対象API:

```text
GET /vocabularies/darwin-core
```

最終的な処理方針は以下。

1. Fuseki内のDarwin Core語彙graphを対象にする。
2. Bio-Databaseの`occurrence-profile` graphと結合する。
3. Bio-Databaseで使用する設定が有効な語彙だけを新規入力候補として返す。
4. 日本語表示名など、UIに必要なメタデータもFusekiから取得して返す。
5. 候補の識別にはIRIを使用する。表示ラベルは識別子として扱わない。

既存Occurrenceに、現在の新規入力候補ではないpredicateが含まれていても、自動削除・自動変換はしない。

---

## 6. 移行状態

この方針への変更時点では、`list.csv`を実行時フィルターとして利用する実装を撤回する。

そのため、FusekiへのBio-Database固有メタデータ投入と、そのメタデータを使ったSPARQL絞り込みが実装されるまでは、`GET /vocabularies/darwin-core`はFusekiに存在するDarwin Core語彙を全件返す。

これは移行中の一時的な挙動であり、最終仕様は前節のFuseki内メタデータによる絞り込みとする。

---

## 7. 受け入れ条件

移行時点:

- backendに`list.csv`を実行時読み込みする依存がない。
- `darwin_core_policy`によるCSVパース・allowlist生成がない。
- `GET /vocabularies/darwin-core`はFusekiから取得したDarwin Core語彙をそのまま候補として返す。

最終形:

- Darwin Core公式語彙graphとBio-Database固有設定graphが分離されている。
- `list.csv`のBio-Database固有情報をFusekiへ投入できる。
- backendはFuseki内のBio-Database固有設定を使って候補を絞り込む。
- 日本語表示名などのUI用メタデータもFusekiから取得できる。
- runtimeの候補判定に`list.csv`を直接使用しない。
