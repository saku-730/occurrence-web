"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useMemo, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { apiFetch } from "@/lib/api";

const API_PREFIX = "/api/backend";
const USER_URI_PREFIX = "https://bio-database.net/users/";
const CREATOR_PREDICATE = "http://purl.org/dc/terms/creator";

interface OccurrenceDetailState {
  status: "loading" | "ready" | "not_found" | "error";
  nquads: string;
}

interface UserSummary {
  user_id: string;
  user_name: string;
}

interface CurrentUser {
  user_id: string;
}

interface ParsedQuad {
  subject: string;
  predicate: string;
  object: string;
  graph: string;
}

interface IntermediateSection {
  subject: string;
  title: string;
  quads: ParsedQuad[];
}

export default function OccurrenceDetailPage() {
  const router = useRouter();
  const params = useParams<{ occurrence_id?: string }>();
  const occurrenceId = params?.occurrence_id ?? "";

  const [state, setState] = useState<OccurrenceDetailState>({
    status: "loading",
    nquads: "",
  });
  const [creatorSummary, setCreatorSummary] = useState<UserSummary | null>(null);
  const [currentUserId, setCurrentUserId] = useState<string | null>(null);

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

  const quads = useMemo(() => parseNQuads(state.nquads), [state.nquads]);
  const creatorUserUri = useMemo(() => findQuadObject(state.nquads, CREATOR_PREDICATE), [state.nquads]);
  const creatorUserId = useMemo(() => extractUserIdFromUserUri(creatorUserUri), [creatorUserUri]);
  const occurrenceUri = `https://bio-database.net/occurrences/${occurrenceId}`;
  const rootQuads = useMemo(
    () =>
      quads.filter(
        (quad) =>
          quad.subject === occurrenceUri &&
          quad.predicate !== CREATOR_PREDICATE &&
          !isIntermediateRelationPredicate(quad.predicate),
      ),
    [occurrenceUri, quads],
  );
  const intermediateSections = useMemo(() => buildIntermediateSections(quads, occurrenceUri), [occurrenceUri, quads]);
  const canEditOccurrence = creatorUserId !== null && currentUserId === creatorUserId;

  useEffect(() => {
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
  }, [creatorUserId]);

  useEffect(() => {
    let active = true;

    apiFetch<CurrentUser>("/auth/me")
      .then((user) => {
        if (active) {
          setCurrentUserId(user.user_id);
        }
      })
      .catch(() => {
        if (active) {
          setCurrentUserId(null);
        }
      });

    return () => {
      active = false;
    };
  }, []);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-7xl px-5 py-8 sm:px-8">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h1 className="text-2xl font-semibold">データ詳細</h1>
            <p className="mt-2 text-sm text-[#65737a]">occurrence に登録された内容を確認できます。</p>
          </div>
          <div className="flex gap-3">
            <button
              className="h-10 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3]"
              onClick={() => router.back()}
              type="button"
            >
              戻る
            </button>
            {canEditOccurrence ? (
              <Link
                className="inline-flex h-10 items-center rounded-md border border-[#176b57] bg-white px-4 text-sm font-medium text-[#176b57] hover:bg-[#eef7f4]"
                href={`/occurrences/${occurrenceId}/edit`}
              >
                編集
              </Link>
            ) : null}
            <Link
              className="inline-flex h-10 items-center rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746]"
              href="/occurrences/search"
            >
              検索へ戻る
            </Link>
          </div>
        </div>

        {state.status === "loading" ? <StatusPanel message="データを読み込んでいます" /> : null}

        {state.status === "not_found" ? <StatusPanel message="データが見つかりませんでした" /> : null}

        {state.status === "error" ? <StatusPanel message="詳細データを取得できませんでした" /> : null}

        {state.status === "ready" ? (
          <div className="space-y-6">
            <section className="rounded-md border border-[#d8dfe2] bg-white px-5 py-5">
              <div className="grid gap-4 md:grid-cols-2">
                <DetailField label="Occurrence ID" value={occurrenceId} />
                <CreatorField
                  userName={creatorSummary?.user_name ?? null}
                  userId={creatorSummary?.user_id ?? creatorUserId}
                />
              </div>

              {rootQuads.length > 0 ? (
                <div className="mt-6 border-t border-[#eef2f3] pt-6">
                  <div className="grid gap-4 md:grid-cols-2">
                    {rootQuads.map((quad, index) => (
                      <DetailField
                        key={`${quad.subject}-${quad.predicate}-${quad.object}-${index}`}
                        label={predicateLabel(quad.predicate)}
                        value={formatQuadValue(quad.predicate, quad.object)}
                      />
                    ))}
                  </div>
                </div>
              ) : null}
            </section>

            {intermediateSections.map((section) => (
              <section key={section.subject} className="rounded-md border border-[#d8dfe2] bg-white px-5 py-5">
                <h2 className="text-sm font-medium text-[#526168]">{section.title}</h2>
                <div className="mt-4 grid gap-4 md:grid-cols-2">
                  {section.quads.map((quad, index) => (
                    <DetailField
                      key={`${quad.subject}-${quad.predicate}-${quad.object}-${index}`}
                      label={predicateLabel(quad.predicate)}
                      value={formatQuadValue(quad.predicate, quad.object)}
                    />
                  ))}
                </div>
              </section>
            ))}
          </div>
        ) : null}
      </main>
    </div>
  );
}

