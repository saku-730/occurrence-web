const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

// Paper occurrence extraction can take several minutes while PDF preprocessing
// and the local LLM run. Keep this endpoint out of Next.js external rewrites so
// the rewrite proxy does not terminate the upstream socket while Rust is still
// processing the request.
export const runtime = "nodejs";
export const maxDuration = 1800;

export async function POST(
  request: Request,
  context: { params: Promise<{ importId: string }> },
) {
  const { importId } = await context.params;
  const cookie = request.headers.get("cookie");

  try {
    const upstream = await fetch(
      `${backendUrl}/paper-imports/${encodeURIComponent(importId)}/extract-occurrences`,
      {
        method: "POST",
        headers: cookie ? { cookie } : undefined,
        cache: "no-store",
      },
    );

    const body = await upstream.text();
    const contentType = upstream.headers.get("content-type");

    return new Response(body, {
      status: upstream.status,
      headers: contentType ? { "content-type": contentType } : undefined,
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
