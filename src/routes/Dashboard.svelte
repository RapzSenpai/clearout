<script lang="ts">
  import { onMount } from 'svelte'
  import { getInstalledApps } from '../lib/tauri-api'
  import type { AppInfo } from '../lib/types'
  import { formatSize } from '../lib/utils'
  import AppRow from '../lib/components/AppRow.svelte'
  import { Search, Package, HardDrive, LetterText, CalendarDays, ListChecks, Recycle, X, RefreshCw, ArrowUp, ArrowDown } from '@lucide/svelte'
  import { Checkbox } from 'bits-ui'
  import { Check } from '@lucide/svelte'
  import { getQueue, setQueue } from '../lib/stores/queue.svelte'
  import { getPendingCleanup, clearPendingCleanup } from '../lib/stores/pending.svelte'

  let { onUninstall, onQueueStart, onReviewPending }: { onUninstall: (app: AppInfo) => void; onQueueStart?: (apps: AppInfo[]) => void; onReviewPending?: () => void } = $props()

  let apps: AppInfo[] = $state([])
  let searchQuery = $state('')
  let sortBy: 'name' | 'size' | 'date' = $state('name')
  let sortOrder: 'asc' | 'desc' = $state('asc')
  let loading = $state(true)
  let refreshing = $state(false)
  let selectedIds: Set<string> = $state(new Set())

  let filteredApps = $derived(
    apps
      .filter(app =>
        app.name.toLowerCase().includes(searchQuery.toLowerCase()) ||
        app.publisher?.toLowerCase().includes(searchQuery.toLowerCase())
      )
      .sort((a, b) => {
        let cmp = 0
        if (sortBy === 'name') {
          cmp = a.name.localeCompare(b.name)
          return sortOrder === 'asc' ? cmp : -cmp
        } else if (sortBy === 'size') {
          cmp = (b.estimated_size || 0) - (a.estimated_size || 0)
          return sortOrder === 'desc' ? cmp : -cmp
        } else {
          cmp = (b.install_date || '').localeCompare(a.install_date || '')
          return sortOrder === 'desc' ? cmp : -cmp
        }
      })
  )

  let totalSize = $derived(
    apps.reduce((sum, app) => sum + (app.estimated_size || 0), 0)
  )

  function toggleSelect(id: string) {
    if (selectedIds.has(id)) selectedIds.delete(id); else selectedIds.add(id)
    selectedIds = new Set(selectedIds)
  }

  function startQueue() {
    const queued = apps.filter(a => selectedIds.has(a.id))
    if (queued.length === 0) return
    setQueue(queued.slice(1))
    selectedIds = new Set()
    if (queued[0]) onUninstall(queued[0])
    onQueueStart?.(queued)
  }

  let pendingCleanup = $derived(getPendingCleanup())
  let pendingCount = $derived(
    pendingCleanup
      ? pendingCleanup.scan.files.length + pendingCleanup.scan.registry.length +
        pendingCleanup.scan.services.length + pendingCleanup.scan.startup.length +
        (pendingCleanup.scan.hosts?.length ?? 0) + (pendingCleanup.scan.tasks?.length ?? 0)
      : 0
  )
  let pendingName = $derived(
    pendingCleanup
      ? (pendingCleanup.apps.length > 1 ? `${pendingCleanup.apps.length} queued apps` : pendingCleanup.primary.name)
      : ''
  )

  async function loadApps() {
    if (apps.length === 0) {
      loading = true
    } else {
      refreshing = true
    }
    try {
      apps = await getInstalledApps()
    } catch (e) {
      console.error('Failed to load apps:', e)
    } finally {
      loading = false
      refreshing = false
    }
  }

  let isScrolled = $state(false)
  let dashboardEl = $state<HTMLDivElement | null>(null)

  let searchInputEl = $state<HTMLInputElement | null>(null)

  function scrollToTop(behavior: ScrollBehavior = 'smooth') {
    const scrollContainer = dashboardEl?.closest('.content')
    if (!scrollContainer) return
    const targetTop = dashboardEl?.offsetTop ?? 0
    scrollContainer.scrollTo({ top: targetTop, behavior })
  }

  function handleSearchInput() {
    if (isScrolled) {
      scrollToTop('smooth')
    }
  }

  function clearSearch() {
    searchQuery = ''
    searchInputEl?.focus()
    if (isScrolled) {
      scrollToTop('smooth')
    }
  }

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      if (searchQuery) {
        e.preventDefault()
        clearSearch()
      } else {
        searchInputEl?.blur()
      }
    }
  }

  function selectSort(tab: 'name' | 'size' | 'date') {
    if (sortBy === tab) {
      sortOrder = sortOrder === 'asc' ? 'desc' : 'asc'
    } else {
      sortBy = tab
      sortOrder = tab === 'name' ? 'asc' : 'desc'
    }
    if (isScrolled) {
      scrollToTop('smooth')
    }
  }

  onMount(() => {
    loadApps()

    const scrollContainer = dashboardEl?.closest('.content')
    if (!scrollContainer) return

    const updateStuck = () => {
      isScrolled = scrollContainer.scrollTop > 2
    }

    scrollContainer.addEventListener('scroll', updateStuck, { passive: true })
    updateStuck()

    return () => {
      scrollContainer.removeEventListener('scroll', updateStuck)
    }
  })
