// OC-01 (V2): plugin brain-session em formato V2 nativo (@opencode/plugin).
//
// Ancora a API V2: Plugin.define({id, setup}) com hooks registrados via
// ctx.session.hook("prompt" | "context" | "compaction") — os destinos oficiais
// de chat.message, experimental.chat.system.transform e
// experimental.session.compacting
// (cf. https://opencode.ai/v2/docs/build/plugins/migrate-v1/).
// Nenhum export V1 (export const Plugin / return {"chat.message"...}),
// nenhum helper `$ Bun`, nenhuma chave experimental.* como hook.
//
// Run: node --test hooks/opencode/brain-session.test.mjs
//   BRAIN_BIN override do binário brain (default "brain" no PATH)

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import pluginDefault, { userQuestion, appendOnly } from "./brain-session.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PLUGIN = path.join(HERE, "brain-session.js");

// Mock do ctx V2: location.directory + session.hook(name, fn) que registra.
function mockCtx() {
  const registered = new Map();
  const ctx = {
    location: { directory: "/tmp/ocprobe" },
    session: {
      hook: async (name, fn) => {
        registered.set(name, fn);
        return { dispose: () => void registered.delete(name) };
      },
    },
    options: {},
  };
  return { ctx, registered };
}

// Binário brain canned: script shell que ecoa `text` (ou falha com exit 1).
// O plugin lê process.env.BRAIN_BIN a cada chamada (lazy), então basta
// apontar a env para o canned antes de disparar os hooks.
function cannedBin(text, { fail = false } = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "brainbin-"));
  const bin = path.join(dir, "brain");
  fs.writeFileSync(bin, fail ? "#!/bin/sh\nexit 1\n" : `#!/bin/sh\nprintf '%b' ${JSON.stringify(text)}\n`, {
    mode: 0o755,
  });
  return bin;
}

async function withBrainBin(bin, fn) {
  const prev = process.env.BRAIN_BIN;
  process.env.BRAIN_BIN = bin;
  try {
    return await fn();
  } finally {
    if (prev === undefined) delete process.env.BRAIN_BIN;
    else process.env.BRAIN_BIN = prev;
  }
}

