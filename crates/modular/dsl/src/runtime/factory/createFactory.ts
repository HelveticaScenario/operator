import type { ModuleSchema } from '@modular/core';
import { deriveChannelCount } from '@modular/core';
import type { Collection, CollectionWithRange, GraphBuilder, ModuleOutput } from '../graph';
import { captureSourceLocation } from '../captureSourceLocation';
import {
    ARGUMENT_SPANS_KEY,
    captureArgumentSpans,
} from './spanRegistry';
import { sanitizeOutputName } from './identifiers';
import type { FactoryFunction, ModuleReturn, MultiOutput } from './namespaceTree';

/**
 * Create a module factory function that returns outputs directly.
 *
 * Body of the original `DSLContext.createFactory()`, extracted so it can be
 * shared by codegen-generated factories in later PRs.
 */
export function createFactory(
    builder: GraphBuilder,
    schema: ModuleSchema,
): FactoryFunction {
    const outputs = schema.outputs || [];

    return (...args: any[]): ModuleReturn => {
        // Capture source location from stack trace
        const sourceLocation = captureSourceLocation();

        // Capture argument spans from the pre-analyzed registry
        const argumentSpans = captureArgumentSpans(sourceLocation);

        const positionalArgs = schema.positionalArgs || [];
        const params: Record<string, any> = {};
        let config: any = {};
        let id: string | undefined;

        // Extract positional args
        for (let i = 0; i < positionalArgs.length; i++) {
            if (i < args.length) {
                params[positionalArgs[i].name] = args[i];
            }
        }

        // Remaining arg (if any) is config.
        if (args.length > positionalArgs.length) {
            config = args[positionalArgs.length];
        }

        if (config) {
            const { id: configId, ...restConfig } = config;
            id = configId;
            for (const key of Object.keys(restConfig)) {
                params[key] = restConfig[key];
            }
        }

        // Attach argument spans for Rust-side highlighting
        if (argumentSpans && Object.keys(argumentSpans).length > 0) {
            params[ARGUMENT_SPANS_KEY] = argumentSpans;
        }

        const node = builder.addModule(schema.name, id, sourceLocation);

        for (const [key, value] of Object.entries(params)) {
            if (value !== undefined) {
                node._setParam(key, value);
            }
        }

        // Derive channel count from params (handles custom logic and PolySignal inference)
        const deriveResult = deriveChannelCount(
            schema.name,
            node.getParamsSnapshot(),
        );

        if (deriveResult.errors && deriveResult.errors.length > 0) {
            const messages = deriveResult.errors
                .map((e) => e.message)
                .join('; ');
            const loc = sourceLocation
                ? ` at line ${sourceLocation.line}`
                : '';
            throw new Error(`${schema.name}${loc}: ${messages}`);
        }

        if (
            deriveResult.channelCount !== null &&
            deriveResult.channelCount !== undefined
        ) {
            node._setDerivedChannelCount(deriveResult.channelCount);
        }

        if (outputs.length === 0) {
            // No outputs - return empty object (shouldn't happen in practice)
            return {} as MultiOutput;
        } else if (outputs.length === 1) {
            const output = outputs[0];
            return node._output(output.name, output.polyphonic ?? false);
        }

        // Multiple outputs - hybrid object: default output with additional output props
        const defaultOutput = outputs.find((o) => o.default) || outputs[0];
        const defaultValue = node._output(
            defaultOutput.name,
            defaultOutput.polyphonic ?? false,
        );

        const additionalOutputs: Record<
            string,
            ModuleOutput | Collection | CollectionWithRange
        > = {};
        for (const output of outputs) {
            if (output.name === defaultOutput.name) continue;
            const safeName = sanitizeOutputName(output.name);
            additionalOutputs[safeName] = node._output(
                output.name,
                output.polyphonic ?? false,
            );
        }

        return Object.assign(defaultValue, additionalOutputs);
    };
}
