const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

export const runtime = "nodejs";
export const maxDuration = 1800;

export async function POST(
  request: Request,
  context: { params: Promise<{ paperId: string }> },
) {
  const { paperId } = await context.params;
  const cookie = request.headers.get("cookie");

  console.log(`paper occurrence extraction started: ${paperId}`);

  try {
    const upstream = await fetch(
      `${backendUrl}/paper-sources/paper/${encodeURIComponent(paperId)}/extract-occurrences`,
      {
        method: "POST",
        headers: cookie ? { cookie } : undefined,
        cache: "no-store",
      },
    );

    const body = await upstream.text();
    const contentType = upstream.headers.get("content-type");
    console.log(
      `paper occurrence extraction finished: ${paperId} status=${upstream.status}`,
    );

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