</script>

<div class="dashboard" bind:this={dashboardEl}>
  <div class="sticky-header" class:stuck={isScrolled}>
    <div class="header">
      <div>
        <button
          type="button"
          class="title-btn"
          class:clickable={isScrolled}
          onclick={() => { if (isScrolled) scrollToTop('smooth') }}
          aria-label={isScrolled ? 'Installed Apps, click to scroll to top' : 'Installed Apps'}
        >
          <h1>Installed Apps</h1>
        </button>
        <p class="subtitle">Windows applications registered in the system</p>
      </div>
      <div class="header-right">
        <div class="stats">
          <div class="stat-chip">
            <Package size={13} />
            <span>{apps.length} apps</span>
          </div>
          <div class="stat-chip">
            <HardDrive size={13} />
            <span>{formatSize(totalSize)}</span>
          </div>
        </div>
        <button class="btn-neo btn-neo--ai" onclick={loadApps} disabled={loading || refreshing} aria-label="Refresh installed apps">
          <RefreshCw size={13} strokeWidth={1.75} class={refreshing ? 'spin' : ''} />
          {refreshing ? 'Refreshing…' : 'Refresh'}
        </button>
      </div>
    </div>

    <div class="toolbar">
      <div class="search-wrapper">
        <Search size={15} class="search-icon" />
        <input
          bind:this={searchInputEl}
          type="search"
          placeholder="Search apps..."
          bind:value={searchQuery}
          oninput={handleSearchInput}
          onkeydown={handleSearchKeydown}
          aria-label="Search installed apps"
        />
        {#if searchQuery}
          <button
            type="button"
            class="search-clear"
            onclick={clearSearch}
            aria-label="Clear search"
          >
            <X size={13} strokeWidth={2} />
          </button>
        {/if}
      </div>
      <div class="sort-control" role="group" aria-label="Sort installed apps">
        <div class="sort-pills">
          <button
            class="sort-pill"
            class:active={sortBy === 'name'}
            onclick={() => selectSort('name')}
            aria-pressed={sortBy === 'name'}
            aria-label="Sort by name {sortBy === 'name' ? (sortOrder === 'asc' ? 'ascending' : 'descending') : ''}"
          >
            <LetterText size={13} />
            <span>Name</span>
            {#if sortBy === 'name'}
              {#if sortOrder === 'asc'}
                <ArrowUp size={11} strokeWidth={2.2} class="sort-arrow" />
              {:else}
                <ArrowDown size={11} strokeWidth={2.2} class="sort-arrow" />
              {/if}
            {/if}
          </button>
          <button
            class="sort-pill"
            class:active={sortBy === 'size'}
            onclick={() => selectSort('size')}
            aria-pressed={sortBy === 'size'}
            aria-label="Sort by size {sortBy === 'size' ? (sortOrder === 'desc' ? 'largest first' : 'smallest first') : ''}"
          >
            <HardDrive size={13} />
            <span>Size</span>
            {#if sortBy === 'size'}
              {#if sortOrder === 'desc'}
                <ArrowDown size={11} strokeWidth={2.2} class="sort-arrow" />
              {:else}
                <ArrowUp size={11} strokeWidth={2.2} class="sort-arrow" />
              {/if}
            {/if}
          </button>
          <button
            class="sort-pill"
            class:active={sortBy === 'date'}
            onclick={() => selectSort('date')}
            aria-pressed={sortBy === 'date'}
            aria-label="Sort by install date {sortBy === 'date' ? (sortOrder === 'desc' ? 'newest first' : 'oldest first') : ''}"
          >
            <CalendarDays size={13} />
            <span>Date</span>
            {#if sortBy === 'date'}
              {#if sortOrder === 'desc'}
                <ArrowDown size={11} strokeWidth={2.2} class="sort-arrow" />
              {:else}
                <ArrowUp size={11} strokeWidth={2.2} class="sort-arrow" />
              {/if}
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>

  {#if pendingCleanup}
    <div class="pending-card" role="region" aria-label="Pending cleanup">
      <div class="pending-icon"><Recycle size={16} /></div>
      <div class="pending-info">
        <span class="pending-title">Cleanup pending</span>
        <span class="pending-desc">{pendingName} — {pendingCount} leftover items found</span>
      </div>
      <button class="btn-neo btn-neo--ai" onclick={() => onReviewPending?.()}>
        <ListChecks size={13} strokeWidth={1.75} />
        Review cleanup
      </button>
      <button class="pending-dismiss" onclick={clearPendingCleanup} aria-label="Dismiss pending cleanup">
        <X size={14} strokeWidth={1.75} />
      </button>
    </div>
  {/if}

  {#if loading}
    <div class="loading">
      <div class="spinner"></div>
      <span>Loading installed apps...</span>
    </div>
  {:else}
    <div class="app-list">
      {#each filteredApps as app (app.id)}
        <div class="app-row-wrap">
          <Checkbox.Root checked={selectedIds.has(app.id)} onCheckedChange={() => toggleSelect(app.id)} class="checkbox-root" aria-label="Queue {app.name}">
            <span class="checkbox-indicator"><Check size={11} strokeWidth={2.6} /></span>
          </Checkbox.Root>
          <div class="app-row-main">
            <AppRow {app} onUninstall={() => onUninstall(app)} />
          </div>
        </div>
      {:else}
        <div class="empty">No apps found</div>
      {/each}
    </div>
    {#if selectedIds.size > 0}
      <div class="queue-bar">
        <span class="queue-count">{selectedIds.size} queued</span>
        <button class="btn-neo btn-neo--delete" onclick={startQueue}>
          <ListChecks size={13} strokeWidth={1.75} />
          Review queue ({selectedIds.size})
        </button>
      </div>
    {/if}
  {/if}
</div>

<style>
  .dashboard {
    max-width: 960px;
  }

  .sticky-header {
    position: sticky;
    top: 0;
    z-index: 20;
    background-color: var(--color-bg);
    padding-top: 28px;
    padding-bottom: 12px;
    margin-bottom: 16px;
    border-bottom: 1px dashed transparent;
    transition: border-bottom-color 0.15s ease;
  }

  .sticky-header.stuck {
    border-bottom-color: var(--color-border);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 16px;
  }

  .title-btn {
    background: transparent;
    border: none;
    padding: 0;
    margin: 0;
    text-align: left;
    display: inline-flex;
    align-items: center;
    cursor: default;
    color: inherit;
    font: inherit;
  }

  .title-btn.clickable {
    cursor: pointer;
  }

  .title-btn.clickable:hover h1 {
    color: var(--color-accent);
  }

  .title-btn:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 4px;
    border-radius: 4px;
  }

  h1 {
    font-size: 20px;
    font-weight: 600;
    margin: 0;
    letter-spacing: -0.02em;
    transition: color 0.12s ease;
  }

  .subtitle {
    font-size: 13px;
    color: var(--color-text-secondary);
    margin: 4px 0 0 0;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .stats {
    display: flex;
    gap: 8px;
  }

  .header-right .btn-neo:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .stat-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
    padding: 6px 12px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 500;
  }

  .pending-card {
    display: flex;
    align-items: center;
    gap: 12px;
    background: var(--color-surface);
    border: 1px dashed color-mix(in srgb, var(--color-accent) 45%, var(--color-border));
    border-radius: 8px;
    padding: 12px 14px;
    margin-bottom: 16px;
  }

  .pending-icon {
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--color-accent-soft);
    color: var(--color-accent-text);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .pending-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .pending-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-primary);
  }

  .pending-desc {
    font-size: 12px;
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pending-dismiss {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    flex-shrink: 0;
    transition: background-color 0.12s ease, color 0.12s ease;
  }

  .pending-dismiss:hover {
    background: var(--color-bg);
    color: var(--color-text-primary);
  }

  .toolbar {
    display: flex;
    gap: 12px;
    align-items: stretch;
  }

  .search-wrapper {
    flex: 1;
    position: relative;
    display: flex;
    align-items: stretch;
  }

  .search-wrapper :global(.search-icon) {
    position: absolute;
    left: 12px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--color-text-secondary);
    pointer-events: none;
  }

  .search-wrapper input {
    width: 100%;
    height: 36px;
    padding: 0 32px 0 36px;
    border-radius: 8px;
    font-size: 13px;
  }

  .search-wrapper input::-webkit-search-cancel-button {
    -webkit-appearance: none;
    appearance: none;
  }

  .search-clear {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    width: 22px;
    height: 22px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--color-text-secondary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    transition: background-color 0.12s ease, color 0.12s ease, transform 0.1s ease;
  }

  .search-clear:hover {
    background: var(--color-accent-soft);
    color: var(--color-text-primary);
  }

  .search-clear:active {
    transform: translateY(-50%) scale(0.92);
  }

  .search-clear:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 1px;
  }

  .sort-control {
    display: flex;
    align-items: center;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 3px;
    flex-shrink: 0;
    height: 36px;
  }

  .sort-pills {
    display: flex;
    gap: 3px;
    height: 100%;
    align-items: center;
  }

  .sort-pill {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 0 12px;
    height: 30px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 12.5px;
    font-weight: 500;
    line-height: 1;
    letter-spacing: -0.01em;
    cursor: pointer;
    -webkit-font-smoothing: antialiased;
    transition-property: background-color, border-color, color, transform;
    transition-duration: 160ms;
    transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
    white-space: nowrap;
  }

  .sort-pill:active {
    transform: scale(0.97);
  }

  .sort-pill:hover {
    background: var(--color-accent-soft);
    color: var(--color-text-primary);
  }

  .sort-pill.active {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-text);
  }

  .sort-pill.active:hover {
    background: var(--color-accent-soft);
  }

  :global(.sort-arrow) {
    margin-left: -2px;
    opacity: 0.85;
  }

  .loading {
    padding: 64px;
    text-align: center;
    color: var(--color-text-secondary);
    font-size: 13px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .spinner {
    width: 20px;
    height: 20px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .app-list {
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    overflow: hidden;
  }

  .app-row-wrap {
    display:flex; align-items:center; gap:10px; padding:8px 12px 8px 14px; border-bottom:1px dashed var(--color-border); transition: background-color 0.12s ease;
  }
  .app-row-wrap:last-child { border-bottom:none; }
  .app-row-wrap:hover { background: var(--color-accent-soft); }
  .app-row-main { flex:1; min-width:0; display:flex; }

  .queue-bar {
    position:fixed; bottom:0; left:200px; right:0; background:var(--color-surface); border-top:1px dashed var(--color-border); padding:10px 28px; display:flex; justify-content:space-between; align-items:center; z-index:30;
  }
  .queue-count { font-size:13px; font-weight:500; }

  .empty {
    padding: 64px;
    text-align: center;
    color: var(--color-text-secondary);
    font-size: 13px;
  }
</style>
