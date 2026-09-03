import { randomUUID } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const backendUrl = (process.env.BACKEND_URL ?? "http://127.0.0.1:3001").replace(/\/$/, "");

export const runtime = "nodejs";

export async function POST(
  request: Request,
  context: { params: Promise<{ paperId: string }> },
) {
  const { paperId } = await context.params;
  const cookie = request.headers.get("cookie");
  const contentType = request.headers.get("content-type") ?? "application/json";
  const body = await request.text();

  const draftDirectory =
    process.env.PAPER_BATCH_DRAFT_DIR ?? path.join(process.cwd(), ".paper-batch-drafts");
  const safePaperId = paperId.replace(/[^0-9A-Za-z-]/g, "_");
  const draftFilename = `${safePaperId}-${Date.now()}-${randomUUID()}.json`;
  const draftPath = path.join(draftDirectory, draftFilename);

  try {
    await mkdir(draftDirectory, { recursive: true });
    await writeFile(draftPath, body, { encoding: "utf8", flag: "wx" });
    console.log(`paper batch registration payload saved: ${draftPath}`);
  } catch (error) {
    console.error("failed to preserve paper batch registration payload", error);
    return Response.json(
      {
        error: "batch_payload_save_failed",
        message: "Failed to preserve the batch registration JSON before sending it to the backend",
      },
      { status: 500 },
    );
  }

  try {
    const upstream = await fetch(
      `${backendUrl}/papers/${encodeURIComponent(paperId)}/occurrences/batch`,
      {
        method: "POST",
        headers: {
          "content-type": contentType,
          ...(cookie ? { cookie } : {}),
        },
        body,
        cache: "no-store",
      },
    );

    const responseBody = await upstream.arrayBuffer();
    const upstreamContentType = upstream.headers.get("content-type");

    return new Response(responseBody, {
      status: upstream.status,
      headers: {
        ...(upstreamContentType ? { "content-type": upstreamContentType } : {}),
        "x-paper-batch-draft": draftFilename,
      },
    });
  } catch (error) {
    console.error(
      `paper batch registration backend request failed; preserved payload: ${draftPath}`,
      error,
    );
    return Response.json(
      {
        error: "backend_unavailable",
        message: "Batch registration failed after the request JSON was preserved locally",
        draft: draftFilename,
      },
      {
        status: 502,
        headers: { "x-paper-batch-draft": draftFilename },
      },
    );
  }
}
