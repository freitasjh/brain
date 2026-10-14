#!/usr/bin/env bash
# brain-hook.sh — envoltório fino sobre o binário `brain hook` (SU-03, RF5/C2).
#
# Eventos: session-start | tool-result | session-end | prompt-submit
# `prompt-submit` é alias de `session-start` com a pergunta extraída
# (o binário só conhece os 3 primeiros; ver `brain hook --help`).
#
# Contrato:
# - Chama o BINÁRIO (`BRAIN_BIN`, default `brain` no PATH). Nunca `uv run`
#   (C2: `uv run` inexistente no desktop = hook silenciosamente morto).
# - Fail-open: backend fora = warn em stderr + exit 0. Só uso inválido sai != 0.
# - Truncamento: pergunta/payload cortados em TRUNC_BYTES com aviso inline.
# - Throttle: tool-result em rajada agrega — 1 chamada por janela
#   (BRAIN_HOOK_THROTTLE_SECS, default 60) em vez de 1 nota por tool.
# - Zero segredo: nenhuma env com credencial é impressa ou repassada.
#
# Hermético em teste: XDG_RUNTIME_DIR + BRAIN_BIN + BRAIN_HOOK_THROTTLE_SECS
# (ver hooks/brain-hook.test.sh). Nada aqui fala com 8321/8322 diretamente —
# é o binário que resolve o backend.

set -u

BRAIN_BIN="${BRAIN_BIN:-brain}"
THROTTLE="${BRAIN_HOOK_THROTTLE_SECS:-60}"
TRUNC_BYTES=4000

warn() { printf 'brain-hook: warn: %s\n' "$*" >&2; }

spool_dir() {
  base="${XDG_RUNTIME_DIR:-/tmp}"
  printf '%s/brain' "$base"
}

# id único por invocação: evento + pid + nanos + seq (sem $RANDOM: dash-compat).
# date +%s%N onde há GNU date; senão epoch em segundos. A seq é em arquivo
# porque $(new_id) roda em subshell — incremento em variável se perde entre
# chamadas — e o arquivo também desempata processos distintos. Duas chamadas
# no mesmo segundo e no mesmo pid — ex. session-start + alias prompt-submit —
# nunca colidem.
new_id() {
  ns=$(date +%s%N 2>/dev/null || true)
  case "$ns" in
    ''|*[!0-9]*) ns="$(date +%s)000000000" ;;
  esac
  seqfile="${XDG_RUNTIME_DIR:-/tmp}/brain/hook-seq"
  mkdir -p "${seqfile%/*}" 2>/dev/null || true
  seq=$(cat "$seqfile" 2>/dev/null || echo 0)
  case "$seq" in ''|*[!0-9]*) seq=0 ;; esac
  seq=$((seq + 1))
  printf '%s' "$seq" > "$seqfile" 2>/dev/null || true
  printf 'sh-%s-%s-%s-%s' "$1" "$$" "$ns" "$seq"
}

# Trunca texto em TRUNC_BYTES com aviso inline; nunca quebra o pipe.
trunc() {
  txt="$1"
  n=$(printf '%s' "$txt" | wc -c | tr -d ' ')
  case "$n" in '' | *[!0-9]*) n=0 ;; esac
  if [ "$n" -gt "$TRUNC_BYTES" ]; then
    printf '%s\n\n[truncated by brain-hook.sh: %s bytes > %s limit]\n' \
      "$(printf '%s' "$txt" | head -c "$TRUNC_BYTES")" "$n" "$TRUNC_BYTES"
  else
    printf '%s' "$txt"
  fi
}