describe("OC-01 V2 shape (sem sintaxe V1)", () => {
  it("node --check: sintaxe válida", () => {
    execFileSync(process.execPath, ["--check", PLUGIN], { stdio: "pipe" });
  });

  it("default export é definição V2 com id estável", () => {
    assert.ok(pluginDefault, "sem default export");
    assert.equal(pluginDefault.id, "brain-session");
    assert.equal(typeof pluginDefault.setup, "function", "setup(ctx) ausente");
  });

  it("fonte não contém export V1, $ helper nem chaves de hook V1", () => {
    const src = fs.readFileSync(PLUGIN, "utf8");
    const code = src
      .split("\n")
      .filter((l) => !l.trimStart().startsWith("//"))
      .join("\n"); // comentários não contam
    assert.ok(!code.includes("export const Plugin"), "export V1 presente");
    assert.ok(!code.includes('"chat.message"'), "chave V1 chat.message presente");
    assert.ok(!code.includes("experimental.chat.system.transform"), "chave V1 system.transform presente");
    assert.ok(!code.includes("experimental.session.compacting"), "chave V1 compacting presente");
    assert.ok(!code.includes("tool.execute.before"), "chave V1 tool.execute.before presente");
    assert.ok(!code.includes("chat.message"), "qualquer chat.message presente");
    assert.ok(!code.includes("experimental.session"), "qualquer experimental.session presente");
    assert.ok(!code.includes('from "@opencode-ai/plugin"'), "import V1 presente");
    assert.ok(!code.includes("`$`") && !code.match(/\b\$\{/), "helper $ presente");
    assert.ok(code.includes("opencode/plugin"), "import @opencode/plugin ausente");
    assert.ok(code.includes("Plugin.define"), "Plugin.define ausente");
  });

  it("setup registra prompt + context + compaction e nada mais", async () => {
    const { ctx, registered } = mockCtx();
    await pluginDefault.setup(ctx);
    assert.deepEqual([...registered.keys()].sort(), ["compaction", "context", "prompt"]);
  });
});

describe("OC-01 comportamento (fail-open, append-only)", () => {
  it("context injeta resumo (objeto[] V2) uma vez por sessão", async () => {
    await withBrainBin(cannedBin("## contexto brain\n- regra X"), async () => {
      const { ctx, registered } = mockCtx();
      await pluginDefault.setup(ctx);
      const system = [{ type: "text", text: "base" }];
      await registered.get("context")({ system });
      assert.deepEqual(system, [
        { type: "text", text: "base" },
        { type: "text", text: "## contexto brain\n- regra X" },
      ]);
      await registered.get("context")({ system });
      assert.equal(system.length, 2, "re-spawn por turno = spam");
    });
  });

  it("prompt registra pergunta do usuário uma vez; assistente nunca vaza", async () => {
    const bin = cannedBin("ctx");
    await withBrainBin(bin, async () => {
      const { ctx, registered } = mockCtx();
      // Espiona argv do hook via wrapper: re-executa com script que loga args.
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "brainargs-"));
      const spy = path.join(dir, "brain");
      const logf = path.join(dir, "args.log");
      fs.writeFileSync(spy, `#!/bin/sh\necho "$@" > ${JSON.stringify(logf)}\nprintf 'ctx'`, { mode: 0o755 });
      process.env.BRAIN_BIN = spy;
      await pluginDefault.setup(ctx);
      await registered.get("prompt")({
        message: { role: "user" },
        parts: [{ type: "text", text: "como compilar?" }],
      });
      const logged = fs.readFileSync(logf, "utf8");
      assert.ok(logged.includes("como compilar?"), `pergunta ausente em argv: ${logged.slice(0, 120)}`);
      // Segundo evento (assistente) não re-spawna: apaga o log e verifica que nada novo chega.
      fs.writeFileSync(logf, "VAZIO");
      await registered.get("prompt")({
        message: { role: "assistant" },
        parts: [{ type: "text", text: "SEGREDO-DO-MODELO" }],
      });
      assert.equal(fs.readFileSync(logf, "utf8"), "VAZIO", "segundo evento re-spawnou");
    });
  });

  it("appendOnly suporta string[] (legado) e objeto[] (V2)", () => {
    const legacy = ["base"];
    appendOnly(legacy, "novo");
    assert.deepEqual(legacy, ["base", "novo"]);
    const v2 = [{ type: "text", text: "base" }];
    appendOnly(v2, "novo");
    assert.deepEqual(v2, [{ type: "text", text: "base" }, { type: "text", text: "novo" }]);
    const empty = ["base"];
    appendOnly(empty, "   ");
    assert.deepEqual(empty, ["base"], "texto vazio não injeta");
  });

  it("userQuestion: só role=user; texto do assistente nunca vaza", () => {
    assert.equal(
      userQuestion({ message: { role: "user" }, parts: [{ type: "text", text: "como compilar?" }] }),
      "como compilar?",
    );
    assert.equal(
      userQuestion({ message: { role: "assistant" }, parts: [{ type: "text", text: "SEGREDO" }] }),
      "",
      "texto do assistente tratado como pergunta",
    );
    assert.equal(userQuestion({ message: { role: "user" }, parts: [] }), "");
  });

  it("compaction anexa a context e nunca sobrescreve prompt", async () => {
    await withBrainBin(cannedBin("## handoff\n- feito A"), async () => {
      const { ctx, registered } = mockCtx();
      await pluginDefault.setup(ctx);
      const event = { context: [], prompt: "RESUMO-DEFAULT" };
      await registered.get("compaction")(event);
      assert.equal(event.prompt, "RESUMO-DEFAULT", "prompt override apagaria o resumo default");
      assert.deepEqual(event.context, ["## handoff\n- feito A"]);
    });
  });

  it("compaction vazia: preserva default (degradado documentado)", async () => {
    await withBrainBin(cannedBin("   "), async () => {
      const { ctx, registered } = mockCtx();
      await pluginDefault.setup(ctx);
      const event = { context: [], prompt: undefined };
      await registered.get("compaction")(event);
      assert.deepEqual(event.context, []);
    });
  });

  it("MCP/binário fora: sem injeção, sem throw", async () => {
    await withBrainBin("/nonexistent-dir-oc01/brain-missing", async () => {
      const { ctx, registered } = mockCtx();
      await pluginDefault.setup(ctx);
      const system = [{ type: "text", text: "base" }];
      await registered.get("context")({ system });
      assert.deepEqual(system, [{ type: "text", text: "base" }]);
      const event = { context: [], prompt: undefined };
      await registered.get("compaction")(event);
      assert.deepEqual(event.context, []);
    });
  });
});
