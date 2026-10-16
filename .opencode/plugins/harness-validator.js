// harness-validator — Rust harness H1 (V2 native, @opencode/plugin)
// Blocks Maven commands; the Rust harness is `cargo test --workspace`.
// V1: exported {name, validate(ctx)} object (does not run in V2).
// V2: Plugin.define({id, setup}) + ctx.tool.hook("execute.before").

import { Plugin } from "@opencode/plugin";

function commandText(event) {
  try {
    const parts = [];
    if (typeof event?.input?.command === "string") parts.push(event.input.command);
    if (typeof event?.input?.cmd === "string") parts.push(event.input.cmd);
    if (Array.isArray(event?.input?.command)) parts.push(event.input.command.join(" "));
    if (Array.isArray(event?.input?.cmd)) parts.push(event.input.cmd.join(" "));
    if (typeof event?.input?.script === "string") parts.push(event.input.script);
    // Fallback: serialize — cheap substring check, no parsing.
    parts.push(JSON.stringify(event?.input ?? ""));
    return parts.join("\n");
  } catch {
    return "";
  }
}

export function validateCommand(cmd) {
  const text = String(cmd ?? "");
  if (/(^|[\s;&|])mvn(\s|$)/.test(text)) {
    return { ok: false, msg: "Use cargo test --workspace instead of mvn (Rust harness H1)" };
  }
  return { ok: true };
}

export default Plugin.define({
  id: "harness-validator",
  async setup(ctx) {
    await ctx.tool.hook("execute.before", (event) => {
      const tool = String(event?.tool ?? "");
      if (tool !== "shell" && tool !== "bash" && tool !== "execute" && tool !== "") return;
      const verdict = validateCommand(commandText(event));
      if (!verdict.ok) throw new Error(verdict.msg);
    });
  },
});
