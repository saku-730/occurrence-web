"use client";

import Link from "next/link";
import { useEffect, useState } from "react";

import { apiFetch } from "@/lib/api";

const navigationItems = [
  { href: "/", label: "Top" },
  { href: "/occurrences/search", label: "データ検索" },
  { href: "/map", label: "地図" },
  { href: "/occurrences/new", label: "データ登録" },
  { href: "/paper-import", label: "論文インポート" },
  { href: "/contact", label: "問い合わせ" },
  { href: "/mypage", label: "マイページ" },
];

interface CurrentUser {
  user_id: string;
  email: string;
  user_name: string;
  role: string;
}

type AuthState = "loading" | "authenticated" | "unauthenticated";

export function SiteHeader() {
  const [authState, setAuthState] = useState<AuthState>("loading");
  const [isLoggingOut, setIsLoggingOut] = useState(false);

  useEffect(() => {
    let active = true;

    apiFetch<CurrentUser>("/auth/me")
      .then(() => {
        if (active) setAuthState("authenticated");
      })
      .catch(() => {
        if (active) setAuthState("unauthenticated");
      });

    return () => {
      active = false;
    };
  }, []);

  async function logout() {
    setIsLoggingOut(true);

    try {
      await apiFetch<unknown>("/auth/logout", { method: "POST" });
      window.location.assign("/");
    } catch {
      // Keep the current authenticated state so the user can retry.
      setIsLoggingOut(false);
    }
  }

  return (
    <header className="overflow-x-auto border-b border-[#d8dfe2] bg-white">
      <div className="mx-auto flex h-16 min-w-max max-w-7xl items-center px-5 sm:px-8">
        <Link className="text-base font-semibold" href="/">
          Bio Database
        </Link>

        <nav className="ml-10" aria-label="メインナビゲーション">
          <ul className="flex items-center gap-6 text-sm">
            {navigationItems.map((item) => (
              <li key={item.href}>
                <Link className="hover:text-[#176b57]" href={item.href}>
                  {item.label}
                </Link>
              </li>
            ))}
          </ul>
        </nav>

        <div className="ml-auto pl-10">
          {authState === "loading" ? (
            <span className="block h-9 w-20" aria-hidden="true" />
          ) : authState === "authenticated" ? (
            <button
              className="h-9 rounded-md border border-[#b8c3c8] px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-wait disabled:text-[#7c898f]"
              disabled={isLoggingOut}
              onClick={() => void logout()}
              type="button"
            >
              {isLoggingOut ? "処理中" : "ログアウト"}
            </button>
          ) : (
            <Link
              className="grid h-9 place-items-center rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746]"
              href="/login"
            >
              ログイン
            </Link>
          )}
        </div>
      </div>
    </header>
  );
}
