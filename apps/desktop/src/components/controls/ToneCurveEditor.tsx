import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { sampleMonotoneCurve } from "../../lib/curve";
import { histogramScale } from "../../lib/histogram";

interface ToneCurveEditorProps {
  points: number[];
  values: number[];
  /** Luma bins used as a faint backdrop, so edits are guided by the photo. */
  histogram?: number[] | null;
  onChange: (index: number, value: number) => void;
  onCommit: () => void;
}

const HEIGHT = 168;
const PAD = 12;
const GRID_STEPS = [0.25, 0.5, 0.75];

export function ToneCurveEditor({
  points,
  values,
  histogram,
  onChange,
  onCommit,
}: ToneCurveEditorProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const dragRef = useRef<number | null>(null);
  const [width, setWidth] = useState(0);
  const onChangeRef = useRef(onChange);
  const onCommitRef = useRef(onCommit);
  onChangeRef.current = onChange;
  onCommitRef.current = onCommit;

  useEffect(() => {
    const element = rootRef.current;
    if (!element) return;
    const measure = (): void => {
      const rect = element.getBoundingClientRect();
      setWidth((current) => (Math.abs(current - rect.width) < 0.5 ? current : rect.width));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const plotWidth = Math.max(1, width - PAD * 2);
  const plotHeight = HEIGHT - PAD * 2;
  const toX = (value: number): number => PAD + value * plotWidth;
  const toY = (value: number): number => PAD + (1 - value) * plotHeight;

  const move = useCallback(
    (event: PointerEvent): void => {
      if (dragRef.current === null) return;
      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;
      const value = 1 - (event.clientY - rect.top - PAD) / plotHeight;
      onChangeRef.current(dragRef.current, Math.min(1, Math.max(0, value)));
    },
    [plotHeight],
  );

  const finish = useCallback((): void => {
    if (dragRef.current === null) return;
    dragRef.current = null;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", finish);
    window.removeEventListener("pointercancel", finish);
    onCommitRef.current();
  }, [move]);

  useEffect(() => () => finish(), [finish]);

  const begin = (index: number, event: ReactPointerEvent<SVGGElement>): void => {
    event.preventDefault();
    event.stopPropagation();
    dragRef.current = index;
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  };

  const onHandleKeyDown = (index: number, event: ReactKeyboardEvent<SVGGElement>): void => {
    const step = event.shiftKey ? 0.1 : 0.01;
    let next = values[index];
    switch (event.key) {
      case "ArrowUp":
      case "ArrowRight":
        next += step;
        break;
      case "ArrowDown":
      case "ArrowLeft":
        next -= step;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    onChange(index, Math.min(1, Math.max(0, next)));
  };

  const onHandleKeyUp = (event: ReactKeyboardEvent<SVGGElement>): void => {
    if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
      onCommit();
    }
  };

  const curve = sampleMonotoneCurve(values, 96);
  const line = curve
    .map((point, index) => `${index === 0 ? "M" : "L"}${toX(point.x).toFixed(2)},${toY(point.y).toFixed(2)}`)
    .join(" ");
  const area = `${line} L${toX(1).toFixed(2)},${toY(0).toFixed(2)} L${toX(0).toFixed(2)},${toY(0).toFixed(2)} Z`;

  let histogramPath: string | null = null;
  if (histogram && histogram.length > 1) {
    const scale = histogramScale(histogram);
    histogramPath = histogram
      .map((bin, index) => {
        const x = toX(index / (histogram.length - 1));
        const height = Math.min(1, bin / scale) * plotHeight * 0.92;
        return `${index === 0 ? "M" : "L"}${x.toFixed(2)},${(toY(0) - height).toFixed(2)}`;
      })
      .join(" ");
    histogramPath += ` L${toX(1).toFixed(2)},${toY(0).toFixed(2)} L${toX(0).toFixed(2)},${toY(0).toFixed(2)} Z`;
  }

  return (
    <div className="tone-curve" ref={rootRef}>
      <svg
        ref={svgRef}
        className="tone-curve__svg"
        viewBox={`0 0 ${Math.max(1, width)} ${HEIGHT}`}
        width="100%"
        height={HEIGHT}
        role="group"
        aria-label="RGB 主曲线，拖动圆点或使用方向键调整输出"
      >
        {histogramPath ? <path className="tone-curve__histogram" d={histogramPath} /> : null}
        {GRID_STEPS.map((step) => (
          <g key={step}>
            <line
              className="tone-curve__grid"
              x1={toX(step)}
              y1={PAD}
              x2={toX(step)}
              y2={toY(0)}
            />
            <line
              className="tone-curve__grid"
              x1={toX(0)}
              y1={toY(step)}
              x2={toX(1)}
              y2={toY(step)}
            />
          </g>
        ))}
        <line
          className="tone-curve__diagonal"
          x1={toX(0)}
          y1={toY(0)}
          x2={toX(1)}
          y2={toY(1)}
        />
        <path className="tone-curve__fill" d={area} />
        <path className="tone-curve__line" d={line} />
        {points.map((point, index) => (
          <g
            key={`${point}-${index}`}
            className="tone-curve__point"
            tabIndex={0}
            role="slider"
            aria-label={`控制点 ${index + 1}`}
            aria-valuemin={0}
            aria-valuemax={1}
            aria-valuenow={Number(values[index].toFixed(3))}
            onPointerDown={(event) => begin(index, event)}
            onKeyDown={(event) => onHandleKeyDown(index, event)}
            onKeyUp={onHandleKeyUp}
          >
            <circle className="tone-curve__hit" cx={toX(point)} cy={toY(values[index])} r={14} />
            <circle className="tone-curve__handle" cx={toX(point)} cy={toY(values[index])} r={5.5} />
          </g>
        ))}
      </svg>
      <div className="tone-curve__labels" aria-hidden="true">
        <span>暗部</span>
        <span>中间调</span>
        <span>高光</span>
      </div>
    </div>
  );
}
