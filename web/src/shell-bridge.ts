import type { Shell } from "../../web/pkg/wat_wasm.js";

type WatSideEffect =
  | { type: "redirect"; url: string; delay_ms?: number }
  | { type: "konami_celebrate" }
  | { type: "persist_vfs"; snapshot: string }
  | { type: "load_vfs" };

const OSC_RE = /\x1b\]9999;([^\x07]*)\x07/g;

export type SideEffectHandler = (effect: WatSideEffect) => void;

export class Bridge {
  private shell: Shell;
  private onSideEffect: SideEffectHandler;

  constructor(shell: Shell, onSideEffect: SideEffectHandler) {
    this.shell = shell;
    this.onSideEffect = onSideEffect;
  }

  prompt(): string {
    return this.shell.prompt();
  }

  /** Continuation prompt shown while a multi-line command is still open. */
  continuationPrompt(): string {
    return this.shell.continuation_prompt();
  }

  /** True if `input` is an unfinished multi-line command (keep buffering). */
  isIncomplete(input: string): boolean {
    return this.shell.is_incomplete(input);
  }

  /** Feed input and return the visible output (OSC sequences stripped and dispatched). */
  feed(input: string): string {
    const raw = this.shell.feed(input);
    return this.processOutput(raw);
  }

  complete(input: string, cursor: number): string[] {
    return this.shell.complete(input, cursor);
  }

  historyAt(index: number): string | undefined {
    return this.shell.history_at(index) ?? undefined;
  }

  private processOutput(raw: string): string {
    let visible = "";
    let lastIndex = 0;
    let match: RegExpExecArray | null;

    OSC_RE.lastIndex = 0;
    while ((match = OSC_RE.exec(raw)) !== null) {
      visible += raw.slice(lastIndex, match.index);
      lastIndex = match.index + match[0].length;
      this.dispatchSideEffect(match[1]);
    }
    visible += raw.slice(lastIndex);
    return visible;
  }

  private dispatchSideEffect(json: string): void {
    try {
      const effect = JSON.parse(json) as WatSideEffect;
      this.onSideEffect(effect);
    } catch {
      console.warn("[wat] unknown side-effect payload:", json);
    }
  }
}
