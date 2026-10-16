// hitl-guardrail — human-in-the-loop guardrail (V2 native, @opencode/plugin)
// Intent preserved from V1 placeholder ({name, guard: () => ({ok: true})}):
// default-allow, fail-open. Registered as a permission evaluate hook that
// never denies by itself — real policy lives in opencode.json `permissions`.
// V2: Plugin.define({id, setup}) + ctx.permission.hook("evaluate").

import { Plugin } from "@opencode/plugin";

export function guard(_event) {
  return { ok: true };
}

export default Plugin.define({
  id: "hitl-guardrail",
  async setup(ctx) {
    await ctx.permission.hook("evaluate", (_event) => {
      // Default-allow: leave the decision to the ordered `permissions` array.
      // Fail-open — never throw here.
    });
  },
});
