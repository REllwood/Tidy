/** Decode the structured errors returned by native commands without losing their message. */
export function errorMessage(error: unknown): string {
  if (error instanceof Error)
    return error.message || "Something went wrong. Please try again.";
  if (typeof error === "string" && error.trim()) return error;
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string" &&
    error.message.trim()
  )
    return error.message;
  return "Something went wrong. Please try again.";
}
export class CommandError extends Error {
  readonly kind: string | undefined;
  constructor(error: unknown) {
    super(errorMessage(error));
    this.name = "CommandError";
    this.kind =
      error &&
      typeof error === "object" &&
      "kind" in error &&
      typeof error.kind === "string"
        ? error.kind
        : undefined;
  }
}
