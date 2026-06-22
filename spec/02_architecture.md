# 02. アーキテクチャ要件

## 採用アーキテクチャ

バックエンドは feature-first なモジュラモノリスとする。  
単一のRustアプリケーションとして動作させるが、内部は機能単位で分割する。

---

## 採用理由

- TDDしやすい
- 小さく実装を進めやすい
- LLMに機能単位で実装依頼しやすい
- 過剰なマイクロサービス化を避けられる
- 将来の分割可能性を残せる

---

## レイヤ方針

厳密なクリーンアーキテクチャではないが、以下の責務分離を守る。

```text
handler -> service -> policy / model / port -> infrastructure
```

---

## Presentation層

責務。

- HTTPリクエストを受ける
- DTOへ変換する
- 認証済みユーザーを抽出する
- serviceを呼び出す
- HTTPレスポンスへ変換する

禁止事項。

- SQLを書かない
- SPARQLを書かない
- Garage/S3互換ストレージの詳細処理を書かない
- メール送信の詳細を書かない
- 複雑な業務ロジックを書かない

---

## Application / Service層

責務。

- ユースケースを実行する
- 処理順序を制御する
- 認可判定を呼び出す
- repository / infrastructure port を呼び出す
- 監査ログを記録する
- 複数ストア間の補償処理を制御する

---

## Domain層

責務。

- ロール
- 公開範囲
- ユーザー状態
- オカレンスURI
- メディアURI
- 値オブジェクト
- 認可ルールの表現

---

## Infrastructure層

責務。

- PostgreSQL接続
- Jena/Fuseki接続
- Garage/S3互換ストレージ接続
- メール送信
- 外部サービスI/O
- SQL / SPARQL / S3 API の具体実装

---

## feature-first 構成例

```text
src/
  main.rs
  lib.rs
  app.rs
  state.rs
  config.rs

  features/
    auth/
      dto.rs
      handler.rs
      service.rs
      repository.rs
      model.rs
      policy.rs

    users/
      dto.rs
      handler.rs
      service.rs
      repository.rs
      model.rs
      policy.rs

    occurrences/
      dto.rs
      handler.rs
      service.rs
      model.rs
      policy.rs
      rdf.rs
      port.rs

    media/
      dto.rs
      handler.rs
      service.rs
      model.rs
      repository.rs
      policy.rs

    search/
      dto.rs
      handler.rs
      service.rs
      sparql.rs

    audit/
      model.rs
      repository.rs
      service.rs

  infrastructure/
    postgres/
    fuseki/
    minio/
    mail/
```

---

## 外部依存の扱い

外部依存は trait / port で抽象化し、serviceの単体テストでは fake 実装を使えるようにする。

例。

- `OccurrenceRdfStore`
- `MediaObjectStore`
- `Mailer`
- `AuditLogRepository`
- `SessionRepository`

---

## 過剰抽象化を避ける

以下は初期段階では避ける。

- 不必要な generic
- 実装が1つしかないのに過剰な trait 分割
- 複雑なドメインイベント
- マイクロサービス前提の分散処理
- CQRS / Event Sourcing

必要になった段階で追加する。
