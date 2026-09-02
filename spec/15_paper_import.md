# 15. 論文PDFインポート

## 目的

論文PDFから書誌情報とOccurrence候補を抽出し、ユーザー確認後に通常のOccurrence登録フローへ渡すための仕様を定義する。

このspecは `feature-paper-import` の実装変更に合わせて更新する。paper importの挙動・保存先・API・LLM設定・抽出プロンプト・レビューUI・登録方式を変更した場合は、必要に応じて本ファイルも同じ変更単位で更新する。

---

## 基準点

LLM抽出について、以下のコミットを「ある程度まともに抽出できる既知の基準点」とする。

`d2c5e52cd384338e6a8b1bd2df24a1307aeb53d2`

この状態は `paper-import-baseline-d2c5e52` ブランチにも固定して保存する。

また、`eventDate` 導入直前の状態を次のコミットで固定する。

`d1b8af499bccee5efd6c3b9bc304b6331617e409`

この状態は `paper-import-baseline-before-event-date` ブランチに保存する。`eventDate` 追加後に抽出エラーや品質悪化が発生した場合は、このブランチを「年月日情報を追加する直前の正常系」として比較・復旧に使用する。

今後プロンプトやLLM設定を試行して結果が悪化した場合、これらの基準コミットまたは基準ブランチへ戻せる状態を維持する。基準ブランチ上では新しい実験を直接行わない。

---

## 基本方針

- PDF本体は Garage に保存する
- 論文の管理情報・書誌情報は PostgreSQL の `papers` に保存する
- Occurrence本体は既存仕様どおりRDFとしてFusekiへ保存する
- LLM抽出結果は自動確定せず、ユーザー確認後に登録する
- paper import専用の別Occurrence形式を最終保存形式にしない
- `papers` をpaper管理のPostgreSQL上のsource of truthとする

---

## LLM抽出

実装は `backend/src/features/paper_import/llama.rs` を中心とする。

入力は次を組み合わせる。

- GROBID等で抽出した論文テキスト
- PDF各ページをレンダリングした画像

出力はJSON Schemaで制約し、基本形は次とする。

```json
{
  "occurrences": [
    {
      "scientificName": "Metaphire hilgendorfi",
      "locality": "奈良県香芝市真美ヶ丘",
      "eventDate": "1998-06"
    }
  ]
}
```

LLMレスポンスschemaでは `scientificName`、`locality`、`eventDate` を各Occurrenceのキーとして要求する。`locality` と `eventDate` は情報がない場合 `null` を許可する。

現時点ではLLMレスポンスschemaに緯度経度を要求しない。`OccurrenceCandidate` に座標用Optionフィールドが残っていても、LLMの基本出力は `scientificName`、`locality`、`eventDate` とする。

### 生成設定

基準点 `d2c5e52...` では以下を使用する。

```text
temperature = 0.7
top_p = 0.8
top_k = 20
min_p = 0.0
presence_penalty = 2.0
repeat_penalty = 1.0
max_tokens = 32768
stream = false
enable_thinking = false
```

設定を変更する場合は、抽出品質の比較ができるよう変更理由を本specまたはコミットに残す。

### 現行モデル運用方針

従来モデルでは、`P. agrestis` のような属名略記が残ることや、複数地点が1つのlocalityへまとめられることがあった。

推論サーバー側でより高性能なモデルへ変更した結果、この2点は実用上かなり改善した。処理速度は低下したが、現時点では抽出品質を優先し、この高性能モデルを継続利用する。

- 属名略記と地点分割について、現時点では複雑なbackend後処理や再問い合わせ処理を追加せず、モデル性能とプロンプトで対応する
- 速度低下は当面許容する
- 今後、速度・精度を見ながら段階的に改善する
- モデル変更で品質が悪化した場合は基準ブランチと比較できる状態を維持する
- リポジトリ上の `LLAMA_MODEL = "local-model"` だけでは実際の推論モデルのweightsを特定できないため、推論サーバー側のモデル変更も運用上の重要な変更として扱う

---

## プロンプト管理

抽出プロンプト本文はRustソースへ長大な文字列として直接書かず、次のファイルで管理する。

`backend/src/features/paper_import/prompt.txt`

`llama.rs` では次の形で参照する。

```rust
pub const OCCURRENCE_EXTRACTION_PROMPT: &str = include_str!("prompt.txt");
```

`include_str!` はcompile時にファイル内容を埋め込むため、`prompt.txt` の変更後はbackendの再ビルドが必要である。

