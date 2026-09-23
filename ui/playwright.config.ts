import { defineConfig } from "@playwright/test";

// The suite runs against `vite preview` of a production build made with
// VITE_SHADOW_TEST_TRANSPORT=1. Each test injects the deterministic fake engine
// from e2e/fakeBackend.ts; the regular production build ignores it
// (e2e/check-bundle.mjs proves the fake is absent from that bundle).
const port = 4178;

export default defineConfig({
  testDir: "./e2e",
  testMatch: /.*\.spec\.ts/,
  fullyParallel: false,
  workers: 1,
  timeout: 45000,
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    viewport: { width: 1440, height: 1000 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  webServer: {
    command: `npm run build:e2e && npx vite preview --outDir dist-e2e --port ${port} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 120000,
  },
});
