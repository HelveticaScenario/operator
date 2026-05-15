/**
 * This file will automatically be loaded by vite and run in the "renderer" context.
 * To learn more about the differences between the "main" and the "renderer" context in
 * Electron, visit:
 *
 * https://electronjs.org/docs/tutorial/process-model
 *
 * By default, Node.js integration in this file is disabled. When enabling Node.js integration
 * in a renderer process, please be aware of potential security implications. You can read
 * more about security risks here:
 *
 * https://electronjs.org/docs/tutorial/security
 *
 * To enable Node.js integration in this file, open up `main.ts` and enable the `nodeIntegration`
 * flag:
 *
 * ```
 *  // Create the browser window.
 *  mainWindow = new BrowserWindow({
 *    width: 800,
 *    height: 600,
 *    webPreferences: {
 *      nodeIntegration: true
 *    }
 *  });
 * ```
 */

import { createRoot } from 'react-dom/client';
import './index.css';
import { Component, StrictMode } from 'react';
import type { ErrorInfo, ReactNode } from 'react';
import App from './App';

class ErrorBoundary extends Component<
    { children: ReactNode },
    { error: Error | null }
> {
    state = { error: null as Error | null };

    static getDerivedStateFromError(error: Error) {
        return { error };
    }

    componentDidCatch(error: Error, info: ErrorInfo) {
        console.error(
            '[ErrorBoundary] Render error:',
            error,
            info.componentStack,
        );
    }

    render() {
        const { error } = this.state;
        if (error) {
            return (
                <div
                    style={{
                        color: '#ff6b6b',
                        fontFamily: 'monospace',
                        padding: 24,
                        whiteSpace: 'pre-wrap',
                    }}
                >
                    <strong>Renderer crashed</strong>
                    {'\n\n'}
                    {error.message}
                    {'\n\n'}
                    {error.stack}
                    {'\n\n'}
                    <button onClick={() => this.setState({ error: null })}>
                        Retry
                    </button>
                </div>
            );
        }
        return this.props.children;
    }
}
import { HelpWindow } from './components/HelpWindow';
import { ThemeProvider } from './themes/ThemeContext';

// Configure Monaco Editor workers for Vite (replaces MonacoWebpackPlugin)
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';

(self as any).MonacoEnvironment = {
    getWorker(_: string, label: string) {
        if (label === 'typescript' || label === 'javascript') {
            return new tsWorker();
        }
        if (label === 'json') {
            return new jsonWorker();
        }
        if (label === 'css' || label === 'scss' || label === 'less') {
            return new cssWorker();
        }
        return new editorWorker();
    },
};

import * as monaco from 'monaco-editor';

// Suppress Monaco Editor's harmless "Canceled" errors that occur during
// Normal operations like dismissing the "Go to Symbol" dialog with Escape.
// These must be registered early before React/webpack error overlays catch them.
window.addEventListener(
    'error',
    (event) => {
        if (event.error?.message === 'Canceled') {
            event.preventDefault();
            event.stopImmediatePropagation();
            return false;
        }
    },
    true,
); // Use capture phase to intercept before other handlers

window.addEventListener(
    'unhandledrejection',
    (event) => {
        if (event.reason?.message === 'Canceled') {
            event.preventDefault();
            event.stopImmediatePropagation();
            return false;
        }
    },
    true,
);

// Make monaco available globally
(window as any).monaco = monaco;

// Set up main process log forwarding to renderer console
// This allows viewing main process logs in the renderer's DevTools
window.electronAPI.onMainLog((entry) => {
    const prefix = '[main]';
    const args = entry.args.map((arg) => {
        // Reconstruct Error objects
        if (arg && typeof arg === 'object' && '__error' in arg) {
            const errorLike = arg as unknown as {
                name: string;
                message: string;
                stack?: string;
            };
            const err = new Error(errorLike.message);
            err.name = errorLike.name;
            if (errorLike.stack) {
                err.stack = errorLike.stack;
            }
            return err;
        }
        return arg;
    });

    switch (entry.level) {
        case 'log':
            console.log(prefix, ...args);
            break;
        case 'info':
            console.info(prefix, ...args);
            break;
        case 'warn':
            console.warn(prefix, ...args);
            break;
        case 'error':
            console.error(prefix, ...args);
            break;
        case 'debug':
            console.debug(prefix, ...args);
            break;
    }
});

const root = document.getElementById('root');
if (!root) {
    throw new Error('Failed to find the root element');
}

const isHelpWindow = window.location.hash === '#help';

createRoot(root).render(
    <StrictMode>
        <ErrorBoundary>
            <ThemeProvider>
                {isHelpWindow ? <HelpWindow /> : <App />}
            </ThemeProvider>
        </ErrorBoundary>
    </StrictMode>,
);
