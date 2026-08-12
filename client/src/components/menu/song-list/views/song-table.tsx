import type { Song } from "@/types/Song";
import { SONG_COLUMNS } from "../song-columns";
import type { SongItemProps } from "../types";
import { SongTableRow } from "./song-table-row";

interface SongTableProps {
  songs: Song[];
  getItemProps: (song: Song, index: number) => SongItemProps;
}

export const SongTable = ({ songs, getItemProps }: SongTableProps) => (
  <table className="w-full table-fixed border-collapse text-[11px]">
    <thead className="song-table__header">
      <tr className="text-left text-[9px] text-muted-foreground">
        {SONG_COLUMNS.map((column) => (
          <th key={column.id} className={column.thClassName}>
            {column.header}
          </th>
        ))}
      </tr>
    </thead>
    <tbody>
      {songs.map((song, index) => (
        <SongTableRow key={song.file_hash} {...getItemProps(song, index)} />
      ))}
    </tbody>
  </table>
);
