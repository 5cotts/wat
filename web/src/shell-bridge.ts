import type { Shell } from "../../web/pkg/wat_wasm.js";

export class Bridge {
  private shell: Shell;

  constructor(shell: Shell) {
    this.shell = shell;
  }

  prompt(): string {
    return this.shell.prompt();
  }

  feed(input: string): string {
    return this.shell.feed(input);
  }

  complete(input: string, cursor: number): string[] {
    return this.shell.complete(input, cursor);
  }

  historyAt(index: number): string | undefined {
    return this.shell.history_at(index) ?? undefined;
  }
}
