/** True only for exact loopback hosts, not names that merely start with them. */
export function isLoopbackProviderUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    if (url.protocol !== "http:" && url.protocol !== "https:") return false;
    const host = url.hostname.toLowerCase().replace(/^\[|\]$/g, "");
    if (host === "localhost" || host.endsWith(".localhost") || host === "::1") {
      return true;
    }
    const octets = host.split(".");
    return (
      octets.length === 4 &&
      octets[0] === "127" &&
      octets.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255)
    );
  } catch {
    return false;
  }
}
