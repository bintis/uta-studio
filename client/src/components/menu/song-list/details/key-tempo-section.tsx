import { SONGS } from "@/queries/keys";
import type { Song } from "@/types/Song";
import { useQueryClient } from "@tanstack/react-query";
import type { Dispatch, SetStateAction } from "react";
import { toast } from "sonner";
import { Shifts, type ShiftType } from "../shifts";

interface KeyTempoSectionProps {
  song: Song;
  supportsShifts: boolean;
  shifting: Record<ShiftType, boolean>;
  setShifting: Dispatch<SetStateAction<Record<ShiftType, boolean>>>;
}

export const KeyTempoSection = ({
  song,
  supportsShifts,
  shifting,
  setShifting,
}: KeyTempoSectionProps) => {
  const queryClient = useQueryClient();
  const sectionClass = supportsShifts ? "px-4 pt-4 pb-2" : "px-4 py-4";

  return (
    <section className={sectionClass} aria-labelledby="song-adjustments-heading">
      <h3
        id="song-adjustments-heading"
        className="mb-2 text-[9px] font-semibold uppercase tracking-[0.16em] text-muted-foreground"
      >
        Key & tempo
      </h3>
      {supportsShifts ? (
        <Shifts
          song={song}
          status={shifting}
          onStart={(type) => setShifting((current) => ({ ...current, [type]: true }))}
          onSuccess={(message, type) => {
            toast.success(message);
            queryClient.invalidateQueries({ queryKey: SONGS });
            setShifting((current) => ({ ...current, [type]: false }));
          }}
          onError={(message, type) => {
            toast.error(message);
            setShifting((current) => ({ ...current, [type]: false }));
          }}
        />
      ) : (
        <p className="max-w-72 text-xs leading-relaxed text-muted-foreground">
          Key and tempo controls become available after compatible analysis.
        </p>
      )}
    </section>
  );
};
