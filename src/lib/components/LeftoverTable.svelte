<script lang="ts">
  import type { LeftoverItem } from '../types'
  import ConfidenceBadge from './ConfidenceBadge.svelte'
  import { formatSize } from '../utils'
  import { openLocation } from '../tauri-api'
  import { File, Folder, Wrench, Settings, Rocket, Globe, Clock, Check, FolderOpen, Copy, Sparkles, Ban } from '@lucide/svelte'
  import { Checkbox, Tooltip, ContextMenu } from 'bits-ui'
  import { getSettings, updateSettings } from '../stores/settings.svelte'

  let {
    items,
    selectedItems,
    onSelect,
    onAnalyzeSingle,
    lockedIds = new Set<string>()
  }: {
    items: LeftoverItem[]
    selectedItems: Set<string>
    onSelect: (id: string) => void
    onAnalyzeSingle?: (item: LeftoverItem) => void
    /** Items under an app install dir that is still installed — cannot be selected. */
    lockedIds?: Set<string>
  } = $props()

  function getTypeIcon(type: string) {
    switch (type) {
      case 'File': return File
      case 'Folder': return Folder
      case 'Registry': return Wrench
      case 'Service': return Settings
      case 'Startup': return Rocket
      case 'Hosts': return Globe
      case 'Task': return Clock
      default: return File
    }
  }

  function isReadOnly(item: LeftoverItem) {
    return item.item_type === 'Hosts' || item.item_type === 'Task'
  }

  async function handleOpen(item: LeftoverItem) {
    if (isReadOnly(item)) return
    try { await openLocation(item.path) } catch (e) { console.error(e) }
  }

  async function handleCopy(item: LeftoverItem) {
    try { await navigator.clipboard.writeText(item.path) } catch {}
  }

  function handleExclude(item: LeftoverItem) {
    const s = getSettings()
    if (item.item_type === 'Hosts') {
      const list = new Set(s.excludedHosts ?? [])
      list.add(item.path.trim())
      updateSettings({ excludedHosts: Array.from(list) })
    } else {
      const list = new Set(s.excludedPaths ?? [])
      list.add(item.path)
      updateSettings({ excludedPaths: Array.from(list) })
    }
  }

  let aiEnabled = $derived(getSettings().aiEnabled && getSettings().apiKey.trim().length > 0)
</script>

