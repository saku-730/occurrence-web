"use client";

import { useEffect, useMemo, useState } from "react";
import { ApiError, apiFetch } from "@/lib/api";
import type { LabelTerm } from "@/components/label-field-picker";

const MAX_TEMPLATES_PER_USER = 10;
const DEFAULT_MODE = "__default__";
const NEW_MODE = "__new__";

export type LabelTemplateBuiltinKey =
  | "scientificName"
  | "creator"
  | "eventDate"
  | "locality"
  | "coordinates"
  | "created";

const BUILTIN_KEYS: LabelTemplateBuiltinKey[] = [
  "scientificName",
  "creator",
  "eventDate",
  "locality",
  "coordinates",
  "created",
];

export interface LabelTemplateEditorValue {
  widthMm: number;
  heightMm: number;
  fontSizeMm: number;
  qrEnabled: boolean;
  qrSizeMm: number;
  builtinFields: LabelTemplateBuiltinKey[];
  customFields: LabelTerm[];
}

type PersistedLabelTemplateField =
  | {
      type: "builtin";
      key: LabelTemplateBuiltinKey;
      enabled: boolean;
    }
  | {
      type: "darwinCore";
      uri: string;
      enabled: boolean;
    };

type PersistedLabelTemplateDefinition = {
  version: 1;
  name: string;
  widthMm: number;
  heightMm: number;
  fontSizeMm: number;
  qr: {
    enabled: boolean;
    sizeMm: number;
  };
  fields: PersistedLabelTemplateField[];
};

type SavedLabelTemplate = {
  id: string;
  template: PersistedLabelTemplateDefinition;
};

type ListLabelTemplatesResponse = {
  templates: SavedLabelTemplate[];
};

type LoadState = "loading" | "ready" | "unauthorized" | "error";
type TemplateAction = "idle" | "saving" | "deleting";

