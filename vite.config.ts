import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// Tauri 通过 devUrl / frontendDist 与这里的配置对应：
// - 开发时固定 5173 端口，端口被占用直接失败而不是静默换端口
// - 产物输出到 dist-ui，避免与站点产物目录 dist/ 混淆
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: 'dist-ui',
    emptyOutDir: true,
    target: 'esnext',
    sourcemap: false,
  },
})
