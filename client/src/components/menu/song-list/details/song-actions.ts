import type { Song } from "@/types/Song";
import {
  AlignLeftIcon,
  AudioLinesIcon,
  ActivityIcon,
  CaptionsIcon,
  PackageCheckIcon,
  FileOutputIcon,
  LanguagesIcon,
  PencilLineIcon,
  RefreshCwIcon,
  Trash2Icon,
  SettingsIcon,
} from "lucide-react";
import type { SongStatusInfo } from "../shared/song-status";
import type { ActionItemProps } from "./action-item";

type AnalysisHandler = (fileHash: string) => void | Promise<void>;

interface AnalysisHandlers {
  enqueueOne: AnalysisHandler;
  deleteSongCache: AnalysisHandler;
  reanalyzeFull: AnalysisHandler;
  reanalyzePitch: AnalysisHandler;
  reanalyzeTranscript: AnalysisHandler;
  realign: AnalysisHandler;
  reanalyzeForceTranscribe: AnalysisHandler;
}

interface BuildActionGroupsParams {
  song: Song;
  status: SongStatusInfo;
  analysisBusy: boolean;
  supportsAnalysisActions: boolean;
  analysisReady: boolean;
  analysisNeedsSetup: boolean;
  analysis: AnalysisHandlers;
  onEditLyrics: () => void;
  onChangeLanguage: () => void;
  onExportUtz: () => void | Promise<void>;
  onExportUltraStar: () => void | Promise<void>;
  onSetupAnalysis: () => void;
  run: (message: string, action: () => void | Promise<void>) => () => Promise<void>;
}

export function buildActionGroups({
  song,
  status,
  analysisBusy,
  supportsAnalysisActions,
  analysisReady,
  analysisNeedsSetup,
  analysis,
  onEditLyrics,
  onChangeLanguage,
  onExportUtz,
  onExportUltraStar,
  onSetupAnalysis,
  run,
}: BuildActionGroupsParams): ActionItemProps[][] {
  const groups: ActionItemProps[][] = [];

  const supportsProvideLyrics = song.transcript_source !== "Usdx";

  if (!status.isReady) {
    const notReadyGroup: ActionItemProps[] = [
      {
        icon: AudioLinesIcon,
        title: analysisBusy ? "Analysis in progress" : "Analyze song",
        description: analysisReady
          ? "Prepare lyrics, timing, key, tempo, and stems."
          : analysisNeedsSetup
            ? "Analysis runtime or models are missing. Set them up first."
            : "Checking the local analysis runtime and models.",
        disabled: analysisBusy || !analysisReady,
        onClick: () => analysis.enqueueOne(song.file_hash),
      },
    ];

    if (analysisNeedsSetup) {
      notReadyGroup.push({
        icon: SettingsIcon,
        title: "Set up analysis",
        description: "Choose a model runtime in Settings. Nothing downloads until you confirm.",
        onClick: onSetupAnalysis,
      });
    }

    if (supportsProvideLyrics) {
      notReadyGroup.push({
        icon: PencilLineIcon,
        title: "Provide lyrics",
        description: "Paste timed LRC, or lyrics to align.",
        disabled: analysisBusy,
        onClick: onEditLyrics,
      });
    }

    groups.push(notReadyGroup);
  }

  if (status.isReady) {
    groups.push([
      {
        icon: PackageCheckIcon,
        title: "Uta package (.utz)",
        description: "Complete, validated package for the uta player.",
        disabled: analysisBusy,
        onClick: onExportUtz,
      },
      {
        icon: FileOutputIcon,
        title: "UltraStar (.txt)",
        description: "Chart plus sibling audio, vocals, cover, and video files.",
        disabled: analysisBusy,
        onClick: onExportUltraStar,
      },
    ]);
  }

  if (analysisNeedsSetup && status.isReady) {
    groups.push([
      {
        icon: SettingsIcon,
        title: "Set up analysis",
        description: "Install or repair the selected models before re-analysis.",
        onClick: onSetupAnalysis,
      },
    ]);
  }

  if (supportsAnalysisActions) {
    // LRC-provided songs have no AI-generated stems/timing to rebuild, so the
    // realign/refetch/transcribe actions don't apply. Offer editing the LRC and
    // an explicit opt-in to replace it with full AI analysis instead.
    if (song.transcript_source === "Lrc") {
      const lrcActions: ActionItemProps[] = [
        {
          icon: PencilLineIcon,
          title: "Edit lyrics (LRC)",
          description: "Replace or re-time the provided LRC.",
          onClick: onEditLyrics,
        },
      ];
      if (analysisReady) {
        lrcActions.push(
          {
            icon: ActivityIcon,
            title: "Frequency analysis",
            description: "Generate or repair editable pitch notes.",
            disabled: analysisBusy,
            onClick: run(`Analyzing pitch for "${song.title}"`, () =>
              analysis.reanalyzePitch(song.file_hash),
            ),
          },
          {
            icon: AudioLinesIcon,
            title: "Analyze with AI",
            description: "Replace the LRC with AI stems, lyrics, timing, and key.",
            disabled: analysisBusy,
            onClick: run(`Analyzing "${song.title}" with AI`, () =>
              analysis.reanalyzeFull(song.file_hash),
            ),
          },
        );
      }
      groups.push(lrcActions);
    } else {
      if (analysisReady) {
        groups.push([
          {
            icon: AlignLeftIcon,
            title: "Realign",
            description: "Rebuild timing from the current lyrics.",
            disabled: analysisBusy,
            onClick: run(`Realigning "${song.title}"`, () => analysis.realign(song.file_hash)),
          },
          {
            icon: RefreshCwIcon,
            title: "Refetch lyrics & align",
            description: "Fetch fresh lyrics, then rebuild timing.",
            disabled: analysisBusy,
            onClick: run(`Refetching lyrics & aligning "${song.title}"`, () =>
              analysis.reanalyzeTranscript(song.file_hash),
            ),
          },
          {
            icon: CaptionsIcon,
            title: "Force transcribe",
            description: "Ignore online lyrics and transcribe the vocals.",
            disabled: analysisBusy,
            onClick: run(`Force transcribing "${song.title}"`, () =>
              analysis.reanalyzeForceTranscribe(song.file_hash),
            ),
          },
          {
            icon: ActivityIcon,
            title: "Frequency analysis",
            description: "Generate or repair editable pitch notes.",
            disabled: analysisBusy,
            onClick: run(`Analyzing pitch for "${song.title}"`, () =>
              analysis.reanalyzePitch(song.file_hash),
            ),
          },
          {
            icon: AudioLinesIcon,
            title: "Full reanalysis",
            description: "Recreate stems, lyrics, timing, key, and tempo.",
            disabled: analysisBusy,
            onClick: run(`Full reanalysis (w/ stems) for "${song.title}"`, () =>
              analysis.reanalyzeFull(song.file_hash),
            ),
          },
        ]);
      }

      groups.push([
        {
          icon: PencilLineIcon,
          title: "Edit lyrics",
          description: "Correct the words and rebuild their timing.",
          onClick: onEditLyrics,
        },
        {
          icon: LanguagesIcon,
          title: "Change language",
          description: "Set the language and choose how to reprocess.",
          onClick: onChangeLanguage,
        },
      ]);
    }

    groups.push([
      {
        icon: Trash2Icon,
        title: "Delete cache",
        description: "Remove every generated file for this song.",
        destructive: true,
        onClick: run(`Cache deleted for "${song.title}"`, () =>
          analysis.deleteSongCache(song.file_hash),
        ),
      },
    ]);
  }

  return groups;
}
