import { describe, expect, it } from "vitest";
import { isLoopbackProviderUrl } from "./providerUrl";

describe("provider URL loopback detection", () => {
  it("accepts exact localhost and 127/8 hosts", () => {
    expect(isLoopbackProviderUrl("http://localhost:8080/v1")).toBe(true);
    expect(isLoopbackProviderUrl("http://127.0.0.1:11434/v1")).toBe(true);
    expect(isLoopbackProviderUrl("http://127.0.0.2/v1")).toBe(true);
    expect(isLoopbackProviderUrl("http://[::1]:8080/v1")).toBe(true);
    expect(isLoopbackProviderUrl("HTTPS://LOCALHOST/v1")).toBe(true);
  });

  it("rejects lookalike and remote hosts", () => {
    expect(isLoopbackProviderUrl("https://localhost.evil.com/v1")).toBe(false);
    expect(isLoopbackProviderUrl("https://127.0.0.1.evil.com/v1")).toBe(false);
    expect(isLoopbackProviderUrl("https://api.openai.com/v1")).toBe(false);
    expect(isLoopbackProviderUrl("ftp://localhost/v1")).toBe(false);
    expect(isLoopbackProviderUrl("not a url")).toBe(false);
  });
});
