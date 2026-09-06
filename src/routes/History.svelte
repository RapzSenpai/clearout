<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { Trash2, FileText, HardDrive, Clock, Archive, RotateCcw, RefreshCw, X } from '@lucide/svelte'

  interface HistoryEntry {
    path: string
    filename: string
    app_name: string
    timestamp: string
    size: number
  }

  interface TrashEntry { id: string; original_path: string; trash_path: string; timestamp: string }
  interface RegBackup { id: string; created: string; root_path: string; key_count: number; value_count: number }

  let entries: HistoryEntry[] = $state([])
  let trash: TrashEntry[] = $state([])
  let regBackups: RegBackup[] = $state([])
  let loading = $state(true)
  // Button spin only for user-clicked refresh — initial mount load stays
  // quiet, matching the Dashboard.
  let refreshing = $state(false)
  let preview = $state('')
  let showPreview = $state(false)
  let tab: 'reports' | 'trash' | 'registry' = $state('reports')

  // Typed confirmation for the highest-stakes clear (registry backups)
  let showClearRegistry = $state(false)
  let clearRegistryInput = $state('')
  let clearing = $state(false)
  let actionError = $state('')

  // In-app confirm dialog — replaces the WebView's native confirm(), which
  // prefixes every dialog with the page origin ("localhost:5173 says …").
  interface ConfirmSpec {
    title: string
    body: string
    label: string
    tone: 'danger' | 'default'
    action: () => Promise<void>
  }
  let confirmState = $state<ConfirmSpec | null>(null)

  function askConfirm(spec: ConfirmSpec) {
    confirmState = spec
  }

  async function acceptConfirm() {
    const action = confirmState?.action
    confirmState = null
    if (action) await action()
  }

  function errorText(e: unknown) {
    return e instanceof Error ? e.message : String(e)
  }

  let activeCount = $derived(
    tab === 'reports' ? entries.length : tab === 'trash' ? trash.length : regBackups.length
  )

  async function load(initial = false) {
    if (initial) {
      loading = true
    } else {
      refreshing = true
      loading = true
    }
    try {
      // Minimum spinner time so the refresh animation is visible even when
      // the three invokes return in a few milliseconds. Initial load skips it.
      const minSpin = initial ? Promise.resolve() : new Promise(res => setTimeout(res, 500))
      const [e, t, r] = await Promise.all([
        invoke<HistoryEntry[]>('list_reports'),
        invoke<TrashEntry[]>('list_trash'),
        invoke<RegBackup[]>('list_registry_backups'),
        minSpin
      ])
      entries = e
      trash = t
      regBackups = r
    } catch (e) {
      console.error(e)
    } finally {
      loading = false
      refreshing = false
    }
  }

  async function clearAll() {
    if (clearing) return
    if (tab === 'reports') {
      askConfirm({
        title: `Delete all ${entries.length} reports?`,
        body: 'This only removes the report files — nothing on your system is affected.',
        label: 'Delete reports',
        tone: 'danger',
        action: runClear
      })
    } else if (tab === 'trash') {
      askConfirm({
        title: `Permanently delete all ${trash.length} items in soft trash?`,
        body: 'These files were removed by ClearOut and can no longer be restored.',
        label: 'Delete permanently',
        tone: 'danger',
        action: runClear
      })
    } else {
      clearRegistryInput = ''
      showClearRegistry = true
    }
  }

  async function runClear() {
    clearing = true
    actionError = ''
    try {
      if (tab === 'reports') await invoke('clear_reports')
      else if (tab === 'trash') await invoke('clear_trash')
      else await invoke('clear_registry_backups')
      showClearRegistry = false
      await load()
    } catch (e) {
      console.error(e)
      actionError = `Clear failed: ${errorText(e)}`
    } finally {
      clearing = false
    }
  }

  async function restoreReg(id: string) {
    askConfirm({
      title: 'Restore this registry key/value?',
      body: 'Only do this if the app is gone — restoring can overwrite newer values.',
      label: 'Restore',
      tone: 'default',
      action: async () => {
        try {
          await invoke('restore_registry_backup', { id })
          await load()
        } catch (e) {
          console.error(e)
          actionError = `Restore failed: ${errorText(e)}`
        }
      }
    })
  }

  async function removeReg(id: string) {
    askConfirm({
      title: 'Delete this registry backup permanently?',
      body: 'This backup is the only way to restore the registry keys it contains.',
      label: 'Delete backup',
      tone: 'danger',
      action: async () => {
        try {
          await invoke('delete_registry_backup', { id })
          actionError = ''
          await load()
        } catch (e) {
          console.error(e)
          actionError = `Delete failed: ${errorText(e)}`
        }
      }
    })
  }

  async function restore(id: string) {
    try { await invoke('restore_trash', { id }); await load() } catch (e) {
      console.error(e)
      actionError = `Restore failed: ${errorText(e)}`
    }
  }

  async function viewEntry(path: string) {
    try {
      preview = await invoke<string>('load_report', { path })
      showPreview = true
    } catch (e) {
      console.error(e)
    }
  }

  async function removeEntry(path: string) {
    askConfirm({
      title: 'Delete this report?',
      body: 'This only removes the report file — nothing on your system is affected.',
      label: 'Delete report',
      tone: 'danger',
      action: async () => {
        try {
          await invoke('delete_report', { path })
          actionError = ''
          await load()
        } catch (e) {
          console.error(e)
          actionError = `Delete failed: ${errorText(e)}`
        }
      }
    })
  }

  function formatSize(bytes: number) {
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  }

  onMount(() => load(true))
