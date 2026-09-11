"use client";

import { useEffect, useRef, useState } from "react";
import { toDataURL } from "qrcode";
import { ApiError, apiFetch } from "@/lib/api";
import { labelValuesFromNQuads } from "@/lib/label-values";
import { LabelFieldPicker, type LabelTerm } from "@/components/label-field-picker";

const OCCURRENCE_DETAIL_API_PREFIX = "/api/backend/occurrences";
const LABEL_TEMPLATE_API_PATH = "/label-templates";
const MAX_LABEL_TEMPLATES = 10;
const DECIMAL_LATITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DECIMAL_LONGITUDE_PREDICATE = "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const EVENT_DATE_PREDICATE = "http://rs.tdwg.org/dwc/terms/eventDate";
const LOCALITY_PREDICATE = "http://rs.tdwg.org/dwc/terms/locality";
const EMPTY_VALUES: Record<string, string[]> = {};
const FIELD_OPTIONS = [
  ["scientificName", "学名"], ["creator", "作成者"],
  ["eventDate", "採集・観察日"], ["locality", "場所"],
  ["coordinates", "緯度・経度"], ["created", "作成日"], ["qrCode", "QRコード"],
] as const;
type LabelField = typeof FIELD_OPTIONS[number][0];
type TemplateBuiltinField = Exclude<LabelField, "qrCode">;
const TEMPLATE_BUILTIN_FIELDS: TemplateBuiltinField[] = [
  "scientificName", "creator", "eventDate", "locality", "coordinates", "created",
];
const DEFAULT_FIELDS: LabelField[] = ["scientificName", "creator", "coordinates", "created", "qrCode"];
const DEFAULT_SETTINGS = { width: 40, height: 20, qr: 15, font: 2.5 };


type LabelPreviewMode = "a4" | "individual";
type TemplateAction = "idle" | "loading" | "saving" | "updating" | "deleting";
type LabelTemplateField = {
  type: "builtin" | "darwinCore";
  key?: string;
  uri?: string;
  enabled: boolean;
};
type LabelTemplateDefinition = {
  version: 1;
  name: string;
  widthMm: number;
  heightMm: number;
  fontSizeMm: number;
  qr: { enabled: boolean; sizeMm: number };
  fields: LabelTemplateField[];
};
type LabelTemplateResponse = { id: string; template: LabelTemplateDefinition };
type ListLabelTemplatesResponse = { templates: LabelTemplateResponse[] };


export interface LabelOccurrence {
  occurrence_id: string;
  occurrence_uri: string;
  creator_user_id: string | null;
  scientific_name: string | null;
  created: string | null;
}

