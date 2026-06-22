# 13. インフラ・環境要件

## 基本構成

- backend: Rust + axum
- frontend: Next.js
- PostgreSQL
- Apache Jena Fuseki
- Garage
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

### Garage（S3互換 object storage）

backend は Garage に S3互換 API で接続する。

```env
GARAGE_ENDPOINT=http://127.0.0.1:9000
GARAGE_ACCESS_KEY_ID=...
GARAGE_SECRET_ACCESS_KEY=...
GARAGE_BUCKET=occurrence-media
GARAGE_REGION=us-east-1
```

### Mail

```env
SMTP_HOST=...
SMTP_PORT=...
SMTP_USERNAME=...
SMTP_PASSWORD=...
MAIL_FROM=...
```

### 実行環境 / Cookie

```env
APP_ENV=development
COOKIE_SECURE=false
```

`APP_ENV=production` の場合は `COOKIE_SECURE=true` 必須。
この組み合わせを満たさない場合、バックエンドは起動時設定読み込みに失敗する。

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

## Garage

保存するもの。

- 画像本体
- 音声本体
- 動画本体

bucket は private 固定。

### インストール方針

Garage は Docker image ではなく、公式配布バイナリを直接ダウンロードして利用する。

- 開発環境では Garage の release binary を取得してローカルに配置する
- Garage binary は `/opt/garage/versions/{version}/garage` にバージョンごとに配置する
- 実行パスは `/usr/local/bin/garage` から対象バージョンの binary へ symlink する
- 例: `/opt/garage/versions/v2.3.0/garage`
- 例: `/usr/local/bin/garage -> /opt/garage/versions/v2.3.0/garage`
- backend は Garage の S3互換 endpoint に接続するだけで、Garage の起動方式には依存しない
- Garage binary の配置先、設定ファイル、起動コマンドは infra 手順として管理する
- compose には Garage service を追加しない

### 開発時の起動方法

開発時はリポジトリ直下で次のコマンドを実行して Garage server を起動する。

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage server
```

- `garage/garage.toml` を開発用 Garage 設定ファイルとする
- `garage` は `/usr/local/bin/garage` の symlink 経由で実行される想定

### メモ

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage layout assign -z home -c 10G 165e
```

とりあえず10Gストレージを割り当てる。

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage layout apply --version 1
```

設定反映

```bash
GARAGE_CONFIG_FILE=./garage/garage.toml garage bucket create occurrence-media
GARAGE_CONFIG_FILE=./garage/garage.toml garage key create occurrence-web
```

バケットとアクセスキーの作成。

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
- Garage bucket

将来タスク。

- バックアップ周期
- リストア手順
- バックアップ検証
- オフサイトバックアップ
