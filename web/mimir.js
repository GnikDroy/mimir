// Host side of the raw ABI in `src/wasm.rs`.
//
// A wasm module that grows its memory detaches every existing view of
// it, so views are created after the allocation they describe and never
// reused across a call into wasm.

const ENCODER = new TextEncoder();
const DECODER = new TextDecoder();

const REQUIRED_EXPORTS = [
  "memory",
  "mimir_alloc",
  "mimir_dealloc",
  "mimir_command",
  "mimir_output_len",
  "mimir_output_copy",
  "mimir_reset",
];

async function instantiate(wasmUrl, imports) {
  const response = await fetch(wasmUrl);
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText} for ${wasmUrl}`);
  }

  try {
    // Cloned so the fallback can reuse the body instead of refetching.
    return await WebAssembly.instantiateStreaming(response.clone(), imports);
  } catch {
    // Streaming requires the server to send `application/wasm`.
    return await WebAssembly.instantiate(await response.arrayBuffer(), imports);
  }
}

export async function loadMimir(wasmUrl) {
  const imports = {
    env: {
      // wasm has no clock, and time management is useless without one.
      mimir_now_ms: () => performance.now(),
    },
  };

  const { instance } = await instantiate(wasmUrl, imports);
  const wasm = instance.exports;

  const missing = REQUIRED_EXPORTS.filter((name) => !(name in wasm));
  if (missing.length > 0) {
    throw new Error(`not a mimir module, missing: ${missing.join(", ")}`);
  }

  function withBuffer(len, use) {
    const ptr = wasm.mimir_alloc(len);
    if (ptr === 0) {
      throw new Error(`mimir_alloc(${len}) failed`);
    }
    try {
      return use(ptr);
    } finally {
      wasm.mimir_dealloc(ptr, len);
    }
  }

  function writeCommand(text) {
    const bytes = ENCODER.encode(text);
    if (bytes.length === 0) {
      return;
    }
    withBuffer(bytes.length, (ptr) => {
      new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
      wasm.mimir_command(ptr, bytes.length);
    });
  }

  function readOutput() {
    const len = wasm.mimir_output_len();
    if (len === 0) {
      return "";
    }
    return withBuffer(len, (ptr) => {
      wasm.mimir_output_copy(ptr);
      // Copied out before decoding so it survives any later growth.
      return DECODER.decode(new Uint8Array(wasm.memory.buffer, ptr, len).slice());
    });
  }

  return {
    send(command) {
      writeCommand(command);
      return readOutput();
    },
    reset: () => wasm.mimir_reset(),
  };
}
