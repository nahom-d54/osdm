import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/ui",
  fullyParallel: false,
  use: {
    baseURL: "http://127.0.0.1:1421",
    viewport: { width: 1440, height: 1000 },
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm exec vite preview --port 1421 --host 127.0.0.1",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: !process.env.CI,
  },
});
