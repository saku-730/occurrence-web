"use client";

import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { FormEvent, Suspense, useEffect, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

interface CompleteRegistrationResponse {
  message: string;
}

interface ErrorBody {
  error?: string;
  message?: string;
}

export default function CompleteRegistrationPage() {
  return (
    <Suspense fallback={null}>
      <CompleteRegistrationContent />
    </Suspense>
  );
}

function CompleteRegistrationContent() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const token = searchParams.get("token") ?? "";

  const [userName, setUserName] = useState("");
  const [password, setPassword] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    if (!token) {
      setErrorMessage("仮登録メールのURLにあるtokenが必要です");
      return;
    }

    setErrorMessage(null);
  }, [token]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setErrorMessage(null);
    setSuccessMessage(null);

    if (!token) {
      setErrorMessage("仮登録メールのURLにあるtokenが必要です");
      return;
    }

    if (password !== passwordConfirm) {
      setErrorMessage("パスワードが一致しません");
      return;
    }

    setIsSubmitting(true);

    try {
      const response = await apiFetch<CompleteRegistrationResponse>(
        "/auth/complete_registration",
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            token,
            user_name: userName.trim(),
            password,
          }),
        },
      );

      setSuccessMessage(response.message);
      setUserName("");
      setPassword("");
      setPasswordConfirm("");
    } catch (error) {
      if (error instanceof ApiError && error.status === 400) {
        if (isErrorBody(error.body)) {
          if (error.body.error === "invalid_token") {
            setErrorMessage("本登録用のtokenが無効です");
          } else if (error.body.error === "invalid_password") {
            setErrorMessage("パスワードは8文字以上128文字以下で入力してください");
          } else if (error.body.error === "invalid_username") {
            setErrorMessage("ユーザー名を入力してください");
          } else {
            setErrorMessage(error.body.message ?? "入力内容を確認してください");
          }
        } else {
          setErrorMessage("入力内容を確認してください");
        }
      } else if (error instanceof ApiError && error.status === 409) {
        setErrorMessage("このメールアドレスはすでに登録されています");
      } else {
        setErrorMessage("本登録に失敗しました");
      }
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-md px-5 py-10 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">本登録</h1>
        </div>

        {successMessage ? (
          <section className="rounded-md border border-[#d8dfe2] bg-white px-6 py-6">
            <h2 className="font-semibold">本登録が完了しました</h2>
            <p className="mt-3 text-sm text-[#526168]">{successMessage}</p>
            <div className="mt-6 flex gap-3">
              <Link
                className="inline-flex h-10 items-center rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746]"
                href="/login"
              >
                ログインへ進む
              </Link>
              <button
                className="h-10 rounded-md border border-[#b8c3c8] bg-white px-5 text-sm font-medium hover:bg-[#eef2f3]"
                onClick={() => router.refresh()}
                type="button"
              >
                もう一度確認する
              </button>
            </div>
          </section>
        ) : (
          <form
            className="rounded-md border border-[#d8dfe2] bg-white px-6 py-6"
            onSubmit={handleSubmit}
          >
            <div className="space-y-5">
              <label className="block">
                <span className="mb-2 block text-sm font-medium">ユーザー名</span>
                <input
                  autoComplete="username"
                  className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
                  disabled={isSubmitting || !token}
                  onChange={(event) => setUserName(event.target.value)}
                  required
                  type="text"
                  value={userName}
                />
              </label>

              <label className="block">
                <span className="mb-2 block text-sm font-medium">パスワード</span>
                <input
                  autoComplete="new-password"
                  className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
                  disabled={isSubmitting || !token}
                  onChange={(event) => setPassword(event.target.value)}
                  required
                  type="password"
                  value={password}
                />
              </label>

              <label className="block">
                <span className="mb-2 block text-sm font-medium">パスワード確認</span>
                <input
                  autoComplete="new-password"
                  className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15 disabled:bg-[#eef2f3]"
                  disabled={isSubmitting || !token}
                  onChange={(event) => setPasswordConfirm(event.target.value)}
                  required
                  type="password"
                  value={passwordConfirm}
                />
              </label>
            </div>

            <p className="mt-4 text-sm text-[#65737a]">
              本登録メールのURLに含まれるtokenを使ってアカウントを確定します。
            </p>

            {errorMessage ? (
              <p className="mt-4 text-sm text-[#a23c32]" role="alert">
                {errorMessage}
              </p>
            ) : null}

            <button
              className="mt-6 h-10 w-full rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-wait disabled:bg-[#829b95]"
              disabled={isSubmitting || !token}
              type="submit"
            >
              {isSubmitting ? "本登録中" : "本登録を完了する"}
            </button>
          </form>
        )}

        <p className="mt-5 text-center text-sm text-[#65737a]">
          仮登録メールをまだ受け取っていない場合は {" "}
          <Link className="font-medium text-[#176b57] hover:underline" href="/register">
            新規登録
          </Link>
          からやり直せます。
        </p>
      </main>
    </div>
  );
}

function isErrorBody(value: unknown): value is ErrorBody {
  return typeof value === "object" && value !== null;
}
