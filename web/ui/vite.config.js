import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.dirname(fileURLToPath(import.meta.url))

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      '/v1': {
        target: 'http://127.0.0.1:7480',
        changeOrigin: true,
        ws: true,
      },
    },
  },
  build: {
    outDir: path.resolve(root, '../../crates/pertisk-daemon/static'),
    emptyOutDir: true,
  },
})
