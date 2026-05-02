# Distributed auction (MozartSpaces-style) – TupleSpace, process group, lock

Real-time bidding using **TupleSpace** (bids as tuples), **process group** (broadcast to bidders), and **distributed lock** (commit winner). XVSM/Linda-style coordination.

## Abstractions

- **TupleSpace** (`host.TS()`): Bids as tuples `("bid", auction_id, bidder_id, amount_ts, amount)`; auctioneer takes bids, writes `("auction", id, "open"|"sold", ...)`.
- **Process group** (`host.PG()`): Auctioneer joins `auction:{id}:bidders`, broadcasts `auction_start`, `new_bid`, `sold`.
- **Lock** (`host.LockAcquire` / `LockRelease`): Commit phase uses lock `auction:{id}:commit` so only one commit runs.

## Convention

| Instance        | Role       | Usage |
|-----------------|------------|--------|
| `auction:coord-1` | Auctioneer | Send `workflow_run` with `auction_id`, `reserve_price`, `max_bids`. Send `place_bid` to inject bids. |

## Quick start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/go/apps/migrating_mozartspaces
./build.sh
./test.sh 8091
```

## API

- **Run auction**: `POST .../auction:coord-1` with `{"op":"workflow_run","auction_id":"auc-1","reserve_price":10,"max_bids":100}`.
- **Place bid**: `{"op":"place_bid","auction_id":"auc-1","bidder_id":"user-1","amount":50}`.
- **Signal**: `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Metrics

`test.sh` injects 150 bids, runs one auction, then a batch of 5 auctions, and prints:

- Auction ID, status, bids processed, winner and amount
- **Compute ms** and **Coord ms** (and %)
- **Batch wall** (ms)

## Native (MozartSpaces/XVSM) reference

See **`native/mozartspaces_ref.md`** for XVSM/Linda coordination concepts and the PlexSpaces mapping.

## Comparison

| Feature       | MozartSpaces / XVSM     | PlexSpaces (this example)        |
|---------------|-------------------------|----------------------------------|
| Tuple space   | Built-in spaces         | `host.TS()` write/take/read_all  |
| Coordination  | Coordinator objects     | One workflow actor + tuple space |
| Commit        | Transaction / lock      | `host.LockAcquire` / LockRelease |
| Notify bidders| Events / channels       | `host.PG().Broadcast`            |

## References

- [PLAN.md – migrating_mozartspaces](../../../../PLAN.md)
- [Go SDK – host.TS(), host.PG(), Lock](../../../../sdks/go/README.md)