export function LabelPreviewDialog({
  creatorNames,
  occurrences,
  onClose,
}: {
  creatorNames: Record<string, string>;
  occurrences: LabelOccurrence[];
  onClose: () => void;
}) {
  const [valuesByOccurrenceId, setValuesByOccurrenceId] = useState<Record<string, Record<string, string[]>>>({});
  const [customFields, setCustomFields] = useState<LabelTerm[]>([]);
  const [previewMode, setPreviewMode] = useState<LabelPreviewMode>("individual");
  const [currentLabelIndex, setCurrentLabelIndex] = useState(0);
  const [settings, setSettings] = useState(DEFAULT_SETTINGS);
  const [fields, setFields] = useState<LabelField[]>(DEFAULT_FIELDS);
  const [qrCodes, setQrCodes] = useState<Record<string, string>>({});
  const [ready, setReady] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [overflowCount, setOverflowCount] = useState(0);
  const [savedTemplates, setSavedTemplates] = useState<LabelTemplateResponse[]>([]);
  const [selectedTemplateId, setSelectedTemplateId] = useState<string | null>(null);
  const [templateName, setTemplateName] = useState("");
  const [templateAction, setTemplateAction] = useState<TemplateAction>("loading");
  const [templateMessage, setTemplateMessage] = useState<string | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const [previewWidth, setPreviewWidth] = useState(600);
  const columns = Math.max(1, Math.floor(200 / settings.width));
  const rows = Math.max(1, Math.floor(277 / settings.height));
  const perPage = columns * rows;

  useEffect(() => {
    dialogRef.current?.showModal();
    const previous = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const observer = new ResizeObserver(([entry]) => setPreviewWidth(entry.contentRect.width));
    if (previewRef.current) observer.observe(previewRef.current);
    return () => {
      document.body.style.overflow = previous;
      observer.disconnect();
    };
  }, []);

  useEffect(() => {
    let active = true;
    void apiFetch<ListLabelTemplatesResponse>(LABEL_TEMPLATE_API_PATH, { cache: "no-store" })
      .then((response) => {
        if (!active) return;
        setSavedTemplates(response.templates);
        setTemplateAction("idle");
      })
      .catch(() => {
        if (!active) return;
        setTemplateAction("idle");
        setTemplateMessage("保存済みテンプレートを取得できませんでした。");
      });
    return () => { active = false; };
  }, []);

  const a4PageRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [pdfAction, setPdfAction] = useState<"idle" | "printing" | "downloading" | "error">("idle");

  useEffect(() => {
    // Inspect the actual print elements after fonts/layout settle, including labels on later pages.
    let active = true;
    void document.fonts.ready.then(() => {
      requestAnimationFrame(() => {
        if (!active) return;
        const labels = a4PageRefs.current.filter(Boolean).flatMap((page) =>
          Array.from(page!.querySelectorAll<HTMLElement>("[data-label-text]")));
        setOverflowCount(labels.filter((label) => label.scrollHeight > label.clientHeight + 1).length);
      });
    });
    return () => { active = false; };
  }, [settings, fields, customFields, valuesByOccurrenceId, ready, creatorNames]);

  useEffect(() => {
    let active = true;

    // Resolve text and QR assets before enabling output so an early click cannot print incomplete labels.
    void Promise.all(occurrences.map(async (occurrence) => ({
      id: occurrence.occurrence_id,
      values: await fetchOccurrenceValues(occurrence.occurrence_id),
      qr: await toDataURL(occurrence.occurrence_uri, {
        color: { dark: "#000000", light: "#ffffff" },
        errorCorrectionLevel: "M", margin: 2, width: 512,
      }),
    }))).then((entries) => {
      if (!active) return;
      setValuesByOccurrenceId(Object.fromEntries(entries.map((entry) => [entry.id, entry.values])));
      setQrCodes(Object.fromEntries(entries.map((entry) => [entry.id, entry.qr])));
      setReady(true);
    }).catch(() => {
      if (active) setLoadError(true);
    });

    return () => {
      active = false;
    };
  }, [occurrences]);

  const a4Pages = Array.from(
    { length: Math.ceil(occurrences.length / perPage) },
    (_, pageIndex) =>
      occurrences.slice(
        pageIndex * perPage,
        (pageIndex + 1) * perPage,
      ),
  );
  const activeOccurrence = occurrences[currentLabelIndex] ?? occurrences[0];
  const isPdfProcessing = pdfAction === "printing" || pdfAction === "downloading";
  const isTemplateBusy = templateAction !== "idle";

  function resetToDefault() {
    setSettings(DEFAULT_SETTINGS);
    setFields(DEFAULT_FIELDS);
    setCustomFields([]);
    setSelectedTemplateId(null);
    setTemplateName("");
    setTemplateMessage(null);
  }

  function applyTemplate(record: LabelTemplateResponse) {
    const definition = record.template;
    const enabledBuiltin = definition.fields
      .filter((field): field is LabelTemplateField & { key: TemplateBuiltinField } =>
        field.type === "builtin" && field.enabled && isTemplateBuiltinField(field.key))
      .map((field) => field.key);
    const enabledCustom = definition.fields
      .filter((field): field is LabelTemplateField & { uri: string } =>
        field.type === "darwinCore" && field.enabled && typeof field.uri === "string")
      .map((field) => ({ uri: field.uri, local_name: labelFromDarwinCoreUri(field.uri) }));

    setSettings({
      width: definition.widthMm,
      height: definition.heightMm,
      qr: definition.qr.sizeMm,
      font: definition.fontSizeMm,
    });
    setFields(definition.qr.enabled ? [...enabledBuiltin, "qrCode"] : enabledBuiltin);
    setCustomFields(enabledCustom);
    setSelectedTemplateId(record.id);
    setTemplateName(definition.name);
    setTemplateMessage(null);
  }

  function currentTemplateDefinition(): LabelTemplateDefinition | null {
    const name = templateName.trim();
    if (!name) {
      setTemplateMessage("テンプレート名を入力してください。");
      return null;
    }
    if (name.length > 100) {
      setTemplateMessage("テンプレート名は100文字以内にしてください。");
      return null;
    }
    if (customFields.some((field) => !isDarwinCoreUri(field.uri))) {
      setTemplateMessage("保存できる任意項目はDarwin CoreのURIだけです。");
      return null;
    }

    return {
      version: 1,
      name,
      widthMm: settings.width,
      heightMm: settings.height,
      fontSizeMm: settings.font,
      qr: { enabled: fields.includes("qrCode"), sizeMm: settings.qr },
      fields: [
        ...TEMPLATE_BUILTIN_FIELDS.map((key) => ({
          type: "builtin" as const,
          key,
          enabled: fields.includes(key),
        })),
        ...customFields.map((field) => ({
          type: "darwinCore" as const,
          uri: field.uri,
          enabled: true,
        })),
      ],
    };
  }

  async function saveTemplate() {
    if (savedTemplates.length >= MAX_LABEL_TEMPLATES) {
      setTemplateMessage(`保存できるテンプレートは${MAX_LABEL_TEMPLATES}件までです。`);
      return;
    }
    const template = currentTemplateDefinition();
    if (!template) return;

    setTemplateAction("saving");
    setTemplateMessage(null);
    try {
      const created = await apiFetch<LabelTemplateResponse>(LABEL_TEMPLATE_API_PATH, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ template }),
      });
      setSavedTemplates((current) => [...current, created]);
      setSelectedTemplateId(created.id);
      setTemplateName(created.template.name);
      setTemplateMessage("テンプレートを保存しました。");
    } catch (error) {
      setTemplateMessage(templateErrorMessage(error));
    } finally {
      setTemplateAction("idle");
    }
  }

  async function updateTemplate() {
    if (!selectedTemplateId) return;
    const template = currentTemplateDefinition();
    if (!template) return;

    setTemplateAction("updating");
    setTemplateMessage(null);
    try {
      const updated = await apiFetch<LabelTemplateResponse>(`${LABEL_TEMPLATE_API_PATH}/${encodeURIComponent(selectedTemplateId)}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ template }),
      });
      setSavedTemplates((current) => current.map((record) => record.id === updated.id ? updated : record));
      setTemplateName(updated.template.name);
      setTemplateMessage("テンプレートを更新しました。");
    } catch (error) {
      setTemplateMessage(templateErrorMessage(error));
    } finally {
      setTemplateAction("idle");
    }
  }

  async function deleteTemplate() {
    if (!selectedTemplateId) return;
    const deletingId = selectedTemplateId;
    setTemplateAction("deleting");
    setTemplateMessage(null);
    try {
      await apiFetch<{ deleted: boolean }>(`${LABEL_TEMPLATE_API_PATH}/${encodeURIComponent(deletingId)}`, {
        method: "DELETE",
      });
      setSavedTemplates((current) => current.filter((record) => record.id !== deletingId));
      resetToDefault();
      setTemplateMessage("テンプレートを削除しました。");
    } catch (error) {
      setTemplateMessage(templateErrorMessage(error));
    } finally {
      setTemplateAction("idle");
    }
  }

  async function createA4Pdf(): Promise<Blob> {
    await document.fonts.ready;
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
      await Promise.all(Array.from(pageElement.querySelectorAll("img")).map((img) => img.decode()));
      const canvas = await html2canvas(pageElement, {
        backgroundColor: "#ffffff",
        scale: 2,
        // Screen previews are scaled to fit. Capture the unscaled millimetre layout.
        onclone: (_document, element) => {
          let parent = element.parentElement;
          while (parent) {
            parent.style.transform = "none";
            parent = parent.parentElement;
          }
        },
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

  const noFields = fields.length === 0 && customFields.length === 0;
  const invalid = noFields || (fields.includes("qrCode") &&
    (settings.qr > settings.height - 2 || settings.qr > settings.width - 2));
  const printDisabled = !ready || invalid || isPdfProcessing;
  const mmToPx = 96 / 25.4;
  const individualScale = Math.min(4, Math.max(0.1, (previewWidth - 40) / (settings.width * mmToPx)));
  const sheetScale = Math.min(0.8, Math.max(0.1, (previewWidth - 40) / (210 * mmToPx)));

  const renderLabel = (occurrence: LabelOccurrence) => (
    <OccurrenceLabel key={occurrence.occurrence_id} occurrence={occurrence}
      creatorName={occurrence.creator_user_id ? creatorNames[occurrence.creator_user_id] ?? null : null}
      values={valuesByOccurrenceId[occurrence.occurrence_id] ?? EMPTY_VALUES} customFields={customFields}
      settings={settings} fields={fields} qrCode={qrCodes[occurrence.occurrence_id]} />
  );

  return (
    <dialog ref={dialogRef} aria-labelledby="label-preview-title"
      onCancel={(event) => { event.preventDefault(); if (!isPdfProcessing) onClose(); }}
      className="fixed inset-0 m-auto h-[94dvh] max-h-none w-[96vw] max-w-[1440px] overflow-hidden rounded-md border border-[#c9d0d3] bg-white p-0 text-[#182126] shadow-xl backdrop:bg-black/40">
      <div className="flex h-full flex-col">
        <header className="flex shrink-0 items-center justify-between border-b border-[#d8dfe2] px-5 py-3">
          <div className="flex items-baseline gap-3">
            <h2 className="text-lg font-semibold" id="label-preview-title">標本ラベル作成</h2>
            <span className="text-sm text-[#526168]">{occurrences.length}件</span>
          </div>
          <button type="button" title="閉じる" aria-label="ラベル作成を閉じる" disabled={isPdfProcessing}
            onClick={onClose} className="size-9 rounded hover:bg-[#eef2f3] disabled:opacity-40">×</button>
        </header>
        <div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] md:grid-cols-[280px_minmax(0,1fr)] md:grid-rows-1">
          <fieldset disabled={isPdfProcessing} className="max-h-[40dvh] overflow-y-auto border-b border-[#d8dfe2] p-5 md:max-h-none md:border-r md:border-b-0">
            <section className="mb-5 border-b border-[#d8dfe2] pb-5">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-sm font-semibold">テンプレート</h3>
                <span className="text-xs tabular-nums text-[#526168]">{savedTemplates.length} / {MAX_LABEL_TEMPLATES}</span>
              </div>
              <label className="mt-3 block text-xs text-[#526168]">保存済みテンプレート
                <select value={selectedTemplateId ?? ""} disabled={isTemplateBusy}
                  className="mt-1 block h-9 w-full rounded border border-[#b8c3c8] bg-white px-2 text-sm text-[#182126] disabled:opacity-50"
                  onChange={(event) => {
                    const id = event.target.value;
                    if (!id) {
                      resetToDefault();
                      return;
                    }
                    const record = savedTemplates.find((template) => template.id === id);
                    if (record) applyTemplate(record);
                  }}>
                  <option value="">デフォルト</option>
                  {savedTemplates.map((record) => <option key={record.id} value={record.id}>{record.template.name}</option>)}
                </select>
              </label>
              <label className="mt-3 block text-xs text-[#526168]">テンプレート名
                <input type="text" maxLength={100} value={templateName} disabled={isTemplateBusy}
                  placeholder="例: ミミズ標本 40×20"
                  className="mt-1 block h-9 w-full rounded border border-[#b8c3c8] bg-white px-2 text-sm text-[#182126] disabled:opacity-50"
                  onChange={(event) => { setTemplateName(event.target.value); setTemplateMessage(null); }} />
              </label>
              <div className="mt-3 grid grid-cols-2 gap-2">
                <button type="button" disabled={isTemplateBusy || invalid || savedTemplates.length >= MAX_LABEL_TEMPLATES}
                  onClick={() => void saveTemplate()}
                  className="rounded bg-[#176b57] px-3 py-2 text-sm text-white hover:bg-[#125746] disabled:opacity-40">
                  {templateAction === "saving" ? "保存中…" : "新規保存"}
                </button>
                <button type="button" disabled={isTemplateBusy || invalid || !selectedTemplateId}
                  onClick={() => void updateTemplate()}
                  className="rounded border border-[#176b57] px-3 py-2 text-sm text-[#176b57] hover:bg-[#e8f2ef] disabled:opacity-40">
                  {templateAction === "updating" ? "更新中…" : "上書き"}
                </button>
              </div>
              {selectedTemplateId && <button type="button" disabled={isTemplateBusy}
                onClick={() => void deleteTemplate()}
                className="mt-2 w-full rounded border border-[#b8c3c8] px-3 py-2 text-sm text-[#526168] hover:bg-[#eef2f3] disabled:opacity-40">
                {templateAction === "deleting" ? "削除中…" : "このテンプレートを削除"}
              </button>}
              {templateAction === "loading" && <p role="status" className="mt-2 text-xs text-[#526168]">テンプレートを読み込んでいます…</p>}
              {savedTemplates.length >= MAX_LABEL_TEMPLATES && <p className="mt-2 text-xs text-[#526168]">上限に達しています。新規保存するには既存テンプレートを削除してください。</p>}
              {templateMessage && <p role="status" className="mt-2 text-xs text-[#526168]">{templateMessage}</p>}
            </section>
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-semibold">レイアウト</h3>
              <button type="button" title="デフォルトに戻す" aria-label="デフォルトに戻す"
                className="size-8 rounded hover:bg-[#eef2f3]"
                onClick={resetToDefault}>↺</button>
            </div>
            <div className="mt-3 grid grid-cols-2 gap-3">
              <Dimension label="横幅 (mm)" value={settings.width} min={20} max={200}
                onChange={(width) => setSettings((current) => ({ ...current, width }))} />
              <Dimension label="縦幅 (mm)" value={settings.height} min={15} max={277}
                onChange={(height) => setSettings((current) => ({ ...current, height }))} />
            </div>
            <div className="mt-5 border-t border-[#d8dfe2] pt-4">
              <h3 className="mb-3 text-sm font-semibold">表示項目</h3>
              <div className="grid grid-cols-2 gap-3 md:grid-cols-1">
                {FIELD_OPTIONS.map(([field, label]) => (
                  <label key={field} className="flex cursor-pointer items-center gap-3 text-sm">
                    <input type="checkbox" className="size-4 accent-[#176b57]" checked={fields.includes(field)}
                      onChange={(event) => setFields((current) => event.target.checked
                        ? [...current, field] : current.filter((value) => value !== field))} />
                    {label}
                  </label>
                ))}
              </div>
            </div>
            <div>
              {customFields.map((term) => (
                <div key={term.uri} className="mt-3 flex items-center justify-between gap-2 text-sm">
                  <span className="min-w-0 break-words" title={term.uri}>{term.local_name}</span>
                  <button type="button" title="項目を削除" aria-label={term.local_name + "を削除"}
                    className="size-8 shrink-0 rounded hover:bg-[#eef2f3]"
                    onClick={() => setCustomFields((current) => current.filter((field) => field.uri !== term.uri))}>×</button>
                </div>
              ))}
              <LabelFieldPicker
                excluded={customFields.map((term) => term.uri)}
                onSelect={(term) => {
                  // A built-in property selected through the picker enables its existing checkbox.
                  const builtIn: Record<string, LabelField> = {
                    [EVENT_DATE_PREDICATE]: "eventDate", [LOCALITY_PREDICATE]: "locality",
                    "http://rs.tdwg.org/dwc/terms/scientificName": "scientificName",
                    "http://purl.org/dc/terms/creator": "creator",
                    "http://purl.org/dc/terms/created": "created",
                  };
                  const field = builtIn[term.uri];
                  if (field) setFields((current) => current.includes(field) ? current : [...current, field]);
                  else setCustomFields((current) => current.some((item) => item.uri === term.uri) ? current : [...current, term]);
                }} />
            </div>
            <div className="mt-5 grid grid-cols-2 gap-3 border-t border-[#d8dfe2] pt-4">
              <Dimension label="文字 (mm)" value={settings.font} min={1.5} max={6} step={0.25}
                onChange={(font) => setSettings((current) => ({ ...current, font }))} />
              {fields.includes("qrCode") && <Dimension label="QR (mm)" value={settings.qr} min={8} max={50}
                onChange={(qr) => setSettings((current) => ({ ...current, qr }))} />}
            </div>
            <p className="mt-5 text-xs text-[#526168]">A4縦 · {columns}列 × {rows}行 · {a4Pages.length}ページ</p>
            {invalid && <p role="alert" className="mt-3 text-sm text-[#a53d32]">{noFields
              ? "表示項目を選択してください。" : "QRコードをラベルの内側に収まるサイズにしてください。"}</p>}
            {overflowCount > 0 && <p role="status" className="mt-3 text-sm text-[#a53d32]">
              {overflowCount}件で文字が収まりません。文字サイズ・ラベル寸法・表示項目を調整してください。
            </p>}
          </fieldset>
          <div className="flex min-h-0 min-w-0 flex-col bg-[#e9edef]">
            <div className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-[#d8dfe2] bg-white px-4 py-3">
              <div className="flex rounded border border-[#b8c3c8] p-0.5">
                {(["individual", "a4"] as const).map((mode) => (
                  <button key={mode} type="button" aria-pressed={previewMode === mode}
                    onClick={() => setPreviewMode(mode)}
                    className={`rounded px-4 py-2 text-sm ${previewMode === mode ? "bg-[#176b57] text-white" : "hover:bg-[#eef2f3]"}`}>
                    {mode === "individual" ? "個別ラベル" : "A4全体"}
                  </button>
                ))}
              </div>
              <span className="text-xs text-[#526168]">{settings.width} × {settings.height} mm</span>
            </div>
            <div ref={previewRef} className="min-h-0 flex-1 overflow-auto">
              {!ready && <p role="status" className="p-4 text-center text-sm">{loadError ? "ラベル用データを取得できませんでした。閉じて再度お試しください。" : "ラベルを準備しています…"}</p>}
              {previewMode === "individual" && activeOccurrence ? (
                <div className="flex min-h-full flex-col items-center justify-center gap-6 p-5">
                  <div style={{ width: settings.width * mmToPx * individualScale, height: settings.height * mmToPx * individualScale }}>
                    <div style={{ width: settings.width * mmToPx, transform: `scale(${individualScale})`, transformOrigin: "top left" }}>
                      {renderLabel(activeOccurrence)}
                    </div>
                  </div>
                  <div className="flex items-center gap-5">
                    <button type="button" title="前のラベル" aria-label="前のラベル" className="size-10 rounded border border-[#b8c3c8] bg-white disabled:opacity-30"
                      disabled={currentLabelIndex === 0} onClick={() => setCurrentLabelIndex((index) => index - 1)}>←</button>
                    <span className="min-w-16 text-center text-sm tabular-nums">{currentLabelIndex + 1} / {occurrences.length}</span>
                    <button type="button" title="次のラベル" aria-label="次のラベル" className="size-10 rounded border border-[#b8c3c8] bg-white disabled:opacity-30"
                      disabled={currentLabelIndex >= occurrences.length - 1} onClick={() => setCurrentLabelIndex((index) => index + 1)}>→</button>
                  </div>
                </div>
              ) : null}
              {/* Keep unscaled sheets mounted for PDF capture, even while inspecting an individual label. */}
              <div aria-hidden={previewMode !== "a4"} style={previewMode !== "a4" ? { position: "fixed", left: -10000, top: 0 } : undefined}
                className="flex flex-col items-center gap-6 p-5">
                {a4Pages.map((page, index) => (
                  <div key={index} style={{ width: 210 * mmToPx * sheetScale, height: 297 * mmToPx * sheetScale }}>
                    <div style={{ transform: `scale(${sheetScale})`, transformOrigin: "top left", width: "210mm" }}>
                      <div ref={(element) => { a4PageRefs.current[index] = element; }}
                        style={{ width: "210mm", height: "297mm", padding: "10mm 5mm", background: "#fff", boxSizing: "border-box" }}>
                        <div style={{ display: "grid", gridTemplateColumns: `repeat(${columns}, ${settings.width}mm)`, gridAutoRows: `${settings.height}mm`, gap: 0 }}>
                          {page.map(renderLabel)}
                        </div>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </div>
        <footer className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-t border-[#d8dfe2] bg-white px-5 py-3">
          <p role="status" className="text-sm text-[#526168]">{pdfAction === "error" ? "PDFを作成できませんでした" : isPdfProcessing ? "PDFを準備しています…" : `${occurrences.length}枚 / A4 ${a4Pages.length}ページ`}</p>
          <div className="flex gap-2">
            <button type="button" disabled={printDisabled} onClick={() => void printA4Pdf()}
              className="rounded border border-[#176b57] px-4 py-2 text-sm text-[#176b57] hover:bg-[#e8f2ef] disabled:opacity-40">印刷</button>
            <button type="button" disabled={printDisabled} onClick={() => void downloadA4Pdf()}
              className="rounded bg-[#176b57] px-4 py-2 text-sm text-white hover:bg-[#125746] disabled:opacity-40">PDFダウンロード</button>
          </div>
        </footer>
      </div>
    </dialog>
  );
}

function Dimension({ label, value, min, max, step = 1, onChange }: {
  label: string; value: number; min: number; max: number; step?: number; onChange: (value: number) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return <label className="block text-xs text-[#526168]">{label}
    <input type="number" min={min} max={max} step={step} value={draft ?? value}
      className="mt-1 block h-9 w-full rounded border border-[#b8c3c8] bg-white px-2 text-sm text-[#182126]"
      onChange={(event) => {
        setDraft(event.target.value);
        const number = event.target.valueAsNumber;
        if (Number.isFinite(number) && number >= min && number <= max) onChange(number);
      }}
      onBlur={() => {
        if (draft !== null && draft.trim() && Number.isFinite(Number(draft))) {
          onChange(Math.min(max, Math.max(min, Number(draft))));
        }
        setDraft(null);
      }} />
  </label>;
}

async function fetchOccurrenceValues(occurrenceId: string): Promise<Record<string, string[]>> {
  const response = await fetch(
    `${OCCURRENCE_DETAIL_API_PREFIX}/${encodeURIComponent(occurrenceId)}`,
    { cache: "no-store", credentials: "include" },
  );
  // A network failure is not the same as an absent value: do not print incomplete records silently.
  if (!response.ok) throw new Error("Occurrence detail could not be loaded");
  return labelValuesFromNQuads(await response.text());
}

function isTemplateBuiltinField(value: string | undefined): value is TemplateBuiltinField {
  return TEMPLATE_BUILTIN_FIELDS.some((field) => field === value);
}

function isDarwinCoreUri(uri: string): boolean {
  return /^https?:\/\/rs\.tdwg\.org\/dwc\/terms\/[^\s<>"']+$/u.test(uri);
}

function labelFromDarwinCoreUri(uri: string): string {
  const tail = uri.split("/").filter(Boolean).pop();
  return tail ? decodeURIComponent(tail) : uri;
}

function templateErrorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 409) return `保存できるテンプレートは${MAX_LABEL_TEMPLATES}件までです。`;
    if (error.status === 401) return "ログイン状態を確認してください。";
    if (error.status === 404) return "テンプレートが見つかりません。再読み込みしてください。";
    if (error.status === 400) return "テンプレートの内容を保存できません。入力内容を確認してください。";
  }
  return "テンプレートを保存できませんでした。もう一度お試しください。";
}

function OccurrenceLabel({ creatorName, values, customFields, occurrence, settings, fields, qrCode }: {
  creatorName: string | null;
  values: Record<string, string[]>;
  customFields: LabelTerm[];
  occurrence: LabelOccurrence;
  settings: typeof DEFAULT_SETTINGS;
  fields: LabelField[];
  qrCode?: string;
}) {
  const withQr = fields.includes("qrCode");
  // The same millimetre-based element is used for both previews and PDF output.
  return (
    <article style={{
      width: `${settings.width}mm`, height: `${settings.height}mm`, padding: "1mm",
      border: "0.2mm solid #000", background: "#fff", color: "#000", overflow: "hidden",
      fontSize: `${settings.font}mm`, lineHeight: 1.2, boxSizing: "border-box",
      display: "grid", gridTemplateColumns: withQr ? `minmax(0, 1fr) ${settings.qr}mm` : "minmax(0, 1fr)",
      gridTemplateRows: "minmax(0, 1fr)",
      gap: withQr ? "1mm" : 0, alignItems: "start",
    }}>
      <div data-label-text style={{ overflowWrap: "anywhere", minWidth: 0, maxHeight: "100%", overflow: "hidden" }}>
        {fields.includes("scientificName") && occurrence.scientific_name && <p style={{ margin: 0, fontWeight: 600 }}>{occurrence.scientific_name}</p>}
        {fields.includes("creator") && <p style={{ margin: 0 }}>{creatorName ?? "-"}</p>}
        {fields.includes("eventDate") && values[EVENT_DATE_PREDICATE]?.map((value) => <p key={value} style={{ margin: 0 }}>{value}</p>)}
        {fields.includes("locality") && values[LOCALITY_PREDICATE]?.map((value) => <p key={value} style={{ margin: 0 }}>{value}</p>)}
        {fields.includes("coordinates") && values[DECIMAL_LATITUDE_PREDICATE]?.[0] && values[DECIMAL_LONGITUDE_PREDICATE]?.[0] &&
          <p style={{ margin: 0 }}>{values[DECIMAL_LATITUDE_PREDICATE][0]}, {values[DECIMAL_LONGITUDE_PREDICATE][0]}</p>}
        {fields.includes("created") && <p style={{ margin: 0 }}>{formatLabelDate(occurrence.created)}</p>}
        {customFields.map((term) => values[term.uri]?.map((value) =>
          <p key={term.uri + value} style={{ margin: 0 }}>{value}</p>))}
      </div>
      {withQr && qrCode && <img alt="オカレンス詳細ページのQRコード" src={qrCode}
        style={{ width: `${settings.qr}mm`, height: `${settings.qr}mm`, alignSelf: "end" }} />}
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
