# 15. 論文PDFインポート

## 目的

論文PDFから書誌情報とOccurrence候補を抽出し、ユーザー確認後に通常のOccurrence登録フローへ渡すための仕様を定義する。

このspecは `feature-paper-import` の実装変更に合わせて更新する。paper importの挙動・保存先・API・LLM設定・抽出プロンプト・レビューUI・登録方式を変更した場合は、必要に応じて本ファイルも同じ変更単位で更新する。

---

## 基準点

LLM抽出について、以下のコミットを「ある程度まともに抽出できる既知の基準点」とする。

`d2c5e52cd384338e6a8b1bd2df24a1307aeb53d2`

この状態は `paper-import-baseline-d2c5e52` ブランチにも固定して保存する。

今後プロンプトやLLM設定を試行して結果が悪化した場合、このコミットまたは基準ブランチへ戻せる状態を維持する。基準ブランチ上では新しい実験を直接行わない。

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
      "locality": "Tokyo"
    }
  ]
}
```

現時点ではLLMレスポンスschemaに緯度経度を要求しない。`OccurrenceCandidate` に座標用Optionフィールドが残っていても、LLMの基本出力は `scientificName` と `locality` とする。

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

この基準点では、日本のlocalityに都道府県名を推測補完するルールはまだ採用していない。localityは可能な限り論文表記を維持する。

---

## 不採用・変更済み案

### 学名省略を確証不足ならそのまま残す

不採用。`P. agrestis` のような略記がreview UIまで残る問題が大きいため、最も妥当な完全属名を積極的に推定する方針を採用した。

### 学名省略を解決できなければOccurrence自体を捨てる

不採用。Occurrenceの取りこぼしが増えるため、属名のみ積極的推定を許可する。

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

プロンプト外出しのような内部変更でも、少なくとも次を維持する。

- request先頭に `OCCURRENCE_EXTRACTION_PROMPT` が入る
- JSON Schemaが従来どおりである
- sampling設定が意図せず変わっていない
- `prompt.txt` に基準プロンプトの重要指示が存在する
- valid responseを従来どおりparseできる
