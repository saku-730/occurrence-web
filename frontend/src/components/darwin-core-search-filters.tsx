"use client";

import { useEffect, useRef, useState } from "react";

import { apiFetch } from "@/lib/api";

const DCTERMS_CREATOR = "http://purl.org/dc/terms/creator";
const DCTERMS_CREATED = "http://purl.org/dc/terms/created";
const DCTERMS_MODIFIED = "http://purl.org/dc/terms/modified";
const DWCIRI_TO_TAXON = "http://rs.tdwg.org/dwc/iri/toTaxon";
const SCIENTIFIC_NAME = "http://rs.tdwg.org/dwc/terms/scientificName";
const SOURCE_PAPER = "https://bio-database.net/terms/sourcePaper";
const USER_URI_BASE = "https://bio-database.net/users/";

export interface DarwinCoreSearchFilter {
  predicate: string;
  value: string;
  value_type: "literal" | "uri";
  match: "exact";
  // UI-only label. The backend contract is built by activeDarwinCoreSearchFilters and
  // never includes this field.
  display_value?: string;
}

interface DarwinCoreTerm {
  uri: string;
  local_name: string;
}

interface SearchTerm extends DarwinCoreTerm {
  source: "system" | "darwin-core";
}

interface UserSearchItem {
  user_id: string;
  user_name: string;
}

const SYSTEM_SEARCH_TERMS: SearchTerm[] = [
  {
    uri: DCTERMS_CREATOR,
    local_name: "作成者",
    source: "system",
  },
  {
    uri: DCTERMS_CREATED,
    local_name: "データ作成日",
    source: "system",
  },
  {
    uri: DCTERMS_MODIFIED,
    local_name: "データ更新日",
    source: "system",
  },
  {
    uri: SOURCE_PAPER,
    local_name: "sourcePaper",
    source: "system",
  },
];

const SUPPLEMENTAL_DARWIN_CORE_TERMS: DarwinCoreTerm[] = [
  {
    uri: DWCIRI_TO_TAXON,
    local_name: "toTaxon",
  },
];

export function emptyDarwinCoreSearchFilter(
  predicate = SCIENTIFIC_NAME,
): DarwinCoreSearchFilter {
  return {
    predicate,
    value: "",
    value_type: "literal",
    match: "exact",
  };
}

export function activeDarwinCoreSearchFilters(
  filters: DarwinCoreSearchFilter[],
): DarwinCoreSearchFilter[] {
  return filters
    .map((filter) => {
      const predicate = filter.predicate.trim();
      const value = filter.value.trim();
      return {
        predicate,
        value,
        value_type:
          predicate === DCTERMS_CREATOR || predicate === SOURCE_PAPER
            ? ("uri" as const)
            : inferSearchValueType(value),
        match: filter.match,
      };
    })
    .filter((filter) => filter.predicate.length > 0 && filter.value.length > 0);
}

