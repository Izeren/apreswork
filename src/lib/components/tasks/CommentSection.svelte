<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import { untrack } from 'svelte';
  import type { Comment, Task } from '../../types';
  import { SYSTEM_AUTHOR } from '../../types';
  import {
    listComments,
    createComment,
    updateComment,
    deleteComment,
    apiErrorMessage,
  } from '../../api';
  import { toastState } from '../../stores/toast.svelte';
  import type { ContextMenuItem } from '../../actions/taskActions';
  import ContextMenu from '../shared/ContextMenu.svelte';
  import ConfirmDialog from '../shared/ConfirmDialog.svelte';
  import MarkdownView from '../shared/MarkdownView.svelte';
  import { formatDateTime } from '../../utils';

  interface Props {
    task: Task;
    listComments?: typeof listComments;
    createComment?: typeof createComment;
    updateComment?: typeof updateComment;
    deleteComment?: typeof deleteComment;
  }

  const {
    task,
    listComments: listCommentsProp,
    createComment: createCommentProp,
    updateComment: updateCommentProp,
    deleteComment: deleteCommentProp,
  }: Props = $props();

  let comments = $state<Comment[]>([]);
  let loading = $state(false);
  /** Shared text input: new-comment draft, or the edited content (M12.7). */
  let draft = $state('');
  let editingId = $state<string | null>(null);
  let submitting = $state(false);
  let deleteTarget = $state<Comment | null>(null);
  let menuOpenId = $state<string | null>(null);
  let menuX = $state(0);
  let menuY = $state(0);
  const isEmptyDraft = $derived(draft.trim().length === 0);

  /** Last task id the effect saw — form state resets only on a task change,
   *  not when the same task's updated_at bumps mid-typing. Plain let: read
   *  only inside the effect, never rendered. */
  let lastTaskId = '';

  function loadComments(taskId: string) {
    loading = true;
    (listCommentsProp ?? listComments)(taskId)
      .then((result) => {
        comments = result;
      })
      .catch((e) => {
        toastState.error(apiErrorMessage(e, 'Failed to load comments'));
      })
      .finally(() => {
        loading = false;
      });
  }

  // Keyed on updated_at too: completing/reopening a chunk bumps the task and
  // records a SYSTEM comment (M12.5) that must appear without a manual refresh.
  $effect(() => {
    const taskId = task.id;
    void task.updated_at;
    untrack(() => {
      if (taskId !== lastTaskId) {
        lastTaskId = taskId;
        comments = [];
        draft = '';
        editingId = null;
        menuOpenId = null;
        deleteTarget = null;
      }
      loadComments(taskId);
    });
  });

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (isEmptyDraft) return;
    submitting = true;
    try {
      if (editingId !== null) {
        await (updateCommentProp ?? updateComment)(editingId, draft);
        editingId = null;
      } else {
        await (createCommentProp ?? createComment)({ task_id: task.id, content: draft });
      }
      draft = '';
      loadComments(task.id);
    } catch (err) {
      toastState.error(apiErrorMessage(err, 'Failed to save comment'));
    } finally {
      submitting = false;
    }
  }

  function startEdit(comment: Comment) {
    editingId = comment.id;
    draft = comment.content;
  }

  function cancelEdit() {
    editingId = null;
    draft = '';
  }

  function handleMenuOpen(e: MouseEvent, comment: Comment) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menuX = rect.left;
    menuY = rect.bottom;
    menuOpenId = comment.id;
  }

  const menuItems = $derived.by((): ContextMenuItem[] => {
    const target = comments.find((c) => c.id === menuOpenId);
    if (!target) return [];
    return [
      { label: 'Edit', action: () => startEdit(target) },
      {
        label: 'Delete',
        destructive: true,
        action: () => {
          deleteTarget = target;
        },
      },
    ];
  });

  async function handleDelete() {
    const target = deleteTarget;
    deleteTarget = null;
    if (!target) return;
    try {
      await (deleteCommentProp ?? deleteComment)(target.id);
      comments = comments.filter((c) => c.id !== target.id);
      if (editingId === target.id) cancelEdit();
      toastState.success('Comment deleted');
    } catch (err) {
      toastState.error(apiErrorMessage(err, 'Failed to delete comment'));
    }
  }
</script>

