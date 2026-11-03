<script lang="ts">
  import Icon from './Icon.svelte';
  import { t } from '../lib/i18n.svelte';
  import { app, go } from '../lib/state.svelte';

  const section = $derived(app.route.name === 'session' ? 'library' : app.route.name);
  const items = $derived([
    { id: 'library', icon: 'library', label: t.rail.library, route: () => go({ name: 'library', tab: 'sessions' }) },
    { id: 'editor', icon: 'editor', label: t.rail.editor, route: () => go({ name: 'editor' }) },
  ] as const);
</script>

<nav class="rail" aria-label={t.rail.main}>
  {#each items as it (it.id)}
    <button class="item" class:on={section === it.id} aria-label={it.label} aria-current={section === it.id ? 'page' : undefined} title={it.label} onclick={it.route}>
      <Icon name={it.icon} size={21} />
    </button>
  {/each}
  <div class="grow"></div>
  <button class="item" class:on={section === 'settings'} aria-label={t.rail.settings} title={t.rail.settings} onclick={() => go({ name: 'settings', page: 'lol' })}>
    <Icon name="settings" size={21} />
  </button>
</nav>

<style>
  .rail {
    width: 56px;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 10px 0;
    background: var(--bg-sunk);
    border-right: 1px solid var(--seam);
  }
  .grow {
    flex: 1;
  }
  .item {
    position: relative;
    width: 40px;
    height: 40px;
    display: grid;
    place-items: center;
    border-radius: var(--r-md);
    color: var(--text-3);
    transition: color 120ms, background 120ms;
  }
  .item:hover {
    color: var(--text);
    background: var(--panel-2);
  }
  .item.on {
    color: var(--you);
    background: var(--panel-2);
  }
  .item.on::before {
    content: '';
    position: absolute;
    left: -8px;
    top: 9px;
    bottom: 9px;
    width: 2px;
    background: var(--you);
  }
</style>
