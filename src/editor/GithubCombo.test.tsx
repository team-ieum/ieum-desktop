// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { api, type OptionList } from "../api";
import { setLang } from "../i18n";
import type { Field } from "../nodes/defs";
import { agoText, GithubCombo } from "./GithubCombo";

beforeEach(() => setLang("ko"));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const FIELDS: Field[] = [
  { key: "owner", title: "소유자", type: "string", optionsSource: "github.owners" },
  { key: "repo", title: "저장소", type: "string", optionsSource: "github.repos", optionsInputs: ["owner"] },
  { key: "head", title: "보낼 브랜치", type: "string", optionsSource: "github.branches", optionsInputs: ["owner", "repo"] },
  { key: "milestone", title: "마일스톤", type: "string", optionsSource: "github.milestones", optionsInputs: ["owner", "repo"] },
];
const [OWNER, REPO, , MILESTONE] = FIELDS;
const ago = (ms: number) => new Date(Date.now() - ms).toISOString();
// 소유자 목록은 입력이 없어 캐시 묶음이 하나 — 모든 테스트가 같은 목록을 쓴다.
const OWNERS: OptionList = {
  items: [
    { id: "hodoon", name: "hodoon", hint: "내 계정", updatedAt: null },
    { id: "team-ieum", name: "team-ieum", hint: "조직", updatedAt: null },
  ],
  truncated: false,
};
const REPOS: OptionList = {
  items: [
    { id: "GitHubTest", name: "GitHubTest", hint: "공개", updatedAt: ago(3_600_000) },
    { id: "git-notes", name: "git-notes", hint: "비공개", updatedAt: ago(3 * 86_400_000) },
    { id: "blog", name: "blog", hint: "공개", updatedAt: ago(10_000) },
  ],
  truncated: false,
};

/** 공급원별 응답 — owners는 늘 OWNERS. */
function mockOptions(other: (source: string, inputs: Record<string, string>) => Promise<OptionList>) {
  return vi.spyOn(api, "githubOptions").mockImplementation((source, inputs) => (source === "github.owners" ? Promise.resolve(OWNERS) : other(source, inputs)));
}

function Combo({ field, init = "", config = {}, onOpenIntegrations }: { field: Field; init?: string; config?: Record<string, string>; onOpenIntegrations?: () => void }) {
  const [value, setValue] = useState(init);
  return (
    <div>
      <GithubCombo
        id="combo"
        event="github.create_pull_request"
        field={field}
        value={value}
        config={{ ...config, [field.key]: value }}
        fields={FIELDS}
        onChange={(v) => setValue(String(v))}
        onOpenIntegrations={onOpenIntegrations}
      />
      <span data-testid="value">{value}</span>
    </div>
  );
}

const box = () => screen.getByRole("combobox");
const options = () => within(screen.getByRole("listbox")).queryAllByRole("option");
const type = (text: string) => fireEvent.change(box(), { target: { value: text } });

test("filters_and_highlights", async () => {
  mockOptions(async () => REPOS);
  render(<Combo field={REPO} config={{ owner: "hlOwner" }} />);
  // 누르면 친 글이 없어도 전체 목록
  fireEvent.click(box());
  await waitFor(() => expect(options()).toHaveLength(3));
  expect(screen.getByText("hlOwner의 저장소 · 최근 수정 순")).toBeTruthy();
  expect(options()[0].textContent).toContain("공개 · 1시간 전");
  expect(options()[1].textContent).toContain("비공개 · 3일 전");
  // 비공개는 이모지 대신 선 아이콘(자물쇠) — 공개 줄엔 없다
  expect(options()[1].querySelector("svg.lucide-lock")).toBeTruthy();
  expect(options()[0].querySelector("svg.lucide-lock")).toBeNull();
  expect(options()[2].textContent).toContain("공개 · 방금");
  // 치면 대소문자 무시로 거르고 맞은 부분을 강조
  type("Git");
  expect(options().map((o) => o.textContent)).toEqual(["GitHubTest공개 · 1시간 전", "git-notes비공개 · 3일 전", '"Git" 그대로 쓰기']);
  expect([...screen.getByRole("listbox").querySelectorAll("mark")].map((m) => m.textContent)).toEqual(["Git", "git"]);
  expect(options()[0].getAttribute("aria-selected")).toBe("true");
});

