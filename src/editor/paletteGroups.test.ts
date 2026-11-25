import { expect, test } from "vitest";
import { dict } from "../i18n";
import { DEFS, PALETTE } from "../nodes/defs";
import { CATEGORIES, categoryEvents, isTriggerSection } from "./paletteGroups";

const isTrigger = (e: string) => DEFS[e].node_type === "TRIGGER";
const cat = (id: string) => CATEGORIES.find((c) => c.id === id)!;

test("categories_cover_palette_exactly", () => {
  expect(CATEGORIES.flatMap(categoryEvents).sort()).toEqual([...PALETTE].sort());
});

test("only_trigger_sections_hold_triggers", () => {
  expect(categoryEvents(cat("start")).every(isTrigger)).toBe(true);
  for (const c of CATEGORIES.filter((c) => c.id !== "start"))
    for (const s of c.sections) {
      if (isTriggerSection(s)) expect(s.events.every(isTrigger)).toBe(true);
      else expect(s.events.some(isTrigger)).toBe(false);
    }
});

test("github_sections_counts", () => {
  expect(cat("github").sections.map((s) => s.events.length)).toEqual([2, 7, 8, 6]);
  expect(cat("local").sections.map((s) => s.events.length)).toEqual([1, 3]);
  expect(categoryEvents(cat("data")).length).toBe(23);
  expect(categoryEvents(cat("start")).length).toBe(6);
});

test("section_and_category_labels_exist", () => {
  for (const lang of ["ko", "en"] as const) {
    const d = dict[lang] as Record<string, string>;
    for (const c of CATEGORIES) {
      expect(d[`palette.cat.${c.id}`], `${lang} cat ${c.id}`).toBeTruthy();
      for (const s of c.sections) if (s.id !== "main") expect(d[`palette.sec.${s.id}`], `${lang} sec ${s.id}`).toBeTruthy();
    }
  }
});
