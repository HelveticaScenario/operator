import type { GraphBuilder } from './builder';
import type { ModuleOutput } from './moduleOutput';
import type { PolySignal, SendGroup } from './types';
import { Collection, $c } from './collection';

export class Bus {
    private builder: GraphBuilder;
    private cb: (mixed: Collection) => unknown;
    private sendGroups: SendGroup[] = [];
    private locked: boolean = false;

    constructor(builder: GraphBuilder, cb: (mixed: Collection) => unknown) {
        this.builder = builder;
        this.cb = cb;

        builder.addBus(this);
    }

    addSend(value: ModuleOutput | ModuleOutput[], gain?: PolySignal): void {
        if (this.locked) {
            throw new Error('`.send` is not allowed in $bus callbacks');
        }
        const outputs = Array.isArray(value) ? [...value] : [value];
        const group: SendGroup = {
            gain,
            outputs,
        };
        this.sendGroups.push(group);
    }

    lock() {
        this.locked = true;
    }

    finalize() {
        const mixFactory = this.builder.getFactory('$mix');
        const mixed = mixFactory(
            this.sendGroups.map((e) => {
                const coll = $c(e.outputs);
                if (e.gain !== undefined) {
                    return coll.gain(e.gain);
                }
                return coll;
            }),
        ) as Collection;
        this.cb(mixed);
    }
}
