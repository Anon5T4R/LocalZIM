import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Lição da suíte: forçar uma única instância do React para nenhuma
  // dependência puxar uma segunda cópia e quebrar os hooks.
  resolve: {
    dedupe: ["react", "react-dom"],
  },

  clearScreen: false,
  // Porta única do LocalZIM na suíte: 1440 (HMR 1441).
  server: {
    port: 1440,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1441,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
