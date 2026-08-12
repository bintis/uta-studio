import { convertFileSrc } from "@/bridge/media";
import { KeyTempoSection } from "@/components/menu/song-list/details/key-tempo-section";
import { AnalysisTuningSection } from "@/components/menu/song-list/details/analysis-tuning-section";
import { AlbumArt } from "@/components/menu/song-list/shared/album-art";
import {
  LanguageBadge,
  isDisplayableLanguage,
} from "@/components/menu/song-list/shared/language-badge";
import {
  formatTranscriptSource,
  getSongStatusInfo,
} from "@/components/menu/song-list/shared/song-status";
import { StatusBadge } from "@/components/menu/song-list/shared/status-badge";
import { SongActionsMenu } from "@/components/menu/song-list/song-actions-menu";
import type { ShiftType } from "@/components/menu/song-list/shifts";
import { Button } from "@/components/ui/button";
import { SONGS } from "@/queries/keys";
import { useAnalysisQueue } from "@/queries/use-songs";
import type { Song } from "@/types/Song";
import { formatSeconds } from "@/utils/format-duration";
import { useQueryClient } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";
import { loadSongByHash } from "@/bridge/songs";
import {
  ArrowLeft,
  AudioLines,
  CheckCircle2,
  Clock3,
  Disc3,
  FileAudio2,
  FileOutput,
  FolderOpen,
  Languages,
  Music2,
  PencilRuler,
  SlidersHorizontal,
  Video,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router";
import { useSongEditorAction } from "@/hooks/use-song-editor-action";

export const SongDetailPage = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const queryClient = useQueryClient();
  const locationSong = (location.state as { song?: Song } | null)?.song;
  const { data: refreshedSong } = useQuery({
    queryKey: ["song", locationSong?.file_hash],
    queryFn: () => loadSongByHash(locationSong!.file_hash),
    enabled: Boolean(locationSong),
    initialData: locationSong,
    refetchInterval: 2000,
  });
  const song = refreshedSong ?? locationSong;
  const { data: queue } = useAnalysisQueue();
  const [shifting, setShifting] = useState<Record<ShiftType, boolean>>({
    tempo: false,
    key: false,
  });

  const queueStatus = song ? queue?.entries[song.file_hash] : undefined;
  const status = song ? getSongStatusInfo(song, queueStatus) : null;
  const analysisBusy = queueStatus === "Queued" || Boolean(status?.isAnalyzing);
  const keyPending = Boolean(
    song?.is_analyzed && song.transcript_source === "Lrc" && song.no_stems && song.key === null,
  );
  const supportsShifts = Boolean(
    song?.is_analyzed && song.transcript_source !== "Usdx" && !keyPending,
  );
  const supportsAnalysisActions = Boolean(song?.is_analyzed && song.transcript_source !== "Usdx");
  const editorAction = useSongEditorAction(song, status, analysisBusy);

  useEffect(() => {
    if (!keyPending) return;
    const interval = window.setInterval(() => {
      queryClient.invalidateQueries({ queryKey: SONGS });
    }, 2000);
    return () => window.clearInterval(interval);
  }, [keyPending, queryClient]);

  if (!song || !status) {
    return (
      <main className="roon-library grid h-full flex-1 place-items-center px-6 text-center">
        <div>
          <Music2 className="mx-auto size-8 text-primary" />
          <h1 className="mt-4 text-xl font-light">Choose a song first</h1>
          <p className="mt-1 text-xs text-muted-foreground">
            Open a track from the library to see its production page.
          </p>
          <Button className="mt-5" onClick={() => navigate("/")}>
            <ArrowLeft /> Back to library
          </Button>
        </div>
      </main>
    );
  }

  const extension = song.path.split(".").pop()?.toUpperCase() || "MEDIA";
  const sourceLabel = song.transcript_source
    ? formatTranscriptSource(song.transcript_source)
    : "Not generated";
  const missingLabel = song.authoring_missing.length
    ? song.authoring_missing.join(" · ").replace(/_/g, " ")
    : "Nothing missing";

  return (
    <main className="roon-library song-detail-page relative h-full min-h-0 flex-1 overflow-y-auto">
      <section className="relative isolate min-h-[clamp(28rem,68vh,48rem)] overflow-hidden border-b border-border/45">
        {song.is_video ? (
          <video
            className="absolute inset-0 size-full object-cover"
            src={convertFileSrc(song.path)}
            poster={song.album_art_path ? convertFileSrc(song.album_art_path) : undefined}
            muted
            autoPlay
            loop
            playsInline
            preload="metadata"
            aria-hidden="true"
          />
        ) : song.album_art_path ? (
          <div className="absolute -inset-10 overflow-hidden bg-black">
            <AlbumArt
              song={song}
              className="size-full scale-110 rounded-none opacity-80 blur-xl"
              lazy={false}
            />
          </div>
        ) : null}
        <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(7,7,12,.18),rgba(7,7,12,.5)_56%,rgba(7,7,12,.92))]" />
        <div className="absolute inset-0 bg-primary/[0.08] mix-blend-color" />

        <div className="relative z-10 flex items-start justify-between p-4 sm:p-6">
          <Button
            variant="outline"
            className="border-white/20 bg-black/22 text-white hover:bg-black/38 hover:text-white"
            onClick={() => navigate("/")}
          >
            <ArrowLeft /> Library
          </Button>
          <div className="flex items-center gap-2">
            <StatusBadge song={song} queueStatus={queueStatus} />
          </div>
        </div>

        <div className="absolute inset-x-0 bottom-0 z-10 flex items-end gap-5 px-5 pb-7 sm:px-8 lg:px-10">
          <AlbumArt
            song={song}
            className="hidden size-32 rounded-sm shadow-2xl shadow-black/50 ring-1 ring-white/20 sm:block lg:size-40"
            fallbackIconClassName="size-10"
            lazy={false}
          />
          <div className="min-w-0 flex-1 text-white">
            <p className="text-[9px] font-medium uppercase tracking-[0.2em] text-white/60">
              Production master
            </p>
            <h1 className="mt-1 line-clamp-2 text-3xl font-light tracking-tight text-balance lg:text-4xl">
              {song.title}
            </h1>
            <p className="mt-2 truncate text-sm text-white/76">
              {song.artist || "Unknown artist"}
              {song.album ? ` · ${song.album}` : ""}
            </p>
          </div>
        </div>
      </section>

      <section className="mx-auto max-w-7xl px-4 py-5 sm:px-7 lg:px-10 lg:py-7">
        <div className="flex flex-wrap items-center justify-between gap-4 border-b border-border/55 pb-5">
          <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[10px] text-muted-foreground">
            <span className="inline-flex items-center gap-1.5">
              <Clock3 className="size-3.5" /> {formatSeconds(song.duration_secs)}
            </span>
            <span className="inline-flex items-center gap-1.5">
              <Disc3 className="size-3.5" /> {song.album || "Unknown album"}
            </span>
            {isDisplayableLanguage(song.language) ? (
              <LanguageBadge language={song.language} />
            ) : null}
            {song.key ? <span>Key {song.override_key ?? song.key}</span> : null}
            <span>{song.tempo.toFixed(1)}× tempo</span>
          </div>
          <div className="flex items-center gap-2">
            <SongActionsMenu
              song={song}
              status={status}
              analysisBusy={analysisBusy}
              supportsAnalysisActions={supportsAnalysisActions}
            />
            <Button
              size="lg"
              className="min-w-36"
              disabled={editorAction.disabled || shifting.key || shifting.tempo}
              onClick={editorAction.trigger}
              title={editorAction.description}
            >
              <PencilRuler /> {editorAction.label}
            </Button>
            {editorAction.needsSetup ? (
              <Button variant="ghost" size="sm" onClick={editorAction.openAnalysisSettings}>
                Analysis settings
              </Button>
            ) : null}
          </div>
        </div>

        <div className="mt-6 grid gap-5 xl:grid-cols-[minmax(0,1.25fr)_minmax(22rem,.75fr)]">
          <section className="rounded-md border border-border/55 bg-card/32 backdrop-blur-xl">
            <KeyTempoSection
              song={song}
              supportsShifts={supportsShifts}
              shifting={shifting}
              setShifting={setShifting}
            />
            <AnalysisTuningSection />
          </section>
          <section className="overflow-hidden rounded-md border border-border/55 bg-card/28 backdrop-blur-xl">
            <div className="border-b border-border/50 px-4 py-3">
              <p className="text-[8px] font-medium uppercase tracking-[0.16em] text-primary">
                Production overview
              </p>
              <h2 className="mt-1 text-sm font-medium">Track information</h2>
            </div>
            <dl className="grid grid-cols-2 gap-px bg-border/35">
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  {song.is_video ? (
                    <Video className="size-3.5" />
                  ) : (
                    <FileAudio2 className="size-3.5" />
                  )}
                  Media
                </dt>
                <dd className="mt-1 text-xs font-medium">
                  {extension} · {song.is_video ? "Video" : "Audio"}
                </dd>
              </div>
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  <AudioLines className="size-3.5" /> Analysis
                </dt>
                <dd className="mt-1 text-xs font-medium">{status.label}</dd>
              </div>
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  <Languages className="size-3.5" /> Lyrics source
                </dt>
                <dd className="mt-1 truncate text-xs font-medium" title={sourceLabel}>
                  {sourceLabel}
                </dd>
              </div>
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  <SlidersHorizontal className="size-3.5" /> Stems
                </dt>
                <dd className="mt-1 text-xs font-medium">
                  {song.no_stems ? "Original mix" : song.is_analyzed ? "Separated" : "Pending"}
                </dd>
              </div>
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  <CheckCircle2 className="size-3.5" /> Chart assets
                </dt>
                <dd className="mt-1 truncate text-xs font-medium" title={missingLabel}>
                  {song.authoring_ready ? "Complete" : missingLabel}
                </dd>
              </div>
              <div className="bg-background/60 p-3">
                <dt className="flex items-center gap-1.5 text-[9px] text-muted-foreground">
                  <FileOutput className="size-3.5" /> Export
                </dt>
                <dd className="mt-1 text-xs font-medium">
                  {song.authoring_ready ? "UTZ · UltraStar" : "Waiting for chart"}
                </dd>
              </div>
            </dl>
            <div className="flex items-start gap-2 border-t border-border/45 px-4 py-3">
              <FolderOpen className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
              <div className="min-w-0">
                <p className="text-[9px] text-muted-foreground">Source file</p>
                <p className="mt-0.5 truncate text-[10px]" title={song.path}>
                  {song.path}
                </p>
              </div>
            </div>
          </section>
        </div>
      </section>
    </main>
  );
};
