export type ElementsOf<T extends unknown[][]> = {
    [K in keyof T]: T[K] extends (infer E)[] ? E : never;
};

/**
 * Compute the Cartesian product of the given arrays.
 *
 * Returns every possible combination of one element from each array.
 * Pairs well with the array overload of `.pipe()` to fan a signal across
 * multiple parameter dimensions.
 *
 * @example $cartesian([1, 2], ['a', 'b'])
 * // → [[1,'a'], [1,'b'], [2,'a'], [2,'b']]
 */
export function $cartesian<A extends unknown[][]>(
    ...arrays: A
): ElementsOf<A>[] {
    return arrays.reduce<unknown[][]>(
        (acc, arr) => acc.flatMap((combo) => arr.map((val) => [...combo, val])),
        [[]],
    ) as ElementsOf<A>[];
}
