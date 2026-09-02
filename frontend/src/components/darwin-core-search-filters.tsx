"use client";

import { useEffect, useState } from "react";

import { apiFetch } from "@/lib/api";

export interface DarwinCoreSearchFilter {
  predicate: string;
  value: string;
  value_type: "literal" | "uri";
  match: "exact";
}

interface DarwinCoreTerm {
  uri: string;
  local_name: string;
}

export function emptyDarwinCoreSearchFilter(
  predicate = "http://rs.tdwg.org/dwc/terms/scientificName",
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
        value_type: inferSearchValueType(value),
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

  function replaceFilter(index: number, next: DarwinCoreSearchFilter) {
    onChange(filters.map((filter, currentIndex) => (currentIndex === index ? next : filter)));
  }

  function removeFilter(index: number) {
    onChange(filters.filter((_, currentIndex) => currentIndex !== index));
  }

  return (
    <div className="space-y-3">
      {filters.map((filter, index) => {
        const selectedTerm = terms.find((term) => term.uri === filter.predicate);
        const customPredicate = selectedTerm ? "" : filter.predicate;

        return (
          <div
            className="grid gap-3 rounded-md border border-[#d8dfe2] bg-white p-3 md:grid-cols-[minmax(16rem,1.3fr)_minmax(12rem,1fr)_auto] md:items-end"
            key={index}
          >
            <div className="min-w-0">
              <label>
                <span className="mb-1 block text-xs font-medium text-[#526168]">Darwin Core項目</span>
                <select
                  className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                  disabled={disabled}
                  onChange={(event) =>
                    replaceFilter(index, {
                      ...filter,
                      predicate: event.target.value,
                    })
                  }
                  value={selectedTerm?.uri ?? ""}
                >
                  <option value="">項目を選択</option>
                  {terms.map((term) => (
                    <option key={term.uri} value={term.uri}>
                      {term.local_name}
                    </option>
                  ))}
                </select>
              </label>

              {selectedTerm ? (
                <span className="mt-1 block truncate text-[11px] text-[#7a878c]" title={selectedTerm.uri}>
                  IRI: {selectedTerm.uri}
                </span>
              ) : null}

              <details className="mt-2 text-xs" open={customPredicate.length > 0}>
                <summary className="cursor-pointer text-[#65747a] hover:text-[#176b57]">
                  候補にないDwC IRIを直接指定
                </summary>
                <input
                  className="mt-2 h-9 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-xs outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                  disabled={disabled}
                  onChange={(event) =>
                    replaceFilter(index, {
                      ...filter,
                      predicate: event.target.value,
                    })
                  }
                  placeholder="http://rs.tdwg.org/dwc/terms/..."
                  type="text"
                  value={customPredicate}
                />
              </details>
            </div>

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
                placeholder="検索する値"
                type="text"
                value={filter.value}
              />
            </label>

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
            用語候補を取得できませんでした。DwC IRIの直接入力は利用できます。
          </span>
        ) : null}
      </div>
    </div>
  );
}

function inferSearchValueType(value: string): "literal" | "uri" {
  const trimmed = value.trim();
  return /^[A-Za-z][A-Za-z0-9+.-]*:[^\s]+$/.test(trimmed) ? "uri" : "literal";
}