</script>

<div class="history">
  <div class="header">
    <div>
      <h1>History</h1>
      <p class="subtitle">Reports & soft trash (7-day auto purge)</p>
    </div>
    <button class="btn-neo btn-neo--ai" onclick={() => load()} disabled={loading} aria-label="Refresh history">
      <RefreshCw size={13} strokeWidth={1.75} class={refreshing ? 'spin' : ''} />
      {refreshing ? 'Refreshing…' : 'Refresh'}
    </button>
  </div>

  <div class="tabs">
    <button class="tab" class:active={tab==='reports'} onclick={() => tab='reports'}><FileText size={12} strokeWidth={1.75} /> Reports ({entries.length})</button>
    <button class="tab" class:active={tab==='trash'} onclick={() => tab='trash'}><Archive size={12} strokeWidth={1.75} /> Trash ({trash.length})</button>
    <button class="tab" class:active={tab==='registry'} onclick={() => tab='registry'}><HardDrive size={12} strokeWidth={1.75} /> Registry backups ({regBackups.length})</button>
    {#if activeCount > 0 && !loading}
      <button class="clear-all" onclick={clearAll} disabled={clearing} aria-label="Clear all items in this tab">
        <Trash2 size={12} strokeWidth={1.75} />
        {clearing ? 'Clearing…' : `Clear all (${activeCount})`}
      </button>
    {/if}
  </div>

  {#if actionError}
    <div class="error-banner" role="alert">
      <span>{actionError}</span>
      <button class="error-dismiss" onclick={() => actionError = ''} aria-label="Dismiss error">
        <X size={13} strokeWidth={1.75} />
      </button>
    </div>
  {/if}

  {#if loading}
    <div class="empty">Loading…</div>
  {:else if tab==='reports'}
    {#if entries.length === 0}
      <div class="empty">No reports yet. Run a scan and delete to create one.</div>
    {:else}
      <div class="list">
        {#each entries as e (e.path)}
          <div class="row">
            <div class="row-icon">
              <FileText size={14} />
            </div>
            <div class="row-info">
              <div class="row-name">{e.app_name}</div>
              <div class="row-meta">
                <Clock size={11} />
                <span>{e.timestamp || e.filename}</span>
                <span class="dot">•</span>
                <HardDrive size={11} />
                <span>{formatSize(e.size)}</span>
              </div>
            </div>
            <div class="row-actions">
              <button class="btn-neo btn-neo--ai" onclick={() => viewEntry(e.path)}>View</button>
              <button class="btn-icon-danger" onclick={() => removeEntry(e.path)} aria-label="Delete report">
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else if tab==='trash'}
    {#if trash.length === 0}
      <div class="empty">Trash empty. Deleted files go here for 7 days.</div>
    {:else}
      <div class="list">
        {#each trash as t (t.id)}
          <div class="row">
            <div class="row-icon"><Archive size={14} /></div>
            <div class="row-info">
              <div class="row-name font-mono">{t.original_path}</div>
              <div class="row-meta"><Clock size={11} /> <span>{t.timestamp}</span></div>
            </div>
            <div class="row-actions">
              <button class="btn-neo btn-neo--ai" onclick={() => restore(t.id)}><RotateCcw size={12} /> Restore</button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    {#if regBackups.length === 0}
      <div class="empty">No registry backups yet. Deleting registry items backs them up here for 30 days.</div>
    {:else}
      <div class="list">
        {#each regBackups as b (b.id)}
          <div class="row">
            <div class="row-icon"><HardDrive size={14} /></div>
            <div class="row-info">
              <div class="row-name font-mono">{b.root_path}</div>
              <div class="row-meta"><Clock size={11} /> <span>{b.created}</span><span class="dot">•</span><span>{b.key_count} key(s), {b.value_count} value(s) • kept 30 days</span></div>
            </div>
            <div class="row-actions">
              <button class="btn-neo btn-neo--ai" onclick={() => restoreReg(b.id)}><RotateCcw size={12} /> Restore</button>
              <button class="btn-icon-danger" onclick={() => removeReg(b.id)} aria-label="Delete registry backup">
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}

  {#if showPreview}
    <div class="overlay" role="dialog" aria-modal="true">
      <div class="modal">
        <div class="modal-head">
          <h3>Report preview</h3>
          <button class="btn-neo btn-neo--ai" onclick={() => showPreview = false}>Close</button>
        </div>
        <pre class="preview">{preview.slice(0, 8000)}</pre>
      </div>
    </div>
  {/if}

  {#if showClearRegistry}
    <div class="overlay" role="dialog" aria-modal="true" aria-label="Confirm clearing registry backups">
      <div class="modal modal--confirm">
        <div class="modal-head">
          <h3>Clear all {regBackups.length} registry backups?</h3>
        </div>
        <p class="confirm-text">
          These backups are the <strong>only way to restore the registry keys and values
          that ClearOut removed</strong>. Deleting them permanently closes that path —
          this cannot be undone.
        </p>
        <label class="confirm-label" for="clear-reg-input">Type <span class="font-mono">DELETE</span> to confirm</label>
        <input
          id="clear-reg-input"
          class="confirm-input font-mono"
          type="text"
          bind:value={clearRegistryInput}
          placeholder="DELETE"
          autocomplete="off"
          spellcheck="false"
        />
        <div class="confirm-actions">
          <button class="btn-secondary" onclick={() => showClearRegistry = false}>Cancel</button>
          <button
            class="confirm-danger"
            disabled={clearRegistryInput !== 'DELETE' || clearing}
            onclick={runClear}
          >
            {clearing ? 'Clearing…' : 'Permanently delete backups'}
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if confirmState}
    <div class="overlay" role="dialog" aria-modal="true" aria-label={confirmState.title}>
      <div class="modal modal--confirm">
        <div class="modal-head">
          <h3>{confirmState.title}</h3>
        </div>
        {#if confirmState.body}
          <p class="confirm-text">{confirmState.body}</p>
        {/if}
        <div class="confirm-actions">
          <button class="btn-secondary" onclick={() => confirmState = null}>Cancel</button>
          {#if confirmState.tone === 'danger'}
            <button class="confirm-danger" onclick={acceptConfirm}>{confirmState.label}</button>
          {:else}
            <button class="btn-neo btn-neo--ai" onclick={acceptConfirm}>{confirmState.label}</button>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .history { max-width: 960px; }
  .header { display:flex; justify-content:space-between; align-items:flex-start; margin-bottom:14px; }
  h1 { font-size:20px; font-weight:600; margin:0; letter-spacing:-0.02em; }
  .subtitle { font-size:13px; color:var(--color-text-secondary); margin:4px 0 0; }
  .tabs { display:flex; gap:6px; margin-bottom:12px; align-items:center; }
  .tab { display:inline-flex; align-items:center; justify-content:center; gap:6px; height:30px; padding:0 12px; border-radius:6px; border:1px solid var(--color-border); background:var(--color-surface); font-size:12.5px; font-weight:500; line-height:1; letter-spacing:-0.01em; cursor:pointer; -webkit-font-smoothing:antialiased; transition: background-color 0.14s ease, border-color 0.14s ease, color 0.14s ease; }
  .tab:hover { background:var(--color-bg); }
  .tab.active { background:var(--color-accent-soft); border-color:var(--color-accent); color:var(--color-accent-text); }

  .clear-all {
    margin-left:auto;
    display:inline-flex; align-items:center; gap:6px;
    height:30px; padding:0 10px;
    border-radius:6px; border:none; background:transparent;
    font-size:12px; font-weight:500; color:var(--color-text-secondary);
    cursor:pointer;
    transition: background-color 0.14s ease, color 0.14s ease;
  }
  .clear-all:hover { background:var(--color-bg); color:var(--color-text-primary); }
  .clear-all:disabled { opacity:0.5; cursor:not-allowed; }

  .error-banner {
    display:flex; align-items:center; justify-content:space-between; gap:10px;
    background: color-mix(in srgb, var(--color-danger) 10%, var(--color-surface));
    border:1px solid color-mix(in srgb, var(--color-danger) 40%, var(--color-border));
    border-radius:8px; padding:8px 12px; margin-bottom:12px;
    font-size:12.5px; color:var(--color-text-primary);
  }
  .error-dismiss {
    display:inline-flex; align-items:center; justify-content:center;
    width:22px; height:22px; border-radius:5px; border:none;
    background:transparent; color:var(--color-text-secondary); cursor:pointer;
  }
  .error-dismiss:hover { background:var(--color-bg); color:var(--color-text-primary); }

  .modal--confirm { width:460px; }
  .confirm-text { font-size:12.5px; color:var(--color-text-secondary); line-height:1.55; margin:0 0 14px 0; }
  .confirm-text strong { color:var(--color-text-primary); }
  .confirm-label { display:block; font-size:12px; color:var(--color-text-secondary); margin-bottom:6px; }
  .confirm-input { width:100%; height:34px; padding:0 12px; border-radius:8px; font-size:13px; margin-bottom:14px; }
  .confirm-actions { display:flex; justify-content:flex-end; gap:10px; }
  .confirm-danger {
    height:32px; padding:0 14px; border-radius:8px;
    border:1px solid var(--color-danger); background:var(--color-danger);
    color:#fff; font-size:12.5px; font-weight:500; cursor:pointer;
    transition: opacity 0.14s ease;
  }
  .confirm-danger:disabled { opacity:0.4; cursor:not-allowed; }
  .confirm-danger:not(:disabled):hover { opacity:0.9; }

  .list { background:var(--color-surface); border:1px solid var(--color-border); border-radius:8px; overflow:hidden; }
  .row { display:flex; align-items:center; gap:12px; padding:10px 14px; border-bottom:1px dashed var(--color-border); }
  .row:last-child { border-bottom:none; }
  .row:hover { background:var(--color-bg); }
  .row-icon { width:28px; height:28px; border-radius:6px; background:var(--color-accent-soft); color:var(--color-accent-text); display:flex; align-items:center; justify-content:center; flex-shrink:0; }
  .row-info { flex:1; min-width:0; }
  .row-name { font-size:13px; font-weight:500; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
  .row-meta { display:flex; align-items:center; gap:6px; font-size:11px; color:var(--color-text-secondary); margin-top:2px; }
  .dot { opacity:0.5; }
  .row-actions { display:flex; gap:8px; align-items:center; }
  .empty { padding:48px; text-align:center; font-size:13px; color:var(--color-text-secondary); }

  .overlay { position:fixed; inset:0; background:rgba(0,0,0,0.4); display:flex; align-items:center; justify-content:center; z-index:60; }
  .modal { background:var(--color-surface); border:1px solid var(--color-border); border-radius:10px; padding:18px; width:720px; max-width:90vw; max-height:80vh; overflow:auto; }
  .modal-head { display:flex; justify-content:space-between; align-items:center; margin-bottom:12px; }
  .modal-head h3 { margin:0; font-size:14px; font-weight:600; }
  .preview { font-family:var(--font-mono); font-size:11px; white-space:pre-wrap; word-break:break-all; background:var(--color-bg); border:1px solid var(--color-border); border-radius:6px; padding:12px; max-height:50vh; overflow:auto; }

  .btn-icon-danger {
    display:inline-flex; align-items:center; justify-content:center; width:30px; height:30px; border-radius:6px; border:1px solid var(--color-border); background:var(--color-surface); color:var(--color-text-secondary); cursor:pointer; transition: background-color 0.14s ease, border-color 0.14s ease;
  }
  .btn-icon-danger:hover { background:var(--color-bg); color:var(--color-text-primary); }
</style>
