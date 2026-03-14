# Plan

## Objective
Keep zero-trust node auth stable and production-ready.

## Implemented
- PAT bootstrap only via `/api/nodes/claim`.
- Runtime challenge/verify/JWT flow in place.
- WebSocket node auth uses JWT.
- Anti-enumeration behavior added for `challenge` and `verify`.
- Structured auth/claim error codes added.
- Rate limiting decision: enforced at load balancer layer.
- DB cleanup for expired challenges handled via Postgres `pg_cron`.

## Remaining Actions
1. Stabilize server tests (current top priority).
   - Fix DB test setup so `cargo test -p phirepass-server` is green.
   - Current failure pattern: multiple SQL statements executed in one prepared query.
2. Add request correlation for auth paths.
   - Introduce request IDs in logs/responses for easier production debugging.
3. Add concise E2E runbook.
   - Document a minimal claim -> challenge -> verify -> websocket validation flow.
4. Ops follow-up.
   - Validate LB rate-limit behavior in production monitoring/logs.

## Quick Resume
1. Ensure database schema is current in your environment.
2. Verify server starts with required env (`JWT_SECRET`, DB/Redis URLs).
3. Run `phirepass-agent login` with PAT scope `server:register`.
4. Run `cargo test -p phirepass-server` and fix remaining failures first.
