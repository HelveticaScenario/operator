import { describe, expect, test } from 'vitest';
import { getEnumVariants, schemaToTypeExpr } from '../schemaTypeResolver';

describe('getEnumVariants', () => {
    test('extracts variants from oneOf with const + description (documented enum)', () => {
        const schema = {
            oneOf: [
                { const: 'sum', description: 'Sum all inputs at each channel' },
                {
                    const: 'average',
                    description: 'Average all inputs at each channel',
                },
                {
                    const: 'max',
                    description:
                        'Take the maximum absolute value at each channel',
                },
                {
                    const: 'min',
                    description:
                        'Take the minimum absolute value at each channel',
                },
            ],
        };
        const result = getEnumVariants(schema, {});
        expect(result).toEqual([
            {
                description: 'Sum all inputs at each channel',
                rawValue: 'sum',
                value: '"sum"',
            },
            {
                description: 'Average all inputs at each channel',
                rawValue: 'average',
                value: '"average"',
            },
            {
                description: 'Take the maximum absolute value at each channel',
                rawValue: 'max',
                value: '"max"',
            },
            {
                description: 'Take the minimum absolute value at each channel',
                rawValue: 'min',
                value: '"min"',
            },
        ]);
    });

    test('extracts variants from bare enum array (undocumented enum)', () => {
        const schema = {
            enum: ['white', 'pink', 'brown'],
        };
        const result = getEnumVariants(schema, {});
        expect(result).toEqual([
            { description: undefined, rawValue: 'white', value: '"white"' },
            { description: undefined, rawValue: 'pink', value: '"pink"' },
            { description: undefined, rawValue: 'brown', value: '"brown"' },
        ]);
    });

    test('returns null for non-enum schemas', () => {
        expect(getEnumVariants({ type: 'number' }, {})).toBeNull();
        expect(getEnumVariants({ type: 'string' }, {})).toBeNull();
        expect(
            getEnumVariants(
                { properties: { x: { type: 'number' } }, type: 'object' },
                {},
            ),
        ).toBeNull();
    });

    test('returns null for null/undefined/boolean schemas', () => {
        expect(getEnumVariants(null as any, {})).toBeNull();
        expect(getEnumVariants(undefined as any, {})).toBeNull();
        expect(getEnumVariants(true as any, {})).toBeNull();
        expect(getEnumVariants(false as any, {})).toBeNull();
    });

    test('follows $ref to resolve enum in $defs', () => {
        const rootSchema = {
            $defs: {
                MixMode: {
                    oneOf: [
                        {
                            const: 'sum',
                            description: 'Sum all inputs',
                        },
                        {
                            const: 'average',
                            description: 'Average all inputs',
                        },
                    ],
                },
            },
        };
        const schema = { $ref: '#/$defs/MixMode' };
        const result = getEnumVariants(schema, rootSchema);
        expect(result).toEqual([
            {
                description: 'Sum all inputs',
                rawValue: 'sum',
                value: '"sum"',
            },
            {
                description: 'Average all inputs',
                rawValue: 'average',
                value: '"average"',
            },
        ]);
    });

    test('returns null for Signal $ref sentinels', () => {
        const rootSchema = {
            $defs: {
                Signal: { title: 'Signal' },
            },
        };
        expect(
            getEnumVariants({ $ref: '#/$defs/Signal' }, rootSchema),
        ).toBeNull();
        expect(
            getEnumVariants({ $ref: '#/$defs/PolySignal' }, rootSchema),
        ).toBeNull();
        expect(
            getEnumVariants({ $ref: '#/$defs/MonoSignal' }, rootSchema),
        ).toBeNull();
    });

    test('returns null for Buffer $ref sentinel', () => {
        const rootSchema = {
            $defs: {
                Buffer: { title: 'Buffer' },
            },
        };
        expect(
            getEnumVariants({ $ref: '#/$defs/Buffer' }, rootSchema),
        ).toBeNull();
    });

    test('returns null for oneOf/anyOf that is not all-const (union types)', () => {
        const schema = {
            oneOf: [{ type: 'string' }, { type: 'number' }],
        };
        expect(getEnumVariants(schema, {})).toBeNull();
    });

    test('handles anyOf the same as oneOf', () => {
        const schema = {
            anyOf: [
                { const: 'a', description: 'First' },
                { const: 'b', description: 'Second' },
            ],
        };
        const result = getEnumVariants(schema, {});
        expect(result).toEqual([
            { description: 'First', rawValue: 'a', value: '"a"' },
            { description: 'Second', rawValue: 'b', value: '"b"' },
        ]);
    });

    test('handles variants where some have descriptions and some do not', () => {
        const schema = {
            oneOf: [
                { const: 'x', description: 'Has description' },
                { const: 'y' },
            ],
        };
        const result = getEnumVariants(schema, {});
        expect(result).toEqual([
            { description: 'Has description', rawValue: 'x', value: '"x"' },
            { description: undefined, rawValue: 'y', value: '"y"' },
        ]);
    });

    test('schemaToTypeExpr resolves Buffer refs to BufferOutputRef', () => {
        const rootSchema = {
            $defs: {
                Buffer: { title: 'Buffer' },
            },
        };
        expect(schemaToTypeExpr({ $ref: '#/$defs/Buffer' }, rootSchema)).toBe(
            'BufferOutputRef',
        );
    });

    test('schemaToTypeExpr resolves Table refs to Table', () => {
        const rootSchema = {
            $defs: {
                Table: { title: 'Table' },
            },
        };
        expect(schemaToTypeExpr({ $ref: '#/$defs/Table' }, rootSchema)).toBe(
            'Table',
        );
    });
});
