import { useEffect, useRef } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { isTauri } from "@/lib/tauri";
import { usePlayback } from "./playback";

/**
 * The meeting's audio, mixed to mono so both sides sound right in
 * headphones. Note times and transcript stamps jump it to their moment.
 */
export function MeetingAudioPlayer({ pageId }: { pageId: string }) {
  const audio = useRef<HTMLAudioElement>(null);
  const request = usePlayback((s) => s.request);
  useEffect(() => {
    const player = audio.current;
    if (!player || !request || request.pageId !== pageId) return;
    player.currentTime = request.seconds;
    void player.play().catch(() => undefined);
  }, [request, pageId]);
  if (!isTauri()) return null;
  return (
    <div className="mt-4">
      <audio
        ref={audio}
        controls
        preload="metadata"
        src={convertFileSrc(pageId, "tidy-audio")}
        className="h-9 w-full"
        aria-label="Meeting audio"
      >
        This meeting's audio cannot be played here.
      </audio>
    </div>
  );
}
