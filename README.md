# resolver

High-performance intent solver in Rust. By [kcolbchain](https://kcolbchain.com) (est. 2015).

## The Problem

Intent-based DeFi (ERC-7683) is the future of trading UX — users express *what* they want, solvers compete to fill it optimally. But:

- All current solvers are **TypeScript or Python** — too slow for competitive auction environments
- Solvers need to **simulate across multiple DEXes and chains** in milliseconds
- No **open-source Rust solver framework** exists — incumbents keep theirs proprietary

A Rust solver is 10-100x faster than JS. Speed wins auctions. Auctions = revenue.

## What resolver Does

```
┌─────────────────────────────────────────────┐
│              Intent Sources                  │
│  (UniswapX, Across, CoW Protocol, custom)   │
├──────────┬──────────┬───────────────────────┤
│ Decoder  │ Simulator│  Profitability Engine  │
│ (parse   │ (route   │  (gas costs, fees,     │
│  orders) │  finding)│   net profit calc)     │
├──────────┴──────────┴───────────────────────┤
│           Execution Engine                   │
│  (build tx, sign, submit, track)            │
├─────────────────────────────────────────────┤
│           Monitor / Dashboard               │
│  (P&L, fill rate, gas spent, win rate)      │
└─────────────────────────────────────────────┘
```

## Features

- **Multi-protocol** — UniswapX, Across, CoW Protocol from one solver
- **Fast simulation** — route across Uniswap V3, Curve, Balancer in <1ms
- **Profitability engine** — accounts for gas, priority fees, bridge costs, slippage
- **CLI interface** — `resolver scan`, `resolver fill`, `resolver monitor`
- **Self-sustaining** — solver earns fees from every filled intent

## Quick Start

```bash
git clone https://github.com/kcolbchain/resolver.git
cd resolver

# Scan for fillable intents (read-only)
cargo run -- scan --chain base --protocol uniswapx

# Run solver in simulation mode
cargo run -- solve --simulate --chain arbitrum

# Monitor performance
cargo run -- monitor
```

## Supported Protocols

| Protocol | Status | Chain Support |
|----------|--------|--------------|
| UniswapX | ✅ MVP | Ethereum, Arbitrum, Base |
| Across | 📋 Planned | Multi-chain |
| CoW Protocol | 📋 Planned | Ethereum |

## Revenue Model

Solvers earn fees from filling intents:
- **UniswapX**: solver captures spread between order price and execution price
- **Across**: relayer fees for cross-chain fills
- **CoW Protocol**: surplus from batch auction settlements

No clients needed. No grants needed. Profitable from day one if fill rate is competitive.

## Architecture

| Module | Description |
|--------|-------------|
| `intents` | Order parsing, validation, expiry tracking |
| `solver` | Core solving logic — routing, simulation, profit calculation |
| `execution` | Transaction building, signing, submission |
| `monitor` | P&L tracking, fill rate, gas analytics |

## License

MIT — see [LICENSE](LICENSE)
