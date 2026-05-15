import './UpdateNotification.css';

export type UpdateNotificationState =
    | { status: 'idle' }
    | {
          status: 'available';
          version: string;
          releaseUrl: string;
          supportsInAppUpdate: boolean;
      }
    | { status: 'downloading'; version: string }
    | { status: 'ready' }
    | { status: 'error'; message: string };

interface Props {
    state: UpdateNotificationState;
    onDownload: () => void;
    onInstall: () => void;
    onSkip: () => void;
    onDismiss: () => void;
}

export function UpdateNotification({
    state,
    onDownload,
    onInstall,
    onSkip,
    onDismiss,
}: Props) {
    if (state.status === 'idle') {
        return null;
    }

    let message: string;
    let primaryAction: { label: string; onClick: () => void } | null = null;
    let secondaryActions: { label: string; onClick: () => void }[] = [];

    switch (state.status) {
        case 'available':
            message = `Version ${state.version} is available.`;
            primaryAction = {
                label: state.supportsInAppUpdate
                    ? 'Download & Install'
                    : 'View Release',
                onClick: onDownload,
            };
            secondaryActions = [
                { label: 'Skip This Version', onClick: onSkip },
                { label: 'Dismiss', onClick: onDismiss },
            ];
            break;
        case 'downloading':
            message = `Downloading ${state.version}…`;
            break;
        case 'ready':
            message = 'Update ready. Restart to install.';
            primaryAction = { label: 'Restart Now', onClick: onInstall };
            secondaryActions = [{ label: 'Later', onClick: onDismiss }];
            break;
        case 'error':
            message = `Update error: ${state.message}`;
            secondaryActions = [{ label: 'Dismiss', onClick: onDismiss }];
            break;
    }

    return (
        <div className="update-notification" role="status" aria-live="polite">
            <span className="update-notification__message">{message}</span>
            <div className="update-notification__actions">
                {primaryAction && (
                    <button
                        className="update-notification__btn update-notification__btn--primary"
                        onClick={primaryAction.onClick}
                    >
                        {primaryAction.label}
                    </button>
                )}
                {secondaryActions.map((action) => (
                    <button
                        key={action.label}
                        className="update-notification__btn update-notification__btn--secondary"
                        onClick={action.onClick}
                    >
                        {action.label}
                    </button>
                ))}
            </div>
        </div>
    );
}
