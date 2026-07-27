#!/usr/bin/env python3
"""eval/test_recall.py — Retrieval accuracy benchmark for brain MCP server.

Measures recall@k for a set of known queries against a seeded knowledge base.
Requires the brain server to be running in SSE mode.

Usage:
    BRAIN_URL=http://localhost:8321 uv run python eval/test_recall.py
"""

from __future__ import annotations

import json
import os
import sys
from dataclasses import dataclass

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

# ---------------------------------------------------------------------------
# Corpus de conhecimento seedado
# ---------------------------------------------------------------------------

SEED_DATA: list[dict] = [
    {
        "layer": "arquitetura",
        "path": "ecommerce/stack-decisao",
        "content": """# Stack Decisão

## Frontend
React 18 com Next.js 14, Tailwind CSS, Shadcn UI.

## Backend
Python FastAPI, PostgreSQL 15, Redis para cache.

## Infra
AWS ECS Fargate, CloudFront CDN, RDS PostgreSQL.
""",
    },
    {
        "layer": "regras",
        "path": "ecommerce/naming-conventions",
        "content": """# Naming Conventions

## Banco de Dados
- Tabelas: snake_case plural (users, orders, products)
- Colunas: snake_case (created_at, user_id)
- PK: id BIGINT AUTO_INCREMENT

## API
- Endpoints: kebab-case (/api/v1/user-orders)
- JSON: camelCase (createdAt, userId)
""",
    },
    {
        "layer": "regras",
        "path": "ecommerce/auth-flow",
        "content": """# Autenticação

## Fluxo
1. Login com email + senha via POST /api/v1/auth/login
2. Servidor retorna JWT com expiração de 1h
3. Cliente envia JWT no header Authorization: Bearer <token>
4. Refresh token via POST /api/v1/auth/refresh (7d expiry)

## Senhas
- Hash: bcrypt com custo 12
- Mínimo 8 caracteres, 1 maiúscula, 1 número
""",
    },
    {
        "layer": "sessoes",
        "path": "ecommerce/sprint-42",
        "content": """# Sprint 42 — Resumo

## Features Entregues
- Carrinho de compras com persistência em Redis
- Checkout com integração Stripe
- Testes de integração para o fluxo de pagamento

## Decisões
- Usamos `stripe` Python SDK v7
- Webhook de confirmação em /api/v1/webhooks/stripe
- Idempotency key para evitar duplicatas
""",
    },
    {
        "layer": "projetos",
        "path": "ecommerce/visao-geral",
        "content": """# Ecommerce Platform

## Descrição
Plataforma de ecommerce B2B com catálogo de produtos,
carrinho, checkout e gestão de pedidos.

## Time
3 backend, 2 frontend, 1 QA

## Stack
Python FastAPI + Next.js + PostgreSQL
""",
    },
]

# ---------------------------------------------------------------------------
# Queries de teste e os paths gold esperados
# ---------------------------------------------------------------------------

@dataclass
class QueryTestCase:
    query: str
    gold_paths: set[str]
    layer: str | None = None
    description: str = ""


TEST_QUERIES: list[QueryTestCase] = [
    QueryTestCase(
        query="qual stack tecnologica usamos",
        gold_paths={"arquitetura/ecommerce/stack-decisao.md"},
        description="Deveria encontrar stack de tecnologia",
    ),
    QueryTestCase(
        query="como nomear tabelas no banco",
        gold_paths={"regras/ecommerce/naming-conventions.md"},
        description="Deveria encontrar convenções de nome",
    ),
    QueryTestCase(
        query="fluxo de login com jwt",
        gold_paths={"regras/ecommerce/auth-flow.md"},
        description="Deveria encontrar fluxo de autenticação",
    ),
    QueryTestCase(
        query="integração com stripe pagamento",
        gold_paths={"sessoes/ecommerce/sprint-42.md"},
        description="Deveria encontrar sessão sobre Stripe",
    ),
    QueryTestCase(
        query="como funciona o carrinho de compras",
        gold_paths={"sessoes/ecommerce/sprint-42.md"},
        description="Deveria encontrar sessão sobre carrinho",
    ),
    QueryTestCase(
        query="descrição do projeto ecommerce",
        gold_paths={"projetos/ecommerce/visao-geral.md"},
        description="Deveria encontrar visão geral do projeto",
    ),
    QueryTestCase(
        query="bcrypt hash de senha",
        gold_paths={"regras/ecommerce/auth-flow.md"},
        description="Deveria encontrar regra de hash de senha",
    ),
]


