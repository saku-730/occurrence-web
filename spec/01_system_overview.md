# 01. システム概要

## 目的

本システムは、生物オカレンス情報を中心に管理するWebアプリケーションである。  
研究者・調査者が、観察・採集・同定・メディアなどの情報を柔軟に登録・閲覧・検索できるようにする。

---

## 採用技術

| 領域 | 技術 |
|---|---|
| フロントエンド | Next.js |
| バックエンド | Rust + axum |
| アプリ管理DB | PostgreSQL |
| RDFストア | Apache Jena Fuseki |
| メディア保存 | Garage |
| RDF検証 | SHACL |
| メール送信 | 外部SMTP/Resend等 |

---

## データ管理方針

| データ | 保存先 |
|---|---|
| ユーザー情報 | PostgreSQL |
| 認証情報 | PostgreSQL |
| セッション | PostgreSQL |
| 仮登録トークン | PostgreSQL |
| パスワードリセットトークン | PostgreSQL |
| 監査ログ | PostgreSQL |
| メディアメタデータ | PostgreSQL |
| オカレンス本体 | Apache Jena |
| オカレンスRDFメタデータ | Apache Jena |
| 公開範囲RDF | Apache Jena |
| メディア本体 | Garage |

---

## MVP対象

MVPで扱うもの。

- ユーザー仮登録
- メール確認
- 本登録
- ログイン
- ログアウト
- パスワードリセット
- セッション管理
- CSRF対策
- ロール管理
- オカレンス作成
- オカレンス閲覧
- オカレンス更新
- オカレンス削除
- RDF/N-Quads保存
- SHACL/保存前検証
- メディアアップロード
- メディア閲覧
- タクソン名検索
- 監査ログ
- OpenAPI

---

## MVP対象外

MVPでは扱わない、または後回しにするもの。

- Darwin Core Archive エクスポート
- CSV / JSON / Turtle / N-Quads エクスポート実装
- 非ログインユーザーのエクスポート
- 管理者用ユーザー管理画面
- 監査ログ閲覧画面
- 高度なGIS機能
- 自動同定
- 外部サービスとの自動同期
- モバイルアプリ
- サムネイル生成
- 動画プレビュー生成
- 孤立メディアの自動削除
- メールアドレス変更
- ログイン失敗回数ロック
- タクソン、地点、人物、標本などの独立概念管理

---

## 公開範囲

オカレンスの公開範囲はMVPでは以下の2種類とする。

- public
- private

値URIは以下を使う。

```text
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/public
https://{APP_PUBLIC_BASE_URL}/terms/access-rights/private
```

---

## 時刻

- すべて UTC で保存する
- PostgreSQL は `TIMESTAMPTZ` を使用する
- RDF は `xsd:dateTime` を使用する
- APIレスポンスも UTC を返す
