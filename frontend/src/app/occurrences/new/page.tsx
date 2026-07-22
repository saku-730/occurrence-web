"use client";

import { useEffect, useRef, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

const OCCURRENCE_GRAPH_URI =
  "https://bio-database.net/graphs/occurrences";
const ASSOCIATED_MEDIA_PREDICATE_URI =
  "http://rs.tdwg.org/ac/terms/associatedMedia";
const ACCESS_RIGHTS_PREDICATE_URI = "http://purl.org/dc/terms/accessRights";
const PUBLIC_ACCESS_RIGHTS_URI =
  "https://bio-database.net/terms/access-rights/public";
const PRIVATE_ACCESS_RIGHTS_URI =
  "https://bio-database.net/terms/access-rights/private";
const MAX_MEDIA_SIZE_BYTES = 1000 * 1024 * 1024;
const DWCIRI_TO_TAXON_URI = "http://rs.tdwg.org/dwc/iri/toTaxon";
const DWCIRI_TO_TAXON_LABEL = "分類";
const DWC_SCIENTIFIC_NAME_URI = "http://rs.tdwg.org/dwc/terms/scientificName";
const GBIF_SUGGEST_ENDPOINT = "https://api.gbif.org/v1/species/suggest";
const GBIF_SPECIES_URI_PREFIX = "https://www.gbif.org/species/";
const GBIF_SUGGEST_DEBOUNCE_MS = 300;

interface DarwinCoreTerm {
  uri: string;
  local_name: string;
}

interface StatementRow {
  id: number;
  predicate: string;
  object: string;
}

interface UploadMediaResponse {
  media_id: string;
  media_uri: string;
  bucket: string;
  object_key: string;
  content_type: string;
  size_bytes: number;
  original_filename: string | null;
}

interface CreateOccurrenceResponse {
  occurrence_id: string;
  occurrence_uri: string;
}

interface GbifSpeciesSuggestion {
  key: number;
  scientificName: string;
  canonicalName?: string;
  rank?: string;
}

type AuthStatus =
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "error";

const initialRows: StatementRow[] = [
  { id: 1, predicate: DWCIRI_TO_TAXON_URI, object: "" },
  { id: 2, predicate: "", object: "" },
];

export default function NewOccurrencePage() {
  const [rows, setRows] = useState(initialRows);
  const nextId = useRef(3);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [darwinCoreTerms, setDarwinCoreTerms] = useState<DarwinCoreTerm[]>([]);
  const [termsStatus, setTermsStatus] = useState<"idle" | "loading" | "loaded" | "error">("idle");
  const [selectedFiles, setSelectedFiles] = useState<File[]>([]);
  const [taxonScientificNames, setTaxonScientificNames] = useState<Record<number, string>>({});
  const [isPublic, setIsPublic] = useState(true);
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submissionMessage, setSubmissionMessage] = useState<string | null>(
    null,
  );
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [createdOccurrence, setCreatedOccurrence] =
    useState<CreateOccurrenceResponse | null>(null);

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

    if (field === "predicate" && value.trim() !== DWCIRI_TO_TAXON_URI) {
      setTaxonScientificNames((current) => {
        if (!(id in current)) return current;
        const next = { ...current };
        delete next[id];
        return next;
      });
    }
  }

  async function loadDarwinCoreTerms() {
    if (termsStatus !== "idle") return;
    setTermsStatus("loading");
    try {
      const terms = await apiFetch<DarwinCoreTerm[]>("/vocabularies/darwin-core");
      // backendから返った語彙候補は制限せず表示し、toTaxonだけは「分類」候補へ一本化する。
      const visibleTerms = terms.filter((term) =>
        term.uri !== DWCIRI_TO_TAXON_URI,
      );
      // 分類は UI 上の特別候補としてだけ追加し、保存値は dwciri:toTaxon に固定する。
      setDarwinCoreTerms([
        { uri: DWCIRI_TO_TAXON_URI, local_name: DWCIRI_TO_TAXON_LABEL },
        ...visibleTerms,
      ]);
      setTermsStatus("loaded");
    } catch {
      setTermsStatus("error");
    }
  }

  function addRow() {
    setRows((currentRows) => [
      ...currentRows,
      { id: nextId.current++, predicate: "", object: "" },
    ]);
  }

  function removeRow(id: number) {
    setRows((currentRows) => currentRows.filter((row) => row.id !== id));
    setTaxonScientificNames((current) => {
      if (!(id in current)) return current;
      const next = { ...current };
      delete next[id];
      return next;
    });
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

    // Clearing the native input allows a removed file to be selected again.
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  function removeFile(target: File) {
    setSelectedFiles((currentFiles) =>
      currentFiles.filter((file) => fileIdentity(file) !== fileIdentity(target)),
    );
  }

  async function submitOccurrence() {
    setErrorMessage(null);
    setCreatedOccurrence(null);

    let statements: StatementRow[];
    try {
      statements = validateStatementRows(rows);
      statements = normalizeTaxonStatements(statements, taxonScientificNames);
      validateSelectedFiles(selectedFiles);
    } catch (error) {
      setErrorMessage(
        error instanceof Error ? error.message : "入力内容を確認してください",
      );
      return;
    }

    if (statements.length === 0 && selectedFiles.length === 0) {
      setErrorMessage("項目または添付ファイルを1つ以上入力してください");
      return;
    }

    setIsSubmitting(true);

    try {
      const mediaUris: string[] = [];
      for (const [index, file] of selectedFiles.entries()) {
        setSubmissionMessage(
          `ファイルをアップロードしています (${index + 1}/${selectedFiles.length})`,
        );
        const formData = new FormData();
        formData.append("file", file, file.name);

        const uploaded = await apiFetch<UploadMediaResponse>("/media", {
          method: "POST",
          body: formData,
        });
        mediaUris.push(uploaded.media_uri);
      }

      setSubmissionMessage("オカレンスデータを登録しています");
      const accessRightsUri = isPublic ? PUBLIC_ACCESS_RIGHTS_URI : PRIVATE_ACCESS_RIGHTS_URI;
      const nquads = buildOccurrenceNQuads(statements, mediaUris, accessRightsUri);
      const created = await apiFetch<CreateOccurrenceResponse>("/occurrences", {
        method: "POST",
        headers: { "Content-Type": "application/n-quads" },
        body: nquads,
      });

      setCreatedOccurrence(created);
      setSubmissionMessage(null);
      setRows(initialRows.map((row) => ({ ...row })));
      setSelectedFiles([]);
      setTaxonScientificNames({});
      setIsPublic(true);
      nextId.current = 3;
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        setAuthStatus("unauthenticated");
      } else {
        setErrorMessage(registrationErrorMessage(error));
      }
      setSubmissionMessage(null);
    } finally {
      setIsSubmitting(false);
    }
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

        <section className="overflow-visible rounded-md border border-[#d8dfe2] bg-white">
          <div className="flex items-center justify-end border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
            <label className="inline-flex items-center gap-2 text-sm font-medium text-[#526168]">
              <input
                checked={isPublic}
                className="h-4 w-4 rounded border-[#b8c3c8] text-[#176b57] focus:ring-[#176b57]"
                disabled={isSubmitting}
                onChange={(event) => setIsPublic(event.target.checked)}
                type="checkbox"
              />
              公開する
            </label>
          </div>
          <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-4 border-b border-[#d8dfe2] bg-[#f7f9fa] px-5 py-3 text-xs font-medium text-[#526168] md:grid">
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
                <div className="min-w-0">
                  <span className="mb-2 block text-sm font-medium md:sr-only">
                    項目名 {index + 1}
                  </span>
                  <PredicateCombobox
                    disabled={isSubmitting}
                    onOpen={() => void loadDarwinCoreTerms()}
                    onSelect={(uri) => updateRow(row.id, "predicate", uri)}
                    terms={darwinCoreTerms}
                    termsStatus={termsStatus}
                    value={row.predicate}
                  />
                </div>

                <ObjectValueField
                  disabled={isSubmitting}
                  onChange={(value) => updateRow(row.id, "object", value)}
                  onTaxonSelect={(scientificName) =>
                    setTaxonScientificNames((current) => {
                      if (!scientificName.trim()) {
                        if (!(row.id in current)) return current;
                        const next = { ...current };
                        delete next[row.id];
                        return next;
                      }

                      return {
                        ...current,
                        [row.id]: scientificName,
                      };
                    })
                  }
                  predicate={row.predicate}
                  value={row.object}
                />

                <button
                  className="h-10 w-fit px-1 text-sm text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline md:w-12"
                  disabled={rows.length === 1 || isSubmitting}
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
              disabled={isSubmitting}
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
              disabled={isSubmitting}
              multiple
              onChange={(event) => addFiles(event.target.files)}
              ref={fileInputRef}
              type="file"
            />
            <button
              className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-not-allowed disabled:bg-[#eef2f3] disabled:text-[#7c898f]"
              disabled={isSubmitting}
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
                      className="shrink-0 px-1 text-sm text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline"
                      disabled={isSubmitting}
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

        {errorMessage ? (
          <p className="mt-5 text-sm text-[#a23c32]" role="alert">
            {errorMessage}
          </p>
        ) : null}

        {createdOccurrence ? (
          <section
            className="mt-6 rounded-md border border-[#9dbeb4] bg-white px-5 py-4"
            aria-live="polite"
          >
            <p className="font-medium">データを登録しました</p>
            <p className="mt-2 break-all text-sm text-[#526168]">
              {createdOccurrence.occurrence_uri}
            </p>
          </section>
        ) : null}

        <div className="mt-6 flex justify-end">
          <button
            className="h-10 rounded-md bg-[#176b57] px-6 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-wait disabled:bg-[#829b95]"
            disabled={isSubmitting}
            onClick={() => void submitOccurrence()}
            type="button"
          >
            {submissionMessage ?? "登録"}
          </button>
        </div>
      </main>
    </div>
  );
}

type PredicateComboboxProps = {
  disabled: boolean;
  onOpen: () => void;
  onSelect: (uri: string) => void;
  terms: DarwinCoreTerm[];
  termsStatus: "idle" | "loading" | "loaded" | "error";
  value: string;
};

function ObjectValueField({
  disabled,
  onChange,
  onTaxonSelect,
  predicate,
  value,
}: {
  disabled: boolean;
  onChange: (value: string) => void;
  onTaxonSelect: (scientificName: string) => void;
  predicate: string;
  value: string;
}) {
  if (predicate === DWCIRI_TO_TAXON_URI) {
    return (
      <TaxonValueCombobox
        disabled={disabled}
        onChange={onChange}
        onSelectScientificName={onTaxonSelect}
        value={value}
      />
    );
  }

  return (
    <label className="min-w-0">
      <span className="mb-2 block text-sm font-medium md:sr-only">値</span>
      <input
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        placeholder="値"
        type="text"
        value={value}
      />
    </label>
  );
}

function TaxonValueCombobox({
  disabled,
  onChange,
  onSelectScientificName,
  value,
}: {
  disabled: boolean;
  onChange: (value: string) => void;
  onSelectScientificName: (scientificName: string) => void;
  value: string;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(value);
  const [suggestions, setSuggestions] = useState<GbifSpeciesSuggestion[]>([]);
  const [suggestStatus, setSuggestStatus] = useState<"idle" | "loading" | "loaded" | "error">("idle");
  const lastSelectedLabel = useRef("");
  const lastSelectedValue = useRef("");

  useEffect(() => {
    if (value === lastSelectedValue.current && lastSelectedLabel.current) {
      setQuery(lastSelectedLabel.current);
      return;
    }

    setQuery(value);
  }, [value]);

  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 3) {
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
          if (!response.ok) {
            throw new Error("gbif suggest failed");
          }

          const items = (await response.json()) as GbifSpeciesSuggestion[];
          setSuggestions(items.slice(0, 10));
          setSuggestStatus("loaded");
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) {
            return;
          }
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
  const filteredSuggestions = suggestions.filter((item) => {
    if (!normalizedQuery) return true;
    return (
      item.scientificName.toLocaleLowerCase().includes(normalizedQuery) ||
      (item.canonicalName ?? "").toLocaleLowerCase().includes(normalizedQuery)
    );
  });

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
          onSelectScientificName("");
          onChange(next);
          setIsOpen(true);
        }}
        onFocus={() => {
          setIsOpen(true);
        }}
        placeholder="分類"
        role="combobox"
        type="text"
        value={query}
      />
      {isOpen ? (
        <div
          className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg"
          role="listbox"
        >
          {suggestStatus === "loading" ? (
            <p className="px-3 py-2 text-sm text-[#65737a]">GBIF候補を読み込み中</p>
          ) : null}
          {suggestStatus === "error" ? (
            <p className="px-3 py-2 text-sm text-[#a23c32]">GBIF候補を取得できませんでした</p>
          ) : null}
          {suggestStatus === "loaded" && filteredSuggestions.length === 0 ? (
            <p className="px-3 py-2 text-sm text-[#65737a]">一致する候補はありません</p>
          ) : null}
          {filteredSuggestions.map((item) => (
            <button
              className="block w-full px-3 py-2 text-left hover:bg-[#eef2f3] focus:bg-[#eef2f3] focus:outline-none"
              key={item.key}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                const label = item.scientificName || item.canonicalName || String(item.key);
                const speciesUri = `${GBIF_SPECIES_URI_PREFIX}${item.key}`;
                lastSelectedLabel.current = label;
                lastSelectedValue.current = speciesUri;
                onSelectScientificName(label);
                onChange(speciesUri);
                setQuery(label);
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
}: PredicateComboboxProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(() => predicateLabelForUri(value));

  useEffect(() => {
    setQuery(predicateLabelForUri(value, terms));
  }, [terms, value]);

  const sortedTerms = [...terms].sort((left, right) => {
    const byName = left.local_name.localeCompare(right.local_name, "ja", {
      sensitivity: "base",
    });

    if (byName !== 0) return byName;
    return left.uri.localeCompare(right.uri, "en", { sensitivity: "base" });
  });

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredTerms = sortedTerms.filter((term) => {
    if (!normalizedQuery) return true;
    return (
      term.local_name.toLocaleLowerCase().includes(normalizedQuery) ||
      term.uri.toLocaleLowerCase().includes(normalizedQuery)
    );
  });

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

          const exactMatch = sortedTerms.find(
            (term) =>
              term.local_name.toLocaleLowerCase() === normalizedNext.toLocaleLowerCase() ||
              term.uri === normalizedNext,
          );

          if (exactMatch) {
            onSelect(exactMatch.uri);
          } else if (isAbsoluteHttpUri(normalizedNext)) {
            onSelect(normalizedNext);
          } else {
            onSelect("");
          }

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
        <div
          className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg"
          role="listbox"
        >
          {termsStatus === "loading" ? (
            <p className="px-3 py-2 text-sm text-[#65737a]">読み込み中</p>
          ) : null}
          {termsStatus === "error" ? (
            <p className="px-3 py-2 text-sm text-[#a23c32]">候補を取得できませんでした</p>
          ) : null}
          {termsStatus === "loaded" && filteredTerms.length === 0 ? (
            <p className="px-3 py-2 text-sm text-[#65737a]">一致する項目はありません</p>
          ) : null}
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
  if (value === DWCIRI_TO_TAXON_URI) {
    return DWCIRI_TO_TAXON_LABEL;
  }

  return terms.find((term) => term.uri === value)?.local_name ?? value;
}

function validateStatementRows(rows: StatementRow[]): StatementRow[] {
  const statements: StatementRow[] = [];

  for (const row of rows) {
    const predicate = row.predicate.trim();
    const object = row.object.trim();

    if (!predicate && !object) continue;
    if (!predicate || !object) {
      throw new Error("項目名と値を両方入力してください");
    }
    if (!isAbsoluteHttpUri(predicate) || hasUnsafeIriCharacter(predicate)) {
      throw new Error("項目名には有効な絶対URIを入力してください");
    }
    if (isAbsoluteHttpUri(object) && hasUnsafeIriCharacter(object)) {
      throw new Error("値のURIに使用できない文字が含まれています");
    }

    statements.push({ ...row, predicate, object });
  }

  return statements;
}

// 分類の入力は、URIならtoTaxon、任意テキストならscientificNameとして保存する。
// テキストをtoTaxonへ保存するとIRI専用語彙の意味を壊すため、送信直前に述語を正規化する。
function normalizeTaxonStatements(
  statements: StatementRow[],
  scientificNamesByRowId: Record<number, string>,
): StatementRow[] {
  const normalizedStatements: StatementRow[] = [];
  const generatedScientificNames: StatementRow[] = [];

  for (const row of statements) {
    if (row.predicate !== DWCIRI_TO_TAXON_URI) {
      normalizedStatements.push(row);
      continue;
    }

    if (!isAbsoluteHttpUri(row.object)) {
      // 手入力の分類名はtoTaxonを作らず、学名リテラルとして保存する。
      normalizedStatements.push({
        ...row,
        predicate: DWC_SCIENTIFIC_NAME_URI,
      });
      continue;
    }

    normalizedStatements.push(row);

    // GBIF候補を選んだURIには、候補表示から得た学名を非表示で補完する。
    const scientificName = scientificNamesByRowId[row.id];
    if (scientificName?.trim()) {
      generatedScientificNames.push({
        id: statements.length + generatedScientificNames.length + 1,
        predicate: DWC_SCIENTIFIC_NAME_URI,
        object: scientificName.trim(),
      });
    }
  }

  return [...normalizedStatements, ...generatedScientificNames];
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
      : `"${escapeRdfLiteral(statement.object)}"`;

    return `_:occurrence <${statement.predicate}> ${object} <${OCCURRENCE_GRAPH_URI}> .`;
  });

  lines.push(
    `_:occurrence <${ACCESS_RIGHTS_PREDICATE_URI}> <${accessRightsUri}> <${OCCURRENCE_GRAPH_URI}> .`,
  );

  for (const mediaUri of mediaUris) {
    lines.push(
      `_:occurrence <${ASSOCIATED_MEDIA_PREDICATE_URI}> <${mediaUri}> <${OCCURRENCE_GRAPH_URI}> .`,
    );
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
  return /[<>"{}|^`\\\s]/u.test(value);
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
  if (!(error instanceof ApiError)) {
    return "登録処理に失敗しました";
  }

  if (error.status === 400) {
    return "入力したRDFデータが不正です";
  }
  if (error.status === 403) {
    return "添付ファイルをこのデータへ関連付ける権限がありません";
  }
  if (error.status === 413) {
    return "添付ファイルのサイズが上限を超えています";
  }
  if (error.status === 415) {
    return "対応していないファイル形式です";
  }
  if (error.status === 502) {
    return "データ保存先との通信に失敗しました";
  }

  return "登録処理に失敗しました";
}

function fileIdentity(file: File): string {
  return `${file.name}:${file.size}:${file.lastModified}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
