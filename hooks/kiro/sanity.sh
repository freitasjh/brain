#!/usr/bin/env bash
# KI-02b — sanidade dos estáticos kiro/steering (I2/I3). TDD-first: vermelho antes, verde depois.
# exe em brain-session.json é "brain" (PATH): o setup gera o mesmo documento com o exe
# resolvido; equivalência é funcional (2 hooks, mesmos triggers/eventos/payload id kiro-{event}-$$, sem --db).
# Uso: bash hooks/kiro/sanity.sh  →  exit 0 sse 3/3 asserts verdes.
set -u
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
KIRO="$ROOT/hooks/kiro"
STEER="$ROOT/steering/brain-learning.md"
SKILL="$ROOT/.agents/skills/brain/SKILL.md"
TRIGGERS="UserPromptSubmit Stop SessionStart AgentSpawn PreToolUse PostToolUse FileCreate FileSave FileDelete PreTaskExecution PostTaskExecution"
JSONS="brain-session.json brain-learning.json brain-prompt-optimizer.json"
TARGETS="$KIRO/brain-session.json $KIRO/brain-learning.json $KIRO/brain-prompt-optimizer.json $STEER"
pass=0; fail=0; warns=""

# A1 (I3): todo trigger ∈ lista fechada (espelha KIRO_TRIGGERS em setup.rs:908).
a1_ok=1
for j in $JSONS; do
  for t in $(python3 -c "import json;print(' '.join(h['trigger'] for h in json.load(open('$KIRO/$j'))['hooks']))"); do
    case " $TRIGGERS " in *" $t "*) ;; *) echo "A1 FAIL: $j trigger '$t' fora da lista"; a1_ok=0;; esac
  done
done
if [ "$a1_ok" = 1 ]; then pass=$((pass+1)); echo "A1 OK: triggers ∈ lista fechada"; else fail=$((fail+1)); fi

# A2 (I2): sem projeto hardcoded e sem path absoluto em agent-hook/steering.
a2_ok=1
if grep -rEnw "boleto|progaterp|mobile-erp|mobile_erp|atlas-ecm" $TARGETS 2>/dev/null; then
  echo "A2 FAIL: nome de projeto hardcoded"; a2_ok=0
fi
if grep -rEn "/home/|/tmp/brain-export|/etc/" $TARGETS 2>/dev/null; then
  echo "A2 FAIL: path absoluto em hook/steering"; a2_ok=0
fi
if [ "$a2_ok" = 1 ]; then pass=$((pass+1)); echo "A2 OK: sem hardcoded/absoluto"; else fail=$((fail+1)); fi

# A3: sem --db; agent-hook nunca instrui CLI direto (curadoria só via MCP).
a3_ok=1
if grep -rn -- "--db" $TARGETS 2>/dev/null; then echo "A3 FAIL: --db presente"; a3_ok=0; fi
for j in brain-learning.json brain-prompt-optimizer.json; do
  if python3 -c "import json,sys;sys.exit(0 if 'brain hook --event' in json.load(open('$KIRO/$j'))['hooks'][0]['action'].get('prompt','') else 1)" 2>/dev/null; then
    echo "A3 FAIL: $j instrui 'brain hook --event' no prompt agent"; a3_ok=0
  fi
done
if [ "$a3_ok" = 1 ]; then pass=$((pass+1)); echo "A3 OK: sem --db, agent sem CLI direto"; else fail=$((fail+1)); fi

# Warns (não bloqueiam): prompt >40 linhas; roteiro colado fora da fonte (I1 drift).
for j in $JSONS; do
  n=$(python3 -c "import json;print(json.load(open('$KIRO/$j'))['hooks'][0]['action'].get('prompt','').count(chr(10))+1)")
  [ "$n" -gt 40 ] && warns="$warns WARN: $j prompt com $n linhas (>40)\n"
done
drift=$(grep -rl "deepsearch" $TARGETS "$ROOT/.agents/rules/BRAIN.MCP.md" 2>/dev/null || true)
[ -n "$drift" ] && warns="$warns WARN: roteiro colado fora da skill: $drift\n"
src_n=$(grep -c "deepsearch" "$SKILL")
[ "$src_n" != 1 ] && warns="$warns WARN: fonte com $src_n ocorrências (esperado 1)\n"
[ -n "$warns" ] && printf "%b" "$warns"

echo "sanity: $pass/3 asserts verdes"
[ "$fail" = 0 ]
