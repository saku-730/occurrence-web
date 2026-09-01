"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useMemo, useRef, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

const API_PREFIX = "/api/backend";
const USER_URI_PREFIX = "https://bio-database.net/users/";
const OCCURRENCE_GRAPH_URI = "https://bio-database.net/graphs/occurrences";
const ASSOCIATED_MEDIA_PREDICATE_URI = "http://rs.tdwg.org/dwc/terms/associatedMedia";
const MAX_MEDIA_SIZE_BYTES = 1000 * 1024 * 1024;
const DWCIRI_TO_TAXON_URI = "http://rs.tdwg.org/dwc/iri/toTaxon";
const DWCIRI_TO_TAXON_LABEL = "分類";
const DWC_SCIENTIFIC_NAME_URI = "http://rs.tdwg.org/dwc/terms/scientificName";
const GBIF_SUGGEST_ENDPOINT = "https://api.gbif.org/v1/species/suggest";
const GBIF_SPECIES_URI_PREFIX = "https://www.gbif.org/species/";
const GBIF_SUGGEST_DEBOUNCE_MS = 300;
const EDIT_MEDIA_FILE_INPUT_ID = "edit-occurrence-media-files";
const CREATOR_PREDICATE = "http://purl.org/dc/terms/creator";
const CREATED_PREDICATE = "http://purl.org/dc/terms/created";
const MODIFIED_PREDICATE = "http://purl.org/dc/terms/modified";
const RDF_TYPE_PREDICATE_URI = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const INTERMEDIATE_RELATION_PREDICATES = new Set([
  "https://bio-database.net/terms/hasIdentification",
  "https://bio-database.net/terms/hasEvent",
  "https://bio-database.net/terms/hasLocation",
]);

interface DarwinCoreTerm {
  uri: string;
  local_name: string;
}

interface StatementRow {
  id: number;
  predicate: string;
  object: string;
}

interface ExistingMediaAttachment {
  mediaUri: string;
  mediaId: string;
  fetchUrl: string;
  unlinkPending: boolean;
}

interface UploadMediaResponse {
  media_id: string;
  media_uri: string;
  bucket: string;
  object_key: string;
  content_type: string;
  size_bytes: number;
  original_filename: string | null;
}

interface UpdateOccurrenceResponse {
  occurrence_id: string;
  occurrence_uri: string;
}

interface OccurrenceDetailResponse {
  nquads: string;
}

interface CurrentUser {
  user_id: string;
}

interface OccurrenceCreatorSummary {
  user_id: string;
  user_name: string;
}
interface GbifSpeciesSuggestion {
  key: number;
  scientificName: string;
  canonicalName?: string;
}


type AuthStatus = "loading" | "authenticated" | "unauthenticated" | "error";

const emptyRows: StatementRow[] = [
  { id: 1, predicate: DWCIRI_TO_TAXON_URI, object: "" },
  { id: 2, predicate: "", object: "" },
];

