"use client";

import { useEffect, useRef, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

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
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [selectedFiles, setSelectedFiles] = useState<File[]>([]);
  const [authStatus, setAuthStatus] = useState<
    "loading" | "authenticated" | "unauthenticated" | "error"
  >("loading");

  useEffect(() => {
    let active = true;

    apiFetch<unknown>("/auth/me")
      .then(() => {
        if (active) setAuthStatus("authenticated");
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (error instanceof ApiError && error.status === 401) {
          setAuthStatus("unauthenticated");
          return;
        }
        setAuthStatus("error");
      });

    return () => {
      active = false;
    };
  }, []);

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

  function addFiles(files: FileList | null) {
    if (!files) return;

    setSelectedFiles((currentFiles) => {
      const knownFiles = new Set(
        currentFiles.map((file) => fileIdentity(file)),
      );
      const addedFiles = Array.from(files).filter(
        (file) => !knownFiles.has(fileIdentity(file)),
      );
      return [...currentFiles, ...addedFiles];
    });

    // Allow selecting the same file again after it has been removed.
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  function removeFile(target: File) {
    setSelectedFiles((currentFiles) =>
      currentFiles.filter((file) => fileIdentity(file) !== fileIdentity(target)),
    );
  }

  if (authStatus !== "authenticated") {
    const message =
      authStatus === "loading"
        ? "ログイン状態を確認しています"
        : authStatus === "unauthenticated"
          ? "データを登録するにはログインが必要です"
          : "ログイン状態を確認できませんでした";

    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">データ登録</h1>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">{message}</p>
          </section>
        </main>
      </div>
    );
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
            <span>項目名</span>
            <span>値</span>
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
                    項目名 {index + 1}
                  </span>
                  <input
                    className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                    onChange={(event) =>
                      updateRow(row.id, "predicate", event.target.value)
                    }
                    placeholder="項目名"
                    type="url"
                    value={row.predicate}
                  />
                </label>

                <label className="min-w-0">
                  <span className="mb-2 block text-sm font-medium md:sr-only">
                    値 {index + 1}
                  </span>
                  <input
                    className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                    onChange={(event) =>
                      updateRow(row.id, "object", event.target.value)
                    }
                    placeholder="値"
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

        <section className="mt-6 overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
            <h2 className="text-sm font-medium text-[#526168]">添付ファイル</h2>
          </div>

          <div className="px-5 py-5">
            <input
              accept=".jpg,.jpeg,.png,.webp,.mp3,.wav,.m4a,.mp4,.mov"
              className="sr-only"
              multiple
              onChange={(event) => addFiles(event.target.files)}
              ref={fileInputRef}
              type="file"
            />
            <button
              className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3]"
              onClick={() => fileInputRef.current?.click()}
              type="button"
            >
              ファイルを追加
            </button>

            {selectedFiles.length > 0 ? (
              <ul className="mt-5 divide-y divide-[#e4e9eb] border-y border-[#e4e9eb]">
                {selectedFiles.map((file) => (
                  <li
                    className="flex min-w-0 items-center gap-4 py-3"
                    key={fileIdentity(file)}
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{file.name}</p>
                      <p className="mt-1 text-xs text-[#65737a]">
                        {formatFileSize(file.size)}
                      </p>
                    </div>
                    <button
                      className="shrink-0 px-1 text-sm text-[#a23c32] hover:underline"
                      onClick={() => removeFile(file)}
                      type="button"
                    >
                      削除
                    </button>
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        </section>
      </main>
    </div>
  );
}


function fileIdentity(file: File): string {
  return `${file.name}:${file.size}:${file.lastModified}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
