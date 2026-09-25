import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { TerminalPanel } from "./TerminalPanel";
import { terminalApi, type TerminalInfo } from "../lib/terminal";
import { useDrawerMemory } from "../hooks/useDrawerMemory";

// xterm.js needs a real layout engine; the screen itself is covered by the
// Playwright suite. Here it is a labelled placeholder.
vi.mock("./TerminalView", () => ({
  TerminalView: ({ id, title }: { id: string; title: string }) => (
    <div role="group" aria-label={title} data-terminal={id} />
  ),
}));
vi.mock("../lib/terminal", async (original) => {
  const real = await original<typeof import("../lib/terminal")>();
  return {
    ...real,
    terminalApi: {
      list: vi.fn(),
      open: vi.fn(),
      input: vi.fn(),
      resize: vi.fn(),
      output: vi.fn(),
      close: vi.fn(),
    },
  };
});

const info = (n: number, over: Partial<TerminalInfo> = {}): TerminalInfo => ({
  id: `${n}`.repeat(32).slice(0, 32),
  title: `Terminal ${n}`,
  number: n,
  workspace: "/work/demo",
  shell: "/bin/bash",
  cols: 80,
  rows: 24,
  created: n,
  exited: false,
  exit_code: null,
  cursor: 0,
  ...over,
});

function Harness() {
  const memory = useDrawerMemory("/work/demo");
  return (
    <TerminalPanel
      workspace="/work/demo"
      toast={vi.fn()}
      memory={memory.memory}
      onMemory={memory.update}
    />
  );
}

beforeEach(() => {
  let next = 1;
  vi.mocked(terminalApi.open).mockImplementation(async () => info(next++));
  vi.mocked(terminalApi.close).mockResolvedValue({ ok: true });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

it("opens a shell when the project has none, and more with +", async () => {
  vi.mocked(terminalApi.list).mockResolvedValue({
    workspace: "/work/demo",
    terminals: [],
  });
  render(<Harness />);
  expect(await screen.findByRole("group", { name: "Terminal 1" })).toBeTruthy();
  expect(terminalApi.open).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "New terminal" }));
  expect(await screen.findByRole("group", { name: "Terminal 2" })).toBeTruthy();
  expect(terminalApi.open).toHaveBeenCalledTimes(2);
  // Switching tabs shows the other shell; both stay open in the engine.
  fireEvent.click(screen.getByRole("button", { name: "Terminal 1" }));
  expect(await screen.findByRole("group", { name: "Terminal 1" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Close Terminal 2" })).toBeTruthy();
  expect(screen.getByText(/the agent never sees it/)).toBeTruthy();
});

it("reattaches to running shells instead of starting new ones", async () => {
  vi.mocked(terminalApi.list).mockResolvedValue({
    workspace: "/work/demo",
    terminals: [info(1), info(2, { exited: true, exit_code: 0 })],
  });
  render(<Harness />);
  await screen.findByRole("button", { name: "Close Terminal 2" });
  expect(terminalApi.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Terminal 2 · exited" }));
  expect(await screen.findByText(/The shell exited with code 0/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Close Terminal 2" }));
  await waitFor(() =>
    expect(terminalApi.close).toHaveBeenCalledWith(info(2).id),
  );
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Close Terminal 2" })).toBe(
      null,
    ),
  );
  expect(screen.getByRole("group", { name: "Terminal 1" })).toBeTruthy();
});