export default function EditOccurrencePage() {
  const router = useRouter();
  const params = useParams<{ occurrence_id?: string }>();
  const occurrenceId = params?.occurrence_id ?? "";

  const [rows, setRows] = useState(emptyRows);
  const nextId = useRef(3);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [darwinCoreTerms, setDarwinCoreTerms] = useState<DarwinCoreTerm[]>([]);
  const [termsStatus, setTermsStatus] = useState<"idle" | "loading" | "loaded" | "error">("idle");
  const [taxonScientificNames, setTaxonScientificNames] = useState<Record<number, string>>({});
  const [selectedFiles, setSelectedFiles] = useState<File[]>([]);
  const [existingMediaAttachments, setExistingMediaAttachments] = useState<ExistingMediaAttachment[]>([]);
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submissionMessage, setSubmissionMessage] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [updatedOccurrence, setUpdatedOccurrence] = useState<UpdateOccurrenceResponse | null>(null);
  const [creatorSummary, setCreatorSummary] = useState<OccurrenceCreatorSummary | null>(null);
  const [currentUserId, setCurrentUserId] = useState<string | null>(null);
  const [detailStatus, setDetailStatus] = useState<"loading" | "ready" | "not_found" | "error">("loading");
  const [detailNQuads, setDetailNQuads] = useState("");

  useEffect(() => {
    let active = true;

    apiFetch<CurrentUser>("/auth/me")
      .then((user) => {
        if (active) setCurrentUserId(user.user_id);
        if (active) setAuthStatus("authenticated");
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (error instanceof ApiError && error.status === 401) {
          setAuthStatus("unauthenticated");
          return;
        }
        setAuthStatus("error");
      });

    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    if (!occurrenceId) {
      setDetailStatus("not_found");
      setDetailNQuads("");
      return;
    }

    let active = true;

    async function loadOccurrence() {
      setDetailStatus("loading");
      setDetailNQuads("");

      try {
        const response = await fetch(`${API_PREFIX}/occurrences/${occurrenceId}`, {
          cache: "no-store",
          credentials: "include",
        });

        if (response.status === 404) {
          if (!active) return;
          setDetailStatus("not_found");
          setDetailNQuads("");
          return;
        }

        if (!response.ok) {
          if (!active) return;
          setDetailStatus("error");
          setDetailNQuads("");
          return;
        }

        const nquads = await response.text();
        if (!active) return;
        setDetailStatus("ready");
        setDetailNQuads(nquads);
      } catch {
        if (!active) return;
        setDetailStatus("error");
        setDetailNQuads("");
      }
    }

    void loadOccurrence();

    return () => {
      active = false;
    };
  }, [occurrenceId]);

  useEffect(() => {
    if (detailStatus !== "ready") return;

    const loadedRows = buildEditableRowsFromNQuads(detailNQuads);
    setRows(loadedRows.length > 0 ? loadedRows : emptyRows.map((row) => ({ ...row })));
    setExistingMediaAttachments(buildExistingMediaAttachmentsFromNQuads(detailNQuads));
    nextId.current = loadedRows.length > 0 ? Math.max(...loadedRows.map((row) => row.id)) + 1 : 3;
    setSelectedFiles([]);
    if (fileInputRef.current) fileInputRef.current.value = "";
  }, [detailNQuads, detailStatus]);

  const creatorUserUri = useMemo(() => findQuadObject(detailNQuads, CREATOR_PREDICATE), [detailNQuads]);
  const creatorUserId = useMemo(() => extractUserIdFromUserUri(creatorUserUri), [creatorUserUri]);
  const canEditOccurrence = creatorUserId !== null && currentUserId === creatorUserId;
  const occurrenceUri = `https://bio-database.net/occurrences/${occurrenceId}`;

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

        const user = (await response.json()) as OccurrenceCreatorSummary;
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

  async function loadDarwinCoreTerms() {
    if (termsStatus !== "idle") return;
    setTermsStatus("loading");

    try {
      const terms = await apiFetch<DarwinCoreTerm[]>("/vocabularies/darwin-core");
      // 編集でもbackendから返った語彙候補は制限せず表示し、toTaxonだけは「分類」候補へ一本化する。
      const visibleTerms = terms.filter((term) => term.uri !== DWCIRI_TO_TAXON_URI);
      setDarwinCoreTerms([
        { uri: DWCIRI_TO_TAXON_URI, local_name: DWCIRI_TO_TAXON_LABEL },
        ...visibleTerms,
      ]);
      setTermsStatus("loaded");
    } catch {
      setTermsStatus("error");
    }
  }

  function updateRow(id: number, field: "predicate" | "object", value: string) {
    setRows((currentRows) =>
      currentRows.map((row) =>
        row.id === id ? { ...row, [field]: value } : row,
      ),
    );

    if (field === "predicate" && value.trim() !== DWCIRI_TO_TAXON_URI) {
      setTaxonScientificNames((current) => {
        if (!(id in current)) return current;
        const next = { ...current };
        delete next[id];
        return next;
      });
    }
  }

  function addRow() {
    setRows((currentRows) => [...currentRows, { id: nextId.current++, predicate: "", object: "" }]);
  }

  function removeRow(id: number) {
    setRows((currentRows) => currentRows.filter((row) => row.id !== id));
  }

  function addFiles(files: FileList | null) {
    if (!files || files.length === 0) return;

    // FileList is tied to the native input. Snapshot it before clearing the input,
    // otherwise React may read an already-cleared list when applying the state update.
    const selectedFileSnapshot = Array.from(files);

    setErrorMessage(null);
    setUpdatedOccurrence(null);

    setSelectedFiles((currentFiles) => {
      const knownFiles = new Set(currentFiles.map((file) => fileIdentity(file)));
      const addedFiles = selectedFileSnapshot.filter((file) => !knownFiles.has(fileIdentity(file)));
      return [...currentFiles, ...addedFiles];
    });

    // いったん入力を空に戻しておくと、同じファイルを再選択したときに UI が詰まりにくい。
    if (fileInputRef.current) fileInputRef.current.value = "";
  }

  function removeFile(target: File) {
    setSelectedFiles((currentFiles) => currentFiles.filter((file) => fileIdentity(file) !== fileIdentity(target)));
  }

  function toggleExistingMediaUnlink(mediaUri: string) {
    setExistingMediaAttachments((currentAttachments) =>
      currentAttachments.map((attachment) =>
        attachment.mediaUri === mediaUri
          ? { ...attachment, unlinkPending: !attachment.unlinkPending }
          : attachment,
      ),
    );
  }

  async function deleteOccurrence() {
    const confirmed = window.confirm("このデータを削除します。元には戻せません。よろしいですか？");
    if (!confirmed) return;

    setErrorMessage(null);
    setUpdatedOccurrence(null);
    setSubmissionMessage("データを削除しています");
    setIsSubmitting(true);

    try {
      await apiFetch<void>(`/occurrences/${occurrenceId}`, {
        method: "DELETE",
      });

      router.push("/occurrences/search");
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        setAuthStatus("unauthenticated");
      } else if (error instanceof ApiError && error.status === 403) {
        setErrorMessage("このデータを削除する権限がありません");
      } else if (error instanceof ApiError && error.status === 404) {
        setErrorMessage("データが見つかりませんでした");
      } else {
        setErrorMessage("削除処理に失敗しました");
      }
      setSubmissionMessage(null);
    } finally {
      setIsSubmitting(false);
    }
  }

  async function submitOccurrence() {
    setErrorMessage(null);
    setUpdatedOccurrence(null);

    let statements: StatementRow[];
    try {
      statements = validateStatementRows(rows);
      statements = normalizeTaxonStatements(statements, taxonScientificNames);
      validateSelectedFiles(selectedFiles);
    } catch (error) {
      setErrorMessage(error instanceof Error ? error.message : "入力内容を確認してください");
      return;
    }

    if (statements.length === 0 && selectedFiles.length === 0 && existingMediaAttachments.length === 0) {
      setErrorMessage("項目または添付ファイルを1つ以上入力してください");
      return;
    }

    setIsSubmitting(true);

    try {
      const mediaUris: string[] = [];
      for (const [index, file] of selectedFiles.entries()) {
        setSubmissionMessage(`ファイルをアップロードしています (${index + 1}/${selectedFiles.length})`);
        const formData = new FormData();
        formData.append("file", file, file.name);

        const uploaded = await apiFetch<UploadMediaResponse>("/media", {
          method: "POST",
          body: formData,
        });
        mediaUris.push(uploaded.media_uri);
      }

      setSubmissionMessage("データを更新しています");
      const retainedMediaUris = existingMediaAttachments
        .filter((attachment) => !attachment.unlinkPending)
        .map((attachment) => attachment.mediaUri);
      const nquads = buildOccurrenceNQuads(statements, [...retainedMediaUris, ...mediaUris]);
      const updated = await apiFetch<UpdateOccurrenceResponse>(`/occurrences/${occurrenceId}`, {
        method: "PUT",
        headers: { "Content-Type": "application/n-quads" },
        body: nquads,
      });

      setUpdatedOccurrence(updated);
      setExistingMediaAttachments(buildExistingMediaAttachmentsFromUris([...retainedMediaUris, ...mediaUris]));
      setSubmissionMessage(null);
      setSelectedFiles([]);
      if (fileInputRef.current) fileInputRef.current.value = "";
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        setAuthStatus("unauthenticated");
      } else if (error instanceof ApiError && error.status === 403) {
        setErrorMessage("このデータを更新する権限がありません");
      } else if (error instanceof ApiError && error.status === 404) {
        setErrorMessage("データが見つかりませんでした");
      } else {
        setErrorMessage(registrationErrorMessage(error));
      }
      setSubmissionMessage(null);
    } finally {
      setIsSubmitting(false);
    }
  }

  if (detailStatus === "loading" || authStatus === "loading") {
    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">データ編集</h1>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">データを読み込んでいます</p>
          </section>
        </main>
      </div>
    );
  }

  if (detailStatus === "not_found") {
    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">データ編集</h1>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">データが見つかりませんでした</p>
          </section>
        </main>
      </div>
    );
  }

  if (detailStatus === "error") {
    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">データ編集</h1>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">詳細データを取得できませんでした</p>
          </section>
        </main>
      </div>
    );
  }

  if (authStatus !== "authenticated" || !canEditOccurrence) {
    const message = authStatus === "unauthenticated"
      ? "編集するにはログインが必要です"
      : "このデータを編集する権限がありません";

    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
            <div>
              <h1 className="text-2xl font-semibold">データ編集</h1>
              <p className="mt-2 text-sm text-[#65737a]">データの作成者だけが編集できます。</p>
            </div>
            <Link
              className="inline-flex h-10 items-center rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746]"
              href={`/occurrences/${occurrenceId}`}
            >
              詳細へ戻る
            </Link>
          </div>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">{message}</p>
          </section>
        </main>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
        <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
          <div>
            <h1 className="text-2xl font-semibold">データ編集</h1>
            <p className="mt-2 text-sm text-[#65737a]">既存データを読み込んで編集します。</p>
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
              href={`/occurrences/${occurrenceId}`}
            >
              詳細へ
            </Link>
          </div>
        </div>

        <section className="rounded-md border border-[#d8dfe2] bg-white px-5 py-5">
          <div className="grid gap-4 md:grid-cols-2">
            <DetailField label="Occurrence ID" value={occurrenceId} />
            <CreatorField
              userName={creatorSummary?.user_name ?? null}
              userId={creatorSummary?.user_id ?? creatorUserId}
            />
          </div>
        </section>

        <section className="mt-6 overflow-visible rounded-md border border-[#d8dfe2] bg-white">
          <div className="hidden grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-4 border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3 text-xs font-medium text-[#526168] md:grid">
            <span>項目名</span>
            <span>値</span>
            <span className="w-12" aria-hidden="true" />
          </div>

          <div className="divide-y divide-[#e4e9eb]">
            {rows.map((row, index) => (
              <div
                className="grid gap-4 px-5 py-5 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] md:items-end"
                key={row.id}
              >
                <div className="min-w-0">
                  <span className="mb-2 block text-sm font-medium md:sr-only">項目名 {index + 1}</span>
                  <PredicateCombobox
                    disabled={isSubmitting}
                    onOpen={() => void loadDarwinCoreTerms()}
                    onSelect={(uri) => updateRow(row.id, "predicate", uri)}
                    terms={darwinCoreTerms}
                    termsStatus={termsStatus}
                    value={row.predicate}
                  />
                </div>

                <ObjectValueField
                  disabled={isSubmitting}
                  onChange={(value) => updateRow(row.id, "object", value)}
                  onTaxonSelect={(scientificName) =>
                    setTaxonScientificNames((current) => {
                      if (!scientificName.trim()) {
                        if (!(row.id in current)) return current;
                        const next = { ...current };
                        delete next[row.id];
                        return next;
                      }

                      return { ...current, [row.id]: scientificName };
                    })
                  }
                  predicate={row.predicate}
                  value={row.object}
                />

                <button
                  className="h-10 w-fit px-1 text-sm text-[#a23c32] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline md:w-12"
                  disabled={rows.length === 1 || isSubmitting}
                  onClick={() => removeRow(row.id)}
                  type="button"
                >
                  削除
                </button>
              </div>
            ))}
          </div>

          <div className="border-t border-[#d8dfe2] px-5 py-4">
            <button
              className="text-sm font-medium text-[#176b57] hover:underline disabled:cursor-not-allowed disabled:text-[#9aa5aa] disabled:no-underline"
              disabled={isSubmitting}
              onClick={addRow}
              type="button"
            >
              入力行を追加
            </button>
          </div>
        </section>

        <section className="mt-6 overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
            <h2 className="text-sm font-medium text-[#526168]">添付ファイル</h2>
          </div>

          <div className="px-5 py-5">
            <input
              accept=".jpg,.jpeg,.png,.webp,.mp3,.wav,.m4a,.mp4,.mov"
              className="sr-only"
              disabled={isSubmitting}
              id={EDIT_MEDIA_FILE_INPUT_ID}
              multiple
              onChange={(event) => addFiles(event.target.files)}
              ref={fileInputRef}
              type="file"
            />
            <label
              aria-disabled={isSubmitting}
              className="inline-flex h-10 cursor-pointer items-center rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] aria-disabled:cursor-not-allowed aria-disabled:bg-[#eef2f3] aria-disabled:text-[#7c898f]"
              htmlFor={isSubmitting ? undefined : EDIT_MEDIA_FILE_INPUT_ID}
            >
              ファイルを追加
            </label>

            {existingMediaAttachments.length > 0 ? (
              <div className="mt-5">
                <p className="text-xs font-medium text-[#65737a]">既存の添付ファイル</p>
                <div className="mt-3 grid gap-4 md:grid-cols-2">
                  {existingMediaAttachments.map((attachment) => (
                    <ExistingMediaAttachmentPreview
                      attachment={attachment}
                      disabled={isSubmitting}
                      key={attachment.mediaUri}
                      onToggleUnlink={() => toggleExistingMediaUnlink(attachment.mediaUri)}
                    />
                  ))}
                </div>
              </div>
            ) : null}

            {selectedFiles.length > 0 ? (
              <div className="mt-5">
                <p className="text-xs font-medium text-[#65737a]">追加予定のファイル</p>
                <div className="mt-3 grid gap-4 md:grid-cols-2">
                  {selectedFiles.map((file) => (
                    <SelectedMediaFilePreview
                      disabled={isSubmitting}
                      file={file}
                      key={fileIdentity(file)}
                      onRemove={() => removeFile(file)}
                    />
                  ))}
                </div>
              </div>
            ) : null}
          </div>
        </section>

        {errorMessage ? (
          <p className="mt-5 text-sm text-[#a23c32]" role="alert">{errorMessage}</p>
        ) : null}

        {updatedOccurrence ? (
          <section className="mt-6 rounded-md border border-[#9dbeb4] bg-white px-5 py-4" aria-live="polite">
            <p className="font-medium">データを更新しました</p>
            <p className="mt-2 break-all text-sm text-[#526168]">{updatedOccurrence.occurrence_uri}</p>
          </section>
        ) : null}

        <div className="mt-6 flex justify-end gap-3">
          <button
            className="h-10 rounded-md border border-[#a23c32] bg-white px-6 text-sm font-medium text-[#a23c32] hover:bg-[#fff4f2] disabled:cursor-wait disabled:border-[#d6a39b] disabled:text-[#d6a39b]"
            disabled={isSubmitting}
            onClick={() => void deleteOccurrence()}
            type="button"
          >
            削除
          </button>
          <button
            className="h-10 rounded-md bg-[#176b57] px-6 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-wait disabled:bg-[#829b95]"
            disabled={isSubmitting}
            onClick={() => void submitOccurrence()}
            type="button"
          >
            {submissionMessage ?? "更新"}
          </button>
        </div>
      </main>
    </div>
  );
}

