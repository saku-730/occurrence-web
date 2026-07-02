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

## 管理方針

このファイルに記載した項目を実装する場合は、先に該当テストを追加する。

実装後は、該当項目をこのファイルから削除するか、実装済みであることが分かる形に更新する。
