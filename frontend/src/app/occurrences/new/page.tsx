"use client";

import { useRef, useState } from "react";

import { SiteHeader } from "@/components/site-header";

interface StatementRow {
  id: number;
  predicate: string;
  object: string;
}

const initialRows: StatementRow[] = [
  { id: 1, predicate: "", object: "" },
  { id: 2, predicate: "", object: "" },
];

export default function NewOccurrencePage() {
  const [rows, setRows] = useState(initialRows);
  const nextId = useRef(3);

  function updateRow(
    id: number,
    field: "predicate" | "object",
    value: string,
  ) {
    setRows((currentRows) =>
      currentRows.map((row) =>
        row.id === id ? { ...row, [field]: value } : row,
      ),
    );
  }

  function addRow() {
    setRows((currentRows) => [
      ...currentRows,
      { id: nextId.current++, predicate: "", object: "" },
    ]);
  }

  function removeRow(id: number) {
    setRows((currentRows) => currentRows.filter((row) => row.id !== id));
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">データ登録</h1>
        </div>

        <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-4 border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3 text-xs font-medium text-[#526168] md:grid">
            <span>述語</span>
            <span>目的語</span>
            <span className="w-12" aria-hidden="true" />
          </div>

          <div className="divide-y divide-[#e4e9eb]">
            {rows.map((row, index) => (
              <div
                className="grid gap-4 px-5 py-5 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end"
                key={row.id}
              >
                <label className="min-w-0">
                  <span className="mb-2 block text-sm font-medium md:sr-only">
                    述語 {index + 1}
                  </span>
                  <input
                    className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                    onChange={(event) =>
                      updateRow(row.id, "predicate", event.target.value)
                    }
                    placeholder="述語URI"
                    type="url"
                    value={row.predicate}
                  />
                </label>

                <label className="min-w-0">
                  <span className="mb-2 block text-sm font-medium md:sr-only">
                    目的語 {index + 1}
                  </span>
                  <input
                    className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                    onChange={(event) =>
                      updateRow(row.id, "object", event.target.value)
                    }
                    placeholder="目的語"
                    type="text"
                    value={row.object}
                  />
                </label>

                <button
                  className="h-10 w-fit px-1 text-sm text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline md:w-12"
                  disabled={rows.length === 1}
                  onClick={() => removeRow(row.id)}
                  type="button"
                >
                  削除
                </button>
              </div>
            ))}
          </div>

          <div className="border-t border-[#d8dfe2] px-5 py-4">
            <button
              className="text-sm font-medium text-[#176b57] hover:underline"
              onClick={addRow}
              type="button"
            >
              入力行を追加
            </button>
          </div>
        </section>
      </main>
    </div>
  );
}