test("keyboard_pick", async () => {
  mockOptions(async () => REPOS);
  render(<Combo field={OWNER} />);
  fireEvent.keyDown(box(), { key: "ArrowDown" });
  await waitFor(() => expect(options()).toHaveLength(2));
  expect(options()[0].getAttribute("aria-selected")).toBe("true");
  fireEvent.keyDown(box(), { key: "ArrowDown" });
  expect(options()[1].getAttribute("aria-selected")).toBe("true");
  expect(box().getAttribute("aria-activedescendant")).toBe(options()[1].id);
  fireEvent.keyDown(box(), { key: "Enter" });
  expect(screen.getByTestId("value").textContent).toBe("team-ieum");
  expect(screen.queryByRole("listbox")).toBeNull();
  // Esc로 닫고, 바깥을 누르면 닫는다
  fireEvent.click(box());
  expect(screen.getByRole("listbox")).toBeTruthy();
  fireEvent.keyDown(box(), { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "목록 보기" }));
  expect(screen.getByRole("listbox")).toBeTruthy();
  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("use_typed_as_is_row", async () => {
  mockOptions(async () => REPOS);
  render(<Combo field={REPO} config={{ owner: "asIsOwner" }} />);
  type("nope");
  await waitFor(() => expect(options().map((o) => o.textContent)).toEqual(['"nope" 그대로 쓰기']));
  fireEvent.click(options()[0]);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(screen.getByTestId("value").textContent).toBe("nope");
  // 목록 항목과 정확히 같으면 그 줄은 없다
  type("blog");
  expect(options().map((o) => o.textContent)).toEqual(["blog공개 · 방금"]);
  // 앞 단계 값도 그대로 쓴다
  type("{{nodes.a.output.repo}}");
  expect(options().map((o) => o.textContent)).toEqual(['"{{nodes.a.output.repo}}" 그대로 쓰기']);
  fireEvent.keyDown(box(), { key: "Enter" });
  expect(screen.getByTestId("value").textContent).toBe("{{nodes.a.output.repo}}");
});

test("blocked_messages", () => {
  const call = vi.spyOn(api, "githubOptions");
  const r = render(<Combo field={REPO} config={{ owner: "" }} />);
  // 열기 전엔 안내 없음
  expect(screen.queryByRole("note")).toBeNull();
  fireEvent.click(box());
  expect(screen.getByRole("note").textContent).toBe("'소유자' 칸을 먼저 채우면 목록에서 고를 수 있어요.");
  // 치는 건 된다
  type("my-repo");
  expect(screen.getByTestId("value").textContent).toBe("my-repo");
  r.unmount();

  const ref = render(<Combo field={REPO} config={{ owner: "{{nodes.a.output.o}}" }} />);
  fireEvent.click(screen.getByRole("button", { name: "목록 보기" }));
  expect(screen.getByRole("note").textContent).toBe("'소유자'가 앞 단계 값이라 목록을 보여 줄 수 없어요. 실행할 때 그 값으로 정해져요.");
  ref.unmount();

  render(<Combo field={REPO} config={{ owner: "a b" }} />);
  fireEvent.click(box());
  expect(screen.getByRole("note").textContent).toBe("'소유자' 값 형식이 올바르지 않아 목록을 보여 줄 수 없어요.");
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(call).not.toHaveBeenCalled();
});

test("loading_rows", () => {
  mockOptions(() => new Promise(() => {}));
  render(<Combo field={REPO} config={{ owner: "loadingOwner" }} />);
  fireEvent.click(box());
  expect(screen.getAllByTestId("combo-skeleton")).toHaveLength(3);
  expect(options()).toHaveLength(0);
});

test("not_connected_opens_settings", async () => {
  for (const code of ["GITHUB_NOT_CONNECTED", "GITHUB_AUTH"]) {
    mockOptions(() => Promise.reject({ code, message: "연결 문제" }));
    const onOpen = vi.fn();
    const r = render(<Combo field={REPO} config={{ owner: `nc-${code.length}` }} onOpenIntegrations={onOpen} />);
    fireEvent.click(box());
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("GitHub 연결이 필요해요."));
    expect(screen.queryByText("연결 문제")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "설정 › 연동 열기" }));
    expect(onOpen).toHaveBeenCalledTimes(1);
    r.unmount();
    vi.restoreAllMocks();
  }
});

test("reconnect_then_reopen_loads", async () => {
  mockOptions(() => Promise.reject({ code: "GITHUB_NOT_CONNECTED", message: "연결 문제" }));
  const r = render(<Combo field={REPO} config={{ owner: "reconnect" }} />);
  fireEvent.click(box());
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("GitHub 연결이 필요해요."));
  r.unmount();
  vi.restoreAllMocks();
  // 설정에서 연결하고 60초 안에 돌아와 다시 연다
  mockOptions(async () => REPOS);
  render(<Combo field={REPO} config={{ owner: "reconnect" }} />);
  fireEvent.click(box());
  await waitFor(() => expect(options()).toHaveLength(3));
  expect(screen.queryByRole("alert")).toBeNull();
});

