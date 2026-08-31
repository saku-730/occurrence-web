"use client";

import { useEffect, useRef, useState } from "react";

import { ApiError, apiFetch } from "@/lib/api";

const OCCURRENCE_GRAPH_URI = "https://bio-database.net/graphs/occurrences";
const ASSOCIATED_MEDIA_PREDICATE_URI = "http://rs.tdwg.org/ac/terms/associatedMedia";
const ACCESS_RIGHTS_PREDICATE_URI = "http://purl.org/dc/terms/accessRights";
const PUBLIC_ACCESS_RIGHTS_URI = "https://bio-database.net/terms/access-rights/public";
const PRIVATE_ACCESS_RIGHTS_URI = "https://bio-database.net/terms/access-rights/private";
const MAX_MEDIA_SIZE_BYTES = 1000 * 1024 * 1024;
const DWCIRI_TO_TAXON_URI = "http://rs.tdwg.org/dwc/iri/toTaxon";
const DWCIRI_TO_TAXON_LABEL = "分類";
const DWC_SCIENTIFIC_NAME_URI = "http://rs.tdwg.org/dwc/terms/scientificName";
const DWC_SCIENTIFIC_NAME_LABEL = "scientificName";
const DWC_DECIMAL_LONGITUDE_URI = "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const DWC_DECIMAL_LONGITUDE_LABEL = "経度";
const DWC_DECIMAL_LATITUDE_URI = "http://rs.tdwg.org/dwc/terms/decimalLatitude";
const DWC_DECIMAL_LATITUDE_LABEL = "緯度";
const DWC_LOCALITY_URI = "http://rs.tdwg.org/dwc/terms/locality";
const DWC_LOCALITY_LABEL = "locality";
const GBIF_SUGGEST_ENDPOINT = "https://api.gbif.org/v1/species/suggest";
const GBIF_SPECIES_URI_PREFIX = "https://www.gbif.org/species/";
const GBIF_SUGGEST_DEBOUNCE_MS = 300;

export type PaperOccurrenceCandidate = {
  scientificName: string;
  toTaxon: string | null;
  taxonScientificName: string | null;
  locality: string | null;
  decimalLatitude: number | null;
  decimalLongitude: number | null;
};

type DarwinCoreTerm = {
  uri: string;
  local_name: string;
};

type StatementRow = {
  id: number;
  predicate: string;
  object: string;
};

type EditorState = {
  key: number;
  rows: StatementRow[];
  nextId: number;
  selectedFiles: File[];
  taxonLabels: Record<number, string>;
  isPublic: boolean;
};

type UploadMediaResponse = {
  media_uri: string;
};

type CreateOccurrenceResponse = {
  occurrence_id: string;
  occurrence_uri: string;
};

type BatchRegistrationResponse = {
  occurrences: CreateOccurrenceResponse[];
};

type GbifSpeciesSuggestion = {
  key: number;
  scientificName: string;
  canonicalName?: string;
};

type TermsStatus = "idle" | "loading" | "loaded" | "error";

