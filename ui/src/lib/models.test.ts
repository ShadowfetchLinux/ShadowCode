import { expect, it } from "vitest";
import { modelLabel } from "./models";
it("distinguishes same-name endpoints without exposing credentials or query strings", () => {
  const models = [
    {
      id: "model:one",
      name: "coder",
      provider: "local",
      endpoint: "https://user:secret@example.test/one?token=secret",
    },
    {
      id: "alternate",
      name: "coder",
      provider: "local",
      endpoint: "http://localhost:9001/v1",
    },
  ];
  expect(modelLabel(models[0], models)).toBe("coder · example.test/one");
  expect(modelLabel(models[1], models)).toBe(
    "coder · localhost:9001/v1 · alternate",
  );
  expect(modelLabel(models[0], [models[0]])).toBe("coder");
});
