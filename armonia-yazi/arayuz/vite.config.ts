// Armonia Yazı arayüzü. Göreli yollar: Tauri dosyadan (file/asset) ve cokluuyap.com alt yoldan yükleyebilsin.
// ARMONIA_YAZI_ORNEKLER (yalnız geliştirme): gerçek örnek belgelerin klasörü, ?ornek=/@fs/... ile açılır.
import { defineConfig, searchForWorkspaceRoot } from "vite";

const ornekler = process.env.ARMONIA_YAZI_ORNEKLER;

export default defineConfig({
  base: "./",
  server: {
    host: "127.0.0.1",
    port: 5180,
    strictPort: true,
    fs: { allow: [searchForWorkspaceRoot(process.cwd()), ...(ornekler ? [ornekler] : [])] },
  },
  build: { target: "es2022", outDir: "dist", assetsInlineLimit: 0 },
});
