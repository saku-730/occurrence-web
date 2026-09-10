---
title: "Deployment"
description: "Bio Databaseの本番ビルド、nginx、Cloudflare Tunnel、プロセス起動に関する公開可能な運用概要"
weight: 30
toc: true
draft: false
---

このページでは、公開可能な範囲のデプロイ構成だけを説明します。秘密鍵、パスワード、実際の内部IPアドレス、認証トークン、接続文字列は掲載しません。

## 本番ビルド

本番デプロイ前に、各コンポーネントの成果物を生成します。

- Rustのrelease build
- Next.jsのproduction build
- Hugoによる静的サイト生成

## プロセス起動

本番では、APIサーバー、Next.js、Garage、Cloudflare Tunnelなどの必要なプロセスを起動します。起動順序、停止方法、ログ出力先を運用手順として管理します。

## nginx

nginxは外部HTTPリクエストを受け、Next.js、Rust API、Hugoの静的ファイルへ振り分けるリバースプロキシです。

## Cloudflare Tunnel

Cloudflare Tunnelを使い、サーバーの公開ポートを直接インターネットへ開けずに外部公開します。

## 公開構成例

```text
bio-database.net
  → nginx
  → Next.js

docs.bio-database.net
  → nginx
  → Hugo public/
```

APIはnginxからRust APIサーバーへ転送します。GarageとFusekiはアプリケーション内部から利用し、直接公開しません。

## ログ確認

障害時には、nginx、Cloudflare Tunnel、Rust API、Next.js、Garage、Fusekiのログを確認します。ログに秘密情報や認証トークンを出力しないことを運用上の前提とします。
