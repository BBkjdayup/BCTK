import js from '@eslint/js'
import pluginVue from 'eslint-plugin-vue'
import globals from 'globals'
import tseslint from 'typescript-eslint'

export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'website/dist/**',
      '.codex-tmp/**',
      'node_modules/**',
      'src-tauri/target/**',
      'src-tauri/gen/**',
    ],
  },
  {
    files: ['**/*.{js,mjs,cjs}'],
    ...js.configs.recommended,
  },
  ...tseslint.configs.recommended,
  ...pluginVue.configs['flat/essential'],
  {
    files: ['src/**/*.ts', 'src/**/*.vue'],
    languageOptions: {
      globals: globals.browser,
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-unused-vars': ['error', {
        argsIgnorePattern: '^_',
        caughtErrorsIgnorePattern: '^_',
        varsIgnorePattern: '^_',
      }],
      'vue/multi-word-component-names': 'off',
      'vue/no-v-html': 'off',
    },
  },
  {
    files: ['website/main.js'],
    languageOptions: {
      globals: globals.browser,
    },
  },
  {
    files: ['src/**/*.vue'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },
  {
    files: ['**/*.d.ts'],
    rules: {
      '@typescript-eslint/ban-types': 'off',
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-empty-object-type': 'off',
    },
  },
  {
    files: ['vite.config.ts', 'scripts/**/*.mjs'],
    languageOptions: {
      globals: globals.node,
    },
  },
)