<div class="table-scroll">
  <div class="table">
  <div class="table-header">
    <div class="col-checkbox"></div>
    <div class="col-type">Type</div>
    <div class="col-path">Path</div>
    <div class="col-size">Size</div>
    <div class="col-confidence">Confidence</div>
  </div>

  {#each items as item (item.id)}
    {@const IconComp = getTypeIcon(item.item_type)}
    <ContextMenu.Root>
      <ContextMenu.Trigger>
        <div class="table-row" class:selected={selectedItems.has(item.id)} class:locked={lockedIds.has(item.id)} title={lockedIds.has(item.id) ? 'Still installed — run the native uninstaller first, then delete this' : undefined}>
          <div class="col-checkbox">
            <Checkbox.Root
              checked={selectedItems.has(item.id)}
              onCheckedChange={() => onSelect(item.id)}
              disabled={lockedIds.has(item.id)}
              class="checkbox-root"
              aria-label="Select {item.path}"
            >
              <span class="checkbox-indicator">
                <Check size={11} strokeWidth={2.6} />
              </span>
            </Checkbox.Root>
          </div>
          <div class="col-type">
            <div class="type-icon-wrapper">
              <IconComp size={14} />
            </div>
          </div>
          <div class="col-path">
            <Tooltip.Root>
              <Tooltip.Trigger>
                <span class="path-text font-mono">{item.path}</span>
              </Tooltip.Trigger>
              <Tooltip.Content class="tooltip-content" sideOffset={4}>
                {item.path}
              </Tooltip.Content>
            </Tooltip.Root>
          </div>
          <div class="col-size font-mono">{formatSize(item.size)}</div>
          <div class="col-confidence">
            <ConfidenceBadge tier={item.confidence_tier} />
          </div>
        </div>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content class="ctx-content">
          {#if isReadOnly(item)}
            <ContextMenu.Item class="ctx-item" onclick={() => handleCopy(item)}>
              <Copy size={13} strokeWidth={1.75} />
              Copy path
            </ContextMenu.Item>
            <ContextMenu.Item class="ctx-item" onclick={() => handleExclude(item)}>
              <Ban size={13} strokeWidth={1.75} />
              Never flag this path
            </ContextMenu.Item>
            <ContextMenu.Separator class="ctx-sep" />
            <div class="ctx-hint">Read-only — review only</div>
          {:else}
            <ContextMenu.Item class="ctx-item" onclick={() => handleOpen(item)}>
              <FolderOpen size={13} strokeWidth={1.75} />
              Open location
            </ContextMenu.Item>
            <ContextMenu.Item class="ctx-item" onclick={() => handleCopy(item)}>
              <Copy size={13} strokeWidth={1.75} />
              Copy path
            </ContextMenu.Item>
            <ContextMenu.Item class="ctx-item" onclick={() => handleExclude(item)}>
              <Ban size={13} strokeWidth={1.75} />
              Never flag this path
            </ContextMenu.Item>
            {#if aiEnabled && onAnalyzeSingle}
              <ContextMenu.Separator class="ctx-sep" />
              <ContextMenu.Item class="ctx-item" onclick={() => onAnalyzeSingle(item)}>
                <Sparkles size={13} strokeWidth={1.75} />
                Analyze with AI
              </ContextMenu.Item>
            {/if}
          {/if}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  {:else}
    <div class="empty">No items found</div>
  {/each}
  </div>
</div>

<style>
  .table-scroll {
    overflow-x: auto;
  }

  .table {
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    overflow: hidden;
    min-width: 620px;
  }

  .table-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    background-color: var(--color-bg);
    border-bottom: 1px dashed var(--color-border);
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .table-row {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px;
    border-bottom: 1px dashed var(--color-border);
    transition: background-color 0.1s ease;
  }

  .table-row:last-child {
    border-bottom: none;
  }

  .table-row:hover {
    background-color: var(--color-accent-soft);
  }

  .table-row.selected {
    background-color: var(--color-accent-soft);
  }

  .table-row.locked {
    opacity: 0.55;
    background-color: color-mix(in srgb, var(--color-accent-soft) 40%, transparent);
  }

  .table-row.locked .path-text {
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--color-accent) 40%, transparent);
  }

  .col-checkbox {
    width: 32px;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .col-type {
    width: 40px;
    flex-shrink: 0;
    display: flex;
    justify-content: center;
  }

  .type-icon-wrapper {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .col-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .path-text {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    display: block;
    cursor: default;
    /* RTL direction puts the ellipsis at the string's START: when a path
       overflows, the drive/folder prefix collapses but the filename at the
       end stays visible — and the column uses all available width. */
    direction: rtl;
    text-align: left;
  }

  .col-size {
    width: 80px;
    flex-shrink: 0;
    font-size: 12px;
    color: var(--color-text-secondary);
    text-align: right;
  }

  .col-confidence {
    width: 80px;
    flex-shrink: 0;
  }

  .empty {
    padding: 48px;
    text-align: center;
    color: var(--color-text-secondary);
    font-size: 13px;
  }

  :global(.tooltip-content) {
    background-color: var(--color-text-primary);
    color: white;
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12px;
    max-width: 400px;
    word-break: break-all;
    z-index: 50;
    animation: fadeIn 0.1s ease;
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

  :global(.ctx-sep) {
    height: 1px;
    background: var(--color-border);
    margin: 4px -4px;
  }

  :global(.ctx-hint) {
    padding: 6px 10px;
    font-size: 11px;
    color: var(--color-text-secondary);
    font-style: italic;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes ctxIn {
    from { opacity: 0; transform: scale(0.98) translateY(2px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }
</style>
