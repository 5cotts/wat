import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { Bridge } from "./shell-bridge.js";

// ── Terminal setup ───────────────────────────────────────────────────────────

const term = new Terminal({
  fontFamily: '"Cascadia Code", "Fira Code", "Menlo", monospace',
  fontSize: 14,
  theme: {
    background: "#1a1a1a",
    foreground: "#d4d4d4",
    cursor: "#d4d4d4",
    black: "#1a1a1a",
    brightBlack: "#555",
    red: "#cd3131",
    green: "#0dbc79",
    yellow: "#e5e510",
    blue: "#2472c8",
    magenta: "#bc3fbc",
    cyan: "#11a8cd",
    white: "#e5e5e5",
    brightWhite: "#ffffff",
  },
  cursorBlink: true,
  scrollback: 2000,
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.loadAddon(new WebLinksAddon());

const el = document.getElementById("terminal")!;
const loadingEl = document.getElementById("loading")!;

term.open(el);
fitAddon.fit();
window.addEventListener("resize", () => fitAddon.fit());

// Show a static prompt before WASM loads (lazy-load on first interaction)
const STATIC_PROMPT = "5cotts@zo ~ % ";
term.write(STATIC_PROMPT);

// ── Konami code detector ─────────────────────────────────────────────────────

const KONAMI = [
  "ArrowUp", "ArrowUp", "ArrowDown", "ArrowDown",
  "ArrowLeft", "ArrowRight", "ArrowLeft", "ArrowRight",
  "b", "a",
];
let konamiIdx = 0;

function checkKonami(key: string): boolean {
  if (key === KONAMI[konamiIdx]) {
    konamiIdx++;
    if (konamiIdx === KONAMI.length) {
      konamiIdx = 0;
      return true;
    }
  } else {
    konamiIdx = key === KONAMI[0] ? 1 : 0;
  }
  return false;
}

// ── Boot WASM ────────────────────────────────────────────────────────────────

let bridge: Bridge | null = null;
let lineBuffer = "";
let historyIndex = -1;
let loadStarted = false;

async function loadWasm() {
  if (loadStarted) return;
  loadStarted = true;

  // Clear static prompt so the real one appears after load
  term.write("\r" + " ".repeat(STATIC_PROMPT.length) + "\r");

  loadingEl.style.display = "flex";

  const { Shell } = await import("../pkg/wat_wasm.js");
  const { Bridge: BridgeClass } = await import("./shell-bridge.js");

  bridge = new BridgeClass(new Shell(), handleSideEffect);

  loadingEl.classList.add("hidden");
  setTimeout(() => loadingEl.remove(), 400);

  term.clear();
  term.write(bridge.prompt());
}

function handleSideEffect(effect: { type: string; url?: string; delay_ms?: number }) {
  switch (effect.type) {
    case "redirect": {
      const delay = effect.delay_ms ?? 0;
      setTimeout(() => {
        if (effect.url) window.location.href = effect.url;
      }, delay);
      break;
    }
    case "konami_celebrate": {
      document.body.classList.add("konami-celebrate");
      term.write("\r\n\x1b[1;35m✦ ✦ ✦  KONAMI!  ✦ ✦ ✦\x1b[0m\r\n");
      setTimeout(() => document.body.classList.remove("konami-celebrate"), 2000);
      break;
    }
    case "persist_vfs": {
      try {
        localStorage.setItem("wat_vfs_snapshot", (effect as { snapshot?: string }).snapshot ?? "");
      } catch { /* quota exceeded or private browsing */ }
      break;
    }
    default:
      console.warn("[wat] unknown side-effect:", effect);
  }
}

function writePrompt() {
  if (bridge) term.write(bridge.prompt());
}

// ── Tab completion ────────────────────────────────────────────────────────────

function handleTab() {
  if (!bridge) return;
  const completions = bridge.complete(lineBuffer, lineBuffer.length);
  if (completions.length === 1) {
    const completion = completions[0];
    // Find the common prefix with the current buffer suffix
    const lastSpace = lineBuffer.lastIndexOf(" ");
    const prefix = lastSpace >= 0 ? lineBuffer.slice(0, lastSpace + 1) : "";
    const toComplete = completion.slice(prefix.length === 0
      ? lineBuffer.length > 0
        ? lineBuffer.indexOf("/") >= 0
          ? lineBuffer.lastIndexOf("/") + 1
          : 0
        : 0
      : lastSpace + 1);
    // Overwrite current suffix
    const currentSuffix = lastSpace >= 0 ? lineBuffer.slice(lastSpace + 1) : lineBuffer;
    const extra = completion.slice(currentSuffix.length > 0
      ? completion.lastIndexOf(currentSuffix) + currentSuffix.length
      : (lastSpace >= 0 ? lastSpace + 1 : 0));
    if (extra) {
      term.write(extra);
      lineBuffer += extra;
    }
  } else if (completions.length > 1) {
    term.writeln("");
    term.writeln(completions.join("  "));
    writePrompt();
    term.write(lineBuffer);
  }
}

// ── Key handling ─────────────────────────────────────────────────────────────

// Capture raw key events for Konami code before xterm gets them
document.addEventListener("keydown", (e) => {
  if (checkKonami(e.key) && bridge) {
    bridge.feed("__konami__");
    // konami_celebrate side effect will fire
  }
});

term.onData((data) => {
  // If WASM not loaded yet, trigger load on first keystroke
  if (!bridge) {
    loadWasm();
    return;
  }

  switch (data) {
    case "\r": {
      // Enter
      term.write("\r\n");
      const out = bridge.feed(lineBuffer);
      if (out) {
        // Convert \n to \r\n for xterm
        term.write(out.replace(/\n/g, "\r\n"));
      }
      lineBuffer = "";
      historyIndex = -1;
      writePrompt();
      break;
    }
    case "\x7f":
    case "\b": {
      // Backspace
      if (lineBuffer.length > 0) {
        lineBuffer = lineBuffer.slice(0, -1);
        term.write("\b \b");
      }
      break;
    }
    case "\t": {
      handleTab();
      break;
    }
    case "\x0c": {
      // Ctrl+L — clear screen
      term.clear();
      writePrompt();
      term.write(lineBuffer);
      break;
    }
    case "\x1b[A": {
      // Up arrow — history prev
      const next = historyIndex + 1;
      const entry = bridge.historyAt(next);
      if (entry !== undefined) {
        // Clear current line buffer on screen
        term.write("\b \b".repeat(lineBuffer.length));
        lineBuffer = entry;
        historyIndex = next;
        term.write(lineBuffer);
      }
      break;
    }
    case "\x1b[B": {
      // Down arrow — history next
      if (historyIndex > 0) {
        const next = historyIndex - 1;
        const entry = bridge.historyAt(next);
        term.write("\b \b".repeat(lineBuffer.length));
        lineBuffer = entry ?? "";
        historyIndex = next;
        term.write(lineBuffer);
      } else if (historyIndex === 0) {
        term.write("\b \b".repeat(lineBuffer.length));
        lineBuffer = "";
        historyIndex = -1;
      }
      break;
    }
    case "\x03": {
      // Ctrl+C
      term.write("^C\r\n");
      lineBuffer = "";
      historyIndex = -1;
      writePrompt();
      break;
    }
    default: {
      if (data >= " " || data === "\t") {
        lineBuffer += data;
        term.write(data);
      }
    }
  }
});
