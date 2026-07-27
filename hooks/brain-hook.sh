#!/usr/bin/env bash
# brain-hook.sh — Hook genérico para agentes de IA.
# Uso: brain-hook.sh <evento> [--stdin]
#
# Eventos: session-start | tool-result | session-end | prompt-submit
#
# Instalação (Claude Code):
#   hooks.SessionStart = "/caminho/para/brain-hook.sh session-start --stdin"
#
# Dependências: curl, jq (opcional para pretty-print)

set -euo pipefail

BRAIN_URL="${BRAIN_URL:-http://localhost:8321}"
EVENT="${1:-}"
MODE="${2:-}"

# Detecta projeto do git
PROJECT=""
if git rev-parse --show-toplevel 2>/dev/null; then
    PROJECT=$(basename "$(git rev-parse --show-toplevel)" 2>/dev/null || echo "unknown")
fi
PROJECT="${PROJECT:-unknown}"

_call_brain() {
    local tool="$1"
    shift
    # Usa o CLI brain se disponível
    if command -v brain &>/dev/null; then
        uv run brain "$tool" "$@" 2>/dev/null || true
    else
        # Fallback: curl para SSE (assume servidor rodando)
        # Nota: MCP não é REST, então isso é limitado
        # Melhor usar o CLI brain via uv run
        :
    fi
}

case "$EVENT" in
    session-start)
        # Carrega contexto relevante do brain antes de começar
        if [ -n "$PROJECT" ]; then
            _call_brain search "projeto $PROJECT" --layer projetos --top-k 3
            _call_brain search "regras $PROJECT" --layer regras --top-k 5
            _call_brain search "arquitetura $PROJECT" --layer arquitetura --top-k 3
        fi
        ;;

    tool-result)
        # Se recebeu stdin com JSON, extrai info relevante
        if [ "$MODE" = "--stdin" ]; then
            INPUT=$(cat /dev/stdin 2>/dev/null || echo "")
            if [ -n "$INPUT" ]; then
                # Extrai tool name e resultado do JSON (se jq disponível)
                if command -v jq &>/dev/null; then
                    TOOL_NAME=$(echo "$INPUT" | jq -r '.tool // .name // "unknown"' 2>/dev/null)
                    RESULT=$(echo "$INPUT" | jq -r '.result // .output // .error // ""' 2>/dev/null | head -c 500)
                    if [ -n "$RESULT" ] && [ "$RESULT" != "null" ]; then
                        _call_brain store sessoes "$PROJECT/tool-$TOOL_NAME" "## $TOOL_NAME\n\n$RESULT"
                    fi
                fi
            fi
        fi
        ;;

    session-end)
        # Salva timestamp do fim da sessão
        _call_brain store sessoes "$PROJECT/session-end-$(date +%s)" "## Session End\n\nProjeto: $PROJECT\nData: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        ;;

    prompt-submit)
        if [ "$MODE" = "--stdin" ]; then
            INPUT=$(cat /dev/stdin 2>/dev/null || echo "")
            if [ -n "$INPUT" ]; then
                PROMPT=$(echo "$INPUT" | head -c 1000)
                _call_brain store sessoes "$PROJECT/prompt-$(date +%s)" "## Prompt\n\n$PROMPT"
            fi
        fi
        ;;

    *)
        echo "Uso: brain-hook.sh <evento> [--stdin]"
        echo "Eventos: session-start, tool-result, session-end, prompt-submit"
        exit 1
        ;;
esac
