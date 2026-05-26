# 09. エクスポート要件

## MVP方針

エクスポート機能は MVP では実装しない。  
将来対応とする。

---

## 将来候補形式

- CSV
- JSON
- RDF/Turtle
- Darwin Core
- RDF/N-Quads

注意。

- アプリ内部および frontend-to-backend の RDF 入力では Turtle を使わない
- Turtle は将来的なエクスポート候補に留める

---

## 権限方針

将来実装時は、エクスポート対象を閲覧可能データに限定する。

### 非ログイン

- エクスポート不可

### editor

- 自分が閲覧可能な occurrence のみ

### admin

- 全 occurrence

---

## Darwin Core エクスポート

- MVPでは未実装
- 内部データモデルを Darwin Core 固定にはしない
- 将来、RDF predicate と Darwin Core field の対応表を定義する
- Darwin Core Archive 対応は将来検討

---

## 将来タスク

- エクスポート形式選択UI
- 非同期エクスポートジョブ
- 大量データ出力
- ダウンロード期限
- 監査ログ記録
