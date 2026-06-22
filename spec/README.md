# オカレンス管理Webアプリ 実装用要件書

このディレクトリは、オカレンス管理Webアプリを LLM による実装補助・TDD 開発で進めるための、分割済み実装用要件書である。

## 分割方針

- 仕様を機能単位に分割する
- 各ファイルは、実装時にそのまま LLM に渡せる粒度を目指す
- TDD を前提とし、テスト要件を明記する
- RDF / PostgreSQL / Garage(S3互換) / メール / OpenAPI などの責務境界を明確にする
- 仕様にない機能を勝手に追加しない

## ファイル一覧

| ファイル | 内容 |
|---|---|
| `00_development_rules.md` | 開発ルール、TDD、コメント方針 |
| `01_system_overview.md` | システム概要、MVP範囲、対象外 |
| `02_architecture.md` | feature-first モジュラモノリス構成 |
| `03_auth.md` | 認証、セッション、CSRF、登録、パスワードリセット |
| `04_authorization_policy.md` | ロール、認可、404/403方針 |
| `05_occurrence_rdf.md` | RDF/N-Quads/URI/named graph/CRUD仕様 |
| `06_shacl_validation.md` | SHACLおよび保存前検証 |
| `07_media.md` | Garage、PostgreSQL media_objects、メディア配信 |
| `08_search.md` | `dwc:scientificName` 検索、taxonomy階層探索 |
| `09_export.md` | エクスポート方針、MVP対象外 |
| `10_audit_log.md` | 監査ログ、pending/success/failed |
| `11_api_contract.md` | API共通仕様、JSONレスポンス、エラー形式 |
| `12_frontend_screens.md` | MVP画面要件、フォーム入力方針 |
| `13_infra_and_env.md` | 環境変数、外部サービス、UTC、バックアップ |
| `14_testing_strategy.md` | テスト戦略、TDD単位、必須テスト観点 |
| `15_feature.md` | MVPでは省略し、将来実装する機能 |

## 最重要方針

- フロントエンドからバックエンドへ送る RDF は **N-Quads のみ**
- Turtle は入力形式として使わない
- バックエンドは occurrence URI を発行し、blank node 主語を置換する
- オカレンス本体は Apache Jena に RDF として保存する
- 認証・ユーザー・セッション・監査ログ・メディアメタデータは PostgreSQL に保存する
- メディア本体は Garage に保存する
- すべての機能追加・仕様変更はテスト駆動で行う
