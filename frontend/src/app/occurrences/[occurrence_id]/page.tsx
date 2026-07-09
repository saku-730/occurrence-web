"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";

import { SiteHeader } from "@/components/site-header";

const OCCURRENCE_GRAPH_URI =
  "https://bio-database.net/graphs/occurrences";
const API_PREFIX = "/api/backend";
const USER_URI_PREFIX = "https://bio-database.net/users/";
const CREATOR_PREDICATE = "http://purl.org/dc/terms/creator";
const CREATED_PREDICATE = "http://purl.org/dc/terms/created";
const MODIFIED_PREDICATE = "http://purl.org/dc/terms/modified";
const ACCESS_RIGHTS_PREDICATE = "http://purl.org/dc/terms/accessRights";
const SCIENTIFIC_NAME_PREDICATE = "http://rs.tdwg.org/dwc/terms/scientificName";
const BASIS_OF_RECORD_PREDICATE = "http://rs.tdwg.org/dwc/terms/basisOfRecord";
const RECORDED_BY_PREDICATE = "http://rs.tdwg.org/dwc/terms/recordedBy";

interface OccurrenceDetailState {
  status: "loading" | "ready" | "not_found" | "error";
  nquads: string;
}

interface UserSummary {
  user_id: string;
  user_name: string;
}

