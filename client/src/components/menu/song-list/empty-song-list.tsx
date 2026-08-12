import { FolderIcon, MusicIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@/components/ui/empty";
import { useLibrarySourceActions } from "@/hooks/use-library-source-actions";

export const EmptySongList = () => {
  const { selectFolder, isPending } = useLibrarySourceActions();
  return (
    <main className="roon-library relative grid h-full flex-1 place-items-center overflow-hidden p-5">
      <div
        aria-hidden
        className="absolute left-1/2 top-1/2 size-[28rem] -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary/10 blur-3xl"
      />
      <Empty className="glass-panel relative max-w-xl rounded-md border-border/55 px-8 py-12 shadow-xl shadow-black/12">
        <EmptyHeader>
          <EmptyMedia variant="icon" className="rounded-lg bg-primary/12 text-primary">
            <MusicIcon />
          </EmptyMedia>
          <p className="text-[9px] font-semibold uppercase tracking-[0.2em] text-primary">
            First step
          </p>
          <EmptyTitle>Choose your song library</EmptyTitle>
          <EmptyDescription>
            Pick a local folder. Uta Studio will scan it, generate stems and charts with AI, then
            let you correct every word and note before exporting.
          </EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button size="lg" onClick={() => selectFolder()} disabled={isPending}>
            <FolderIcon /> Choose song folder
          </Button>
        </EmptyContent>
      </Empty>
    </main>
  );
};
