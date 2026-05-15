import type { ValidationError } from '@modular/core';

interface ErrorDisplayProps {
    error: string | null;
    errors?: ValidationError[] | null;
    onDismiss: () => void;
}

export function ErrorDisplay({ error, errors, onDismiss }: ErrorDisplayProps) {
    if (!error && (!errors || errors.length === 0)) {
        return null;
    }

    return (
        <div className="error-display">
            <div className="error-content">
                <span className="error-icon">⚠️</span>
                <div className="error-messages">
                    {error && <pre className="error-message">{error}</pre>}
                    {errors && errors.length > 0 && (
                        <ul className="validation-errors">
                            {errors.map((err, i) => (
                                <li key={i} className="validation-error">
                                    <div className="validation-error-main">
                                        <strong>{err.message}</strong>
                                        {err.location && (
                                            <span className="error-location">
                                                {' '}
                                                at {err.location}
                                            </span>
                                        )}
                                    </div>
                                    {err.expectedType && (
                                        <div className="validation-error-expected">
                                            Expected: {err.expectedType}
                                        </div>
                                    )}
                                </li>
                            ))}
                        </ul>
                    )}
                </div>
                <button className="error-dismiss" onClick={onDismiss}>
                    ×
                </button>
            </div>
        </div>
    );
}
