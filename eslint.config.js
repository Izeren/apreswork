// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import js from '@eslint/js';
import ts from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import sonarjs from 'eslint-plugin-sonarjs';
import importX from 'eslint-plugin-import-x';
import prettier from 'eslint-config-prettier';
import svelteParser from 'svelte-eslint-parser';
import globals from 'globals';

export default ts.config(
  js.configs.recommended,
  ...ts.configs.recommended,
  ...svelte.configs['flat/recommended'],
  sonarjs.configs.recommended,
  prettier,
  ...svelte.configs['flat/prettier'],
  {
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: {
      parser: svelteParser,
      parserOptions: {
        parser: ts.parser,
        extraFileExtensions: ['.svelte'],
      },
    },
    rules: {
      // `void expr;` is the Svelte idiom for pinning an $effect dependency
      // without using its value; a bare `expr;` would trip no-unused-expressions.
      'sonarjs/void-use': 'off',
    },
  },
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.es2021,
      },
    },
    plugins: { 'import-x': importX },
    settings: {
      'import-x/resolver': { node: { extensions: ['.ts', '.svelte', '.js'] } },
    },
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      'import-x/no-cycle': ['error', { ignoreExternal: true }],
      // The DI migration removed module-level stubbing from the suite. This keeps it
      // removed: a new vi.mock is a new module the test no longer really exercises.
      'no-restricted-syntax': [
        'error',
        {
          selector:
            "CallExpression[callee.object.name='vi'][callee.property.name=/^(mock|doMock)$/]",
          message:
            'vi.mock replaces a whole module for the file, so the test stops exercising the real one and keeps passing when it changes. Inject the collaborator instead — pass an api object as a prop, or take the dependency as a parameter. If a module singleton with import-time side effects leaves no seam, make it injectable rather than stubbing it; an eslint-disable here needs the reason in the comment.',
        },
      ],
    },
  },
  { ignores: ['build/', 'node_modules/', 'src-tauri/', 'dist/'] },
);
