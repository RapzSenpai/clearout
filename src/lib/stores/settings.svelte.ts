import type { Settings } from '../types'

const STORAGE_KEY = 'clearout-settings'

const defaults: Settings = {
  aiEnabled: false,
  aiProvider: 'groq',
  apiKey: '',
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
      return { ...defaults, ...parsed }
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
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s))
  } catch {}
}

let settings = $state<Settings>(load())

export function getSettings(): Settings {
  return settings
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

export function resetSettings() {
  settings = { ...defaults }
  persist(settings)
  applyAppearance(settings.theme, settings.accent)
}
