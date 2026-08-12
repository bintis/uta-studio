import { Button } from "@/components/ui/button";
import { ButtonGroup, ButtonGroupText } from "@/components/ui/button-group";
import { Spinner } from "@/components/ui/spinner";
import { MinusIcon, PlusIcon } from "lucide-react";
import type { MouseEvent } from "react";

interface Props {
  ariaLabel: string;
  disabled?: {
    plus?: boolean;
    minus?: boolean;
  };
  loading?: boolean;
  label?: string | null;
  onClick?: {
    plus?: () => void;
    minus?: () => void;
  };
}

export const Stepper = ({
  ariaLabel,
  label,
  loading,
  disabled: { plus: plusDisabled, minus: minusDisabled } = {},
  onClick = {},
}: Props) => {
  const withStopPropagation = (callback?: () => void) => (event: MouseEvent) => {
    event.stopPropagation();
    callback?.();
  };

  return (
    <ButtonGroup
      aria-label={ariaLabel}
      className="ml-auto max-w-full shrink-0"
      data-song-details-nav-group
    >
      <Button
        type="button"
        onClick={withStopPropagation(onClick.minus)}
        disabled={minusDisabled}
        variant="outline"
        size="icon-sm"
        aria-label={`Decrease ${ariaLabel}`}
      >
        <MinusIcon />
      </Button>
      <ButtonGroupText className="min-w-12 justify-center bg-background px-2 font-variant-numeric tabular-nums">
        {loading ? <Spinner className="size-3 will-change-transform" /> : (label ?? "—")}
      </ButtonGroupText>
      <Button
        type="button"
        onClick={withStopPropagation(onClick.plus)}
        disabled={plusDisabled}
        variant="outline"
        size="icon-sm"
        aria-label={`Increase ${ariaLabel}`}
      >
        <PlusIcon />
      </Button>
    </ButtonGroup>
  );
};
