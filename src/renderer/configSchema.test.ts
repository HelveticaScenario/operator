import { describe, expect, it } from 'vitest';
import { AppConfigSchema } from '../main/appConfig';
import { configSchema } from './configSchema';

describe('configSchema', () => {
    it('describes every field the app reads from config.json', () => {
        expect(Object.keys(configSchema.properties).sort()).toEqual(
            Object.keys(AppConfigSchema.shape).sort(),
        );
    });
});
