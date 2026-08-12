import type { Song } from "@/types/Song";
import type { SongItemProps } from "../types";
import { SongGridCard } from "./song-grid-card";

interface SongGridProps {
  songs: Song[];
  getItemProps: (song: Song, index: number) => SongItemProps;
}

export const SongGrid = ({ songs, getItemProps }: SongGridProps) => (
  <div
    role="list"
    className="grid grid-cols-[repeat(auto-fill,minmax(min(100%,9.5rem),1fr))] gap-x-4 gap-y-6 px-1 py-2 sm:grid-cols-[repeat(auto-fill,minmax(10.5rem,1fr))]"
  >
    {songs.map((song, index) => (
      <SongGridCard key={song.file_hash} {...getItemProps(song, index)} />
    ))}
  </div>
);
