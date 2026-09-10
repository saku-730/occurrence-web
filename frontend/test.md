# フロントエンドテスト

## 標本ラベル

実行 (Node.js 24): `node --test --test-isolation=none tests/label-values.test.mjs`

- [x] EventのeventDate、Locationのlocality、任意述語を詳細N-Quadsから抽出する。
- [x] 欠落・空値・blank node目的語を表示せず、複数値を保持して重複を除く。
- [x] 型付き・言語付きリテラルの型情報を除き、エスケープ文字とUnicodeを一度だけ復元し、IRIを表示する。
