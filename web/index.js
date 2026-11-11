const log = document.getElementById("log");
const cmd = document.getElementById("cmd");

const write = (text, kind) => {
  const line = document.createElement("div");
  line.textContent = text;
  if (kind) line.className = kind;
  log.appendChild(line);
  log.scrollTop = log.scrollHeight;
};

const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });

worker.onmessage = ({ data }) => {
  if (data.type === "ready") log.textContent = "";
  else if (data.type === "output") write(data.text.trimEnd());
  else if (data.type === "reset") write("engine reset.");
  else write(data.text, "err");
};

// A worker that fails to start never sends an `error` message itself.
worker.onerror = (event) => write(event.message ?? String(event), "err");

document.getElementById("prompt").onsubmit = (event) => {
  event.preventDefault();
  const line = cmd.value.trim();
  if (!line) return;
  write("> " + line);
  worker.postMessage({ type: "command", text: line });
  cmd.value = "";
};

document.getElementById("reset").onclick = () => {
  worker.postMessage({ type: "reset" });
};

worker.postMessage({ type: "command", text: "uci" });
