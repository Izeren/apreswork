<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { renderMarkdown } from '../../markdown';
  import { openExternalUrl } from '../../api';
  import { toastState } from '../../stores/toast.svelte';

  interface Props {
    source: string;
    openUrl?: (url: string) => Promise<void>;
  }

  const { source, openUrl = openExternalUrl }: Props = $props();

  const html = $derived(renderMarkdown(source));

  // Keyboard activation of a focused link fires click via bubbling — no keydown handler needed.
  function handleClick(event: MouseEvent): void {
    const anchor = (event.target as Element).closest('a');
    if (anchor && anchor.getAttribute('href')) {
      event.preventDefault();
      openUrl((anchor as HTMLAnchorElement).href).catch(() =>
        toastState.error('Failed to open link'),
      );
    }
  }
</script>

<div class="markdown-view" role="none" onclick={handleClick}>
  <!-- Content is sanitized by the DOMPurify allowlist in markdown.ts before reaching here. -->
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  {@html html}
</div>

<style>
  .markdown-view {
    font-size: var(--font-size-sm);
    color: var(--color-text);
    line-height: 1.6;
  }

  .markdown-view :global(p) {
    margin: 0 0 0.5em;
  }

  .markdown-view :global(p:last-child) {
    margin-bottom: 0;
  }

  .markdown-view :global(ul),
  .markdown-view :global(ol) {
    margin: 0 0 0.5em;
    padding-left: 1.5em;
  }

  .markdown-view :global(li) {
    margin-bottom: 0.2em;
  }

  .markdown-view :global(h1),
  .markdown-view :global(h2),
  .markdown-view :global(h3),
  .markdown-view :global(h4),
  .markdown-view :global(h5),
  .markdown-view :global(h6) {
    margin: 0.75em 0 0.3em;
    font-weight: var(--font-weight-semibold);
    line-height: 1.3;
  }

  .markdown-view :global(h1) {
    font-size: 1.2em;
  }

  .markdown-view :global(h2) {
    font-size: 1.1em;
  }

  .markdown-view :global(h3),
  .markdown-view :global(h4),
  .markdown-view :global(h5),
  .markdown-view :global(h6) {
    font-size: 1em;
  }

  .markdown-view :global(code) {
    background: var(--color-bg-secondary, #f5f5f5);
    border-radius: 3px;
    padding: 0.1em 0.3em;
    font-size: 0.9em;
  }

  .markdown-view :global(pre) {
    background: var(--color-bg-secondary, #f5f5f5);
    border-radius: var(--radius-md, 4px);
    padding: 0.6em 0.8em;
    overflow-x: auto;
    margin: 0 0 0.5em;
  }

  .markdown-view :global(pre code) {
    background: none;
    padding: 0;
    border-radius: 0;
  }

  .markdown-view :global(blockquote) {
    border-left: 3px solid var(--color-border, #e0e0e0);
    margin: 0 0 0.5em;
    padding: 0.2em 0.6em;
    color: var(--color-text-secondary);
  }

  .markdown-view :global(table) {
    border-collapse: collapse;
    margin: 0 0 0.5em;
    font-size: 0.9em;
  }

  .markdown-view :global(th),
  .markdown-view :global(td) {
    border: 1px solid var(--color-border, #e0e0e0);
    padding: 0.3em 0.5em;
  }

  .markdown-view :global(input[type='checkbox']) {
    margin-right: 0.4em;
  }

  .markdown-view :global(a) {
    color: var(--color-primary);
    text-decoration: underline;
    cursor: pointer;
  }

  .markdown-view :global(hr) {
    border: none;
    border-top: 1px solid var(--color-border, #e0e0e0);
    margin: 0.75em 0;
  }
</style>
