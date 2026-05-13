import { defineConfig } from "vite";

export default defineConfig({
  server: {
    // Debug plugin は WebView から 127.0.0.1 を読む。Vite の default `localhost`
    // だと環境によって IPv6 loopback だけに bind され、DAW 内 WebView の解決先と
    // ずれて黒画面になり得る。
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
