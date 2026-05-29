import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";

const term = new Terminal({
  fontFamily: '"Cascadia Code", "Fira Code", monospace',
  fontSize: 14,
  theme: {
    background: "#1a1a1a",
    foreground: "#d4d4d4",
    cursor: "#d4d4d4",
  },
  cursorBlink: true,
});

const fitAddon = new FitAddon();
term.loadAddon(fitAddon);
term.loadAddon(new WebLinksAddon());

const el = document.getElementById("terminal")!;
term.open(el);
fitAddon.fit();
window.addEventListener("resize", () => fitAddon.fit());

term.writeln("Loading wat...");

async function boot() {
  const { Shell } = await import("../pkg/wat_wasm.js");
  const { Bridge } = await import("./shell-bridge.js");

  const bridge = new Bridge(new Shell());

  let lineBuffer = "";

  function writePrompt() {
    term.write(bridge.prompt());
  }

  term.clear();
  writePrompt();

  term.onData((data) => {
    switch (data) {
      case "\r": {
        term.writeln("");
        const out = bridge.feed(lineBuffer);
        if (out) term.write(out);
        lineBuffer = "";
        writePrompt();
        break;
      }
      case "": {
        if (lineBuffer.length > 0) {
          lineBuffer = lineBuffer.slice(0, -1);
          term.write("\b \b");
        }
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
}

boot().catch((err) => {
  term.writeln(`\r\nFailed to load WASM: ${err}`);
});