function DetailField({
  label,
  value,
}: {
  label: string;
  value: string | null;
}) {
  return (
    <div className="rounded-md border border-[#eef2f3] bg-[#fafcfc] px-4 py-3">
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
    <div className="rounded-md border border-[#eef2f3] bg-[#fafcfc] px-4 py-3">
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

function findQuadObject(nquads: string, predicateUri: string): string | null {
  for (const quad of parseNQuads(nquads)) {
    if (quad.predicate === predicateUri) {
      return normalizeObject(quad.object);
    }
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
    return null;
  }
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

function parseNQuads(nquads: string): ParsedQuad[] {
  return nquads
    .split(String.fromCharCode(10))
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
    .filter((quad): quad is ParsedQuad => quad !== null);
}

function buildIntermediateSections(quads: ParsedQuad[], occurrenceUri: string): IntermediateSection[] {
  const sections = new Map<string, IntermediateSection>();

  for (const quad of quads) {
    if (quad.subject !== occurrenceUri) continue;

    const title = intermediateRelationLabel(quad.predicate);
    if (!title) continue;

    const subjectKey = normalizeObject(quad.object);
    if (!sections.has(subjectKey)) {
      sections.set(subjectKey, {
        subject: subjectKey,
        title,
        quads: [],
      });
    }
  }

  for (const quad of quads) {
    if (isIntermediateRelationPredicate(quad.predicate) || quad.predicate === "http://www.w3.org/1999/02/22-rdf-syntax-ns#type") {
      continue;
    }

    const section = sections.get(quad.subject);
    if (!section) continue;

    section.quads.push(quad);
  }

  return Array.from(sections.values()).filter((section) => section.quads.length > 0);
}

function isIntermediateRelationPredicate(predicateUri: string): boolean {
  return intermediateRelationLabel(predicateUri) !== null;
}

function intermediateRelationLabel(predicateUri: string): string | null {
  const labels: Record<string, string> = {
    "https://bio-database.net/terms/hasIdentification": "Identification",
    "https://bio-database.net/terms/hasEvent": "Event",
    "https://bio-database.net/terms/hasLocation": "Location",
  };

  return labels[predicateUri] ?? null;
}

const XSD_DATE_TIME_URI = "http://www.w3.org/2001/XMLSchema#dateTime";

function formatQuadValue(predicate: string, object: string): string {
  const normalized = normalizeObject(object);

  if (predicate === "http://purl.org/dc/terms/created" || predicate === "http://purl.org/dc/terms/modified") {
    return formatLocalDateTime(stripDatatype(normalized, XSD_DATE_TIME_URI));
  }

  return normalized;
}

function stripDatatype(value: string, datatypeUri: string): string {
  const suffix = `^^<${datatypeUri}>`;
  if (value.endsWith(suffix)) {
    return value.slice(0, -suffix.length);
  }

  return value;
}

function formatLocalDateTime(value: string): string {
  const trimmed = value.trim();
  const normalized = trimmed.startsWith("\"") && trimmed.endsWith("\"")
    ? trimmed.slice(1, -1)
    : trimmed;
  const date = new Date(normalized);
  if (Number.isNaN(date.getTime())) {
    return normalized.endsWith("Z") ? normalized.slice(0, -1) : normalized;
  }

  return new Intl.DateTimeFormat("ja-JP", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
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

