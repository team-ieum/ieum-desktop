import { expect, test } from "vitest";
import { choiceLabel, dict, resolveLang, setLang } from "./i18n";
import { DEFS, fieldKey } from "./nodes/defs";

test("choices_are_translated_in_both_langs", () => {
  const missing: string[] = [];
  for (const lang of ["ko", "en"] as const) {
    setLang(lang);
    for (const def of Object.values(DEFS)) {
      const walk = (fields: typeof def.inputFields) => {
        for (const f of fields) {
          for (const c of f.choices ?? []) if (choiceLabel(def.event, f.key, c.id) === undefined) missing.push(`${lang} ${def.event} ${f.key}.${c.id}`);
          walk(f.children ?? []);
        }
      };
      walk(def.inputFields);
    }
  }
  setLang("ko");
  expect(missing).toEqual([]);
  // 겹치는 칸은 노드마다 다른 이름.
  expect(choiceLabel("formatter.number_phone", "format", "dash")).toBe("하이픈 (010-1234-5678)");
  expect(choiceLabel("formatter.date_format", "format", "date")).toBe("2026-11-07");
  expect(choiceLabel("branch.if", "operator", "equals")).toBe("같음");
});

test("ko_and_en_have_same_keys", () => {
  expect(Object.keys(dict.en).sort()).toEqual(Object.keys(dict.ko).sort());
  for (const v of [...Object.values(dict.ko), ...Object.values(dict.en)]) expect(v).not.toBe("");
});

test("node_defs_are_translated", () => {
  const keys = new Set(Object.keys(dict.ko));
  const missing: string[] = [];
  for (const def of Object.values(DEFS)) {
    for (const k of [`node.${def.event}.title`, `node.${def.event}.desc`]) if (!keys.has(k)) missing.push(k);
    const walk = (fields: typeof def.inputFields, parent: string) => {
      for (const f of fields) {
        const k = fieldKey(def.event, parent ? `${parent}.${f.key}` : f.key);
        if (!keys.has(k)) missing.push(k);
        walk(f.children ?? [], parent ? `${parent}.${f.key}` : f.key);
      }
    };
    walk(def.inputFields, "");
  }
  expect(missing).toEqual([]);
});

test("resolve_lang_follows_system_when_unset", () => {
  expect(resolveLang("system", "ko-KR")).toBe("ko");
  expect(resolveLang("system", "ja-JP")).toBe("en");
  expect(resolveLang("en", "ko-KR")).toBe("en");
});
