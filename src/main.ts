import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';
import { applyCachedAppearance } from './lib/appearance';
import { applyCachedLanguage } from './lib/i18n.svelte';

applyCachedAppearance();
applyCachedLanguage();

async function start() {
  if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
    await import('./dev/browser-preview');
  }
  mount(App, { target: document.getElementById('app')! });
}

start();
