export default function Home() {
  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <header className="border-b border-[#d8dfe2] bg-white">
        <div className="mx-auto flex h-16 max-w-7xl items-center px-5 sm:px-8">
          <span className="mr-3 grid size-9 place-items-center rounded-md bg-[#176b57] text-sm font-bold text-white">BD</span>
          <div>
            <p className="text-sm font-semibold">Bio Database</p>
            <p className="text-xs text-[#65737a]">Occurrence management</p>
          </div>
        </div>
      </header>
      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">オカレンス</h1>
          <p className="mt-1 text-sm text-[#65737a]">登録された生物観察・標本データ</p>
        </div>
        <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="grid min-h-64 place-items-center px-6 py-12 text-center">
            <div>
              <p className="font-medium">データはまだ表示されていません</p>
              <p className="mt-2 text-sm text-[#65737a]">一覧取得画面をこの領域に実装します。</p>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
