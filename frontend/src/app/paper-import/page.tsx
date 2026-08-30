"use client";

import { useEffect, useState } from "react";
import type { ChangeEvent, FormEvent } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

type AuthStatus =
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "error";

type UploadStatus = "idle" | "uploading" | "success" | "error";
type ExtractionStatus = "idle" | "extracting" | "success" | "error";

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

type CompleteBibliographicMetadataResponse = {
  status: "staged" | "metadata_required";
  import_id: string;
  doi: string | null;
  title: string | null;
  requires_bibliographic_input: boolean;
  message: string;
};

type ExtractedOccurrenceCandidate = {
  scientificName: string;
  locality: string | null;
};

type ExtractOccurrencesResponse = {
  status: "reviewing";
  import_id: string;
  occurrences: ExtractedOccurrenceCandidate[];
};

const MAX_PDF_SIZE_BYTES = 100 * 1024 * 1024;

export default function PaperImportPage() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [uploadStatus, setUploadStatus] = useState<UploadStatus>("idle");
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [uploadResult, setUploadResult] =
    useState<StartPaperImportResponse | null>(null);
  const [titleInput, setTitleInput] = useState("");
  const [doiInput, setDoiInput] = useState("");
  const [extractionStatus, setExtractionStatus] =
    useState<ExtractionStatus>("idle");
  const [extractionError, setExtractionError] = useState<string | null>(null);
  const [extractionResult, setExtractionResult] =
    useState<ExtractOccurrencesResponse | null>(null);

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

  const resetImportFlow = () => {
    setUploadResult(null);
    setTitleInput("");
    setDoiInput("");
    setExtractionStatus("idle");
    setExtractionError(null);
    setExtractionResult(null);
  };

  const handleFileChange = (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0] ?? null;

    resetImportFlow();
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
    resetImportFlow();

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
      setTitleInput(result.title ?? "");
      setDoiInput(result.doi ?? "");
      setUploadStatus("success");
    } catch (error: unknown) {
      setUploadStatus("error");
      setUploadError(getUploadErrorMessage(error));
    }
  };

  const handleExtractOccurrences = async () => {
    if (!uploadResult?.import_id || extractionStatus === "extracting") return;

    const title = titleInput.trim();
    const doi = doiInput.trim();

    if (!title && !doi) {
      setExtractionStatus("error");
      setExtractionError("Occurrence抽出を続けるにはタイトルまたはDOIが必要です。");
      return;
    }

    setExtractionStatus("extracting");
    setExtractionError(null);
    setExtractionResult(null);

    try {
      let readyResult = uploadResult;

      if (uploadResult.requires_bibliographic_input) {
        const metadata = await apiFetch<CompleteBibliographicMetadataResponse>(
          `/paper-imports/${uploadResult.import_id}/bibliographic-metadata`,
          {
            method: "PATCH",
            headers: {
              "Content-Type": "application/json",
            },
            body: JSON.stringify({
              doi: doi || null,
              title: title || null,
            }),
          },
        );

        if (metadata.requires_bibliographic_input) {
          setExtractionStatus("error");
          setExtractionError("タイトルまたはDOIを入力してください。");
          return;
        }

        readyResult = {
          ...uploadResult,
          status: "staged",
          doi: metadata.doi,
          title: metadata.title,
          requires_bibliographic_input: false,
        };
        setUploadResult(readyResult);
        setTitleInput(metadata.title ?? title);
        setDoiInput(metadata.doi ?? doi);
      }

      const result = await apiFetch<ExtractOccurrencesResponse>(
        `/paper-imports/${readyResult.import_id}/extract-occurrences`,
        { method: "POST" },
      );

      setExtractionResult(result);
      setExtractionStatus("success");
    } catch (error: unknown) {
      setExtractionStatus("error");
      setExtractionError(getExtractionErrorMessage(error));
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
            <h2 className="text-sm font-medium text-[#526168]">1. 論文PDF</h2>
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
                disabled={uploadStatus === "uploading" || extractionStatus === "extracting"}
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

            <div className="mt-6 flex justify-end">
              <button
                type="submit"
                disabled={!selectedFile || uploadStatus === "uploading" || extractionStatus === "extracting"}
                className="rounded-md bg-[#31434b] px-5 py-2.5 text-sm font-medium text-white hover:bg-[#25343a] disabled:cursor-not-allowed disabled:bg-[#9ca8ad]"
              >
                {uploadStatus === "uploading" ? "送信中..." : "PDFを送信"}
              </button>
            </div>
          </form>
        </section>

        {uploadResult && (
          <section className="mt-6 overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
            <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
              <h2 className="text-sm font-medium text-[#526168]">2. 書誌情報の確認</h2>
            </div>

            <div className="px-5 py-6">
              <div
                className={`mb-5 rounded-md border px-4 py-3 text-sm ${
                  uploadResult.status === "already_imported"
                    ? "border-[#d8dfe2] bg-[#f5f7f8] text-[#526168]"
                    : "border-[#bfd8c7] bg-[#f3faf5] text-[#345d40]"
                }`}
              >
                {getUploadResultMessage(uploadResult)}
              </div>

              <dl className="mb-5 grid gap-2 text-sm text-[#526168] sm:grid-cols-[9rem_1fr]">
                <dt>ファイル名</dt>
                <dd className="break-all">
                  {uploadResult.original_filename ?? selectedFile?.name ?? "-"}
                </dd>
                <dt>状態</dt>
                <dd>{uploadResult.status}</dd>
                {uploadResult.import_id && (
                  <>
                    <dt>Import ID</dt>
                    <dd className="break-all font-mono text-xs">{uploadResult.import_id}</dd>
                  </>
                )}
                {uploadResult.paper_id && (
                  <>
                    <dt>Paper ID</dt>
                    <dd className="break-all font-mono text-xs">{uploadResult.paper_id}</dd>
                  </>
                )}
              </dl>

              {uploadResult.status !== "already_imported" && uploadResult.import_id && (
                <>
                  <div className="grid gap-4">
                    <label className="block">
                      <span className="mb-2 block text-sm font-medium text-[#344249]">タイトル</span>
                      <input
                        type="text"
                        value={titleInput}
                        onChange={(event) => setTitleInput(event.target.value)}
                        readOnly={Boolean(uploadResult.title)}
                        placeholder="GROBIDで取得できなかった場合は入力してください"
                        className="w-full rounded-md border border-[#c9d2d6] px-3 py-2.5 text-sm text-[#344249] read-only:bg-[#f5f7f8] read-only:text-[#526168]"
                      />
                    </label>

                    <label className="block">
                      <span className="mb-2 block text-sm font-medium text-[#344249]">DOI</span>
                      <input
                        type="text"
                        value={doiInput}
                        onChange={(event) => setDoiInput(event.target.value)}
                        readOnly={Boolean(uploadResult.doi)}
                        placeholder="例: 10.1234/example または https://doi.org/..."
                        className="w-full rounded-md border border-[#c9d2d6] px-3 py-2.5 text-sm text-[#344249] read-only:bg-[#f5f7f8] read-only:text-[#526168]"
                      />
                    </label>
                  </div>

                  {uploadResult.requires_bibliographic_input && (
                    <p className="mt-3 text-sm text-[#8b641f]">
                      GROBIDからタイトルとDOIの両方を取得できませんでした。どちらか一方を入力してください。
                    </p>
                  )}

                  {extractionError && (
                    <div
                      role="alert"
                      className="mt-4 rounded-md border border-[#e1b8b8] bg-[#fff5f5] px-4 py-3 text-sm text-[#8d3131]"
                    >
                      {extractionError}
                    </div>
                  )}

                  <div className="mt-6 flex justify-end">
                    <button
                      type="button"
                      onClick={handleExtractOccurrences}
                      disabled={extractionStatus === "extracting" || extractionStatus === "success"}
                      className="rounded-md bg-[#31434b] px-5 py-2.5 text-sm font-medium text-white hover:bg-[#25343a] disabled:cursor-not-allowed disabled:bg-[#9ca8ad]"
                    >
                      {extractionStatus === "extracting"
                        ? "LLMで抽出中..."
                        : extractionStatus === "success"
                          ? "抽出完了"
                          : "この論文からOccurrenceを抽出"}
                    </button>
                  </div>
                </>
              )}
            </div>
          </section>
        )}

        {extractionResult && (
          <section className="mt-6 overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
            <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
              <h2 className="text-sm font-medium text-[#526168]">3. LLM抽出結果</h2>
            </div>

            <div className="px-5 py-6">
              <p className="mb-5 text-sm text-[#526168]">
                抽出されたOccurrence候補です。まだ正式登録はされていません。
              </p>

              {extractionResult.occurrences.length === 0 ? (
                <div className="rounded-md border border-[#d8dfe2] bg-[#f5f7f8] px-4 py-6 text-center text-sm text-[#65737a]">
                  Occurrence候補は抽出されませんでした。
                </div>
              ) : (
                <div className="overflow-x-auto rounded-md border border-[#d8dfe2]">
                  <table className="w-full border-collapse text-left text-sm">
                    <thead className="bg-[#f5f7f8] text-[#526168]">
                      <tr>
                        <th className="border-b border-[#d8dfe2] px-4 py-3 font-medium">#</th>
                        <th className="border-b border-[#d8dfe2] px-4 py-3 font-medium">学名</th>
                        <th className="border-b border-[#d8dfe2] px-4 py-3 font-medium">Locality</th>
                      </tr>
                    </thead>
                    <tbody>
                      {extractionResult.occurrences.map((occurrence, index) => (
                        <tr key={`${occurrence.scientificName}-${occurrence.locality ?? ""}-${index}`}>
                          <td className="border-b border-[#e5eaec] px-4 py-3 text-[#65737a]">
                            {index + 1}
                          </td>
                          <td className="border-b border-[#e5eaec] px-4 py-3 font-medium text-[#344249]">
                            {occurrence.scientificName}
                          </td>
                          <td className="border-b border-[#e5eaec] px-4 py-3 text-[#526168]">
                            {occurrence.locality ?? "-"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </div>
          </section>
        )}
      </main>
    </div>
  );
}

function getUploadResultMessage(result: StartPaperImportResponse): string {
  switch (result.status) {
    case "staged":
      return "PDFを仮保存し、GROBIDによる書誌情報抽出が完了しました。";
    case "metadata_required":
      return "PDFを仮保存しましたが、GROBIDからタイトルまたはDOIを取得できませんでした。";
    case "already_imported":
      return "このPDFはすでに正式登録されています。";
  }
}

function getUploadErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) {
    return "PDFの送信に失敗しました。通信状態を確認してもう一度お試しください。";
  }

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
      return getBackendMessage(error.body) ?? `PDFの送信に失敗しました（HTTP ${error.status}）。`;
  }
}

function getExtractionErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) {
    return "Occurrence抽出に失敗しました。通信状態を確認してもう一度お試しください。";
  }

  switch (error.status) {
    case 400:
      return "論文インポートの指定が不正です。";
    case 401:
      return "ログインセッションが無効です。再ログインしてください。";
    case 404:
      return "この論文はOccurrence抽出を開始できる状態ではありません。";
    case 502:
      return "PDFの読み込みまたはLLMによるOccurrence抽出に失敗しました。";
    default:
      return getBackendMessage(error.body) ?? `Occurrence抽出に失敗しました（HTTP ${error.status}）。`;
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
