"use client";

import { useEffect, useState } from "react";
import type { FormEvent } from "react";

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
  const [isEditingUserName, setIsEditingUserName] = useState(false);
  const [userNameInput, setUserNameInput] = useState("");
  const [isSavingUserName, setIsSavingUserName] = useState(false);
  const [userNameError, setUserNameError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;

    apiFetch<CurrentUser>("/auth/me")
      .then((user) => {
        if (active) {
          setState({ status: "ready", user });
          setUserNameInput(user.user_name);
        }
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

  async function saveUserName(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (state.status !== "ready" || isSavingUserName) return;

    const userName = userNameInput.trim();
    if (!userName) {
      setUserNameError("ユーザー名を入力してください");
      return;
    }

    setIsSavingUserName(true);
    setUserNameError(null);
    try {
      const user = await apiFetch<CurrentUser>("/auth/me", {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ user_name: userName }),
      });
      setState({ status: "ready", user });
      setUserNameInput(user.user_name);
      setIsEditingUserName(false);
    } catch (error: unknown) {
      if (error instanceof ApiError && error.status === 400) {
        setUserNameError("有効なユーザー名を入力してください");
      } else if (error instanceof ApiError && error.status === 401) {
        setState({ status: "unauthenticated" });
      } else {
        setUserNameError("ユーザー名を変更できませんでした");
      }
    } finally {
      setIsSavingUserName(false);
    }
  }

  function cancelUserNameEdit() {
    if (state.status === "ready") setUserNameInput(state.user.user_name);
    setUserNameError(null);
    setIsEditingUserName(false);
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />

      <main className="mx-auto w-full max-w-3xl px-5 py-8 sm:px-8">
        <div className="mb-6">
          <h1 className="text-2xl font-semibold">マイページ</h1>
        </div>

        <UserDetails
          isEditingUserName={isEditingUserName}
          isSavingUserName={isSavingUserName}
          onCancelUserNameEdit={cancelUserNameEdit}
          onChangeUserName={setUserNameInput}
          onSaveUserName={saveUserName}
          onStartUserNameEdit={() => {
            setUserNameError(null);
            setIsEditingUserName(true);
          }}
          state={state}
          userNameError={userNameError}
          userNameInput={userNameInput}
        />
      </main>
    </div>
  );
}

function UserDetails({
  isEditingUserName,
  isSavingUserName,
  onCancelUserNameEdit,
  onChangeUserName,
  onSaveUserName,
  onStartUserNameEdit,
  state,
  userNameError,
  userNameInput,
}: {
  isEditingUserName: boolean;
  isSavingUserName: boolean;
  onCancelUserNameEdit: () => void;
  onChangeUserName: (value: string) => void;
  onSaveUserName: (event: FormEvent<HTMLFormElement>) => void;
  onStartUserNameEdit: () => void;
  state: UserState;
  userNameError: string | null;
  userNameInput: string;
}) {
  if (state.status === "loading") {
    return <StatusPanel message="ユーザー情報を読み込んでいます" />;
  }

  if (state.status === "unauthenticated") {
    return <StatusPanel message="マイページを表示するにはログインが必要です" />;
  }

  if (state.status === "error") {
    return <StatusPanel message="ユーザー情報を取得できませんでした" />;
  }

  const isDemoUser = state.user.email.endsWith("@demo.invalid");

  return (
    <section className="overflow-hidden rounded-md border border-[#d8dfe2] bg-white">
      <dl className="divide-y divide-[#e4e9eb]">
        <div className="grid gap-2 px-5 py-4 sm:grid-cols-[10rem_minmax(0,1fr)] sm:gap-5">
          <dt className="text-sm font-medium text-[#526168]">ユーザー名</dt>
          <dd className="min-w-0">
            {isEditingUserName ? (
              <form className="space-y-3" onSubmit={onSaveUserName}>
                <input
                  autoComplete="nickname"
                  autoFocus
                  className="h-10 w-full rounded-md border border-[#b8c3c8] bg-white px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/20"
                  disabled={isSavingUserName}
                  onChange={(event) => onChangeUserName(event.target.value)}
                  type="text"
                  value={userNameInput}
                />
                {userNameError ? (
                  <p className="text-sm text-[#a23c32]" role="alert">
                    {userNameError}
                  </p>
                ) : null}
                <div className="flex flex-wrap gap-2">
                  <button
                    className="h-9 rounded-md bg-[#176b57] px-4 text-sm font-medium text-white hover:bg-[#125746] disabled:cursor-not-allowed disabled:bg-[#9ca8ad]"
                    disabled={isSavingUserName}
                    type="submit"
                  >
                    {isSavingUserName ? "保存中..." : "保存"}
                  </button>
                  <button
                    className="h-9 rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3] disabled:cursor-not-allowed disabled:text-[#9ca8ad]"
                    disabled={isSavingUserName}
                    onClick={onCancelUserNameEdit}
                    type="button"
                  >
                    キャンセル
                  </button>
                </div>
              </form>
            ) : (
              <div className="flex min-w-0 items-center justify-between gap-4">
                <span className="min-w-0 break-words text-sm">{state.user.user_name}</span>
                {!isDemoUser ? (
                  <button
                    className="shrink-0 text-sm font-medium text-[#176b57] hover:underline"
                    onClick={onStartUserNameEdit}
                    type="button"
                  >
                    変更
                  </button>
                ) : null}
              </div>
            )}
          </dd>
        </div>
        {!isDemoUser ? <DetailRow label="メールアドレス" value={state.user.email} /> : null}
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
