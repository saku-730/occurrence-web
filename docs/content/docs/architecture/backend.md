---
title: "Backend"
description: "Rust APIサーバーの責務、入力検証、RDF生成、ストレージ連携、エラーハンドリング"
weight: 10
toc: true
draft: false
---

## Rustを採用した理由

Rustは、型による入力・状態の表現、非同期I/O、メモリ安全性を備えています。RDFストア、PostgreSQL、Garageなど複数の外部サービスを扱うAPIサーバーで、エラー処理を明示的に保つために採用しています。

## APIの責務

APIサーバーは、認証、認可、HTTPリクエストの処理、入力検証、RDFへの変換、外部ストレージとの通信を担当します。フロントエンドや外部クライアントからFuseki・PostgreSQL・Garageへ直接接続させません。

## 入力検証

オカレンス登録では、N-Quadsの構文、Named Graph、blank node subject、バックエンド管理述語、アクセス権URIを検証します。メディアでは、MIME type、拡張子、magic bytes、ファイルサイズを検証します。

## RDF生成

フロントエンドが送るフラットな項目名と値を受け取り、述語に応じてOccurrence、Identification、Event、Locationへ振り分けます。作成者、作成日時、更新日時、アクセス権はバックエンドが付加・管理します。

## Fusekiとの通信

FusekiにはSPARQL QueryとSPARQL Updateで接続します。オカレンスの保存、詳細取得、検索、更新、削除を行い、分類群の階層関係も検索に利用します。

## PostgreSQLとの役割分担

PostgreSQLは、ユーザー、認証情報、セッション、パスワードリセットトークン、メディアメタデータを管理します。RDFの意味関係を持つオカレンスデータはFusekiに保存します。

## Garageとの通信

メディア本体はS3互換APIを介してGarageへ保存します。アップロード時は一時ファイルからストリーム転送し、PostgreSQLへのメタデータ保存が失敗した場合はGarageの保存を補償削除します。

## エラーハンドリング

入力不正は400、未認証は401、存在しないリソースは404、サイズ超過は413として返します。FusekiやGarageなど外部ストレージの失敗は、クライアントに内部情報を出さずに502として扱います。

## Traitによるストレージ抽象化

オカレンスのRDFストアはTraitで抽象化しています。

```text
OccurrenceRdfStore
├── FusekiClient
└── FakeOccurrenceRdfStore
```

`FusekiClient` は本番のFuseki通信を実装し、`FakeOccurrenceRdfStore` はテストで決定的な応答を返します。サービス層をTraitに依存させることで、実Fusekiを必要としない単体テストと、実Fusekiを使う統合テストを分けています。
