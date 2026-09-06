import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'
import { getSettings, applyAppearance } from './lib/stores/settings.svelte'

// Apply the saved theme before the first paint so there is no flash
// of the wrong palette. data-theme is always explicit (system resolved
// against the OS here), and data-accent drives accent families.
const s = getSettings()
applyAppearance(s.theme, s.accent)

const app = mount(App, {
  target: document.getElementById('app')!,
})

export default app
