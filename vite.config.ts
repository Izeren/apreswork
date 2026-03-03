import { defineConfig } from 'vitest/config'
import { svelte } from '@sveltejs/vite-plugin-svelte'
import { svelteTesting } from '@testing-library/svelte/vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  build: {
    outDir: 'build',
  },
  test: {
    setupFiles: ['./src/lib/testSetup.ts'],
  },
})
