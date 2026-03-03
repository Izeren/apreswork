// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import type { RecurringTemplate, CreateTemplateInput, UpdateTemplateInput } from '../types';
import * as api from '../api';
import { toastState } from './toast.svelte';

export interface TemplatesClient {
  listTemplates: () => Promise<RecurringTemplate[]>;
  createTemplate: (input: CreateTemplateInput) => Promise<RecurringTemplate>;
  updateTemplate: (id: string, input: UpdateTemplateInput) => Promise<RecurringTemplate>;
  deleteTemplate: (id: string) => Promise<void>;
}

const defaultClient: TemplatesClient = {
  listTemplates: api.listTemplates,
  createTemplate: api.createTemplate,
  updateTemplate: api.updateTemplate,
  deleteTemplate: api.deleteTemplate,
};

export class TemplateState {
  items: RecurringTemplate[] = $state([]);
  loading: boolean = $state(false);
  loaded: boolean = $state(false);
  selectedId: string | null = $state(null);

  selected: RecurringTemplate | undefined = $derived.by(() =>
    this.items.find((t) => t.id === this.selectedId),
  );

  readonly #client: TemplatesClient;

  constructor(client: TemplatesClient = defaultClient) {
    this.#client = client;
  }

  async load(force = false): Promise<void> {
    if (this.loading || (this.loaded && !force)) {
      return;
    }

    this.loading = true;
    try {
      this.items = await this.#client.listTemplates();
      this.loaded = true;
      if (this.selectedId && !this.items.some((template) => template.id === this.selectedId)) {
        this.selectedId = null;
      }
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to load templates'));
    } finally {
      this.loading = false;
    }
  }

  async create(input: CreateTemplateInput): Promise<RecurringTemplate | undefined> {
    try {
      const template = await this.#client.createTemplate(input);
      this.items = [...this.items, template];
      this.loaded = true;
      toastState.success('Template created');
      return template;
    } catch (e) {
      toastState.error(api.apiErrorMessage(e, 'Failed to create template'));
      return undefined;
    }
  }

  async update(id: string, input: UpdateTemplateInput): Promise<RecurringTemplate | undefined> {
    const snapshot = this.items;
    this.items = this.items.map((t) => (t.id === id ? { ...t, ...input } : t));
    try {
      const updated = await this.#client.updateTemplate(id, input);
      this.items = this.items.map((t) => (t.id === id ? updated : t));
      toastState.success('Template updated');
      return updated;
    } catch (e) {
      this.items = snapshot;
      toastState.error(api.apiErrorMessage(e, 'Failed to update template'));
      return undefined;
    }
  }

  async remove(id: string): Promise<boolean> {
    const snapshot = this.items;
    this.items = this.items.filter((t) => t.id !== id);
    try {
      await this.#client.deleteTemplate(id);
      toastState.success('Template deleted');
      if (this.selectedId === id) {
        this.selectedId = null;
      }
      return true;
    } catch (e) {
      this.items = snapshot;
      toastState.error(api.apiErrorMessage(e, 'Failed to delete template'));
      return false;
    }
  }

  async toggleActive(id: string): Promise<RecurringTemplate | undefined> {
    const template = this.items.find((t) => t.id === id);
    if (!template) return undefined;
    return this.update(id, { is_active: !template.is_active });
  }

  select(id: string | null): void {
    this.selectedId = id;
  }

  /** Drop all profile-scoped state (profile switch); `loaded` clears so the
   *  next `load()` refetches instead of serving the old profile's cache. */
  reset(): void {
    this.items = [];
    this.loading = false;
    this.loaded = false;
    this.selectedId = null;
  }
}

export const templateState = new TemplateState();