# Pergunta: --stdin JSON (.prompt/.question/.text) ou $USER_PROMPT (Kiro) ou "".
read_question() {
  q=""
  if [ "${1:-}" = "--stdin" ]; then
    input=$(cat /dev/stdin 2>/dev/null || true)
    if command -v jq >/dev/null 2>&1 && printf '%s' "$input" | jq -e . >/dev/null 2>&1; then
      q=$(printf '%s' "$input" | jq -r '.prompt // .question // .text // ""' 2>/dev/null || true)
      if [ "$q" = "null" ]; then q=""; fi
    else
      q=$(printf '%s' "$input" | head -c 1000)
    fi
  fi
  if [ -z "$q" ]; then q="${USER_PROMPT:-}"; fi
  trunc "$q"
}

# Chamada fail-open: stdout do binário passa (é o mecanismo de injeção no
# Kiro); falha vira warn e exit 0. Só uso inválido do wrapper sai != 0.
call_hook() {
  event="$1"
  shift
  # shellcheck disable=SC2086
  if "$BRAIN_BIN" hook --event "$event" "$@"; then
    return 0
  fi
  warn "evento $event: binário indisponível ou backend fora — seguindo sem bloquear a sessão"
  return 0
}

# tool-result com throttle: agrega a rajada num único --question por janela.
tool_result() {
  mode="${1:-}"
  bufdir=$(spool_dir)
  mkdir -p "$bufdir" 2>/dev/null || true
  buf="$bufdir/tool-burst.jsonl"
  stamp="$bufdir/tool-burst.stamp"
  if [ "$mode" = "--stdin" ]; then
    input=$(cat /dev/stdin 2>/dev/null || true)
    printf '%s\n' "$input" >> "$buf" 2>/dev/null || true
  else
    printf '{"tool":"unknown"}\n' >> "$buf" 2>/dev/null || true
  fi
  now=$(date +%s)
  last=0
  if [ -f "$stamp" ]; then last=$(cat "$stamp" 2>/dev/null || echo 0); fi
  case "$last" in '' | *[!0-9]*) last=0 ;; esac
  # Dentro da janela E com flush anterior: agrega em silêncio, sem chamar.
  if [ -f "$stamp" ] && [ $((now - last)) -lt "$THROTTLE" ]; then
    return 0
  fi
  n=$(wc -l < "$buf" 2>/dev/null | tr -d ' ' || echo 1)
  case "$n" in '' | *[!0-9]*) n=1 ;; esac
  sample=$(trunc "$(head -c 3000 "$buf" 2>/dev/null || true)")
  question="tool-result x$n agregado(s) (throttle ${THROTTLE}s): ${sample}"
  payload="{\"id\":\"$(new_id tool-result)\"}"
  printf '%s' "$now" > "$stamp" 2>/dev/null || true
  if "$BRAIN_BIN" hook --event tool-result --payload "$payload" --question "$question"; then
    : > "$buf" 2>/dev/null || true
    return 0
  fi
  warn "evento tool-result: binário indisponível ou backend fora — seguindo sem bloquear a sessão"
  # Backend morto: limita o buffer (últimas 50 linhas) para não crescer sem teto.
  tail -n 50 "$buf" > "$buf.tmp" 2>/dev/null && mv "$buf.tmp" "$buf" 2>/dev/null || true
  return 0
}

EVENT="${1:-}"
MODE="${2:-}"

case "$EVENT" in
  session-start)
    q=$(read_question "$MODE")
    call_hook session-start --payload "{\"id\":\"$(new_id session-start)\"}" --question "$q"
    ;;
  prompt-submit)
    # Alias: o binário não tem prompt-submit; a pergunta vira --question do start.
    q=$(read_question "$MODE")
    call_hook session-start --payload "{\"id\":\"$(new_id session-start)\"}" --question "$q"
    ;;
  tool-result)
    tool_result "$MODE"
    ;;
  session-end)
    call_hook session-end --payload "{\"id\":\"$(new_id session-end)\"}"
    ;;
  *)
    echo "Uso: brain-hook.sh <evento> [--stdin]" >&2
    echo "Eventos: session-start, tool-result, session-end, prompt-submit" >&2
    exit 2
    ;;
esac
