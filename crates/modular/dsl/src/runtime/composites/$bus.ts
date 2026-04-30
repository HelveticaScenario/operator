import type { Collection, GraphBuilder } from '../graph';
import { Bus } from '../graph';

/** `$bus(cb)` — create a send/return bus driven by `cb(mixed)`. */
export function create$bus(builder: GraphBuilder) {
    return (cb: (mixed: Collection) => unknown): Bus => new Bus(builder, cb);
}
