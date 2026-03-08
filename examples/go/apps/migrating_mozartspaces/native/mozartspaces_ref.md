# MozartSpaces / XVSM reference

MozartSpaces and XVSM (eXtended Virtual Shared Memory) extend the Linda model with coordinator objects and structured spaces.

## Concepts

- **Tuple space**: Shared associative memory; producers write tuples, consumers read/take by pattern.
- **Coordinator objects**: Define how tuples are stored and retrieved (FIFO, LIFO, label-based, priority).
- **Coordination**: Multiple processes coordinate via the same space without direct references.

## PlexSpaces mapping

| XVSM / MozartSpaces     | PlexSpaces                          |
|-------------------------|-------------------------------------|
| Tuple space             | `host.TS()` (write, take, read, read_all) |
| Coordinator (FIFO/LIFO) | Application logic in actor + tuple patterns |
| Commit / critical section | `host.LockAcquire` / `LockRelease` |
| Notify participants     | `host.PG().Join` / `Broadcast`      |

This example implements a **distributed auction**: bids are tuples, the auctioneer takes bids from the tuple space, uses a lock to commit the winner, and broadcasts results via a process group.
