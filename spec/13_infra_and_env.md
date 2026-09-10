# 13. インフラ・環境要件

## 基本構成

- backend: Rust + axum
- frontend: Next.js
- PostgreSQL
- Apache Jena Fuseki
- Garage
- GROBID
- ABR geocoder
  - デジタル庁公式 `digital-go-jp/abr-geocoder` の Docker Compose 構成で稼働
  - ABR 専用 PostgreSQL / abrdb / abrg を公式 compose で管理
- Nominatim
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
OBJECT_STORE_BACKEND=s3
S3_ENDPOINT=http://127.0.0.1:3900
S3_REGION=garage
S3_BUCKET=occurrence-media
S3_ACCESS_KEY=...
S3_SECRET_KEY=...
S3_FORCE_PATH_STYLE=true
```

### GROBID

論文PDFの書誌情報抽出にGROBIDの `processHeaderDocument` を利用する。
未指定時は `http://127.0.0.1:8070` を使用する。

```env
GROBID_BASE_URL=http://127.0.0.1:8070
```

### Geocoding / ABR / Nominatim

住所ジオコーディングでは役割を次のように分離する。

- ABR: 日本語住所の正規化・階層分割
- Nominatim: 最終的な緯度経度の取得

ABR が返す緯度経度は使用しない。
Nominatim 成功時だけ `dwc:decimalLatitude` / `dwc:decimalLongitude` と `dwciri:georeferenceSources <https://nominatim.openstreetmap.org/>` を Location RDF に追加する。

backend 側の設定例。

```env
ABR_BASE_URL=http://127.0.0.1:3001
NOMINATIM_BASE_URL=https://nominatim.openstreetmap.org
NOMINATIM_USER_AGENT=bio-database/1.0
```

ABR 自体は npm で直接起動せず、デジタル庁公式 `digital-go-jp/abr-geocoder` リポジトリの Docker Compose 構成を使用する。
公式 compose 内の ABR 専用 PostgreSQL は Bio-Database 本体の PostgreSQL とは別サービスとして扱う。
ホスト側ポートが衝突する場合は公式 `.env.example` に従って `DB_PORT` / `PORT` を変更する。

- Nominatim 公開APIへのアクセスは直列化する
- 同一の ABR 正規化住所は backend でキャッシュする
- 公開 Nominatim 利用時は最大 1 request / second を超えない
- Nominatim へはアプリケーションを識別できる `User-Agent` を送信する

詳細は `spec/18_geocoding.md`、ABR の公式 Docker Compose 導入手順は `spec/16_server_setup.md` を参照する。

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
- papers
  - PDF本体は保存せずGarageのbucket/object keyを保持する
  - SHA-256によるPDF重複判定
  - GROBIDで抽出した論文書誌情報
- audit_logs
- app settings

ABR の公式 Docker Compose が使用する PostgreSQL は ABR データ専用であり、この Bio-Database アプリケーション用 PostgreSQL には含めない。

---

## Jena/Fuseki

保存するもの。

- occurrence RDF
- occurrence metadata RDF
- accessRights RDF
- license RDF
- media URI reference
- Nominatim 由来の geocoding RDF
  - `dwc:decimalLatitude`
  - `dwc:decimalLongitude`
  - `dwciri:georeferenceSources <https://nominatim.openstreetmap.org/>`
  - ABR は住所前処理のみなので georeference source として保存しない
- GBIF Backbone Taxonomy graph
  - graph URI: `https://bio-database.net/graphs/taxonomy/gbif-backbone`
  - taxon URI: `https://bio-database.net/taxa/gbif/{id}`
  - `{id}` はGBIF taxon key
  - 取り込み元バージョンと取得日時を記録する
- Darwin Core vocabulary graph
  - graph URI: `https://bio-database.net/graphs/vocabularies/darwin-core`
  - Darwin Core Termsとdwciriの語彙情報を格納する
  - マスターデータ投入処理だけが更新する
- occurrence profile graph
  - graph URI: `https://bio-database.net/graphs/app/occurrence-profile`
  - dwcからdwciriへの変換用メタ情報を格納する
  - backendがread-onlyで参照する
- master ontology graph

---

## Garage

保存するもの。

- 画像本体
- 音声本体
- 動画本体
- importした論文PDF本体

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


---

## セットアップ

### 各種インストール

- RUST

- PostgreSQL

- Apache jena

- Garage

- Next.js

- ABR geocoder
  - 公式 Docker Compose 構成を使用する

- Nominatim は公開APIを利用するためローカルインストール不要

### データベース マスターデータセットアップ
