import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import type { ComponentPropsWithoutRef } from "react";

import Markdown from "markdown-to-jsx/react";
import { notFound } from "next/navigation";

import { SiteHeader } from "@/components/site-header";

const CONTENT_ROOT = path.join(process.cwd(), "content");

interface MarkdownPageProps {
  params: Promise<{
    contentPath: string[];
  }>;
}

const MARKDOWN_OPTIONS = {
  disableParsingRawHTML: true,
  forceBlock: true,
  forceWrapper: true,
  wrapper: "div",
  wrapperProps: {
    className: "space-y-5",
  },
  overrides: {
    h1: {
      props: {
        className: "text-2xl font-semibold",
      },
    },
    h2: {
      props: {
        className: "pt-2 text-lg font-semibold",
      },
    },
    h3: {
      props: {
        className: "pt-1 text-base font-semibold",
      },
    },
    p: {
      props: {
        className: "text-sm leading-7 text-[#324047]",
      },
    },
    ul: {
      props: {
        className: "list-disc space-y-2 pl-5 text-sm leading-7 text-[#324047]",
      },
    },
    ol: {
      props: {
        className: "list-decimal space-y-2 pl-5 text-sm leading-7 text-[#324047]",
      },
    },
    li: {
      props: {
        className: "pl-1",
      },
    },
    a: {
      component: MarkdownLink,
    },
    blockquote: {
      props: {
        className: "border-l-4 border-[#d8dfe2] pl-4 text-sm leading-7 text-[#4b5960]",
      },
    },
    code: {
      props: {
        className: "rounded bg-[#eef2f3] px-1.5 py-0.5 font-mono text-[0.9em]",
      },
    },
    pre: {
      props: {
        className:
          "overflow-x-auto rounded-md bg-[#eef2f3] p-4 text-sm leading-6 [&>code]:bg-transparent [&>code]:p-0",
      },
    },
    hr: {
      props: {
        className: "border-0 border-t border-[#d8dfe2]",
      },
    },
    table: {
      props: {
        className: "w-full border-collapse text-left text-sm",
      },
    },
    th: {
      props: {
        className: "border border-[#d8dfe2] bg-[#f5f7f8] px-3 py-2 font-semibold",
      },
    },
    td: {
      props: {
        className: "border border-[#d8dfe2] px-3 py-2 align-top",
      },
    },
  },
} as const;

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

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-3xl px-5 py-8 sm:px-8">
        <article className="rounded-md border border-[#d8dfe2] bg-white px-6 py-7">
          <Markdown options={MARKDOWN_OPTIONS}>{markdown}</Markdown>
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

function MarkdownLink({
  href = "",
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<"a">) {
  const classes = ["text-[#176b57] underline-offset-2 hover:underline", className]
    .filter(Boolean)
    .join(" ");
  const isExternalHttpLink = /^https?:\/\//i.test(href);

  return (
    <a
      {...props}
      className={classes}
      href={href}
      {...(isExternalHttpLink
        ? {
            rel: "noopener noreferrer",
            target: "_blank",
          }
        : {})}
    >
      {children}
    </a>
  );
}
