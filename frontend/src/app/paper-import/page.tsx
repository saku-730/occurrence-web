"use client";

import { ChangeEvent, FormEvent, useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

type AuthStatus =
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "error";

type UploadStatus = "idle" | "uploading" | "success" | "error";

type StartPaperImportResponse = {
  status: "staged" | "metadata_required" | "already_imported";
  import_id: string | null;
  paper_id: string | null;
  original_filename: string | null;
  content_type: string;
  size_bytes: number;
  sha256: string;
  doi: string | null;
  title: string | null;
  requires_bibliographic_input: boolean;
  authors: string | null;
  publication_year: number | null;
  journal: string | null;
  volume: string | null;
  issue: string | null;
  pages: string | null;
  article_number: string | null;
  message: string;
};

const MAX_PDF_SIZE_BYTES = 100 * 1024 * 1024;

export default function PaperImportPage() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [uploadStatus, setUploadStatus] = useState<UploadStatus>("idle");
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [uploadResult, setUploadResult] =
    useState<StartPaperImportResponse | null>(null);

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

  const handleFileChange = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0] ?? null;

    setUploadResult(null);
    setUploadError(null);
    setUploadStatus("idle");

    if (!file) {
      setSelectedFile(null);
      return;
    }

    if (!file.name.toLowerCase().endsWith(".pdf")) {
      setSelectedFile(null);
      setUploadStatus("error");
      setUploadError("PDFファイルを選択してください。");
      event.target.value = "";
      return;
    }

    if (file.size === 0) {
      setSelectedFile(null);
      setUploadStatus("error");
      setUploadError("空のPDFファイルはアップロードできません。");
      event.target.value = "";
      return;
    }

    if (file.size > MAX_PDF_SIZE_BYTES) {
      setSelectedFile(null);
      setUploadStatus("error");
      setUploadError("PDFファイルは100MB以下にしてください。");
      event.target.value = "";
      return;
    }

    setSelectedFile(file);
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();

    if (!selectedFile || uploadStatus === "uploading") return;

    setUploadStatus("uploading");
    setUploadError(null);
    setUploadResult(null);

    const formData = new FormData();
    const pdfFile =
      selectedFile.type === "application/pdf"
        ? selectedFile
        : new File([selectedFile], selectedFile.name, {
            type: "application/pdf",
            lastModified: selectedFile.lastModified,
          });
    formData.append("file", pdfFile);

    try {
      const result = await apiFetch<StartPaperImportResponse>("/paper-import", {
        method: "POST",
        body: formData,
      });
      setUploadResult(result);
      setUploadStatus("success");
    } catch (error: unknown) {
      setUploadStatus("error");
      setUploadError(getUploadErrorMessage(error));
    }
  };

  if (authStatus !== "authenticated") {
    const message =
      authStatus === "loading"
        ? "ログイン状態を確認しています"
        : authStatus === "unauthenticated"
          ? "論文をインポートするにはログインが必要です"
          : "ログイン状態を確認できませんでした";

    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">論文インポート</h1>
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
          <h1 className="text-2xl font-semibold">論文インポート</h1>
        </div>

        <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
            <h2 className="text-sm font-medium text-[#526168]">論文PDF</h2>
          </div>

          <form className="px-5 py-6" onSubmit={handleSubmit}>
            <p className="mb-5 text-sm leading-6 text-[#526168]">
              インポートする論文PDFを選択してバックエンドへ送信します。PDFは最大100MBです。
            </p>

            <label className="block">
              <span className="mb-2 block text-sm font-medium text-[#344249]">
                PDFファイル
              </span>
              <input
                type="file"
                accept="application/pdf,.pdf"
                onChange={handleFileChange}
                disabled={uploadStatus === "uploading"}
                className="block w-full rounded-md border border-[#c9d2d6] bg-white px-3 py-2 text-sm text-[#344249] file:mr-4 file:rounded file:border-0 file:bg-[#e8edef] file:px-4 file:py-2 file:text-sm file:font-medium file:text-[#344249] hover:file:bg-[#dde5e8] disabled:cursor-not-allowed disabled:opacity-60"
              />
            </label>

            {selectedFile && (
              <div className="mt-3 rounded-md bg-[#f5f7f8] px-4 py-3 text-sm text-[#526168]">
                <div className="font-medium text-[#344249]">{selectedFile.name}</div>
                <div className="mt-1">{formatFileSize(selectedFile.size)}</div>
              </div>
            )}

            {uploadError && (
              <div
                role="alert"
                className="mt-4 rounded-md border border-[#e1b8b8] bg-[#fff5f5] px-4 py-3 text-sm text-[#8d3131]"
              >
                {uploadError}
              </div>
            )}

            {uploadResult && (
              <div className="mt-4 rounded-md border border-[#bfd8c7] bg-[#f3faf5] px-4 py-4 text-sm text-[#345d40]">
                <p className="font-medium">{getUploadResultMessage(uploadResult)}</p>
                <dl className="mt-3 grid gap-2 text-[#526168] sm:grid-cols-[9rem_1fr]">
                  <dt>ファイル名</dt>
                  <dd className="break-all">
                    {uploadResult.original_filename ?? selectedFile?.name ?? "-"}
                  </dd>
                  <dt>状態</dt>
                  <dd>{uploadResult.status}</dd>
                  {uploadResult.import_id && (
                    <>
                      <dt>Import ID</dt>
                      <dd className="break-all font-mono text-xs">
                        {uploadResult.import_id}
                      </dd>
                    </>
                  )}
                  {uploadResult.paper_id && (
                    <>
                      <dt>Paper ID</dt>
                      <dd className="break-all font-mono text-xs">
                        {uploadResult.paper_id}
                      </dd>
                    </>
                  )}
                  {uploadResult.title && (
                    <>
                      <dt>タイトル</dt>
                      <dd>{uploadResult.title}</dd>
                    </>
                  )}
                  {uploadResult.doi && (
                    <>
                      <dt>DOI</dt>
                      <dd className="break-all">{uploadResult.doi}</dd>
                    </>
                  )}
                </dl>
              </div>
            )}

            <div className="mt-6 flex justify-end">
              <button
                type="submit"
                disabled={!selectedFile || uploadStatus === "uploading"}
                className="rounded-md bg-[#31434b] px-5 py-2.5 text-sm font-medium text-white hover:bg-[#25343a] disabled:cursor-not-allowed disabled:bg-[#9ca8ad]"
              >
                {uploadStatus === "uploading" ? "送信中..." : "PDFを送信"}
              </button>
            </div>
          </form>
        </section>
      </main>
    </div>
  );
}

