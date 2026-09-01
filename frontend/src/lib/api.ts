const API_PREFIX = "/api/backend";

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly body: unknown) {
    super(message);
    this.name = "ApiError";
  }
}

/**
 * Calls the Rust backend through the same-origin Next.js layer.
 * Most endpoints use the generic external rewrite. Long-running paper
 * occurrence extraction uses a dedicated Route Handler instead so the rewrite
 * proxy cannot reset the socket while the local LLM is still processing.
 * Authentication uses the backend's session cookie, so credentials are always
 * included here rather than relying on every caller to remember that option.
 */
export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const normalizedPath = normalizePath(path);
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