# ---------------------------------------------------------------------------
# Benchmark runner
# ---------------------------------------------------------------------------

def seed_data() -> None:
    """Popula o vault com os dados de teste."""
    import subprocess

    brain_dir = os.path.join(os.path.dirname(__file__), "..")
    for item in SEED_DATA:
        result = subprocess.run(
            ["uv", "run", "--directory", brain_dir, "brain", "store",
             item["layer"], item["path"], item["content"]],
            capture_output=True, text=True, timeout=30,
            env={**os.environ, "BRAIN_URL": os.environ.get("BRAIN_URL", "http://localhost:8321")},
        )
        if result.returncode != 0:
            print(f"  [WARN] Seed failed for {item['path']}: {result.stderr.strip()}")


def run_benchmark(top_k: int = 5) -> dict:
    """Roda o benchmark e retorna métricas."""
    import subprocess

    brain_dir = os.path.join(os.path.dirname(__file__), "..")
    results = []

    for tc in TEST_QUERIES:
        args = [
            "uv", "run", "--directory", brain_dir,
            "brain", "search", tc.query,
            "--top-k", str(top_k),
        ]
        if tc.layer:
            args.extend(["--layer", tc.layer])

        result = subprocess.run(
            args, capture_output=True, text=True, timeout=30,
            env={**os.environ, "BRAIN_URL": os.environ.get("BRAIN_URL", "http://localhost:8321")},
        )

        found_paths: set[str] = set()
        try:
            data = json.loads(result.stdout)
            for r in data.get("results", []):
                found_paths.add(r["path"])
        except (json.JSONDecodeError, TypeError):
            pass

        hits = len(found_paths & tc.gold_paths)
        total_gold = len(tc.gold_paths)
        results.append({
            "query": tc.query,
            "gold": list(tc.gold_paths),
            "found": list(found_paths),
            "hits": hits,
            "total_gold": total_gold,
            "recall": hits / total_gold if total_gold > 0 else 0.0,
        })

    # Métricas agregadas
    total_hits = sum(r["hits"] for r in results)
    total_gold = sum(r["total_gold"] for r in results)
    recall_at_k = total_hits / total_gold if total_gold > 0 else 0.0

    return {
        "top_k": top_k,
        "total_queries": len(results),
        "total_hits": total_hits,
        "total_gold": total_gold,
        "recall_at_k": round(recall_at_k, 4),
        "queries": results,
    }


def print_report(metrics: dict) -> None:
    """Exibe relatório formatado."""
    print(f"\n{'='*60}")
    print(f"  BRAIN RETRIEVAL BENCHMARK  (recall@{metrics['top_k']})")
    print(f"{'='*60}")
    print(f"  Queries:    {metrics['total_queries']}")
    print(f"  Gold total: {metrics['total_gold']}")
    print(f"  Hits:       {metrics['total_hits']}")
    print(f"  Recall@{metrics['top_k']}: {metrics['recall_at_k']:.1%}")
    print(f"{'='*60}\n")

    for q in metrics["queries"]:
        status = "✅" if q["hits"] >= q["total_gold"] else "⚠️" if q["hits"] > 0 else "❌"
        print(f"  {status} {q['query']}")
        print(f"     Gold: {q['gold']}")
        print(f"     Found: {q['found']}")
        print(f"     Recall: {q['recall']:.0%}")
        print()


def main():
    print("Seeding knowledge base...")
    seed_data()

    print(f"Running benchmark with BRAIN_URL={os.environ.get('BRAIN_URL', 'http://localhost:8321')}...")
    metrics = run_benchmark(top_k=5)
    print_report(metrics)

    # Exit code: 0 se recall >= 70%, 1 caso contrário
    if metrics["recall_at_k"] >= 0.7:
        print("✅ Benchmark PASSED (recall@5 >= 70%)")
        sys.exit(0)
    else:
        print("❌ Benchmark FAILED (recall@5 < 70%)")
        sys.exit(1)


if __name__ == "__main__":
    main()
