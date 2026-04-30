import type { GraphBuilder } from './builder';
import { ModuleOutput } from './moduleOutput';

/**
 * DeferredModuleOutput is a placeholder for a signal that will be assigned later.
 * Useful for feedback loops and forward references in the DSL.
 * Supports the same chainable methods as ModuleOutput; transforms are stored
 * and applied when the deferred signal is resolved.
 */
export class DeferredModuleOutput extends ModuleOutput {
    private resolvedModuleOutput: ModuleOutput | null = null;
    private resolving: boolean = false;
    static idCounter = 0;

    constructor(builder: GraphBuilder) {
        super(
            builder,
            `DEFERRED-${DeferredModuleOutput.idCounter++}`,
            'output',
        );
        // Register this deferred output with the builder for string replacement during toPatch
        builder.registerDeferred(this);
    }

    /**
     * Set the actual signal this deferred output should resolve to.
     */
    set(signal: ModuleOutput): void {
        this.resolvedModuleOutput = signal;
    }

    /**
     * Resolve this deferred output to an actual ModuleOutput.
     * @returns The resolved ModuleOutput, or null if not set.
     */
    resolve(): ModuleOutput | null {
        if (this.resolving) {
            throw new Error(
                'Circular reference detected while resolving DeferredModuleOutput',
            );
        }

        if (this.resolvedModuleOutput === null) {
            return null;
        }

        let output = this.resolvedModuleOutput;
        if (output instanceof DeferredModuleOutput) {
            this.resolving = true;
            const resolved = output.resolve();
            this.resolving = false;

            if (resolved === null) {
                return null;
            }
            output = resolved;
        }

        return output;
    }
}
