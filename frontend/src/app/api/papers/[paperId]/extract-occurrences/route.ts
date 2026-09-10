const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

export const runtime = "nodejs";

type RouteContext = { params: Promise<{ paperId: string }> };

export async function POST(request: Request, context: RouteContext) {
  const { paperId } = await context.params;
  console.log(`paper occurrence extraction requested: ${paperId}`);

  return proxyBackend(
    `${backendUrl}/paper-sources/paper/${encodeURIComponent(paperId)}/extract-occurrences`,
    "POST",
    request.headers.get("cookie"),
  );
}

export async function GET(request: Request, context: RouteContext) {
  const { paperId } = await context.params;

  return proxyBackend(
    `${backendUrl}/paper-sources/paper/${encodeURIComponent(paperId)}/extract-occurrences/status`,
    "GET",
    request.headers.get("cookie"),
  );
}

async function proxyBackend(url: string, method: "GET" | "POST", cookie: string | null) {
  try {
    const upstream = await fetch(url, {
      method,
      headers: cookie ? { cookie } : undefined,
      cache: "no-store",
    });
    const body = await upstream.text();
    const contentType = upstream.headers.get("content-type");

    return new Response(body, {
      status: upstream.status,
      headers: contentType ? { "content-type": contentType } : undefined,
    });
  } catch (error) {
    console.error("paper occurrence extraction backend request failed", error);
    return Response.json(
      {
        error: "backend_unavailable",
        message: "Failed to reach the backend during occurrence extraction",
      },
      { status: 502 },
    );
  }
}
