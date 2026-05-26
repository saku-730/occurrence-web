# 13. インフラ・環境要件

## 基本構成

- backend: Rust + axum
- frontend: Next.js
- PostgreSQL
- Apache Jena Fuseki
- MinIO
- 外部メール送信サービス
- 開発用 Mailpit

---

## 環境変数候補

### App

```env
APP_HOST=127.0.0.1
APP_PORT=3000
APP_PUBLIC_BASE_URL=https://example.org
APP_ENV=development
```

### PostgreSQL

```env
DATABASE_URL=postgres://...
```

### Jena/Fuseki

```env
FUSEKI_BASE_URL=http://127.0.0.1:3030
FUSEKI_DATASET=occurrence
```

### MinIO

```env
MINIO_ENDPOINT=http://127.0.0.1:9000
MINIO_ACCESS_KEY=...
MINIO_SECRET_KEY=...
MINIO_BUCKET=occurrence-media
MINIO_REGION=us-east-1
```

### Mail

```env
SMTP_HOST=...
SMTP_PORT=...
SMTP_USERNAME=...
SMTP_PASSWORD=...
MAIL_FROM=...
```

### Cookie

```env
COOKIE_SECURE=false
```

本番環境では `COOKIE_SECURE=true` 必須。

---

## UTC方針

- PostgreSQL: `TIMESTAMPTZ`
- RDF: `xsd:dateTime`
- APIレスポンス: UTC
- ログ: UTC

---

## PostgreSQL

保存するもの。

- users
- roles
- sessions
- pending_registrations
- password_reset_tokens
- media_objects
- audit_logs
- app settings

---

## Jena/Fuseki

保存するもの。

- occurrence RDF
- occurrence metadata RDF
- accessRights RDF
- license RDF
- media URI reference
- taxonomy ontology graph
- master ontology graph

---

## MinIO

保存するもの。

- 画像本体
- 音声本体
- 動画本体

bucket は private 固定。

---

## メール送信

- 仮登録確認メール
- パスワードリセットメール

メール送信失敗時。

- HTTP 502
- DB変更はロールバック
- 操作全体失敗

---

## バックアップ方針

MVP段階では詳細な自動化は必須ではないが、以下をバックアップ可能な構成にする。

- PostgreSQL
- Jena/Fusekiデータ
- MinIO bucket

将来タスク。

- バックアップ周期
- リストア手順
- バックアップ検証
- オフサイトバックアップ