<div class="comments-section">
  <span class="section-label">Comments</span>

  <form class="comment-form" onsubmit={handleSubmit}>
    <textarea
      class="comment-input"
      rows="3"
      placeholder="Add a comment… (Markdown supported)"
      aria-label={editingId !== null ? 'Edit comment' : 'Add a comment'}
      disabled={submitting}
      bind:value={draft}
    ></textarea>
    <div class="comment-form-actions">
      {#if editingId !== null}
        <button type="button" class="btn-muted btn-cancel-edit" onclick={cancelEdit}>Cancel</button>
      {/if}
      <button type="submit" class="btn-primary btn-submit" disabled={submitting || isEmptyDraft}>
        {editingId !== null ? 'Save' : 'Comment'}
      </button>
    </div>
  </form>

  {#if loading && comments.length === 0}
    <p class="comments-state">Loading comments…</p>
  {:else if comments.length === 0}
    <p class="comments-state comments-state--empty">No comments yet</p>
  {:else}
    <ul class="comments-list" aria-label="Comments">
      {#each comments as comment (comment.id)}
        {#if comment.author === SYSTEM_AUTHOR}
          <!-- System comments: smaller, muted, no author header (M12.6). -->
          <li class="comment comment--system">
            <span class="comment-system-text">{comment.content}</span>
            <span class="comment-timestamp">{formatDateTime(comment.created_at)}</span>
          </li>
        {:else}
          <li class="comment">
            <div class="comment-header">
              <span class="comment-author">{comment.author}</span>
              <span class="comment-timestamp">
                {formatDateTime(comment.created_at)}{comment.updated_at !== comment.created_at
                  ? ' (edited)'
                  : ''}
              </span>
              <button
                class="icon-btn comment-menu-btn"
                aria-label="Comment actions"
                title="Comment actions"
                onclick={(e) => handleMenuOpen(e, comment)}>⋮</button
              >
            </div>
            <div class="comment-content"><MarkdownView source={comment.content} /></div>
          </li>
        {/if}
      {/each}
    </ul>
  {/if}
</div>

<ContextMenu
  open={menuOpenId !== null}
  x={menuX}
  y={menuY}
  items={menuItems}
  onclose={() => (menuOpenId = null)}
/>

<ConfirmDialog
  open={deleteTarget !== null}
  title="Delete comment"
  message="Delete this comment? This action cannot be undone."
  confirmLabel="Delete"
  cancelLabel="Keep"
  destructive={true}
  onconfirm={handleDelete}
  oncancel={() => (deleteTarget = null)}
/>

<style>
  .comments-section {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .comment-form {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .comment-input {
    width: 100%;
    font-family: inherit;
    background: var(--color-bg);
    color: var(--color-text);
    resize: vertical;
  }

  .comment-input:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 1px;
  }

  .comment-form-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--spacing-2);
  }

  .btn-submit {
    padding: var(--spacing-1) var(--spacing-3);
  }

  .btn-cancel-edit {
    padding: var(--spacing-1) var(--spacing-3);
  }

  .comments-state {
    font-size: var(--font-size-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .comments-state--empty {
    color: var(--color-text-tertiary);
    font-style: italic;
  }

  .comments-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--spacing-2);
  }

  .comment {
    display: flex;
    flex-direction: column;
    gap: var(--spacing-1);
    padding: var(--spacing-2) var(--spacing-3);
    background: var(--color-bg-secondary);
    border-radius: var(--radius-md);
  }

  .comment-header {
    display: flex;
    align-items: center;
    gap: var(--spacing-2);
  }

  .comment-author {
    font-size: var(--font-size-xs);
    font-weight: var(--font-weight-semibold);
    color: var(--color-text);
  }

  .comment-timestamp {
    flex: 1;
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .comment-menu-btn {
    width: 22px;
    height: 22px;
  }

  .comment-content {
    font-size: var(--font-size-sm);
    color: var(--color-text);
    line-height: 1.5;
    word-break: break-word;
  }

  .comment--system {
    flex-direction: row;
    align-items: baseline;
    gap: var(--spacing-2);
    background: transparent;
    padding: 0 var(--spacing-3);
  }

  .comment-system-text {
    flex: 1;
    font-size: var(--font-size-xs);
    color: var(--color-text-tertiary);
  }

  .comment--system .comment-timestamp {
    flex: none;
    white-space: nowrap;
  }
</style>
