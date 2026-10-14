// OC-01: plugin opencode só assina eventos verificados no runtime instalado.
//
// Método: extrai os call sites `trigger("...")` do binário opencode instalado
// (opencode 1.18.31) e exige que toda chave exportada pelo plugin esteja nesse
// conjunto. Hook em evento sem call site nunca dispara — silencioso morto
// (cf. regras/global/opencode-plugin-hooks-enforcement-reality).
//
// Run: node --test hooks/opencode/brain-session.test.mjs
//   OPENCODE_BIN override do binário (default ~/.local/.../opencode.exe)
//   BRAIN_BIN override do binário brain (default "brain" no PATH)

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { Plugin } from "./brain-session.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PLUGIN = path.join(HERE, "brain-session.js");
const OPENCODE_BIN =
  process.env.OPENCODE_BIN ||
  path.join(os.homedir(), ".local/lib/node_modules/opencode-ai/bin/opencode.exe");

// Extrai trigger("x") / trigger('x') do binário. Sem regex exótica: o runtime
// chama Plugin.trigger(W,...) e W é sempre literal nesses call sites.
function runtimeTriggers(binPath) {
  const buf = fs.readFileSync(binPath); // binário ~185MB, leitura direta
  const text = buf.toString("utf8");
  const out = new Set();
  for (const m of text.matchAll(/trigger\((["'])([^"']+)\1/g)) out.add(m[2]);
  return out;
}

function pluginKeys() {
  const src = fs.readFileSync(PLUGIN, "utf8");
  // chaves de hook de primeiro nível do objeto retornado: "name": ou 'name':
  const keys = new Set();
  for (const m of src.matchAll(/^\s*["']([^"']+)["']\s*:/gm)) keys.add(m[1]);
  return keys;
}

// Toda chave que o plugin exporta precisa existir no runtime. Lista fechada de
// mortos conhecidos: documentados ou plausíveis, mas sem call site em 1.18.31.
const KNOWN_DEAD = [
  "session.created",
  "session.idle",
  "message.updated",
  "message.part.updated",
  "permission.ask",
  "event",
  "config",
];

describe("OC-01 eventos verificados no runtime instalado", () => {
  it("binário opencode instalado expõe a lista de triggers reais", () => {
    assert.ok(fs.existsSync(OPENCODE_BIN), `sem runtime instalado em ${OPENCODE_BIN}`);
    const triggers = runtimeTriggers(OPENCODE_BIN);
    // 16 literais em 1.18.31; se a versão mudar, este número ancora a mudança.
    assert.ok(triggers.size >= 10, `triggers suspeitos: ${triggers.size}`);
    for (const must of [
      "tool.execute.before",
      "tool.execute.after",
      "experimental.chat.system.transform",
      "experimental.session.compacting",
      "chat.message",
    ])
      assert.ok(triggers.has(must), `runtime sem ${must}`);
    console.log(`    triggers reais (${triggers.size}): ${[...triggers].sort().join(", ")}`);
  });

  it("plugin só assina eventos com call site no runtime", () => {
    const triggers = runtimeTriggers(OPENCODE_BIN);
    const keys = pluginKeys();
    assert.ok(keys.size > 0, "nenhuma chave de hook no plugin");
    for (const k of keys) assert.ok(triggers.has(k), `hook morto: "${k}" sem trigger() no runtime`);
    console.log(`    hooks do plugin (${keys.size}): ${[...keys].sort().join(", ")}`);
  });

  it("plugin não contém nenhum morto conhecido", () => {
    const src = fs.readFileSync(PLUGIN, "utf8");
    const code = src
      .split("\n")
      .filter((l) => !l.trimStart().startsWith("//"))
      .join("\n"); // comentários não contam (classe code_only)
    for (const dead of KNOWN_DEAD)
      assert.ok(!code.includes(`"${dead}"`), `morto conhecido no plugin: ${dead}`);
  });
});

describe("OC-01 fail-open (MCP/binário fora)", () => {
  async function loadPlugin(fakeStdout, exits = {}) {
    const calls = [];
    const canned = fakeStdout;
    const $ = (strings, ...vals) => ({
      cwd: () => ({
        nothrow: () => ({
          text: async () => {
            calls.push(String.raw(strings, ...vals.map((v) => String(v))));
            if (exits.throw) throw new Error("spawn failed");
            return canned;
          },
        }),
      }),
    });
    const hooks = await Plugin({ client: {}, $, directory: "/tmp/ocprobe" });
    return { hooks, calls };
  }

  it("node --check: sintaxe válida", () => {
    execFileSync(process.execPath, ["--check", PLUGIN], { stdio: "pipe" });
  });

  it("system.transform injeta resumo no array (contrato output.system: string[])", async () => {
    const { hooks, calls } = await loadPlugin("## contexto brain\n- regra X");
    const output = { system: ["base"] };
    await hooks["experimental.chat.system.transform"]({ sessionID: "s1" }, output);
    assert.deepEqual(output.system, ["base", "## contexto brain\n- regra X"]);
    assert.equal(calls.length, 1, "uma chamada ao brain por sessão");
    // segunda chamada: sem re-spawn (flag once por sessão)
    await hooks["experimental.chat.system.transform"]({ sessionID: "s1" }, output);
    assert.equal(calls.length, 1, "re-spawn por turno = spam");
  });

  it("MCP/binário fora: sem injeção, sem throw", async () => {
    const { hooks } = await loadPlugin("", { throw: true });
    const output = { system: ["base"] };
    await hooks["experimental.chat.system.transform"]({ sessionID: "s1" }, output);
    assert.deepEqual(output.system, ["base"]);
  });

  it("compacting anexa a output.context e nunca sobrescreve output.prompt", async () => {
    const { hooks } = await loadPlugin("## handoff\n- feito A");
    const output = { context: [], prompt: undefined };
    await hooks["experimental.session.compacting"]({ sessionID: "s1" }, output);
    assert.equal(output.prompt, undefined, "prompt override apagaria o resumo default");
    assert.deepEqual(output.context, ["## handoff\n- feito A"]);
  });

  it("compacting vazio: preserva default (degradado documentado)", async () => {
    const { hooks } = await loadPlugin("   ");
    const output = { context: [], prompt: undefined };
    await hooks["experimental.session.compacting"]({ sessionID: "s1" }, output);
    assert.deepEqual(output.context, []);
  });

  it("chat.message registra pergunta do usuário uma vez; texto do assistente nunca vaza como pergunta", async () => {
    const { hooks, calls } = await loadPlugin("ctx");
    await hooks["chat.message"](
      { sessionID: "s1" },
      { message: { role: "user" }, parts: [{ type: "text", text: "como compilar?" }] },
    );
    assert.equal(calls.length, 1);
    assert.ok(calls[0].includes("como compilar?"), `pergunta ausente em: ${calls[0].slice(0, 120)}`);
    await hooks["chat.message"](
      { sessionID: "s1" },
      { message: { role: "assistant" }, parts: [{ type: "text", text: "SEGREDO-DO-MODELO" }] },
    );
    assert.equal(calls.length, 1, "segundo evento não re-spawna");
    assert.ok(!calls.join("\n").includes("SEGREDO-DO-MODELO"), "texto do assistente vazou");
  });
});
