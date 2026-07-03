"use client";

import { useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

interface CurrentUser {
  user_id: string;
  email: string;
  user_name: string;
  role: string;
}

type UserState =
  | { status: "loading" }
  | { status: "unauthenticated" }
  | { status: "error" }
  | { status: "ready"; user: CurrentUser };

export default function MyPage() {
  const [state, setState] = useState<UserState>({ status: "loading" });

  useEffect(() => {
    let active = true;

    apiFetch<CurrentUser>("/auth/me")
      .then((user) => {
        if (active) setState({ status: "ready", user });
      })
      .catch((error: unknown) => {
        if (!active) return;

        if (error instanceof ApiError && error.status === 401) {
          setState({ status: "unauthenticated" });
          return;
        }

        setState({ status: "error" });
      });

    return () => {
      active = false;
    };
  }, []);

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-3xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">マイページ</h1>
        </div>

        <UserDetails state={state} />
      </main>
    </div>
  );
}

function UserDetails({ state }: { state: UserState }) {
  if (state.status === "loading") {
    return <StatusPanel message="ユーザー情報を読み込んでいます" />;
  }

  if (state.status === "unauthenticated") {
    return <StatusPanel message="マイページを表示するにはログインが必要です" />;
  }

  if (state.status === "error") {
    return <StatusPanel message="ユーザー情報を取得できませんでした" />;
  }

  return (
    <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
      <dl className="divide-y divide-[#e4e9eb]">
        <DetailRow label="ユーザー名" value={state.user.user_name} />
        <DetailRow label="メールアドレス" value={state.user.email} />
      </dl>
    </section>
  );
}

function DetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-1 px-5 py-4 sm:grid-cols-[10rem_minmax(0,1fr)] sm:gap-5">
      <dt className="text-sm font-medium text-[#526168]">{label}</dt>
      <dd className="min-w-0 break-words text-sm">{value}</dd>
    </div>
  );
}

function StatusPanel({ message }: { message: string }) {
  return (
    <section className="grid min-h-48 place-items-center rounded-md border border-[#d8dfe2] bg-white px-6 py-12 text-center">
      <p className="text-sm text-[#65737a]">{message}</p>
    </section>
  );
}
