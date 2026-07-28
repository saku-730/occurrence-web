"use client";
import { toDataURL } from "qrcode";

import Link from "next/link";
import { FormEvent, useEffect, useRef, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

const SCIENTIFIC_NAME_PREDICATE =
  "http://rs.tdwg.org/dwc/terms/scientificName";
const CREATOR_PREDICATE = "http://purl.org/dc/terms/creator";
const USER_URI_BASE = "https://bio-database.net/users/";
const OCCURRENCE_DETAIL_API_PREFIX = "/api/backend/occurrences";
const DECIMAL_LATITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DECIMAL_LONGITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLongitude";

interface CurrentUser {
  user_id: string;
  email: string;
  user_name: string;
  role: string;
}
interface UserSummary {
  user_id: string;
  user_name: string;
}


interface SearchFilter {
  predicate: string;
  value: string;
  value_type: "literal" | "uri";
  match: "exact";
}

interface OccurrenceItem {
  occurrence_id: string;
  occurrence_uri: string;
  // dcterms:creatorのURIからバックエンドが抽出したUUID。表示名は/users/{id}で解決する。
  creator_user_id: string | null;
  scientific_name: string | null;
  created: string | null;
  modified: string | null;
  access_rights: string | null;
}

interface OccurrenceCoordinates {
  latitude: string | null;
  longitude: string | null;
}

const EMPTY_COORDINATES: OccurrenceCoordinates = {
  latitude: null,
  longitude: null,
};

interface SearchResponse {
  items: OccurrenceItem[];
  page: {
    limit: number;
    next_cursor: string | null;
    has_next: boolean;
  };
}

type SearchStatus = "loading" | "ready" | "unauthenticated" | "error";
type LabelPreviewMode = "a4" | "individual";
type LabelDisplaySize = "print" | "individual";

const LABELS_PER_A4_PAGE = 65;

export default function OccurrenceSearchPage() {
  const [query, setQuery] = useState("");
  const [ownOnly, setOwnOnly] = useState(false);
  const [creatorNames, setCreatorNames] = useState<Record<string, string>>({});
  const [appliedQuery, setAppliedQuery] = useState("");
  const [appliedOwnOnly, setAppliedOwnOnly] = useState(false);
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [status, setStatus] = useState<SearchStatus>("loading");
  const [selectedOccurrenceIds, setSelectedOccurrenceIds] = useState<Set<string>>(
    new Set(),
  );

  const [isLabelPreviewOpen, setIsLabelPreviewOpen] = useState(false);

  useEffect(() => {
    let active = true;

    // An empty initial request provides the standard visible occurrence list.
    searchOccurrences("", null, false)
      .then((response) => {
        if (!active) return;
        setResult(response);
        setStatus("ready");
      })
      .catch(() => {
        if (active) setStatus("error");
      });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    const creatorIds = [...new Set(result?.items
      .map((item) => item.creator_user_id)
      .filter((creatorUserId): creatorUserId is string => creatorUserId !== null) ?? [])];

    if (creatorIds.length === 0) {
      setCreatorNames({});
      return;
    }

    let active = true;

    // 一覧応答のcreator UUIDを重複なく既存ユーザー概要APIへ問い合わせる。
    Promise.all(creatorIds.map(async (creatorUserId) => {
      try {
        const user = await apiFetch<UserSummary>(`/users/${encodeURIComponent(creatorUserId)}`);
        return [creatorUserId, user.user_name] as const;
      } catch {
        return null;
      }
    })).then((entries) => {
      if (!active) return;
      setCreatorNames(Object.fromEntries(entries.filter((entry): entry is readonly [string, string] => entry !== null)));
    });

    return () => {
      active = false;
    };
  }, [result]);

  // 検索結果が変わった選択は次のページや別条件へ引き継がない。
  useEffect(() => {
    setSelectedOccurrenceIds(new Set());
  }, [result]);

  async function runSearch(
    searchQuery: string,
    cursor: string | null,
    searchOwnOnly: boolean,
  ) {
    setStatus("loading");

    try {
      const response = await searchOccurrences(
        searchQuery,
        cursor,
        searchOwnOnly,
      );
      setResult(response);
      setAppliedQuery(searchQuery);
      setAppliedOwnOnly(searchOwnOnly);
      setStatus("ready");
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        setStatus("unauthenticated");
        return;
      }

      setStatus("error");
    }
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void runSearch(query.trim(), null, ownOnly);
  }

  const selectedOccurrences =
    result?.items.filter((item) => selectedOccurrenceIds.has(item.occurrence_id)) ?? [];
  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-base font-semibold">データ検索</h1>
        </div>

        <form className="mb-6 max-w-2xl" onSubmit={handleSubmit}>
          <div className="flex items-end gap-3">
            <label className="min-w-0 flex-1">
              <span className="mb-2 block text-sm font-medium">学名</span>
              <input
                className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                onChange={(event) => setQuery(event.target.value)}
                placeholder="例: Quercus serrata"
                type="search"
                value={query}
              />
            </label>
            <button
              className="h-10 shrink-0 rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#829b95]"
              disabled={status === "loading"}
              type="submit"
            >
              検索
            </button>
          </div>

          <label className="mt-4 flex w-fit cursor-pointer items-center gap-2 text-sm">
            <input
              checked={ownOnly}
              className="size-4 accent-[#176b57]"
              onChange={(event) => { const checked = event.target.checked; setOwnOnly(checked); void runSearch(query.trim(), null, checked); }}
              type="checkbox"
            />
            自分のデータのみ表示
          </label>
        </form>

        <section aria-label="検索結果">
          <div className="mb-3 flex min-h-10 items-center justify-end gap-3">
            {selectedOccurrenceIds.size > 0 ? (
              <span className="text-sm text-[#526168]">
                {selectedOccurrenceIds.size}件選択
              </span>
            ) : null}
            <button
              className="h-10 rounded-md border border-[#176b57] bg-white px-4 text-sm font-medium text-[#176b57] hover:bg-[#e8f2ef] disabled:cursor-not-allowed disabled:border-[#b8c3c8] disabled:text-[#829b95] disabled:hover:bg-white"
              disabled={selectedOccurrences.length === 0}
              onClick={() => setIsLabelPreviewOpen(true)}
              type="button"
            >
              ラベル作成
            </button>
          </div>

          <SearchResults
            creatorNames={creatorNames}
            onSelectedOccurrenceIdsChange={setSelectedOccurrenceIds}
            result={result}
            selectedOccurrenceIds={selectedOccurrenceIds}
            status={status}
          />
        </section>

        {status === "ready" && result?.page.has_next && result.page.next_cursor ? (
          <div className="mt-5 flex justify-center">
            <button
              className="rounded-md border border-[#b8c3c8] bg-white px-5 py-2 text-sm font-medium hover:bg-[#eef2f3]"
              onClick={() =>
                void runSearch(
                  appliedQuery,
                  result.page.next_cursor,
                  appliedOwnOnly,
                )
              }
              type="button"
            >
              次のページ
            </button>
          </div>
        ) : null}
      {isLabelPreviewOpen && selectedOccurrences.length > 0 ? (
        <LabelPreviewDialog
          creatorNames={creatorNames}
          occurrences={selectedOccurrences}
          onClose={() => setIsLabelPreviewOpen(false)}
        />
      ) : null}

      </main>
    </div>
  );
}


