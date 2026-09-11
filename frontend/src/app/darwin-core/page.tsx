"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { apiFetch } from "@/lib/api";

const DWCIRI_TO_TAXON_URI = "http://rs.tdwg.org/dwc/iri/toTaxon";
const DWC_DECIMAL_LONGITUDE_URI =
  "http://rs.tdwg.org/dwc/terms/decimalLongitude";
const DWC_DECIMAL_LATITUDE_URI =
  "http://rs.tdwg.org/dwc/terms/decimalLatitude";

interface DarwinCoreTerm {
  uri: string;
  local_name: string;
}

const FIXED_TERMS: DarwinCoreTerm[] = [
  { uri: DWCIRI_TO_TAXON_URI, local_name: "分類" },
  { uri: DWC_DECIMAL_LONGITUDE_URI, local_name: "経度" },
  { uri: DWC_DECIMAL_LATITUDE_URI, local_name: "緯度" },
];

export default function DarwinCoreTermsPage() {
  const [terms, setTerms] = useState<DarwinCoreTerm[]>([]);
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<"loading" | "loaded" | "error">(
    "loading",
  );

  useEffect(() => {
    let active = true;

    void apiFetch<DarwinCoreTerm[]>("/vocabularies/darwin-core", {
      cache: "no-store",
    })
      .then((fetchedTerms) => {
        if (!active) return;

        const byUri = new Map<string, DarwinCoreTerm>();
        for (const term of fetchedTerms) {
          byUri.set(term.uri, term);
        }
        for (const term of FIXED_TERMS) {
          byUri.set(term.uri, term);
        }

        setTerms(
          [...byUri.values()].sort((left, right) => {
            const byName = left.local_name.localeCompare(right.local_name, "ja", {
              sensitivity: "base",
            });
            if (byName !== 0) return byName;
            return left.uri.localeCompare(right.uri, "en", {
              sensitivity: "base",
            });
          }),
        );
        setStatus("loaded");
      })
      .catch(() => {
        if (active) setStatus("error");
      });

    return () => {
      active = false;
    };
  }, []);

  const filteredTerms = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return terms;

    return terms.filter(
      (term) =>
        term.local_name.toLocaleLowerCase().includes(normalized) ||
        term.uri.toLocaleLowerCase().includes(normalized),
    );
  }, [query, terms]);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold">Darwin Core 項目一覧</h1>
            <p className="mt-2 text-sm text-[#65737a]">
              Bio-Databaseのデータ登録で利用できるDarwin Core項目を表示しています。
            </p>
          </div>
          <Link
            className="text-sm font-medium text-[#176b57] hover:underline"
            href="/occurrences/new"
          >
            データ登録へ戻る
          </Link>
        </div>

        <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-4">
            <label className="block max-w-xl">
              <span className="mb-2 block text-sm font-medium text-[#526168]">
                項目を検索
              </span>
              <input
                className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                onChange={(event) => setQuery(event.target.value)}
                placeholder="項目名またはURI"
                type="search"
                value={query}
              />
            </label>
          </div>

          {status === "loading" ? (
            <p className="px-5 py-8 text-sm text-[#65737a]" role="status">
              Darwin Core項目を読み込んでいます
            </p>
          ) : null}

          {status === "error" ? (
            <p className="px-5 py-8 text-sm text-[#a23c32]" role="alert">
              Darwin Core項目を取得できませんでした。
            </p>
          ) : null}

          {status === "loaded" ? (
            <>
              <div className="flex items-center justify-between border-b border-[#d8dfe2] px-5 py-3 text-xs text-[#65737a]">
                <span>{filteredTerms.length}件</span>
                {query.trim() ? <span>全{terms.length}件</span> : null}
              </div>

              {filteredTerms.length === 0 ? (
                <p className="px-5 py-8 text-sm text-[#65737a]">
                  一致する項目はありません。
                </p>
              ) : (
                <div className="overflow-x-auto">
                  <table className="w-full border-collapse text-left">
                    <thead className="bg-[#f7f9fa] text-xs font-medium text-[#526168]">
                      <tr>
                        <th className="w-56 px-5 py-3">項目名</th>
                        <th className="px-5 py-3">URI</th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-[#e4e9eb]">
                      {filteredTerms.map((term) => (
                        <tr key={term.uri}>
                          <td className="px-5 py-4 text-sm font-medium">
                            {term.local_name}
                          </td>
                          <td className="px-5 py-4 text-sm">
                            <a
                              className="break-all text-[#176b57] hover:underline"
                              href={term.uri}
                              rel="noreferrer"
                              target="_blank"
                            >
                              {term.uri}
                            </a>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          ) : null}
        </section>
      </main>
    </div>
  );
}
