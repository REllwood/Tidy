/** Export readable text without interpreting transcript markup as HTML. */
export function transcriptText(body: string): string {
  const blocks: unknown = JSON.parse(body);
  if (!Array.isArray(blocks))
    throw new Error("Saved transcript is not a document.");
  function text(value: unknown): string {
    if (typeof value === "string") return value;
    if (Array.isArray(value)) return value.map(text).join("");
    if (value && typeof value === "object" && "text" in value)
      return text(value.text);
    if (value && typeof value === "object" && "content" in value)
      return text(value.content);
    return "";
  }
  function lines(value: unknown): string[] {
    if (!Array.isArray(value)) return [];
    return value.flatMap((b) =>
      b && typeof b === "object" ? [text(b.content), ...lines(b.children)] : [],
    );
  }
  return lines(blocks).filter(Boolean).join("\n\n");
}
