<script lang="ts">
  import './app.css'
  import Dashboard from './routes/Dashboard.svelte'
  import LeftoverReview from './routes/LeftoverReview.svelte'
  import Settings from './routes/Settings.svelte'
  import History from './routes/History.svelte'
  import type { AppInfo, ScanResult } from './lib/types'
  import { LayoutDashboard, SettingsIcon, History as HistoryIcon, Palette, ShieldAlert } from '@lucide/svelte'
  import { Tooltip } from 'bits-ui'
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { refreshSystemTheme } from './lib/stores/settings.svelte'
  import { getPendingCleanup, type PendingCleanup } from './lib/stores/pending.svelte'

  let currentView: 'dashboard' | 'review' | 'settings' | 'history' = $state('dashboard')
  // Bumped every time the sidebar Appearance link is clicked so Settings can
  // scroll to + flash its Appearance section (even when already on Settings).
  let settingsFocus = $state(0)

  function openAppearance() {
    settingsFocus += 1
    currentView = 'settings'
  }

  onMount(() => {
    // main.ts already applied the saved appearance pre-paint; keep "system"
    // in sync when the OS scheme changes while the app is open.
    refreshSystemTheme()
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', refreshSystemTheme)
    return () => mq.removeEventListener('change', refreshSystemTheme)
  })

  // Service removal + restore points need elevation — surface it up front so
  // first-time users don't hit mysterious "Access is denied" later.
  let adminChecked = $state(false)
  let isAdmin = $state(false)
  onMount(async () => {
    try {
      isAdmin = await invoke<boolean>('is_admin')
    } catch {
      isAdmin = false
    }
    adminChecked = true
  })
  let selectedApp: AppInfo | null = $state(null)
  let selectedApps: AppInfo[] = $state([])
  let scanResult: ScanResult | null = $state(null)
  let restoreSnapshot: PendingCleanup | null = $state(null)

  function handleUninstall(app: AppInfo) {
    selectedApp = app
    selectedApps = [app]
    restoreSnapshot = null
    currentView = 'review'
  }

  function handleQueue(apps: AppInfo[]) {
    selectedApps = apps
    selectedApp = apps[0] ?? null
    restoreSnapshot = null
    currentView = 'review'
  }

  // Resume a pending cleanup from the Dashboard card without re-scanning.
  function handleReviewPending() {
    const p = getPendingCleanup()
    if (!p) return
    selectedApp = p.primary
    selectedApps = p.apps
    restoreSnapshot = p
    currentView = 'review'
  }

  import { clearQueue } from './lib/stores/queue.svelte'

  function handleBack() {
    clearQueue()
    currentView = 'dashboard'
    selectedApp = null
    selectedApps = []
    scanResult = null
    restoreSnapshot = null
  }

  function handleScanComplete(result: ScanResult) {
    scanResult = result
  }
</script>

