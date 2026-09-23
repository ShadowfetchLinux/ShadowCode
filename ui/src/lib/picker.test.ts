import { expect, it } from "vitest";
import {
  availabilityLabel,
  groupTargets,
  isSelectable,
  usageLabel,
  type PickerTarget,
} from "./picker";

const cursor: PickerTarget = {
  id: "cli:cursor:auto",
  provider: "cli:cursor",
  group: "subscriptions",
  name: "Cursor · Auto",
  availability: "ready",
  featured: true,
  usage: { state: "unavailable", label: "Usage unavailable · Open Cursor usage" },
};
const sonnetClaude: PickerTarget = {
  id: "cli:claude:sonnet",
  provider: "cli:claude",
  group: "subscriptions",
  name: "Claude Code · sonnet",
  availability: "sign_in",
  featured: true,
  usage: { state: "unavailable" },
};
const local: PickerTarget = {
  id: "local:gguf:abc",
  provider: "llamacpp",
  group: "local",
  name: "qwen · This computer",
  inference: "local",
  availability: "setup_required",
  usage: { state: "local", label: "Runs on this computer · No subscription quota" },
};

it("never fabricates remaining percents", () => {
  expect(usageLabel(cursor.usage)).toBe("Usage unavailable · Open Cursor usage");
  expect(usageLabel({ state: "ok", remaining_percent: 40, label: "40% remaining" })).toBe(
    "40% remaining",
  );
  expect(usageLabel({ state: "stale", label: "Last checked 2m ago" })).toBe(
    "Last checked 2m ago",
  );
  expect(usageLabel(undefined, "local")).toContain("No subscription quota");
  expect(usageLabel({ state: "unavailable" })).not.toMatch(/72%|unlimited|100%/);
});

it("keeps the same model on two providers as two rows", () => {
  const groups = groupTargets([cursor, sonnetClaude, local]);
  expect(groups.subscriptions.map((t) => t.id)).toEqual([
    "cli:cursor:auto",
    "cli:claude:sonnet",
  ]);
  expect(groups.local.map((t) => t.id)).toEqual(["local:gguf:abc"]);
  expect(isSelectable(cursor)).toBe(true);
  expect(isSelectable(sonnetClaude)).toBe(false);
  expect(availabilityLabel(sonnetClaude)).toBe("Sign in");
});
