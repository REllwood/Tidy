import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@/lib/tauri";
import { modelsApi, ollamaApi } from "@/lib/api";

const MODELS_KEY = ["models"] as const;

export function useModels() {
  return useQuery({ queryKey: MODELS_KEY, queryFn: modelsApi.list });
}

export function useOllamaStatus() {
  return useQuery({ queryKey: ["ollama-status"], queryFn: ollamaApi.status });
}

/** Track in-flight download progress (0–1) per model id, driven by core events. */
export function useDownloadProgress() {
  const [progress, setProgress] = useState<Record<string, number>>({});
  useEffect(() => {
    if (!isTauri()) return;
    let un: (() => void) | undefined;
    listen<{ id: string; downloaded: number; total: number }>(
      "model-download-progress",
      (e) => {
        const { id, downloaded, total } = e.payload;
        setProgress((p) => ({ ...p, [id]: total ? downloaded / total : 0 }));
      },
    ).then((fn) => (un = fn));
    return () => un?.();
  }, []);
  return progress;
}

export function useModelMutations() {
  const qc = useQueryClient();
  const invalidate = () => qc.invalidateQueries({ queryKey: MODELS_KEY });
  return {
    download: useMutation({
      mutationFn: (id: string) => modelsApi.download(id),
      onSettled: invalidate,
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
