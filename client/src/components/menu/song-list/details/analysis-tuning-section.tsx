import {
  ALIGN_BACKENDS,
  ASR_ENGINES,
  DEFAULTS,
  MODELS,
  PITCH_MODELS,
  SEPARATORS,
  VOCAL_THRESHOLD_MAX,
} from "@/components/menu/settings/constants";
import { NumberButtonGroup, SettingsSelect } from "@/components/menu/settings/settings-controls";
import { modelSettingsVisibility } from "@/components/menu/settings/settings-logic";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Slider } from "@/components/ui/slider";
import { useConfigMutation } from "@/mutations/use-config-mutation";
import { useConfig } from "@/queries/use-config";
import { SlidersHorizontal } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";

const INTEL_ONLY_SEPARATORS = new Set(["openvino_demucs"]);
const noFocusClassName = () => "";

function TuningRow({
  label,
  description,
  children,
}: {
  label: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-2 border-t border-border/40 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_12.5rem] sm:items-center sm:gap-5">
      <div className="min-w-0">
        <p className="text-[11px] font-medium">{label}</p>
        <p className="mt-0.5 text-[9px] leading-relaxed text-muted-foreground">{description}</p>
      </div>
      <div className="min-w-0 justify-self-stretch sm:justify-self-end">{children}</div>
    </div>
  );
}

export function AnalysisTuningSection() {
  const { data: config } = useConfig();
  const { mutate } = useConfigMutation();
  const engine = config?.asr_engine ?? DEFAULTS.asr_engine;
  const visibility = modelSettingsVisibility(engine);
  const isParakeet = engine === "parakeet";
  const [thresholdPct, setThresholdPct] = useState(
    Math.round(
      (config?.vocal_detection_threshold_pct ?? DEFAULTS.vocal_detection_threshold_pct) * 100,
    ),
  );
  const modelOptions = useMemo(() => MODELS.map((model) => ({ value: model, label: model })), []);
  const separatorOptions = useMemo(
    () =>
      config?.compute_backend === "intel"
        ? SEPARATORS
        : SEPARATORS.filter(({ value }) => !INTEL_ONLY_SEPARATORS.has(value)),
    [config?.compute_backend],
  );

  useEffect(() => {
    setThresholdPct(
      Math.round(
        (config?.vocal_detection_threshold_pct ?? DEFAULTS.vocal_detection_threshold_pct) * 100,
      ),
    );
  }, [config?.vocal_detection_threshold_pct]);

  return (
    <section
      className="border-t border-border/45 px-4 py-4"
      aria-labelledby="analysis-tuning-title"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0">
          <h3
            id="analysis-tuning-title"
            className="text-[9px] font-semibold uppercase tracking-[0.16em] text-muted-foreground"
          >
            Analysis tuning
          </h3>
          <p className="mt-1 max-w-xl text-[10px] leading-relaxed text-muted-foreground">
            {SEPARATORS.find(({ value }) => value === config?.separator)?.label ?? "UVR Karaoke"} ·{" "}
            {ASR_ENGINES.find(({ value }) => value === engine)?.label ?? "Whisper"} · vocal gate{" "}
            {thresholdPct}%
          </p>
        </div>
        <Popover>
          <PopoverTrigger asChild>
            <Button variant="outline">
              <SlidersHorizontal /> Tune analysis…
            </Button>
          </PopoverTrigger>
          <PopoverContent
            align="end"
            side="top"
            sideOffset={8}
            className="w-[min(94vw,30rem)] max-h-[min(76vh,44rem)] gap-0 overflow-y-auto rounded-md bg-background/92 p-0 shadow-2xl backdrop-blur-2xl"
          >
            <PopoverHeader className="px-4 py-4">
              <PopoverTitle>Analysis parameters</PopoverTitle>
              <PopoverDescription>
                These defaults apply the next time this song—or another track—is analyzed. Existing
                chart data stays unchanged until you choose a re-analysis action.
              </PopoverDescription>
            </PopoverHeader>

            <TuningRow
              label="Vocal separator"
              description="Changes the vocal and instrumental stems."
            >
              <SettingsSelect
                label="Vocal separator"
                placeholder="Select a separator"
                value={config?.separator ?? DEFAULTS.separator}
                options={separatorOptions}
                onValueChange={(separator) => mutate({ separator })}
              />
            </TuningRow>

            <TuningRow label="Transcription" description="Chooses the lyric recognition family.">
              <SettingsSelect
                label="Transcription"
                placeholder="Select an engine"
                value={engine}
                options={ASR_ENGINES}
                onValueChange={(asr_engine) => mutate({ asr_engine })}
              />
            </TuningRow>

            {visibility.whisperModel ? (
              <TuningRow
                label={isParakeet ? "Whisper fallback" : "Whisper model"}
                description={
                  isParakeet
                    ? "Used for unsupported languages and empty Parakeet results."
                    : "Larger models can improve detail but take longer."
                }
              >
                <SettingsSelect
                  label="Whisper model"
                  placeholder="Select a model"
                  value={config?.whisper_model ?? DEFAULTS.whisper_model}
                  options={modelOptions}
                  onValueChange={(whisper_model) => mutate({ whisper_model })}
                />
              </TuningRow>
            ) : null}

            <TuningRow label="Recognition precision" description="Whisper beam search breadth.">
              <NumberButtonGroup
                name="beam_size"
                value={config?.beam_size ?? DEFAULTS.beam_size}
                segment={0}
                getFocusClassName={noFocusClassName}
                onChange={(beam_size) => mutate({ beam_size })}
              />
            </TuningRow>

            <TuningRow
              label="Batch size"
              description="Lower this when GPU or system memory is tight."
            >
              <NumberButtonGroup
                name="batch_size"
                value={config?.batch_size ?? DEFAULTS.batch_size}
                segment={0}
                getFocusClassName={noFocusClassName}
                onChange={(batch_size) => mutate({ batch_size })}
              />
            </TuningRow>

            {visibility.wordAlignment ? (
              <TuningRow
                label="Word alignment"
                description="Controls word-level timing refinement."
              >
                <SettingsSelect
                  label="Word alignment"
                  placeholder="Select an aligner"
                  value={config?.align_backend ?? DEFAULTS.align_backend}
                  options={ALIGN_BACKENDS}
                  onValueChange={(align_backend) => mutate({ align_backend })}
                />
              </TuningRow>
            ) : null}

            <TuningRow label="Frequency analysis" description="Builds the editable pitch guide.">
              <SettingsSelect
                label="Frequency analysis"
                placeholder="Select a pitch model"
                value={config?.pitch_model ?? DEFAULTS.pitch_model}
                options={PITCH_MODELS}
                onValueChange={(pitch_model) => mutate({ pitch_model })}
              />
            </TuningRow>

            <TuningRow
              label={`Vocal detection · ${thresholdPct}%`}
              description="Lower for soft singing; raise to reject more silence and leakage."
            >
              <Slider
                min={0}
                max={Math.round(VOCAL_THRESHOLD_MAX * 100)}
                step={1}
                value={[thresholdPct]}
                aria-label="Vocal detection sensitivity"
                onValueChange={([value]) => setThresholdPct(value)}
                onValueCommit={([value]) => mutate({ vocal_detection_threshold_pct: value / 100 })}
              />
            </TuningRow>
          </PopoverContent>
        </Popover>
      </div>
    </section>
  );
}
