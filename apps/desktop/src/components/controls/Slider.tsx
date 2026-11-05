import { useId, useRef, type CSSProperties } from "react";
import { RotateCcw } from "lucide-react";
import { cn } from "../../lib/cn";

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  defaultValue?: number;
  unit?: string;
  hint?: string;
  disabled?: boolean;
  compact?: boolean;
  showReset?: boolean;
  formatValue?: (value: number) => string;
  onChange: (value: number) => void;
  onCommit?: () => void;
}

export function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  defaultValue,
  unit,
  hint,
  disabled = false,
  compact = false,
  showReset = true,
  formatValue,
  onChange,
  onCommit,
}: SliderProps) {
  const id = useId();
  const dirty = useRef(false);
  const percent = max > min ? ((value - min) / (max - min)) * 100 : 0;
  const style = { "--slider-fill": `${Math.min(100, Math.max(0, percent))}%` } as CSSProperties;
  const display = formatValue ? formatValue(value) : `${Math.round(value)}`;
  const canReset = showReset && defaultValue !== undefined && Math.abs(value - defaultValue) > 0.0001;

  const commit = (): void => {
    if (!dirty.current) return;
    dirty.current = false;
    onCommit?.();
  };

  return (
    <div className={cn("control-row", compact && "control-row--compact", disabled && "is-disabled")}>
      <div className="control-head">
        <label className="control-label" htmlFor={id} title={hint}>
          {label}
        </label>
        <div className="control-right">
          <span className="control-value">
            {display}
            {unit ? <span className="control-unit">{unit}</span> : null}
          </span>
          {canReset ? (
            <button
              type="button"
              className="control-reset"
              aria-label={`${label}恢复默认`}
              title={`恢复默认（${defaultValue}${unit ?? ""}）`}
              disabled={disabled}
              onClick={() => {
                onChange(defaultValue);
                dirty.current = false;
                onCommit?.();
              }}
            >
              <RotateCcw size={11} />
            </button>
          ) : (
            <span className="control-reset-spacer" aria-hidden="true" />
          )}
        </div>
      </div>
      <input
        id={id}
        className="range"
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        style={style}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        onChange={(event) => {
          dirty.current = true;
          onChange(Number(event.currentTarget.value));
        }}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
      />
    </div>
  );
}
