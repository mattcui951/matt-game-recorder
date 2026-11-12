// Dev-only: lets the UI run in a plain browser tab for visual checks.
// Open http://localhost:1430/?media=<url from %APPDATA%/Matt Game Recorder/dev-media-url.txt>
// Reads come from the running app's debug-only JSON endpoints; writes are no-ops.
import { emit } from '@tauri-apps/api/event';
import { mockIPC, mockWindows } from '@tauri-apps/api/mocks';

const media = new URLSearchParams(location.search).get('media') ?? '';
const dev = (what: string) => fetch(`${media}dev/${what}`).then((r) => r.json());
const idleUpdate = { phase: 'idle', version: null, progress: null, error: null, checkedAt: null };

mockWindows('main');
mockIPC(
  async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case 'bootstrap':
        return { settings: await dev('settings'), mediaUrl: media, version: '0.1.0', status: await dev('status'), systemLanguage: 'en', update: idleUpdate, updatedFrom: null };
      case 'check_update':
        return idleUpdate;
      case 'import_cpu_assist':
        return { on: false, available: false, speedup: null, measuring: false };
      case 'champion_names_zh':
        return {};
      case 'get_library':
        return dev('library');
      case 'get_status':
        return dev('status');
      case 'list_projects':
        return dev('projects');
      case 'get_project':
        return (await dev('projects')).find((p: { id: string }) => p.id === a.id);
      case 'disk_info':
        return { free: 1.62e12, total: 2.0e12, library: 3.1e9, level: 'good' };
      case 'list_devices':
        return {
          audio: { outputs: [{ id: 'spk', name: 'Speakers (Realtek(R) Audio)' }], inputs: [{ id: 'mic', name: 'Microphone (PM450 USB Microphone)' }] },
          webcams: [],
          monitors: [{ index: 0, handle: 1, name: 'DISPLAY1', primary: true, width: 3440, height: 1440 }, { index: 1, handle: 2, name: 'DISPLAY2', primary: false, width: 2560, height: 1080 }],
          encoders: [
            { id: 'av1_nvenc', label: 'NVIDIA NVENC AV1 (GPU, smallest files)', available: true },
            { id: 'hevc_nvenc', label: 'NVIDIA NVENC HEVC (GPU)', available: true },
            { id: 'h264_nvenc', label: 'NVIDIA NVENC H.264 (GPU, most compatible)', available: true },
            { id: 'av1_amf', label: 'AMD AMF AV1 (GPU)', available: false },
          ],
        };
      case 'get_sprite': {
        const lib = await dev('library');
        const r = lib.recordings.find((x: { id: string }) => x.id === a.id);
        return r ? `.cache/sprites/${r.id}.jpg` : null;
      }
      case 'ensure_art':
        return true;
      case 'list_jobs':
        return dev('jobs');
      case 'pause_job':
        return fetch(`${media}dev/${a.paused ? 'pause' : 'resume'}?${a.id}`).then(() => null);
      case 'scan_outplayed':
        return { folder: 'D:\\outplayed\\Outplayed', files: 312, bytes: 4.47e11, withEvents: 286, alreadyImported: 27, deletedAfterImport: 2 };
      case 'plugin:event|listen':
        return 0;
      default:
        console.info('[preview] ignored', cmd, a);
        return null;
    }
  },
  { shouldMockEvents: true },
);

// Keep the status live like the real app does.
setInterval(async () => {
  emit('status', await dev('status'));
  for (const j of await dev('jobs')) emit('job', j);
}, 1000);
