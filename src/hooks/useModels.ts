import { useCallback, useEffect, useState } from "react";
import { useMutation, useMutationState, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/tauri";
import { modelsApi, ollamaApi } from "@/lib/api";

const DOWNLOAD_KEY = ["whisper-download"] as const;

export function useModelDownloads() {
  return useMutationState({
    filters: { mutationKey: DOWNLOAD_KEY, status: "pending" },
    select: (m) => m.state.variables as string,
  });
}

const MODELS_KEY = ["models"] as const;

export function useModels() {
  return useQuery({ queryKey: MODELS_KEY, queryFn: modelsApi.list });
}

export function useOllamaStatus() {
  return useQuery({ queryKey: ["ollama-status"], queryFn: ollamaApi.status });
}

interface DownloadProgressEvent {
  id: string;
  downloaded: number;
  total: number;
}

/**
 * Track in-flight download progress (0–1) per model id, driven by core events.
 * A finished download clears itself, and `reset` drops a stale entry when a
 * download starts again or is cancelled.
 */
export function useDownloadProgress() {
  const [progress, setProgress] = useState<Record<string, number>>({});
  const reset = useCallback((id: string) => {
    setProgress((p) => {
      if (!(id in p)) return p;
      const next = { ...p };
      delete next[id];
      return next;
    });
  }, []);
  useEffect(() => {
    const apply = ({ id, downloaded, total }: DownloadProgressEvent) => {
      setProgress((p) => {
        // A completed download leaves nothing behind for the next attempt.
        if (total > 0 && downloaded >= total) {
          if (!(id in p)) return p;
          const next = { ...p };
          delete next[id];
          return next;
        }
        return { ...p, [id]: total ? downloaded / total : 0 };
      });
    };
    if (!isTauri()) {
      // Browser preview: the mock backend emits the same payload on `window`.
      const onWindowEvent = (e: Event) =>
        apply((e as CustomEvent<DownloadProgressEvent>).detail);
      window.addEventListener("model-download-progress", onWindowEvent);
      return () =>
        window.removeEventListener("model-download-progress", onWindowEvent);
    }
    let un: (() => void) | undefined;
    let disposed = false;
    listen<DownloadProgressEvent>("model-download-progress", (e) =>
      apply(e.payload),
    ).then((fn) => { if (disposed) fn(); else un = fn; });
    return () => { disposed = true; un?.(); };
  }, []);
  return { progress, reset };
}

export function useModelMutations() {
  const qc = useQueryClient();
  // Awaited so a mutation stays pending until the refreshed list has arrived,
  // which keeps the "Selected" badge honest after a delete reselects a model.
  const invalidate = () => qc.invalidateQueries({ queryKey: MODELS_KEY });
  return {
    download: useMutation({
      mutationKey: DOWNLOAD_KEY,
      mutationFn: (id: string) => modelsApi.download(id),
      onSettled: invalidate,
    }),
    cancelDownload: useMutation({
      mutationFn: (id: string) => modelsApi.cancelDownload(id),
    }),
    select: useMutation({
      mutationFn: (id: string) => modelsApi.select(id),
      onSettled: invalidate,
    }),
    remove: useMutation({
      mutationFn: (id: string) => modelsApi.remove(id),
      onSettled: invalidate,
    }),
  };
}