test("error_retry", async () => {
  let fail = true;
  const call = mockOptions(() => (fail ? Promise.reject({ code: "GITHUB_RATE_LIMIT", message: "GitHub 요청 한도를 넘었어요." }) : Promise.resolve(REPOS)));
  render(<Combo field={REPO} config={{ owner: "retryOwner" }} />);
  fireEvent.click(box());
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("GitHub 요청 한도를 넘었어요."));
  expect(screen.queryByRole("button", { name: "설정 › 연동 열기" })).toBeNull();
  fail = false;
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "다시 시도" })));
  await waitFor(() => expect(options()).toHaveLength(3));
  expect(call).toHaveBeenCalledTimes(2);
});

test("empty_list", async () => {
  mockOptions(async () => ({ items: [], truncated: false }));
  render(<Combo field={REPO} config={{ owner: "emptyOwner" }} />);
  fireEvent.click(box());
  await waitFor(() => expect(screen.getByText("고를 수 있는 항목이 없어요.")).toBeTruthy());
});

test("chips_me_and_milestone", async () => {
  mockOptions(async (source) => (source === "github.milestones" ? { items: [{ id: "3", name: "v0.1.2 릴리스", hint: null, updatedAt: null }], truncated: false } : REPOS));
  // 닫힌 칸에도 미리 불러 칩을 보인다
  const me = render(<Combo field={OWNER} init="hodoon" />);
  await waitFor(() => expect(screen.getByText("내 계정")).toBeTruthy());
  me.unmount();
  const org = render(<Combo field={OWNER} init="team-ieum" />);
  await act(async () => {});
  expect(screen.queryByText("내 계정")).toBeNull();
  org.unmount();
  render(<Combo field={MILESTONE} init="3" config={{ owner: "chipOwner", repo: "r" }} />);
  await waitFor(() => expect(screen.getByText("v0.1.2 릴리스")).toBeTruthy());
  expect(screen.queryByRole("listbox")).toBeNull();
});

test("truncated_note", async () => {
  mockOptions(async () => ({ ...REPOS, truncated: true }));
  render(<Combo field={REPO} config={{ owner: "truncOwner" }} />);
  fireEvent.click(box());
  await waitFor(() => expect(screen.getByText("앞 300개만 보여요 — 목록에 없으면 직접 쳐 주세요")).toBeTruthy());
});

test("arrow_while_loading_keeps_first_active", async () => {
  let done!: (l: OptionList) => void;
  mockOptions(() => new Promise((r) => (done = r)));
  render(<Combo field={REPO} config={{ owner: "loading-arrow" }} />);
  fireEvent.keyDown(box(), { key: "ArrowDown" });
  fireEvent.keyDown(box(), { key: "ArrowDown" });
  await act(async () => done(REPOS));
  expect(options()[0].getAttribute("aria-selected")).toBe("true");
  fireEvent.keyDown(box(), { key: "Enter" });
  expect(screen.getByTestId("value").textContent).toBe("GitHubTest");
});

test("active_option_scrolls_into_view", async () => {
  mockOptions(async () => REPOS);
  const scroll = vi.fn();
  Element.prototype.scrollIntoView = scroll;
  try {
    render(<Combo field={OWNER} />);
    fireEvent.keyDown(box(), { key: "ArrowDown" });
    await waitFor(() => expect(options()).toHaveLength(2));
    fireEvent.keyDown(box(), { key: "ArrowDown" });
    expect(scroll).toHaveBeenLastCalledWith({ block: "nearest" });
    expect(scroll.mock.contexts[scroll.mock.contexts.length - 1]).toBe(options()[1]);
  } finally {
    // @ts-expect-error jsdom 기본값(없음)으로 되돌린다
    delete Element.prototype.scrollIntoView;
  }
});

test("상대 시간: 30일부터 달, 365일부터 해 단위", () => {
  const now = Date.parse("2026-11-22T00:00:00Z");
  const at = (days: number) => new Date(now - days * 86_400_000).toISOString();
  expect(agoText(at(11), now)).toBe("11일 전");
  expect(agoText(at(29), now)).toBe("29일 전");
  expect(agoText(at(60), now)).toBe("2개월 전");
  expect(agoText(at(400), now)).toBe("1년 전");
  expect(agoText(at(731), now)).toBe("2년 전");
  setLang("en");
  expect(agoText(at(60), now)).toBe("2 months ago");
  expect(agoText(at(731), now)).toBe("2 years ago");
});
