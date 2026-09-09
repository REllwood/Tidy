#!/usr/bin/env bash
# Build the pinned, self-contained inference sidecar. No model weights are bundled.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)
revision=427291b5b34cd914a31b3fd3b61a68f6184f4b9f
cache="${TIDY_AI_BUILD_CACHE:-$root/target/tidy-ai-runtime}/$revision"
mkdir -p "$cache" src-tauri/binaries src-tauri/resources
if [[ ! -f "$cache/source/CMakeLists.txt" ]]; then
  curl --fail --location --retry 3 "https://codeload.github.com/ggml-org/llama.cpp/tar.gz/$revision" --output "$cache/source.tar.gz"
  mkdir -p "$cache/source"
  tar -xzf "$cache/source.tar.gz" --strip-components=1 -C "$cache/source"
fi
cmake -S "$cache/source" -B "$cache/build" \
  -DLLAMA_BUILD_COMMIT="$revision" -DLLAMA_BUILD_NUMBER=0 -DCMAKE_BUILD_TYPE=Release -DCMAKE_OSX_DEPLOYMENT_TARGET=14.4 \
  -DBUILD_SHARED_LIBS=OFF -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON \
  -DGGML_NATIVE=OFF -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF -DLLAMA_BUILD_TESTS=OFF \
  -DLLAMA_BUILD_EXAMPLES=OFF -DLLAMA_BUILD_SERVER=ON -DLLAMA_BUILD_TOOLS=ON \
  -DLLAMA_BUILD_UI=OFF -DGGML_OPENMP=OFF
cmake --build "$cache/build" --config Release --target llama-server -j 4
triple=$(rustc -vV | awk '/host:/{print $2}')
cp "$cache/build/bin/llama-server" "src-tauri/binaries/tidy-ai-$triple"
cp "$cache/source/LICENSE" src-tauri/resources/llama-cpp-LICENSE.txt
chmod +x "src-tauri/binaries/tidy-ai-$triple"
# Fail if the runtime depends on libraries from the maintainer's machine.
if otool -L "src-tauri/binaries/tidy-ai-$triple" | tail -n +2 | awk '{print $1}' | grep -Ev '^(/System/Library/|/usr/lib/)' ; then
  echo 'Inference runtime has non-system dynamic dependencies.' >&2
  exit 1
fi
echo "Staged local AI runtime at revision $revision"
