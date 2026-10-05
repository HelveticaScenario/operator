import { describe, expect, test } from 'vitest';

import { transformErrorsWithSourceLocations } from '../validationErrorLocations';
import type { ValidationError } from '@modular/core';
import type { SourceLocationInfo } from '../../../shared/ipcTypes';

const err = (location: string): ValidationError => ({
    field: 'params.freq',
    location,
    message: 'invalid value',
});

const loc = (line: number, idIsExplicit = false): SourceLocationInfo => ({
    column: 1,
    idIsExplicit,
    line,
});

describe('transformErrorsWithSourceLocations', () => {
    test('resolves an auto-generated location to the module of that exact type', () => {
        const map = {
            '$sine-1': loc(3),
            '$saw-1': loc(9),
        };
        const [out] = transformErrorsWithSourceLocations(
            [err('$saw(...)')],
            map,
        );
        expect(out.location).toBe('line 9');
    });

    test('a type that is a prefix of another type never matches the longer type', () => {
        const map = {
            '$mix-1': loc(2),
            '$mixDown-1': loc(7),
        };
        const [out] = transformErrorsWithSourceLocations(
            [err('$mixDown(...)')],
            map,
        );
        expect(out.location).toBe('line 7');
    });

    test('several modules of the same type keep the type hint instead of guessing a line', () => {
        const map = {
            '$sine-1': loc(3),
            '$sine-2': loc(9),
        };
        const [out] = transformErrorsWithSourceLocations(
            [err('$sine(...)')],
            map,
        );
        expect(out.location).toBe('$sine(...)');
    });

    test('explicit-ID locations and explicit map entries are left alone', () => {
        const map = {
            myOsc: loc(4, true),
        };
        const input = [err("'myOsc'")];
        expect(transformErrorsWithSourceLocations(input, map)).toEqual(input);
    });
});
