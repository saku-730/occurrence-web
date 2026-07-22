import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import type { ReactNode } from "react";

import { notFound } from "next/navigation";

import { SiteHeader } from "@/components/site-header";

const CONTENT_ROOT = path.join(process.cwd(), "content");

interface MarkdownPageProps {
  params: Promise<{
    contentPath: string[];
  }>;
}

export async function generateStaticParams() {
  const files = await listMarkdownFiles(CONTENT_ROOT);

  return files.map((filePath) => ({
    contentPath: filePath
      .replace(/\.md$/, "")
      .split(path.sep)
      .filter(Boolean),
  }));
}

export default async function MarkdownContentPage({ params }: MarkdownPageProps) {
  const { contentPath } = await params;
  const markdownPath = resolveMarkdownPath(contentPath);

  if (!markdownPath) {
    notFound();
  }

  let markdown: string;
  try {
    markdown = await readFile(markdownPath, "utf8");
  } catch {
    notFound();
  }

  const content = renderMarkdown(markdown);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-3xl px-5 py-8 sm:px-8">
        <article className="rounded-md border border-[#d8dfe2] bg-white px-6 py-7">
          <div className="space-y-5">{content}</div>
        </article>
      </main>
    </div>
  );
}

async function listMarkdownFiles(directory: string, baseDirectory = directory): Promise<string[]> {
  const entries = await readdir(directory, { withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name);

    if (entry.isDirectory()) {
      files.push(...(await listMarkdownFiles(entryPath, baseDirectory)));
      continue;
    }

    if (entry.isFile() && entry.name.endsWith(".md")) {
      files.push(path.relative(baseDirectory, entryPath));
    }
  }

  return files;
}

function resolveMarkdownPath(contentPath: string[]): string | null {
  if (contentPath.length === 0) return null;
  if (contentPath.some((segment) => segment === "." || segment === ".." || segment.includes(path.sep))) {
    return null;
  }

  return path.join(CONTENT_ROOT, ...contentPath) + ".md";
}

function renderMarkdown(markdown: string): ReactNode[] {
  const blocks: ReactNode[] = [];
  const lines = markdown.split(/\r?\n/);
  let paragraph: string[] = [];
  let listItems: string[] = [];

  function flushParagraph() {
    if (paragraph.length === 0) return;
    const text = paragraph.join(" ");
    blocks.push(
      <p className="text-sm leading-7 text-[#324047]" key={`p-${blocks.length}`}>
        {renderInlineMarkdown(text)}
      </p>,
    );
    paragraph = [];
  }

  function flushList() {
    if (listItems.length === 0) return;
    blocks.push(
      <ul className="list-disc space-y-2 pl-5 text-sm leading-7 text-[#324047]" key={`ul-${blocks.length}`}>
        {listItems.map((item, index) => (
          <li key={`${item}-${index}`}>{renderInlineMarkdown(item)}</li>
        ))}
      </ul>,
    );
    listItems = [];
  }

  for (const rawLine of lines) {
    const line = rawLine.trim();

    if (!line) {
      flushParagraph();
      flushList();
      continue;
    }

    if (line.startsWith("# ")) {
      flushParagraph();
      flushList();
      blocks.push(
        <h1 className="text-2xl font-semibold" key={`h1-${blocks.length}`}>
          {renderInlineMarkdown(line.slice(2).trim())}
        </h1>,
      );
      continue;
    }

    if (line.startsWith("## ")) {
      flushParagraph();
      flushList();
      blocks.push(
        <h2 className="pt-2 text-lg font-semibold" key={`h2-${blocks.length}`}>
          {renderInlineMarkdown(line.slice(3).trim())}
        </h2>,
      );
      continue;
    }

    if (line.startsWith("- ")) {
      flushParagraph();
      listItems.push(line.slice(2).trim());
      continue;
    }

    flushList();
    paragraph.push(line);
  }

  flushParagraph();
  flushList();

  return blocks;
}

function renderInlineMarkdown(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const linkPattern = /\[([^\]]+)\]\((https?:\/\/[^)]+)\)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = linkPattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }

    nodes.push(
      <a
        className="text-[#176b57] underline-offset-2 hover:underline"
        href={match[2]}
        key={`${match[2]}-${match.index}`}
        rel="noreferrer"
        target="_blank"
      >
        {match[1]}
      </a>,
    );
    lastIndex = linkPattern.lastIndex;
  }

  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }

  return nodes;
}
