# 15. 将来実装予定機能

## 目的

このファイルでは、仕様としては必要だが MVP では省略または簡略化する機能を管理する。

MVP では、主要なデータ登録・取得・検索・更新・削除の動作を優先する。  
ここに記載した機能は、MVP 完了後に実装する。

---

## CSRF token 検証

### MVPでの扱い

MVP では、session cookie に以下を設定することで基本的な CSRF リスクを下げる。

- `HttpOnly`
- `SameSite=Lax`
- `Path=/`

ただし、MVP では専用の CSRF token 検証は実装しない。

### 将来実装する内容

session token とは別に CSRF token を発行する。

状態変更系 API では、フロントエンドが CSRF token を明示的に送信する。

対象 API。

- `POST`
- `PUT`
- `PATCH`
- `DELETE`

バックエンドは、session cookie と CSRF token の両方を検証する。

例。

```http
Cookie: session=...
X-CSRF-Token: ...
```

CSRF token がない、または不正な場合は `403 Forbidden` を返す。

GET 系 API は CSRF token を要求しない。

---

## admin ロール権限

### MVPでの扱い

MVP では editor ユーザーを中心に実装する。

現状のバックエンドでは、ログインユーザーの role 取得は暫定実装であり、admin 権限の完全な判定は行わない。

### 将来実装する内容

PostgreSQL の users テーブルなど、永続化されたユーザー情報から role を取得する。

admin は以下を実行できる。

- 全 occurrence の閲覧
- 全 occurrence の更新
- 全 occurrence の削除
- 他ユーザーの role 変更

editor は以下を実行できない。

- 他人の private occurrence 閲覧
- 他人の occurrence 更新
- 他人の occurrence 削除
- 他ユーザーの role 変更

admin 権限の判定は handler に分散させず、将来的には policy 層に集約する。

想定配置。

```text
features/occurrences/policy.rs
features/users/policy.rs
```

---

## 監査ログ

### MVPでの扱い

主要CRUDと認証機能の実利用を優先し、PostgreSQLへの監査ログ保存は実装しない。

### 将来実装する内容

- `audit_logs` テーブルを追加する
- 外部副作用の前に `pending` を保存する
- 正常完了時に `success`、失敗時に `failed` へ更新する
- ログイン失敗と状態変更操作を記録する
- 監査ログ保存失敗時は対象操作を開始しない

詳細な記録項目とaction名は `10_audit_log.md` に従う。

---

## username変更

### MVPでの扱い

本登録時にusernameを設定できるが、登録後の変更APIは実装しない。

### 将来実装する内容

- ログインユーザーが自分のusernameを変更するAPIを追加する
- 空文字、空白のみ、長さ上限を検証する
- 他ユーザーのusernameを変更できないことを保証する
- OpenAPIとフロントエンドのアカウント設定画面を追加する

---

## 認証メール送信とDB更新の原子性

### MVPでの扱い

仮登録とパスワードリセットでは、tokenをDBへ保存した後にメールを送信する。メール送信失敗時にtokenのDB変更を自動で巻き戻す処理は実装しない。

### 将来実装する内容

- メール送信失敗時に仮登録tokenを無効化または削除する
- メール送信失敗時にpassword reset tokenを無効化または削除する
- 失敗時は `502 Bad Gateway` を返す
- DBとSMTPをまたぐ処理について、outbox方式または明示的な補償処理を採用する
- メール送信成功前に成功レスポンスを返さない

---

## 仮登録フローの完全化

### MVPでの扱い

email正規化、token hash保存、期限検証、本登録は実装済みとする。以下の再送・重複制御は後回しにする。

### 将来実装する内容

- 同じemailへ再送した場合、古い未完了tokenを無効化する
- 登録済みemailには新しいtokenを作成せず、メールも送信しない
- 仮登録tokenの有効期限を仕様と実装で統一する
- 再送、登録済みemail、メール失敗時のテストを追加する

---

## rolling session

### MVPでの扱い

sessionは発行時から7日で期限切れとする。アクセスごとの `expires_at` 延長は実装しない。

### 将来実装する内容

- 有効sessionの利用時に有効期限を延長する
- 延長頻度を制限し、全requestで不要なDB更新を行わない
- revokedまたは期限切れsessionは延長しない
- Cookieの `Max-Age` とDBの `expires_at` を整合させる

---

## アカウント論理削除

### MVPでの扱い

ユーザー退会とアカウント論理削除は実装しない。

### 将来実装する内容

- `users` に論理削除状態を追加する
- 論理削除済みユーザーのログインを拒否する
- 退会時に既存sessionをすべて失効する
- occurrence、media、監査ログの保持方針を定義する
- 外部キーを維持したまま個人情報を匿名化する方法を決める

---

## media孤立データと複合操作の補償

### MVPでの扱い

media uploadとoccurrence保存は別APIとして扱う。media upload成功後にoccurrence保存が失敗した場合、Garage objectとPostgreSQL metadataが孤立して残ることを許容する。

