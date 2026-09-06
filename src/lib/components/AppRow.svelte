<script lang="ts">
  import type { AppInfo } from '../types'
  import { formatSize } from '../utils'
  import { openLocation } from '../tauri-api'
  import { Trash2, FolderOpen, Copy, Check } from '@lucide/svelte'
  import { ContextMenu } from 'bits-ui'

  let { app, onUninstall }: { app: AppInfo; onUninstall: () => void } = $props()
  let iconError = $state(false)
  let copied = $state(false)

  function formatDate(date: string | undefined): string {
    if (!date) return '—'
    if (date.length === 8) {
      return `${date.slice(0, 4)}-${date.slice(4, 6)}-${date.slice(6, 8)}`
    }
    return date
  }

  async function handleOpenLocation() {
    const loc = app.install_location?.trim().replace(/^"|"$/g, '')
    if (!loc) return
    try { await openLocation(loc) } catch (e) { console.error(e) }
  }

  async function handleCopy() {
    const info = `${app.name}${app.version ? ` v${app.version}` : ''}${app.publisher ? ` — ${app.publisher}` : ''}`
    try {
      await navigator.clipboard.writeText(info)
      copied = true
      setTimeout(() => copied = false, 1200)
    } catch {}
  }

  let canOpen = $derived(!!app.install_location?.trim())
</script>

<ContextMenu.Root>
  <ContextMenu.Trigger class="app-row-trigger">
    <div class="app-row">
      <div class="app-icon">
        {#if app.icon && !iconError}
          <img
            src="data:image/png;base64,{app.icon}"
            alt="{app.name} icon"
            class="app-icon-img"
            onerror={() => iconError = true}
          />
        {:else}
          <span class="icon-letter">{app.name.charAt(0).toUpperCase()}</span>
        {/if}
      </div>
      <div class="app-info">
        <div class="app-name">{app.name}</div>
        <div class="app-publisher">{app.publisher || 'Unknown publisher'}</div>
      </div>
      <div class="app-version">{app.version || '—'}</div>
      <div class="app-size">{formatSize(app.estimated_size)}</div>
      <div class="app-date">{formatDate(app.install_date)}</div>
      <button class="btn-icon-danger" onclick={onUninstall} title="Uninstall" aria-label="Uninstall {app.name}">
        <Trash2 size={15} />
      </button>
    </div>
  </ContextMenu.Trigger>
  <ContextMenu.Portal>
    <ContextMenu.Content class="ctx-content">
      <ContextMenu.Item class="ctx-item" disabled={!canOpen} onclick={handleOpenLocation}>
        <FolderOpen size={13} strokeWidth={1.75} />
        Open install location
      </ContextMenu.Item>
      <ContextMenu.Item class="ctx-item" onclick={handleCopy}>
        {#if copied}
          <Check size={13} strokeWidth={1.75} />
          Copied
        {:else}
          <Copy size={13} strokeWidth={1.75} />
          Copy app info
        {/if}
      </ContextMenu.Item>
    </ContextMenu.Content>
  </ContextMenu.Portal>
</ContextMenu.Root>

<style>
  :global(.app-row-trigger) {
    display: block;
    width: 100%;
  }

  .app-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px dashed var(--color-border);
    transition: background-color 0.1s ease;
  }

  .app-row:last-child {
    border-bottom: none;
  }


  .app-icon {
    width: 36px;
    height: 36px;
    border-radius: 6px;
    background-color: var(--color-bg);
    border: 1px solid var(--color-border);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    overflow: hidden;
  }

  .app-icon-img {
    width: 28px;
    height: 28px;
    object-fit: contain;
  }

  .icon-letter {
    font-size: 14px;
    font-weight: 600;
    color: var(--color-accent);
  }

  .app-info {
    flex: 1;
    min-width: 0;
  }

  .app-name {
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .app-publisher {
    font-size: 12px;
    color: var(--color-text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .app-version,
  .app-size,
  .app-date {
    font-size: 12px;
    color: var(--color-text-secondary);
    min-width: 80px;
  }

  .app-date {
    min-width: 100px;
  }

  .btn-icon-danger {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: var(--color-surface);
    color: var(--color-text-secondary);
    cursor: pointer;
    flex-shrink: 0;
    -webkit-font-smoothing: antialiased;
    transition-property: background-color, border-color, color, transform;
    transition-duration: 160ms;
    transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
  }

  .btn-icon-danger:hover {
    background: var(--color-bg);
    border-color: var(--color-border);
    color: var(--color-text-primary);
  }

  .btn-icon-danger:active {
    transform: scale(0.97);
  }

  .btn-icon-danger:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  :global(.ctx-content) {
    min-width: 200px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 4px;
    box-shadow: 0 8px 24px rgba(0,0,0,0.08), 0 2px 8px rgba(0,0,0,0.06);
    z-index: 60;
    animation: ctxIn 0.12s cubic-bezier(0.16, 1, 0.3, 1);
  }

  :global(.ctx-item) {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 10px;
    border-radius: 6px;
    font-size: 12.5px;
    font-weight: 500;
    letter-spacing: -0.01em;
    color: var(--color-text-primary);
    cursor: pointer;
    outline: none;
    transition: background-color 0.12s ease, color 0.12s ease;
    -webkit-font-smoothing: antialiased;
  }

  :global(.ctx-item:hover),
  :global(.ctx-item[data-highlighted]) {
    background: var(--color-accent-soft);
    color: var(--color-text-primary);
  }

  :global(.ctx-item[data-disabled]) {
    opacity: 0.45;
    cursor: default;
  }

  @keyframes ctxIn {
    from { opacity: 0; transform: scale(0.98) translateY(2px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }
</style>
