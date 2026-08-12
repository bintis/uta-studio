import * as React from "react";
import { cn } from "@/lib/utils";

export interface Segment {
  value: number;
  color: string;
}

function SegmentedProgress({
  segments,
  className,
  ...props
}: React.ComponentProps<"div"> & { segments: Segment[] }) {
  return (
    <div
      data-slot="segmented-progress"
      className={cn("flex h-2 w-full overflow-hidden rounded-full bg-muted", className)}
      {...props}
    >
      {segments.map((segment, i) => (
        <div
          key={i}
          className={cn("h-full transition-all", segment.color)}
          style={{ width: `${segment.value}%` }}
        />
      ))}
    </div>
  );
}

export { SegmentedProgress };
