import { useState } from "react";
import { t, tDyn } from "../i18n";

/** 카탈로그 출력 칸(`outputFields`) — children은 목록 항목·객체 안 칸. */
export interface OutputField {
  key: string;
  title?: string;
  children?: OutputField[];
}

const ROW = "flex items-baseline gap-2 py-[3px] pr-2.5";
const CARET = "w-3 flex-none text-[10px] text-neutral-500";
const KEY = "flex-none font-semibold text-neutral-600";
/** 이보다 긴 글은 4줄 안에서도 자른다(20만 자 글이 첫 화면을 막지 않게). */
const CLIP = 300;

const labelOf = (key: string, f?: OutputField) => tDyn(`outputField.${key}`) ?? f?.title ?? key;
const count = (n: number) => t("output.count").replace("{n}", String(n));
const keyCount = (n: number) => t("output.keys").replace("{n}", String(n));
const short = (s: string) => (s.length > 60 ? `${s.slice(0, 60)}…` : s);

/** 목록 항목 요약 — 그 항목의 글 값 앞 두 개. */
function summary(item: object): string {
  const texts = Object.values(item).filter((v): v is string => typeof v === "string");
  return texts.slice(0, 2).map(short).join(" · ");
}

function Text({ s }: { s: string }) {
  const [all, setAll] = useState(false);
  let head = s.split("\n", 5).slice(0, 4).join("\n");
  if (head.length > CLIP) head = head.slice(0, CLIP);
  const cut = head.length < s.length;
  if (!cut && !s.includes("\n"))
    return (
      <span className="break-all text-neutral-800">
        <span className="text-neutral-500">"</span>
        {s}
        <span className="text-neutral-500">"</span>
      </span>
    );
  return (
    <span className="break-all whitespace-pre-wrap text-neutral-800">
      {all || !cut ? s : head}
      {cut && !all && (
        <>
          {" … "}
          <button type="button" className="cursor-pointer font-semibold text-main-deep-blue" onClick={() => setAll(true)}>
            {t("output.more")}
          </button>
        </>
      )}
    </span>
  );
}

function Value({ v }: { v: unknown }) {
  if (typeof v === "string") return <Text s={v} />;
  if (typeof v === "number") return <span className="break-all text-[#1a5fb4]">{String(v)}</span>;
  if (typeof v === "boolean") return <span className="text-[#8a3ab9]">{String(v)}</span>;
  return <span className="text-neutral-500">{String(v)}</span>;
}

/** 한 줄. 목록·객체는 접힌 채 개수만 — 펼칠 때만 안을 그린다. `fields`는 이 값 안 칸들의 정의. */
function Row({ name, value, fields, depth, item }: { name: string; value: unknown; fields?: OutputField[]; depth: number; item?: boolean }) {
  const [open, setOpen] = useState(false);
  const pad = { paddingLeft: 10 + depth * 16 };
  if (value === null || typeof value !== "object")
    return (
      <div className={ROW} style={pad}>
        <span className={CARET} />
        <span className={KEY}>{name}</span>
        <Value v={value} />
      </div>
    );
  const list = Array.isArray(value);
  const meta = list ? count(value.length) : (item && summary(value)) || keyCount(Object.keys(value).length);
  return (
    <>
      <button type="button" aria-expanded={open} onClick={() => setOpen(!open)} className={`${ROW} w-full cursor-pointer text-left`} style={pad}>
        <span className={CARET}>{open ? "▾" : "▸"}</span>
        <span className={KEY}>{name}</span>
        <span className="break-all text-neutral-500">{meta}</span>
      </button>
      {open &&
        (list
          ? value.map((v, i) => <Row key={i} name={String(i + 1)} value={v} fields={fields} depth={depth + 1} item />)
          : Object.entries(value).map(([k, v]) => {
              const f = fields?.find((x) => x.key === k);
              return <Row key={k} name={labelOf(k, f)} value={v} fields={f?.children} depth={depth + 1} />;
            }))}
    </>
  );
}

/** 노드 테스트 결과 — 키·값 목록(시안 B)과 JSON 원문 탭. */
export function OutputView({ output, fields }: { output: Record<string, unknown>; fields?: OutputField[] }) {
  const [tab, setTab] = useState<"list" | "json">("list");
  const tabClass = (on: boolean) =>
    `cursor-pointer rounded-[6px] px-2 py-0.5 ${on ? "bg-neutral-200 font-semibold text-neutral-800" : "text-neutral-600"}`;
  return (
    <div className="flex flex-col gap-2">
      {tab === "list" ? (
        <div className="max-h-[260px] overflow-auto rounded-brand-sm border border-neutral-200 bg-white py-1.5 text-[12.5px]">
          {Object.entries(output).map(([k, v]) => {
            const f = fields?.find((x) => x.key === k);
            return <Row key={k} name={labelOf(k, f)} value={v} fields={f?.children} depth={0} />;
          })}
        </div>
      ) : (
        <pre className="m-0 max-h-[220px] overflow-auto whitespace-pre-wrap rounded-brand-sm border border-neutral-200 bg-white p-2.5 font-mono text-xs leading-normal text-neutral-700">
          {JSON.stringify(output, null, 2)}
        </pre>
      )}
      <div role="tablist" className="flex gap-1 text-xs">
        <button type="button" role="tab" aria-selected={tab === "list"} className={tabClass(tab === "list")} onClick={() => setTab("list")}>
          {t("output.list")}
        </button>
        <button type="button" role="tab" aria-selected={tab === "json"} className={tabClass(tab === "json")} onClick={() => setTab("json")}>
          JSON
        </button>
      </div>
    </div>
  );
}