同一ユーザー・同一SHA-256の再uploadでは既存mediaを再利用するため、同じbytesの無制限な重複は抑制する。

### 将来実装する内容

- media付きoccurrence作成を一体のworkflowとして管理する
- occurrence保存失敗時にGarage objectとmetadataを補償削除する
- 孤立mediaを検出する定期処理を追加する
- 一定期間参照されないmediaだけを削除対象にする
- 補償処理失敗を再試行できる状態管理を追加する

---

## validation errorレスポンスの統一

### MVPでの扱い

RDF、media、認証入力のvalidationは実装するが、エラーごとのstatusとJSON形式は既存handlerの形式を使用する。すべてを `validation_failed` と `details[]` に統一する処理は実装しない。

### 将来実装する内容

- RDF/SHACL validationのstatusを `400` または `422` のどちらにするか統一する
- validation errorへ `details[]` を追加する
- Axum extractorが生成する不正UUIDやJSON rejectionも共通JSON形式へ変換する
- OpenAPIのerror schemaと実レスポンスを一致させる

---

## IRI目的語 / リテラル目的語に応じた述語変換

### MVPでの扱い

backendは保存前にN-Quadsをoxrdfでparseし、目的語がIRIかリテラルかをRDF termとして判定する。

利用する語彙メタデータ述語。

```text
https://bio-database.net/terms/objectKind
https://bio-database.net/terms/iriEquivalent
https://bio-database.net/terms/literalEquivalent
```

変換ルール。

- `objectKind` が `mixed` の場合は変換しない
- `objectKind` が見つからない場合は変換しない
- `objectKind` が `literal` で目的語がIRIの場合、`iriEquivalent` があれば述語を置換する
- `objectKind` が `IRI` で目的語がリテラルの場合、`literalEquivalent` があれば述語を置換する
- equivalentが見つからない場合は変換しない
- 変換後にOccurrence、Identification、Event、Locationへの振り分けを行う

### 後回し

- 語彙メタデータのバージョン管理
- 不正profileをより細かく診断するテスト
- 複数graphに同じpredicateメタデータがある場合の優先順位定義

---

## 複数コンポーネントとコンポーネント単位の日時管理

### MVPでの扱い

Occurrenceは、Identification、Event、Locationをそれぞれ最大1件だけ持つ。

各コンポーネントは、次の固定URIで保存する。

```text
https://bio-database.net/occurrences/{occurrence_id}/identifications/1
https://bio-database.net/occurrences/{occurrence_id}/events/1
https://bio-database.net/occurrences/{occurrence_id}/locations/1
```

`dcterms:created` と `dcterms:modified` はOccurrence本体だけにバックエンドが付与する。Identification、Event、Location、または各学名・各値には、バックエンド管理の作成日時・更新日時を付与しない。

このため、複数の `dwc:scientificName` が同じIdentificationに存在しても、RDFだけから登録順や「最新の学名」を判定できない。`dwc:dateIdentified` は利用者が任意で入力する同定日であり、バックエンドが付与する履歴順序ではない。

### 将来実装する内容

- Identification、Event、Locationをそれぞれ複数件持てるようにする
- 各コンポーネントに連番URIを割り当てる
  - 例: `/identifications/1`、`/identifications/2`
- 各コンポーネントへバックエンド管理の `dcterms:created` と `dcterms:modified` を付与する
- コンポーネントの更新時に、対象コンポーネントだけの `dcterms:modified` を更新する
- 検索一覧では、学名を表示するIdentificationをコンポーネントの管理日時で明示的に選択できるようにする
  - 最新の学名を表示する検索では、Identificationの `dcterms:created` または設計で定めた更新日時を基準にする
- 詳細画面では複数のIdentification、Event、Locationを時系列または利用者指定順で表示できるようにする
- コンポーネントの追加・更新・削除と、既存の単一ノードデータからの移行方針を仕様化する

### 設計上の注意

RDFトリプルそのものには順序がない。そのため、SPARQLの `SAMPLE()`、`MIN()`、`MAX()` だけで複数の学名から1件を選んでも、それを「最新」とみなしてはならない。最新表示には、比較可能でバックエンドが管理するコンポーネント単位の日時が必要である。

---

## ラベル印刷とラベルテンプレート

### 現在の進捗

検索画面でoccurrenceを複数選択し、ラベル作成モーダルを開ける。検索結果が同じoccurrenceを複数行返しても、フロントエンドではoccurrence IDごとに重複排除して一覧、選択、ラベル作成の対象を1件にする。

ラベル作成モーダルには、次の2種類のプレビューがある。

- A4全体: A4縦の用紙に5列 x 13行、最大65枚を配置する。ラベル間隔は0とする。65件を超える場合は追加のA4ページを作る。
- 個別: 選択順に1枚ずつ大きく表示し、前後矢印で確認する。個別表示は確認用の拡大表示であり、印刷寸法ではない。

