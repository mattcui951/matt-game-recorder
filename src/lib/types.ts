export type Timing = { before: number; after: number };

export type GameSettings = {
  enabled: boolean;
  openAppOnEnd: boolean;
  dashboardOnSecondary: boolean;
  collectApm: boolean;
  captureCursor: boolean;
  captureMode: 'manual' | 'highlights' | 'match' | 'session';
  events: Record<string, boolean>;
  timing: Record<string, Timing>;
  hotkeys: {
    bookmarkToggle: string;
    bookmarkToggleBefore: number;
    placeBookmark: string;
    placeBefore: number;
    placeAfter: number;
    screenshot: string;
    toggleApp: string;
    toggleWebcam: string;
    toggleDashboard: string;
    dashboardLayout: string;
    ultimateKey: string;
  };
};

export type CustomTheme = { id: string; name: string; dark: boolean; colors: Record<string, string> };

export type Appearance = {
  theme: string;
  followSystem: boolean;
  lightTheme: string;
  darkTheme: string;
  /** Tweaks to built-in themes: theme id -> token -> #rrggbb. */
  overrides: Record<string, Record<string, string>>;
  fontUi: string;
  fontScore: string;
  uiScale: number;
  corners: 'sharp' | 'default' | 'round';
  customThemes: CustomTheme[];
  windowBg: string;
};

export type Settings = {
  appearance: Appearance;
  general: {
    captureOwnWindows: boolean;
    captureNotifications: boolean;
    discordPresence: boolean;
    discordAppId: string;
    watermark: boolean;
    startWithWindows: boolean;
    layout: 'art' | 'plain';
    language: 'system' | 'en' | 'zh-CN';
    checkUpdates: boolean;
  };
  video: {
    preset: 'low' | 'medium' | 'high' | 'custom';
    resolution: number;
    bitrateKbps: number;
    fps: number;
    force169: boolean;
    cropMode: 'fit' | 'stretch';
    encoder: string;
    encoderPreset: string;
    buffer: 'ram' | 'disk';
  };
  audio: {
    systemEnabled: boolean;
    systemDevice: string;
    systemVolume: number;
    micEnabled: boolean;
    micDevice: string;
    micVolume: number;
    gameOnly: boolean;
    micMono: boolean;
  };
  webcam: { enabled: boolean; device: string; mode: 'always' | 'visible' };
  screenRecorder: {
    captureCursor: boolean;
    showBorder: boolean;
    display: number;
    hotkeyToggle: string;
    hotkeyBookmark: string;
    bookmarkBefore: number;
    bookmarkAfter: number;
  };
  storage: { mode: 'manual' | 'auto'; limitGb: number; ageLimitEnabled: boolean; ageLimitMonths: number; keepDeletedDays: number; mediaFolder: string };
  notifications: { sessionStart: boolean; matchStart: boolean; matchEnd: boolean; manualCapture: boolean; gameEventCapture: boolean };
  games: { lol: GameSettings };
};

export type Kda = { kills: number; deaths: number; assists: number };

export type RecEvent = {
  t: number;
  kind: string;
  label: string;
  multi?: number;
  before: number;
  after: number;
  /** The other champion (Data Dragon key): who was killed, or who killed you. */
  champion?: string;
  /** Multikills: seconds from the streak's first kill (`t`) to its last. */
  span?: number;
};

export type Session = {
  id: string;
  game: string;
  title: string;
  startedAt: string;
  endedAt?: string | null;
  champion?: string | null;
  championKey?: string | null;
  kda?: Kda | null;
  result?: 'win' | 'loss' | null;
  gameMode?: string | null;
  captureMode: string;
  relDir: string;
};

export type Recording = {
  id: string;
  sessionId: string;
  kind: 'match' | 'session' | 'highlight' | 'manual' | 'clip' | 'screen' | 'export' | 'supercut' | 'screenshot';
  title: string;
  file: string;
  thumb?: string | null;
  createdAt: string;
  duration: number;
  size: number;
  width: number;
  height: number;
  fps: number;
  codec: string;
  favorite: boolean;
  events: RecEvent[];
  apm?: number[];
  sourceId?: string | null;
  deletedAt?: string | null;
  originalDir?: string | null;
  relDir: string;
};

export type Snapshot = { sessions: Session[]; recordings: Recording[] };

export type Status = {
  state: 'idle' | 'waiting' | 'recording' | 'saving' | 'error';
  mode?: string | null;
  sessionId?: string | null;
  title?: string | null;
  elapsed: number;
  champion?: string | null;
  championKey?: string | null;
  kda?: Kda | null;
  eventsStatus: 'good' | 'partial' | 'down' | 'idle';
  events: RecEvent[];
  clipsSaved: number;
  bookmarking: boolean;
  screenRecording: boolean;
  screenElapsed: number;
  message?: string | null;
  apm: number;
};

export type CpuAssist = { on: boolean; available: boolean; speedup: number | null; measuring: boolean };

export type UpdateState = {
  phase: 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'error';
  version: string | null;
  progress: number | null;
  error: string | null;
  checkedAt: number | null;
};

export type Job = {
  id: string;
  kind: string;
  /** An import's source: "outplayed" | "library". */
  tag: string;
  title: string;
  state: 'running' | 'done' | 'error' | 'cancelled';
  progress: number;
  message?: string | null;
  resultId?: string | null;
  pausable: boolean;
  paused: boolean;
  elapsed: number;
};

export type DiskInfo = { free: number; total: number; library: number; level: 'good' | 'low' | 'critical' };

export type Devices = {
  audio: { outputs: { id: string; name: string }[]; inputs: { id: string; name: string }[] };
  webcams: string[];
  monitors: { index: number; handle: number; name: string; primary: boolean; width: number; height: number }[];
  encoders: { id: string; label: string; available: boolean }[];
};

export type ProjectClip = { id: string; recordingId: string; start: number; end: number; crop: 'fit' | 'fill' | 'stretch'; volume: number };

export type Project = {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  aspect: string;
  volume: number;
  protect: boolean;
  watermark: boolean;
  media: string[];
  clips: ProjectClip[];
};

export type OutplayedScan = { folder?: string | null; files: number; bytes: number; withEvents: number; alreadyImported: number; deletedAfterImport: number };
