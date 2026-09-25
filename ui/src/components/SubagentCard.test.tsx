import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SubagentRun } from "../lib/subagents";

const mocks = vi.hoisted(() => ({ agents: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import { SubagentCard } from "./SubagentCard";
import { Composer } from "./Composer";
import { mentionQuery } from "./AgentMentions";

const run = (over: Partial<SubagentRun> = {}): SubagentRun => ({
  runId: "r1",
  agent: "general",
  description: "update the note",
  prompt: "Change old to new",
  mode: "write",
  model: "fixture",
  status: "completed",
  summary: "Changed **note.txt**",
  jobId: "j1",
  sessionId: "child-session",
  files: [{ path: "note.txt", status: "modified", additions: 1, deletions: 1 }],
  filesTruncated: false,
  binaryFiles: [],
  patch: true,
  applied: false,
  notes: [],
  steps: 3,
  tokens: 900,
  durationS: 2.5,
  ...over,
});

afterEach(cleanup);

describe("SubagentCard", () => {
  it("is one line until opened, then shows the result and opens the transcript", () => {
    const onOpen = vi.fn();
    render(<SubagentCard run={run()} onOpen={onOpen} />);
    const head = screen.getByRole("button", { name: /@general/ });
    expect(head.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByRole("status").textContent).toContain("Done");
    expect(screen.queryByText("note.txt")).toBeNull();
    fireEvent.click(head);
    expect(head.getAttribute("aria-expanded")).toBe("true");
    expect(
      screen.getByRole("list", { name: "Changed files" }).textContent,
    ).toContain("note.txt+1 −1");
    expect(screen.getByText(/reviews the diff and applies it/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Open transcript/ }));
    expect(onOpen).toHaveBeenCalledWith("child-session");
  });

  it("shows a running read-only run and a failure", () => {
    const { rerender } = render(
      <SubagentCard
        run={run({ status: "running", mode: "read-only", files: [] })}
      />,
    );
    expect(screen.getByRole("status").textContent).toContain("Working");
    expect(screen.getByText("read-only")).toBeTruthy();
    rerender(
      <SubagentCard
        run={run({ status: "failed", summary: "", error: "Model offline" })}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /@general/ }));
    expect(screen.getByText("Model offline")).toBeTruthy();
  });
});

describe("@ agent mentions", () => {
  it("only opens for a message that is just @name", () => {
    expect(mentionQuery("@")).toBe("");
    expect(mentionQuery("@exp")).toBe("exp");
    expect(mentionQuery("@explore find")).toBeNull();
    expect(mentionQuery("mail a@b")).toBeNull();
  });

  it("suggests agents in the composer and completes with Tab", async () => {
    mocks.agents.mockResolvedValue({
      agents: [
        {
          name: "explore",
          description: "Search",
          mode: "read-only",
          source: "builtin",
          path: "",
        },
        {
          name: "general",
          description: "Edits",
          mode: "write",
          source: "builtin",
          path: "",
        },
      ],
      issues: [],
    });
    const onTask = vi.fn();
    const promptRef = { current: null as HTMLTextAreaElement | null };
    render(
      <Composer
        task="@ex"
        onTask={onTask}
        promptRef={promptRef}
        attachments={[]}
        onRemoveAttachment={() => {}}
        onAttach={() => {}}
        canAttachImages={false}
        attachDisabled={false}
        picker={null}
        controls={null}
        commands={[]}
        placeholder=""
        hint=""
        busy={false}
        queueing={false}
        submitting={false}
        locked={false}
        canSend
        sendBlocked={null}
        stopDisabled={false}
        onSubmit={() => {}}
        onStop={() => {}}
      />,
    );
    const menu = await screen.findByRole("listbox", { name: "Subagents" });
    expect(menu.textContent).toContain("@explore");
    expect(menu.textContent).not.toContain("@general");
    fireEvent.keyDown(screen.getByLabelText("Message ShadowCode"), {
      key: "Tab",
    });
    expect(onTask).toHaveBeenCalledWith("@explore ");
  });
});
