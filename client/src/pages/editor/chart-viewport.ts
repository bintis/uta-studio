export const MIN_TIME_ZOOM = 30;
export const MAX_TIME_ZOOM = 180;
export const MIN_PITCH_SPAN = 8;
export const MAX_PITCH_SPAN = 72;

const finiteOr = (value: number, fallback: number): number =>
  Number.isFinite(value) ? value : fallback;

export const clampTimeZoom = (zoom: number): number =>
  Math.max(MIN_TIME_ZOOM, Math.min(MAX_TIME_ZOOM, finiteOr(zoom, MIN_TIME_ZOOM)));

export const visibleTime = (width: number, zoom: number): number =>
  Math.max(1, finiteOr(width, 1)) / clampTimeZoom(zoom);

export const maximumViewStart = (duration: number, width: number, zoom: number): number =>
  Math.max(0, finiteOr(duration, 0) - visibleTime(width, zoom));

export const clampViewStart = (
  viewStart: number,
  duration: number,
  width: number,
  zoom: number,
): number => Math.max(0, Math.min(maximumViewStart(duration, width, zoom), finiteOr(viewStart, 0)));

export const zoomTimeAroundPointer = ({
  viewStart,
  pointerX,
  currentZoom,
  nextZoom,
  duration,
  width,
}: {
  viewStart: number;
  pointerX: number;
  currentZoom: number;
  nextZoom: number;
  duration: number;
  width: number;
}): { zoom: number; viewStart: number } => {
  const zoom = clampTimeZoom(nextZoom);
  const x = Math.max(0, Math.min(Math.max(0, width), finiteOr(pointerX, 0)));
  const anchorTime = finiteOr(viewStart, 0) + x / clampTimeZoom(currentZoom);
  return {
    zoom,
    viewStart: clampViewStart(anchorTime - x / zoom, duration, width, zoom),
  };
};

export const clampPitchSpan = (span: number): number =>
  Math.max(MIN_PITCH_SPAN, Math.min(MAX_PITCH_SPAN, finiteOr(span, 24)));

export const clampPitchCenter = (center: number, span: number): number => {
  const safeSpan = clampPitchSpan(span);
  return Math.max(safeSpan / 2, Math.min(127 - safeSpan / 2, finiteOr(center, 60)));
};

export const pitchRange = (center: number, span: number): { min: number; max: number } => {
  const safeSpan = clampPitchSpan(span);
  const safeCenter = clampPitchCenter(center, safeSpan);
  return { min: safeCenter - safeSpan / 2, max: safeCenter + safeSpan / 2 };
};

export const assignTimedItemLanes = (
  items: Array<{ start: number; end: number }>,
  minimumVisibleSeconds = 0,
): { lanes: number[]; count: number } => {
  const laneEnds: number[] = [];
  const lanes = Array.from({ length: items.length }, () => 0);
  const ordered = items
    .map((item, index) => ({ item, index }))
    .sort(
      (left, right) =>
        left.item.start - right.item.start ||
        left.item.end - right.item.end ||
        left.index - right.index,
    );

  for (const { item, index } of ordered) {
    const start = Math.max(0, finiteOr(item.start, 0));
    const end = Math.max(start, finiteOr(item.end, start), start + minimumVisibleSeconds);
    let lane = laneEnds.findIndex((laneEnd) => start >= laneEnd + 0.01);
    if (lane < 0) {
      lane = laneEnds.length;
      laneEnds.push(end);
    } else {
      laneEnds[lane] = end;
    }
    lanes[index] = lane;
  }

  return { lanes, count: Math.max(1, laneEnds.length) };
};