export function DarwinCoreSearchFilters({
  filters,
  onChange,
  disabled = false,
}: {
  filters: DarwinCoreSearchFilter[];
  onChange: (filters: DarwinCoreSearchFilter[]) => void;
  disabled?: boolean;
}) {
  const [terms, setTerms] = useState<DarwinCoreTerm[]>([]);
  const [termLoadFailed, setTermLoadFailed] = useState(false);
  const [showDefaultToTaxon, setShowDefaultToTaxon] = useState(
    isBlankScientificNameDefault(filters),
  );
  const previousFiltersRef = useRef(filters);
  const suppressDefaultToTaxonRef = useRef(false);

  useEffect(() => {
    let active = true;

    apiFetch<DarwinCoreTerm[]>("/vocabularies/darwin-core")
      .then((response) => {
        if (!active) return;
        setTerms(response);
        setTermLoadFailed(false);
      })
      .catch(() => {
        if (active) setTermLoadFailed(true);
      });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (previousFiltersRef.current === filters) return;

    if (suppressDefaultToTaxonRef.current) {
      suppressDefaultToTaxonRef.current = false;
    } else if (isBlankScientificNameDefault(filters)) {
      setShowDefaultToTaxon(true);
    }

    previousFiltersRef.current = filters;
  }, [filters]);

  const darwinCoreTerms = [
    ...SUPPLEMENTAL_DARWIN_CORE_TERMS,
    ...terms.filter(
      (term) => !SUPPLEMENTAL_DARWIN_CORE_TERMS.some((supplemental) => supplemental.uri === term.uri),
    ),
  ];
  const searchTerms: SearchTerm[] = [
    ...SYSTEM_SEARCH_TERMS,
    ...darwinCoreTerms.map((term) => ({ ...term, source: "darwin-core" as const })),
  ];
  const hasToTaxonFilter = filters.some((filter) => filter.predicate === DWCIRI_TO_TAXON);
  const visibleFilters =
    showDefaultToTaxon && isBlankScientificNameDefault(filters) && !hasToTaxonFilter
      ? [...filters, emptyDarwinCoreSearchFilter(DWCIRI_TO_TAXON)]
      : filters;

  function replaceFilter(index: number, next: DarwinCoreSearchFilter) {
    if (index >= filters.length) {
      setShowDefaultToTaxon(false);
      if (next.predicate.length === 0) return;
      onChange([...filters, next]);
      return;
    }

    onChange(filters.map((filter, currentIndex) => (currentIndex === index ? next : filter)));
  }

  function removeFilter(index: number) {
    if (index >= filters.length) {
      setShowDefaultToTaxon(false);
      return;
    }

    if (filters[index]?.predicate === DWCIRI_TO_TAXON) {
      suppressDefaultToTaxonRef.current = true;
      setShowDefaultToTaxon(false);
    }
    onChange(filters.filter((_, currentIndex) => currentIndex !== index));
  }

  return (
    <div className="space-y-3">
      {visibleFilters.map((filter, index) => {
        const selectedTerm = searchTerms.find((term) => term.uri === filter.predicate);
        const customPredicate = selectedTerm ? "" : filter.predicate;

        return (
          <div
            className="grid gap-3 rounded-md border border-[#d8dfe2] bg-white p-3 md:grid-cols-[minmax(16rem,1.3fr)_minmax(12rem,1fr)_auto] md:items-end"
            key={`${filter.predicate || "custom"}-${index}`}
          >
            <div className="min-w-0">
              <label>
                <span className="mb-1 block text-xs font-medium text-[#526168]">検索項目</span>
                <select
                  className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                  disabled={disabled}
                  onChange={(event) =>
                    replaceFilter(index, {
                      ...filter,
                      predicate: event.target.value,
                      value: "",
                      value_type: "literal",
                      display_value: undefined,
                    })
                  }
                  value={selectedTerm?.uri ?? ""}
                >
                  <option value="">項目を選択</option>
                  <optgroup label="Bio-Database">
                    {SYSTEM_SEARCH_TERMS.map((term) => (
                      <option key={term.uri} value={term.uri}>
                        {term.local_name}
                      </option>
                    ))}
                  </optgroup>
                  <optgroup label="Darwin Core">
                    {darwinCoreTerms.map((term) => (
                      <option key={term.uri} value={term.uri}>
                        {term.local_name}
                      </option>
                    ))}
                  </optgroup>
                </select>
              </label>

              {selectedTerm ? (
                <span className="mt-1 block truncate text-[11px] text-[#7a878c]" title={selectedTerm.uri}>
                  {selectedTerm.source === "system" ? "内部項目" : "IRI"}: {selectedTerm.uri}
                </span>
              ) : null}

              <details className="mt-2 text-xs" open={customPredicate.length > 0}>
                <summary className="cursor-pointer text-[#65747a] hover:text-[#176b57]">
                  候補にないIRIを直接指定
                </summary>
                <input
                  className="mt-2 h-9 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-xs outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                  disabled={disabled}
                  onChange={(event) =>
                    replaceFilter(index, {
                      ...filter,
                      predicate: event.target.value,
                      value: "",
                      display_value: undefined,
                    })
                  }
                  placeholder="http://rs.tdwg.org/dwc/terms/..."
                  type="text"
                  value={customPredicate}
                />
              </details>
            </div>

            {filter.predicate === DCTERMS_CREATOR ? (
              <CreatorSearchInput
                disabled={disabled}
                filter={filter}
                onChange={(next) => replaceFilter(index, next)}
              />
            ) : (
              <label className="min-w-0">
                <span className="mb-1 block text-xs font-medium text-[#526168]">検索値</span>
                <input
                  className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                  disabled={disabled}
                  onChange={(event) => {
                    const value = event.target.value;
                    replaceFilter(index, {
                      ...filter,
                      value,
                      value_type: inferSearchValueType(value),
                    });
                  }}
                  placeholder={searchValuePlaceholder(filter.predicate)}
                  type="text"
                  value={filter.value}
                />
              </label>
            )}

            <button
              className="h-10 rounded-md border border-[#b8c3c8] px-3 text-sm hover:bg-[#eef2f3] disabled:cursor-not-allowed disabled:text-[#9aa5a9]"
              disabled={disabled}
              onClick={() => removeFilter(index)}
              type="button"
            >
              削除
            </button>
          </div>
        );
      })}

      <div className="flex flex-wrap items-center gap-3">
        <button
          className="h-9 rounded-md border border-[#176b57] bg-white px-3 text-sm font-medium text-[#176b57] hover:bg-[#e8f2ef] disabled:cursor-not-allowed disabled:border-[#b8c3c8] disabled:text-[#829b95]"
          disabled={disabled}
          onClick={() => onChange([...filters, emptyDarwinCoreSearchFilter("")])}
          type="button"
        >
          条件を追加
        </button>
        <span className="text-xs text-[#65747a]">複数条件はAND検索です。</span>
        {termLoadFailed ? (
          <span className="text-xs text-[#a53d32]">
            Darwin Core候補を取得できませんでした。Bio-Database項目とIRI直接入力は利用できます。
          </span>
        ) : null}
      </div>
    </div>
  );
}

