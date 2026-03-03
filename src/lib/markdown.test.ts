// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { renderMarkdown } from './markdown';

type RenderCase = [name: string, source: string, mustContain: string[], mustNotContain: string[]];

const cases: RenderCase[] = [
  ['script injection', '<script>alert(1)</script>', [], ['<script']],
  [
    'event handler via raw HTML',
    '<a href="https://x.com" onclick="alert(1)">x</a>',
    [],
    ['onclick'],
  ],
  ['img/onerror', '<img src=x onerror=alert(1)>', [], ['<img', 'onerror']],
  ['javascript URL', '[x](javascript:alert(1))', [], ['javascript:']],
  ['data URL', '[x](data:text/html,<script>)', [], ['data:']],
  ['relative URL', '[x](/local/path)', [], ['href=']],
  [
    'good link',
    '[site](https://example.com)',
    ['href="https://example.com"', 'rel="noopener noreferrer"'],
    [],
  ],
  ['mailto', '[m](mailto:a@b.c)', ['mailto:a@b.c'], []],
  ['task list open', '- [ ] open', ['type="checkbox"', 'disabled'], []],
  ['task list done', '- [x] done', ['type="checkbox"', 'disabled', 'checked'], []],
  ['raw input smuggling', '<input type="text" value="x">', [], ['type="text"']],
  ['bold renders', '**b**', ['<strong>'], []],
  ['heading renders', '# h', ['<h1>'], []],
  [
    'fenced code with script — escaped not executed',
    '```\n<script>alert(1)</script>\n```',
    ['&lt;script&gt;'],
    ['<script>'],
  ],
  ['line breaks', 'line1\nline2', ['<br'], []],
  [
    'svg namespace entry (mXSS vector)',
    '<svg><foreignObject><body onload=alert(1)></foreignObject></svg>',
    [],
    ['<svg', '<foreignobject', 'onload'],
  ],
  [
    'style attribute stripped (clickjacking overlay)',
    '<a href="https://ok.com" style="position:fixed;inset:0">x</a>',
    [],
    ['style='],
  ],
  ['DOM clobbering via id/name', '<a id="__proto__" name="location">x</a>', [], ['id=', 'name=']],
  ['GFM autolink', 'visit https://example.com/auto now', ['href="https://example.com/auto"'], []],
  ['table alignment survives', '| a |\n|:---:|\n| b |', ['align="center"'], []],
  ['ordered list start survives', '3. first\n4. second', ['start="3"'], []],
];

describe('renderMarkdown — sanitization (trust-boundary pin)', () => {
  it.each(cases)('%s', (_name, source, mustContain, mustNotContain) => {
    const result = renderMarkdown(source);
    for (const s of mustContain) {
      expect(result, `must contain: ${s}`).toContain(s);
    }
    for (const s of mustNotContain) {
      expect(result, `must not contain: ${s}`).not.toContain(s);
    }
  });
});
