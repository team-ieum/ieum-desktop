export type PasswordIssue = "length" | "lower" | "upper" | "digit" | "special" | "ascii";

export const PASSWORD_RULES: PasswordIssue[] = ["length", "lower", "upper", "digit", "special", "ascii"];

/** BE 가입 정규식과 같은 규칙. 특수문자 집합도 BE 그대로. */
export function checkPassword(pw: string): PasswordIssue[] {
  const issues: PasswordIssue[] = [];
  if (pw.length < 8 || pw.length > 72) issues.push("length");
  if (!/[a-z]/.test(pw)) issues.push("lower");
  if (!/[A-Z]/.test(pw)) issues.push("upper");
  if (!/\d/.test(pw)) issues.push("digit");
  if (!/[!@#$%^&*()_+\-=[\]{};':"\\|,.<>/?]/.test(pw)) issues.push("special");
  if (!/^[\x20-\x7E]*$/.test(pw)) issues.push("ascii");
  return issues;
}
