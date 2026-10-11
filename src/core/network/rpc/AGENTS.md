# AGENTS.md - RPC Module

For when to use RPCs vs replication vs NetworkAction, see `../AGENTS.md`.

## Design Decisions

- **JSON serialization** - Chosen for simplicity and debuggability, same as replication

## Notes for AI Agents

1. **Match RPC IDs** - Server and client must produce identical `RpcTypeId` values (same name passed to `from_name`)
2. **Register before receiving** - Handlers must be registered before RPCs arrive; unknown IDs return `RpcNotFound`
3. **Don't use RPCs for state** - Continuous state should use component replication
4. **Sender is self-reported** - `dispatch()` passes `SerializedRpc.sender` from the payload, not the connection's client ID. Don't trust it for authority checks
5. **Sender 0 is ambiguous** - The server sends with sender 0 (`is_from_server()` checks `== 0`), which is also `LOCAL_HOST_ID` for a ListenServer host
