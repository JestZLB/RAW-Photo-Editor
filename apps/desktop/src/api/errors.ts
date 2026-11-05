import type { ApiError } from "./types";

export const FALLBACK_ERROR_MESSAGE = "这一步没有完成，请稍后再试。";

export function apiError(code: string, message: string, userMessage: string): ApiError {
  return { code, message, userMessage };
}

export function isApiError(value: unknown): value is ApiError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.userMessage === "string"
  );
}

export function normalizeApiError(
  value: unknown,
  fallbackMessage: string = FALLBACK_ERROR_MESSAGE,
): ApiError {
  if (isApiError(value)) return value;
  if (typeof value === "string" && value.trim().length > 0) {
    return apiError("UNKNOWN", value, fallbackMessage);
  }
  if (value instanceof Error) {
    return apiError("UNKNOWN", value.message, fallbackMessage);
  }
  return apiError("UNKNOWN", "Unexpected error", fallbackMessage);
}

/** Returns a friendly, user-facing message without leaking stack traces. */
export function userMessageOf(value: unknown, fallbackMessage: string = FALLBACK_ERROR_MESSAGE): string {
  if (isApiError(value)) {
    return value.userMessage.trim() || fallbackMessage;
  }
  if (value instanceof Error && value.message) {
    return fallbackMessage;
  }
  return fallbackMessage;
}
