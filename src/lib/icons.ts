// Stroke icons (24×24, drawn with stroke="currentColor") shared by the menu and the home page.
export const ICONS = {
  home: 'M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6H9v6H4a1 1 0 0 1-1-1z',
  meeting: 'M8 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM16 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM2 20c0-3 3-5 6-5s6 2 6 5M14 15c3 0 8 1 8 5',
  transcribe: 'M3 12h2M7 8v8M11 5v14M15 9v6M19 11v2',
  dictation: 'M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3zM5 11a7 7 0 0 0 14 0M12 18v3',
  textklipp: 'M6 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM8.5 7.5L20 18M8.5 16.5L20 6',
  deidentify: 'M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z',
  summarize: 'M4 6h16M4 10h16M4 14h10M4 18h7',
  history: 'M4 5a2 2 0 0 1 2-2h13v16H6a2 2 0 0 0-2 2zM4 19V5',
  tasks: 'M4 4h16v16H4zM8 12l3 3 5-6',
  plus: 'M12 5v14M5 12h14',
  search: 'M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM21 21l-5-5',
  open: 'M4 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z',
} as const;
export type IconName = keyof typeof ICONS;

/** Icon for a job type in lists. */
export function jobIcon(jobType: string): string {
  return ({ meeting: ICONS.meeting, transcribe: ICONS.transcribe, deidentify: ICONS.deidentify, summarize: ICONS.summarize } as Record<string, string>)[jobType] ?? ICONS.history;
}
