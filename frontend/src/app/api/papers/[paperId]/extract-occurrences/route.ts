import { request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";

const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");
const UPSTREAM_TIMEOUT_MS = 30 * 60 * 1000;

export const runtime = "nodejs";
export const maxDuration = 1800;

type UpstreamResponse = {
  status: number;
  contentType: string | undefined;
  body: string;
};

export async function POST(
  request: Request,
  context: { params: Promise<{ paperId: string }> },
) {
  const { paperId } = await context.params;
  const cookie = request.headers.get("cookie");

  console.log(`paper occurrence extraction started: ${paperId}`);

  try {
    const upstream = await requestBackend(
      `${backendUrl}/paper-sources/paper/${encodeURIComponent(paperId)}/extract-occurrences`,
      cookie,
    );

    console.log(
      `paper occurrence extraction finished: ${paperId} status=${upstream.status}`,
    );

    return new Response(upstream.body, {
      status: upstream.status,
      headers: upstream.contentType
        ? { "content-type": upstream.contentType }
        : undefined,
    });
  } catch (error) {
    console.error("paper occurrence extraction upstream request failed", error);
    return Response.json(
      {
        error: "backend_unavailable",
        message: "Failed to reach the backend during occurrence extraction",
      },
      { status: 502 },
    );
  }
}

function requestBackend(url: string, cookie: string | null): Promise<UpstreamResponse> {
  return new Promise((resolve, reject) => {
    const target = new URL(url);
    const requestFn = target.protocol === "https:" ? httpsRequest : httpRequest;

    const upstreamRequest = requestFn(
      target,
      {
        method: "POST",
        headers: cookie ? { cookie } : undefined,
      },
      (upstreamResponse) => {
        const chunks: Buffer[] = [];

        upstreamResponse.on("data", (chunk: Buffer | string) => {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
        });
        upstreamResponse.on("end", () => {
          resolve({
            status: upstreamResponse.statusCode ?? 502,
            contentType:
              typeof upstreamResponse.headers["content-type"] === "string"
                ? upstreamResponse.headers["content-type"]
                : undefined,
            body: Buffer.concat(chunks).toString("utf8"),
          });
        });
        upstreamResponse.on("error", reject);
      },
    );

    // Node/Undici fetch has a roughly five-minute headers timeout. The Rust
    // endpoint may legitimately return no headers until llama.cpp finishes, so
    // use the raw Node HTTP client and allow the same 30-minute ceiling as the
    // backend LLM request instead.
    upstreamRequest.setTimeout(UPSTREAM_TIMEOUT_MS, () => {
      upstreamRequest.destroy(
        new Error("paper occurrence extraction upstream request timed out"),
      );
    });

    upstreamRequest.on("error", reject);
    upstreamRequest.end();
  });
}
