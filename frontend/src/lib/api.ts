const API_PREFIX = "/api/backend";

export class ApiError extends Error {
  constructor(message: string, readonly status: number, readonly body: unknown) {
    super(message);
    this.name = "ApiError";
  }
}

/**
 * Calls the Rust backend through the same-origin Next.js rewrite.
 * Authentication uses the backend's session cookie, so credentials are always
 * included here rather than relying on every caller to remember that option.
 */
export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(`${API_PREFIX}${normalizePath(path)}`, {
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

function normalizePath(path: string): string {
  return path.startsWith("/") ? path : `/${path}`;
}

async function readResponseBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get("content-type");
  if (contentType?.includes("application/json")) return response.json();
  return response.text();
}
