"use client";

import Link from "next/link";
import { FormEvent, useState } from "react";

import { SiteHeader } from "@/components/site-header";

type TemplateField = "scientificName" | "creator" | "coordinates" | "created" | "qrCode";

interface LabelTemplate {
  id: string;
  name: string;
  width_mm: number;
  height_mm: number;
  qr_size_mm: number;
  fields: TemplateField[];
}

const LABEL_TEMPLATE_STORAGE_KEY = "occurrence-web.label-templates";
const TEMPLATE_FIELDS: Array<{ id: TemplateField; label: string }> = [
  { id: "scientificName", label: "学名" },
  { id: "creator", label: "作成者" },
  { id: "coordinates", label: "緯度・経度" },
  { id: "created", label: "作成日" },
  { id: "qrCode", label: "QRコード" },
];

export default function NewLabelTemplatePage() {
  const [name, setName] = useState("");
  const [widthMm, setWidthMm] = useState(40);
  const [heightMm, setHeightMm] = useState(20);
  const [qrSizeMm, setQrSizeMm] = useState(15);
  const [fields, setFields] = useState<TemplateField[]>([
    "scientificName",
    "creator",
    "coordinates",
    "created",
    "qrCode",
  ]);
  const [saveState, setSaveState] = useState<"idle" | "saved" | "error">("idle");

  function toggleField(field: TemplateField, checked: boolean) {
    setFields((current) =>
      checked ? [...current, field] : current.filter((value) => value !== field),
    );
  }

  function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();

    if (!name.trim() || widthMm <= 0 || heightMm <= 0 || qrSizeMm <= 0) {
      setSaveState("error");
      return;
    }

    const template: LabelTemplate = {
      id: crypto.randomUUID(),
      name: name.trim(),
      width_mm: widthMm,
      height_mm: heightMm,
      qr_size_mm: qrSizeMm,
      fields,
    };

    try {
      const stored = window.localStorage.getItem(LABEL_TEMPLATE_STORAGE_KEY);
      const templates = stored ? (JSON.parse(stored) as LabelTemplate[]) : [];
      window.localStorage.setItem(
        LABEL_TEMPLATE_STORAGE_KEY,
        JSON.stringify([...templates, template]),
      );
      setSaveState("saved");
    } catch {
      setSaveState("error");
    }
  }

  return (
    <div className="min-h-screen bg-[#f5f7f8] text-[#182126]">
      <SiteHeader />
      <main className="mx-auto w-full max-w-6xl px-5 py-8 sm:px-8">
        <div className="mb-7 flex items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold">ラベルテンプレート作成</h1>
            <p className="mt-2 text-sm text-[#526168]">デフォルトテンプレートは変更されません。</p>
          </div>
          <Link
            className="inline-flex h-10 items-center rounded-md border border-[#b8c3c8] bg-white px-4 text-sm font-medium hover:bg-[#eef2f3]"
            href="/occurrences/search"
          >
            検索へ戻る
          </Link>
        </div>

        <form className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_24rem]" onSubmit={handleSubmit}>
          <section className="border border-[#d8dfe2] bg-white p-5">
            <label className="block">
              <span className="mb-2 block text-sm font-medium">テンプレート名</span>
              <input
                className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
                onChange={(event) => setName(event.target.value)}
                required
                value={name}
              />
            </label>

            <fieldset className="mt-6">
              <legend className="mb-3 text-sm font-medium">ラベルサイズ</legend>
              <div className="grid grid-cols-3 gap-3">
                <NumberField label="横 mm" onChange={setWidthMm} value={widthMm} />
                <NumberField label="縦 mm" onChange={setHeightMm} value={heightMm} />
                <NumberField label="QR mm" onChange={setQrSizeMm} value={qrSizeMm} />
              </div>
            </fieldset>

            <fieldset className="mt-6">
              <legend className="mb-3 text-sm font-medium">表示項目</legend>
              <div className="grid gap-3 sm:grid-cols-2">
                {TEMPLATE_FIELDS.map((field) => (
                  <label className="flex cursor-pointer items-center gap-2 text-sm" key={field.id}>
                    <input
                      checked={fields.includes(field.id)}
                      className="size-4 accent-[#176b57]"
                      onChange={(event) => toggleField(field.id, event.target.checked)}
                      type="checkbox"
                    />
                    {field.label}
                  </label>
                ))}
              </div>
            </fieldset>

            <div className="mt-8 flex items-center gap-4">
              <button
                className="h-10 rounded-md bg-[#176b57] px-5 text-sm font-medium text-white hover:bg-[#125746]"
                type="submit"
              >
                テンプレートを作成
              </button>
              {saveState === "saved" ? <p className="text-sm text-[#176b57]">作成しました</p> : null}
              {saveState === "error" ? <p className="text-sm text-[#a53d32]">入力内容を確認してください</p> : null}
            </div>
          </section>

          <aside>
            <p className="mb-3 text-sm font-medium">プレビュー</p>
            <div className="grid min-h-80 place-items-center border border-[#d8dfe2] bg-white p-6">
              <article
                className="relative w-full max-w-[20rem] overflow-hidden border border-[#182126] bg-white p-4 text-sm text-[#182126]"
                style={{ aspectRatio: `${widthMm} / ${heightMm}` }}
              >
                {fields.includes("scientificName") ? <p className="font-semibold">Quercus serrata</p> : null}
                <div className="mt-2 space-y-1 text-xs">
                  {fields.includes("creator") ? <p>Yamada Taro</p> : null}
                  {fields.includes("coordinates") ? <p>36.225333, 140.106861</p> : null}
                  {fields.includes("created") ? <p>2026年7月28日</p> : null}
                </div>
                {fields.includes("qrCode") ? (
                  <div
                    className="absolute bottom-3 right-3 grid place-items-center border border-[#182126] bg-white text-[10px]"
                    style={{ height: `${qrSizeMm * 2.6}px`, width: `${qrSizeMm * 2.6}px` }}
                  >
                    QR
                  </div>
                ) : null}
              </article>
            </div>
          </aside>
        </form>
      </main>
    </div>
  );
}

function NumberField({
  label,
  onChange,
  value,
}: {
  label: string;
  onChange: (value: number) => void;
  value: number;
}) {
  return (
    <label>
      <span className="mb-2 block text-xs font-medium text-[#526168]">{label}</span>
      <input
        className="h-10 w-full rounded-md border border-[#b8c3c8] px-3 text-sm outline-none focus:border-[#176b57] focus:ring-2 focus:ring-[#176b57]/15"
        min="1"
        onChange={(event) => onChange(Number(event.target.value))}
        step="1"
        type="number"
        value={value}
      />
    </label>
  );
}
