# 04. 認可・ロール要件

## ロール

MVPのロールは以下の2種類。

- admin
- editor

新規登録ユーザーの初期ロールは editor とする。

---

## 初期管理者

- 初期 admin は SQL で手動作成する
- 初回登録ユーザーを自動 admin にする処理は作らない
- 管理CLIは MVP では作らない

---

## 管理者操作

admin は以下を実行できる。

- 全オカレンスの閲覧
- 全オカレンスの更新
- 全オカレンスの削除
- 他ユーザーのロール変更

admin は自分自身を editor に降格できない。  
ただしMVPでは初期adminが1人であることを前提とし、この制約を過度に複雑化しない。

---

## editor 操作

editor は以下を実行できる。

- オカレンス作成
- 自分のオカレンス閲覧
- 自分のオカレンス更新
- 自分のオカレンス削除
- 自分のオカレンス公開範囲変更
- 他人の public オカレンス閲覧
- 自分の username 変更

editor は以下を実行できない。

- 他人のオカレンス更新
- 他人のオカレンス削除
- 他人の private オカレンス閲覧
- 他ユーザーのロール変更
- オカレンス作成者の変更

---

## 非ログインユーザー

非ログインユーザーは以下のみ可能。

- public オカレンス閲覧
- public メディア閲覧

非ログインユーザーは以下を実行できない。

- オカレンス作成
- オカレンス更新
- オカレンス削除
- private オカレンス閲覧
- メディアアップロード
- エクスポート

---

## 認可マトリクス

| 操作 | 非ログイン | editor 作成者 | editor 非作成者 | admin |
|---|---:|---:|---:|---:|
| public occurrence 閲覧 | 可 | 可 | 可 | 可 |
| private occurrence 閲覧 | 不可 | 可 | 不可 | 可 |
| occurrence 作成 | 不可 | 可 | 可 | 可 |
| 自分の occurrence 更新 | 不可 | 可 | - | 可 |
| 他人の occurrence 更新 | 不可 | 不可 | 不可 | 可 |
| 自分の occurrence 削除 | 不可 | 可 | - | 可 |
| 他人の occurrence 削除 | 不可 | 不可 | 不可 | 可 |
| 作成者変更 | 不可 | 不可 | 不可 | 不可 |
| ロール変更 | 不可 | 不可 | 不可 | 可 |

---

## private occurrence のレスポンス方針

権限のないユーザーが private occurrence にアクセスした場合、存在自体を隠す。

- 403 ではなく 404 を返す
- 一覧・検索結果にも出さない

---

## policy配置

認可ロジックは feature ごとの `policy.rs` に集約する。  
handler や repository に認可条件を分散させない。

例。

```text
features/occurrences/policy.rs
features/media/policy.rs
features/users/policy.rs
```
