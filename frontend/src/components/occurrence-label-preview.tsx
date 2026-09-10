"use client";

import { useEffect, useRef, useState } from "react";
import { toDataURL } from "qrcode";

const OCCURRENCE_DETAIL_API_PREFIX = "/api/backend/occurrences";
const DECIMAL_LATITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DECIMAL_LONGITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const LABELS_PER_A4_PAGE = 65;

type LabelPreviewMode = "a4" | "individual";
type LabelDisplaySize = "print" | "individual";

export interface LabelOccurrence {
  occurrence_id: string;
  occurrence_uri: string;
  creator_user_id: string | null;
  scientific_name: string | null;
  created: string | null;
}

interface OccurrenceCoordinates {
  latitude: string | null;
  longitude: string | null;
}

const EMPTY_COORDINATES: OccurrenceCoordinates = {
  latitude: null,
  longitude: null,
};

export function LabelPreviewDialog({
  creatorNames,
  occurrences,
  onClose,
}: {
  creatorNames: Record<string, string>;
  occurrences: LabelOccurrence[];
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
          <h2 className="text-base font-semibold" id="label-preview-title">ラベル作成</h2>
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

function OccurrenceLabel({
  creatorName,
  coordinates,
  displaySize = "print",
  occurrence,
}: {
  creatorName: string | null;
  coordinates: OccurrenceCoordinates;
  displaySize?: LabelDisplaySize;
  occurrence: LabelOccurrence;
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
      {scientificName ? <h3 className={headingClassName}>{scientificName}</h3> : null}
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

function formatLabelDate(value: string | null): string {
  if (!value) return "-";

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
  }).format(date);
}
