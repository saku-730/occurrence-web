# 07. メディア要件

## 基本方針

- メディア本体は Garage に保存する
- backend から Garage へは S3互換 API で接続する
- メディアメタデータは PostgreSQL に保存する
- RDFからメディアを参照する場合は media URI を使う
- Garage object key に元ファイル名を使わない
- Garage bucket は private 固定
- フロントエンドから Garage に直接アクセスさせない

---

## メディアURI

```text
https://{APP_PUBLIC_BASE_URL}/media/{media_uuid}
```

`{media_uuid}` は PostgreSQL `media_objects.id` と同じ。

---

## PostgreSQLテーブル案

```sql
CREATE TABLE media_objects (
    id UUID PRIMARY KEY,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL UNIQUE,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    original_filename TEXT,
    uploaded_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

---

## object key

- backend が UUID を使って生成する
- 元ファイル名をそのまま使わない
- `media_objects.object_key` は一意にする
- `original_filename` はメタデータとして PostgreSQL に保存してよい

例。

```text
media/{media_uuid}
```

---

## 対応拡張子

### 画像

- jpg
- jpeg
- png
- webp

### 音声

- mp3
- wav
- m4a

### 動画

- mp4
- mov

---

## MIME type 検証

- MIME type 検証は必須
- 拡張子だけで判断しない
- 許可MIME type以外は 400 または 415
- サイズ上限超過は 413

---

## サイズ上限

| 種別 | 上限 |
|---|---:|
| 画像 | 500MB |
| 音声 | 500MB |
| 動画 | 1000MB |

---

## Garage bucket

- bucket は1つ
- private 固定
- bucket名は環境変数で設定可能にする
- 例: `occurrence-media`

---

## 配信方針

- backend が認可判定を行う
- public occurrence に紐づくメディアは非ログインでも閲覧可能
- private occurrence にのみ紐づくメディアは権限があるユーザーのみ閲覧可能
- 同じメディアが複数 occurrence に紐づく場合、1つでも public occurrence に紐づいていれば閲覧可能

MVPでは backend 経由配信を基本とする。  
presigned URL は将来検討。

---

## アップロード方針

ユーザー体験としては、オカレンス登録とメディアアップロードを同時に行えるようにする。

内部実装では分割してよい。

例。

1. frontend がメディアをアップロード
2. backend が Garage に保存
3. backend が `media_objects` に保存
4. media URI を返す
5. frontend が occurrence RDF に media URI を含める
6. occurrence 作成APIを呼ぶ

ただし、ユーザーからは一連の登録操作に見えるようにする。

---

## API パス

### `POST /media`

メディアファイルをアップロードする。

- login 必須
- request は `multipart/form-data`
- backend が MIME type、拡張子、サイズ上限を検証する
- backend が Garage に object を保存する
- backend が PostgreSQL `media_objects` にメタデータを保存する
- response は `media_id` と `media_uri` を返す
- occurrence との紐付けは、この response の `media_uri` を occurrence RDF に含めることで表現する

### `GET /media/{media_id}`

メディアファイルを backend 経由で取得する。

- frontend は Garage に直接アクセスしない
- backend が `media_objects` を参照して Garage object を取得する
- backend が occurrence RDF との紐付けを確認して認可判定を行う
- public occurrence に紐づく media は非ログインでも取得可能
- private occurrence にのみ紐づく media は、作成者または admin のみ取得可能

### `DELETE /media/{media_id}`

メディアファイルを明示的に削除する。

- login 必須
- 原則として upload したユーザーまたは admin のみ削除可能
- Garage object を削除する
- PostgreSQL `media_objects` レコードを削除する
- occurrence RDF に残っている media URI は自動削除しない
- occurrence RDF からの参照削除は occurrence 更新APIで行う

---

## メディア付きオカレンス作成時の一貫性

以下の順序で失敗した場合は補償処理を行う。

1. Garage 保存成功
2. PostgreSQL `media_objects` 保存成功
3. Jena 保存失敗

この場合。

- Garage object を削除する
- `media_objects` レコードを削除する
- 操作全体を失敗扱いにする

---

## 削除方針

MVPでは自動削除しない。

- occurrence 削除時に Garage object を自動削除しない
- occurrence 削除時に `media_objects` を自動削除しない
- 孤立メディアの自動削除は MVP 対象外

将来タスク。

- 孤立メディア検出
- 一定期間後の自動削除
- 管理者によるメディア掃除画面

---

## サムネイル・プレビュー

MVPでは実装しない。

- サムネイル生成なし
- 動画プレビュー生成なし
- 音声波形生成なし
