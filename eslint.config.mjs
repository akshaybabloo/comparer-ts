// @ts-check
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import prettier from 'eslint-config-prettier';

export default [
  // 1. Tell ESLint to ignore specific folders (like build outputs)
  {
    ignores: ['dist/', 'node_modules/', 'build/']
  },
  // 2. Apply standard JavaScript and TypeScript configurations
  js.configs.recommended,
  ...tseslint.configs.recommended,
  // 3. Custom rule overrides (optional)
  {
    rules: {
      '@typescript-eslint/no-explicit-any': 'warn', // Warns instead of throwing an error for 'any'
      'no-console': 'off'
    }
  },
  // 4. Prettier integration to avoid conflicts with ESLint
  prettier
];