function DetailField({ label, value }: { label: string; value: string | null }) {
  return (
    <div className="rounded-md border border-[#eef2f3] bg-[#fafcfc] px-4 py-3">
      <p className="text-xs font-medium uppercase tracking-wide text-[#65737a]">{label}</p>
      <p className="mt-2 break-all text-sm text-[#182126]">{value ?? "-"}</p>
    </div>
  );
}

function CreatorField({ userName, userId }: { userName: string | null; userId: string | null; }) {
  const primary = userName ?? userId ?? "-";

  return (
    <div className="rounded-md border border-[#eef2f3] bg-[#fafcfc] px-4 py-3">
      <p className="text-xs font-medium uppercase tracking-wide text-[#65737a]">作成者</p>
      <p className="mt-2 break-all text-sm text-[#182126]">{primary}</p>
      {userName && userId ? <p className="mt-1 break-all text-xs text-[#65737a]">ID: {userId}</p> : null}
    </div>
  );
}

function ExistingMediaAttachmentPreview({
  attachment,
  disabled,
  onToggleUnlink,
}: {
  attachment: ExistingMediaAttachment;
  disabled: boolean;
  onToggleUnlink: () => void;
}) {
  const [state, setState] = useState<
    | { status: "loading" }
    | { status: "ready"; objectUrl: string; contentType: string }
    | { status: "error" }
  >({ status: "loading" });

  useEffect(() => {
    let active = true;
    let objectUrl: string | null = null;

    async function loadMedia() {
      setState({ status: "loading" });

      try {
        const response = await fetch(attachment.fetchUrl, {
          cache: "no-store",
          credentials: "include",
        });

        if (!response.ok) {
          if (active) setState({ status: "error" });
          return;
        }

        const blob = await response.blob();
        objectUrl = URL.createObjectURL(blob);
        const contentType = response.headers.get("content-type") ?? blob.type;

        if (active) {
          setState({ status: "ready", objectUrl, contentType });
        }
      } catch {
        if (active) setState({ status: "error" });
      }
    }

    void loadMedia();

    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [attachment.fetchUrl]);

  return (
    <div className={`rounded-md border border-[#eef2f3] bg-[#fafcfc] p-4 transition-opacity ${attachment.unlinkPending ? "opacity-45" : "opacity-100"}`}>
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="text-xs font-medium uppercase tracking-wide text-[#65737a]">Media ID</p>
          <p className="mt-1 break-all text-sm text-[#182126]">{attachment.mediaId}</p>
          <p className="mt-1 truncate text-xs text-[#65737a]">{attachment.mediaUri}</p>
        </div>
        <div className="flex shrink-0 gap-2">
          <a
            className="rounded-md border border-[#b8c3c8] bg-white px-3 py-2 text-xs font-medium hover:bg-[#eef2f3]"
            download
            href={attachment.fetchUrl}
          >
            ダウンロード
          </a>
          <button
            className={attachment.unlinkPending
              ? "rounded-md border border-[#9dbeb4] bg-white px-3 py-2 text-xs font-medium text-[#176b57] hover:bg-[#eef8f5] disabled:cursor-not-allowed disabled:text-[#9aa5aa]"
              : "rounded-md border border-[#d6a39b] bg-white px-3 py-2 text-xs font-medium text-[#a23c32] hover:bg-[#fff4f2] disabled:cursor-not-allowed disabled:text-[#d6a39b]"}
            disabled={disabled}
            onClick={onToggleUnlink}
            type="button"
          >
            {attachment.unlinkPending ? "紐付け" : "紐付け解除"}
          </button>
        </div>
      </div>

      {state.status === "loading" ? <MediaPreviewStatus message="読み込み中" /> : null}
      {state.status === "error" ? <MediaPreviewStatus error message="添付ファイルを取得できませんでした" /> : null}
      {state.status === "ready" ? (
        <MediaObjectRenderer contentType={state.contentType} objectUrl={state.objectUrl} />
      ) : null}
    </div>
  );
}

function SelectedMediaFilePreview({
  disabled,
  file,
  onRemove,
}: {
  disabled: boolean;
  file: File;
  onRemove: () => void;
}) {
  const [objectUrl, setObjectUrl] = useState<string | null>(null);

  useEffect(() => {
    const nextObjectUrl = URL.createObjectURL(file);
    setObjectUrl(nextObjectUrl);

    return () => URL.revokeObjectURL(nextObjectUrl);
  }, [file]);

  return (
    <div className="rounded-md border border-[#eef2f3] bg-[#fafcfc] p-4">
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{file.name}</p>
          <p className="mt-1 text-xs text-[#65737a]">{formatFileSize(file.size)}</p>
        </div>
        <div className="flex shrink-0 gap-2">
          {objectUrl ? (
            <a
              className="rounded-md border border-[#b8c3c8] bg-white px-3 py-2 text-xs font-medium hover:bg-[#eef2f3]"
              download={file.name}
              href={objectUrl}
            >
              ダウンロード
            </a>
          ) : null}
          <button
            className="rounded-md border border-[#d6a39b] bg-white px-3 py-2 text-xs font-medium text-[#a23c32] hover:bg-[#fff4f2] disabled:cursor-not-allowed disabled:text-[#d6a39b]"
            disabled={disabled}
            onClick={onRemove}
            type="button"
          >
            削除
          </button>
        </div>
      </div>

      {objectUrl ? (
        <MediaObjectRenderer contentType={file.type} objectUrl={objectUrl} />
      ) : (
        <MediaPreviewStatus message="読み込み中" />
      )}
    </div>
  );
}

function MediaObjectRenderer({ contentType, objectUrl }: { contentType: string; objectUrl: string }) {
  if (contentType.startsWith("image/")) {
    return (
      // eslint-disable-next-line @next/next/no-img-element
      <img
        alt="添付画像"
        className="max-h-[520px] w-full rounded-md border border-[#d8dfe2] object-contain"
        src={objectUrl}
      />
    );
  }

  if (contentType.startsWith("video/")) {
    return <video className="w-full rounded-md border border-[#d8dfe2] bg-black" controls src={objectUrl} />;
  }

  if (contentType.startsWith("audio/")) {
    return <audio className="w-full" controls src={objectUrl} />;
  }

  return (
    <div className="rounded-md border border-dashed border-[#c9d2d6] px-4 py-8 text-center">
      <p className="text-sm text-[#65737a]">プレビューできない形式です</p>
      <p className="mt-2 break-all text-xs text-[#7c898f]">{contentType || "application/octet-stream"}</p>
    </div>
  );
}

function MediaPreviewStatus({ error = false, message }: { error?: boolean; message: string }) {
  const textColor = error ? "text-[#a23c32]" : "text-[#65737a]";
  return (
    <div className={`grid min-h-36 place-items-center rounded-md border border-dashed border-[#c9d2d6] px-4 text-center text-sm ${textColor}`}>
      {message}
    </div>
  );
}

function PredicateCombobox({ disabled, onOpen, onSelect, terms, termsStatus, value }: { disabled: boolean; onOpen: () => void; onSelect: (uri: string) => void; terms: DarwinCoreTerm[]; termsStatus: "idle" | "loading" | "loaded" | "error"; value: string; }) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(() => predicateLabelForUri(value));

  useEffect(() => {
    setQuery(predicateLabelForUri(value, terms));
  }, [terms, value]);

  const sortedTerms = [...terms].sort((left, right) => {
    const byName = left.local_name.localeCompare(right.local_name, "ja", { sensitivity: "base" });
    return byName !== 0 ? byName : left.uri.localeCompare(right.uri, "en", { sensitivity: "base" });
  });
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredTerms = sortedTerms.filter((term) => !normalizedQuery || term.local_name.toLocaleLowerCase().includes(normalizedQuery) || term.uri.toLocaleLowerCase().includes(normalizedQuery));

  return (
    <div className="relative">
      <input
        aria-autocomplete="list"
        aria-expanded={isOpen}
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onBlur={() => window.setTimeout(() => setIsOpen(false), 120)}
        onChange={(event) => {
          const next = event.target.value;
          const normalizedNext = next.trim();
          setQuery(next);
          const exactMatch = sortedTerms.find((term) => term.local_name.toLocaleLowerCase() === normalizedNext.toLocaleLowerCase() || term.uri === normalizedNext);
          if (exactMatch) onSelect(exactMatch.uri);
          else if (isAbsoluteHttpUri(normalizedNext)) onSelect(normalizedNext);
          else onSelect("");
          setIsOpen(true);
        }}
        onFocus={() => { setIsOpen(true); onOpen(); }}
        placeholder="項目名"
        role="combobox"
        type="text"
        value={query}
      />
      {isOpen ? (
        <div className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg" role="listbox">
          {termsStatus === "loading" ? <p className="px-3 py-2 text-sm text-[#65737a]">読み込み中</p> : null}
          {termsStatus === "error" ? <p className="px-3 py-2 text-sm text-[#a23c32]">候補を取得できませんでした</p> : null}
          {termsStatus === "loaded" && filteredTerms.length === 0 ? <p className="px-3 py-2 text-sm text-[#65737a]">一致する項目はありません</p> : null}
          {filteredTerms.map((term) => (
            <button
              className="block w-full px-3 py-2 text-left hover:bg-[#eef2f3] focus:bg-[#eef2f3] focus:outline-none"
              key={term.uri}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => { onSelect(term.uri); setQuery(predicateLabelForUri(term.uri, terms)); setIsOpen(false); }}
              role="option"
              type="button"
            >
              <span className="block text-sm font-medium">{term.local_name}</span>
              <span className="block truncate text-xs text-[#65737a]">{term.uri}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ObjectValueField({ disabled, onChange, onTaxonSelect, predicate, value }: { disabled: boolean; onChange: (value: string) => void; onTaxonSelect: (scientificName: string) => void; predicate: string; value: string; }) {
  if (predicate === DWCIRI_TO_TAXON_URI) {
    return <TaxonValueCombobox disabled={disabled} onChange={onChange} onSelectScientificName={onTaxonSelect} value={value} />;
  }

  return (
    <label className="min-w-0">
      <span className="mb-2 block text-sm font-medium md:sr-only">値</span>
      <input
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onChange={(event) => onChange(event.target.value)}
        placeholder="値"
        type="text"
        value={value}
      />
    </label>
  );
}

function TaxonValueCombobox({ disabled, onChange, onSelectScientificName, value }: { disabled: boolean; onChange: (value: string) => void; onSelectScientificName: (scientificName: string) => void; value: string; }) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState(value);
  const [suggestions, setSuggestions] = useState<GbifSpeciesSuggestion[]>([]);
  const [suggestStatus, setSuggestStatus] = useState<"idle" | "loading" | "loaded" | "error">("idle");
  const lastSelectedLabel = useRef("");
  const lastSelectedValue = useRef("");

  useEffect(() => {
    if (value === lastSelectedValue.current && lastSelectedLabel.current) {
      setQuery(lastSelectedLabel.current);
      return;
    }
    setQuery(value);
  }, [value]);

  useEffect(() => {
    const trimmed = query.trim();
    if (trimmed.length < 3) {
      setSuggestions([]);
      setSuggestStatus("idle");
      return;
    }
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      setSuggestStatus("loading");
      void fetch(`${GBIF_SUGGEST_ENDPOINT}?q=${encodeURIComponent(trimmed)}`, { signal: controller.signal })
        .then(async (response) => {
          if (!response.ok) throw new Error("gbif suggest failed");
          const items = (await response.json()) as GbifSpeciesSuggestion[];
          setSuggestions(items.slice(0, 10));
          setSuggestStatus("loaded");
        })
        .catch(() => {
          if (controller.signal.aborted) return;
          setSuggestions([]);
          setSuggestStatus("error");
        });
    }, GBIF_SUGGEST_DEBOUNCE_MS);
    return () => { window.clearTimeout(timer); controller.abort(); };
  }, [query]);

  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredSuggestions = suggestions.filter((item) => !normalizedQuery || item.scientificName.toLocaleLowerCase().includes(normalizedQuery) || (item.canonicalName ?? "").toLocaleLowerCase().includes(normalizedQuery));

  return (
    <div className="relative min-w-0">
      <input
        aria-autocomplete="list"
        aria-expanded={isOpen}
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
        disabled={disabled}
        onBlur={() => window.setTimeout(() => setIsOpen(false), 120)}
        onChange={(event) => { const next = event.target.value; setQuery(next); onSelectScientificName(""); onChange(next); setIsOpen(true); }}
        onFocus={() => setIsOpen(true)}
        placeholder="分類"
        role="combobox"
        type="text"
        value={query}
      />
      {isOpen ? (
        <div className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-[#b8c3c8] bg-white py-1 shadow-lg" role="listbox">
          {suggestStatus === "loading" ? <p className="px-3 py-2 text-sm text-[#65737a]">GBIF候補を読み込み中</p> : null}
          {suggestStatus === "error" ? <p className="px-3 py-2 text-sm text-[#a23c32]">GBIF候補を取得できませんでした</p> : null}
          {suggestStatus === "loaded" && filteredSuggestions.length === 0 ? <p className="px-3 py-2 text-sm text-[#65737a]">一致する候補はありません</p> : null}
          {filteredSuggestions.map((item) => (
            <button
              className="block w-full px-3 py-2 text-left hover:bg-[#eef2f3] focus:bg-[#eef2f3] focus:outline-none"
              key={item.key}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => {
                const label = item.scientificName || item.canonicalName || String(item.key);
                const speciesUri = `${GBIF_SPECIES_URI_PREFIX}${item.key}`;
                lastSelectedLabel.current = label;
                lastSelectedValue.current = speciesUri;
                onSelectScientificName(label);
                onChange(speciesUri);
                setQuery(label);
                setIsOpen(false);
              }}
              role="option"
              type="button"
            >
              <span className="block text-sm font-medium">{item.scientificName}</span>
              <span className="block truncate text-xs text-[#65737a]">GBIF key: {item.key}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function predicateLabelForUri(value: string, terms: DarwinCoreTerm[] = []): string {
  if (value === DWCIRI_TO_TAXON_URI) {
    return DWCIRI_TO_TAXON_LABEL;
  }

  return terms.find((term) => term.uri === value)?.local_name ?? value;
}

// 分類の入力は、URIならtoTaxon、任意テキストならscientificNameとして保存する。
function normalizeTaxonStatements(
  statements: StatementRow[],
  scientificNamesByRowId: Record<number, string>,
): StatementRow[] {
  const normalizedStatements: StatementRow[] = [];
  const generatedScientificNames: StatementRow[] = [];

  for (const row of statements) {
    if (row.predicate !== DWCIRI_TO_TAXON_URI) {
      normalizedStatements.push(row);
      continue;
    }

    if (!isAbsoluteHttpUri(row.object)) {
      normalizedStatements.push({ ...row, predicate: DWC_SCIENTIFIC_NAME_URI });
      continue;
    }

    normalizedStatements.push(row);
    const scientificName = scientificNamesByRowId[row.id];
    if (scientificName?.trim()) {
      generatedScientificNames.push({
        id: statements.length + generatedScientificNames.length + 1,
        predicate: DWC_SCIENTIFIC_NAME_URI,
        object: scientificName.trim(),
      });
    }
  }

  return [...normalizedStatements, ...generatedScientificNames];
}

function validateStatementRows(rows: StatementRow[]): StatementRow[] {
  const statements: StatementRow[] = [];

  for (const row of rows) {
    const predicate = row.predicate.trim();
    const object = row.object.trim();

    if (!predicate && !object) continue;
    if (!predicate || !object) {
      throw new Error("項目名と値を両方入力してください");
    }
    if (!isAbsoluteHttpUri(predicate) || hasUnsafeIriCharacter(predicate)) {
      throw new Error("項目名には有効な絶対URIを入力してください");
    }
    if (isAbsoluteHttpUri(object) && hasUnsafeIriCharacter(object)) {
      throw new Error("値のURIに使用できない文字が含まれています");
    }

    statements.push({ ...row, predicate, object });
  }

  return statements;
}

function validateSelectedFiles(files: File[]) {
  if (files.some((file) => file.size > MAX_MEDIA_SIZE_BYTES)) {
    throw new Error("添付ファイルは1ファイル1000MB以下にしてください");
  }
}

function buildOccurrenceNQuads(statements: StatementRow[], mediaUris: string[]): string {
  const lines = statements.map((statement) => {
    const object = isAbsoluteHttpUri(statement.object)
      ? `<${statement.object}>`
      : `"${escapeRdfLiteral(statement.object)}"`;

    return `_:occurrence <${statement.predicate}> ${object} <${OCCURRENCE_GRAPH_URI}> .`;
  });

  for (const mediaUri of mediaUris) {
    lines.push(`_:occurrence <${ASSOCIATED_MEDIA_PREDICATE_URI}> <${mediaUri}> <${OCCURRENCE_GRAPH_URI}> .`);
  }

  return `${lines.join("\n")}\n`;
}


function buildExistingMediaAttachmentsFromNQuads(nquads: string): ExistingMediaAttachment[] {
  const mediaUris = parseNQuads(nquads)
    .filter((quad) => quad.predicate === ASSOCIATED_MEDIA_PREDICATE_URI)
    .map((quad) => normalizeObject(quad.object));

  return buildExistingMediaAttachmentsFromUris(mediaUris);
}

function buildExistingMediaAttachmentsFromUris(mediaUris: string[]): ExistingMediaAttachment[] {
  const attachments = new Map<string, ExistingMediaAttachment>();

  for (const mediaUri of mediaUris) {
    const mediaId = extractMediaId(mediaUri);
    if (!mediaId || attachments.has(mediaUri)) continue;

    attachments.set(mediaUri, {
      mediaUri,
      mediaId,
      fetchUrl: `${API_PREFIX}/media/${encodeURIComponent(mediaId)}`,
      unlinkPending: false,
    });
  }

  return Array.from(attachments.values());
}

function extractMediaId(mediaUri: string): string | null {
  try {
    const parsed = new URL(mediaUri);
    const segments = parsed.pathname.split("/").filter(Boolean);
    const mediaIndex = segments.lastIndexOf("media");
    const mediaId = mediaIndex >= 0 ? segments[mediaIndex + 1] : null;
    return mediaId && mediaId.length > 0 ? decodeURIComponent(mediaId) : null;
  } catch {
    return null;
  }
}

function buildEditableRowsFromNQuads(nquads: string): StatementRow[] {
  const rows: StatementRow[] = [];
  let id = 1;
  for (const quad of parseNQuads(nquads)) {
    if (
      quad.predicate === CREATOR_PREDICATE ||
      quad.predicate === CREATED_PREDICATE ||
      quad.predicate === MODIFIED_PREDICATE ||
      quad.predicate === RDF_TYPE_PREDICATE_URI ||
      quad.predicate === ASSOCIATED_MEDIA_PREDICATE_URI
    ) {
      continue;
    }
    if (INTERMEDIATE_RELATION_PREDICATES.has(quad.predicate)) {
      continue;
    }

    rows.push({
      id: id++,
      predicate: quad.predicate,
      object: editableObjectValue(quad.object),
    });
  }

  return rows;
}

function parseNQuads(nquads: string) {
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
    .filter((quad): quad is { subject: string; predicate: string; object: string; graph: string } => quad !== null);
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

function editableObjectValue(object: string): string {
  const normalized = normalizeObject(object);
  const withoutDatatype = stripDatatype(normalized, "http://www.w3.org/2001/XMLSchema#dateTime");

  if (withoutDatatype.startsWith('"') && withoutDatatype.endsWith('"')) {
    return unescapeRdfLiteral(withoutDatatype.slice(1, -1));
  }

  return withoutDatatype;
}

function normalizeObject(object: string): string {
  if (object.startsWith("<") && object.endsWith(">")) {
    return object.slice(1, -1);
  }

  if (object.startsWith('"') && object.endsWith('"')) {
    return object;
  }

  return object;
}

function stripDatatype(value: string, datatypeUri: string): string {
  const suffix = `^^<${datatypeUri}>`;
  if (value.endsWith(suffix)) {
    return value.slice(0, -suffix.length);
  }

  return value;
}

function unescapeRdfLiteral(value: string): string {
  return value
    .replaceAll("\\n", "\n")
    .replaceAll("\\r", "\r")
    .replaceAll("\\t", "\t")
    .replaceAll('\\"', '"')
    .replaceAll("\\\\", "\\");
}

function isAbsoluteHttpUri(value: string): boolean {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

function hasUnsafeIriCharacter(value: string): boolean {
  return /[<>"{}|^`\\\s]/u.test(value);
}

function escapeRdfLiteral(value: string): string {
  return value
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"')
    .replaceAll("\n", "\\n")
    .replaceAll("\r", "\\r")
    .replaceAll("\t", "\\t");
}

function registrationErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) {
    return "更新処理に失敗しました";
  }

  if (error.status === 400) {
    return "入力したRDFデータが不正です";
  }
  if (error.status === 403) {
    return "このデータを更新する権限がありません";
  }
  if (error.status === 413) {
    return "添付ファイルのサイズが上限を超えています";
  }
  if (error.status === 415) {
    return "対応していないファイル形式です";
  }
  if (error.status === 502) {
    return "データ保存先との通信に失敗しました";
  }

  return "更新処理に失敗しました";
}

function fileIdentity(file: File): string {
  return `${file.name}:${file.size}:${file.lastModified}`;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
}