function CreatorSearchInput({
  filter,
  onChange,
  disabled,
}: {
  filter: DarwinCoreSearchFilter;
  onChange: (filter: DarwinCoreSearchFilter) => void;
  disabled: boolean;
}) {
  const [suggestions, setSuggestions] = useState<UserSearchItem[]>([]);
  const [loadFailed, setLoadFailed] = useState(false);
  const query = filter.display_value ?? "";
  const selected = filter.value.startsWith(USER_URI_BASE) && query.length > 0;

  useEffect(() => {
    if (query.trim().length === 0 || selected) return;

    let active = true;
    const timer = window.setTimeout(() => {
      apiFetch<UserSearchItem[]>(`/users/search?user_name=${encodeURIComponent(query.trim())}`)
        .then((response) => {
          if (!active) return;
          setSuggestions(response);
          setLoadFailed(false);
        })
        .catch(() => {
          if (!active) return;
          setSuggestions([]);
          setLoadFailed(true);
        });
    }, 250);

    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [query, selected]);

  return (
    <div className="relative min-w-0">
      <label>
        <span className="mb-1 block text-xs font-medium text-[#526168]">ユーザー名</span>
        <input
          autoComplete="off"
          className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
          disabled={disabled}
          onChange={(event) => {
            setSuggestions([]);
            setLoadFailed(false);
            onChange({
              ...filter,
              value: "",
              value_type: "uri",
              display_value: event.target.value,
            });
          }}
          placeholder="ユーザー名を入力"
          type="text"
          value={query}
        />
      </label>

      {selected ? (
        <span className="mt-1 block text-[11px] text-[#176b57]">選択済み</span>
      ) : query.trim().length > 0 ? (
        <span className="mt-1 block text-[11px] text-[#7a878c]">候補から作成者を選択してください。</span>
      ) : null}

      {!selected && suggestions.length > 0 ? (
        <div className="absolute z-20 mt-1 max-h-56 w-full overflow-y-auto rounded-md border border-[#c7d0d4] bg-white p-1 shadow-lg">
          {suggestions.map((user) => (
            <button
              className="flex w-full items-center justify-between gap-3 rounded px-3 py-2 text-left text-sm hover:bg-[#eef2f3]"
              key={user.user_id}
              onClick={() => {
                setSuggestions([]);
                onChange({
                  ...filter,
                  value: `${USER_URI_BASE}${user.user_id}`,
                  value_type: "uri",
                  display_value: user.user_name,
                });
              }}
              type="button"
            >
              <span className="truncate">{user.user_name}</span>
              <span className="shrink-0 font-mono text-[10px] text-[#7a878c]">
                {user.user_id.slice(0, 8)}
              </span>
            </button>
          ))}
        </div>
      ) : null}

      {loadFailed ? (
        <span className="mt-1 block text-[11px] text-[#a53d32]">ユーザー候補を取得できませんでした。</span>
      ) : null}
    </div>
  );
}

function searchValuePlaceholder(predicate: string): string {
  if (predicate === DCTERMS_CREATED || predicate === DCTERMS_MODIFIED) {
    return "例: 2026-09-02T08:00:00Z";
  }
  if (predicate === SOURCE_PAPER) {
    return "https://bio-database.net/papers/...";
  }
  if (predicate === DWCIRI_TO_TAXON) {
    return "https://www.gbif.org/species/...";
  }
  return "検索する値";
}

function inferSearchValueType(value: string): "literal" | "uri" {
  const trimmed = value.trim();
  return /^[A-Za-z][A-Za-z0-9+.-]*:[^\s]+$/.test(trimmed) ? "uri" : "literal";
}

function isBlankScientificNameDefault(filters: DarwinCoreSearchFilter[]): boolean {
  return (
    filters.length === 1 &&
    filters[0]?.predicate === SCIENTIFIC_NAME &&
    filters[0].value.trim().length === 0
  );
}
