// Copyright 2026 Aleksandr Iushmanov (@izeren)
// SPDX-License-Identifier: Apache-2.0

import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { shouldSuppressContextMenu } from './lib/actions/suppressContextMenu';

// Suppress the native WebView right-click menu app-wide; editable fields
// (input, textarea, contenteditable) are exempt so spell-check and clipboard
// menus still appear. Ctrl+Shift+C/I remain available for dev inspection.
document.addEventListener('contextmenu', (e) => {
  if (shouldSuppressContextMenu(e.target)) e.preventDefault();
});

const app = mount(App, {
  target: document.getElementById('app')!,
});

export default app;