function LabelPreviewDialog({
  creatorNames,
  occurrences,
  onClose,
}: {
  creatorNames: Record<string, string>;
  occurrences: OccurrenceItem[];
  onClose: () => void;
}) {
  const [coordinatesByOccurrenceId, setCoordinatesByOccurrenceId] = useState<
    Record<string, OccurrenceCoordinates>
  >({});
  const [previewMode, setPreviewMode] = useState<LabelPreviewMode>("a4");
  const [currentLabelIndex, setCurrentLabelIndex] = useState(0);
  const a4PageRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [pdfAction, setPdfAction] = useState<"idle" | "printing" | "downloading" | "error">("idle");

  useEffect(() => {
    let active = true;

    void Promise.all(
      occurrences.map(async (occurrence) => [
        occurrence.occurrence_id,
        await fetchOccurrenceCoordinates(occurrence.occurrence_id),
      ] as const),
    ).then((entries) => {
      if (active) {
        setCoordinatesByOccurrenceId(Object.fromEntries(entries));
      }
    });

    return () => {
      active = false;
    };
  }, [occurrences]);

  const a4Pages = Array.from(
    { length: Math.ceil(occurrences.length / LABELS_PER_A4_PAGE) },
    (_, pageIndex) =>
      occurrences.slice(
        pageIndex * LABELS_PER_A4_PAGE,
        (pageIndex + 1) * LABELS_PER_A4_PAGE,
      ),
  );
  const activeOccurrence = occurrences[currentLabelIndex] ?? occurrences[0];
  const isPdfProcessing = pdfAction === "printing" || pdfAction === "downloading";

  async function createA4Pdf(): Promise<Blob> {
    const pageElements = a4PageRefs.current.filter(
      (page): page is HTMLDivElement => page !== null,
    );
    if (pageElements.length === 0) {
      throw new Error("A4 preview pages are unavailable");
    }

    const [{ default: html2canvas }, { jsPDF }] = await Promise.all([
      import("html2canvas"),
      import("jspdf"),
    ]);
    const pdf = new jsPDF({
      compress: true,
      format: "a4",
      orientation: "portrait",
      unit: "mm",
    });

    for (const [index, pageElement] of pageElements.entries()) {
      const canvas = await html2canvas(pageElement, {
        backgroundColor: "#ffffff",
        scale: 2,
      });

      if (index > 0) {
        pdf.addPage("a4", "portrait");
      }

      pdf.addImage(canvas.toDataURL("image/png"), "PNG", 0, 0, 210, 297);
    }

    return pdf.output("blob");
  }

  async function downloadA4Pdf() {
    setPdfAction("downloading");

    try {
      const pdfBlob = await createA4Pdf();
      const url = URL.createObjectURL(pdfBlob);
      const downloadLink = document.createElement("a");
      downloadLink.href = url;
      downloadLink.download = "occurrence-labels.pdf";
      document.body.append(downloadLink);
      downloadLink.click();
      downloadLink.remove();
      window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
      setPdfAction("idle");
    } catch {
      setPdfAction("error");
    }
  }

  async function printA4Pdf() {
    // Open synchronously so browsers do not block the print preview as a popup.
    const printWindow = window.open("", "_blank");
    if (!printWindow) {
      setPdfAction("error");
      return;
    }

    setPdfAction("printing");

    try {
      const pdfBlob = await createA4Pdf();
      const url = URL.createObjectURL(pdfBlob);
      let printed = false;
      const triggerPrint = () => {
        if (printed) return;
        printed = true;
        printWindow.focus();
        printWindow.print();
      };

      printWindow.addEventListener("load", triggerPrint, { once: true });
      printWindow.location.href = url;
      window.setTimeout(triggerPrint, 1_500);
      window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
      setPdfAction("idle");
    } catch {
      printWindow.close();
      setPdfAction("error");
    }
  }

  return (
    <div
      aria-labelledby="label-preview-title"
      aria-modal="true"
      className="fixed inset-0 z-50 grid place-items-center bg-[#182126]/45 p-5"
      onMouseDown={onClose}
      role="dialog"
    >
      <section
        className="flex max-h-[calc(100vh-2.5rem)] w-full max-w-7xl flex-col rounded-md bg-white shadow-xl"
        onMouseDown={(event) => event.stopPropagation()}
      >
        <header className="flex items-center justify-between border-b border-[#d8dfe2] px-5 py-4">
          <h2 className="text-base font-semibold" id="label-preview-title">
            ラベル作成
          </h2>
          <button
            aria-label="ラベル作成を閉じる"
            className="grid size-9 place-items-center rounded-md text-xl leading-none text-[#526168] hover:bg-[#eef2f3]"
            onClick={onClose}
            type="button"
          >
            ×
          </button>
        </header>

        <div className="flex items-center justify-between border-b border-[#d8dfe2] px-5 py-3">
          <div className="flex overflow-hidden rounded-md border border-[#b8c3c8]">
            <button
              aria-pressed={previewMode === "a4"}
              className={`h-9 px-3 text-sm font-medium ${previewMode === "a4" ? "bg-[#176b57] text-white" : "bg-white hover:bg-[#eef2f3]"}`}
              onClick={() => setPreviewMode("a4")}
              type="button"
            >
              A4全体
            </button>
            <button
              aria-pressed={previewMode === "individual"}
              className={`h-9 border-l border-[#b8c3c8] px-3 text-sm font-medium ${previewMode === "individual" ? "bg-[#176b57] text-white" : "bg-white hover:bg-[#eef2f3]"}`}
              onClick={() => setPreviewMode("individual")}
              type="button"
            >
              個別
            </button>
          </div>
          <p className="text-sm text-[#526168]">{occurrences.length}件</p>
        </div>

        <div className="overflow-x-auto overflow-y-auto bg-[#eef2f3] p-5">
          {previewMode === "a4" ? (
            <div className="space-y-8">
              {a4Pages.map((pageOccurrences, pageIndex) => (
                <div
                  className="aspect-[210/297] w-[210mm] min-w-[210mm] max-w-none bg-white p-[10mm_5mm] shadow-sm"
                  ref={(element) => {
                    a4PageRefs.current[pageIndex] = element;
                  }}
                  key={pageOccurrences[0]?.occurrence_id ?? pageIndex}
                >
                  <div className="grid h-full grid-cols-5 grid-rows-13 content-start gap-0">
                    {pageOccurrences.map((occurrence) => (
                      <OccurrenceLabel
                        creatorName={occurrence.creator_user_id ? creatorNames[occurrence.creator_user_id] ?? null : null}
                        coordinates={coordinatesByOccurrenceId[occurrence.occurrence_id] ?? EMPTY_COORDINATES}
                        key={occurrence.occurrence_id}
                        occurrence={occurrence}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          ) : activeOccurrence ? (
            <div className="mx-auto flex max-w-3xl items-center justify-center gap-4">
              <button
                aria-label="前のラベル"
                className="grid size-10 shrink-0 place-items-center rounded-md border border-[#b8c3c8] bg-white text-lg hover:bg-[#e8f2ef] disabled:cursor-not-allowed disabled:text-[#a8b2b6] disabled:hover:bg-white"
                disabled={currentLabelIndex === 0}
                onClick={() => setCurrentLabelIndex((index) => index - 1)}
                type="button"
              >
                &lt;
              </button>
              <div className="min-w-0 flex-1">
                <OccurrenceLabel
                  creatorName={activeOccurrence.creator_user_id ? creatorNames[activeOccurrence.creator_user_id] ?? null : null}
                  coordinates={coordinatesByOccurrenceId[activeOccurrence.occurrence_id] ?? EMPTY_COORDINATES}
                  displaySize="individual"
                  occurrence={activeOccurrence}
                />
                <p className="mt-3 text-center text-sm text-[#526168]">
                  {currentLabelIndex + 1} / {occurrences.length}
                </p>
              </div>
              <button
                aria-label="次のラベル"
                className="grid size-10 shrink-0 place-items-center rounded-md border border-[#b8c3c8] bg-white text-lg hover:bg-[#e8f2ef] disabled:cursor-not-allowed disabled:text-[#a8b2b6] disabled:hover:bg-white"
                disabled={currentLabelIndex === occurrences.length - 1}
                onClick={() => setCurrentLabelIndex((index) => index + 1)}
                type="button"
              >
                &gt;
              </button>
            </div>
          ) : null}
        </div>

        <footer className="flex items-center justify-between border-t border-[#d8dfe2] px-5 py-4">
          <div className="flex items-center gap-2">
            {previewMode === "a4" ? (
              <>
                <button
                  className="h-10 rounded-md border border-[#176b57] bg-white px-4 text-sm font-medium text-[#176b57] hover:bg-[#e8f2ef] disabled:cursor-not-allowed disabled:border-[#b8c3c8] disabled:text-[#829b95] disabled:hover:bg-white"
                  disabled={isPdfProcessing}
                  onClick={() => void printA4Pdf()}
                  type="button"
                >
                  {pdfAction === "printing" ? "準備中" : "印刷"}
                </button>
                <button
                  className="h-10 rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#829b95]"
                  disabled={isPdfProcessing}
                  onClick={() => void downloadA4Pdf()}
                  type="button"
                >
                  {pdfAction === "downloading" ? "準備中" : "PDFダウンロード"}
                </button>
              </>
            ) : null}
            {pdfAction === "error" ? (
              <p className="text-sm text-[#a53d32]">PDFを作成できませんでした</p>
            ) : null}
          </div>
          <button
            className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3]"
            onClick={onClose}
            type="button"
          >
            閉じる
          </button>
        </footer>
      </section>
    </div>
  );
}

async function fetchOccurrenceCoordinates(occurrenceId: string): Promise<OccurrenceCoordinates> {
  try {
    const response = await fetch(
      `${OCCURRENCE_DETAIL_API_PREFIX}/${encodeURIComponent(occurrenceId)}`,
      { cache: "no-store", credentials: "include" },
    );

    if (!response.ok) return EMPTY_COORDINATES;

    return extractCoordinatesFromNQuads(await response.text());
  } catch {
    return EMPTY_COORDINATES;
  }
}

function extractCoordinatesFromNQuads(nquads: string): OccurrenceCoordinates {
  return {
    latitude: findLiteralObject(nquads, DECIMAL_LATITUDE_PREDICATE),
    longitude: findLiteralObject(nquads, DECIMAL_LONGITUDE_PREDICATE),
  };
}

function findLiteralObject(nquads: string, predicate: string): string | null {
  const prefix = `> <${predicate}> `;

  for (const line of nquads.split(String.fromCharCode(10))) {
    if (!line.includes(prefix)) continue;

    const match = line.match(/^<[^>]+> <[^>]+> "((?:\\.|[^"\\])*)"/u);
    if (!match) continue;

    return match[1].replace(/\\"/gu, "\"").replace(/\\\\/gu, "\\");
  }

  return null;
}

// CSS millimeter units keep the label and QR code at their intended physical
// dimensions when the browser preview is printed.

function OccurrenceLabel({
  creatorName,
  coordinates,
  displaySize = "print",
  occurrence,
}: {
  creatorName: string | null;
  coordinates: OccurrenceCoordinates;
  displaySize?: LabelDisplaySize;
  occurrence: OccurrenceItem;
}) {
  const scientificName = occurrence.scientific_name ?? "";
  const isIndividualPreview = displaySize === "individual";
  const labelClassName = isIndividualPreview
    ? "relative h-[20rem] w-[40rem] max-w-none overflow-hidden border border-[#182126] bg-white p-4 text-[2.5rem] leading-tight text-[#182126]"
    : "relative h-[20mm] w-[40mm] max-w-none overflow-hidden border border-[#182126] bg-white p-[1mm] text-[2.5mm] leading-tight text-[#182126]";
  const headingClassName = isIndividualPreview
    ? "mt-0 break-words text-[2.5rem] font-semibold leading-tight"
    : "mt-0 break-words text-[2.5mm] font-semibold leading-tight";
  const detailsClassName = isIndividualPreview
    ? "mt-2 grid grid-cols-[minmax(0,1fr)_15rem] items-end gap-4 text-[2.5rem] leading-tight"
    : "mt-[0.5mm] grid grid-cols-[minmax(0,1fr)_15mm] items-end gap-[1mm] text-[2.5mm] leading-tight";
  const qrCodeClassName = isIndividualPreview
    ? "h-[15rem] w-[15rem] bg-white p-1"
    : "h-[15mm] w-[15mm] bg-white p-[0.25mm]";
  const [qrCodeDataUrl, setQrCodeDataUrl] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    void toDataURL(occurrence.occurrence_uri, {
      color: { dark: "#182126", light: "#ffffff" },
      errorCorrectionLevel: "M",
      margin: 0,
      width: 256,
    })
      .then((dataUrl) => {
        if (active) setQrCodeDataUrl(dataUrl);
      })
      .catch(() => {
        if (active) setQrCodeDataUrl(null);
      });

    return () => {
      active = false;
    };
  }, [occurrence.occurrence_uri]);

  return (
    <article className={labelClassName}>
      {scientificName ? (
        <h3 className={headingClassName}>{scientificName}</h3>
      ) : null}
      <div className={detailsClassName}>
        <div>
          <p>{creatorName ?? "-"}</p>
          {coordinates.latitude && coordinates.longitude ? (
            <p>{`${coordinates.latitude}, ${coordinates.longitude}`}</p>
          ) : null}
          <p className="text-[#182126]">{formatLabelDate(occurrence.created)}</p>
        </div>
        {qrCodeDataUrl ? (
          <img
            alt="オカレンス詳細ページのQRコード"
            className={qrCodeClassName}
            src={qrCodeDataUrl}
          />
        ) : null}
      </div>
    </article>
  );
}

function SearchResults({
  creatorNames,
  onSelectedOccurrenceIdsChange,
  result,
  selectedOccurrenceIds,
  status,
}: {
  creatorNames: Record<string, string>;
  onSelectedOccurrenceIdsChange: React.Dispatch<React.SetStateAction<Set<string>>>;
  result: SearchResponse | null;
  selectedOccurrenceIds: Set<string>;
  status: SearchStatus;
}) {
  if (status === "loading") {
    return <StatusPanel message="検索しています" />;
  }

  if (status === "unauthenticated") {
    return <StatusPanel message="自分のデータを検索するにはログインが必要です" />;
  }

  if (status === "error") {
    return <StatusPanel message="検索結果を取得できませんでした" />;
  }

  if (!result || result.items.length === 0) {
    return <StatusPanel message="該当するデータはありません" />;
  }

  const occurrenceIds = result.items.map((item) => item.occurrence_id);
  const allSelected = occurrenceIds.every((occurrenceId) =>
    selectedOccurrenceIds.has(occurrenceId),
  );

  function toggleOccurrenceSelection(occurrenceId: string, checked: boolean) {
    onSelectedOccurrenceIdsChange((current) => {
      const next = new Set(current);
      if (checked) {
        next.add(occurrenceId);
      } else {
        next.delete(occurrenceId);
      }
      return next;
    });
  }

  function toggleAllOccurrenceSelection(checked: boolean) {
    onSelectedOccurrenceIdsChange(
      checked ? new Set(occurrenceIds) : new Set(),
    );
  }

  return (
    <div className="overflow-x-auto rounded-md border border-[#d8dfe2] bg-white">
      <table className="w-full min-w-[900px] border-collapse text-left text-sm">
        <thead className="border-b border-[#d8dfe2] bg-[#eef2f3] text-xs text-[#526168]">
          <tr>
            <TableHeader>
              <input
                aria-label="検索結果をすべて選択"
                checked={allSelected}
                className="size-4 accent-[#176b57]"
                onChange={(event) =>
                  toggleAllOccurrenceSelection(event.target.checked)
                }
                type="checkbox"
              />
            </TableHeader>
            <TableHeader>ID</TableHeader>
            <TableHeader>学名</TableHeader>
            <TableHeader>作成者</TableHeader>
            <TableHeader>作成日時</TableHeader>
            <TableHeader>更新日時</TableHeader>
            <TableHeader>公開範囲</TableHeader>
          </tr>
        </thead>
        <tbody className="divide-y divide-[#e4e9eb]">
          {result.items.map((item) => (
            <tr key={item.occurrence_id} className="hover:bg-[#f8faf9]">
              <TableCell>
                <input
                  aria-label={`${item.occurrence_id}を選択`}
                  checked={selectedOccurrenceIds.has(item.occurrence_id)}
                  className="size-4 accent-[#176b57]"
                  onChange={(event) =>
                    toggleOccurrenceSelection(
                      item.occurrence_id,
                      event.target.checked,
                    )
                  }
                  type="checkbox"
                />
              </TableCell>
              <TableCell>
                <Link
                  className="font-medium text-[#176b57] hover:underline"
                  href={`/occurrences/${item.occurrence_id}`}
                >
                  {item.occurrence_id}
                </Link>
              </TableCell>
              <TableCell>{item.scientific_name ?? "-"}</TableCell>
              <TableCell>{item.creator_user_id ? creatorNames[item.creator_user_id] ?? "-" : "-"}</TableCell>
              <TableCell>{formatDate(item.created)}</TableCell>
              <TableCell>{formatDate(item.modified)}</TableCell>
              <TableCell>{item.access_rights ?? "-"}</TableCell>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function TableHeader({ children }: { children: React.ReactNode }) {
  return <th className="whitespace-nowrap px-4 py-3 font-medium">{children}</th>;
}

function TableCell({ children }: { children: React.ReactNode }) {
  return <td className="px-4 py-3 align-top">{children}</td>;
}

function StatusPanel({ message }: { message: string }) {
  return (
    <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
      <p className="text-sm text-[#65737a]">{message}</p>
    </section>
  );
}

async function searchOccurrences(
  query: string,
  cursor: string | null,
  ownOnly: boolean,
): Promise<SearchResponse> {
  const filters: SearchFilter[] = [];

  if (query) {
    filters.push({
      predicate: SCIENTIFIC_NAME_PREDICATE,
      value: query,
      value_type: "literal",
      match: "exact",
    });
  }

  if (ownOnly) {
    // The creator URI must come from the authenticated session, never from a
    // user-editable field, otherwise another user's ownership could be queried.
    const currentUser = await apiFetch<CurrentUser>("/auth/me");
    filters.push({
      predicate: CREATOR_PREDICATE,
      value: `${USER_URI_BASE}${currentUser.user_id}`,
      value_type: "uri",
      match: "exact",
    });
  }

  return apiFetch<SearchResponse>("/occurrences/search", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      filters,
      page: {
        limit: 50,
        cursor,
      },
    }),
  });
}

function formatLabelDate(value: string | null): string {
  if (!value) return "-";

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
  }).format(date);
}

function formatDate(value: string | null): string {
  if (!value) return "-";

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