function getUploadResultMessage(result: StartPaperImportResponse): string {
  switch (result.status) {
    case "staged":
      return "PDFをバックエンドへ送信し、インポート用に仮保存しました。";
    case "metadata_required":
      return "PDFを仮保存しました。続行するにはDOIまたはタイトルの入力が必要です。";
    case "already_imported":
      return "このPDFはすでに登録されています。";
  }
}

function getUploadErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) {
    return "PDFの送信に失敗しました。通信状態を確認してもう一度お試しください。";
  }

  const backendMessage = getBackendMessage(error.body);
  if (backendMessage) return backendMessage;

  switch (error.status) {
    case 400:
      return "PDFのアップロード内容が不正です。";
    case 401:
      return "ログインセッションが無効です。再ログインしてください。";
    case 413:
      return "PDFファイルは100MB以下にしてください。";
    case 415:
      return "PDFファイルのみアップロードできます。";
    case 502:
      return "PDFの保存または書誌情報抽出に失敗しました。";
    default:
      return `PDFの送信に失敗しました（HTTP ${error.status}）。`;
  }
}

function getBackendMessage(body: unknown): string | null {
  if (
    typeof body === "object" &&
    body !== null &&
    "message" in body &&
    typeof body.message === "string"
  ) {
    return body.message;
  }
  return null;
}

function formatFileSize(sizeBytes: number): string {
  if (sizeBytes < 1024) return `${sizeBytes} B`;
  if (sizeBytes < 1024 * 1024) return `${(sizeBytes / 1024).toFixed(1)} KB`;
  return `${(sizeBytes / (1024 * 1024)).toFixed(1)} MB`;
}
