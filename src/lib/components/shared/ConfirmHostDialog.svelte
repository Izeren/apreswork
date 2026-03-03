<!-- Copyright 2026 Aleksandr Iushmanov (@izeren) -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  import ConfirmDialog from './ConfirmDialog.svelte';
  import type { ConfirmHost } from '../../actions/confirmHost.svelte';

  interface Props {
    host: ConfirmHost;
  }

  const { host }: Props = $props();
</script>

<ConfirmDialog
  open={host.spec !== null}
  title={host.spec?.title ?? ''}
  message={host.spec?.message ?? ''}
  confirmLabel={host.spec?.confirmLabel ?? 'Confirm'}
  destructive={host.spec?.destructive ?? false}
  onconfirm={() => host.settle(true)}
  oncancel={() => host.settle(false)}
/>
