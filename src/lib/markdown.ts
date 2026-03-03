// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { marked } from 'marked';
import DOMPurify from 'dompurify';

// Both marked and DOMPurify are module-global singletons configured once at
// import time; nothing else may import them (one definition per policy).
// Raw HTML in descriptions is dropped entirely — markdown-generated tags only.
marked.use({
  gfm: true,
  breaks: true, // casual plain-text descriptions keep their line breaks
  renderer: {
    // Drop any raw HTML blocks/spans; only markdown-generated tags are allowed.
    html: (): string => '',
  },
});

// Strict allowlist of what marked's GFM output can produce. No img (remote
// images from agent-supplied content are a tracking/exfil surface — deliberate).
const ALLOWED_TAGS = [
  'a',
  'p',
  'br',
  'strong',
  'em',
  'del',
  'code',
  'pre',
  'blockquote',
  'ul',
  'ol',
  'li',
  'input',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'hr',
  'table',
  'thead',
  'tbody',
  'tr',
  'th',
  'td',
];
const ALLOWED_ATTR = ['href', 'type', 'checked', 'disabled', 'start', 'align'];
// Absolute http(s)/mailto only; relative and javascript:/data: hrefs are dropped.
const ALLOWED_URI_REGEXP = /^(?:https?:|mailto:)/i;
// DOMPurify tests every non-URI-safe attribute value against ALLOWED_URI_REGEXP;
// these carry plain tokens ("checkbox", "center", "3"), not URLs, and would be
// silently stripped without this exemption.
const ADD_URI_SAFE_ATTR = ['type', 'align', 'start'];

DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  // Task-list checkboxes are render-only; any other input that slips through
  // the markdown layer is forced into the same harmless shape.
  if (node.tagName === 'INPUT') {
    node.setAttribute('type', 'checkbox');
    node.setAttribute('disabled', '');
  }
  if (node.tagName === 'A') {
    node.setAttribute('rel', 'noopener noreferrer');
  }
});

/** Markdown → sanitized HTML. The only sanctioned path to {@html} content. */
export function renderMarkdown(source: string): string {
  const html = marked(source, { async: false });
  return DOMPurify.sanitize(html, {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOWED_URI_REGEXP,
    ADD_URI_SAFE_ATTR,
    // the allowlist is exhaustive — no data-*/aria-* pass-through
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
  });
}
