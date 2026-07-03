"use client";

import { FormEvent, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

interface LoginResponse {
  message: string;
  email: string;
  user_name: string;
}

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setErrorMessage(null);
    setIsSubmitting(true);

    try {
      await apiFetch<LoginResponse>("/auth/login", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email: email.trim(), password }),
      });

      // A full navigation is unnecessary: the next page mounts a fresh header,
      // which reads the session cookie through /auth/me.
      router.push("/");
      router.refresh();
    } catch (error) {
      if (
        error instanceof ApiError &&
        (error.status === 400 || error.status === 401)
      ) {
        setErrorMessage("メールアドレスまたはパスワードが正しくありません");
      } else {
        setErrorMessage("ログイン処理に失敗しました");
      }
      setIsSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-md px-5 py-10 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">ログイン</h1>
        </div>

        <form
          className="rounded-md border border-[#d8dfe2] bg-white px-6 py-6"
          onSubmit={handleSubmit}
        >
          <div className="space-y-5">
            <label className="block">
              <span className="mb-2 block text-sm font-medium">
                メールアドレス
              </span>
              <input
                autoComplete="email"
                className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                disabled={isSubmitting}
                onChange={(event) => setEmail(event.target.value)}
                required
                type="email"
                value={email}
              />
            </label>

            <label className="block">
              <span className="mb-2 block text-sm font-medium">
                パスワード
              </span>
              <input
                autoComplete="current-password"
                className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                disabled={isSubmitting}
                onChange={(event) => setPassword(event.target.value)}
                required
                type="password"
                value={password}
              />
            </label>
          </div>

          {errorMessage ? (
            <p
              className="mt-4 text-sm text-[#a23c32]"
              role="alert"
            >
              {errorMessage}
            </p>
          ) : null}

          <button
            className="mt-6 h-10 w-full rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-wait disabled:bg-[#829b95]"
            disabled={isSubmitting}
            type="submit"
          >
            {isSubmitting ? "ログイン中" : "ログイン"}
          </button>
        </form>

        <p className="mt-5 text-center text-sm text-[#65737a]">
          アカウントをお持ちでない方は{" "}
          <Link
            className="font-medium text-[#176b57] hover:underline"
            href="/register"
          >
            新規登録
          </Link>
        </p>
      </main>
    </div>
  );
}
