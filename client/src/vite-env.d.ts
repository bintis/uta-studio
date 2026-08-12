/// <reference types="vite/client" />

import type { AppConfig } from "./types/AppConfig";
import type { SongsMeta } from "./types/SongsMeta";

declare global {
  interface Window {
    __UTA_STUDIO_APP_CONFIG__?: AppConfig;
    __UTA_STUDIO_SONGS_META__?: SongsMeta;
  }
}

export {};
