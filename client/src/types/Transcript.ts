export type Word = {
  word: string;
  start: number;
  end: number;
  score?: number;
  estimated?: boolean;
  reading?: string;
};

export type Segment = {
  text: string;
  start: number;
  end: number;
  words: Word[];
};

export type Transcript = {
  language: string;
  segments: Segment[];
  source?: string;
};

export type AudioPaths = {
  instrumental: string;
  vocals: string | null;
};
