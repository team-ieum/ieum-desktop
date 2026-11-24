// 노드 추가 패널의 분류표 — 앱(카테고리)·소제목별로 어떤 노드가 들어가는지 명시한다.
export type CategoryId = "start" | "ai" | "flow" | "data" | "http" | "github" | "local";
export type SectionId =
  | "direct" | "schedule" | "transform" | "text" | "number" | "date" | "list"
  | "trigger" | "issue" | "pr" | "lookup" | "action" | "main";
export type Section = { id: SectionId; events: string[] };
export type Category = { id: CategoryId; zone: "basic" | "apps"; sections: Section[] };

const text = [
  "case", "default", "extract", "extract_pattern", "length", "markdown_to_html",
  "replace", "split", "strip_html", "trim", "truncate", "url_encode",
].map((n) => `formatter.text_${n}`);

/** 화면 순서 그대로. */
export const CATEGORIES: Category[] = [
  {
    id: "start", zone: "basic",
    sections: [
      { id: "direct", events: ["manual.run"] },
      { id: "schedule", events: ["schedule.hourly", "schedule.daily", "schedule.weekly", "schedule.monthly", "schedule.every"] },
    ],
  },
  { id: "ai", zone: "basic", sections: [{ id: "main", events: ["local.ai"] }] },
  { id: "flow", zone: "basic", sections: [{ id: "main", events: ["branch.if"] }] },
  {
    id: "data", zone: "basic",
    sections: [
      { id: "transform", events: ["transform.map"] },
      { id: "text", events: text },
      { id: "number", events: ["currency", "format", "math", "phone"].map((n) => `formatter.number_${n}`) },
      { id: "date", events: ["date_add", "date_compare", "date_format"].map((n) => `formatter.${n}`) },
      { id: "list", events: ["formatter.list_pick", "formatter.list_to_text", "formatter.lookup_table"] },
    ],
  },
  { id: "http", zone: "basic", sections: [{ id: "main", events: ["http.request"] }] },
  {
    id: "github", zone: "apps",
    sections: [
      { id: "trigger", events: ["github.new_issue", "github.new_pull_request"] },
      {
        id: "issue",
        events: ["create_issue", "update_issue", "find_issue", "find_or_create_issue", "list_issues", "add_labels", "create_comment"].map((n) => `github.${n}`),
      },
      {
        id: "pr",
        events: [
          "create_pull_request", "update_pull_request", "request_reviewers", "find_pull_request",
          "find_or_create_pull_request", "get_pr_files", "get_pr_reviews", "submit_pr_review",
        ].map((n) => `github.${n}`),
      },
      {
        id: "lookup",
        events: ["find_branch", "find_org", "find_repo", "find_user", "get_file", "check_org_membership"].map((n) => `github.${n}`),
      },
    ],
  },
  {
    id: "local", zone: "apps",
    sections: [
      { id: "trigger", events: ["local.folder_new_file"] },
      { id: "action", events: ["local.file_move", "local.file_rename", "local.file_trash"] },
    ],
  },
];

/** sections 순서대로 펼친 event 목록. */
export const categoryEvents = (c: Category): string[] => c.sections.flatMap((s) => s.events);
export const isTriggerSection = (s: Section): boolean => s.id === "trigger";