export default function OccurrenceDetailPage() {
  const router = useRouter();
  const params = useParams<{ occurrence_id?: string }>();
  const occurrenceId = params?.occurrence_id ?? "";

  const [state, setState] = useState<OccurrenceDetailState>({
    status: "loading",
    nquads: "",
  });

  useEffect(() => {
    if (!occurrenceId) {
      setState({ status: "not_found", nquads: "" });
      return;
    }

    let active = true;

    async function loadOccurrence() {
      setState({ status: "loading", nquads: "" });

      try {
        const response = await fetch(`${API_PREFIX}/occurrences/${occurrenceId}`, {
          cache: "no-store",
          credentials: "include",
        });

        if (response.status === 404) {
          if (!active) return;
          setState({ status: "not_found", nquads: "" });
          return;
        }

        if (!response.ok) {
          if (!active) return;
          setState({ status: "error", nquads: "" });
          return;
        }

        const nquads = await response.text();
        if (!active) return;
        setState({ status: "ready", nquads });
      } catch {
        if (!active) return;
        setState({ status: "error", nquads: "" });
      }
    }

    void loadOccurrence();

    return () => {
      active = false;
    };
  }, [occurrenceId]);

  const summary = useMemo(() => summarizeOccurrence(state.nquads, occurrenceId), [
    occurrenceId,
    state.nquads,
  ]);
  const quads = useMemo(() => parseNQuads(state.nquads), [state.nquads]);

  const [creatorSummary, setCreatorSummary] = useState<UserSummary | null>(null);

  useEffect(() => {
    const creatorUserId = summary.creatorUserId;

    if (!creatorUserId) {
      setCreatorSummary(null);
      return;
    }

    const resolvedCreatorUserId = creatorUserId;
    let active = true;

    async function loadCreatorSummary() {
      try {
        const response = await fetch(`${API_PREFIX}/users/${encodeURIComponent(resolvedCreatorUserId)}`, {
          cache: "no-store",
          credentials: "include",
        });

        if (!response.ok) {
          if (!active) return;
          setCreatorSummary(null);
          return;
        }

        const user = (await response.json()) as UserSummary;
        if (!active) return;
        setCreatorSummary(user);
      } catch {
        if (!active) return;
        setCreatorSummary(null);
      }
    }

    void loadCreatorSummary();

    return () => {
      active = false;
    };
  }, [summary.creatorUserId]);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h1 className="text-2xl font-semibold">データ詳細</h1>
            <p className="mt-2 text-sm text-[#65737a]">
              occurrence に登録された内容を確認できます。
            </p>
          </div>
          <div className="flex gap-3">
            <button
              className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3]"
              onClick={() => router.back()}
              type="button"
            >
              戻る
            </button>
            <Link
              className="inline-flex h-10 items-center rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746]"
              href="/occurrences/search"
            >
              検索へ戻る
            </Link>
          </div>
        </div>

        {state.status === "loading" ? (
          <StatusPanel message="データを読み込んでいます" />
        ) : null}

        {state.status === "not_found" ? (
          <StatusPanel message="データが見つかりませんでした" />
        ) : null}

        {state.status === "error" ? (
          <StatusPanel message="詳細データを取得できませんでした" />
        ) : null}

        {state.status === "ready" ? (
          <div className="space-y-6">
            <section className="rounded-md border border-[#d8dfe2] bg-white px-5 py-5">
              <div className="grid gap-4 md:grid-cols-2">
                <DetailField label="Occurrence ID" value={summary.occurrenceId ?? occurrenceId} />
                <DetailField label="Occurrence URI" value={summary.occurrenceUri ?? `https://bio-database.net/occurrences/${occurrenceId}`} />
                <DetailField label="学名" value={summary.scientificName} />
                <DetailField label="記録種別" value={summary.basisOfRecord} />
                <DetailField label="記録者" value={summary.recordedBy} />
                <DetailField label="作成日時" value={summary.created} />
                <DetailField label="更新日時" value={summary.modified} />
                <DetailField label="公開範囲" value={summary.accessRights} />
                <CreatorField
                  userName={creatorSummary?.user_name ?? null}
                  userId={creatorSummary?.user_id ?? summary.creatorUserId}
                />
              </div>
            </section>

            <section className="rounded-md border border-[#d8dfe2] bg-white px-5 py-5">
              <h2 className="text-sm font-medium text-[#526168]">登録内容</h2>
              {quads.length === 0 ? (
                <p className="mt-2 text-sm text-[#65737a]">
                  項目がありません。
                </p>
              ) : (
                <div className="mt-4 overflow-x-auto">
                  <table className="w-full border-collapse text-left text-sm">
                    <thead>
                      <tr className="border-b border-[#d8dfe2] text-xs uppercase tracking-wide text-[#65737a]">
                        <th className="py-2 pr-4 font-medium">主語</th>
                        <th className="py-2 pr-4 font-medium">項目名</th>
                        <th className="py-2 pr-4 font-medium">値</th>
                      </tr>
                    </thead>
                    <tbody>
                      {quads.map((quad, index) => (
                        <tr key={`${quad.subject}-${quad.predicate}-${index}`} className="border-b border-[#eef2f3] align-top last:border-b-0">
                          <td className="py-3 pr-4 text-[#65737a]">
                            {formatNode(quad.subject)}
                          </td>
                          <td className="py-3 pr-4 font-medium text-[#182126]">
                            {predicateLabel(quad.predicate)}
                          </td>
                          <td className="py-3 pr-4 break-all text-[#182126]">
                            {formatNode(quad.object)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </section>
          </div>
        ) : null}
      </main>
    </div>
  );
}

function DetailField({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="min-w-0">
      <p className="text-xs font-medium uppercase tracking-wide text-[#65737a]">
        {label}
      </p>
      <p className="mt-2 break-all text-sm text-[#182126]">{value ?? "-"}</p>
    </div>
  );
}

function CreatorField({
  userName,
  userId,
}: {
  userName: string | null;
  userId: string | null;
}) {
  const primary = userName ?? userId ?? "-";

  return (
    <div className="min-w-0">
      <p className="text-xs font-medium uppercase tracking-wide text-[#65737a]">
        作成者
      </p>
      <p className="mt-2 break-all text-sm text-[#182126]">{primary}</p>
      {userName && userId ? (
        <p className="mt-1 break-all text-xs text-[#65737a]">ID: {userId}</p>
      ) : null}
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

function summarizeOccurrence(nquads: string, occurrenceId: string) {
  const lines = nquads
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);

  const occurrenceUri = `https://bio-database.net/occurrences/${occurrenceId}`;
  const creatorUserUri = findPredicateValue(lines, CREATOR_PREDICATE);

  return {
    occurrenceId,
    occurrenceUri: occurrenceUri,
    scientificName: findPredicateValue(lines, SCIENTIFIC_NAME_PREDICATE),
    basisOfRecord: findPredicateValue(lines, BASIS_OF_RECORD_PREDICATE),
    recordedBy: findPredicateValue(lines, RECORDED_BY_PREDICATE),
    created: findPredicateValue(lines, CREATED_PREDICATE),
    modified: findPredicateValue(lines, MODIFIED_PREDICATE),
    accessRights: findPredicateValue(lines, ACCESS_RIGHTS_PREDICATE),
    creatorUserUri,
    creatorUserId: extractUserIdFromUserUri(creatorUserUri),
  };
}

function findPredicateValue(lines: string[], predicateUri: string): string | null {
  for (const line of lines) {
    if (!line.includes(`<${predicateUri}>`)) continue;
    const match = line.match(/^<([^>]+)> <([^>]+)> (.+) <([^>]+)> \.$/u);
    if (!match) continue;
    return normalizeObject(match[3]);
  }

  return null;
}

function extractUserIdFromUserUri(userUri: string | null): string | null {
  if (!userUri) {
    return null;
  }

  const trimmed = userUri.trim();

  if (trimmed.startsWith(USER_URI_PREFIX)) {
    const userId = trimmed.slice(USER_URI_PREFIX.length).trim();
    return userId.length > 0 ? userId : null;
  }

  try {
    const parsed = new URL(trimmed);
    if (!parsed.pathname.startsWith("/users/")) {
      return null;
    }

    const userId = parsed.pathname.split("/").filter(Boolean).at(-1) ?? "";
    return userId.length > 0 ? decodeURIComponent(userId) : null;
  } catch {
    // URLとして解釈できない場合でも、最後の保険としてそのまま返さない。
    return null;
  }
}

function parseNQuads(nquads: string) {
  return nquads
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const match = line.match(/^<([^>]+)> <([^>]+)> (.+) <([^>]+)> \.$/u);
      if (!match) {
        return null;
      }

      return {
        subject: match[1],
        predicate: match[2],
        object: match[3],
        graph: match[4],
      };
    })
    .filter((quad): quad is { subject: string; predicate: string; object: string; graph: string } => quad !== null);
}

function predicateLabel(predicateUri: string): string {
  const known: Record<string, string> = {
    "http://purl.org/dc/terms/creator": "作成者",
    "http://purl.org/dc/terms/created": "作成日時",
    "http://purl.org/dc/terms/modified": "更新日時",
    "http://purl.org/dc/terms/accessRights": "公開範囲",
    "http://rs.tdwg.org/dwc/terms/scientificName": "学名",
    "http://rs.tdwg.org/dwc/terms/basisOfRecord": "記録種別",
    "http://rs.tdwg.org/dwc/terms/recordedBy": "記録者",
  };

  if (known[predicateUri]) {
    return known[predicateUri];
  }

  const fragment = predicateUri.split(/[\/#]/u).filter(Boolean).at(-1);
  return fragment ?? predicateUri;
}

function formatNode(node: string): string {
  const normalized = normalizeObject(node);
  if (normalized.startsWith(USER_URI_PREFIX)) {
    return normalized.slice(USER_URI_PREFIX.length);
  }

  if (normalized === OCCURRENCE_GRAPH_URI) {
    return "occurrence graph";
  }

  return normalized;
}

function normalizeObject(object: string): string {
  if (object.startsWith("<") && object.endsWith(">")) {
    return object.slice(1, -1);
  }

  if (object.startsWith("\"") && object.endsWith("\"")) {
    return object.slice(1, -1);
  }

  return object;
}
