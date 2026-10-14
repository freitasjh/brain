#!/usr/bin/env bash
# SU-03 — teste morto-vivo do hooks/brain-hook.sh (RF5/C2). TDD-first.
# Hermético: tmp/scratch, binário fake, backend morto; nunca produção 8321/8322.
# Uso: bash hooks/brain-hook.test.sh  →  exit 0 sse hunts verdes.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HOOK="$ROOT/hooks/brain-hook.sh"
pass=0; fail=0
ok() { pass=$((pass+1)); echo "T$1 OK: $2"; }
no() { fail=$((fail+1)); echo "T$1 FAIL: $2"; }

# bash -n nos dois (sintaxe)
if bash -n "$HOOK"; then ok "0" "bash -n limpo"; else no "0" "bash -n"; fi
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck -S warning "$HOOK"; then ok "0b" "shellcheck limpo"; else no "0b" "shellcheck"; fi
else
  echo "T0b SKIP: shellcheck ausente neste ambiente (bash -n vale como sintaxe; revisar no CI com shellcheck)"
fi

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
export XDG_RUNTIME_DIR="$T/run"
mkdir -p "$XDG_RUNTIME_DIR"
LOG="$T/calls.log"
touch "$LOG"

# Fake `brain`: registra argv, simula backend vivo.
mkdir -p "$T/bin"
cat > "$T/bin/brain" <<EOF
#!/usr/bin/env bash
echo "\$*" >> "$LOG"
exit 0
EOF
chmod +x "$T/bin/brain"

# T1: 4 eventos via binário (sem `uv run`), exit 0 cada.
t1_ok=1
for ev in session-start tool-result session-end prompt-submit; do
  if ! BRAIN_BIN="$T/bin/brain" BRAIN_HOOK_THROTTLE_SECS=0 bash "$HOOK" "$ev" </dev/null >/dev/null 2>&1; then
    echo "T1 FAIL: evento $ev exit!=0"; t1_ok=0
  fi
done
# Código apenas: o comentário que explica o porquê cita `uv run` de propósito
# (mesma classe do code_only do plugin opencode — assert é sobre comportamento).
if grep -v '^[[:space:]]*#' "$HOOK" | grep -q "uv run"; then echo "T1 FAIL: código ainda contém 'uv run'"; t1_ok=0; fi
for ev in session-start tool-result session-end; do
  if ! grep -q "hook --event $ev" "$LOG"; then echo "T1 FAIL: sem chamada hook --event $ev"; t1_ok=0; fi
done
if [ "$t1_ok" = 1 ]; then ok "1" "4 eventos via binário, exit 0"; else no "1" "4 eventos via binário"; fi

# T2: backend morto → warn em stderr + exit 0 (fail-open), nos 3 eventos.
: > "$LOG"
t2_ok=1; warnhit=0
for ev in session-start tool-result session-end; do
  err=$(BRAIN_BIN="$T/nao-existe-brain" bash "$HOOK" "$ev" </dev/null 2>"$T/err.txt" >"$T/out.txt"); code=$?
  if [ "$code" != 0 ]; then echo "T2 FAIL: $ev com backend morto saiu $code"; t2_ok=0; fi
  if grep -qi "warn\|indisponível\|sem bloquear\|seguindo" "$T/err.txt"; then warnhit=$((warnhit+1)); fi
done
[ "$warnhit" -ge 1 ] || { echo "T2 FAIL: nenhum warn em stderr"; t2_ok=0; }
if [ "$t2_ok" = 1 ]; then ok "2" "backend morto: warn + exit 0 (fail-open)"; else no "2" "fail-open"; fi

# T3: rajada tool-result agrega (throttle): 3 rápidas → 1 invocação ao binário.
: > "$LOG"
rm -rf "$XDG_RUNTIME_DIR/brain"
for i in 1 2 3; do
  echo "{\"tool\":\"t$i\",\"result\":\"r$i\"}" | BRAIN_BIN="$T/bin/brain" BRAIN_HOOK_THROTTLE_SECS=3600 bash "$HOOK" tool-result --stdin >/dev/null 2>&1
done
n=$(grep -c "hook --event tool-result" "$LOG" || true)
if [ "$n" = 1 ]; then ok "3" "rajada 3 tool-result → 1 nota agregada"; else no "3" "rajada gerou $n chamadas (esperado 1)"; fi

# T4: truncamento — payload gigante não vaza integral ao binário.
: > "$LOG"
python3 -c "print('{\"tool\":\"big\",\"result\":\"' + 'A'*20000 + '\"}')" | BRAIN_BIN="$T/bin/brain" BRAIN_HOOK_THROTTLE_SECS=0 bash "$HOOK" tool-result --stdin >/dev/null 2>&1 || true
if grep -q "hook --event tool-result" "$LOG"; then
  biggest=$(awk '{ print length }' "$LOG" | sort -n | tail -1)
  if [ "$biggest" -lt 20000 ]; then ok "4" "payload truncado (maior linha $biggest < 20000)"; else no "4" "sem truncamento (linha $biggest)"; fi
else
  no "4" "tool-result gigante não chegou ao binário"
fi

# T5: colisão mesmo segundo — session-start + alias prompt-submit geram ids
# distintos e ambos chegam ao binário (dedup por id não descarta a pergunta).
: > "$LOG"
BRAIN_BIN="$T/bin/brain" BRAIN_HOOK_THROTTLE_SECS=0 bash "$HOOK" session-start </dev/null >/dev/null 2>&1
BRAIN_BIN="$T/bin/brain" BRAIN_HOOK_THROTTLE_SECS=0 bash "$HOOK" prompt-submit </dev/null >/dev/null 2>&1
ids=$(grep -o 'sh-[^" ]*' "$LOG" | sort -u)
n_ids=$(printf '%s\n' "$ids" | grep -c . || true)
n_calls=$(grep -c "hook --event session-start" "$LOG" || true)
if [ "$n_calls" = 2 ] && [ "$n_ids" = 2 ]; then ok "5" "session-start + prompt-submit: 2 ids distintos, ambos gravados"; else no "5" "colisão: calls=$n_calls ids_unicos=$n_ids (esperado 2/2)"; fi

# T5b: mesma invocação shell (mesmo $$, mesmo segundo) — new_id nunca repete.
eval "$(awk '/^new_id\(\)/{f=1} f{print} f&&/^\}/{exit}' "$HOOK")"
a=$(new_id session-start); b=$(new_id session-start)
if [ -n "$a" ] && [ "$a" != "$b" ]; then ok "5b" "new_id único por invocação ($a != $b)"; else no "5b" "new_id repetiu: '$a' vs '$b'"; fi

echo "hook-test: $pass passed, $fail failed"
[ "$fail" = 0 ]
