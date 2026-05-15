/**
 * Tests for template interpolation position mapping in sequence tracking.
 */

import { describe, expect, test } from 'vitest';

/**
 * Represents an interpolation region in a template string.
 */
interface InterpolationRegion {
    sourceStart: number;
    sourceEnd: number;
    sourceLen: number;
    evaluatedStart: number;
    evaluatedLen: number;
}

/**
 * Extract interpolation regions from a template literal pattern.
 * Duplicated from moduleStateTracking.ts for unit testing.
 */
function extractInterpolationRegions(
    sourcePattern: string,
    evaluatedPattern: string,
): InterpolationRegion[] | null {
    const interpolationRegex = /\$\{/g;
    const regions: InterpolationRegion[] = [];
    let match;

    while ((match = interpolationRegex.exec(sourcePattern)) !== null) {
        const startIdx = match.index;
        let depth = 1;
        let endIdx = startIdx + 2;

        while (endIdx < sourcePattern.length && depth > 0) {
            if (sourcePattern[endIdx] === '{') {
                depth++;
            } else if (sourcePattern[endIdx] === '}') {
                depth--;
            }
            endIdx++;
        }

        if (depth === 0) {
            regions.push({
                evaluatedLen: 0,
                evaluatedStart: 0,
                sourceEnd: endIdx,
                sourceLen: endIdx - startIdx,
                sourceStart: startIdx,
            });
        }
    }

    if (regions.length === 0) {
        return null;
    }

    const literalPieces: {
        text: string;
        sourceStart: number;
        sourceEnd: number;
    }[] = [];
    let pos = 0;

    for (const region of regions) {
        if (pos < region.sourceStart) {
            literalPieces.push({
                sourceEnd: region.sourceStart,
                sourceStart: pos,
                text: sourcePattern.slice(pos, region.sourceStart),
            });
        }
        pos = region.sourceEnd;
    }

    if (pos < sourcePattern.length) {
        literalPieces.push({
            sourceEnd: sourcePattern.length,
            sourceStart: pos,
            text: sourcePattern.slice(pos),
        });
    }

    let evalPos = 0;
    let regionIdx = 0;

    for (let i = 0; i < literalPieces.length; i++) {
        const piece = literalPieces[i];
        const pieceIdx = evaluatedPattern.indexOf(piece.text, evalPos);

        if (pieceIdx === -1) {
            return null;
        }

        const interpolationBeforeThisPiece =
            regionIdx < regions.length &&
            (i === 0 ? regions[0].sourceStart < piece.sourceStart : true);

        if (interpolationBeforeThisPiece) {
            regions[regionIdx].evaluatedStart = evalPos;
            regions[regionIdx].evaluatedLen = pieceIdx - evalPos;
            regionIdx++;
        }

        evalPos = pieceIdx + piece.text.length;
    }

    if (regionIdx < regions.length) {
        regions[regionIdx].evaluatedStart = evalPos;
        regions[regionIdx].evaluatedLen = evaluatedPattern.length - evalPos;
    }

    return regions;
}

/**
 * Build a position mapping function from evaluated string positions to source positions.
 */
function buildPositionMapper(
    regions: InterpolationRegion[],
): (evalPos: number) => number | null {
    return (evalPos: number): number | null => {
        let sourceOffset = 0;
        let evalOffset = 0;

        for (const region of regions) {
            const evalRegionStart = region.evaluatedStart;

            if (evalPos < evalRegionStart) {
                return evalPos + (sourceOffset - evalOffset);
            }

            if (evalPos < evalRegionStart + region.evaluatedLen) {
                return null;
            }

            sourceOffset = region.sourceEnd;
            evalOffset = evalRegionStart + region.evaluatedLen;
        }

        return evalPos + (sourceOffset - evalOffset);
    };
}

// Interpolation region extraction tests

describe('extractInterpolationRegions', () => {
    test('handles single interpolation at start', () => {
        const regions = extractInterpolationRegions('${note} c e', 'g c e');
        expect(regions).toEqual([
            {
                evaluatedLen: 1,
                evaluatedStart: 0,
                sourceEnd: 7,
                sourceLen: 7,
                sourceStart: 0,
            },
        ]);
    });

    test('handles single interpolation in middle', () => {
        const regions = extractInterpolationRegions('a ${x} b', 'a XX b');
        expect(regions).toEqual([
            {
                evaluatedLen: 2,
                evaluatedStart: 2,
                sourceEnd: 6,
                sourceLen: 4,
                sourceStart: 2,
            },
        ]);
    });

    test('handles single interpolation at end', () => {
        const regions = extractInterpolationRegions('a b ${x}', 'a b c');
        expect(regions).toEqual([
            {
                evaluatedLen: 1,
                evaluatedStart: 4,
                sourceEnd: 8,
                sourceLen: 4,
                sourceStart: 4,
            },
        ]);
    });

    test('handles multiple interpolations', () => {
        const regions = extractInterpolationRegions(
            'a ${x} b ${y} c',
            'a XX b YYY c',
        );
        expect(regions).toEqual([
            {
                evaluatedLen: 2,
                evaluatedStart: 2,
                sourceEnd: 6,
                sourceLen: 4,
                sourceStart: 2,
            },
            {
                evaluatedLen: 3,
                evaluatedStart: 7,
                sourceEnd: 13,
                sourceLen: 4,
                sourceStart: 9,
            },
        ]);
    });

    test('returns null for patterns without interpolations', () => {
        const regions = extractInterpolationRegions('a b c', 'a b c');
        expect(regions).toBeNull();
    });

    test('handles empty interpolation result', () => {
        const regions = extractInterpolationRegions('${x} b', ' b');
        expect(regions).toEqual([
            {
                evaluatedLen: 0,
                evaluatedStart: 0,
                sourceEnd: 4,
                sourceLen: 4,
                sourceStart: 0,
            },
        ]);
    });
});

// Position mapping tests

describe('buildPositionMapper', () => {
    test('maps positions for single interpolation at start', () => {
        const regions = extractInterpolationRegions('${note} c e', 'g c e')!;
        const map = buildPositionMapper(regions);

        expect(map(0)).toBeNull();
        expect(map(1)).toBe(7);
        expect(map(2)).toBe(8);
        expect(map(4)).toBe(10);
    });

    test('maps positions for interpolation in middle', () => {
        const regions = extractInterpolationRegions('a ${x} b', 'a XX b')!;
        const map = buildPositionMapper(regions);

        expect(map(0)).toBe(0);
        expect(map(1)).toBe(1);
        expect(map(2)).toBeNull();
        expect(map(3)).toBeNull();
        expect(map(4)).toBe(6);
        expect(map(5)).toBe(7);
    });

    test('maps positions for multiple interpolations', () => {
        const regions = extractInterpolationRegions(
            'a ${x} b ${y} c',
            'a XX b YYY c',
        )!;
        const map = buildPositionMapper(regions);

        expect(map(0)).toBe(0);
        expect(map(1)).toBe(1);
        expect(map(2)).toBeNull();
        expect(map(3)).toBeNull();
        expect(map(4)).toBe(6);
        expect(map(5)).toBe(7);
        expect(map(6)).toBe(8);
        expect(map(7)).toBeNull();
        expect(map(8)).toBeNull();
        expect(map(9)).toBeNull();
        expect(map(10)).toBe(13);
        expect(map(11)).toBe(14);
    });
});
