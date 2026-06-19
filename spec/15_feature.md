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

## 管理方針

このファイルに記載した項目を実装する場合は、先に該当テストを追加する。

実装後は、該当項目をこのファイルから削除するか、実装済みであることが分かる形に更新する。
