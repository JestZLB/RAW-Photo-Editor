import type { Histogram } from "../../api/types";
import { histogramScale } from "../../lib/histogram";

const WIDTH = 100;
const HEIGHT = 40;
const CHANNELS = ["red", "green", "blue"] as const;

function linePath(values: number[], max: number): string {
  return values
    .map((value, index) => {
      const x = (index / Math.max(1, values.length - 1)) * WIDTH;
      const y = HEIGHT - Math.min(1, value / max) * (HEIGHT - 2) - 1;
      return `${index === 0 ? "M" : "L"}${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}

function areaPath(values: number[], max: number): string {
  return `${linePath(values, max)} L${WIDTH},${HEIGHT} L0,${HEIGHT} Z`;
}

export function HistogramView({ histogram }: { histogram: Histogram | null }) {
  if (!histogram) {
    return (
      <div className="histogram is-empty" role="status">
        调整后显示直方图
      </div>
    );
  }
  const values: number[] = [];
  for (const channel of CHANNELS) values.push(...histogram[channel]);
  values.push(...histogram.luma);
  const max = histogramScale(values);
  return (
    <div className="histogram" role="img" aria-label="RGB 与亮度直方图">
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        preserveAspectRatio="none"
        shapeRendering="geometricPrecision"
      >
        <g className="histogram__grid" aria-hidden="true">
          {[0.25, 0.5, 0.75].map((fraction) => (
            <line
              key={fraction}
              x1={WIDTH * fraction}
              y1={0}
              x2={WIDTH * fraction}
              y2={HEIGHT}
            />
          ))}
          <line x1={0} y1={HEIGHT - 0.5} x2={WIDTH} y2={HEIGHT - 0.5} />
        </g>
        <path className="histogram__luma" d={areaPath(histogram.luma, max)} />
        <g className="histogram__rgb">
          {CHANNELS.map((channel) => (
            <g key={channel} className={`histogram__channel histogram__channel--${channel}`}>
              <path d={areaPath(histogram[channel], max)} />
              <path className="histogram__edge" d={linePath(histogram[channel], max)} />
            </g>
          ))}
        </g>
      </svg>
      <div className="histogram__labels" aria-hidden="true">
        <span>暗部</span>
        <span>中间调</span>
        <span>高光</span>
      </div>
    </div>
  );
}
