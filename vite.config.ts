import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
  },
  build: {
    // 沙箱环境中 Node 删除 dist 会被安全删除层拦截（trash 失败导致构建中止）。
    // 每次构建前由脚本/手动清空 dist，vite 只负责写入，因此关闭内置清空。
    emptyOutDir: false,
  },
});
