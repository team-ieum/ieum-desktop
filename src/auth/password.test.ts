import { expect, test } from "vitest";
import { checkPassword } from "./password";

test("check_password_rules", () => {
  expect(checkPassword("Abcdef1!")).toEqual([]);
  expect(checkPassword("abc")).toEqual(expect.arrayContaining(["length", "upper", "digit", "special"]));
  expect(checkPassword("Abcdefg1")).toEqual(["special"]);
  expect(checkPassword("Abcdef1!한")).toContain("ascii");
  expect(checkPassword("Ab1!" + "a".repeat(69))).toContain("length");
  expect(checkPassword("Ab1!" + "a".repeat(68))).toEqual([]);
  expect(checkPassword("ABCDEF1!")).toEqual(["lower"]);
  // 공백·~·`는 허용되지만 특수문자로 치지 않는다(BE 정규식).
  expect(checkPassword("Abcdef1 ~`")).toEqual(["special"]);
});
