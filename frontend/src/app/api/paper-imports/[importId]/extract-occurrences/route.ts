const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

// Long-running paper occurrence extraction must not use the generic external
// rewrite proxy. This Route Handler waits directly for the Rust backend while
// forwarding the browser's session cookie.
export const runtime = "nodejs";
export const maxDuration = 1800;

export async function POST(
  request: Request,
  context: { params: Promise<{ importId: string }> },
) {
  const { importId } = await context.params;
  const cookie = request.headers.get("cookie");
  const upstreamUrl = `${backendUrl}/paper-imports/${encodeURIComponent(importId)}/extract-occurrences`;

  console.info(`paper occurrence extraction started: ${importId}`);

  try {
    const upstream = await fetch(upstreamUrl, {
      method: "POST",
      headers: cookie ? { cookie } : undefined,
      cache: "no-store",
    });

    const body = await upstream.text();
    const contentType = upstream.headers.get("content-type");

    console.info(
      `paper occurrence extraction finished: ${importId} status=${upstream.status}`,
    );

    return new Response(body, {
      status: upstream.status,
      headers: contentType ? { "content-type": contentType } : undefined,
    });
  } catch (error) {
    console.error(`paper occurrence extraction failed: ${importId}`, error);
    return Response.json(
      {
        error: "backend_unavailable",
        message: "Failed to reach the backend during occurrence extraction",
      },
      { status: 502 },
    );
  }
}
