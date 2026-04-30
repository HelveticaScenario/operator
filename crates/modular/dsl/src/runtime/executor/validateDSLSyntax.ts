/** Validate DSL script syntax without executing. */
export function validateDSLSyntax(source: string): {
    valid: boolean;
    error?: string;
} {
    try {
        // Create function only for syntax validation - not executed
        const _fn = new Function(source);
        return { valid: true };
    } catch (error) {
        if (error instanceof Error) {
            return { error: error.message, valid: false };
        }
        return { error: 'Unknown syntax error', valid: false };
    }
}