export function LabelTemplateManager({
  value,
  defaultValue,
  invalid,
  disabled,
  onApply,
}: {
  value: LabelTemplateEditorValue;
  defaultValue: LabelTemplateEditorValue;
  invalid: boolean;
  disabled: boolean;
  onApply: (value: LabelTemplateEditorValue) => void;
}) {
  const [templates, setTemplates] = useState<SavedLabelTemplate[]>([]);
  const [loadState, setLoadState] = useState<LoadState>("loading");
  const [mode, setMode] = useState(DEFAULT_MODE);
  const [name, setName] = useState("");
  const [action, setAction] = useState<TemplateAction>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [messageKind, setMessageKind] = useState<"status" | "error">("status");

  useEffect(() => {
    let active = true;

    void apiFetch<ListLabelTemplatesResponse>("/label-templates", { cache: "no-store" })
      .then((response) => {
        if (!active) return;
        setTemplates(response.templates);
        setLoadState("ready");
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (error instanceof ApiError && error.status === 401) {
          setLoadState("unauthorized");
          setMessage("テンプレートの保存にはログインが必要です。");
        } else {
          setLoadState("error");
          setMessage("保存済みテンプレートを取得できませんでした。");
        }
        setMessageKind("error");
      });

    return () => {
      active = false;
    };
  }, []);

  const selectedTemplateId = mode === DEFAULT_MODE || mode === NEW_MODE ? null : mode;
  const selectedTemplate = useMemo(
    () => templates.find((template) => template.id === selectedTemplateId) ?? null,
    [selectedTemplateId, templates],
  );
  const busy = action !== "idle";
  const atLimit = templates.length >= MAX_TEMPLATES_PER_USER;
  const creating = selectedTemplateId === null;
  const saveDisabled = disabled || busy || loadState !== "ready" || invalid || !name.trim()
    || (creating && atLimit);

  function selectTemplate(nextMode: string) {
    setMode(nextMode);
    setMessage(null);

    if (nextMode === DEFAULT_MODE) {
      setName("");
      onApply(cloneEditorValue(defaultValue));
      return;
    }

    if (nextMode === NEW_MODE) {
      setName("");
      return;
    }

    const saved = templates.find((template) => template.id === nextMode);
    if (!saved) return;

    setName(saved.template.name);
    onApply(editorValueFromTemplate(saved.template));
    setMessageKind("status");
    setMessage(`「${saved.template.name}」を適用しました。`);
  }

  async function saveTemplate() {
    const trimmedName = name.trim();
    if (!trimmedName || saveDisabled) return;

    setAction("saving");
    setMessage(null);

    try {
      const template = persistedTemplateFromEditor(value, trimmedName);
      const saved = selectedTemplateId
        ? await apiFetch<SavedLabelTemplate>(`/label-templates/${encodeURIComponent(selectedTemplateId)}`, {
            method: "PUT",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ template }),
          })
        : await apiFetch<SavedLabelTemplate>("/label-templates", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ template }),
          });

      setTemplates((current) => selectedTemplateId
        ? current.map((item) => item.id === saved.id ? saved : item)
        : [...current, saved]);
      setMode(saved.id);
      setName(saved.template.name);
      setMessageKind("status");
      setMessage(selectedTemplateId ? "テンプレートを更新しました。" : "テンプレートを保存しました。");
    } catch (error: unknown) {
      setMessageKind("error");
      setMessage(templateErrorMessage(error));
    } finally {
      setAction("idle");
    }
  }

  async function deleteTemplate() {
    if (!selectedTemplate || disabled || busy) return;
    if (!window.confirm(`「${selectedTemplate.template.name}」を削除しますか？`)) return;

    setAction("deleting");
    setMessage(null);

    try {
      await apiFetch<{ deleted: boolean }>(`/label-templates/${encodeURIComponent(selectedTemplate.id)}`, {
        method: "DELETE",
      });
      setTemplates((current) => current.filter((template) => template.id !== selectedTemplate.id));
      setMode(NEW_MODE);
      setName("");
      setMessageKind("status");
      setMessage("テンプレートを削除しました。現在のレイアウトはそのまま残しています。");
    } catch (error: unknown) {
      setMessageKind("error");
      setMessage(templateErrorMessage(error));
    } finally {
      setAction("idle");
    }
  }

  return (
    <section className="mb-5 rounded border border-[#c9d0d3] bg-[#f7f9f9] p-3" aria-labelledby="label-template-heading">
      <div className="flex items-center justify-between gap-2">
        <h3 id="label-template-heading" className="text-sm font-semibold">保存テンプレート</h3>
        <span className="text-xs tabular-nums text-[#526168]">{templates.length} / {MAX_TEMPLATES_PER_USER}</span>
      </div>

      <label className="mt-3 block text-xs text-[#526168]">
        テンプレート
        <select
          value={mode}
          disabled={disabled || busy || loadState !== "ready"}
          onChange={(event) => selectTemplate(event.target.value)}
          className="mt-1 block h-9 w-full rounded border border-[#b8c3c8] bg-white px-2 text-sm text-[#182126] disabled:opacity-50"
        >
          <option value={DEFAULT_MODE}>デフォルト</option>
          <option value={NEW_MODE}>現在の設定を新規保存</option>
          {templates.map((template) => (
            <option key={template.id} value={template.id}>{template.template.name}</option>
          ))}
        </select>
      </label>

      <label className="mt-3 block text-xs text-[#526168]">
        テンプレート名
        <input
          type="text"
          maxLength={100}
          value={name}
          disabled={disabled || busy || loadState !== "ready"}
          onChange={(event) => setName(event.target.value)}
          placeholder="例: ミミズ標本 40×20"
          className="mt-1 block h-9 w-full rounded border border-[#b8c3c8] bg-white px-2 text-sm text-[#182126] disabled:opacity-50"
        />
      </label>

      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          disabled={saveDisabled}
          onClick={() => void saveTemplate()}
          className="rounded bg-[#176b57] px-3 py-2 text-xs text-white hover:bg-[#125746] disabled:opacity-40"
        >
          {action === "saving" ? "保存中…" : selectedTemplateId ? "上書き保存" : "新規保存"}
        </button>
        {selectedTemplateId && (
          <button
            type="button"
            disabled={disabled || busy}
            onClick={() => {
              setMode(NEW_MODE);
              setName("");
              setMessage(null);
            }}
            className="rounded border border-[#b8c3c8] bg-white px-3 py-2 text-xs hover:bg-[#eef2f3] disabled:opacity-40"
          >
            新規として保存
          </button>
        )}
        {selectedTemplateId && (
          <button
            type="button"
            disabled={disabled || busy}
            onClick={() => void deleteTemplate()}
            className="rounded border border-[#b8c3c8] bg-white px-3 py-2 text-xs text-[#a53d32] hover:bg-[#f7ecea] disabled:opacity-40"
          >
            {action === "deleting" ? "削除中…" : "削除"}
          </button>
        )}
      </div>

      {loadState === "loading" && <p role="status" className="mt-2 text-xs text-[#526168]">テンプレートを読み込んでいます…</p>}
      {atLimit && !selectedTemplateId && loadState === "ready" && (
        <p role="status" className="mt-2 text-xs text-[#a36a16]">保存上限の10件に達しています。新規保存するには既存テンプレートを削除してください。</p>
      )}
      {invalid && loadState === "ready" && (
        <p role="status" className="mt-2 text-xs text-[#a36a16]">現在のレイアウトにエラーがあるため、修正してから保存してください。</p>
      )}
      {message && (
        <p role={messageKind === "error" ? "alert" : "status"}
          className={`mt-2 text-xs ${messageKind === "error" ? "text-[#a53d32]" : "text-[#526168]"}`}>
          {message}
        </p>
      )}
    </section>
  );
}

