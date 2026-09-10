"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import type { FormEvent } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

type PaperListItem = {
  id: string;
  title: string | null;
  doi: string | null;
  first_imported_at: string;
};

type ListPapersResponse = {
  papers: PaperListItem[];
};

type LoadStatus = "loading" | "ready" | "unauthenticated" | "error";

export default function PapersPage() {
  const [query, setQuery] = useState("");
  const [papers, setPapers] = useState<PaperListItem[]>([]);
  const [status, setStatus] = useState<LoadStatus>("loading");

  const loadPapers = useCallback(async (search: string) => {
    setStatus("loading");
    try {
      const trimmed = search.trim();
      const suffix = trimmed ? `?q=${encodeURIComponent(trimmed)}` : "";
      const response = await apiFetch<ListPapersResponse>(`/papers${suffix}`);
      setPapers(response.papers);
      setStatus("ready");
    } catch (error: unknown) {
      if (error instanceof ApiError && error.status === 401) {
        setStatus("unauthenticated");
        return;
      }
      setStatus("error");
    }
  }, []);

  useEffect(() => {
    void loadPapers("");
  }, [loadPapers]);

  const handleSearch = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    void loadPapers(query);
  };

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />
      <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
        <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
          <h1 className="text-2xl font-semibold">インポート済み論文</h1>
          <Link
            href="/paper-import"
            className="rounded-md border border-[#c9d2d6] bg-white px-4 py-2 text-sm font-medium text-[#344249] hover:bg-[#eef2f3]"
          >
            論文インポートへ戻る
          </Link>
        </div>

        <form
          onSubmit={handleSearch}
          className="mb-6 flex flex-col gap-3 rounded-md border border-[#d8dfe2] bg-white p-4 sm:flex-row"
        >
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="タイトルまたはDOIで検索"
            className="min-w-0 flex-1 rounded-md border border-[#c9d2d6] px-3 py-2.5 text-sm outline-none focus:border-[#7f929b]"
          />
          <button
            type="submit"
            disabled={status === "loading"}
            className="rounded-md bg-[#31434b] px-5 py-2.5 text-sm font-medium text-white disabled:cursor-not-allowed disabled:bg-[#9ca8ad]"
          >
            検索
          </button>
        </form>

        {status === "loading" && (
          <section className="grid min-h-48 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">論文一覧を読み込んでいます</p>
          </section>
        )}

        {status === "unauthenticated" && (
          <section className="grid min-h-48 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">論文一覧を表示するにはログインが必要です</p>
          </section>
        )}

        {status === "error" && (
          <section className="grid min-h-48 place-items-center rounded-md border border-[#e1b8b8] bg-[#fff5f5] px-6 py-12 text-center">
            <p className="text-sm text-[#8d3131]">論文一覧を取得できませんでした</p>
          </section>
        )}

        {status === "ready" && papers.length === 0 && (
          <section className="grid min-h-48 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">該当する論文はありません</p>
          </section>
        )}

        {status === "ready" && papers.length > 0 && (
          <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
            <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3 text-sm text-[#526168]">
              {papers.length}件
            </div>
            <div className="overflow-x-auto">
              <table className="w-full min-w-[760px] border-collapse text-left text-sm">
                <thead className="border-b border-[#d8dfe2] bg-[#f7f9f9] text-xs text-[#526168]">
                  <tr>
                    <th className="w-1/2 px-5 py-3 font-medium">タイトル</th>
                    <th className="w-1/4 px-5 py-3 font-medium">DOI</th>
                    <th className="whitespace-nowrap px-5 py-3 font-medium">
                      インポート日時
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-[#e3e8ea]">
                  {papers.map((paper) => (
                    <tr key={paper.id} className="hover:bg-[#f8faf9]">
                      <td className="px-5 py-4 align-top font-medium leading-6">
                        <Link
                          href={`/occurrences/search?sourcePaper=${encodeURIComponent(paper.id)}`}
                          className="text-[#176b57] underline decoration-[#aab6bb] underline-offset-2 hover:text-[#125746]"
                        >
                          {paper.title?.trim() || "タイトル未取得"}
                        </Link>
                      </td>
                      <td className="max-w-72 break-all px-5 py-4 align-top text-[#344249]">
                        {paper.doi ? (
                          <a
                            href={doiUrl(paper.doi)}
                            target="_blank"
                            rel="noreferrer"
                            className="underline decoration-[#aab6bb] underline-offset-2 hover:text-black"
                          >
                            {paper.doi}
                          </a>
                        ) : (
                          "-"
                        )}
                      </td>
                      <td className="whitespace-nowrap px-5 py-4 align-top text-[#344249]">
                        {formatImportedAt(paper.first_imported_at)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </section>
        )}
      </main>
    </div>
  );
}

function doiUrl(doi: string): string {
  const value = doi.trim();
  if (/^https?:\/\//i.test(value)) return value;
  return `https://doi.org/${value.replace(/^doi:\s*/i, "")}`;
}

function formatImportedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("ja-JP", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}
