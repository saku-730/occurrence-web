const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

export const runtime = "nodejs";
export const maxDuration = 1800;

export async function POST(
  request: Request,
  context: { params: Promise<{ sourceKind: string; sourceId: string }> },
) {
  const { sourceKind, sourceId } = await context.params;
  const cookie = request.headers.get("cookie");

  if (!/^(import|paper)$/.test(sourceKind)) {
    return Response.json(
      { error: "invalid_paper_source", message: "Invalid paper source" },
      { status: 400 },
    );
  }

  console.log(`paper source extraction started: ${sourceKind}/${sourceId}`);

  try {
    const upstream = await fetch(
      `${backendUrl}/paper-sources/${encodeURIComponent(sourceKind)}/${encodeURIComponent(sourceId)}/extract-occurrences`,
      {
        method: "POST",
        headers: cookie ? { cookie } : undefined,
        cache: "no-store",
      },
    );

    const body = await upstream.text();
    const contentType = upstream.headers.get("content-type");
    console.log(
      `paper source extraction finished: ${sourceKind}/${sourceId} status=${upstream.status}`,
    );

    return new Response(body, {
      status: upstream.status,
      headers: contentType ? { "content-type": contentType } : undefined,
    });
  } catch (error) {
    console.error("paper source extraction upstream request failed", error);
    return Response.json(
      {
        error: "backend_unavailable",
        message: "Failed to reach the backend during occurrence extraction",
      },
      { status: 502 },
    );
  }
}
