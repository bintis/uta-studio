import { onShiftKeyDone, onShiftTempoDone, shiftKey, shiftTempo } from "@/bridge/analysis";
import type { Song } from "@/types/Song";
import { calculateKeyShift } from "@/utils/shift-key";
import { useEffect, useRef } from "react";
import { Stepper } from "./stepper";

export type ShiftType = "tempo" | "key";

interface ShiftListener {
  register: typeof onShiftKeyDone;
  successMessage: string;
  errorMessage: (error: string) => string;
}

const SHIFT_LISTENERS: Record<ShiftType, ShiftListener> = {
  key: {
    register: onShiftKeyDone,
    successMessage: "Song key shifted successfully",
    errorMessage: (error) => `Error while shifting the key: ${error}`,
  },
  tempo: {
    register: onShiftTempoDone,
    successMessage: "Song tempo shifted successfully",
    errorMessage: (error) => `Error while shifting the tempo: ${error}`,
  },
};

interface Props {
  song: Song;
  status: Record<ShiftType, boolean>;
  onStart: (shiftType: ShiftType) => void;
  onSuccess: (message: string, shiftType: ShiftType) => void;
  onError: (message: string, shiftType: ShiftType) => void;
}

export const Shifts = ({ song, status, onSuccess, onError, onStart }: Props) => {
  const onSuccessRef = useRef(onSuccess);
  const onErrorRef = useRef(onError);
  onSuccessRef.current = onSuccess;
  onErrorRef.current = onError;

  const withOnStart = (callback: () => void, shiftType: ShiftType) => () => {
    onStart(shiftType);

    try {
      callback();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      onErrorRef.current(`Error while shifting the ${shiftType}: ${message}`, shiftType);
    }
  };

  useEffect(() => {
    let cancelled = false;
    const unlisteners: Partial<Record<ShiftType, () => void>> = {};

    (Object.entries(SHIFT_LISTENERS) as Array<[ShiftType, ShiftListener]>).forEach(
      ([shiftType, config]) => {
        config
          .register(({ file_hash, error }) => {
            if (file_hash !== song.file_hash) return;

            if (!error) onSuccessRef.current(config.successMessage, shiftType);
            else onErrorRef.current(config.errorMessage(error), shiftType);
          })
          .then((unlisten) => {
            if (cancelled) unlisten();
            else unlisteners[shiftType] = unlisten;
          });
      },
    );

    return () => {
      cancelled = true;
      unlisteners.tempo?.();
      unlisteners.key?.();
    };
  }, [song.file_hash]);

  if (!song.is_analyzed || song.transcript_source === "Usdx") return null;

  const loading = status.key || status.tempo;

  const onShiftKey = (direction: "up" | "down") => {
    if (!song.key) return;

    const { key, keyOffset, pitchRatio } = calculateKeyShift(
      song.key,
      song.key_offset + (direction === "up" ? 1 : -1),
    );
    shiftKey(song.file_hash, key, pitchRatio, keyOffset);
  };

  const controls = [
    {
      shiftType: "tempo" as const,
      title: "Tempo",
      description: "Create an export-speed variant in 0.1× steps.",
      value: `${song.tempo.toFixed(1)}×`,
      onPlus: () => shiftTempo(song.file_hash, song.tempo + 0.1),
      onMinus: () => shiftTempo(song.file_hash, song.tempo - 0.1),
      disabled: { plus: song.tempo >= 2, minus: song.tempo <= 0.5 },
    },
    {
      shiftType: "key" as const,
      title: "Key",
      description: song.key ? `Original key: ${song.key}` : "Analyze again to detect the key.",
      value: song.override_key ?? song.key,
      onPlus: () => onShiftKey("up"),
      onMinus: () => onShiftKey("down"),
      disabled: {
        plus: song.key_offset >= 5 || !song.key,
        minus: song.key_offset <= -5 || !song.key,
      },
    },
  ];

  return (
    <div className="divide-y">
      {controls.map(({ shiftType, title, description, value, onPlus, onMinus, disabled }) => (
        <div key={shiftType} className="flex flex-wrap items-center gap-x-3 gap-y-2 py-2">
          <div className="min-w-0 basis-48 flex-1">
            <p className="text-xs font-medium">{title}</p>
            <p className="mt-0.5 text-[0.625rem] leading-snug text-muted-foreground">
              {description}
            </p>
          </div>
          <Stepper
            ariaLabel={title.toLowerCase()}
            loading={status[shiftType]}
            label={value}
            onClick={{
              plus: withOnStart(onPlus, shiftType),
              minus: withOnStart(onMinus, shiftType),
            }}
            disabled={{ plus: loading || disabled.plus, minus: loading || disabled.minus }}
          />
        </div>
      ))}
    </div>
  );
};