プロンプトを変更するときは、Rustの通信処理・sampling設定・JSON Schemaを同時に不用意に変更しない。プロンプトだけの実験なのか、生成設定の実験なのかを分離する。

---

## 基準プロンプトの重要挙動

`d2c5e52...` 時点のプロンプトでは次を重視する。

- 1 Occurrence = 1分類群 × 1地点
- 複数地点は別Occurrenceへ分割する
- `P. agrestis` 等の属名省略を残さず、文脈と一般的分類学知識を用いて完全形へ推定展開する
- 属名展開は100%の確証を要求しない
- Occurrenceそのもの、種小名、地点は論文にないものを創作しない
- scientificName + locality が同一なら重複排除する
- JSONを一度出力したら繰り返し生成しない

この基準点では、日本のlocalityに都道府県名を推測補完するルールと `eventDate` 抽出はまだ採用していない。

---

## 現行プロンプトでの属名略記ルール

基準点の抽出品質を維持しつつ、`P. agrestis` のような属名略記がreview UIまで残る問題への対策として、現行 `feature-paper-import` では略記禁止をさらに強化する。

- scientificName の先頭属名トークンに `P.`、`M.`、`A.`、`Ph.` 等の略記を残してはならない
- 略記が1件でも残ったJSONは不正な出力として扱う、という指示をプロンプト上で明示する
- 100%の確証がなくても、最も可能性が高い完全な属名を1つ選ぶ
- 確証不足を理由に略記を残したり、そのOccurrence自体を捨てたりしない
- 一般的な分類学知識の利用を許可する
- JSON出力直前に全 scientificName の先頭属名トークンを再確認させる

ただし、処理負荷を増やさないため同じ指示を多数の節で繰り返さない。略記禁止を短いハード制約として集中して記述し、従来の冗長な推定説明・最終確認の重複は削減する。

---

## 現行プロンプトでのlocality正規化

日本のlocalityについて、地点名自体は論文にある情報を維持しつつ、都道府県が省略されている行政地名は論文文脈から上位行政区画を補完する。

- 市・区・郡・町・村などから始まり都道府県がないlocalityは、論文中から最も可能性が高い都道府県を推定して先頭へ追加する
- 推定には論文タイトル、調査地域説明、周辺本文、表・図見出し、同じ論文中の他の地名を使う
- 100%の確証は要求しない
- 地点そのものを別の地点へ置換してはならず、既存地点の上位行政区画だけを補う
- すでに都道府県がある場合は重複追加しない

例。

```text
香芝市真美ヶ丘
→ 奈良県香芝市真美ヶ丘
```

この補完は「存在しない地点を創作する」こととは区別し、既存localityの行政階層正規化として扱う。

---

## eventDate抽出と正規化

論文中の採集・観察・記録年月日は Darwin Core の `dwc:eventDate` として扱う。

- `verbatimEventDate` は使用しない
- LLMが論文中の日付表現を直接正規化して `eventDate` を返す
- 全Occurrenceで `eventDate` キーを出力し、日付を特定できなければ `null` とする
- 出版年ではなく、そのOccurrenceに対応する採集日・観察日・記録日を取得する
- 年だけ分かる場合は `YYYY`
- 年月まで分かる場合は `YYYY-MM`
- 年月日まで分かる場合は `YYYY-MM-DD`
- 明示された期間は `開始/終了` とする。例: `1998-05/1998-07`
- 不明な月・日を `01` などで補完して精度を偽らない
- 和暦などは西暦へ変換できる場合に変換する
- `5月上旬` 等の曖昧な日付は、確実に表現できる精度まで落として `YYYY-MM` 等とする
- 上記形式へ安全に変換できない場合は、元表記をそのまま返さず `null` とする
- eventDateの取得・正規化に失敗してもOccurrence自体を捨てない

### eventDateの耐障害性

初期実装ではJSON Schemaの `pattern` とRust側の厳格な日付検証を二重に適用していた。このため、1件でも `1998-13` のような不正値が混ざると抽出全体が `InvalidOccurrence` となり、正常なscientificName/localityまで失う問題があった。

現行実装では次の方針とする。

- JSON Schemaでは `eventDate` を `string | null` とし、regex `pattern` は使用しない
- 正規化形式の指示はプロンプトで明確に行う
- backendはLLMレスポンスをparseした後に各eventDateを個別に確認する
- `YYYY`、`YYYY-MM`、`YYYY-MM-DD`、または同形式の `開始/終了` として受理できる値はtrimして保持する
- 明らかに不正なeventDateは、そのOccurrenceの `eventDate` だけを `null` にする
- eventDate単独の不正を理由にOccurrence全体や抽出リクエストを失敗させない
- scientificNameが空、JSONそのものが壊れている等、Occurrenceとして成立しないエラーは従来どおり失敗とする

