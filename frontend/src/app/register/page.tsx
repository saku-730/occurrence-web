"use client";

import Link from "next/link";
import { FormEvent, useState } from "react";

import { SiteHeader } from "@/components/site-header";
import { ApiError, apiFetch } from "@/lib/api";

interface PreRegisterResponse {
  message: string;
  email: string;
}

interface ErrorBody {
  error?: string;
  message?: string;
}

export default function RegisterPage() {
  const [email, setEmail] = useState("");
  const [sentEmail, setSentEmail] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setErrorMessage(null);
    setIsSubmitting(true);

    try {
      const response = await apiFetch<PreRegisterResponse>(
        "/auth/pre_register",
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ email: email.trim() }),
        },
      );

      // The backend returns success only after issuing the temporary token and
      // sending the registration email, so the completion view is safe here.
      setSentEmail(response.email);
    } catch (error) {
      if (
        error instanceof ApiError &&
        error.status === 409 &&
        isErrorBody(error.body) &&
        error.body.error === "email_already_registered"
      ) {
        setErrorMessage("このメールアドレスはすでに登録されています");
      } else if (error instanceof ApiError && error.status === 400) {
        setErrorMessage("有効なメールアドレスを入力してください");
      } else {
        setErrorMessage("仮登録メールを送信できませんでした");
      }
    } finally {
      setIsSubmitting(false);
    }
  }

  function resetForm() {
    setSentEmail(null);
    setEmail("");
    setErrorMessage(null);
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-md px-5 py-10 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">新規登録</h1>
        </div>

        {sentEmail ? (
          <section className="rounded-md border border-[#d8dfe2] bg-white px-6 py-6">
            <h2 className="font-semibold">仮登録メールを送信しました</h2>
            <p className="mt-3 break-all text-sm text-[#526168]">
              {sentEmail}
            </p>
            <p className="mt-3 text-sm leading-6 text-[#526168]">
              メールに記載された案内から登録を完了してください。
            </p>
            <button
              className="mt-6 text-sm font-medium text-[#176b57] hover:underline"
              onClick={resetForm}
              type="button"
            >
              別のメールアドレスを入力
            </button>
          </section>
        ) : (
          <>
            <form
              className="rounded-md border border-[#d8dfe2] bg-white px-6 py-6"
              onSubmit={handleSubmit}
            >
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

              {errorMessage ? (
                <p className="mt-4 text-sm text-[#a23c32]" role="alert">
                  {errorMessage}
                </p>
              ) : null}

              <button
                className="mt-6 h-10 w-full rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-wait disabled:bg-[#829b95]"
                disabled={isSubmitting}
                type="submit"
              >
                {isSubmitting ? "送信中" : "仮登録メールを送信"}
              </button>
            </form>

            <p className="mt-5 text-center text-sm text-[#65737a]">
              すでにアカウントをお持ちの方は{" "}
              <Link
                className="font-medium text-[#176b57] hover:underline"
                href="/login"
              >
                ログイン
              </Link>
            </p>
          </>
        )}
      </main>
    </div>
  );
}

function isErrorBody(value: unknown): value is ErrorBody {
  return typeof value === "object" && value !== null;
}