function persistedTemplateFromEditor(
  value: LabelTemplateEditorValue,
  name: string,
): PersistedLabelTemplateDefinition {
  const enabledBuiltins = new Set(value.builtinFields);
  return {
    version: 1,
    name,
    widthMm: value.widthMm,
    heightMm: value.heightMm,
    fontSizeMm: value.fontSizeMm,
    qr: {
      enabled: value.qrEnabled,
      sizeMm: value.qrSizeMm,
    },
    fields: [
      ...BUILTIN_KEYS.map((key) => ({
        type: "builtin" as const,
        key,
        enabled: enabledBuiltins.has(key),
      })),
      ...value.customFields.map((field) => ({
        type: "darwinCore" as const,
        uri: field.uri,
        enabled: true,
      })),
    ],
  };
}

function editorValueFromTemplate(template: PersistedLabelTemplateDefinition): LabelTemplateEditorValue {
  const builtinFields = template.fields.flatMap((field) =>
    field.type === "builtin" && field.enabled && isBuiltinKey(field.key) ? [field.key] : []);
  const customFields = template.fields.flatMap((field) =>
    field.type === "darwinCore" && field.enabled ? [{ uri: field.uri, local_name: localNameFromUri(field.uri) }] : []);

  return {
    widthMm: template.widthMm,
    heightMm: template.heightMm,
    fontSizeMm: template.fontSizeMm,
    qrEnabled: template.qr.enabled,
    qrSizeMm: template.qr.sizeMm,
    builtinFields,
    customFields,
  };
}

function cloneEditorValue(value: LabelTemplateEditorValue): LabelTemplateEditorValue {
  return {
    ...value,
    builtinFields: [...value.builtinFields],
    customFields: value.customFields.map((field) => ({ ...field })),
  };
}

function isBuiltinKey(value: string): value is LabelTemplateBuiltinKey {
  return BUILTIN_KEYS.includes(value as LabelTemplateBuiltinKey);
}

function localNameFromUri(uri: string): string {
  const raw = uri.split(/[\/#]/u).filter(Boolean).at(-1) ?? uri;
  try {
    return decodeURIComponent(raw);
  } catch {
    return raw;
  }
}

function templateErrorMessage(error: unknown): string {
  if (!(error instanceof ApiError)) return "テンプレートを保存できませんでした。";

  const body = error.body && typeof error.body === "object"
    ? error.body as { error?: unknown; message?: unknown }
    : null;
  const code = typeof body?.error === "string" ? body.error : null;

  if (error.status === 401) return "テンプレートの保存にはログインが必要です。";
  if (error.status === 409 || code === "label_template_limit_reached") {
    return "保存できるテンプレートは1ユーザー10件までです。";
  }
  if (error.status === 404) return "対象のテンプレートが見つかりません。一覧を開き直してください。";
  if (error.status === 400) {
    return "現在の設定をテンプレートとして保存できません。寸法・表示項目・任意項目を確認してください。";
  }
  return "テンプレートの保存処理に失敗しました。";
}