export function PaperOccurrenceBulkEditor({
  paperId,
  candidates,
}: {
  paperId: string;
  candidates: PaperOccurrenceCandidate[];
}) {
  const [editors, setEditors] = useState<EditorState[]>(() =>
    candidates.map((candidate, index) => buildEditorState(candidate, index)),
  );
  const nextEditorKey = useRef(candidates.length);
  const [darwinCoreTerms, setDarwinCoreTerms] = useState<DarwinCoreTerm[]>([]);
  const [termsStatus, setTermsStatus] = useState<TermsStatus>("idle");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submissionMessage, setSubmissionMessage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [registered, setRegistered] = useState<CreateOccurrenceResponse[] | null>(null);

  async function loadDarwinCoreTerms() {
    if (termsStatus !== "idle") return;
    setTermsStatus("loading");
    try {
      const terms = await apiFetch<DarwinCoreTerm[]>("/vocabularies/darwin-core");
      const visibleTerms = terms.filter(
        (term) =>
          term.uri !== DWCIRI_TO_TAXON_URI &&
          term.uri !== DWC_SCIENTIFIC_NAME_URI &&
          term.uri !== DWC_DECIMAL_LONGITUDE_URI &&
          term.uri !== DWC_DECIMAL_LATITUDE_URI &&
          term.uri !== DWC_LOCALITY_URI,
      );
      setDarwinCoreTerms([
        { uri: DWCIRI_TO_TAXON_URI, local_name: DWCIRI_TO_TAXON_LABEL },
        { uri: DWC_SCIENTIFIC_NAME_URI, local_name: DWC_SCIENTIFIC_NAME_LABEL },
        { uri: DWC_DECIMAL_LONGITUDE_URI, local_name: DWC_DECIMAL_LONGITUDE_LABEL },
        { uri: DWC_DECIMAL_LATITUDE_URI, local_name: DWC_DECIMAL_LATITUDE_LABEL },
        { uri: DWC_LOCALITY_URI, local_name: DWC_LOCALITY_LABEL },
        ...visibleTerms,
      ]);
      setTermsStatus("loaded");
    } catch {
      setTermsStatus("error");
    }
  }

  function updateEditor(index: number, next: EditorState) {
    setEditors((current) =>
      current.map((editor, currentIndex) => (currentIndex === index ? next : editor)),
    );
  }

  function removeEditor(key: number) {
    setEditors((current) => current.filter((editor) => editor.key !== key));
    setErrorMessage(null);
  }

  function addEditor() {
    const key = nextEditorKey.current++;
    setEditors((current) => [...current, buildEmptyEditorState(key)]);
    setErrorMessage(null);
  }

  async function handleBulkRegister() {
    if (isSubmitting || registered || editors.length === 0) return;

    setErrorMessage(null);
    setSubmissionMessage(null);

    const prepared: Array<{ statements: StatementRow[]; editor: EditorState }> = [];
    try {
      for (const [index, editor] of editors.entries()) {
        const statements = validateStatementRows(editor.rows);
        validateSelectedFiles(editor.selectedFiles);
        if (statements.length === 0 && editor.selectedFiles.length === 0) {
          throw new Error(`Occurrence ${index + 1}: 項目または添付ファイルを1つ以上入力してください`);
        }
        prepared.push({ statements, editor });
      }
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "入力内容を確認してください");
      return;
    }

    setIsSubmitting(true);
    try {
      const nquadsList: string[] = [];

      for (const [occurrenceIndex, item] of prepared.entries()) {
        const mediaUris: string[] = [];
        for (const [fileIndex, file] of item.editor.selectedFiles.entries()) {
          setSubmissionMessage(
            `Occurrence ${occurrenceIndex + 1}/${prepared.length}: 添付ファイルをアップロードしています (${fileIndex + 1}/${item.editor.selectedFiles.length})`,
          );
          const formData = new FormData();
          formData.append("file", file, file.name);
          const uploaded = await apiFetch<UploadMediaResponse>("/media", {
            method: "POST",
            body: formData,
          });
          mediaUris.push(uploaded.media_uri);
        }

        const accessRightsUri = item.editor.isPublic
          ? PUBLIC_ACCESS_RIGHTS_URI
          : PRIVATE_ACCESS_RIGHTS_URI;
        nquadsList.push(buildOccurrenceNQuads(item.statements, mediaUris, accessRightsUri));
      }

      setSubmissionMessage(`${nquadsList.length}件のOccurrenceを一括登録しています`);
      const response = await apiFetch<BatchRegistrationResponse>(
        `/papers/${encodeURIComponent(paperId)}/occurrences/batch`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ occurrences: nquadsList }),
        },
      );

      setRegistered(response.occurrences);
      setSubmissionMessage(null);
    } catch (error) {
      setErrorMessage(registrationErrorMessage(error));
      setSubmissionMessage(null);
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <div>
      {editors.length > 0 ? (
        <div className="space-y-8">
          {editors.map((editor, index) => (
            <OccurrenceEditorCard
              key={editor.key}
              index={index}
              editor={editor}
              disabled={isSubmitting || Boolean(registered)}
              darwinCoreTerms={darwinCoreTerms}
              termsStatus={termsStatus}
              onLoadTerms={() => void loadDarwinCoreTerms()}
              onChange={(next) => updateEditor(index, next)}
              onDelete={() => removeEditor(editor.key)}
            />
          ))}
        </div>
      ) : (
        <p className="text-sm text-[#65737a]">登録対象のOccurrenceはありません。</p>
      )}

      {errorMessage ? (
        <div className="mt-6 rounded-md border border-[#e1b8b8] bg-[#fff5f5] px-4 py-3 text-sm text-[#8d3131]" role="alert">
          {errorMessage}
        </div>
      ) : null}

      {registered ? (
        <section className="mt-6 rounded-md border border-[#9dbeb4] bg-[#f3faf5] px-5 py-4" aria-live="polite">
          <p className="font-medium">{registered.length}件のデータを登録しました</p>
          <p className="mt-2 text-sm text-[#526168]">
            確認画面に表示した分類・scientificNameと論文出典情報を保存し、paperをregisteredに更新しました。
          </p>
        </section>
      ) : null}

      <div className="mt-8 flex flex-wrap items-center justify-between gap-3 border-t border-[#d8dfe2] pt-6">
        <button
          className="h-11 rounded-md border border-[#176b57] bg-white px-5 text-sm font-medium text-[#176b57] hover:bg-[#f2f8f6] disabled:cursor-not-allowed disabled:border-[#aeb8bc] disabled:text-[#8a969b]"
          disabled={isSubmitting || Boolean(registered)}
          onClick={addEditor}
          type="button"
        >
          Occurrenceを追加
        </button>
        <button
          className="h-11 rounded-md bg-[#176b57] px-7 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#829b95]"
          disabled={isSubmitting || Boolean(registered) || editors.length === 0}
          onClick={() => void handleBulkRegister()}
          type="button"
        >
          {submissionMessage ??
            (registered
              ? "一括登録済み"
              : editors.length === 0
                ? "登録対象なし"
                : `${editors.length}件を一括登録`)}
        </button>
      </div>
    </div>
  );
}

