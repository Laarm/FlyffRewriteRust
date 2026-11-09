// Type test (Plan/11 WP-PROTO-01), compiled by `pnpm typecheck`: protobuf-es must keep mapping 64-bit
// fields to `bigint`, so a missed n64()/b64() conversion is a compile error, never silent precision
// loss. Not imported anywhere.
import type { IntentStamp } from '../gen/proto/flyff/v1/common_pb';

export const asBigint: bigint = ({} as IntentStamp).serverFrame;

// @ts-expect-error a 64-bit field is a bigint, assigning it to a number must not compile
export const asNumber: number = ({} as IntentStamp).serverFrame;
