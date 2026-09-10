const API_PREFIX = "/api/backend";
const PAPER_BATCH_JOB_POLL_INTERVAL_MS = 3000;

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly body: unknown) {
    super(message);
    this.name = "ApiError";
  }
}

type PaperBatchJobStatus = {
  paper_id: string;
  status: "not_started" | "processing" | "completed" | "failed";
  phase?: "geocoding" | "registering";
  processed_occurrences?: number;
  total_occurrences?: number;
  occurrences?: unknown[];
  error?: string;
  message?: string;
};

/**
 * Calls the Rust backend through the same-origin Next.js layer.
 * Long-running paper batch registration is started as a backend job and then
 * polled with short requests so no proxy connection stays open for the whole
 * ABR/Nominatim/Fuseki operation.
 * Authentication uses the backend's session cookie, so credentials are always
 * included here rather than relying on every caller to remember that option.
 */
export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const normalizedPath = normalizePath(path);

  if (isPaperBatchRegistration(normalizedPath, init)) {
    return runPaperBatchRegistration<T>(normalizedPath, init);
  }

  const response = await fetch(resolveApiUrl(normalizedPath), {
    ...init,
    credentials: "include",
  });

  if (!response.ok) {
    const body = await readResponseBody(response);
    throw new ApiError(`Backend request failed with status ${response.status}`, response.status, body);
  }
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function runPaperBatchRegistration<T>(path: string, init: RequestInit): Promise<T> {
  const jobPath = `${path}-jobs`;
  const startResponse = await fetch(`${API_PREFIX}${jobPath}`, {
    ...init,
    credentials: "include",
  });

  if (!startResponse.ok) {
    const body = await readResponseBody(startResponse);
    throw new ApiError(
      `Backend request failed with status ${startResponse.status}`,
      startResponse.status,
      body,
    );
  }

  for (;;) {
    await sleep(PAPER_BATCH_JOB_POLL_INTERVAL_MS);

    const statusResponse = await fetch(`${API_PREFIX}${jobPath}/status`, {
      method: "GET",
      credentials: "include",
      cache: "no-store",
    });

    if (!statusResponse.ok) {
      const body = await readResponseBody(statusResponse);
      throw new ApiError(
        `Backend request failed with status ${statusResponse.status}`,
        statusResponse.status,
        body,
      );
    }

    const job = (await statusResponse.json()) as PaperBatchJobStatus;
    if (job.status === "processing") continue;

    if (job.status === "completed") {
      return { occurrences: job.occurrences ?? [] } as unknown as T;
    }

    if (job.status === "failed") {
      const status = registrationJobErrorStatus(job.error);
      throw new ApiError(job.message ?? "Paper batch registration failed", status, job);
    }

    throw new ApiError("Paper batch registration job was not started", 409, job);
  }
}

function isPaperBatchRegistration(path: string, init: RequestInit): boolean {
  return (
    /^\/papers\/[^/]+\/occurrences\/batch$/.test(path) &&
    (init.method ?? "GET").toUpperCase() === "POST"
  );
}

function registrationJobErrorStatus(error: string | undefined): number {
  switch (error) {
    case "invalid_session":
      return 401;
    case "invalid_paper_occurrence":
    case "invalid_rdf":
      return 400;
    case "paper_not_found":
      return 404;
    case "forbidden_media":
      return 403;
    case "rdf_store_error":
      return 502;
    default:
      return 500;
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function resolveApiUrl(path: string): string {
  if (/^\/paper-imports\/[^/]+\/extract-occurrences$/.test(path)) {
    return `/api${path}`;
  }
  return `${API_PREFIX}${path}`;
}

function normalizePath(path: string): string {
  return path.startsWith("/") ? path : `/${path}`;
}

async function readResponseBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get("content-type");
  if (contentType?.includes("application/json")) return response.json();
  return response.text();
}
