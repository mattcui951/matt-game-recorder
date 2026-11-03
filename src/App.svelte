<script lang="ts">
  import Rail from './components/Rail.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import Toasts from './components/Toasts.svelte';
  import UpdateBanner from './components/UpdateBanner.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import Editor from './views/Editor.svelte';
  import Library from './views/Library.svelte';
  import SessionView from './views/SessionView.svelte';
  import Settings from './views/Settings.svelte';
  import { t } from './lib/i18n.svelte';
  import { app, artSrc, errorText, init } from './lib/state.svelte';

  const isDashboard = location.hash === '#dashboard';
  let failed = $state('');
  init().catch((e) => (failed = errorText(e)));

  // Champion art behind the app: the latest match's splash, heavily dimmed.
  const artKey = $derived.by(() => {
    if (app.settings?.general.layout !== 'art') return null;
    if (app.route.name === 'session') {
      const s = app.library.sessions.find((x) => x.id === (app.route as { sessionId: string }).sessionId);
      if (s?.championKey) return s.championKey;
    }
    return app.library.sessions.find((s) => s.championKey)?.championKey ?? null;
  });
  const art = $derived(artSrc(artKey, 'splash'));
</script>

{#if failed}
  <div class="fatal">
    <h2>{t.app.cannotStart}</h2>
    <p>{failed}</p>
  </div>
{:else if !app.ready}
  <div class="boot"></div>
{:else if isDashboard}
  <Dashboard />
{:else}
  <div class="shell">
    <TitleBar />
    <UpdateBanner />
    <div class="body">
      <Rail />
      <main class="main">
        {#if art}<div class="art" style="background-image:url('{art}')" aria-hidden="true"></div>{/if}
        <div class="view">
          {#if app.route.name === 'library'}
            <Library tab={app.route.tab} />
          {:else if app.route.name === 'session'}
            {#key app.route.sessionId}
              <SessionView sessionId={app.route.sessionId} recordingId={app.route.recordingId} at={app.route.at} />
            {/key}
          {:else if app.route.name === 'editor'}
            <Editor projectId={app.route.projectId} />
          {:else}
            <Settings page={app.route.page} />
          {/if}
        </div>
      </main>
    </div>
  </div>
  <Toasts />
{/if}

<style>
  .shell {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .main {
    position: relative;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }
  .art {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center 20%;
    opacity: 0.11;
    filter: saturate(0.7);
    mask-image: linear-gradient(to bottom, black 0%, transparent 70%);
    pointer-events: none;
  }
  .view {
    position: relative;
    height: 100%;
  }
  .boot {
    height: 100vh;
    background: var(--bg);
  }
  .fatal {
    display: grid;
    place-content: center;
    gap: 8px;
    height: 100vh;
    text-align: center;
    color: var(--text-2);
    padding: 24px;
  }
</style>
