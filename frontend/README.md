# Occurrence Web Frontend

生物のオカレンス情報を管理・登録・共有するWebアプリケーションのフロントエンドです。

## Development

前提:

- Node.js 20.9以上
- バックエンドが `http://127.0.0.1:3001` で起動していること

```bash
npm install
cp .env.example .env.local
npm run dev
```

フロントエンドは `http://localhost:3002` で起動します。ブラウザからの
`/api/backend/*` リクエストは、`BACKEND_URL`で指定したRustバックエンドへ
Next.jsが中継します。

## Commands

```bash
npm run dev
npm run lint
npm run build
```
