export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes < 1024) return `${Math.round(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(bytes < 10 * 1024 ? 1 : 0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(bytes < 10 * 1024 * 1024 ? 1 : 0)} MB`;
}

export function formatSigned(value: number, digits = 0): string {
  const rounded = Number(value.toFixed(digits));
  if (rounded > 0) return `+${rounded.toFixed(digits)}`;
  return rounded.toFixed(digits);
}

export function formatDimensions(width: number, height: number): string {
  return `${Math.round(width)} × ${Math.round(height)}`;
}

export function formatPercent(value: number, digits = 0): string {
  return `${value.toFixed(digits)}%`;
}

export function formatRelativeTime(iso: string, now: Date = new Date()): string {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return "刚刚";
  const diff = Math.max(0, now.getTime() - then.getTime());
  const minute = 60_000;
  const hour = 60 * minute;
  const day = 24 * hour;
  if (diff < minute) return "刚刚";
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < day) return `${Math.floor(diff / hour)} 小时前`;
  if (diff < 30 * day) return `${Math.floor(diff / day)} 天前`;
  return then.toLocaleDateString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit" });
}

export function formatZoom(percent: number): string {
  return `${Math.round(percent)}%`;
}

export function shortenPath(path: string, maxLength = 48): string {
  if (path.length <= maxLength) return path;
  const head = Math.ceil((maxLength - 1) / 2);
  const tail = Math.floor((maxLength - 1) / 2);
  return `${path.slice(0, head)}…${path.slice(-tail)}`;
}
