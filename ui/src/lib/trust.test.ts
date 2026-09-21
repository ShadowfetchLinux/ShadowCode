import { expect, it } from "vitest";
import { isProjectTrustError, trustErrorHint, trustRequestFor } from "./trust";

it("detects the job-gate trust error so the dialog can open", () => {
  expect(
    isProjectTrustError("Error: Trust this project before starting an agent task"),
  ).toBe(true);
  expect(isProjectTrustError("Permission denied")).toBe(false);
});

it("adds a banner hint only for the job-gate trust error", () => {
  expect(
    trustErrorHint("Error: Trust this project before starting an agent task"),
  ).toBe(
    "This folder is not trusted. Click Trust this folder, then Trust and open, and send the task again.",
  );
  expect(trustErrorHint("Permission denied")).toBeUndefined();
});

it("builds a trust dialog request from the open workspace path", () => {
  expect(trustRequestFor("/home/user/app/", { level: "workspace" })).toEqual({
    path: "/home/user/app/",
    name: "app",
    permissions: { level: "workspace" },
  });
});
