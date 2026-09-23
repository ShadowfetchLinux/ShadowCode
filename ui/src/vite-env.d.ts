/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" only for the Playwright build that injects a fake backend. */
  readonly VITE_SHADOW_TEST_TRANSPORT?: string;
}
