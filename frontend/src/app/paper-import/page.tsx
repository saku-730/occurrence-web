"use client";

import { useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

type AuthStatus =
  | "loading"
  | "authenticated"
  | "unauthenticated"
  | "error";

export default function PaperImportPage() {
  const [authStatus, setAuthStatus] = useState<AuthStatus>("loading");

  useEffect(() => {
    let active = true;

    apiFetch<unknown>("/auth/me")
      .then(() => {
        if (active) setAuthStatus("authenticated");
      })
      .catch((error: unknown) => {
        if (!active) return;

        if (error instanceof ApiError && error.status === 401) {
          setAuthStatus("unauthenticated");
          return;
        }

        setAuthStatus("error");
      });

    return () => {
      active = false;
    };
  }, []);

  if (authStatus !== "authenticated") {
    const message =
      authStatus === "loading"
        ? "ログイン状態を確認しています"
        : authStatus === "unauthenticated"
          ? "論文をインポートするにはログインが必要です"
          : "ログイン状態を確認できませんでした";

    return (
      <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
        <SiteHeader />
        <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
          <h1 className="mb-6 text-2xl font-semibold">論文インポート</h1>
          <section className="grid min-h-56 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
            <p className="text-sm text-[#65737a]">{message}</p>
          </section>
        </main>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-5xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">論文インポート</h1>
        </div>

        <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
          <div className="border-b border-[#d8dfe2] bg-[#eef2f3] px-5 py-3">
            <h2 className="text-sm font-medium text-[#526168]">論文PDF</h2>
          </div>
          <div className="px-5 py-6">
            <p className="text-sm leading-6 text-[#526168]">
              論文PDFをアップロードし、抽出されたオカレンス候補を確認してから登録します。
            </p>
          </div>
        </section>
      </main>
    </div>
  );
}