デフォルトラベルは固定仕様とし、利用者は変更できない。

- ラベル寸法: 横40mm x 縦20mm
- QRコード: 15mm x 15mm
- 背景: 白
- 文字色: 黒
- 表示値: 学名、作成者、緯度経度、作成日、occurrence詳細URLのQRコード
- 学名がない場合: ラベル自体は作成し、学名の文字だけを表示しない
- 作成日: 時刻を含めず日付だけを表示する
- occurrence ID: 表示しない
- QRコード: 専用の固定領域へ置き、文字がQRコードの下や背後へ回り込まないようにする

緯度経度は、ラベル作成時に各occurrenceの詳細APIからN-Quadsを取得し、`dwc:decimalLatitude` と `dwc:decimalLongitude` を抽出する。両方がある場合だけ表示する。

A4全体プレビューでは以下を実装済みとする。

- 印刷: A4プレビューだけをPDF化し、別ウィンドウの印刷ダイアログを開く
- PDFダウンロード: `occurrence-labels.pdf` をダウンロードする
- PDF生成: `html2canvas` と `jspdf` を使い、日本語文字とQRコードを含むA4プレビューをPDF化する

検索画面の `ラベルテンプレート` は `/label-templates/new` へのテキストリンクである。

`/label-templates/new` には新規テンプレート作成画面を追加済みである。テンプレート名、横幅、縦幅、QR寸法、表示項目を入力し、プレビューを確認できる。表示項目は学名、作成者、緯度経度、作成日、QRコードである。

### 暫定実装と問題点

作成画面は現在、テンプレートをブラウザの `localStorage` の `occurrence-web.label-templates` キーへ保存する。これは画面試作のためだけの暫定実装である。

`localStorage` のデータは同一ブラウザ・同一端末にしか存在せず、ログインユーザーとの所有関係、他端末同期、認可、バックアップ、削除管理を実現できない。また、保存したテンプレートはラベル作成モーダルへまだ適用されない。

### 次に実装すること

#### 1. PostgreSQLへの永続化

テンプレートはRDFとして検索・共有する意味データではなく、ユーザー単位のUI・印刷設定である。そのためFusekiではなくPostgreSQLに保存する。

Goose migrationで `label_templates` テーブルを追加する。最低限の列は以下とする。

```text
id UUID PRIMARY KEY
user_id UUID NOT NULL REFERENCES users(id)
name TEXT NOT NULL
width_mm INTEGER NOT NULL
height_mm INTEGER NOT NULL
qr_size_mm INTEGER NOT NULL
fields JSONB NOT NULL
created_at TIMESTAMPTZ NOT NULL
updated_at TIMESTAMPTZ NOT NULL
```

デフォルトラベルはコード上の固定テンプレートとして扱い、`label_templates` テーブルには保存しない。

#### 2. API、service、認可

ログイン済みユーザーだけが自分のテンプレートを操作できるAPIを追加する。

- `POST /label-templates`: テンプレート作成
- `GET /label-templates`: ログインユーザー自身のテンプレート一覧取得
- `GET /label-templates/{id}`: 自分のテンプレート取得
- `PUT /label-templates/{id}`: 自分のテンプレート更新
- `DELETE /label-templates/{id}`: 自分のテンプレート削除

未ログインは `401`、存在しないテンプレートまたは他人のテンプレートは `404` とする。他人の存在を通知しない。

入力値は正の数値であること、QR寸法がラベル寸法内に収まること、`fields` が許可済みの表示項目だけで構成されることを検証する。

#### 3. フロントエンドへの適用

- `/label-templates/new` の `localStorage` 保存をAPI呼び出しへ置き換える
- テンプレート一覧ページを追加し、作成、編集、削除へ遷移できるようにする
- ラベル作成モーダルでデフォルトまたは自分のテンプレートを選択できるようにする
- A4全体、個別、PDF、印刷で同じテンプレート定義を使う
- PostgreSQL移行後は `localStorage` の `occurrence-web.label-templates` を読まず、暫定コードを削除する

将来、項目の配置・文字サイズ・罫線などを編集可能にする場合も、任意HTMLや任意CSSは保存・実行しない。許可した表示項目とレイアウト属性だけを構造化データとして保存する。

#### 4. テストと仕様更新

実装開始時は先に `backend/test.md` へテスト内容を追記する。

- service: 正常なテンプレートの作成、取得、更新、削除
- service: 寸法、QR寸法、表示項目のvalidation
- handler: 未ログイン `401`、他ユーザーの取得・更新・削除が `404`
- PostgreSQL統合: 実DBへ保存・取得できること
- フロントエンド: 作成済みテンプレートを選択するとA4、個別、PDF出力へ同じ設定が反映されること

API実装時はOpenAPIと `spec/11_api_contract.md` を更新する。

---

## 管理方針

このファイルに記載した項目を実装する場合は、先に該当テストを追加する。

実装後は、該当項目をこのファイルから削除するか、実装済みであることが分かる形に更新する。
