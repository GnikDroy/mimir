import {
  Chessboard,
  COLOR,
  FEN,
  INPUT_EVENT_TYPE,
} from "https://cdn.jsdelivr.net/npm/cm-chessboard@8.15.4/src/Chessboard.js";
import {
  PromotionDialog,
  PROMOTION_DIALOG_RESULT_TYPE,
} from "https://cdn.jsdelivr.net/npm/cm-chessboard@8.15.4/src/extensions/promotion-dialog/PromotionDialog.js";

const ASSETS = "https://cdn.jsdelivr.net/npm/cm-chessboard@8.15.4/assets/";

const logEl = document.getElementById("log");
const turnEl = document.getElementById("turn");
const statusTextEl = document.getElementById("status-text");
const movetimeEl = document.getElementById("movetime");
const sideButtons = document.querySelectorAll("#side button");

function setStatus(text, side) {
  turnEl.className = side === undefined ? "" : `disc ${side}`;
  statusTextEl.textContent = text;
}

function write(text, kind) {
  const line = document.createElement("div");
  line.textContent = text;
  if (kind) line.className = kind;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

const worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });

const pending = new Map();
let nextId = 1;

function ask(command, echo = false) {
  return new Promise((resolve) => {
    const id = nextId++;
    pending.set(id, { resolve, echo });
    if (echo) {
      write(`> ${command}`, "sent");
    }
    worker.postMessage({ type: "command", text: command, id });
  });
}

worker.onmessage = ({ data }) => {
  if (data.type === "error") {
    pending.clear();
    write(data.text, "err");
    setStatus(data.text);
    return;
  }

  if (data.type === "output" && pending.has(data.id)) {
    const { resolve, echo } = pending.get(data.id);
    pending.delete(data.id);
    if (echo && data.text) {
      write(data.text.trimEnd());
    }
    resolve(data.text);
  }
};

worker.onerror = (event) => {
  pending.clear();
  const text = event.message ?? String(event);
  write(text, "err");
  setStatus(text);
};

const board = new Chessboard(document.getElementById("board"), {
  position: FEN.start,
  assetsUrl: ASSETS,
  style: { borderType: "none", cssClass: "black-and-white" },
  extensions: [{ class: PromotionDialog }],
});

let moves = [];
let legal = [];
let fen = FEN.start;
let human = COLOR.white;

// The engine is the only authority on the position.
async function sync() {
  const position = moves.length > 0 ? `startpos moves ${moves.join(" ")}` : "startpos";
  await ask(`position ${position}`, true);

  const shown = await ask("display");
  fen = shown.match(/^Fen: (.*)$/m)[1];
  await board.setPosition(fen, true);

  legal = (await ask("go perft 1"))
    .split("\n")
    .map((line) => line.match(/^([a-h][1-8][a-h][1-8][qrbn]?):/))
    .filter(Boolean)
    .map(([, move]) => move);

  return shown.match(/^Checkers:(.*)$/m)[1].trim();
}

async function step() {
  const checkers = await sync();
  const turn = fen.split(" ")[1];

  if (legal.length === 0) {
    board.disableMoveInput();
    if (checkers === "") {
      setStatus("stalemate · draw");
    } else {
      setStatus(turn === human ? "checkmate · you lose" : "checkmate · you win");
    }
    return;
  }

  if (Number(fen.split(" ")[4]) >= 100) {
    board.disableMoveInput();
    setStatus("draw · fifty-move rule");
    return;
  }

  if (turn === human) {
    setStatus(checkers === "" ? "your move" : "your move · check", human);
    board.enableMoveInput(inputHandler, human);
    return;
  }

  board.disableMoveInput();
  setStatus("thinking…", turn);

  const best = (await ask(`go movetime ${movetimeEl.value}`, true)).match(/^bestmove (\S+)/m);
  if (best === null || best[1] === "0000") {
    setStatus("the engine returned no move");
    return;
  }
  moves.push(best[1]);
  await step();
}

function inputHandler(event) {
  if (event.type !== INPUT_EVENT_TYPE.validateMoveInput) {
    return true;
  }

  const prefix = event.squareFrom + event.squareTo;
  const candidates = legal.filter((move) => move.startsWith(prefix));

  if (candidates.length === 0) {
    return false;
  }

  if (candidates.length === 1) {
    moves.push(candidates[0]);
    step();
    return true;
  }

  board.showPromotionDialog(event.squareTo, human, (result) => {
    if (result.type === PROMOTION_DIALOG_RESULT_TYPE.pieceSelected) {
      moves.push(prefix + result.piece.charAt(1));
      step();
    } else {
      board.setPosition(fen);
    }
  });
  return true;
}

async function newGame() {
  human = document.querySelector("#side button.active").dataset.side;
  moves = [];
  await ask("ucinewgame", true);
  await board.setOrientation(human);
  await step();
}

document.getElementById("new-game").onclick = () => newGame();

for (const button of sideButtons) {
  button.onclick = () => {
    for (const other of sideButtons) {
      other.classList.toggle("active", other === button);
    }
    newGame();
  };
}

await ask("uci", true);
newGame();
