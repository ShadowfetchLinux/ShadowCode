/**
 * Fake preview routes for the Playwright suite. Install it after
 * `installFakeBackend`: it wraps that bridge, answers `/api/preview…` and
 * passes everything else through. The "proxy" it hands out is an origin the
 * spec serves with `page.route` (a tiny static page plus the real picker
 * script from native/core/src/preview/picker.js).
 *
 * Self-contained like fakeBackend.ts (Playwright serialises it).
 */
export type FakePreviewOptions = {
  /** Origin the spec serves the previewed page from. */
  proxyOrigin: string;
};

export function installFakePreview(options: FakePreviewOptions) {
  const inner = (window as any).__SHADOW_TEST_TRANSPORT__;
  if (!inner) throw new Error("installFakeBackend must run first");
  const fake = (window as any).__SHADOW_FAKE__;
  const log: { method: string; path: string; body: any }[] = fake.log;
  const loopback = (host: string) =>
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host === "[::1]" ||
    /^127\.\d+\.\d+\.\d+$/.test(host);

  function route(method: string, path: string, body: any): unknown {
    if (path === "/api/preview/servers" && method === "GET")
      return {
        workspace: "/work/demo",
        servers: [
          {
            port: 5173,
            url: "http://localhost:5173/settings",
            source: "background",
            listening: true,
            pid: 4242,
            process: "node",
            command: "node vite",
            background_id: "bg1",
            background_name: "dev",
          },
        ],
      };
    if (path === "/api/preview/open" && method === "POST") {
      const url = new URL(String(body?.url || ""));
      if (url.protocol !== "http:" || !loopback(url.hostname))
        throw new Error(
          "The preview only opens servers on this computer (localhost, *.localhost, 127.0.0.1 or [::1])",
        );
      if (body?.app_origin !== window.location.origin)
        throw new Error("The preview can only be embedded by the window");
      const rest = url.pathname + url.search + url.hash;
      return {
        proxy_origin: options.proxyOrigin,
        proxy_port: Number(new URL(options.proxyOrigin).port),
        target_origin: url.origin,
        url: options.proxyOrigin + rest,
        target_url: url.href,
      };
    }
    return undefined;
  }

  (window as any).__SHADOW_TEST_TRANSPORT__ = {
    ...inner,
    async request(path: string, method: string, body: unknown) {
      const answer = route(method, path, body);
      if (answer === undefined) return inner.request(path, method, body);
      log.push({ method, path, body });
      return JSON.parse(JSON.stringify(answer));
    },
  };
}
