import { useEffect, useState } from "react";

interface DimensionInputProps {
  value: number;
  onChange: (value: number) => void;
  ariaLabel: string;
  min?: number;
  max?: number;
  disabled?: boolean;
}

export function DimensionInput({
  value,
  onChange,
  ariaLabel,
  min = 16,
  max = 20000,
  disabled = false,
}: DimensionInputProps) {
  const [text, setText] = useState(String(value));

  useEffect(() => {
    setText(String(value));
  }, [value]);

  const commit = (): void => {
    const parsed = Number(text);
    if (!Number.isFinite(parsed)) {
      setText(String(value));
      return;
    }
    const next = Math.min(max, Math.max(min, Math.round(parsed)));
    setText(String(next));
    if (next !== value) onChange(next);
  };

  return (
    <input
      className="input dimension-input"
      type="number"
      inputMode="numeric"
      min={min}
      max={max}
      value={text}
      disabled={disabled}
      aria-label={ariaLabel}
      onChange={(event) => setText(event.currentTarget.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        }
      }}
    />
  );
}
