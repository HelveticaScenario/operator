import React from 'react';
import Form, { type IChangeEvent } from '@rjsf/core';
import type { RJSFSchema, UiSchema } from '@rjsf/utils';
import validator from '@rjsf/validator-ajv8';
import type { AppConfig, PrettierConfig } from '../../shared/ipcTypes';
import { configSchema } from '../configSchema';

const schema = configSchema.properties.prettier as unknown as RJSFSchema;

const uiSchema: UiSchema = {
    'ui:submitButtonOptions': { norender: true },
    printWidth: {
        'ui:options': { inputType: 'number' },
    },
    tabWidth: {
        'ui:options': { inputType: 'number' },
    },
};

interface FormatterSettingsTabProps {
    config: AppConfig;
    onConfigChange: (partial: Partial<AppConfig>) => void;
}

export function FormatterSettingsTab({
    config,
    onConfigChange,
}: FormatterSettingsTabProps) {
    const formData = config.prettier ?? {};

    const handleChange = (e: IChangeEvent) => {
        const data: PrettierConfig = e.formData ?? {};
        onConfigChange({ prettier: data });
    };

    return (
        <div className="settings-tab-content settings-rjsf">
            <p className="settings-description">
                Configure Prettier formatting options for the patch editor.
                These are merged with built-in defaults.
            </p>
            <Form
                schema={schema}
                uiSchema={uiSchema}
                validator={validator}
                formData={formData}
                onChange={handleChange}
                liveValidate={false}
                showErrorList={false}
            />
        </div>
    );
}
