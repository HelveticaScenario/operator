// Augment Array.prototype with pipe() for TypeScript
declare global {
    interface Array<T> {
        pipe<U>(this: this, pipelineFunc: (self: this) => U): U;
    }
}

/**
 * Install pipe() on Array.prototype so arrays in the DSL can use it.
 * Non-enumerable to avoid polluting for-in loops.
 */
export function installArrayPipe(): void {
    if (typeof Array.prototype.pipe !== 'function') {
        Object.defineProperty(Array.prototype, 'pipe', {
            configurable: true,
            enumerable: false,
            value: function pipe<T>(
                this: unknown,
                pipelineFunc: (self: typeof this) => T,
            ): T {
                return pipelineFunc(this);
            },
            writable: true,
        });
    }
}
