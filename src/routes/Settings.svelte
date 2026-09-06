<script lang="ts">
  import { getSettings, updateSettings } from '../lib/stores/settings.svelte'
  import { Brain, Shield, Scan, Ban, Trash2, Moon, Sun, Monitor, Clock, Palette } from '@lucide/svelte'
  import { Switch, Select } from 'bits-ui'
  import { invoke } from '@tauri-apps/api/core'
  import { onMount } from 'svelte'

  let { focusAppearance = 0 }: { focusAppearance?: number } = $props()

  let settings = $state(getSettings())
  let apiKeyDebounce: ReturnType<typeof setTimeout> | null = null

  let isApiKeyEmpty = $derived(!settings.apiKey.trim())

  function persist(partial: Partial<typeof settings>) {
    settings = { ...settings, ...partial }
    updateSettings(partial)
  }

  function handleToggle(key: keyof typeof settings, value: boolean) {
    persist({ [key]: value } as any)
  }

  function handleSelect(key: keyof typeof settings, value: string) {
    persist({ [key]: value } as any)
  }

  // --- Theme palette ------------------------------------------------------
  type AccentId = 'green' | 'cyan' | 'purple' | 'amber' | 'mono'
  const ACCENTS: { id: AccentId; name: string; dark: string; light: string }[] = [
    { id: 'green', name: 'Green', dark: '#3ECF8E', light: '#15945B' },
    { id: 'cyan', name: 'Cyan', dark: '#4FD6E4', light: '#0E7A8C' },
    { id: 'purple', name: 'Purple', dark: '#B28CFF', light: '#6248D6' },
    { id: 'amber', name: 'Amber', dark: '#E8A33D', light: '#8F5E00' },
    { id: 'mono', name: 'Mono', dark: '#C9C9C9', light: '#4B4B4B' },
  ]
  const DARK_BG = '#0A0A0A'
  const LIGHT_BG = '#F4F4F2'

  // Refresh the "System" ring when the OS scheme changes while open.
  let osDark = $state(
    typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-color-scheme: dark)').matches
  )
  let effectiveMode = $derived(
    settings.theme === 'system' ? (osDark ? 'dark' : 'light') : settings.theme
  )

  onMount(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => { osDark = mq.matches }
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  })

  // Sidebar "Appearance" click: land on the Appearance section, not the top.
  let sectionFlash = $state(false)
  $effect(() => {
    if (focusAppearance > 0) {
      const el = document.getElementById('appearance-section')
      el?.scrollIntoView({ block: 'center', behavior: 'smooth' })
      sectionFlash = true
      const t = setTimeout(() => { sectionFlash = false }, 1600)
      return () => clearTimeout(t)
    }
  })

  function pickTheme(mode: 'light' | 'dark', accent: AccentId) {
    persist({ theme: mode, accent })
  }

  function handleApiKeyInput() {
    if (apiKeyDebounce) clearTimeout(apiKeyDebounce)
    apiKeyDebounce = setTimeout(() => {
      // Clearing the field actually clears the stored key — otherwise the UI
      // shows empty while the old key silently stays in use.
      updateSettings({ apiKey: settings.apiKey.trim() })
    }, 450)
  }

  function removeExcluded(path: string) {
    const s = getSettings()
    updateSettings({
      excludedPaths: (s.excludedPaths ?? []).filter(p => p !== path),
      excludedHosts: (s.excludedHosts ?? []).filter(p => p !== path)
    })
    settings = getSettings()
  }

  function clearAllExcluded() {
    updateSettings({ excludedPaths: [], excludedHosts: [] })
    settings = getSettings()
  }

  let schedulerEnabled = $state(false)
  let schedulerTime = $state('02:00')
  let schedulerMsg = $state('')
  let isAdmin = $state(true)

  onMount(async () => {
    try {
      const status: any = await invoke('get_scheduler_status')
      schedulerEnabled = status.enabled
      if (status.time) schedulerTime = status.time.slice(0,5)
    } catch {}
    try { isAdmin = await invoke<boolean>('is_admin') } catch { isAdmin = false }
  })

  async function toggleScheduler(v: boolean) {
    schedulerEnabled = v
    schedulerMsg = ''
    try {
      await invoke('set_scheduler', { enabled: v, time: schedulerTime })
      schedulerMsg = v ? `Weekly Sunday ${schedulerTime} enabled` : 'Scheduler disabled'
    } catch (e) {
      schedulerMsg = String(e)
      schedulerEnabled = !v
    }
  }

  async function updateSchedulerTime() {
    if (!schedulerEnabled) return
    schedulerMsg = ''
    try {
      await invoke('set_scheduler', { enabled: true, time: schedulerTime })
      schedulerMsg = `Updated to ${schedulerTime}`
    } catch (e) { schedulerMsg = String(e) }
  }
