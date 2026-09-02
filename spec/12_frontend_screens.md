# 12. フロントエンド画面要件

## 基本方針

- ユーザーが RDF / N-Quads を知らなくても操作できるUIにする
- 作成・編集はフォーム入力を基本とする
- フロントエンド内部でフォーム入力から N-Quads を生成する
- 詳細画面ではすべての RDF 項目を表示するが、人間向けに整形する

---

## MVP画面

MVPで必要な画面。

- 仮登録画面
- 仮登録完了/案内画面
- 本登録画面
- ログイン画面
- パスワードリセット申請画面
- パスワードリセット実行画面
- オカレンス一覧画面
- オカレンス検索画面
- オカレンス詳細画面
- オカレンス作成画面
- オカレンス編集画面
- 公開閲覧ページ

MVP対象外。

- 管理者用ユーザー管理画面
- 監査ログ閲覧画面
- エクスポート画面
- メディア管理補助画面

---

## 仮登録画面

### 入力

- email

### 正常系

- 未登録emailなら確認メール送信案内を表示する
- 登録済emailなら「すでに登録済みです」と表示し、メールは送らない

### 異常系

- email形式不正
- メール送信失敗
- サーバーエラー

---

## 本登録画面

### 入力

- username
- password

### 正常系

- 本登録完了後、ログイン画面へ誘導する
- 自動ログインしない

### 異常系

- token不正
- token期限切れ
- password長不正

---

## ログイン画面

### 入力

- email
- password

### 正常系

- ログイン成功後、オカレンス一覧へ遷移する

### 異常系

- 存在しないemailとパスワード間違いは同じ文言を表示する

---

## オカレンス作成画面

### 方針

- RDF/N-Quads直接入力画面にはしない
- フォーム選択・入力で作成できるようにする
- フロントエンドが内部で N-Quads を生成する
- frontendは中間ノードを意識せず、従来どおり項目の述語・目的語セットを送る
- Identification、Event、Locationへの振り分けと中間ノード生成はbackendが行う

### 入力候補

項目は固定必須ではない。  
ユーザーが必要な述語・値を追加できるUIを想定する。

Darwin Coreの入力候補は `GET /vocabularies/darwin-core` から取得する。

- backendはFusekiの `https://bio-database.net/graphs/app/occurrence-profile` を参照し、`https://bio-database.net/terms/useAtBioDatabase true` の語彙だけを候補として返す。
- 候補の表示名は同graphの `skos:prefLabel` のうち言語タグが `@ja` の値を優先する。
- 日本語 `skos:prefLabel` が存在しない場合のみ、Darwin Core vocabulary graphの `localName` を表示名として使用する。
- 保存時のpredicateは表示名ではなく、候補に対応するIRIを使用する。
- この表示方針はオカレンス新規作成、オカレンス編集、論文取り込み後のOccurrence編集で共通とする。

MVPでは最低限、以下のような入力補助を用意してよい。

- `dwc:scientificName`
- 任意のURI値
- 任意のリテラル値
- `dcterms:accessRights`
- `dcterms:license`
- メディア添付

ただし、backend は `dwc:scientificName` を必須にはしない。

---

## オカレンス編集画面

### 方針

- RDF/N-Quadsを直接編集させない
- フォーム入力で編集できるようにする
- 更新は backend 側では RDF丸ごと置換として扱う
- 作成者と作成日時は変更できない

---

## オカレンス詳細画面

### 方針

- すべての RDF 項目を表示する
- 詳細APIが返すIdentification、Event、Locationの構造を平坦化せず解釈して表示する
- ただし RDFそのままの読みにくい表示にはしない
- 人間向けに整形する

### 表示例

- 種名など主要項目は目立たせる
- 作成日時・更新日時などのメタデータは小さく控えめに表示する
- URIはラベル表示できる場合はラベル化する
- URIの詳細確認もできるようにする
- メディアはプレビュー可能なものだけ表示する

---

## 検索画面

### MVP

