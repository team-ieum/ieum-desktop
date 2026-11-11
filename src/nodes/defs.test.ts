import { expect, test } from "vitest";
import { newBranchKey } from "./defs";

test("new_branch_key_format", () => {
  for (let i = 0; i < 20; i++) expect(newBranchKey()).toMatch(/^b_[0-9a-f]{8}$/);
});