</script>

<div class="settings">
  <h1>Settings</h1>

  <div class="section">
    <div class="section-header">
      <Brain size={18} class="section-icon" />
      <div>
        <h2>AI Advisory</h2>
        <p class="description">Get AI assessments for leftover items. Uses free-tier providers — no data sent without your explicit action.</p>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Enable AI</span>
        <span class="setting-desc">Show "Ask AI" button on leftover items</span>
      </div>
      <Switch.Root
        checked={settings.aiEnabled}
        onCheckedChange={(v) => handleToggle('aiEnabled', v)}
        class="switch-root"
      >
        <Switch.Thumb class="switch-thumb" />
      </Switch.Root>
    </div>

    {#if settings.aiEnabled}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Provider</span>
          <span class="setting-desc">Choose free-tier model</span>
        </div>
        <Select.Root
          value={settings.aiProvider}
          onValueChange={(v) => handleSelect('aiProvider', v as string)}
          type="single"
        >
          <Select.Trigger class="select-trigger-sm">
            <Select.Value />
          </Select.Trigger>
          <Select.Content class="select-content">
            <Select.Item value="groq" class="select-item">Groq (Free tier)</Select.Item>
            <Select.Item value="openrouter" class="select-item">OpenRouter (Free models)</Select.Item>
          </Select.Content>
        </Select.Root>
      </div>

      <div class="setting-row setting-row--api">
        <div class="setting-info">
          <span class="setting-label">API Key</span>
          <span class="setting-desc">Stored locally only, never shared</span>
          {#if isApiKeyEmpty}
            <span class="hint hint--error">Don't leave the field empty</span>
          {/if}
        </div>
        <div class="api-field">
          <input
            type="password"
            placeholder="gsk_... or sk-or-..."
            bind:value={settings.apiKey}
            oninput={handleApiKeyInput}
          />
        </div>
      </div>
    {/if}
  </div>

  <div class="section">
    <div class="section-header">
      <Shield size={18} class="section-icon" />
      <div>
        <h2>Safety</h2>
        <p class="description">Deletion safeguards</p>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Allow Force Service Removal</span>
        <span class="setting-desc">If a leftover service won't stop, still attempt to delete it (files locked by running processes still fail)</span>
      </div>
      <Switch.Root
        checked={settings.forceKillAllowed}
        onCheckedChange={(v) => handleToggle('forceKillAllowed', v)}
        class="switch-root"
      >
        <Switch.Thumb class="switch-thumb" />
      </Switch.Root>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Restore Point by Default</span>
        <span class="setting-desc">Create system restore point before deletion</span>
      </div>
      <Switch.Root
        checked={settings.restorePointDefault}
        onCheckedChange={(v) => handleToggle('restorePointDefault', v)}
        class="switch-root"
      >
        <Switch.Thumb class="switch-thumb" />
      </Switch.Root>
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <Scan size={18} class="section-icon" />
      <div>
        <h2>Scan</h2>
        <p class="description">How deep to search for leftovers</p>
      </div>
    </div>

    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Scan Depth</span>
        <span class="setting-desc">Fast = surface-only crawl, quicker. Thorough (default) = deepest, finds nested leftovers</span>
      </div>
      <Select.Root
        value={settings.scanDepth}
        onValueChange={(v) => handleSelect('scanDepth', v as string)}
        type="single"
      >
        <Select.Trigger class="select-trigger-sm">
          <Select.Value />
        </Select.Trigger>
        <Select.Content class="select-content">
          <Select.Item value="fast" class="select-item">Fast</Select.Item>
          <Select.Item value="thorough" class="select-item">Thorough</Select.Item>
        </Select.Content>
      </Select.Root>
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <Ban size={18} class="section-icon" />
      <div>
        <h2>Excluded</h2>
        <p class="description">Paths you chose "Never flag" from right-click — rescans will skip them</p>
      </div>
      {#if (settings.excludedPaths?.length ?? 0) > 0 || (settings.excludedHosts?.length ?? 0) > 0}
        <button class="btn-neo btn-neo--ai" onclick={clearAllExcluded} style="margin-left:auto">Clear all</button>
      {/if}
    </div>
    {#if (settings.excludedPaths?.length ?? 0) === 0 && (settings.excludedHosts?.length ?? 0) === 0}
      <p class="empty-hint">No exclusions yet. Right-click any leftover → Never flag this path.</p>
    {:else}
      <div class="exclude-list">
        {#each [...(settings.excludedPaths ?? []), ...(settings.excludedHosts ?? [])] as p (p)}
          <div class="exclude-row">
            <span class="exclude-path font-mono">{p}</span>
            <button class="btn-icon" onclick={() => removeExcluded(p)} aria-label="Remove exclusion"><Trash2 size={13} /></button>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <div class="section" class:flash={sectionFlash} id="appearance-section">
    <div class="section-header">
      <Palette size={18} class="section-icon" />
      <div>
        <h2>Appearance</h2>
        <p class="description">Pick a terminal accent family. Mode sets light or dark; System follows your OS.</p>
      </div>
    </div>

    <div class="seg" role="group" aria-label="Color mode">
      <button
        class="seg-btn"
        class:active={settings.theme === 'system'}
        onclick={() => persist({ theme: 'system' })}
        aria-pressed={settings.theme === 'system'}
      >
        <Monitor size={13} strokeWidth={1.75} />
        System
      </button>
      <button
        class="seg-btn"
        class:active={settings.theme === 'light'}
        onclick={() => persist({ theme: 'light' })}
        aria-pressed={settings.theme === 'light'}
      >
        <Sun size={13} strokeWidth={1.75} />
        Light
      </button>
      <button
        class="seg-btn"
        class:active={settings.theme === 'dark'}
        onclick={() => persist({ theme: 'dark' })}
        aria-pressed={settings.theme === 'dark'}
      >
        <Moon size={13} strokeWidth={1.75} />
        Dark
      </button>
    </div>

    <div class="theme-groups">
      <div class="theme-group">
        <span class="theme-group-label">Dark</span>
        <div class="theme-tiles">
          {#each ACCENTS as a (a.id)}
            <button
              class="theme-tile"
              class:active={settings.accent === a.id && effectiveMode === 'dark'}
              onclick={() => pickTheme('dark', a.id)}
              aria-label="Dark {a.name} theme"
              aria-pressed={settings.accent === a.id && effectiveMode === 'dark'}
            >
              <span class="tile-card" style="background:{DARK_BG};border-color:{a.id === 'mono' ? '#3A3A3A' : '#2A2A2A'}">
                <span class="tile-dot" style="background:{a.dark}"></span>
              </span>
              {a.name}
            </button>
          {/each}
        </div>
      </div>
      <div class="theme-group">
        <span class="theme-group-label">Light</span>
        <div class="theme-tiles">
          {#each ACCENTS as a (a.id)}
            <button
              class="theme-tile"
              class:active={settings.accent === a.id && effectiveMode === 'light'}
              onclick={() => pickTheme('light', a.id)}
              aria-label="Light {a.name} theme"
              aria-pressed={settings.accent === a.id && effectiveMode === 'light'}
            >
              <span class="tile-card" style="background:{LIGHT_BG};border-color:#DCDCD8">
                <span class="tile-dot" style="background:{a.light}"></span>
              </span>
              {a.name}
            </button>
          {/each}
        </div>
      </div>
    </div>
    <p class="theme-note">Green is the default. Red stays reserved for destructive actions in every theme.</p>
  </div>

  <div class="section">
    <div class="section-header">
      <Clock size={18} class="section-icon" />
      <div>
        <h2>Automation</h2>
        <p class="description">Weekly scheduled scan — creates Windows Task</p>
      </div>
    </div>
    {#if !isAdmin}
      <p class="empty-hint">Run as admin to manage scheduled tasks.</p>
    {/if}
    <div class="setting-row">
      <div class="setting-info">
        <span class="setting-label">Weekly scan</span>
        <span class="setting-desc">Sunday {schedulerTime} — opens ClearOut</span>
        {#if schedulerMsg}<span class="hint">{schedulerMsg}</span>{/if}
      </div>
      <Switch.Root checked={schedulerEnabled} onCheckedChange={toggleScheduler} class="switch-root" disabled={!isAdmin}>
        <Switch.Thumb class="switch-thumb" />
      </Switch.Root>
    </div>
    {#if schedulerEnabled}
      <div class="setting-row">
        <div class="setting-info">
          <span class="setting-label">Time</span>
          <span class="setting-desc">24h format HH:MM</span>
        </div>
        <input type="time" value={schedulerTime} onchange={(e) => { schedulerTime = (e.target as HTMLInputElement).value; updateSchedulerTime() }} class="time-input" />
      </div>
    {/if}
  </div>
</div>

<style>
  .settings {
    max-width: 640px;
  }

  h1 {
    font-size: 20px;
    font-weight: 600;
    margin: 0 0 28px 0;
    letter-spacing: -0.02em;
  }

  .section {
    margin-bottom: 20px;
    padding: 20px;
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
  }

  .section.flash {
    box-shadow: 0 0 0 2px var(--color-accent);
    transition: box-shadow 0.2s ease;
  }

  .section-header {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    margin-bottom: 14px;
  }

  :global(.section-icon) {
    color: var(--color-accent-text);
    margin-top: 2px;
    flex-shrink: 0;
  }

  h2 {
    font-size: 13px;
    font-weight: 600;
    margin: 0;
  }

  .description {
    font-size: 12px;
    color: var(--color-text-secondary);
    margin: 2px 0 0 0;
    line-height: 1.4;
  }

  .setting-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 13px 0;
    border-bottom: 1px dashed var(--color-border);
    gap: 16px;
  }

  .setting-row:last-child {
    border-bottom: none;
    padding-bottom: 0;
  }

  .setting-row:first-of-type {
    padding-top: 0;
  }

  .setting-row--api {
    align-items: flex-start;
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: 3px;
    flex: 1;
    min-width: 0;
  }

  .setting-label {
    font-size: 13px;
    font-weight: 500;
  }

  .setting-desc {
    font-size: 12px;
    color: var(--color-text-secondary);
  }

  .hint {
    font-size: 11px;
    color: var(--color-text-secondary);
    margin-top: 2px;
  }

  .hint--error {
    color: var(--color-danger);
  }



  .api-field {
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .api-field input {
    min-width: 240px;
    padding: 7px 11px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 13px;
    outline: none;
    background: var(--color-surface);
    transition: border-color 0.12s ease;
  }

  .api-field input:focus {
    border-color: var(--color-accent);
  }

  :global(.switch-root) {
    width: 38px;
    height: 22px;
    background-color: var(--color-border);
    border-radius: 11px;
    position: relative;
    cursor: pointer;
    transition: background-color 0.15s ease;
    border: none;
    padding: 0;
    flex-shrink: 0;
  }

  :global(.switch-root[data-state="checked"]) {
    background-color: var(--color-accent);
  }

  :global(.switch-thumb) {
    display: block;
    width: 18px;
    height: 18px;
    background-color: white;
    border-radius: 50%;
    transition: transform 0.15s ease;
    transform: translateX(2px);
    pointer-events: none;
  }

  :global(.switch-root[data-state="checked"] .switch-thumb) {
    transform: translateX(18px);
  }

  :global(.select-trigger-sm) {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 12px;
    border: 1px solid var(--color-border);
    border-radius: 6px;
    background: var(--color-surface);
    font-size: 13px;
    color: var(--color-text-primary);
    cursor: pointer;
    min-width: 160px;
    justify-content: space-between;
    outline: none;
    transition: border-color 0.1s ease;
  }

  :global(.select-trigger-sm:hover) {
    border-color: var(--color-accent);
  }

  :global(.select-content) {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 4px;
    min-width: 160px;
    z-index: 50;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
  }

  :global(.select-item) {
    padding: 8px 10px;
    border-radius: 4px;
    font-size: 13px;
    cursor: pointer;
    outline: none;
    transition: background-color 0.1s ease;
  }

  :global(.select-item:hover) {
    background-color: var(--color-accent-soft);
  }

  :global(.select-item[data-highlighted]) {
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
  }

  .empty-hint { font-size:12px; color:var(--color-text-secondary); padding:8px 0; }
  .exclude-list { display:flex; flex-direction:column; gap:6px; margin-top:8px; }
  .exclude-row { display:flex; align-items:center; gap:8px; padding:6px 8px; background:var(--color-bg); border:1px solid var(--color-border); border-radius:6px; }
  .exclude-path { flex:1; font-size:11px; word-break:break-all; }
  .btn-icon { width:28px; height:28px; border-radius:6px; border:1px solid var(--color-border); background:var(--color-surface); display:flex; align-items:center; justify-content:center; cursor:pointer; color:var(--color-text-secondary); }
  .btn-icon:hover { background:var(--color-bg); color:var(--color-text-primary); }
  .time-input { padding:6px 10px; border:1px solid var(--color-border); border-radius:6px; background:var(--color-surface); color:var(--color-text-primary); font-size:13px; }

  /* Appearance — mode segmented + theme tiles */
  .seg {
    display: inline-flex;
    gap: 3px;
    padding: 3px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    margin-bottom: 16px;
  }

  .seg-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 12px;
    border-radius: 6px;
    border: 1px solid transparent;
    background: transparent;
    color: var(--color-text-secondary);
    font-size: 12px;
    font-weight: 500;
    letter-spacing: -0.01em;
    cursor: pointer;
    transition: background-color 0.12s ease, color 0.12s ease, border-color 0.12s ease;
  }

  .seg-btn:hover {
    color: var(--color-text-primary);
  }

  .seg-btn.active {
    background: var(--color-accent-soft);
    border-color: var(--color-accent);
    color: var(--color-accent-text);
  }

  .theme-groups {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .theme-group-label {
    display: block;
    font-size: 10.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.12em;
    color: var(--color-text-secondary);
    margin-bottom: 7px;
  }

  .theme-tiles {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }

  .theme-tile {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    border: none;
    background: transparent;
    padding: 0;
    cursor: pointer;
    color: var(--color-text-secondary);
    font-size: 10.5px;
    letter-spacing: -0.01em;
    transition: color 0.12s ease;
  }

  .theme-tile:hover {
    color: var(--color-text-primary);
  }

  .tile-card {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 56px;
    height: 34px;
    border-radius: 7px;
    border: 1px solid;
    transition: box-shadow 0.12s ease, transform 0.12s ease;
  }

  .theme-tile:hover .tile-card {
    transform: translateY(-1px);
  }

  .theme-tile.active {
    color: var(--color-accent-text);
  }

  .theme-tile.active .tile-card {
    box-shadow: 0 0 0 2px var(--color-accent);
  }

  .tile-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }

  .theme-note {
    font-size: 11px;
    color: var(--color-text-secondary);
    margin: 14px 0 0 0;
  }
</style>