function OccurrenceEditorCard({
  index,
  editor,
  disabled,
  darwinCoreTerms,
  termsStatus,
  onLoadTerms,
  onChange,
  onDelete,
}: {
  index: number;
  editor: EditorState;
  disabled: boolean;
  darwinCoreTerms: DarwinCoreTerm[];
  termsStatus: TermsStatus;
  onLoadTerms: () => void;
  onChange: (editor: EditorState) => void;
  onDelete: () => void;
}) {
  const fileInputRef = useRef<HTMLInputElement>(null);

  function replaceRows(rows: StatementRow[], taxonLabels = editor.taxonLabels) {
    onChange({ ...editor, rows, taxonLabels });
  }

  function updateRow(id: number, field: "predicate" | "object", value: string) {
    const nextLabels = { ...editor.taxonLabels };
    if (field === "predicate" && value !== DWCIRI_TO_TAXON_URI) {
      delete nextLabels[id];
    }
    replaceRows(
      editor.rows.map((row) => (row.id === id ? { ...row, [field]: value } : row)),
      nextLabels,
    );
  }

  function updateScientificName(value: string) {
    const existing = editor.rows.find((row) => row.predicate === DWC_SCIENTIFIC_NAME_URI);
    if (existing) {
      replaceRows(
        editor.rows.map((row) =>
          row.id === existing.id ? { ...row, object: value } : row,
        ),
      );
      return;
    }

    onChange({
      ...editor,
      rows: [
        ...editor.rows,
        { id: editor.nextId, predicate: DWC_SCIENTIFIC_NAME_URI, object: value },
      ],
      nextId: editor.nextId + 1,
    });
  }

  function updateTaxonAndScientificName(taxonRowId: number, taxonUri: string, name: string) {
    let hasScientificName = false;
    const rows = editor.rows.map((row) => {
      if (row.id === taxonRowId) return { ...row, object: taxonUri };
      if (row.predicate === DWC_SCIENTIFIC_NAME_URI) {
        hasScientificName = true;
        return { ...row, object: name };
      }
      return row;
    });
    const taxonLabels = { ...editor.taxonLabels, [taxonRowId]: name };

    if (hasScientificName) {
      replaceRows(rows, taxonLabels);
      return;
    }

    onChange({
      ...editor,
      rows: [
        ...rows,
        { id: editor.nextId, predicate: DWC_SCIENTIFIC_NAME_URI, object: name },
      ],
      nextId: editor.nextId + 1,
      taxonLabels,
    });
  }

  function clearTaxonAndUpdateScientificName(taxonRowId: number, name: string) {
    let hasScientificName = false;
    const rows = editor.rows.map((row) => {
      if (row.id === taxonRowId) return { ...row, object: "" };
      if (row.predicate === DWC_SCIENTIFIC_NAME_URI) {
        hasScientificName = true;
        return { ...row, object: name };
      }
      return row;
    });
    const taxonLabels = { ...editor.taxonLabels };
    delete taxonLabels[taxonRowId];

    if (hasScientificName) {
      replaceRows(rows, taxonLabels);
      return;
    }

    onChange({
      ...editor,
      rows: [
        ...rows,
        { id: editor.nextId, predicate: DWC_SCIENTIFIC_NAME_URI, object: name },
      ],
      nextId: editor.nextId + 1,
      taxonLabels,
    });
  }

  function addRow() {
    onChange({
      ...editor,
      rows: [...editor.rows, { id: editor.nextId, predicate: "", object: "" }],
      nextId: editor.nextId + 1,
    });
  }

  function removeRow(id: number) {
    const taxonLabels = { ...editor.taxonLabels };
    delete taxonLabels[id];
    onChange({
      ...editor,
      rows: editor.rows.filter((row) => row.id !== id),
      taxonLabels,
    });
  }

  function addFiles(files: FileList | null) {
    if (!files) return;
    const knownFiles = new Set(editor.selectedFiles.map(fileIdentity));
    const added = Array.from(files).filter((file) => !knownFiles.has(fileIdentity(file)));
    onChange({ ...editor, selectedFiles: [...editor.selectedFiles, ...added] });
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  function removeFile(target: File) {
    onChange({
      ...editor,
      selectedFiles: editor.selectedFiles.filter(
        (file) => fileIdentity(file) !== fileIdentity(target),
      ),
    });
  }

  return (
    <section className="overflow-visible rounded-md border border-[#c9d2d6] bg-white">
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
        <h3 className="text-sm font-semibold text-[#344249]">Occurrence {index + 1}</h3>
        <div className="flex flex-wrap items-center gap-4">
          <label className="inline-flex items-center gap-2 text-sm font-medium text-[#526168]">
            <input
              checked={editor.isPublic}
              className="h-4 w-4 rounded border-[#b8c3c8] text-[#176b57] focus:ring-[#176b57]"
              disabled={disabled}
              onChange={(event) => onChange({ ...editor, isPublic: event.target.checked })}
              type="checkbox"
            />
            公開する
          </label>
          <button
            className="text-sm font-medium text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline"
            disabled={disabled}
            onClick={onDelete}
            type="button"
          >
            このOccurrenceを削除
          </button>
        </div>
      </div>

      <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-4 border-b border-[#d8dfe2] bg-[#f7f9fa] px-5 py-3 text-xs font-medium text-[#526168] md:grid">
        <span>項目名</span>
        <span>値</span>
        <span className="w-12" aria-hidden="true" />
      </div>

      <div className="divide-y divide-[#e4e9eb]">
        {editor.rows.map((row, rowIndex) => (
          <div
            className="grid gap-4 px-5 py-5 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end"
            key={row.id}
          >
            <div className="min-w-0">
              <span className="mb-2 block text-sm font-medium md:sr-only">項目名 {rowIndex + 1}</span>
              <PredicateCombobox
                disabled={disabled}
                onOpen={onLoadTerms}
                onSelect={(uri) => updateRow(row.id, "predicate", uri)}
                terms={darwinCoreTerms}
                termsStatus={termsStatus}
                value={row.predicate}
              />
            </div>

            {row.predicate === DWCIRI_TO_TAXON_URI ? (
              <TaxonValueCombobox
                disabled={disabled}
                displayName={editor.taxonLabels[row.id] ?? ""}
                taxonUri={row.object}
                onFreeTextChange={(name) => clearTaxonAndUpdateScientificName(row.id, name)}
                onSelect={(taxonUri, name) =>
                  updateTaxonAndScientificName(row.id, taxonUri, name)
                }
              />
            ) : (
              <ObjectValueField
                disabled={disabled}
                onChange={(value) =>
                  row.predicate === DWC_SCIENTIFIC_NAME_URI
                    ? updateScientificName(value)
                    : updateRow(row.id, "object", value)
                }
                predicate={row.predicate}
                value={row.object}
              />
            )}

            <button
              className="h-10 w-fit px-1 text-sm text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline md:w-12"
              disabled={editor.rows.length === 1 || disabled}
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
          className="text-sm font-medium text-[#176b57] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline"
          disabled={disabled}
          onClick={addRow}
          type="button"
        >
          入力行を追加
        </button>
      </div>

      <div className="border-t border-[#d8dfe2]">
        <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
          <h4 className="text-sm font-medium text-[#526168]">添付ファイル</h4>
        </div>
        <div className="px-5 py-5">
          <input
            accept=".jpg,.jpeg,.png,.webp,.mp3,.wav,.m4a,.mp4,.mov"
            className="sr-only"
            disabled={disabled}
            multiple
            onChange={(event) => addFiles(event.target.files)}
            ref={fileInputRef}
            type="file"
          />
          <button
            className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-not-allowed disabled:bg-[#eef2f3] disabled:text-[#7c898f]"
            disabled={disabled}
            onClick={() => fileInputRef.current?.click()}
            type="button"
          >
            ファイルを追加
          </button>

          {editor.selectedFiles.length > 0 ? (
            <ul className="mt-5 divide-y divide-[#e4e9eb] border-y border-[#e4e9eb]">
              {editor.selectedFiles.map((file) => (
                <li className="flex min-w-0 items-center gap-4 py-3" key={fileIdentity(file)}>
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium">{file.name}</p>
                    <p className="mt-1 text-xs text-[#65737a]">{formatFileSize(file.size)}</p>
                  </div>
                  <button
                    className="shrink-0 px-1 text-sm text-[#a23c32] hover:underline disabled:text-[#9aa5aa]"
                    disabled={disabled}
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
      </div>
    </section>
  );
}

function buildEditorState(candidate: PaperOccurrenceCandidate, index: number): EditorState {
  const rows: StatementRow[] = [];
  const taxonLabels: Record<number, string> = {};
  let nextId = 1;

  const toTaxon = candidate.toTaxon?.trim() ?? "";
  const taxonScientificName = candidate.taxonScientificName?.trim() ?? "";
  if (toTaxon && taxonScientificName) {
    const id = nextId++;
    rows.push({ id, predicate: DWCIRI_TO_TAXON_URI, object: toTaxon });
    taxonLabels[id] = taxonScientificName;
  }

  rows.push({
    id: nextId++,
    predicate: DWC_SCIENTIFIC_NAME_URI,
    object: candidate.scientificName.trim(),
  });
  rows.push({
    id: nextId++,
    predicate: DWC_DECIMAL_LONGITUDE_URI,
    object: candidate.decimalLongitude == null ? "" : String(candidate.decimalLongitude),
  });
  rows.push({
    id: nextId++,
    predicate: DWC_DECIMAL_LATITUDE_URI,
    object: candidate.decimalLatitude == null ? "" : String(candidate.decimalLatitude),
  });
  if (candidate.locality?.trim()) {
    rows.push({
      id: nextId++,
      predicate: DWC_LOCALITY_URI,
      object: candidate.locality.trim(),
    });
  }

  return {
    key: index,
    rows,
    nextId,
    selectedFiles: [],
    taxonLabels,
    isPublic: true,
  };
}

function buildEmptyEditorState(key: number): EditorState {
  return {
    key,
    rows: [{ id: 1, predicate: DWCIRI_TO_TAXON_URI, object: "" }],
    nextId: 2,
    selectedFiles: [],
    taxonLabels: {},
    isPublic: true,
  };
}

function ObjectValueField({
  disabled,
  onChange,
  predicate,
  value,
}: {
  disabled: boolean;
  onChange: (value: string) => void;
  predicate: string;
  value: string;
}) {
  const placeholder =
    predicate === DWC_DECIMAL_LONGITUDE_URI
      ? "140.106861"
      : predicate === DWC_DECIMAL_LATITUDE_URI
        ? "36.225333"
        : "値";

  return (
    <label className="min-w-0">
      <span className="mb-2 block text-sm font-medium md:sr-only">値</span>
      <input
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        type="text"
        value={value}
      />
    </label>
  );
}

function TaxonValueCombobox({
  disabled,
  displayName,
  taxonUri,
  onFreeTextChange,
  onSelect,
}: {
  disabled: boolean;
  displayName: string;
  taxonUri: string;
  onFreeTextChange: (scientificName: string) => void;
  onSelect: (taxonUri: string, scientificName: string) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(displayName || taxonUri);
  const [suggestions, setSuggestions] = useState<GbifSpeciesSuggestion[]>([]);
  const [suggestStatus, setSuggestStatus] = useState<"idle" | "loading" | "loaded" | "error">("idle");

  useEffect(() => {
    setQuery(displayName || taxonUri);
  }, [displayName, taxonUri]);

  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 3 || isAbsoluteHttpUri(trimmed)) {
      setSuggestions([]);
      setSuggestStatus("idle");
      return;
    }

    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      setSuggestStatus("loading");
      void fetch(`${GBIF_SUGGEST_ENDPOINT}?q=${encodeURIComponent(trimmed)}`, {
        signal: controller.signal,
      })
        .then(async (response) => {
          if (!response.ok) throw new Error("gbif suggest failed");
          const items = (await response.json()) as GbifSpeciesSuggestion[];
          setSuggestions(items.slice(0, 10));
          setSuggestStatus("loaded");
        })
        .catch(() => {
          if (controller.signal.aborted) return;
          setSuggestions([]);
          setSuggestStatus("error");
        });
    }, GBIF_SUGGEST_DEBOUNCE_MS);

    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [query]);

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredSuggestions = suggestions.filter(
    (item) =>
      !normalizedQuery ||
      item.scientificName.toLocaleLowerCase().includes(normalizedQuery) ||
      (item.canonicalName ?? "").toLocaleLowerCase().includes(normalizedQuery),
  );

  return (
    <div className="relative min-w-0">
      <input
        aria-autocomplete="list"
        aria-expanded={isOpen}
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onBlur={() => window.setTimeout(() => setIsOpen(false), 120)}
        onChange={(event) => {
          const next = event.target.value;
          setQuery(next);
          onFreeTextChange(next);
          setIsOpen(true);
        }}
        onFocus={() => setIsOpen(true)}
        placeholder="分類"
        role="combobox"
        type="text"
        value={query}
      />
      {taxonUri ? (
        <p className="mt-1 break-all text-xs text-[#65737a]">{taxonUri}</p>
      ) : null}
      {isOpen ? (
        <div className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg" role="listbox">
          {suggestStatus === "loading" ? <p className="px-3 py-2 text-sm text-[#65737a]">GBIF候補を読み込み中</p> : null}
          {suggestStatus === "error" ? <p className="px-3 py-2 text-sm text-[#a23c32]">GBIF候補を取得できませんでした</p> : null}
          {suggestStatus === "loaded" && filteredSuggestions.length === 0 ? <p className="px-3 py-2 text-sm text-[#65737a]">一致する候補はありません</p> : null}
          {filteredSuggestions.map((item) => (
            <button
              className="block w-full px-3 py-2 text-left hover:bg-[#eef2f3] focus:bg-[#eef2f3] focus:outline-none"
              key={item.key}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                const label = item.scientificName || item.canonicalName || String(item.key);
                const speciesUri = `${GBIF_SPECIES_URI_PREFIX}${item.key}`;
                setQuery(label);
                onSelect(speciesUri, label);
                setIsOpen(false);
              }}
              role="option"
              type="button"
            >
              <span className="block text-sm font-medium">{item.scientificName}</span>
              <span className="block truncate text-xs text-[#65737a]">GBIF key: {item.key}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function PredicateCombobox({
  disabled,
  onOpen,
  onSelect,
  terms,
  termsStatus,
  value,
}: {
  disabled: boolean;
  onOpen: () => void;
  onSelect: (uri: string) => void;
  terms: DarwinCoreTerm[];
  termsStatus: TermsStatus;
  value: string;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(() => predicateLabelForUri(value));

  useEffect(() => {
    setQuery(predicateLabelForUri(value, terms));
  }, [terms, value]);

  const sortedTerms = [...terms].sort((left, right) => {
    const byName = left.local_name.localeCompare(right.local_name, "ja", { sensitivity: "base" });
    return byName !== 0 ? byName : left.uri.localeCompare(right.uri, "en", { sensitivity: "base" });
  });
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredTerms = sortedTerms.filter(
    (term) =>
      !normalizedQuery ||
      term.local_name.toLocaleLowerCase().includes(normalizedQuery) ||
      term.uri.toLocaleLowerCase().includes(normalizedQuery),
  );

  return (
    <div className="relative">
      <input
        aria-autocomplete="list"
        aria-expanded={isOpen}
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onBlur={() => window.setTimeout(() => setIsOpen(false), 120)}
        onChange={(event) => {
          const next = event.target.value;
          const normalizedNext = next.trim();
          setQuery(next);
          const exact = sortedTerms.find(
            (term) =>
              term.local_name.toLocaleLowerCase() === normalizedNext.toLocaleLowerCase() ||
              term.uri === normalizedNext,
          );
          if (exact) onSelect(exact.uri);
          else if (isAbsoluteHttpUri(normalizedNext)) onSelect(normalizedNext);
          else onSelect("");
          setIsOpen(true);
        }}
        onFocus={() => {
          setIsOpen(true);
          onOpen();
        }}
        placeholder="項目名"
        role="combobox"
        type="text"
        value={query}
      />
      {isOpen ? (
        <div className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg" role="listbox">
          {termsStatus === "loading" ? <p className="px-3 py-2 text-sm text-[#65737a]">読み込み中</p> : null}
          {termsStatus === "error" ? <p className="px-3 py-2 text-sm text-[#a23c32]">候補を取得できませんでした</p> : null}
          {termsStatus === "loaded" && filteredTerms.length === 0 ? <p className="px-3 py-2 text-sm text-[#65737a]">一致する項目はありません</p> : null}
          {filteredTerms.map((term) => (
            <button
              className="block w-full px-3 py-2 text-left hover:bg-[#eef2f3] focus:bg-[#eef2f3] focus:outline-none"
              key={term.uri}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                onSelect(term.uri);
                setQuery(predicateLabelForUri(term.uri, terms));
                setIsOpen(false);
              }}
              role="option"
              type="button"
            >
              <span className="block text-sm font-medium">{term.local_name}</span>
              <span className="block truncate text-xs text-[#65737a]">{term.uri}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function predicateLabelForUri(value: string, terms: DarwinCoreTerm[] = []): string {
  if (value === DWCIRI_TO_TAXON_URI) return DWCIRI_TO_TAXON_LABEL;
  if (value === DWC_SCIENTIFIC_NAME_URI) return DWC_SCIENTIFIC_NAME_LABEL;
  if (value === DWC_DECIMAL_LONGITUDE_URI) return DWC_DECIMAL_LONGITUDE_LABEL;
  if (value === DWC_DECIMAL_LATITUDE_URI) return DWC_DECIMAL_LATITUDE_LABEL;
  if (value === DWC_LOCALITY_URI) return DWC_LOCALITY_LABEL;
  return terms.find((term) => term.uri === value)?.local_name ?? value;
}

function validateStatementRows(rows: StatementRow[]): StatementRow[] {
  const statements: StatementRow[] = [];
  const optionalBlankPredicates = new Set([
    DWCIRI_TO_TAXON_URI,
    DWC_DECIMAL_LONGITUDE_URI,
    DWC_DECIMAL_LATITUDE_URI,
  ]);

  for (const row of rows) {
    const predicate = row.predicate.trim();
    const object = row.object.trim();
    if (!predicate && !object) continue;
    if (predicate && !object && optionalBlankPredicates.has(predicate)) continue;
    if (!predicate || !object) throw new Error("項目名と値を両方入力してください");
    if (!isAbsoluteHttpUri(predicate) || hasUnsafeIriCharacter(predicate)) {
      throw new Error("項目名には有効な絶対URIを入力してください");
    }
    if (isAbsoluteHttpUri(object) && hasUnsafeIriCharacter(object)) {
      throw new Error("値のURIに使用できない文字が含まれています");
    }
    if (predicate === DWCIRI_TO_TAXON_URI && !isAbsoluteHttpUri(object)) {
      throw new Error("分類にはGBIF等の有効なURIを指定してください");
    }
    statements.push({ ...row, predicate, object });
  }
  return statements;
}

function validateSelectedFiles(files: File[]) {
  if (files.some((file) => file.size > MAX_MEDIA_SIZE_BYTES)) {
    throw new Error("添付ファイルは1ファイル1000MB以下にしてください");
  }
}

function buildOccurrenceNQuads(
  statements: StatementRow[],
  mediaUris: string[],
  accessRightsUri: string,
): string {
  const lines = statements.map((statement) => {
    const object = isAbsoluteHttpUri(statement.object)
      ? `<${statement.object}>`
      : `\"${escapeRdfLiteral(statement.object)}\"`;
    return `_:occurrence <${statement.predicate}> ${object} <${OCCURRENCE_GRAPH_URI}> .`;
  });
  lines.push(`_:occurrence <${ACCESS_RIGHTS_PREDICATE_URI}> <${accessRightsUri}> <${OCCURRENCE_GRAPH_URI}> .`);
  for (const mediaUri of mediaUris) {
    lines.push(`_:occurrence <${ASSOCIATED_MEDIA_PREDICATE_URI}> <${mediaUri}> <${OCCURRENCE_GRAPH_URI}> .`);
  }
  return `${lines.join("\n")}\n`;
}

function isAbsoluteHttpUri(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function hasUnsafeIriCharacter(value: string): boolean {
  return /[<>"{}|^\x60\\\s]/u.test(value);
}

function escapeRdfLiteral(value: string): string {
  return value
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"')
    .replaceAll("\n", "\\n")
    .replaceAll("\r", "\\r")
    .replaceAll("\t", "\\t");
}

function registrationErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) return "一括登録処理に失敗しました";
  if (error.status === 400) return "いずれかのOccurrenceのRDFデータが不正です";
  if (error.status === 401) return "ログインセッションが無効です";
  if (error.status === 403) return "添付ファイルをOccurrenceへ関連付ける権限がありません";
  if (error.status === 404) return "元論文が見つかりません";
  if (error.status === 502) return "Fusekiへの保存中に失敗しました";
  return `一括登録処理に失敗しました（HTTP ${error.status}）`;
}

function fileIdentity(file: File): string {
  return `${file.name}:${file.size}:${file.lastModified}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
