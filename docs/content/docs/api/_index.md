---
title: "API"
description: "Bio Database APIの認証、オカレンス、検索、分類候補、メディア、エラーレスポンス"
weight: 50
toc: true
draft: false
---

このページは実装済みAPIの概要です。OpenAPI仕様はバックエンドからも提供しますが、ここでは主要なリソースと認証条件を説明します。

## 認証

- `POST /auth/pre_register`: 仮登録
- `POST /auth/complete_registration`: 本登録
- `POST /auth/login`: ログイン
- `POST /auth/logout`: ログアウト
- `GET /auth/me`: 現在のログインユーザー
- `POST /auth/request_password_reset`: パスワードリセット依頼
- `POST /auth/reset_password`: パスワード更新

ログインが必要な操作では、セッションCookieを使用します。

## Occurrence作成

`POST /occurrences` でN-Quadsを送信してオカレンスを作成します。ログインが必要です。作成者、作成日時、更新日時、アクセス権はバックエンドが管理します。

## Occurrence取得

`GET /occurrences/{occurrence_id}` で詳細を取得します。公開データは誰でも取得でき、非公開データは作成者だけが取得できます。

## Occurrence検索

`POST /occurrences/search` で条件検索を行います。未ログインの利用者には公開データだけを返します。

```json
{
  "items": [],
  "page": {
    "limit": 50,
    "next_cursor": null,
    "has_next": false
  }
}
```

検索APIはページネーションにcursorを使います。次ページがある場合、`next_cursor` に不透明なカーソル文字列が返ります。

## Occurrence更新

`PUT /occurrences/{occurrence_id}` でオカレンスを更新します。ログイン済みで、かつ作成者である必要があります。

## Occurrence削除

`DELETE /occurrences/{occurrence_id}` でオカレンスを削除します。ログイン済みで、かつ作成者である必要があります。

## 分類群候補

`GET /vocabularies/darwin-core` は、入力補助用のDarwin Core語彙候補を返します。分類群名の候補表示には、GBIF Species APIも利用します。

## メディアアップロード

`POST /media` は認証済みユーザーのファイルアップロードです。`GET /media/{media_id}` はメディアを取得し、公開オカレンスに紐付くメディアは未ログインでも取得できます。`DELETE /media/{media_id}` はアップロード者だけが実行できます。

## エラーレスポンス

- `400 Bad Request`: 入力形式、URI、N-Quads、検索条件などが不正
- `401 Unauthorized`: ログインが必要
- `404 Not Found`: 対象が存在しない、または閲覧権限がない
- `413 Payload Too Large`: メディアがサイズ上限を超過
- `502 Bad Gateway`: FusekiやGarageなど外部ストレージとの通信失敗
