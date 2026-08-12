import { useEffect, useRef, useState } from "react";

export type AudioWaveform = {
  peaks: Float32Array;
  duration: number;
};

const PEAK_BUCKETS = 6000;

export function useAudioWaveform(
  source?: string,
  suspended = false,
): {
  waveform: AudioWaveform | null;
  loading: boolean;
} {
  const [waveform, setWaveform] = useState<AudioWaveform | null>(null);
  const [loading, setLoading] = useState(false);
  const activeSourceRef = useRef<string | undefined>(undefined);
  const decodedSourceRef = useRef<string | undefined>(undefined);

  useEffect(() => {
    if (activeSourceRef.current !== source) {
      activeSourceRef.current = source;
      decodedSourceRef.current = undefined;
      setWaveform(null);
    }
    if (!source) {
      setWaveform(null);
      setLoading(false);
      return;
    }
    if (suspended || decodedSourceRef.current === source) {
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    let context: AudioContext | null = null;
    let idleHandle: number | undefined;
    let timeoutHandle: ReturnType<typeof globalThis.setTimeout> | undefined;
    setLoading(true);

    const decode = async () => {
      try {
        const response = await fetch(source, { signal: controller.signal });
        if (!response.ok) throw new Error(`Audio request failed (${response.status})`);
        const encoded = await response.arrayBuffer();
        if (controller.signal.aborted) return;
        context = new AudioContext();
        const audio = await context.decodeAudioData(encoded.slice(0));
        if (controller.signal.aborted) return;
        const buckets = Math.max(1, Math.min(PEAK_BUCKETS, audio.length));
        const peaks = new Float32Array(buckets * 2);
        const samplesPerBucket = audio.length / buckets;
        for (let bucket = 0; bucket < buckets; bucket += 1) {
          const start = Math.floor(bucket * samplesPerBucket);
          const end = Math.max(start + 1, Math.floor((bucket + 1) * samplesPerBucket));
          let minimum = 0;
          let maximum = 0;
          for (let channel = 0; channel < audio.numberOfChannels; channel += 1) {
            const samples = audio.getChannelData(channel);
            const stride = Math.max(1, Math.floor((end - start) / 96));
            for (let sample = start; sample < end; sample += stride) {
              minimum = Math.min(minimum, samples[sample] ?? 0);
              maximum = Math.max(maximum, samples[sample] ?? 0);
            }
          }
          peaks[bucket * 2] = minimum;
          peaks[bucket * 2 + 1] = maximum;
        }
        decodedSourceRef.current = source;
        setWaveform({ peaks, duration: audio.duration });
      } catch (error) {
        if (!controller.signal.aborted) {
          console.warn("Could not decode chart waveform", error);
          setWaveform(null);
        }
      } finally {
        if (!controller.signal.aborted) setLoading(false);
        if (context) void context.close();
      }
    };

    if (typeof window.requestIdleCallback === "function") {
      idleHandle = window.requestIdleCallback(() => void decode(), { timeout: 1200 });
    } else {
      timeoutHandle = globalThis.setTimeout(() => void decode(), 350);
    }

    return () => {
      controller.abort();
      if (idleHandle !== undefined) window.cancelIdleCallback(idleHandle);
      if (timeoutHandle !== undefined) window.clearTimeout(timeoutHandle);
      if (context) void context.close();
    };
  }, [source, suspended]);

  return { waveform, loading };
}
