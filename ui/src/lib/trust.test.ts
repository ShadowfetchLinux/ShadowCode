import { expect, it } from "vitest";
import { isProjectTrustError, trustRequestFor } from "./trust";

it("detects the job-gate trust error so the dialog can open", () => {
  expect(
    isProjectTrustError("Error: Trust this project before starting an agent task"),
  ).toBe(true);
  expect(isProjectTrustError("Permission denied")).toBe(false);
});

it("builds a trust dialog request from the open workspace path", () => {
  expect(trustRequestFor("/home/user/app/", { level: "workspace" })).toEqual({
    path: "/home/user/app/",
    name: "app",
    permissions: { level: "workspace" },
  });
});
