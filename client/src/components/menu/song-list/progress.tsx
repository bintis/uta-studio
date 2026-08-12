import { LoaderCircleIcon } from "lucide-react";

import { Progress as ShadCnProgress } from "@/components/ui/progress";
import { useSongsMeta } from "@/queries/use-songs";

export const Progress = () => {
  const { data: meta } = useSongsMeta();

  if (!meta) {
    return null;
  }

  const { count, processed_count, folder } = meta;

  // The folder label is committed before the first scan batch reaches SQLite.
  if (folder && count === 0 && processed_count === 0) {
    return (
      <div className="flex items-center gap-1 border-b border-border/45 px-5 py-2 text-[10px] text-muted-foreground">
        <LoaderCircleIcon className="size-3 animate-spin" />
        Scanning local folder…
      </div>
    );
  }

  if (count === processed_count) {
    return null;
  }

  return <ShadCnProgress max={count} value={processed_count} />;
};