<Tooltip.Provider>
<div class="app-container">
  <nav class="sidebar">
    <div class="logo">
      <div class="logo-icon">
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M3 6h18"/>
          <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/>
          <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>
          <line x1="10" y1="11" x2="10" y2="17"/>
          <line x1="14" y1="11" x2="14" y2="17"/>
        </svg>
      </div>
      <div class="logo-block">
        <span class="logo-text">ClearOut</span>
        <span class="brand-credit">built by <span class="w-rapz">rapz</span><span class="w-god">god</span><span class="w-the">the</span><span class="w-father">father</span></span>
      </div>
    </div>
    <div class="nav-group">
      <button
        class="nav-btn"
        class:active={currentView === 'dashboard'}
        onclick={() => currentView = 'dashboard'}
        aria-current={currentView === 'dashboard' ? 'page' : undefined}
      >
        <LayoutDashboard size={16} />
        Dashboard
      </button>
      <button
        class="nav-btn"
        class:active={currentView === 'history'}
        onclick={() => currentView = 'history'}
        aria-current={currentView === 'history' ? 'page' : undefined}
      >
        <HistoryIcon size={16} />
        History
      </button>
      <button
        class="nav-btn"
        class:active={currentView === 'settings'}
        onclick={() => currentView = 'settings'}
        aria-current={currentView === 'settings' ? 'page' : undefined}
      >
        <SettingsIcon size={16} />
        Settings
      </button>
    </div>
    <div class="sidebar-foot">
      <button class="appearance-link" onclick={openAppearance} aria-label="Open Appearance settings">
        <Palette size={14} strokeWidth={1.75} />
        Appearance
      </button>
      <span class="theme-hint">Themes &amp; mode — in Settings</span>
    </div>
  </nav>

  <main class="content">
    {#if adminChecked && !isAdmin}
      <div class="admin-banner" role="status">
        <ShieldAlert size={14} strokeWidth={1.75} />
        <span>Running without administrator rights — leftover <strong>service removal</strong> and <strong>restore points</strong> need elevation. Relaunch ClearOut as administrator for full cleanup power.</span>
      </div>
    {/if}
    {#if currentView === 'dashboard'}
      <Dashboard onUninstall={handleUninstall} onQueueStart={handleQueue} onReviewPending={handleReviewPending} />
    {:else if currentView === 'review' && selectedApp}
      <LeftoverReview
        app={selectedApp}
        apps={selectedApps}
        onBack={handleBack}
        onScanComplete={handleScanComplete}
        restore={restoreSnapshot}
      />
    {:else if currentView === 'history'}
      <History />
    {:else if currentView === 'settings'}
      <Settings focusAppearance={settingsFocus} />
    {/if}
  </main>
</div>
</Tooltip.Provider>

<style>
  .app-container {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .sidebar {
    width: 200px;
    background-color: var(--color-surface);
    border-right: 1px solid var(--color-border);
    padding: 20px 12px;
    display: flex;
    flex-direction: column;
    gap: 24px;
    flex-shrink: 0;
  }

  .sidebar-foot {
    margin-top: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-top: 12px;
    border-top: 1px dashed var(--color-border);
  }

  .appearance-link {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 30px;
    padding: 0 10px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 12.5px;
    font-weight: 500;
    letter-spacing: -0.01em;
    cursor: pointer;
    -webkit-font-smoothing: antialiased;
    transition-property: background-color, border-color, color, transform;
    transition-duration: 140ms;
    transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
  }

  .appearance-link:hover {
    background: var(--color-bg);
    color: var(--color-text-primary);
  }

  .appearance-link:active {
    transform: scale(0.97);
  }

  .theme-hint {
    font-size: 10.5px;
    color: var(--color-text-secondary);
    letter-spacing: -0.01em;
    padding-left: 2px;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 4px 10px 4px 8px;
  }

  .logo-block {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .brand-credit {
    font-size: 8px;
    letter-spacing: 0;
    line-height: 1.3;
    color: var(--color-text-secondary);
    white-space: nowrap;
    font-weight: 400;
    padding-left: 1px;
  }

  .logo-icon {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    background-color: var(--color-accent);
    color: var(--color-on-accent);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .logo-text {
    font-size: 15px;
    font-weight: 600;
    color: var(--color-text-primary);
    letter-spacing: -0.01em;
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-btn {
    background: transparent;
    border: none;
    padding: 8px 10px;
    text-align: left;
    border-radius: 6px;
    cursor: pointer;
    font-size: 13px;
    color: var(--color-text-secondary);
    display: flex;
    align-items: center;
    gap: 8px;
    transition: background-color 0.1s ease, color 0.1s ease;
    font-weight: 400;
  }

  .nav-btn:hover {
    background-color: var(--color-accent-soft);
    color: var(--color-text-primary);
  }

  .nav-btn.active {
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
    font-weight: 500;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: 28px 36px;
    background-color: var(--color-bg);
  }

  .admin-banner {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    position: sticky;
    top: 0;
    z-index: 40;
    margin: -8px 0 16px 0;
    padding: 8px 12px;
    border-radius: 8px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, var(--color-surface));
    border: 1px dashed color-mix(in srgb, var(--color-danger) 35%, var(--color-border));
    -webkit-font-smoothing: antialiased;
  }

  .admin-banner :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }
</style>
