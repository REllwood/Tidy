export function MeetingAnswerText({
  text,
  sourceCount,
}: {
  text: string;
  sourceCount: number;
}) {
  const lines = text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const render = (line: string) =>
    line.split(/(\[\d+\])/g).map((part, i) => {
      const match = /^\[(\d+)\]$/.exec(part);
      const id = match ? Number(match[1]) : 0;
      return id > 0 && id <= sourceCount ? (
        <a
          key={i}
          href={`#meeting-source-${id}`}
          aria-label={`View source ${id}`}
          className="mx-1 inline-flex size-5 items-center justify-center rounded bg-brand-soft text-[11px] font-medium text-brand hover:underline"
        >
          {id}
        </a>
      ) : (
        part
      );
    });
  return (
    <div className="space-y-3 text-sm leading-7">
      {lines.map((line, i) =>
        line.startsWith("- ") ? (
          <div key={i} className="flex gap-3">
            <span
              aria-hidden
              className="mt-3 size-1 shrink-0 rounded-full bg-brand"
            />
            <p>{render(line.slice(2))}</p>
          </div>
        ) : (
          <p key={i}>{render(line)}</p>
        ),
      )}
    </div>
  );
}
