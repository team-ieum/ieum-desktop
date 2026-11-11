import { expect, test } from "vitest";
import { dict, resolveLang } from "./i18n";
import { DEFS, fieldKey } from "./nodes/defs";

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
        for (const c of f.choices ?? []) if (!keys.has(`choice.${f.key}.${c.id}`)) missing.push(`choice.${f.key}.${c.id}`);
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
