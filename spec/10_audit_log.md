# 10. 監査ログ要件

## 基本方針

- 監査ログは PostgreSQL に保存する
- 監査ログは無期限に保持する
- 閲覧操作は MVP では記録しない
- ログイン失敗は記録する
- 監査ログ保存に失敗した場合、操作全体を失敗扱いにする

---

## 保存項目

| カラム | 内容 |
|---|---|
| id | 監査ログID |
| actor_user_id | 実行ユーザーID。未ログインの場合はNULL可 |
| action | 操作名 |
| target_type | 対象種別 |
| target_id | 対象ID |
| result | pending / success / failed |
| occurred_at | 発生時刻 UTC |
| ip_address | IPアドレス |
| user_agent | User-Agent |
| detail_json | 詳細情報 JSON |

---

## result

以下の3状態を使う。

- `pending`
- `success`
- `failed`

---

## 処理順序

Jena保存やGarage保存など、外部副作用の前に監査ログを `pending` として作成する。

正常完了後。

- `success` に更新する

失敗時。

- `failed` に更新する

監査ログ作成自体に失敗した場合。

- 対象操作を開始しない
- 操作全体を失敗扱いにする

---

## Jena保存を伴う操作

Jena保存前に `pending` を作成する。  
Jena保存後に監査ログ作成が失敗して補償削除する方式は採用しない。

---

## 記録対象

MVPで記録する。

- 仮登録申請
- 本登録完了
- ログイン成功
- ログイン失敗
- ログアウト
- パスワードリセット申請
- パスワードリセット完了
- オカレンス作成
- オカレンス更新
- オカレンス削除
- 公開範囲変更
- 管理者によるロール変更

MVPで記録しない。

- オカレンス閲覧
- メディア閲覧
- 一覧表示
- 検索

---

## action名候補

```text
auth.pre_register.requested
auth.registration.completed
auth.login.succeeded
auth.login.failed
auth.logout
auth.password_reset.requested
auth.password_reset.completed
occurrence.created
occurrence.updated
occurrence.deleted
occurrence.access_rights_changed
user.role_changed
```

---

## エラー時の扱い

- 監査ログ保存失敗は操作全体失敗
- 監査ログ更新失敗も操作全体失敗
- 可能なら `failed` への更新を試みる
- ただし `failed` 更新自体が失敗した場合は、サーバーログに記録する
