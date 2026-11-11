// Runs the engine off the main thread.
//
// `stop` cannot interrupt a search. Always use `go movetime`/`depth`/`nodes`.
// Search is synchronous inside the module so, no meesage reaches it until the search ends.

import { loadMimir } from "./mimir.js";

// Started here, not lazily in the handler: the handler is async, so a
// null check inside it would let concurrent messages each load a module.
const loading = loadMimir(new URL("./mimir.wasm", import.meta.url));

loading.then(
  () => self.postMessage({ type: "ready" }),
  (error) => self.postMessage({ type: "error", text: String(error) }),
);

self.onmessage = async ({ data }) => {
  try {
    const engine = await loading;

    switch (data.type) {
      case "command": {
        const output = engine.send(String(data.text));
        if (output.length > 0) {
          self.postMessage({ type: "output", text: output });
        }
        break;
      }
      case "reset":
        engine.reset();
        self.postMessage({ type: "reset" });
        break;
    }
  } catch (error) {
    self.postMessage({ type: "error", text: String(error) });
  }
};
