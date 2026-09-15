import type { Settings } from '../types'

const STORAGE_KEY = 'clearout-settings'

const defaults: Settings = {
  aiEnabled: false,
  aiProvider: 'groq',
  apiKey: '',
  aiEndpoint: '',
  aiModel: 'llama3.1:8b',
  forceKillAllowed: false,
  restorePointDefault: true,
  scanDepth: 'thorough',
  excludedPaths: [],
  excludedHosts: [],
  theme: 'system',
  accent: 'green'
}

function load(): Settings {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored) {
      const parsed = JSON.parse(stored)
      // migrate: ensure new fields exist
      if (!parsed.theme) parsed.theme = 'system'
      if (!parsed.accent) parsed.accent = 'green'
      if (!parsed.excludedPaths) parsed.excludedPaths = []
      if (!parsed.excludedHosts) parsed.excludedHosts = []
      // ponytail: apiKey never persists; ceiling is re-enter per device, upgrade is Credential Manager autofill below.
      delete parsed.apiKey
      return { ...defaults, ...parsed, apiKey: '' }
    }
  } catch {}
  return { ...defaults }
}

/**
 * Resolve the requested mode (system follows the OS) into a concrete
 * light/dark value, then write both data attributes on <html>:
 *   data-theme="dark" | "light"   — neutral palette
 *   data-accent="cyan" | ...      — accent palette (absent = green)
 * Always setting data-theme makes accent overrides deterministic and lets
 * JS (not the CSS media query) own the "system" resolution.
 */
export function applyAppearance(theme: Settings['theme'], accent: Settings['accent']) {
  const root = document.documentElement
  const resolved = theme === 'light' || theme === 'dark'
    ? theme
    : window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  root.setAttribute('data-theme', resolved)
  if (accent && accent !== 'green') {
    root.setAttribute('data-accent', accent)
  } else {
    root.removeAttribute('data-accent')
  }
}

function persist(s: Settings) {
  try {
    // ponytail: strip apiKey beats encrypted store; ceiling is memory-only key, upgrade is session lock.
    const { apiKey: _drop, ...safe } = s as Settings & { apiKey?: string }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(safe))
  } catch {}
}

let secureKeyPresent = $state(false)

export function setSecureKeyPresent(v: boolean) {
  secureKeyPresent = v
}

export function hasSecureKey(): boolean {
  return secureKeyPresent
}

let settings = $state<Settings>(load())

export function getSettings(): Settings {
  return settings
}

/**
 * Ask AI allowed? Mirrors the backend key rule: Ollama runs locally and
 * custom endpoints may be keyless, so no key gates them. Cloud providers
 * need one. Single guard for every caller — check here, not at call sites.
 */
export function aiReady(): boolean {
  if (!settings.aiEnabled) return false
  if (settings.aiProvider === 'ollama') return true
  // Custom without an endpoint fails per item in Review; gate it here
  // where the Test button explains the fix instead.
  if (settings.aiProvider === 'custom') return settings.aiEndpoint.trim().length > 0
  return settings.apiKey.trim().length > 0 || secureKeyPresent
}

export function updateSettings(partial: Partial<Settings>) {
  settings = { ...settings, ...partial }
  persist(settings)
  if ('theme' in partial || 'accent' in partial) {
    applyAppearance(settings.theme, settings.accent)
  }
}

/** Re-apply when the OS scheme changes while the user is on "system". */
export function refreshSystemTheme() {
  if (settings.theme === 'system') applyAppearance(settings.theme, settings.accent)
}
