// harness-validator — Rust version 0.6.0 Phase C
// Validates harness uses `cargo test --workspace` not mvn
export const harnessValidator = {
  name: "harness-validator",
  validate: (ctx) => {
    const cmd = ctx.command || "";
    if (cmd.includes("mvn ")) {
      return { ok: false, msg: "Use cargo test --workspace instead of mvn (Rust harness H1)" };
    }
    if (cmd.includes("cargo test")) return { ok: true };
    return { ok: true };
  }
};
// plugin entry: opencode checks for existence, actual validation is done via cargo test --workspace
console.log("[harness-validator] Rust harness: cargo test --workspace + cargo build --workspace + clippy, E2E 12-step brain");