`eventDate` はLLM候補から抽出APIレスポンスへ引き継ぎ、review UIでは値が存在する場合のみ初期行として表示する。ユーザー確認後は通常のN-Quads生成に含め、既存のOccurrence登録処理へ渡す。backendの通常RDFルーティングにより `dwc:eventDate` はEvent側へ保存される前提とする。

---

## review UIの初期項目

LLM抽出後の各Occurrenceは次を初期表示する。

- GBIF解決あり: `分類`、`scientificName`、`locality`、および取得できた場合 `eventDate`
- GBIF解決なし: `scientificName`、`locality`、および取得できた場合 `eventDate`
- `eventDate = null` の場合は空のeventDate行を自動追加しない
- ユーザーは従来どおり任意のDarwin Core項目を追加・削除・編集できる

---

## 不採用・変更済み案

### 学名省略を確証不足ならそのまま残す

不採用。`P. agrestis` のような略記がreview UIまで残る問題が大きいため、最も妥当な完全属名を積極的に推定する方針を採用した。

### 学名省略を解決できなければOccurrence自体を捨てる

不採用。Occurrenceの取りこぼしが増えるため、属名のみ積極的推定を許可する。

### 略記禁止を長い説明の追加だけで強化する

不採用。プロンプトが長くなり処理負荷や指示競合が増えるため、「略記が1件でも残れば不正」という短いハード制約を中心にする。

### 属名略記・地点分割をすぐ複雑なbackend後処理で補正する

現時点では不採用。高性能モデルへの変更で実用上改善したため、まずは速度低下を許容して単純な構成を維持する。再発率が高くなった場合に再検討する。

### `verbatimEventDate` を併用する

現時点では不採用。paper importではLLMが日付を `eventDate` へ正規化して返す単純な構成を採用し、原表記を別フィールドには保存しない。

### 不明な月日を補って完全な日付にする

不採用。論文が持つ精度以上の日付を生成することになるため、年・年月など分かる精度のまま保存する。

### eventDateの1件不正で抽出全体を失敗させる

不採用。日付は追加情報であり、正しく抽出できたscientificName/localityまで破棄する理由にならない。不正なeventDateだけを `null` に落とす。

### JSON SchemaのregexでeventDate形式を完全に縛る

不採用。llama.cpp側のstructured outputを必要以上に複雑にし、日付抽出追加前には存在しなかった失敗要因を増やすため。schemaは型とキー構造を固定し、日付形式はプロンプトとbackendの軽量sanitizationで扱う。

### 長大なプロンプトを `llama.rs` にハードコードする

不採用。プロンプト試行のたびにRustコード自体の編集が必要になり、差分確認もしにくいため `prompt.txt` へ分離する。

### プロンプト変更とsampling変更を同時に行う

原則不採用。どの変更が品質へ影響したか分からなくなるため、比較実験では変更要因を分離する。

### 基準点を上書きし続ける

不採用。抽出品質が悪化した場合に戻れなくなるため、既知の基準コミットと専用ブランチを保持する。

---

## 実装変更時の注意

- paper import関連の実装を変更したら、本specとの整合性を確認する
- APIを変更した場合は `spec/11_api_contract.md` とOpenAPIも確認する
- frontendのreview UIを変更した場合は `spec/12_frontend_screens.md` も確認する
- RDF保存形式やprovenanceを変更した場合は `spec/05_occurrence_rdf.md` も確認する
- テスト方針を変更した場合は `spec/14_testing_strategy.md` も確認する
- 仕様に影響しない純粋なリファクタリングであればspec本文変更は必須ではないが、設計上の前提や運用上の注意が変わる場合は必ず記録する

---

## テスト上の最低確認

プロンプト変更・外出しのような内部変更でも、少なくとも次を維持する。

- request先頭に `OCCURRENCE_EXTRACTION_PROMPT` が入る
- JSON Schemaが `scientificName`、`locality`、`eventDate` を要求する
- JSON SchemaのeventDateにregex `pattern` を付けない
- sampling設定が意図せず変わっていない
- `prompt.txt` に属名略記の禁止と完全形への展開指示が存在する
- `prompt.txt` に日本の都道府県補完ルールが存在する
- `prompt.txt` に `eventDate` のISO形式正規化ルールが存在する
- validなeventDateは保持される
- invalidなeventDateは `null` に変換され、Occurrence自体は失敗しない
- valid responseを従来どおりparseできる
