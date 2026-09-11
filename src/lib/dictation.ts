export type DictationSettings = {
  enabled: boolean; model: string; autoInsert: boolean; saveHistory: boolean;
};
export type DictationEntry = {
  id: string; createdAt: number; text: string; saved: boolean; delivery: string; originalText?: string;
};
export type DictationSnapshot = {
  revision: number; supported: boolean; settings: DictationSettings;
  phase: "idle" | "starting" | "recording" | "processing";
  startedAt: number | null; message: string; holdShortcutReady: boolean; toggleShortcutReady: boolean;
  inputMode: "button" | "hold" | "toggle";
  backend: string;
  preparingModel: boolean;
  shortcutError: string | null; storageError: string | null; entries: DictationEntry[];
};
