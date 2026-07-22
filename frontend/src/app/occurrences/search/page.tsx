"use client";

import Link from "next/link";
import { FormEvent, useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

const SCIENTIFIC_NAME_PREDICATE =
  "http://rs.tdwg.org/dwc/terms/scientificName";
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
  const [query, setQuery] = useState("");
  const [ownOnly, setOwnOnly] = useState(false);
  const [creatorNames, setCreatorNames] = useState<Record<string, string>>({});
  const [appliedQuery, setAppliedQuery] = useState("");
  const [appliedOwnOnly, setAppliedOwnOnly] = useState(false);
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [status, setStatus] = useState<SearchStatus>("loading");

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

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">データ検索</h1>
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

        <SearchResults creatorNames={creatorNames} result={result} status={status} />

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
      </main>
    </div>
  );
}


function SearchResults({
  creatorNames,
  result,
  status,
}: {
  creatorNames: Record<string, string>;
  result: SearchResponse | null;
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

  return (
    <div className="overflow-x-auto rounded-md border border-[#d8dfe2] bg-white">
      <table className="w-full min-w-[900px] border-collapse text-left text-sm">
        <thead className="border-b border-[#d8dfe2] bg-[#eef2f3] text-xs text-[#526168]">
          <tr>
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

function formatDate(value: string | null): string {
  if (!value) return "-";

  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
