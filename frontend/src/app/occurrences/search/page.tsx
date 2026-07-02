"use client";

import Link from "next/link";
import { FormEvent, useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { apiFetch } from "@/lib/api";

const SCIENTIFIC_NAME_PREDICATE =
  "http://rs.tdwg.org/dwc/terms/scientificName";

interface OccurrenceItem {
  occurrence_id: string;
  occurrence_uri: string;
  scientific_name: string | null;
  basis_of_record: string | null;
  recorded_by: string | null;
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

export default function OccurrenceSearchPage() {
  const [query, setQuery] = useState("");
  const [appliedQuery, setAppliedQuery] = useState("");
  const [result, setResult] = useState<SearchResponse | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");

  useEffect(() => {
    let active = true;

    // The initial empty filter request doubles as the normal occurrence list.
    searchOccurrences("", null)
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

  async function runSearch(searchQuery: string, cursor: string | null) {
    setStatus("loading");
    try {
      const response = await searchOccurrences(searchQuery, cursor);
      setResult(response);
      setAppliedQuery(searchQuery);
      setStatus("ready");
    } catch {
      setStatus("error");
    }
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    void runSearch(query.trim(), null);
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">データ検索</h1>
        </div>

        <form
          className="mb-6 flex max-w-2xl items-end gap-3"
          onSubmit={handleSubmit}
        >
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
        </form>

        <SearchResults result={result} status={status} />

        {status === "ready" && result?.page.has_next && result.page.next_cursor ? (
          <div className="mt-5 flex justify-center">
            <button
              className="rounded-md border border-[#b8c3c8] bg-white px-5 py-2 text-sm font-medium hover:bg-[#eef2f3]"
              onClick={() =>
                void runSearch(appliedQuery, result.page.next_cursor)
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
  result,
  status,
}: {
  result: SearchResponse | null;
  status: "loading" | "ready" | "error";
}) {
  if (status === "loading") {
    return <StatusPanel message="検索しています" />;
  }

  if (status === "error") {
    return <StatusPanel message="検索結果を取得できませんでした" />;
  }

  if (!result || result.items.length === 0) {
    return <StatusPanel message="該当するデータはありません" />;
  }

  return (
    <div className="overflow-x-auto rounded-md border border-[#d8dfe2] bg-white">
      <table className="w-full min-w-[1280px] border-collapse text-left text-sm">
        <thead className="border-b border-[#d8dfe2] bg-[#eef2f3] text-xs text-[#526168]">
          <tr>
            <TableHeader>ID</TableHeader>
            <TableHeader>URI</TableHeader>
            <TableHeader>学名</TableHeader>
            <TableHeader>記録種別</TableHeader>
            <TableHeader>記録者</TableHeader>
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
              <TableCell>
                <span className="block max-w-64 break-all">
                  {item.occurrence_uri}
                </span>
              </TableCell>
              <TableCell>{item.scientific_name ?? "-"}</TableCell>
              <TableCell>{item.basis_of_record ?? "-"}</TableCell>
              <TableCell>{item.recorded_by ?? "-"}</TableCell>
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

function searchOccurrences(
  query: string,
  cursor: string | null,
): Promise<SearchResponse> {
  return apiFetch<SearchResponse>("/occurrences/search", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      filters: query
        ? [
            {
              predicate: SCIENTIFIC_NAME_PREDICATE,
              value: query,
              value_type: "literal",
              match: "exact",
            },
          ]
        : [],
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
