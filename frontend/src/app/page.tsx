"use client";
import Link from "next/link";

import { useEffect, useState } from "react";

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

interface SearchOccurrencesResponse {
  items: OccurrenceItem[];
  page: {
    limit: number;
    next_cursor: string | null;
    has_next: boolean;
  };
}

type LoadState =
  | { status: "loading" }
  | { status: "unauthenticated" }
  | { status: "error" }
  | { status: "ready"; items: OccurrenceItem[] };

export default function Home() {
  const [loadState, setLoadState] = useState<LoadState>({ status: "loading" });

  useEffect(() => {
    let active = true;

    async function loadRecentOccurrences() {
      try {
        // creatorは任意入力にせず、認証済みセッションのuser_idからURIを組み立てる。
        const currentUser = await apiFetch<CurrentUser>("/auth/me");
        const response = await apiFetch<SearchOccurrencesResponse>(
          "/occurrences/search",
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({
              filters: [
                {
                  predicate: CREATOR_PREDICATE,
                  value: `${USER_URI_BASE}${currentUser.user_id}`,
                  value_type: "uri",
                  match: "exact",
                },
              ],
              page: { limit: 10, cursor: null },
            }),
          },
        );

        if (active) {
          setLoadState({ status: "ready", items: response.items });
        }
      } catch (error) {
        if (!active) return;

        // /auth/meの401は通信障害と区別し、ログインが必要な状態として表示する。
        if (error instanceof ApiError && error.status === 401) {
          setLoadState({ status: "unauthenticated" });
          return;
        }

        setLoadState({ status: "error" });
      }
    }

    void loadRecentOccurrences();
    return () => {
      active = false;
    };
  }, []);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">最近追加したデータ</h1>
          <p className="mt-1 text-sm text-[#65737a]">
            あなたが登録した最新10件
          </p>
        </div>

        <RecentOccurrences state={loadState} />
      </main>
    </div>
  );
}

function RecentOccurrences({ state }: { state: LoadState }) {
  if (state.status === "loading") {
    return <StatusPanel message="データを読み込んでいます" />;
  }

  if (state.status === "unauthenticated") {
    return <StatusPanel message="最近追加したデータを表示するにはログインが必要です" />;
  }

  if (state.status === "error") {
    return <StatusPanel message="データを取得できませんでした" />;
  }

  if (state.items.length === 0) {
    return <StatusPanel message="登録したデータはありません" />;
  }

  return (
    <div className="overflow-x-auto rounded-md border border-[#d8dfe2] bg-white">
      <table className="w-full min-w-[760px] border-collapse text-left text-sm">
        <thead className="border-b border-[#d8dfe2] bg-[#eef2f3] text-xs text-[#526168]">
          <tr>
            <th className="px-4 py-3 font-medium">学名</th>
            <th className="px-4 py-3 font-medium">記録種別</th>
            <th className="px-4 py-3 font-medium">記録者</th>
            <th className="px-4 py-3 font-medium">公開範囲</th>
            <th className="px-4 py-3 font-medium">登録日時</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-[#e4e9eb]">
          {state.items.map((item) => (
            <tr key={item.occurrence_id} className="hover:bg-[#f8faf9]">
              <td className="px-4 py-3 font-medium">
                <Link
                  className="text-[#176b57] hover:underline"
                  href={`/occurrences/${item.occurrence_id}`}
                >
                  {item.scientific_name ?? "名称未登録"}
                </Link>
              </td>
              <td className="px-4 py-3">{item.basis_of_record ?? "-"}</td>
              <td className="px-4 py-3">{item.recorded_by ?? "-"}</td>
              <td className="px-4 py-3">{item.access_rights ?? "-"}</td>
              <td className="whitespace-nowrap px-4 py-3">
                {formatCreatedAt(item.created)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function StatusPanel({ message }: { message: string }) {
  return (
    <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
      <p className="text-sm text-[#65737a]">{message}</p>
    </section>
  );
}

function formatCreatedAt(created: string | null): string {
  if (!created) return "-";

  const date = new Date(created);
  if (Number.isNaN(date.getTime())) return created;

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
