import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 在 dev 模式下需要固定端口，Vite 侧同步这个约定。
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
