"use client";

import Link from "next/link";
import { FormEvent, useEffect, useState } from "react";

import {
  DarwinCoreSearchFilters,
  type DarwinCoreSearchFilter,
  activeDarwinCoreSearchFilters,
  emptyDarwinCoreSearchFilter,
} from "@/components/darwin-core-search-filters";
import { LabelPreviewDialog } from "@/components/occurrence-label-preview";
import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

const CREATOR_PREDICATE = "http://purl.org/dc/terms/creator";
const USER_URI_BASE = "https://bio-database.net/users/";

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

interface OccurrenceItem {
  occurrence_id: string;
  occurrence_uri: string;
  creator_user_id: string | null;
  scientific_name: string | null;
  created: string | null;
  modified: string | null;
  access_rights: string | null;
}

interface SearchResponse {
  items: OccurrenceItem[];
  page: {
    limit: number;
    next_cursor: string | null;
    has_next: boolean;
  };
}

type SearchStatus = "loading" | "ready" | "unauthenticated" | "error";

export default function OccurrenceSearchPage() {
  const [filters, setFilters] = useState<DarwinCoreSearchFilter[]>([
    emptyDarwinCoreSearchFilter(),
  ]);
  const [appliedFilters, setAppliedFilters] = useState<DarwinCoreSearchFilter[]>([]);
  const [ownOnly, setOwnOnly] = useState(false);
  const [appliedOwnOnly, setAppliedOwnOnly] = useState(false);
  const [creatorNames, setCreatorNames] = useState<Record<string, string>>({});
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [status, setStatus] = useState<SearchStatus>("loading");
  const [selectedOccurrenceIds, setSelectedOccurrenceIds] = useState<Set<string>>(new Set());
  const [isLabelPreviewOpen, setIsLabelPreviewOpen] = useState(false);

  useEffect(() => {
    let active = true;

    searchOccurrences([], null, false)
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
    const creatorIds = [
      ...new Set(
        result?.items
          .map((item) => item.creator_user_id)
          .filter((creatorUserId): creatorUserId is string => creatorUserId !== null) ?? [],
      ),
    ];

    if (creatorIds.length === 0) return;

    let active = true;

    Promise.all(
      creatorIds.map(async (creatorUserId) => {
        try {
          const user = await apiFetch<UserSummary>(
            `/users/${encodeURIComponent(creatorUserId)}`,
          );
          return [creatorUserId, user.user_name] as const;
        } catch {
          return null;
        }
      }),
    ).then((entries) => {
      if (!active) return;
      setCreatorNames(
        Object.fromEntries(
          entries.filter(
            (entry): entry is readonly [string, string] => entry !== null,
          ),
        ),
      );
    });

    return () => {
      active = false;
    };
  }, [result]);

  async function runSearch(
    searchFilters: DarwinCoreSearchFilter[],
    cursor: string | null,
    searchOwnOnly: boolean,
  ) {
    setStatus("loading");
    const normalizedFilters = activeDarwinCoreSearchFilters(searchFilters);

    try {
      const response = await searchOccurrences(normalizedFilters, cursor, searchOwnOnly);
      setResult(response);
      setSelectedOccurrenceIds(new Set());
      setIsLabelPreviewOpen(false);
      if (cursor === null) {
        setAppliedFilters(normalizedFilters);
        setAppliedOwnOnly(searchOwnOnly);
      }
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
    void runSearch(filters, null, ownOnly);
  }

  function clearFilters() {
    const next = [emptyDarwinCoreSearchFilter()];
    setFilters(next);
    void runSearch([], null, ownOnly);
  }

  const selectedOccurrences =
    result?.items.filter((item) => selectedOccurrenceIds.has(item.occurrence_id)) ?? [];

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-base font-semibold">データ検索</h1>
          <p className="mt-1 text-sm text-[#65747a]">
            任意のDarwin Core項目を使ってOccurrenceを検索できます。
          </p>
        </div>

        <form className="mb-6 space-y-4" onSubmit={handleSubmit}>
          <DarwinCoreSearchFilters
            disabled={status === "loading"}
            filters={filters}
            onChange={setFilters}
          />

          <div className="flex flex-wrap items-center gap-3">
            <button
              className="h-10 rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#829b95]"
              disabled={status === "loading"}
              type="submit"
            >
              検索
            </button>
            <button
              className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-not-allowed"
              disabled={status === "loading"}
              onClick={clearFilters}
              type="button"
            >
              条件をクリア
            </button>
            <label className="ml-1 flex cursor-pointer items-center gap-2 text-sm">
              <input
                checked={ownOnly}
                className="size-4 accent-[#176b57]"
                onChange={(event) => {
                  const checked = event.target.checked;
                  setOwnOnly(checked);
                  void runSearch(filters, null, checked);
                }}
                type="checkbox"
              />
              自分のデータのみ表示
            </label>
          </div>
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
                void runSearch(appliedFilters, result.page.next_cursor, appliedOwnOnly)
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
      if (checked) next.add(occurrenceId);
      else next.delete(occurrenceId);
      return next;
    });
  }

  function toggleAllOccurrenceSelection(checked: boolean) {
    onSelectedOccurrenceIdsChange(checked ? new Set(occurrenceIds) : new Set());
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
                onChange={(event) => toggleAllOccurrenceSelection(event.target.checked)}
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
                    toggleOccurrenceSelection(item.occurrence_id, event.target.checked)
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
              <TableCell>
                {item.creator_user_id ? creatorNames[item.creator_user_id] ?? "-" : "-"}
              </TableCell>
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
  dwcFilters: DarwinCoreSearchFilter[],
  cursor: string | null,
  ownOnly: boolean,
): Promise<SearchResponse> {
  const filters: DarwinCoreSearchFilter[] = [...dwcFilters];

  if (ownOnly) {
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

function formatDate(value: string | null): string {
  if (!value) return "-";

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