- 検索述語選択
- MVPでは選択肢は `dwc:scientificName` のみ
- 検索値入力
- 空検索で一覧表示
- cursor-based pagination

---

<<<<<<< HEAD
## 論文インポート画面

### LLM抽出後のレビュー

論文からLLM抽出したOccurrence候補は、登録前に通常のOccurrence作成画面に近いフォームで確認・編集する。

初期表示する項目。

- GBIFで分類群を解決できた場合は `dwciri:toTaxon`（分類）
- `dwc:scientificName`
- `dwc:locality`
- LLMが採集・観察年月日を取得できた場合のみ `dwc:eventDate`

`eventDate` の扱い。

- LLMからは `YYYY`、`YYYY-MM`、`YYYY-MM-DD` または同形式の `開始/終了` として正規化済みの値を受け取る
- `eventDate = null` の候補には空のeventDate行を自動追加しない
- 表示されたeventDateはユーザーが登録前に編集・削除できる
- `verbatimEventDate` はpaper importの初期項目として使用しない
- 一括登録時は他の入力項目と同様にN-Quadsへ変換し、`dwc:eventDate` としてbackendへ送る

### 全Occurrenceへの項目一括適用

LLM抽出結果のレビュー画面上部に、現在の登録対象Occurrenceすべてへ共通の項目・値を適用するUIを設ける。

- 項目名は既存のDarwin Core項目候補UIと同じ候補リストを利用する
- 値は通常のOccurrence入力と同じ文字列/URI入力として扱う
- 例: `dwc:basisOfRecord = PreservedSpecimen` のような論文内で共通する値を全件へ一度に設定できる
- 選択したpredicateが存在しないOccurrenceには新しい入力行を追加する
- 同じpredicateが既に存在するOccurrenceでは重複行を追加せず、そのpredicateの既存値を指定値へ置き換える
- 一括適用後も各Occurrenceの個別フォームで値を再編集・削除できる
- `dwciri:toTaxon` はGBIF照合結果と `dwc:scientificName` が連動する特殊項目のため、この単純な一括適用UIの対象外とする
- 登録済みまたは一括登録処理中は一括適用UIを無効化する
- 一括適用はfrontend上のレビュー状態だけを更新し、適用操作だけでbackendへ保存しない。最終的な一括登録時に他の項目と一緒にN-Quads化する
=======
## Markdownコンテンツ表示

- `frontend/content/` 配下のMarkdown表示には標準的なMarkdownパーサライブラリを使用し、正規表現ベースの独自Markdownパーサは実装しない。
- CommonMark/GFM相当の基本構文として、見出し、段落、順序付き・順序なしリスト、リンク、引用、インラインコード、コードブロック、水平線、表を表示できるようにする。
- `/...`、`#...` などサイト内リンクは通常の同一タブ遷移とする。
- `http://`、`https://` の外部リンクだけを別タブで開き、`rel="noopener noreferrer"` を付与する。
- Markdown本文中の生HTMLは解釈せず、コンテンツファイルから任意HTMLを直接描画しない。

---

## Darwin Core用語集

- `/terms/darwin-core` は `frontend/content/terms/darwin-core.md` を表示する。
- 同ページには `frontend/content/terms/darwin-core/` 配下に存在する各Darwin Core用語Markdownへのリンク一覧を掲載する。
- 用語ページのURLは `/terms/darwin-core/{Markdownファイル名から.mdを除いた値}` とする。
- 一覧は用語名の先頭文字ごとに整理する。
- `template.md`、`list.csv`、一時ロックファイルなど、用語ページではない管理ファイルは一覧へ含めない。
- 括弧などURL上でエンコードが必要な文字を含むファイル名は、表示名は元のファイル名を維持し、リンクURL側だけパーセントエンコードする。
>>>>>>> main

---

## メディアUI

- ユーザー体験としてはオカレンス作成と同時にアップロードできる
- 内部的に別APIでもよい
- アップロード失敗時は分かりやすく表示する
- サイズ超過は可能なら送信前に検出する
